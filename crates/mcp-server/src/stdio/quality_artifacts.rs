//! ADR-061 durable nextest publication and owner-bound Resource reads.

pub(super) mod performance;
mod security;
pub(super) use performance::DurablePerformancePublisher;
pub(super) use security::DurableSecurityPublisher;

use super::{project::Registry, resources::QualityResourceReader};
use rust_engineering_application::coverage::{
    CoverageArtifactKind, CoverageArtifactReference, CoverageDurablePublisher, CoverageObservation,
};
use rust_engineering_application::nextest::{
    NextestArtifactKind, NextestArtifactReference, NextestCompleteness, NextestDurablePublisher,
    NextestObservation,
};
use rust_engineering_application::semver_check::{
    SemverArtifactReference, SemverDurablePublisher, SemverObservation,
};
use rust_engineering_application::{
    InspectionError, OperationControl, ProjectError, QualityArtifactAccess, QualityArtifactChunk,
    QualityArtifactIndexPage, QualityArtifactInput, QualityArtifactStore, QualityAuthority,
    QualityOwnerFacts, QualityRetentionGrant, quality_member_charge,
};
use rust_engineering_domain::semver_check::SemverFindingCompleteness;
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    ArtifactSource, GuestArtifactName, PayloadFormatVersion, PluginIdentity, ProjectRef,
    QUALITY_DEFAULT_TTL_SECONDS, QUALITY_MAX_ARTIFACT_BYTES, QualityArtifactDescriptor,
    QualityArtifactDraft, QualityArtifactError, QualityArtifactId, QualityArtifactKind,
    QualityJobId, QualityMimeType, SourceBundle, UnixSeconds, UtcInstant,
};
use rust_engineering_project::NativeQualityArtifactStore;
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

const MEMBER_CAP: u64 = QUALITY_MAX_ARTIFACT_BYTES;
const NEXTEST_BINARY_SHA256: [u8; 32] = [
    0x38, 0xdc, 0x9f, 0x7e, 0x6e, 0xf5, 0x8c, 0xeb, 0x01, 0x77, 0x1b, 0xd1, 0xd3, 0x12, 0xbc, 0x89,
    0x2f, 0x36, 0x74, 0x17, 0xbe, 0x58, 0xa3, 0x68, 0x3c, 0x63, 0x5c, 0x16, 0x0f, 0x4d, 0x7c, 0x2f,
];

pub(super) struct QualityRuntime {
    pub(super) publisher: DurableNextestPublisher,
    pub(super) coverage_publisher: DurableCoveragePublisher,
    pub(super) semver_publisher: DurableSemverPublisher,
    pub(super) security_publisher: DurableSecurityPublisher,
    pub(super) performance_publisher: DurablePerformancePublisher,
    /// The same publishing store the publishers hold. `rust.benchmark.compare`
    /// reads already-published datasets through it and never publishes, so it
    /// takes the store itself rather than a publisher handle.
    pub(super) store: Arc<Mutex<NativeQualityArtifactStore>>,
    pub(super) reader: Arc<dyn QualityResourceReader>,
    pub(super) state_root_identity: ((i64, u64), u32),
}

pub(super) fn attach(
    state_root: &std::path::Path,
    registry: Arc<Mutex<Registry>>,
) -> Result<Option<QualityRuntime>, QualityArtifactError> {
    let publisher = match NativeQualityArtifactStore::open(state_root) {
        Ok(store) => store,
        Err(
            QualityArtifactError::UnsupportedPlatform
            | QualityArtifactError::UnsupportedStateRoot
            | QualityArtifactError::Busy,
        ) => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let state_root_identity = publisher.state_root_identity();
    // The Resource reader is lock-free and must remain available while a
    // publisher holds the non-blocking store lock (ADR-061/F9). Recovery stays
    // on the publishing `open` above and on the explicit operator CLI.
    let reader = match NativeQualityArtifactStore::attach(state_root) {
        Ok(store) => store,
        Err(
            QualityArtifactError::UnsupportedPlatform
            | QualityArtifactError::UnsupportedStateRoot
            | QualityArtifactError::Busy,
        ) => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let store = Arc::new(Mutex::new(publisher));
    Ok(Some(QualityRuntime {
        publisher: DurableNextestPublisher {
            store: Arc::clone(&store),
        },
        coverage_publisher: DurableCoveragePublisher {
            store: Arc::clone(&store),
        },
        semver_publisher: DurableSemverPublisher {
            store: Arc::clone(&store),
        },
        security_publisher: DurableSecurityPublisher {
            store: Arc::clone(&store),
        },
        performance_publisher: DurablePerformancePublisher {
            store: Arc::clone(&store),
        },
        store,
        reader: Arc::new(DurableQualityReader {
            store: Mutex::new(reader),
            authority: Mutex::new(LiveAuthority { registry }),
        }),
        state_root_identity,
    }))
}

struct Proceed;
impl OperationControl for Proceed {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}

struct LiveAuthority {
    registry: Arc<Mutex<Registry>>,
}
impl QualityAuthority for LiveAuthority {
    fn revalidate_owner(
        &mut self,
        project: &ProjectRef,
    ) -> Result<QualityOwnerFacts, QualityArtifactError> {
        // Resource reads execute on the current-thread Tokio runtime. Never
        // wait behind a job that owns the registry on its blocking worker:
        // contention is deliberately masked as unavailable at this boundary.
        self.registry
            .try_lock()
            .map_err(|_| QualityArtifactError::Unauthorized)?
            .quality_owner_facts(project, &Proceed)
            .map_err(|_| QualityArtifactError::Unauthorized)
    }
}

struct DurableQualityReader {
    store: Mutex<NativeQualityArtifactStore>,
    authority: Mutex<LiveAuthority>,
}
impl QualityResourceReader for DurableQualityReader {
    fn read_chunk(
        &self,
        owner: &ProjectRef,
        id: &QualityArtifactId,
        offset: u64,
        length: u32,
    ) -> Result<QualityArtifactChunk, ()> {
        let facts = self
            .authority
            .try_lock()
            .map_err(|_| ())?
            .revalidate_owner(owner)
            .map_err(|_| ())?;
        let mut store = self.store.try_lock().map_err(|_| ())?;
        let binding = store.owner_binding(&facts).map_err(|_| ())?;
        let chunk = store
            .read_chunk(binding, id, offset, length)
            .map_err(|_| ())?;
        (chunk.descriptor.owner_binding == binding)
            .then_some(chunk)
            .ok_or(())
    }

    fn read_index(
        &self,
        owner: &ProjectRef,
        job: &QualityJobId,
        cursor: Option<&str>,
    ) -> Result<QualityArtifactIndexPage, ()> {
        let facts = self
            .authority
            .try_lock()
            .map_err(|_| ())?
            .revalidate_owner(owner)
            .map_err(|_| ())?;
        let mut store = self.store.try_lock().map_err(|_| ())?;
        let binding = store.owner_binding(&facts).map_err(|_| ())?;
        let page = store
            .read_index_page(binding, job, cursor.map(str::as_bytes))
            .map_err(|_| ())?;
        (page.rows.iter().all(|row| row.owner_binding == binding))
            .then_some(page)
            .ok_or(())
    }

    fn is_live(&self, owner: &ProjectRef, id: &QualityArtifactId) -> bool {
        self.read_chunk(owner, id, 0, 0).is_ok()
    }
}

#[derive(Clone)]
pub(super) struct DurableNextestPublisher {
    store: Arc<Mutex<NativeQualityArtifactStore>>,
}

#[derive(Clone)]
pub(super) struct DurableCoveragePublisher {
    store: Arc<Mutex<NativeQualityArtifactStore>>,
}

#[derive(Clone)]
pub(super) struct DurableSemverPublisher {
    store: Arc<Mutex<NativeQualityArtifactStore>>,
}

struct CallbackAuthority<'a> {
    revalidate: &'a mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
}
impl QualityAuthority for CallbackAuthority<'_> {
    fn revalidate_owner(
        &mut self,
        _: &ProjectRef,
    ) -> Result<QualityOwnerFacts, QualityArtifactError> {
        (self.revalidate)().map_err(|_| QualityArtifactError::Unauthorized)
    }
}

