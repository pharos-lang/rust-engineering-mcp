//! Opaque Resources carry bounded bytes, never filesystem authority.
use super::{
    project::Registry,
    tasks::TaskArtifactLiveness,
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
use rust_engineering_application::{
    ArtifactAccessError, AuthorizedArtifact, QUALITY_CURSOR_MAX_BYTES, QUALITY_INDEX_PAGE_MEMBERS,
    QUALITY_RESOURCE_CHUNK_BYTES, QualityArtifactChunk, QualityArtifactIndexPage, RegistryClock,
};
use rust_engineering_artifact::{ArtifactLimits, MemoryArtifactStore};
use rust_engineering_domain::{
    ArtifactError, ArtifactId, ProjectRef, QualityArtifactId, QualityJobId,
};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const PREFIX: &str = "rust-artifact://";
const QUALITY_PREFIX: &str = "rust-quality-artifact://";
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
pub(super) fn quality_index_uri(owner: &ProjectRef, job: &QualityJobId) -> String {
    format!("{QUALITY_PREFIX}{owner}/{job}")
}
pub(super) fn quality_chunk_uri(
    owner: &ProjectRef,
    id: &QualityArtifactId,
    offset: u64,
    length: u32,
) -> String {
    format!("{QUALITY_PREFIX}{owner}/{id}?offset={offset}&length={length}")
}
/// Integrators provide the owner-bound, live-registry authorization bridge. URI text is never authority.
pub(super) trait QualityResourceReader: Send + Sync {
    fn read_chunk(
        &self,
        owner: &ProjectRef,
        id: &QualityArtifactId,
        offset: u64,
        length: u32,
    ) -> Result<QualityArtifactChunk, ()>;
    fn read_index(
        &self,
        owner: &ProjectRef,
        job: &QualityJobId,
        cursor: Option<&str>,
    ) -> Result<QualityArtifactIndexPage, ()>;
    fn is_live(&self, owner: &ProjectRef, id: &QualityArtifactId) -> bool;
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
enum QualityUri {
    Index(ProjectRef, QualityJobId, Option<String>),
    Chunk(ProjectRef, QualityArtifactId, u64, u32),
}
fn parse_decimal(value: &str) -> Option<u64> {
    (!value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && !(value.len() > 1 && value.starts_with('0')))
    .then(|| value.parse().ok())
    .flatten()
}
fn parse_quality(value: &str) -> Result<QualityUri, ErrorData> {
    let rest = value.strip_prefix(QUALITY_PREFIX).ok_or_else(not_found)?;
    if rest.contains('#') || rest.matches('?').count() > 1 {
        return Err(not_found());
    }
    let (path, query) = rest
        .split_once('?')
        .map_or((rest, None), |(path, query)| (path, Some(query)));
    let (owner, object) = path.split_once('/').ok_or_else(not_found)?;
    if path.matches('/').count() != 1 {
        return Err(not_found());
    }
    let owner: ProjectRef = owner.parse().map_err(|_| not_found())?;
    if let Ok(job) = object.parse::<QualityJobId>() {
        let cursor = match query {
            None => None,
            Some(query) => {
                let value = query.strip_prefix("cursor=").ok_or_else(not_found)?;
                if value.is_empty()
                    || value.len() > QUALITY_CURSOR_MAX_BYTES
                    || !value
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
                {
                    return Err(not_found());
                }
                Some(value.to_owned())
            }
        };
        return Ok(QualityUri::Index(owner, job, cursor));
    }
    let id: QualityArtifactId = object.parse().map_err(|_| not_found())?;
    let query = query.ok_or_else(not_found)?;
    let (offset, length) = query.split_once("&length=").ok_or_else(not_found)?;
    let offset = offset
        .strip_prefix("offset=")
        .and_then(parse_decimal)
        .ok_or_else(not_found)?;
    let length = parse_decimal(length)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value as usize <= QUALITY_RESOURCE_CHUNK_BYTES)
        .ok_or_else(not_found)?;
    Ok(QualityUri::Chunk(owner, id, offset, length))
}
fn encode_quality_chunk(
    owner: &ProjectRef,
    chunk: QualityArtifactChunk,
) -> Result<ReadResourceResult, ErrorData> {
    if chunk.bytes.len() > QUALITY_RESOURCE_CHUNK_BYTES
        || chunk.bytes.len() as u64 > chunk.descriptor.size_bytes
    {
        return Err(internal());
    }
    let mut meta = serde_json::Map::new();
    meta.insert(
        "artifact_id".into(),
        chunk.descriptor.artifact_id.to_string().into(),
    );
    meta.insert("job_id".into(), chunk.descriptor.job_id.to_string().into());
    meta.insert("offset".into(), chunk.offset.into());
    meta.insert("size_bytes".into(), chunk.descriptor.size_bytes.into());
    let length = u32::try_from(chunk.bytes.len()).map_err(|_| internal())?;
    let content = ResourceContents::blob(
        STANDARD.encode(chunk.bytes),
        quality_chunk_uri(owner, &chunk.descriptor.artifact_id, chunk.offset, length),
    )
    .with_mime_type("application/octet-stream")
    .with_meta(rmcp::model::MetaObject(meta));
    let result = ReadResourceResult::new(vec![content])
        .with_cache_scope(CacheScope::Private)
        .with_ttl_ms(0);
    (serde_json::to_vec(&result).map_err(|_| internal())?.len() <= MAX_RESPONSE)
        .then_some(result)
        .ok_or_else(internal)
}
fn encode_quality_index(
    owner: &ProjectRef,
    job: &QualityJobId,
    page: QualityArtifactIndexPage,
) -> Result<ReadResourceResult, ErrorData> {
    if page.rows.len() > QUALITY_INDEX_PAGE_MEMBERS
        || page
            .next_cursor
            .as_ref()
            .is_some_and(|cursor| cursor.len() > QUALITY_CURSOR_MAX_BYTES)
    {
        return Err(internal());
    }
    let rows: Vec<_> = page.rows.into_iter().map(|row| serde_json::json!({"artifact_id": row.artifact_id, "member_index": row.member_index, "kind": row.kind, "size_bytes": row.size_bytes})).collect();
    let content = ResourceContents::text(serde_json::to_string(&serde_json::json!({"job_id": job, "members": rows, "next_cursor": page.next_cursor.map(|cursor| String::from_utf8_lossy(&cursor).into_owned())})).map_err(|_| internal())?, quality_index_uri(owner, job)).with_mime_type("application/json");
    let result = ReadResourceResult::new(vec![content])
        .with_cache_scope(CacheScope::Private)
        .with_ttl_ms(0);
    (serde_json::to_vec(&result).map_err(|_| internal())?.len() <= MAX_RESPONSE)
        .then_some(result)
        .ok_or_else(internal)
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
    quality_reader: Option<Arc<dyn QualityResourceReader>>,
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
            quality_reader: None,
        })
    }
    /// Called by the M3 integrator after constructing the authorized durable-store bridge.
    #[allow(dead_code)] // wired by the M3 stdio integrator; keeping M1 bootstrap unchanged.
    pub(super) fn with_quality_reader(mut self, reader: Arc<dyn QualityResourceReader>) -> Self {
        self.quality_reader = Some(reader);
        self
    }
    pub(super) fn store(&self) -> Arc<Mutex<Store>> {
        Arc::clone(&self.store)
    }
    pub(super) fn clock(&self) -> ArtifactClock {
        self.clock.clone()
    }
    pub(super) fn task_liveness(&self) -> Arc<dyn TaskArtifactLiveness> {
        Arc::new(ResourceLiveness {
            registry: Arc::clone(&self.registry),
            store: Arc::clone(&self.store),
            clock: self.clock.clone(),
            quality_reader: self.quality_reader.clone(),
        })
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
        if !self.ready.load(Ordering::Acquire) {
            return Err(not_found());
        }
        if value.starts_with(QUALITY_PREFIX) {
            let reader = self.quality_reader.as_ref().ok_or_else(not_found)?;
            return match parse_quality(value)? {
                QualityUri::Index(owner, job, cursor) => reader
                    .read_index(&owner, &job, cursor.as_deref())
                    .map_err(|_| not_found())
                    .and_then(|page| encode_quality_index(&owner, &job, page))
                    .map(Into::into),
                QualityUri::Chunk(owner, id, offset, length) => reader
                    .read_chunk(&owner, &id, offset, length)
                    .map_err(|_| not_found())
                    .and_then(|chunk| encode_quality_chunk(&owner, chunk))
                    .map(Into::into),
            };
        }
        let (owner, id) = parse(value)?;
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

struct LivenessControl;
impl rust_engineering_application::OperationControl for LivenessControl {
    fn check(&self) -> Result<(), rust_engineering_application::ProjectError> {
        Ok(())
    }
}

struct ResourceLiveness {
    registry: Arc<Mutex<Registry>>,
    store: Arc<Mutex<Store>>,
    clock: ArtifactClock,
    quality_reader: Option<Arc<dyn QualityResourceReader>>,
}
impl TaskArtifactLiveness for ResourceLiveness {
    fn ephemeral_live(&self, owner: &ProjectRef, id: &ArtifactId) -> bool {
        let Ok(mut registry) = self.registry.try_lock() else {
            return false;
        };
        let Ok(mut store) = self.store.try_lock() else {
            return false;
        };
        registry
            .read_artifact_without_touch(owner, id, &mut *store, &self.clock, &LivenessControl)
            .is_ok()
    }
    fn durable_live(&self, owner: &ProjectRef, id: &QualityArtifactId) -> bool {
        self.quality_reader
            .as_ref()
            .is_some_and(|reader| reader.is_live(owner, id))
    }
}
#[cfg(test)]
mod tests;
