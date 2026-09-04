//! Explicit catalog administration. This module is never called by MCP tools.
mod floor;
use floor::Floor;
use rust_engineering_catalog::bundle::{self, BundleError, PublisherTrust, VerifiedBundle};
use rust_engineering_domain::{Clock, FreshnessPolicy, SnapshotEvidence, UnixSeconds};
use rust_engineering_project::catalog_store::{
    CatalogStore, StoreError, read_catalog_file, read_trust_file,
};
use serde::Serialize;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Invocation {
    action: Action,
    store: PathBuf,
    trust: PathBuf,
    json: bool,
    model_dir: Option<PathBuf>,
    index_store: Option<PathBuf>,
}
enum Action {
    Status,
    Import(PathBuf),
    Sync(PathBuf),
    SyncRemote(crate::catalog_sync::SyncSource),
    RebuildIndex,
}
pub fn parse(args: impl Iterator<Item = OsString>) -> Option<Invocation> {
    let mut args = args;
    let action = args.next()?;
    let mut action = match action.to_str()? {
        "status" => Action::Status,
        "import" => Action::Import(PathBuf::from(args.next()?)),
        "sync" => Action::Sync(PathBuf::new()),
        "rebuild-index" => Action::RebuildIndex,
        _ => return None,
    };
    let (mut store, mut trust, mut json, mut source) = (None, None, false, None);
    let (mut model_dir, mut index_store) = (None, None);
    let (mut url, mut allowed_host) = (None, None);
    while let Some(flag) = args.next() {
        match flag.to_str()? {
            "--json" if !json => json = true,
            "--store" if store.is_none() => store = Some(PathBuf::from(args.next()?)),
            "--trust" if trust.is_none() => trust = Some(PathBuf::from(args.next()?)),
            "--source" if source.is_none() => source = Some(PathBuf::from(args.next()?)),
            "--model-dir" if model_dir.is_none() => model_dir = Some(PathBuf::from(args.next()?)),
            "--index-store" if index_store.is_none() => {
                index_store = Some(PathBuf::from(args.next()?))
            }
            "--url" if url.is_none() => url = Some(args.next()?.into_string().ok()?),
            "--allow-host" if allowed_host.is_none() => {
                allowed_host = Some(args.next()?.into_string().ok()?)
            }
            _ => return None,
        }
    }
    if matches!(action, Action::Sync(_)) {
        action = match (source, url, allowed_host) {
            (Some(path), None, None) => Action::Sync(path),
            (None, Some(url), Some(host)) => {
                Action::SyncRemote(crate::catalog_sync::SyncSource::new(&url, &host).ok()?)
            }
            _ => return None,
        };
    } else if source.is_some() || url.is_some() || allowed_host.is_some() {
        return None;
    }
    let store = store?;
    let trust = trust?;
    if !store.is_absolute()
        || !trust.is_absolute()
        || matches!(&action,Action::Import(p)|Action::Sync(p) if !p.is_absolute())
    {
        return None;
    }
    if model_dir.as_ref().is_some_and(|p| !p.is_absolute())
        || index_store.as_ref().is_some_and(|p| !p.is_absolute())
    {
        return None;
    }
    match action {
        Action::RebuildIndex if model_dir.is_none() || index_store.is_none() => return None,
        Action::Status if model_dir.is_none() && index_store.is_some() => return None,
        Action::Import(_) | Action::Sync(_) | Action::SyncRemote(_) if index_store.is_some() => {
            return None;
        }
        _ => {}
    }
    Some(Invocation {
        action,
        store,
        trust,
        json,
        model_dir,
        index_store,
    })
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Report {
    format_version: u32,
    status: &'static str,
    operation: &'static str,
    error_code: Option<&'static str>,
    message: &'static str,
    network_used: bool,
    catalog: Option<CatalogStatus>,
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogStatus {
    pub semantics: &'static str,
    pub publisher: String,
    pub channel: String,
    pub publisher_key_sha256: String,
    pub sequence: u64,
    pub floor_sequence: u64,
    pub floor_bundle_sha256: String,
    pub reservation_pending: bool,
    pub bundle_sha256: String,
    pub catalog_sha256: String,
    pub schema_version: u32,
    pub evidence: SnapshotEvidence,
    pub rustsec_available: bool,
    pub semantic_index_available: bool,
}
struct WallClock;
impl Clock for WallClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        )
    }
}
fn status(
    bundle: &VerifiedBundle,
    trust: &PublisherTrust,
    floor: &Floor,
) -> Result<CatalogStatus, BundleError> {
    use rust_engineering_application::CatalogRepository;
    let policy = FreshnessPolicy::new(
        "catalog-snapshot-v1"
            .parse()
            .map_err(|_| BundleError::Integrity)?,
        86400,
        604800,
    )
    .map_err(|_| BundleError::Integrity)?;
    Ok(CatalogStatus {
        semantics: "latest_known",
        publisher: trust.publisher.clone(),
        channel: trust.channel.clone(),
        publisher_key_sha256: trust.key_fingerprint()?,
        sequence: bundle.manifest().sequence,
        floor_sequence: floor.sequence(),
        floor_bundle_sha256: floor.bundle_sha256().to_owned(),
        reservation_pending: bundle.manifest().sequence < floor.sequence(),
        bundle_sha256: bundle.fingerprint().to_owned(),
        catalog_sha256: bundle.repository().metadata().fingerprint.to_string(),
        schema_version: 1,
        evidence: SnapshotEvidence::assess(
            bundle.manifest().catalog_provenance.clone(),
            policy,
            &WallClock,
        ),
        rustsec_available: bundle.rustsec_bytes().is_some(),
        semantic_index_available: false,
    })
}
#[derive(Debug)]
enum Error {
    Store(StoreError),
    Bundle(BundleError),
    Missing,
    State,
    ActiveUnverified,
    TrustMismatch,
    RebuildUnavailable,
    Sync(crate::catalog_sync::SyncError),
}
impl From<StoreError> for Error {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}
impl From<BundleError> for Error {
    fn from(e: BundleError) -> Self {
        Self::Bundle(e)
    }
}
fn execute(
    invocation: &Invocation,
    network: &std::cell::Cell<bool>,
) -> Result<CatalogStatus, Error> {
    let trust = PublisherTrust::parse(&read_trust_file(&invocation.trust, 4096)?)?;
    let mut store = CatalogStore::open(&invocation.store)?;
    let floor = store
        .read_floor()?
        .map(|b| Floor::parse(&b, &trust).map_err(Error::from))
        .transpose()?;
    let active_bytes = store.read_active()?;
    if floor.is_none() && active_bytes.is_some() {
        return Err(Error::State);
    }
    let active = active_bytes.as_ref().map(|b| bundle::verify(b, &trust));
    drop(active_bytes);
    if let (Some(floor), Some(Ok(active))) = (&floor, &active)
        && (active.manifest().sequence > floor.sequence()
            || (active.manifest().sequence == floor.sequence() && !floor.matches(active)))
    {
        return Err(Error::State);
    }
    match &invocation.action {
        Action::Status => {
            let active = active
                .as_ref()
                .ok_or(Error::Missing)?
                .as_ref()
                .map_err(|_| Error::ActiveUnverified)?;
            let mut result = status(active, &trust, floor.as_ref().ok_or(Error::State)?)?;
            if active.semantic_index_bytes().is_some()
                && invocation.model_dir.is_some()
                && invocation.index_store.is_none()
            {
                result.semantic_index_available = crate::catalog_semantic::validate_imported_index(
                    active,
                    invocation.model_dir.as_deref(),
                )
                .is_ok();
            }
            if let (Some(model), Some(index)) = (&invocation.model_dir, &invocation.index_store) {
                result.semantic_index_available =
                    crate::catalog_semantic::validate_persisted_index(
                        active.repository(),
                        model,
                        index,
                    )
                    .is_ok();
            }
            Ok(result)
        }
        Action::Import(_) | Action::Sync(_) | Action::SyncRemote(_) => {
            let active_sequence = active
                .as_ref()
                .and_then(|v| v.as_ref().ok())
                .map(|b| b.manifest().sequence);
            drop(active);
            let bytes = match &invocation.action {
                Action::Import(path) | Action::Sync(path) => {
                    read_catalog_file(path, bundle::MAX_BUNDLE_BYTES)?
                }
                Action::SyncRemote(source) => {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| Error::Sync(crate::catalog_sync::SyncError::Unavailable))?;
                    network.set(true); // Explicit network acquisition attempted; includes failed transfers.
                    runtime.block_on(source.fetch()).map_err(Error::Sync)?
                }
                _ => return Err(Error::Missing),
            };
            let candidate = bundle::verify(&bytes, &trust)?;
            if let Some(sequence) = active_sequence {
                candidate.require_newer_than(sequence)?;
            }
            if floor.as_ref().is_some_and(|f| !f.permits(&candidate)) {
                return Err(Error::Bundle(BundleError::Rollback));
            }
            crate::catalog_semantic::validate_imported_index(
                &candidate,
                invocation.model_dir.as_deref(),
            )
            .map_err(|_| Error::RebuildUnavailable)?;
            let next_floor_bytes = Floor::new(&candidate).bytes()?;
            let next_floor = Floor::parse(&next_floor_bytes, &trust)?;
            let mut result = status(&candidate, &trust, &next_floor)?;
            result.semantic_index_available = candidate.semantic_index_bytes().is_some();
            store.reserve_floor(&next_floor_bytes)?;
            store.commit(&bytes)?;
            // A readback verifies exactly the durable activated bytes before success.
            if store.read_active()?.as_deref() != Some(bytes.as_slice()) {
                return Err(Error::Store(StoreError::DurabilityUncertain));
            }
            Ok(result)
        }
        Action::RebuildIndex => {
            let active = active
                .as_ref()
                .ok_or(Error::Missing)?
                .as_ref()
                .map_err(|_| Error::ActiveUnverified)?;
            let model = invocation
                .model_dir
                .as_ref()
                .ok_or(Error::RebuildUnavailable)?;
            let index_path = invocation
                .index_store
                .as_ref()
                .ok_or(Error::RebuildUnavailable)?;
            let mut index_store = CatalogStore::open(index_path)?;
            let bytes = crate::catalog_semantic::rebuild(active.repository(), model)
                .map_err(|_| Error::RebuildUnavailable)?;
            index_store.commit(&bytes)?;
            if index_store.read_active()?.as_deref() != Some(bytes.as_slice()) {
                return Err(Error::Store(StoreError::DurabilityUncertain));
            }
            let mut result = status(active, &trust, floor.as_ref().ok_or(Error::State)?)?;
            result.semantic_index_available = true;
            Ok(result)
        }
    }
}
fn error(error: Error) -> (&'static str, &'static str) {
    match error {
        Error::State => (
            "CATALOG_STATE_INVALID",
            "Retained sequence state is invalid or missing; restore trusted state from a verified backup without resetting its floor",
        ),
        Error::ActiveUnverified => (
            "CATALOG_ACTIVE_UNVERIFIED",
            "Active data cannot verify under current trust; restore exact reserved bytes signed by this key, or import a strictly newer signed generation after key rotation",
        ),
        Error::TrustMismatch => (
            "CATALOG_TRUST_MISMATCH",
            "This store belongs to another publisher/channel; restore its original trust configuration or choose a separate store",
        ),
        Error::Store(StoreError::Changed) => (
            "CATALOG_STATE_CHANGED",
            "A catalog path or record changed during access; retry with stable owned paths",
        ),
        Error::Sync(crate::catalog_sync::SyncError::Denied) => (
            "NETWORK_DENIED",
            "HTTPS URL must match the explicit allowed host",
        ),
        Error::Sync(crate::catalog_sync::SyncError::Budget) => (
            "OUTPUT_LIMIT_EXCEEDED",
            "HTTPS snapshot response exceeded the acquisition budget",
        ),
        Error::Sync(_) => (
            "CATALOG_SYNC_UNAVAILABLE",
            "HTTPS acquisition failed; check the approved source and TLS connection",
        ),
        Error::Store(StoreError::UnsupportedPlatform) => (
            "UNSUPPORTED_PLATFORM",
            "Catalog storage requires the validated macOS 26+ APFS adapter",
        ),
        Error::Store(StoreError::Busy) => (
            "CATALOG_BUSY",
            "Another catalog operation holds the store lock; retry after it finishes",
        ),
        Error::Store(StoreError::DurabilityUncertain) => (
            "CATALOG_DURABILITY_UNCERTAIN",
            "Reopen catalog status before retrying; activation may have occurred",
        ),
        Error::Store(StoreError::LimitExceeded) | Error::Bundle(BundleError::Budget) => (
            "OUTPUT_LIMIT_EXCEEDED",
            "Catalog acquisition or validation exceeded its budget",
        ),
        Error::Store(StoreError::InvalidPath | StoreError::Denied) => (
            "SANDBOX_DENIED",
            "Use physical no-follow paths and a private owned mode-0700 store",
        ),
        Error::Store(StoreError::Io) => (
            "CATALOG_IO_ERROR",
            "Check input files, store permissions and available disk space",
        ),
        Error::Bundle(BundleError::Rollback) => (
            "CATALOG_ROLLBACK",
            "Import requires a strictly newer signed sequence in the same publisher channel",
        ),
        Error::Bundle(BundleError::InvalidTrust | BundleError::UntrustedPublisher) => (
            "CATALOG_UNTRUSTED_PUBLISHER",
            "Supply the host-approved publisher/channel and Ed25519 public key",
        ),
        Error::Bundle(BundleError::InvalidSignature) => (
            "CATALOG_INVALID_SIGNATURE",
            "Publisher signature verification failed; active state is unchanged",
        ),
        Error::Bundle(BundleError::UnsupportedFormat) => (
            "CATALOG_UNSUPPORTED_SCHEMA",
            "Bundle format or catalog schema is not supported by this binary",
        ),
        Error::Bundle(_) => (
            "CATALOG_INVALID_BUNDLE",
            "Bundle archive, canonical manifest or payload validation failed",
        ),
        Error::Missing => (
            "CATALOG_UNAVAILABLE",
            "Import a signed snapshot into this store first",
        ),
        Error::RebuildUnavailable => (
            "SEMANTIC_REBUILD_UNAVAILABLE",
            "A verified local model and native index persistence are required for rebuild",
        ),
    }
}
pub fn run(invocation: Invocation) -> ExitCode {
    let operation = match invocation.action {
        Action::Status => "status",
        Action::Import(_) => "import",
        Action::Sync(_) | Action::SyncRemote(_) => "sync",
        Action::RebuildIndex => "rebuild-index",
    };
    let network = std::cell::Cell::new(false);
    let (report, code) = match execute(&invocation, &network) {
        Ok(catalog) => (
            Report {
                format_version: 1,
                status: "passed",
                operation,
                error_code: None,
                message: "Authenticated local catalog generation verified",
                network_used: network.get(),
                catalog: Some(catalog),
            },
            0,
        ),
        Err(e) => {
            let (code, message) = error(e);
            (
                Report {
                    format_version: 1,
                    status: "unavailable",
                    operation,
                    error_code: Some(code),
                    message,
                    network_used: network.get(),
                    catalog: None,
                },
                1,
            )
        }
    };
    let result = if invocation.json {
        serde_json::to_vec(&report)
            .map_err(io::Error::other)
            .and_then(|mut bytes| {
                bytes.push(b'\n');
                io::stdout().lock().write_all(&bytes)
            })
    } else {
        let mut out = io::stdout().lock();
        writeln!(out, "{}: {}", report.status, report.message).and_then(|()| {
            if let Some(c) = report.catalog {
                writeln!(out, "Publisher: {}/{}\nSequence: {}\nReserved sequence: {}\nReservation pending: {}\nCatalog: {}\nSemantics: latest_known\nNetwork acquisition attempted: {}", c.publisher, c.channel, c.sequence, c.floor_sequence, c.reservation_pending, c.catalog_sha256, report.network_used)
            } else {
                writeln!(out, "Code: {}", report.error_code.unwrap_or("CATALOG_UNAVAILABLE"))
            }
        })
    };
    if result.is_err() {
        ExitCode::FAILURE
    } else {
        ExitCode::from(code)
    }
}
