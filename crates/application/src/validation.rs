//! Shared captured-validation authorization and bounded log publication.
use crate::{
    ArtifactAccessError, ArtifactInput, ArtifactStore, InspectionControl, InspectionError,
    ProjectError, ProjectRegistry, ProjectSourceBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    ArtifactError, ArtifactMetadata, CheckObservation, Clock, FreshnessPolicy, IntegrityStatus,
    OperationalErrorCode, ProjectIdentityFingerprint, ProjectRef, Provenance, SnapshotEvidence,
    SourceBundle, SourceKind, UnixSeconds,
};

const MAX_STREAM: usize = 256 * 1024;
const LOG_STREAM: usize = (MAX_STREAM - 128) / 2;

fn bounded_stream(value: &str) -> (&str, bool) {
    let mut end = value.len().min(LOG_STREAM);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (&value[..end], end < value.len())
}

pub(crate) struct CapturedValidation {
    pub(crate) reference: ProjectRef,
    pub(crate) identity: ProjectIdentityFingerprint,
    pub(crate) created_at: UnixSeconds,
    pub(crate) source: SourceBundle,
}

struct BytesInput<'a> {
    remaining: &'a [u8],
    truncated: bool,
}
impl ArtifactInput for BytesInput<'_> {
    fn truncated(&self) -> bool {
        self.truncated
    }
    fn read(&mut self, out: &mut [u8]) -> Result<usize, ArtifactError> {
        let count = out.len().min(self.remaining.len());
        out[..count].copy_from_slice(&self.remaining[..count]);
        self.remaining = &self.remaining[count..];
        Ok(count)
    }
}

pub(crate) struct ValidationPublication {
    pub(crate) project_ref: ProjectRef,
    pub(crate) project_identity_fingerprint: ProjectIdentityFingerprint,
    pub(crate) evidence: SnapshotEvidence,
    pub(crate) log: Option<ArtifactMetadata>,
    pub(crate) retention_remaining_seconds: Option<u64>,
}

struct LogInput<'a> {
    remaining: &'a [u8],
    truncated: bool,
}
impl ArtifactInput for LogInput<'_> {
    fn truncated(&self) -> bool {
        self.truncated
    }
    fn read(&mut self, out: &mut [u8]) -> Result<usize, ArtifactError> {
        let count = out.len().min(self.remaining.len());
        out[..count].copy_from_slice(&self.remaining[..count]);
        self.remaining = &self.remaining[count..];
        Ok(count)
    }
}

