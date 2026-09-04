//! Opaque Resources carry bounded bytes, never filesystem authority.
use super::{
    project::Registry,
    workers::{WorkerError, Workers},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use rmcp::{
    model::{
        CacheScope, ErrorData, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
        ResourceContents,
    },
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{ArtifactAccessError, AuthorizedArtifact, RegistryClock};
use rust_engineering_artifact::{ArtifactLimits, MemoryArtifactStore};
use rust_engineering_domain::{ArtifactError, ArtifactId, ProjectRef};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const PREFIX: &str = "rust-artifact://";
const MAX_CONTENT: usize = 256 * 1024;
const MAX_RESPONSE: usize = 512 * 1024;
#[derive(Clone)]
pub(super) struct ArtifactClock(Instant);
impl RegistryClock for ArtifactClock {
    fn seconds(&self) -> u64 {
        self.0.elapsed().as_secs()
    }
}
pub(super) type Store = MemoryArtifactStore<ArtifactClock>;
pub(super) fn uri(owner: &ProjectRef, id: &ArtifactId) -> String {
    format!("{PREFIX}{owner}/{id}")
}
fn not_found() -> ErrorData {
    ErrorData::resource_not_found("Artifact resource not found", None)
}
fn internal() -> ErrorData {
    ErrorData::internal_error("Artifact resource read failed", None)
}
fn worker_error(error: WorkerError) -> ErrorData {
    match error {
        WorkerError::Internal => internal(),
        WorkerError::Busy => ErrorData::new(
            rmcp::model::ErrorCode(-32000),
            "Artifact worker is busy; retry after the active operation",
            None,
        ),
        WorkerError::Cancelled | WorkerError::TimedOut => ErrorData::new(
            rmcp::model::ErrorCode(-32000),
            "Artifact resource operation interrupted",
            None,
        ),
    }
}
fn parse(value: &str) -> Result<(ProjectRef, ArtifactId), ErrorData> {
    // Both opaque IDs are exactly 36 ASCII bytes. No URI normalization, escaping,
    // query, fragment, trailing slash or path interpretation is permitted.
    if value.len() != PREFIX.len() + 36 + 1 + 36 {
        return Err(not_found());
    }
    let (owner, id) = value
        .strip_prefix(PREFIX)
        .and_then(|v| v.split_once('/'))
        .ok_or_else(not_found)?;
    Ok((
        owner.parse().map_err(|_| not_found())?,
        id.parse().map_err(|_| not_found())?,
    ))
}
fn encode(artifact: AuthorizedArtifact) -> Result<ReadResourceResult, ErrorData> {
    if artifact.content.len() > MAX_CONTENT
        || artifact.metadata.size_bytes as usize != artifact.content.len()
    {
        return Err(internal());
    }
    let mut hash = String::with_capacity(64);
    for byte in artifact.metadata.sha256 {
        use std::fmt::Write;
        write!(&mut hash, "{byte:02x}").map_err(|_| internal())?;
    }
    let mut metadata = serde_json::Map::new();
    metadata.insert(
        "retention_remaining_seconds".into(),
        artifact.retention_remaining_seconds.into(),
    );
    metadata.insert("sha256".into(), hash.into());
    metadata.insert("size_bytes".into(), artifact.metadata.size_bytes.into());
    metadata.insert("truncated".into(), artifact.metadata.truncated.into());
    let content = ResourceContents::blob(
        STANDARD.encode(&artifact.content),
        uri(&artifact.metadata.owner, &artifact.metadata.id),
    )
    .with_mime_type("application/octet-stream")
    .with_meta(rmcp::model::MetaObject(metadata));
    let response = ReadResourceResult::new(vec![content])
        .with_cache_scope(CacheScope::Private)
        .with_ttl_ms(0);
    // Bound the complete typed response before the SDK adds its small envelope.
    if serde_json::to_vec(&response).map_err(|_| internal())?.len() > MAX_RESPONSE {
        return Err(internal());
    }
    Ok(response)
}
pub(super) struct Resources {
    registry: Arc<Mutex<Registry>>,
    store: Arc<Mutex<Store>>,
    clock: ArtifactClock,
    workers: Workers,
    ready: Arc<AtomicBool>,
}
impl Resources {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, ArtifactError> {
        let clock = ArtifactClock(Instant::now());
        // Explicit empty literal-redaction configuration; no host environment or
        // automatic secret-discovery claim is involved.
        let store = MemoryArtifactStore::new(clock.clone(), ArtifactLimits::default(), Vec::new())?;
        Ok(Self {
            registry,
            workers,
            ready,
            clock,
            store: Arc::new(Mutex::new(store)),
        })
    }
    pub(super) fn store(&self) -> Arc<Mutex<Store>> {
        Arc::clone(&self.store)
    }
    pub(super) fn clock(&self) -> ArtifactClock {
        self.clock.clone()
    }
    pub(super) async fn read(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        self.read_uri(&request.uri, context.ct).await
    }
    async fn read_uri(
        &self,
        value: &str,
        cancel: CancellationToken,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let (owner, id) = parse(value)?;
        if !self.ready.load(Ordering::Acquire) {
            return Err(not_found());
        }
        let registry = Arc::clone(&self.registry);
        let store = Arc::clone(&self.store);
        let clock = self.clock.clone();
        let joined = self
            .workers
            .run_joined(
                cancel,
                Instant::now() + Duration::from_secs(10),
                move |control| {
                    // Same lock order as publication; no await while either guard exists.
                    let mut registry =
                        registry.lock().map_err(|_| ArtifactAccessError::Internal)?;
                    let mut store = store.lock().map_err(|_| ArtifactAccessError::Internal)?;
                    registry.read_artifact(&owner, &id, &mut *store, &clock, control)
                },
            )
            .await
            .map_err(worker_error)?;
        let artifact = match joined.result {
            Err(ArtifactAccessError::NotFound) => return Err(not_found()),
            Err(ArtifactAccessError::Internal) => return Err(internal()),
            Err(ArtifactAccessError::Cancelled) => {
                return Err(worker_error(
                    joined.interrupted.unwrap_or(WorkerError::Cancelled),
                ));
            }
            Ok(_) if joined.interrupted.is_some() => {
                return Err(worker_error(
                    joined.interrupted.unwrap_or(WorkerError::Cancelled),
                ));
            }
            Ok(artifact) => artifact,
        };
        encode(artifact).map(Into::into)
    }
}
#[cfg(test)]
mod tests;