struct Bytes<'a>(&'a [u8]);
impl QualityArtifactInput for Bytes<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, QualityArtifactError> {
        let take = buffer.len().min(self.0.len());
        buffer[..take].copy_from_slice(&self.0[..take]);
        self.0 = &self.0[take..];
        Ok(take)
    }
}

/// One artifact of a durable job, described before its bytes exist.
///
/// The kind/format/mime/guest-name quadruple is the store's own closed pairing
/// (`QualityArtifactDescriptor::validate`); stating it per member keeps a
/// producer from claiming a combination the descriptor validator rejects.
pub(super) struct JobMember<'a> {
    pub(super) kind: QualityArtifactKind,
    pub(super) mime_type: QualityMimeType,
    pub(super) payload_format_version: PayloadFormatVersion,
    pub(super) guest_name: GuestArtifactName,
    pub(super) sensitivity: ArtifactSensitivity,
    /// Always derived from the observation. A hardcoded `Complete` would make
    /// the descriptor a claim about the payload rather than a record of it.
    pub(super) completeness: ArtifactCompleteness,
    pub(super) bytes: &'a [u8],
}

/// Everything one durable job declares once, for every member it publishes.
pub(super) struct Job<'a> {
    pub(super) project: &'a ProjectRef,
    pub(super) captured_at: UnixSeconds,
    pub(super) source: &'a SourceBundle,
    pub(super) selection: ArtifactSelection,
    pub(super) runtime: ArtifactRuntime,
    /// The host's retention permission for this evidence. It is fixed by the
    /// producer, never widened by a peer, a project or a tool argument.
    pub(super) retention: QualityRetentionGrant,
}