fn artifact_error(error: ArtifactError) -> InspectionError {
    match error {
        ArtifactError::QuotaExceeded => InspectionError::OutputLimit,
        _ => InspectionError::Internal,
    }
}
fn access_error(error: ArtifactAccessError) -> InspectionError {
    match error {
        ArtifactAccessError::NotFound => InspectionError::Project(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound,
        )),
        ArtifactAccessError::Cancelled => InspectionError::Project(ProjectError::Cancelled),
        ArtifactAccessError::Internal => InspectionError::Internal,
    }
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub(crate) fn publish_coverage_stage0(
        &mut self,
        captured: CapturedValidation,
        observation: &mut crate::coverage::CoverageObservation,
        artifacts: &mut impl ArtifactStore,
        control: &dyn InspectionControl,
    ) -> Result<Vec<crate::coverage::CoverageArtifactReference>, InspectionError> {
        use crate::coverage::{CoverageArtifactKind, CoverageArtifactReference};
        let reference = &captured.reference;
        let streams = [
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
        self.reap_artifacts(artifacts).map_err(artifact_error)?;
        let mut published = Vec::new();
        for (kind, bytes, truncated) in streams {
            if bytes.is_empty() {
                continue;
            }
            match artifacts.capture(
                reference,
                &mut BytesInput {
                    remaining: bytes,
                    truncated,
                },
            ) {
                Ok(metadata) => {
                    published.push(CoverageArtifactReference::Ephemeral { kind, metadata })
                }
                Err(ArtifactError::QuotaExceeded) => {
                    observation.parse_complete = false;
                }
                Err(error) => return Err(artifact_error(error)),
            }
        }
        self.resolve_inner(reference, control, true)?;
        Ok(published)
    }
    pub(crate) fn publish_nextest_stage0(
        &mut self,
        captured: CapturedValidation,
        observation: &mut crate::nextest::NextestObservation,
        artifacts: &mut impl ArtifactStore,
        control: &dyn InspectionControl,
    ) -> Result<Vec<crate::nextest::NextestArtifactReference>, InspectionError> {
        use crate::nextest::{NextestArtifactKind, NextestArtifactReference, NextestCompleteness};

        let reference = &captured.reference;
        let streams = [
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
        self.reap_artifacts(artifacts).map_err(artifact_error)?;
        let mut published = Vec::new();
        for (kind, bytes, already_truncated) in streams {
            if bytes.is_empty() {
                continue;
            }
            match artifacts.capture(
                reference,
                &mut BytesInput {
                    remaining: bytes,
                    truncated: already_truncated,
                },
            ) {
                Ok(metadata) => {
                    match kind {
                        NextestArtifactKind::JunitXml => {
                            observation.artifacts.junit_truncated |= metadata.truncated
                        }
                        NextestArtifactKind::StdoutLog => {
                            observation.artifacts.stdout_truncated |= metadata.truncated
                        }
                        NextestArtifactKind::StderrLog => {
                            observation.artifacts.stderr_truncated |= metadata.truncated
                        }
                    }
                    published.push(NextestArtifactReference::Ephemeral { kind, metadata });
                }
                Err(ArtifactError::QuotaExceeded) => {
                    observation.validation_complete = false;
                    observation.completeness = NextestCompleteness::Partial;
                }
                Err(error) => return Err(artifact_error(error)),
            }
        }
        if observation.artifacts.junit_truncated
            || observation.artifacts.stdout_truncated
            || observation.artifacts.stderr_truncated
        {
            observation.validation_complete = false;
            observation.completeness = NextestCompleteness::Partial;
        }
        if let Err(error) = self.resolve_inner(reference, control, true) {
            for artifact in &published {
                let NextestArtifactReference::Ephemeral { metadata, .. } = artifact else {
                    continue;
                };
                artifacts
                    .remove(reference, &metadata.id)
                    .map_err(|_| InspectionError::Internal)?;
            }
            return Err(error.into());
        }
        Ok(published)
    }

    pub(crate) fn capture_validation(
        &mut self,
        reference: &ProjectRef,
        artifacts: &mut impl ArtifactStore,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<CapturedValidation, InspectionError> {
        self.reap_artifacts(artifacts).map_err(artifact_error)?;
        let identity = match self.resolve_inner(reference, control, false) {
            Ok(identity) => identity,
            Err(error) => {
                self.reap_artifacts(artifacts).map_err(artifact_error)?;
                return Err(error.into());
            }
        };
        let created_at = clock.now();
        let source = match self.source_inner(reference, control, false) {
            Ok(source) => source,
            Err(error) => {
                self.reap_artifacts(artifacts).map_err(artifact_error)?;
                return Err(error.into());
            }
        };
        Ok(CapturedValidation {
            reference: reference.clone(),
            identity: identity.fingerprint,
            created_at,
            source,
        })
    }

    pub(crate) fn publish_validation(
        &mut self,
        captured: CapturedValidation,
        observation: &mut CheckObservation,
        artifacts: &mut impl ArtifactStore,
        clocks: (&impl Clock, &impl RegistryClock),
        control: &dyn InspectionControl,
    ) -> Result<ValidationPublication, InspectionError> {
        let reference = &captured.reference;
        let created_at = captured.created_at;
        control.check()?;
        if observation.stdout.len() > MAX_STREAM || observation.stderr.len() > MAX_STREAM {
            return Err(InspectionError::OutputLimit);
        }
        let provenance = Provenance::new(
            SourceKind::ProjectSnapshot,
            observation
                .source_fingerprint
                .to_string()
                .parse()
                .map_err(|_| InspectionError::Internal)?,
            Some(created_at),
            Some(clocks.0.now()),
            IntegrityStatus::Verified,
            false,
        )
        .map_err(|_| InspectionError::Internal)?;
        let policy = FreshnessPolicy::new(
            "captured-project-v1"
                .parse()
                .map_err(|_| InspectionError::Internal)?,
            60,
            300,
        )
        .map_err(|_| InspectionError::Internal)?;
        let evidence = SnapshotEvidence::assess(provenance, policy, clocks.0);
        let (stdout, stdout_cut) = bounded_stream(&observation.stdout);
        let (stderr, stderr_cut) = bounded_stream(&observation.stderr);
        observation.stdout_truncated |= stdout_cut;
        observation.stderr_truncated |= stderr_cut;
        let marker = |cut| if cut { "\n[stream truncated]" } else { "" };
        let log = format!(
            "=== stdout ===\n{}{}\n=== stderr ===\n{}{}",
            stdout,
            marker(observation.stdout_truncated),
            stderr,
            marker(observation.stderr_truncated),
        );
        self.reap_artifacts(artifacts).map_err(artifact_error)?;
        let metadata = artifacts.capture(
            reference,
            &mut LogInput {
                remaining: log.as_bytes(),
                truncated: observation.stdout_truncated || observation.stderr_truncated,
            },
        );
        let (log, retention_remaining_seconds) = match metadata {
            Ok(metadata) => {
                // Authorization renews only after cancellation, lease and retention checks.
                let authorized =
                    match self.read_artifact(reference, &metadata.id, artifacts, clocks.1, control)
                    {
                        Ok(authorized) => authorized,
                        Err(error) => {
                            artifacts
                                .remove(reference, &metadata.id)
                                .map_err(|_| InspectionError::Internal)?;
                            return Err(access_error(error));
                        }
                    };
                (
                    Some(authorized.metadata),
                    Some(authorized.retention_remaining_seconds),
                )
            }
            Err(ArtifactError::QuotaExceeded) => {
                // Log retention is optional; the validation assessment remains usable.
                observation.stdout_truncated |= !observation.stdout.is_empty();
                observation.stderr_truncated |= !observation.stderr.is_empty();
                if let Err(error) = self.resolve_inner(reference, control, false) {
                    self.reap_artifacts(artifacts).map_err(artifact_error)?;
                    return Err(error.into());
                }
                control.check()?;
                self.reap_artifacts(artifacts).map_err(artifact_error)?;
                let now = self.clock.seconds();
                let entry = self
                    .entries
                    .get_mut(reference)
                    .ok_or(InspectionError::Project(ProjectError::Rejected(
                        OperationalErrorCode::ProjectNotFound,
                    )))?;
                if !now
                    .checked_sub(entry.last_used)
                    .is_some_and(|age| age < self.ttl_seconds)
                {
                    self.entries.remove(reference);
                    self.reap_artifacts(artifacts).map_err(artifact_error)?;
                    return Err(InspectionError::Project(ProjectError::Rejected(
                        OperationalErrorCode::ProjectNotFound,
                    )));
                }
                entry.last_used = now;
                (None, None)
            }
            Err(error) => return Err(artifact_error(error)),
        };
        // Retain the gateway's truncation flags separately: a bounded producer
        // can already have discarded bytes before ArtifactStore sees the stream.
        observation.stdout.clear();
        observation.stderr.clear();
        Ok(ValidationPublication {
            project_ref: captured.reference,
            project_identity_fingerprint: captured.identity,
            evidence,
            log,
            retention_remaining_seconds,
        })
    }
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub(crate) fn publish_quality(
        &mut self,
        captured: CapturedValidation,
        result: (
            Option<rust_engineering_domain::SourceFingerprint>,
            rust_engineering_domain::QualityProfile,
            Vec<rust_engineering_domain::QualityStageReport>,
        ),
        artifacts: &mut impl ArtifactStore,
        clocks: (&impl Clock, &impl RegistryClock),
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::ProjectQualityGate, InspectionError> {
        use rust_engineering_domain::*;
        let (fingerprint, profile, mut stages) = result;
        let reference = &captured.reference;
        let mut evidence = match &fingerprint {
            Some(fp) => {
                let provenance = Provenance::new(
                    SourceKind::ProjectSnapshot,
                    fp.to_string()
                        .parse()
                        .map_err(|_| InspectionError::Internal)?,
                    Some(captured.created_at),
                    Some(captured.created_at),
                    IntegrityStatus::Verified,
                    false,
                )
                .map_err(|_| InspectionError::Internal)?;
                let policy = FreshnessPolicy::new(
                    "captured-project-v1"
                        .parse()
                        .map_err(|_| InspectionError::Internal)?,
                    60,
                    300,
                )
                .map_err(|_| InspectionError::Internal)?;
                Evidence::Snapshot(SnapshotEvidence::assess(provenance, policy, clocks.0))
            }
            None => Evidence::Local,
        };
        let mut pending = Vec::new();
        let publication = (|| {
            for row in &mut stages {
                control.check()?;
                let Some(observation) = row
                    .observation
                    .as_mut()
                    .and_then(QualityObservation::execution_mut)
                else {
                    continue;
                };
                if observation.stdout.len() > MAX_STREAM || observation.stderr.len() > MAX_STREAM {
                    return Err(InspectionError::OutputLimit);
                }
                let (stdout, stdout_cut) = bounded_stream(&observation.stdout);
                let (stderr, stderr_cut) = bounded_stream(&observation.stderr);
                let marker = |cut| if cut { "\n[stream truncated]" } else { "" };
                let truncated = observation.stdout_truncated
                    || observation.stderr_truncated
                    || stdout_cut
                    || stderr_cut;
                let log = format!(
                    "=== stdout ===\n{}{}\n=== stderr ===\n{}{}",
                    stdout,
                    marker(observation.stdout_truncated || stdout_cut),
                    stderr,
                    marker(observation.stderr_truncated || stderr_cut)
                );
                observation.stdout_truncated |= stdout_cut;
                observation.stderr_truncated |= stderr_cut;
                let had_stdout = !observation.stdout.is_empty();
                let had_stderr = !observation.stderr.is_empty();
                observation.stdout.clear();
                observation.stderr.clear();
                match artifacts.capture(
                    reference,
                    &mut LogInput {
                        remaining: log.as_bytes(),
                        truncated,
                    },
                ) {
                    Ok(metadata) => {
                        if metadata.owner != *reference {
                            return Err(InspectionError::Internal);
                        }
                        pending.push(metadata.id.clone());
                        let authorized = self
                            .read_artifact_without_touch(
                                reference,
                                &metadata.id,
                                artifacts,
                                clocks.1,
                                control,
                            )
                            .map_err(access_error)?;
                        if authorized.metadata != metadata {
                            return Err(InspectionError::Internal);
                        }
                        row.log = Some(metadata);
                    }
                    Err(ArtifactError::QuotaExceeded) => {
                        observation.stdout_truncated |= had_stdout;
                        observation.stderr_truncated |= had_stderr;
                    }
                    Err(error) => return Err(artifact_error(error)),
                }
            }
            // Recheck all artifacts after staging the last one; no per-log touch.
            for row in &stages {
                if let Some(metadata) = &row.log {
                    let authorized = self
                        .read_artifact_without_touch(
                            reference,
                            &metadata.id,
                            artifacts,
                            clocks.1,
                            control,
                        )
                        .map_err(access_error)?;
                    if authorized.metadata != *metadata {
                        return Err(InspectionError::Internal);
                    }
                }
            }
            self.resolve_inner(reference, control, false)?;
            control.check()?;
            let now = clocks.1.seconds();
            for row in &mut stages {
                if let Some(metadata) = &row.log {
                    if now < metadata.created_seconds
                        || metadata.expires_seconds <= metadata.created_seconds
                    {
                        return Err(InspectionError::Internal);
                    }
                    row.retention_remaining_seconds = Some(
                        metadata
                            .expires_seconds
                            .checked_sub(now)
                            .filter(|n| *n > 0)
                            .ok_or(InspectionError::Project(ProjectError::Rejected(
                                OperationalErrorCode::ProjectNotFound,
                            )))?,
                    );
                }
                if let Some(QualityObservation::Audit { observation, .. }) = &mut row.observation
                    && let Some(snapshot) = &observation.snapshot
                {
                    observation.snapshot = Some(SnapshotEvidence::assess(
                        snapshot.provenance().clone(),
                        snapshot.freshness().policy().clone(),
                        clocks.0,
                    ));
                }
                row.classify();
            }
            if let Evidence::Snapshot(snapshot) = &evidence {
                evidence = Evidence::Snapshot(SnapshotEvidence::assess(
                    snapshot.provenance().clone(),
                    snapshot.freshness().policy().clone(),
                    clocks.0,
                ));
            }
            // Publication commit point: later cancellation cannot revoke delivery
            // or undo a completed lease renewal. Earlier signals roll back the group.
            control.check()?;
            self.touch_authorized_reference(reference)
                .map_err(access_error)?;
            Ok(())
        })();
        if let Err(error) = publication {
            let mut rollback_failed = false;
            for id in pending {
                if artifacts.remove(reference, &id).is_err() {
                    rollback_failed = true;
                }
            }
            if self.reap_artifacts(artifacts).is_err() {
                rollback_failed = true;
            }
            return Err(if rollback_failed {
                InspectionError::Internal
            } else {
                error
            });
        }
        Ok(ProjectQualityGate {
            project_ref: captured.reference,
            project_identity_fingerprint: captured.identity,
            semantics: InspectionSemantics::LatestKnown,
            profile,
            source_fingerprint: fingerprint,
            stages,
            evidence,
        })
    }
}