/// Reserves one job, streams its members in declared order and commits their
/// descriptors, revalidating the owner immediately before every member's bytes.
///
/// All or nothing: a member that cannot be published fails the whole
/// publication rather than returning a set that silently omits evidence, and
/// the reservation is released on every path. `member_index` is the member's
/// position in `members`, which is the order the caller declared.
///
/// This is the single site for the reservation, quota and revalidation logic;
/// every durable publisher that commits owner-bound evidence reaches the store
/// through it rather than restating it.
pub(super) fn publish_job(
    store: &Arc<Mutex<NativeQualityArtifactStore>>,
    job: Job<'_>,
    members: &[JobMember<'_>],
    revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
    if members.is_empty() {
        return Ok(Vec::new());
    }
    let reserved_bytes = members
        .iter()
        .try_fold(0_u64, |sum, member| {
            sum.checked_add(member.bytes.len() as u64)
        })
        .ok_or(InspectionError::OutputLimit)?
        .max(1);
    let declared_members = members.iter().try_fold(0_u16, |sum, member| {
        sum.checked_add(quality_member_charge(member.kind, None).map_err(quality_error)?)
            .ok_or(InspectionError::OutputLimit)
    })?;
    let mut entropy = [0_u8; 16];
    getrandom::fill(&mut entropy).map_err(|_| InspectionError::Internal)?;
    let job_id = QualityJobId::from_random_bytes(entropy);
    let created =
        UtcInstant::from_unix_seconds(job.captured_at.0).map_err(|_| InspectionError::Internal)?;
    let expires = created
        .checked_add_seconds(QUALITY_DEFAULT_TTL_SECONDS)
        .map_err(|_| InspectionError::Internal)?;
    let captured_source_sha256 = source_digest(job.source);
    let mut store = store.lock().map_err(|_| InspectionError::Internal)?;
    let mut authority = CallbackAuthority { revalidate };
    let mut access = QualityArtifactAccess {
        store: &mut *store,
        authority: &mut authority,
        retention: job.retention,
    };
    let reservation = access
        .begin(
            job.project,
            job_id,
            reserved_bytes,
            declared_members,
            expires.clone(),
        )
        .map_err(quality_error)?;
    let mut published = Vec::with_capacity(members.len());
    let outcome = (|| -> Result<(), InspectionError> {
        for (index, member) in members.iter().enumerate() {
            let mut id = [0_u8; 16];
            getrandom::fill(&mut id).map_err(|_| InspectionError::Internal)?;
            let descriptor = access
                .publish(
                    job.project,
                    &reservation,
                    QualityArtifactDraft {
                        artifact_id: QualityArtifactId::from_random_bytes(id),
                        member_index: u16::try_from(index)
                            .map_err(|_| InspectionError::OutputLimit)?,
                        kind: member.kind,
                        mime_type: member.mime_type,
                        payload_format_version: member.payload_format_version,
                        completeness: member.completeness,
                        sensitivity: member.sensitivity,
                        created_at_utc: created.clone(),
                        expires_at_utc: expires.clone(),
                        source: ArtifactSource {
                            captured_source_sha256,
                            guest_name: member.guest_name,
                            selection: job.selection,
                        },
                        runtime: job.runtime.clone(),
                    },
                    (member.bytes.len() as u64).max(1),
                    &mut Bytes(member.bytes),
                )
                .map_err(quality_error)?;
            published.push(descriptor);
        }
        Ok(())
    })();
    if access.finish(&reservation).is_err() {
        access.store.reconcile_recover().map_err(quality_error)?;
        return Err(InspectionError::Internal);
    }
    outcome.map(|()| published)
}

/// Domain-separated identity of the toolchain an observation ran under.
fn toolchain_identity(runtime: &rust_engineering_domain::RuntimeIdentity) -> [u8; 32] {
    let mut toolchain = Sha256::new();
    toolchain.update(b"rust-mcp/quality-toolchain/v1\0");
    toolchain.update(runtime.rust_version.as_bytes());
    toolchain.update([0]);
    toolchain.update(runtime.cargo_version.as_bytes());
    toolchain.finalize().into()
}

fn keep_or_omit<K: Copy, T>(
    kind: K,
    result: Result<T, QualityArtifactError>,
    published: &mut Vec<T>,
    omitted: &mut Vec<K>,
) {
    match result {
        Ok(value) => published.push(value),
        Err(_) => omitted.push(kind),
    }
}

impl NextestDurablePublisher for DurableNextestPublisher {
    fn publish(
        &mut self,
        project: &ProjectRef,
        captured_at: UnixSeconds,
        source: &SourceBundle,
        observation: &mut NextestObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<NextestArtifactReference>, InspectionError> {
        let members = [
            (
                NextestArtifactKind::JunitXml,
                observation.artifacts.junit_xml.as_slice(),
                observation.artifacts.junit_truncated,
            ),
            (
                NextestArtifactKind::StdoutLog,
                observation.artifacts.stdout.as_slice(),
                observation.artifacts.stdout_truncated,
            ),
            (
                NextestArtifactKind::StderrLog,
                observation.artifacts.stderr.as_slice(),
                observation.artifacts.stderr_truncated,
            ),
        ];
        let members: Vec<_> = members
            .into_iter()
            .filter(|(_, bytes, _)| !bytes.is_empty())
            .collect();
        if members.is_empty() {
            return Ok(Vec::new());
        }
        let reserved_bytes = members
            .iter()
            .try_fold(0_u64, |sum, (_, bytes, _)| {
                sum.checked_add(bytes.len() as u64)
            })
            .ok_or(InspectionError::OutputLimit)?;
        let declared_members = members.iter().try_fold(0_u16, |sum, (kind, _, _)| {
            let artifact_kind = match kind {
                NextestArtifactKind::JunitXml => QualityArtifactKind::JunitXml,
                NextestArtifactKind::StdoutLog | NextestArtifactKind::StderrLog => {
                    QualityArtifactKind::ToolLog
                }
            };
            sum.checked_add(quality_member_charge(artifact_kind, None).map_err(quality_error)?)
                .ok_or(InspectionError::OutputLimit)
        })?;
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| InspectionError::Internal)?;
        let job_id = QualityJobId::from_random_bytes(entropy);
        let created =
            UtcInstant::from_unix_seconds(captured_at.0).map_err(|_| InspectionError::Internal)?;
        let expires = created
            .checked_add_seconds(QUALITY_DEFAULT_TTL_SECONDS)
            .map_err(|_| InspectionError::Internal)?;
        let source_digest = source_digest(source);
        let runtime = artifact_runtime(observation)?;
        let selection = if observation.options.target().is_some() {
            ArtifactSelection::Target
        } else if observation.options.package().is_some() {
            ArtifactSelection::Package
        } else {
            ArtifactSelection::Workspace
        };
        let mut store = self.store.lock().map_err(|_| InspectionError::Internal)?;
        let mut authority = CallbackAuthority { revalidate };
        let mut access = QualityArtifactAccess {
            store: &mut *store,
            authority: &mut authority,
            // Configuring the private state root is the host's explicit grant
            // for nextest's potentially sensitive JUnit and logs. Peers cannot
            // widen this fixed production policy.
            retention: QualityRetentionGrant::PotentiallySensitive,
        };
        let reservation = access
            .begin(
                project,
                job_id,
                reserved_bytes.max(1),
                declared_members,
                expires.clone(),
            )
            .map_err(quality_error)?;
        let mut published = Vec::with_capacity(members.len());
        let mut omitted = Vec::new();
        for (index, (kind, bytes, truncated)) in members.into_iter().enumerate() {
            let mut id = [0_u8; 16];
            if getrandom::fill(&mut id).is_err() {
                omitted.push(kind);
                continue;
            }
            let (artifact_kind, mime_type, payload_format_version, guest_name, sensitivity) =
                match kind {
                    NextestArtifactKind::JunitXml => (
                        QualityArtifactKind::JunitXml,
                        QualityMimeType::ApplicationJunitXml,
                        PayloadFormatVersion::JunitXmlV1,
                        GuestArtifactName::JunitXml,
                        ArtifactSensitivity::SymbolDerived,
                    ),
                    NextestArtifactKind::StdoutLog | NextestArtifactKind::StderrLog => (
                        QualityArtifactKind::ToolLog,
                        QualityMimeType::TextPlain,
                        PayloadFormatVersion::Utf8LogV1,
                        GuestArtifactName::ToolLog,
                        ArtifactSensitivity::PotentiallySensitive,
                    ),
                };
            let descriptor = access.publish(
                project,
                &reservation,
                QualityArtifactDraft {
                    artifact_id: QualityArtifactId::from_random_bytes(id),
                    member_index: u16::try_from(index).map_err(|_| InspectionError::OutputLimit)?,
                    kind: artifact_kind,
                    mime_type,
                    payload_format_version,
                    completeness: if truncated {
                        ArtifactCompleteness::Truncated
                    } else {
                        ArtifactCompleteness::Complete
                    },
                    sensitivity,
                    created_at_utc: created.clone(),
                    expires_at_utc: expires.clone(),
                    source: ArtifactSource {
                        captured_source_sha256: source_digest,
                        guest_name,
                        selection,
                    },
                    runtime: runtime.clone(),
                },
                MEMBER_CAP.min(u64::try_from(bytes.len()).unwrap_or(u64::MAX).max(1)),
                &mut Bytes(bytes),
            );
            keep_or_omit(
                kind,
                descriptor.map(|value| NextestArtifactReference::Durable(Box::new(value))),
                &mut published,
                &mut omitted,
            );
        }
        if access.finish(&reservation).is_err() {
            access.store.reconcile_recover().map_err(quality_error)?;
        }
        for kind in omitted {
            match kind {
                NextestArtifactKind::JunitXml => observation.artifacts.junit_truncated = true,
                NextestArtifactKind::StdoutLog => observation.artifacts.stdout_truncated = true,
                NextestArtifactKind::StderrLog => observation.artifacts.stderr_truncated = true,
            }
        }
        observation.artifacts.junit_xml.clear();
        observation.artifacts.stdout.clear();
        observation.artifacts.stderr.clear();
        if observation.artifacts.junit_truncated
            || observation.artifacts.stdout_truncated
            || observation.artifacts.stderr_truncated
        {
            observation.validation_complete = false;
            observation.completeness = NextestCompleteness::Partial;
        }
        Ok(published)
    }
}

impl CoverageDurablePublisher for DurableCoveragePublisher {
    fn publish(
        &mut self,
        project: &ProjectRef,
        captured_at: UnixSeconds,
        source: &SourceBundle,
        observation: &mut CoverageObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<CoverageArtifactReference>, InspectionError> {
        let members = [
            (
                CoverageArtifactKind::Json,
                observation.artifacts.json.as_slice(),
                observation.artifacts.json_truncated,
            ),
            (
                CoverageArtifactKind::Lcov,
                observation.artifacts.lcov.as_slice(),
                observation.artifacts.lcov_truncated,
            ),
            (
                CoverageArtifactKind::ArchiveBundle,
                observation.artifacts.html_bundle.as_slice(),
                observation.artifacts.html_truncated,
            ),
            (
                CoverageArtifactKind::StdoutLog,
                observation.artifacts.stdout.as_slice(),
                observation.artifacts.stdout_truncated,
            ),
            (
                CoverageArtifactKind::StderrLog,
                observation.artifacts.stderr.as_slice(),
                observation.artifacts.stderr_truncated,
            ),
        ];
        let members: Vec<_> = members
            .into_iter()
            .filter(|(_, bytes, _)| !bytes.is_empty())
            .collect();
        if members.is_empty() {
            return Ok(Vec::new());
        }
        let reserved_bytes = members.iter().try_fold(0_u64, |sum, (_, bytes, _)| {
            sum.checked_add(bytes.len() as u64)
                .ok_or(InspectionError::OutputLimit)
        })?;
        let declared_members = members.iter().try_fold(0_u16, |sum, (kind, bytes, _)| {
            let quality_kind = match kind {
                CoverageArtifactKind::Json => QualityArtifactKind::CoverageJson,
                CoverageArtifactKind::Lcov => QualityArtifactKind::Lcov,
                CoverageArtifactKind::ArchiveBundle => QualityArtifactKind::ArchiveBundle,
                CoverageArtifactKind::StdoutLog | CoverageArtifactKind::StderrLog => {
                    QualityArtifactKind::ToolLog
                }
            };
            let archive_entries = if matches!(kind, CoverageArtifactKind::ArchiveBundle) {
                Some(coverage_archive_entries(bytes)?)
            } else {
                None
            };
            sum.checked_add(
                quality_member_charge(quality_kind, archive_entries).map_err(quality_error)?,
            )
            .ok_or(InspectionError::OutputLimit)
        })?;
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| InspectionError::Internal)?;
        let job_id = QualityJobId::from_random_bytes(entropy);
        let created =
            UtcInstant::from_unix_seconds(captured_at.0).map_err(|_| InspectionError::Internal)?;
        let expires = created
            .checked_add_seconds(QUALITY_DEFAULT_TTL_SECONDS)
            .map_err(|_| InspectionError::Internal)?;
        let selection = if observation.options.target().is_some() {
            ArtifactSelection::Target
        } else if observation.options.package().is_some() {
            ArtifactSelection::Package
        } else {
            ArtifactSelection::Workspace
        };
        let source_digest = source_digest(source);
        let runtime = coverage_artifact_runtime(observation)?;
        let mut store = self.store.lock().map_err(|_| InspectionError::Internal)?;
        let mut authority = CallbackAuthority { revalidate };
        let mut access = QualityArtifactAccess {
            store: &mut *store,
            authority: &mut authority,
            retention: QualityRetentionGrant::PotentiallySensitive,
        };
        let reservation = access
            .begin(
                project,
                job_id,
                reserved_bytes.max(1),
                declared_members,
                expires.clone(),
            )
            .map_err(quality_error)?;
        let mut published = Vec::with_capacity(members.len());
        let mut omitted = Vec::new();
        for (index, (kind, bytes, truncated)) in members.into_iter().enumerate() {
            let mut id = [0_u8; 16];
            if getrandom::fill(&mut id).is_err() {
                omitted.push(kind);
                continue;
            }
            let (artifact_kind, mime_type, format, guest_name, sensitivity) = match kind {
                CoverageArtifactKind::Json => (
                    QualityArtifactKind::CoverageJson,
                    QualityMimeType::ApplicationJson,
                    PayloadFormatVersion::CoverageJsonV1,
                    GuestArtifactName::CoverageJson,
                    ArtifactSensitivity::SourceDerived,
                ),
                CoverageArtifactKind::Lcov => (
                    QualityArtifactKind::Lcov,
                    QualityMimeType::TextPlain,
                    PayloadFormatVersion::LcovV1,
                    GuestArtifactName::Lcov,
                    ArtifactSensitivity::SourceDerived,
                ),
                CoverageArtifactKind::ArchiveBundle => (
                    QualityArtifactKind::ArchiveBundle,
                    QualityMimeType::ApplicationXTar,
                    PayloadFormatVersion::UstarV1,
                    GuestArtifactName::ReportArchive,
                    ArtifactSensitivity::PotentiallySensitive,
                ),
                CoverageArtifactKind::StdoutLog | CoverageArtifactKind::StderrLog => (
                    QualityArtifactKind::ToolLog,
                    QualityMimeType::TextPlain,
                    PayloadFormatVersion::Utf8LogV1,
                    GuestArtifactName::ToolLog,
                    ArtifactSensitivity::PotentiallySensitive,
                ),
            };
            let descriptor = access.publish(
                project,
                &reservation,
                QualityArtifactDraft {
                    artifact_id: QualityArtifactId::from_random_bytes(id),
                    member_index: u16::try_from(index).map_err(|_| InspectionError::OutputLimit)?,
                    kind: artifact_kind,
                    mime_type,
                    payload_format_version: format,
                    completeness: if truncated {
                        ArtifactCompleteness::Truncated
                    } else {
                        ArtifactCompleteness::Complete
                    },
                    sensitivity,
                    created_at_utc: created.clone(),
                    expires_at_utc: expires.clone(),
                    source: ArtifactSource {
                        captured_source_sha256: source_digest,
                        guest_name,
                        selection,
                    },
                    runtime: runtime.clone(),
                },
                MEMBER_CAP.min(u64::try_from(bytes.len()).unwrap_or(u64::MAX).max(1)),
                &mut Bytes(bytes),
            );
            keep_or_omit(
                kind,
                descriptor.map(|value| CoverageArtifactReference::Durable(Box::new(value))),
                &mut published,
                &mut omitted,
            );
        }
        if access.finish(&reservation).is_err() {
            access.store.reconcile_recover().map_err(quality_error)?;
        }
        for kind in omitted {
            match kind {
                CoverageArtifactKind::Json => observation.artifacts.json_truncated = true,
                CoverageArtifactKind::Lcov => observation.artifacts.lcov_truncated = true,
                CoverageArtifactKind::ArchiveBundle => observation.artifacts.html_truncated = true,
                CoverageArtifactKind::StdoutLog => observation.artifacts.stdout_truncated = true,
                CoverageArtifactKind::StderrLog => observation.artifacts.stderr_truncated = true,
            }
        }
        observation.artifacts.json.clear();
        observation.artifacts.lcov.clear();
        observation.artifacts.html_bundle.clear();
        observation.artifacts.stdout.clear();
        observation.artifacts.stderr.clear();
        if observation.artifacts.json_truncated
            || observation.artifacts.lcov_truncated
            || observation.artifacts.html_truncated
            || observation.artifacts.stdout_truncated
            || observation.artifacts.stderr_truncated
        {
            observation.parse_complete = false;
        }
        Ok(published)
    }
}

impl SemverDurablePublisher for DurableSemverPublisher {
    fn publish(
        &mut self,
        project: &ProjectRef,
        captured_at: UnixSeconds,
        baseline: &SourceBundle,
        candidate: &SourceBundle,
        observation: &mut SemverObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Option<SemverArtifactReference>, InspectionError> {
        let mut bytes = observation.raw_output_bytes();
        let producer_truncated = observation.stdout_truncated || observation.stderr_truncated;
        let store_truncated = bytes.len() > MEMBER_CAP as usize;
        bytes.truncate(MEMBER_CAP as usize);
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| InspectionError::Internal)?;
        let job_id = QualityJobId::from_random_bytes(entropy);
        let created =
            UtcInstant::from_unix_seconds(captured_at.0).map_err(|_| InspectionError::Internal)?;
        let expires = created
            .checked_add_seconds(QUALITY_DEFAULT_TTL_SECONDS)
            .map_err(|_| InspectionError::Internal)?;
        let selection = if observation.options.selection().target().is_some() {
            ArtifactSelection::Target
        } else if observation.options.selection().package().is_some() {
            ArtifactSelection::Package
        } else {
            ArtifactSelection::Workspace
        };
        let mut store = self.store.lock().map_err(|_| InspectionError::Internal)?;
        let mut authority = CallbackAuthority { revalidate };
        let mut access = QualityArtifactAccess {
            store: &mut *store,
            authority: &mut authority,
            retention: QualityRetentionGrant::PotentiallySensitive,
        };
        let reservation = access
            .begin(
                project,
                job_id,
                (bytes.len() as u64).max(1),
                1,
                expires.clone(),
            )
            .map_err(quality_error)?;
        let outcome = (|| -> Result<SemverArtifactReference, InspectionError> {
            let mut id = [0_u8; 16];
            getrandom::fill(&mut id).map_err(|_| InspectionError::Internal)?;
            let descriptor = access
                .publish(
                    project,
                    &reservation,
                    QualityArtifactDraft {
                        artifact_id: QualityArtifactId::from_random_bytes(id),
                        member_index: 0,
                        kind: QualityArtifactKind::ToolLog,
                        mime_type: QualityMimeType::TextPlain,
                        payload_format_version: PayloadFormatVersion::Utf8LogV1,
                        completeness: if producer_truncated || store_truncated {
                            ArtifactCompleteness::Truncated
                        } else {
                            ArtifactCompleteness::Complete
                        },
                        sensitivity: ArtifactSensitivity::PotentiallySensitive,
                        created_at_utc: created,
                        expires_at_utc: expires,
                        source: ArtifactSource {
                            captured_source_sha256: semver_source_digest(baseline, candidate),
                            guest_name: GuestArtifactName::ToolLog,
                            selection,
                        },
                        runtime: semver_artifact_runtime(observation)?,
                    },
                    (bytes.len() as u64).max(1),
                    &mut Bytes(&bytes),
                )
                .map_err(quality_error)?;
            Ok(SemverArtifactReference::Durable(Box::new(descriptor)))
        })();
        if access.finish(&reservation).is_err() {
            access.store.reconcile_recover().map_err(quality_error)?;
        }
        let published = outcome.ok();
        if published.is_none() {
            observation.stdout_truncated = true;
            observation.stderr_truncated = true;
            observation.completeness = SemverFindingCompleteness::Incomplete;
        }
        observation.stdout.clear();
        observation.stderr.clear();
        Ok(published)
    }
}

fn quality_error(error: QualityArtifactError) -> InspectionError {
    match error {
        QualityArtifactError::QuotaExceeded | QualityArtifactError::InvalidLimit => {
            InspectionError::OutputLimit
        }
        QualityArtifactError::Unauthorized
        | QualityArtifactError::Expired
        | QualityArtifactError::NotFound => InspectionError::Project(ProjectError::Rejected(
            rust_engineering_domain::OperationalErrorCode::ProjectNotFound,
        )),
        _ => InspectionError::Internal,
    }
}

fn source_digest(source: &SourceBundle) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"rust-mcp/quality-captured-source/v1\0");
    for directory in source.directories() {
        hash.update((directory.len() as u64).to_le_bytes());
        hash.update(directory.as_bytes());
    }
    for file in source.files() {
        hash.update((file.path().len() as u64).to_le_bytes());
        hash.update(file.path().as_bytes());
        hash.update((file.bytes().len() as u64).to_le_bytes());
        hash.update(file.bytes());
    }
    hash.finalize().into()
}

fn semver_source_digest(baseline: &SourceBundle, candidate: &SourceBundle) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"rust-mcp/quality-semver-source-pair/v1\0");
    hash.update(source_digest(baseline));
    hash.update(source_digest(candidate));
    hash.finalize().into()
}

fn artifact_runtime(observation: &NextestObservation) -> Result<ArtifactRuntime, InspectionError> {
    Ok(ArtifactRuntime {
        image_digest: digest_text(&observation.runtime.image_id)?,
        toolchain_identity: toolchain_identity(&observation.runtime),
        plugin: ArtifactPlugin {
            identity: PluginIdentity::Nextest,
            version: 1,
            digest: NEXTEST_BINARY_SHA256,
        },
        implementation_digest: digest_text(&observation.execution_fingerprint.to_string())?,
    })
}

fn coverage_artifact_runtime(
    observation: &CoverageObservation,
) -> Result<ArtifactRuntime, InspectionError> {
    let mut plugin = Sha256::new();
    plugin.update(b"cargo-llvm-cov\0");
    plugin.update(observation.identity.cargo_llvm_cov_version.as_bytes());
    Ok(ArtifactRuntime {
        image_digest: digest_text(&observation.runtime.image_id)?,
        toolchain_identity: toolchain_identity(&observation.runtime),
        plugin: ArtifactPlugin {
            identity: PluginIdentity::Coverage,
            version: 1,
            digest: plugin.finalize().into(),
        },
        implementation_digest: digest_text(&observation.execution_fingerprint.to_string())?,
    })
}

fn semver_artifact_runtime(
    observation: &SemverObservation,
) -> Result<ArtifactRuntime, InspectionError> {
    Ok(ArtifactRuntime {
        image_digest: digest_text(&observation.runtime.image_id)?,
        toolchain_identity: toolchain_identity(&observation.runtime),
        plugin: ArtifactPlugin {
            identity: PluginIdentity::Semver,
            version: 1,
            digest: digest_text(
                "sha256:f87889a5e26b6ee6f7656e8494c37842ab041349b04e5084a66c144df2ccc02b",
            )?,
        },
        implementation_digest: digest_text(&observation.execution_fingerprint.to_string())?,
    })
}

fn coverage_archive_entries(bytes: &[u8]) -> Result<u16, InspectionError> {
    if bytes.len() < 1024 || !bytes.len().is_multiple_of(512) {
        return Err(InspectionError::InvalidMetadata);
    }
    let mut offset = 0usize;
    let mut entries = 0_u16;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            return (bytes[offset..].iter().all(|byte| *byte == 0) && entries > 0)
                .then_some(entries)
                .ok_or(InspectionError::InvalidMetadata);
        }
        let text =
            std::str::from_utf8(&header[124..136]).map_err(|_| InspectionError::InvalidMetadata)?;
        let size = usize::from_str_radix(text.trim_matches(['\0', ' ']), 8)
            .map_err(|_| InspectionError::InvalidMetadata)?;
        entries = entries.checked_add(1).ok_or(InspectionError::OutputLimit)?;
        let padded = size.checked_add(511).ok_or(InspectionError::OutputLimit)? / 512 * 512;
        offset = offset
            .checked_add(512)
            .and_then(|value| value.checked_add(padded))
            .ok_or(InspectionError::OutputLimit)?;
    }
    Err(InspectionError::InvalidMetadata)
}

fn digest_text(value: &str) -> Result<[u8; 32], InspectionError> {
    let hex = value
        .strip_prefix("sha256:")
        .ok_or(InspectionError::Internal)?;
    if hex.len() != 64 {
        return Err(InspectionError::Internal);
    }
    let mut bytes = [0_u8; 32];
    for (index, slot) in bytes.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
            .map_err(|_| InspectionError::Internal)?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_application::coverage::{CoverageArtifactStreams, CoverageIdentity};
    use rust_engineering_application::nextest::{
        ArtifactStreams, NextestCounts, NextestOptions, NextestSelection,
    };
    use rust_engineering_application::semver_check::{
        SEMVER_DEFAULT_TIMEOUT_SECONDS, SemverOptions,
    };
    use rust_engineering_domain::coverage::{
        CoverageMetrics, CoverageOptions, CoverageSelection, CoverageSummary,
    };
    use rust_engineering_domain::semver_check::{
        SemverCommandOptions, SemverExit, SemverFindingCompleteness, SemverFindingCounts,
        SemverProjectSelection,
    };
    use rust_engineering_domain::{
        ExecutionFingerprint, ExecutionTermination, RuntimeIdentity, SourceFile,
    };

    fn fingerprint(hex: char) -> Result<ExecutionFingerprint, String> {
        format!("sha256:{}", hex.to_string().repeat(64))
            .parse()
            .map_err(|error| format!("{error:?}"))
    }

    fn source(path: &str, bytes: &[u8]) -> Result<SourceBundle, String> {
        let file = SourceFile::new(path.to_owned(), bytes.to_vec())
            .map_err(|error| format!("{error:?}"))?;
        SourceBundle::new(vec![file]).map_err(|error| format!("{error:?}"))
    }

    fn archive(payload: &[u8]) -> Vec<u8> {
        let padded = payload.len().div_ceil(512) * 512;
        let mut bytes = vec![0_u8; 512 + padded + 1024];
        let size = format!("{:011o}\0", payload.len());
        bytes[124..136].copy_from_slice(size.as_bytes());
        bytes[512..512 + payload.len()].copy_from_slice(payload);
        bytes
    }

    fn runtime() -> Result<RuntimeIdentity, String> {
        Ok(RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: format!("sha256:{}", "1".repeat(64)),
            configuration_fingerprint: fingerprint('2')?,
            execution_fingerprint: fingerprint('3')?,
            rust_version: "rustc 1.98.1".into(),
            cargo_version: "cargo 1.98.1".into(),
            declared_toolchain: None,
        })
    }

    #[test]
    fn a_realistic_html_bundle_exceeds_the_old_cap_but_fits_the_durable_member_cap() {
        let old_cap = 256 * 1024;
        let fixture_bytes = vec![b'h'; old_cap + 64 * 1024];
        assert!(fixture_bytes.len() > old_cap);
        assert!(u64::try_from(fixture_bytes.len()).is_ok_and(|size| size <= MEMBER_CAP));
        assert_eq!(MEMBER_CAP, 32 * 1024 * 1024);
    }

    #[test]
    fn durable_batch_keeps_prior_and_later_members_when_one_member_fails() {
        let mut published = Vec::new();
        let mut omitted = Vec::new();
        keep_or_omit(0, Ok("junit"), &mut published, &mut omitted);
        keep_or_omit(
            1,
            Err(QualityArtifactError::QuotaExceeded),
            &mut published,
            &mut omitted,
        );
        keep_or_omit(2, Ok("stderr"), &mut published, &mut omitted);
        assert_eq!(published, ["junit", "stderr"]);
        assert_eq!(omitted, [1]);
    }

    #[test]
    fn byte_input_is_incremental_and_handles_empty_buffers() {
        let mut input = Bytes(b"abcdef");
        let mut first = [0_u8; 2];
        assert_eq!(input.read(&mut first), Ok(2));
        assert_eq!(&first, b"ab");
        assert_eq!(input.read(&mut []), Ok(0));
        let mut rest = [0_u8; 8];
        assert_eq!(input.read(&mut rest), Ok(4));
        assert_eq!(&rest[..4], b"cdef");
        assert_eq!(input.read(&mut rest), Ok(0));
    }

    #[test]
    fn callback_authority_masks_callback_failures() -> Result<(), String> {
        let project: ProjectRef = "prj_00000000000000000000000000000000"
            .parse()
            .map_err(|error| format!("{error:?}"))?;
        let facts = QualityOwnerFacts {
            granted_root_device: 7,
            granted_root_inode: 11,
            workspace_root: "/workspace".into(),
        };
        let mut success = || Ok(facts.clone());
        let mut authority = CallbackAuthority {
            revalidate: &mut success,
        };
        assert_eq!(authority.revalidate_owner(&project), Ok(facts));

        let mut failure = || Err(InspectionError::Internal);
        let mut authority = CallbackAuthority {
            revalidate: &mut failure,
        };
        assert_eq!(
            authority.revalidate_owner(&project),
            Err(QualityArtifactError::Unauthorized)
        );
        Ok(())
    }

    #[test]
    fn artifact_errors_map_to_closed_inspection_categories() {
        for error in [
            QualityArtifactError::QuotaExceeded,
            QualityArtifactError::InvalidLimit,
        ] {
            assert_eq!(quality_error(error), InspectionError::OutputLimit);
        }
        for error in [
            QualityArtifactError::Unauthorized,
            QualityArtifactError::Expired,
            QualityArtifactError::NotFound,
        ] {
            assert_eq!(
                quality_error(error),
                InspectionError::Project(ProjectError::Rejected(
                    rust_engineering_domain::OperationalErrorCode::ProjectNotFound
                ))
            );
        }
        for error in [
            QualityArtifactError::InvalidId,
            QualityArtifactError::InvalidDescriptor,
            QualityArtifactError::InvalidTimestamp,
            QualityArtifactError::InvalidKindVersion,
            QualityArtifactError::Busy,
            QualityArtifactError::UnsupportedPlatform,
            QualityArtifactError::UnsupportedStateRoot,
            QualityArtifactError::Io,
            QualityArtifactError::RecoveryRequired,
            QualityArtifactError::RetentionDenied,
        ] {
            assert_eq!(quality_error(error), InspectionError::Internal);
        }
    }

    #[test]
    fn source_digests_are_deterministic_ordered_and_domain_separated() -> Result<(), String> {
        let baseline = source("src/lib.rs", b"pub fn old() {}\n")?;
        let same = source("src/lib.rs", b"pub fn old() {}\n")?;
        let candidate = source("src/lib.rs", b"pub fn new() {}\n")?;
        assert_eq!(source_digest(&baseline), source_digest(&same));
        assert_ne!(source_digest(&baseline), source_digest(&candidate));
        assert_ne!(
            semver_source_digest(&baseline, &candidate),
            semver_source_digest(&candidate, &baseline)
        );
        assert_ne!(
            semver_source_digest(&baseline, &candidate),
            source_digest(&baseline)
        );
        Ok(())
    }

    #[test]
    fn runtime_toolchain_digest_uses_both_versions() -> Result<(), String> {
        let runtime = runtime()?;
        let digest = toolchain_identity(&runtime);
        let mut changed = runtime.clone();
        changed.cargo_version.push_str(" changed");
        assert_ne!(digest, toolchain_identity(&changed));
        changed = runtime.clone();
        changed.rust_version.push_str(" changed");
        assert_ne!(digest, toolchain_identity(&changed));
        Ok(())
    }

    #[test]
    fn nextest_runtime_descriptor_binds_fixed_plugin_and_execution() -> Result<(), String> {
        let observation = NextestObservation {
            options: NextestOptions::try_from(NextestSelection::default())
                .map_err(|error| error.to_string())?,
            validation_complete: true,
            completeness: NextestCompleteness::Complete,
            counts: NextestCounts::default(),
            tests: Vec::new(),
            tests_omitted: 0,
            doctests_run: false,
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            runtime: runtime()?,
            execution_fingerprint: fingerprint('3')?,
            artifacts: ArtifactStreams::default(),
        };
        let descriptor = artifact_runtime(&observation).map_err(|error| format!("{error:?}"))?;
        assert_eq!(descriptor.image_digest, [0x11; 32]);
        assert_eq!(descriptor.plugin.identity, PluginIdentity::Nextest);
        assert_eq!(descriptor.plugin.version, 1);
        assert_eq!(descriptor.plugin.digest, NEXTEST_BINARY_SHA256);
        assert_eq!(descriptor.implementation_digest, [0x33; 32]);
        Ok(())
    }

    #[test]
    fn coverage_runtime_descriptor_hashes_the_observed_plugin_version() -> Result<(), String> {
        let metrics =
            CoverageMetrics::new((1, 1), (1, 1), (1, 1)).map_err(|error| error.to_string())?;
        let observation = CoverageObservation {
            options: CoverageOptions::try_from(CoverageSelection::default())
                .map_err(|error| error.to_string())?,
            summary: CoverageSummary {
                aggregate: metrics,
                packages: Vec::new(),
                files: Vec::new(),
                files_omitted: 0,
            },
            identity: CoverageIdentity {
                cargo_llvm_cov_version: "0.9.0".into(),
                manifest_path: "/source/Cargo.toml".into(),
                llvm_tools_version: "1.98.1".into(),
            },
            doctests_run: false,
            cfg_coverage_enabled: true,
            target: "aarch64-unknown-linux-gnu",
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            parse_complete: true,
            runtime: runtime()?,
            execution_fingerprint: fingerprint('3')?,
            artifacts: CoverageArtifactStreams::default(),
        };
        let descriptor =
            coverage_artifact_runtime(&observation).map_err(|error| format!("{error:?}"))?;
        let mut expected = Sha256::new();
        expected.update(b"cargo-llvm-cov\0");
        expected.update(b"0.9.0");
        let expected: [u8; 32] = expected.finalize().into();
        assert_eq!(descriptor.plugin.identity, PluginIdentity::Coverage);
        assert_eq!(descriptor.plugin.digest, expected);
        assert_eq!(descriptor.image_digest, [0x11; 32]);
        Ok(())
    }

    #[test]
    fn semver_runtime_descriptor_uses_the_admitted_binary_identity() -> Result<(), String> {
        let selection = SemverCommandOptions::try_from(SemverProjectSelection::default())
            .map_err(|error| error.to_string())?;
        let observation = SemverObservation {
            options: SemverOptions::new(
                selection.clone(),
                selection,
                SEMVER_DEFAULT_TIMEOUT_SECONDS,
            )
            .map_err(|error| error.to_string())?,
            exit: SemverExit::NoBreak,
            counts: SemverFindingCounts::default(),
            findings: Vec::new(),
            findings_omitted: 0,
            completeness: SemverFindingCompleteness::Incomplete,
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            runtime: runtime()?,
            execution_fingerprint: fingerprint('3')?,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        };
        let descriptor =
            semver_artifact_runtime(&observation).map_err(|error| format!("{error:?}"))?;
        assert_eq!(descriptor.plugin.identity, PluginIdentity::Semver);
        assert_eq!(descriptor.plugin.version, 1);
        assert_eq!(
            descriptor.plugin.digest,
            [
                0xf8, 0x78, 0x89, 0xa5, 0xe2, 0x6b, 0x6e, 0xe6, 0xf7, 0x65, 0x6e, 0x84, 0x94, 0xc3,
                0x78, 0x42, 0xab, 0x04, 0x13, 0x49, 0xb0, 0x4e, 0x50, 0x84, 0xa6, 0x6c, 0x14, 0x4d,
                0xf2, 0xcc, 0xc0, 0x2b,
            ]
        );
        assert_eq!(descriptor.implementation_digest, [0x33; 32]);
        Ok(())
    }

    #[test]
    fn runtime_descriptor_rejects_non_digest_image_identity() -> Result<(), String> {
        let mut observation = NextestObservation {
            options: NextestOptions::try_from(NextestSelection::default())
                .map_err(|error| error.to_string())?,
            validation_complete: true,
            completeness: NextestCompleteness::Complete,
            counts: NextestCounts::default(),
            tests: Vec::new(),
            tests_omitted: 0,
            doctests_run: false,
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            runtime: runtime()?,
            execution_fingerprint: fingerprint('3')?,
            artifacts: ArtifactStreams::default(),
        };
        observation.runtime.image_id = "mutable-tag".into();
        assert!(matches!(
            artifact_runtime(&observation),
            Err(InspectionError::Internal)
        ));
        Ok(())
    }

    #[test]
    fn digest_text_accepts_only_prefixed_exact_hex() {
        let valid = format!("sha256:{}", "ab".repeat(32));
        assert_eq!(digest_text(&valid), Ok([0xab; 32]));
        for invalid in [
            "ab",
            "sha256:ab",
            "sha256:zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        ] {
            assert_eq!(digest_text(invalid), Err(InspectionError::Internal));
        }
    }

    #[test]
    fn coverage_archive_counts_entries_and_rejects_bad_framing() {
        assert_eq!(coverage_archive_entries(&archive(b"abc")), Ok(1));

        let mut two = archive(b"first");
        let second = archive(b"second");
        two.truncate(two.len() - 1024);
        two.extend_from_slice(&second);
        assert_eq!(coverage_archive_entries(&two), Ok(2));

        assert_eq!(
            coverage_archive_entries(&[0_u8; 512]),
            Err(InspectionError::InvalidMetadata)
        );
        assert_eq!(
            coverage_archive_entries(&[0_u8; 1025]),
            Err(InspectionError::InvalidMetadata)
        );
        assert_eq!(
            coverage_archive_entries(&[0_u8; 1024]),
            Err(InspectionError::InvalidMetadata)
        );
        let mut invalid_size = archive(b"x");
        invalid_size[124..136].fill(b'x');
        assert_eq!(
            coverage_archive_entries(&invalid_size),
            Err(InspectionError::InvalidMetadata)
        );
        let mut trailing = archive(b"x");
        let final_byte = trailing.len() - 1;
        trailing[final_byte] = 1;
        assert_eq!(
            coverage_archive_entries(&trailing),
            Err(InspectionError::InvalidMetadata)
        );
    }
}
