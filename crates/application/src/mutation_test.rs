//! Application boundary for one captured `cargo mutants` run (M3-05).
//!
//! The verdict rules live here, not at the protocol boundary, and they are
//! deliberately asymmetric:
//!
//! * a clean result requires complete, mutually consistent machine-readable
//!   evidence, a passing baseline, every generated mutant tested and every
//!   viable mutant caught;
//! * a failing result (a missed mutant, or a failing baseline) stands on the
//!   parsed report alone, because missing *extra* evidence must never erase a
//!   proven failure;
//! * anything else — a partial parse, an inconsistent cross-check, a capped
//!   selection, a timeout — is incomplete and is never promoted to either.

use crate::{
    ArtifactInput, ArtifactStore, InspectionControl, InspectionError, ProjectRegistry,
    ProjectSourceBackend, QualityOwnerFacts, QualityProjectBackend, ReferenceGenerator,
    RegistryClock,
};
use rust_engineering_domain::mutation_test::{
    MUTATION_MAX_ROWS, MUTATION_MAX_VERSION, MutationBaseline, MutationCounts,
    MutationGuestIdentity, MutationMutantRow, MutationTestCommandOptions,
};
use rust_engineering_domain::{
    ArtifactError, ArtifactMetadata, ExecutionFingerprint, ExecutionTermination, ProjectRef,
    QualityArtifactDescriptor, RuntimeIdentity, SourceBundle, UnixSeconds,
};

/// ADR-060 job execute budget default and maximum. A mutation run is bounded by
/// the smaller of this and the derived per-selection budget below.
pub const MUTATION_DEFAULT_TIMEOUT_SECONDS: u64 = 300;
pub const MUTATION_MAX_TIMEOUT_SECONDS: u64 = 3_600;
/// Largest report bundle retained as one artifact member (ADR-062 §4).
pub const MUTATION_ARTIFACT_MAX_BYTES: usize = 8 * 1024 * 1024;
/// Stage-0's per-artifact ceiling. A bundle above it is dropped rather than
/// truncated: half a tar archive is not an archive.
pub const MUTATION_STAGE0_MAX_BYTES: usize = 256 * 1024;

/// Total wall budget for one mutation job, derived only from validated options.
/// One build plus one bounded run per capped mutant, clamped into the ADR-060
/// range.
pub fn total_budget_seconds(options: &MutationTestCommandOptions) -> u64 {
    options
        .mutant_timeout_seconds()
        .saturating_mul(u64::from(options.max_mutants()))
        .saturating_add(options.build_timeout_seconds())
        .clamp(
            MUTATION_DEFAULT_TIMEOUT_SECONDS,
            MUTATION_MAX_TIMEOUT_SECONDS,
        )
}

/// M3-01's synchronous rule, applied unchanged: only a run whose whole work
/// budget fits in 60 seconds may execute synchronously.
///
/// No mutation selection reaches that bound — the derived budget starts at the
/// ADR-060 default of 300 seconds — so under the current gate every call
/// returns the structured `TASKS_REQUIRED` remediation. This is stated as a
/// computed property rather than a hard-coded `false` so the rule keeps holding
/// if the derivation is ever recalibrated.
pub fn synchronous_qualified(options: &MutationTestCommandOptions) -> bool {
    total_budget_seconds(options) <= 60
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationCompleteness {
    Complete,
    Partial,
    Invalid,
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MutationArtifactStreams {
    /// The only oracle: bytes of `mutants.out/outcomes.json`.
    pub outcomes_json: Vec<u8>,
    /// A validated, never previewed USTAR bundle of the bounded `diff/`,
    /// `logs/` and outcome list files. `lock.json` is excluded at the exporter.
    pub report_bundle: Vec<u8>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub outcomes_truncated: bool,
    pub bundle_unavailable: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// USTAR entries in the bundle, for the ADR-061 member accounting a durable
    /// publisher owes before it can store the bundle as one member.
    pub bundle_entries: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationArtifactKind {
    OutcomesJson,
    ArchiveBundle,
    StdoutLog,
    StderrLog,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MutationArtifactReference {
    Ephemeral {
        kind: MutationArtifactKind,
        metadata: ArtifactMetadata,
    },
    Durable(Box<QualityArtifactDescriptor>),
}

#[derive(Clone, Debug)]
pub struct MutationTestObservation {
    pub options: MutationTestCommandOptions,
    pub completeness: MutationCompleteness,
    /// Every structural cross-check held: `outcomes.json`, the per-class list
    /// files and the listing-pass denominator agree.
    pub validation_complete: bool,
    pub baseline: MutationBaseline,
    pub counts: MutationCounts,
    pub mutants: Vec<MutationMutantRow>,
    pub mutants_omitted: u64,
    /// The generated set exceeded `max_mutants`; nothing was built or run.
    pub cap_exceeded: bool,
    pub mutants_version: String,
    pub guest_identity: MutationGuestIdentity,
    pub termination: ExecutionTermination,
    pub exit_code: Option<i32>,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub artifacts: MutationArtifactStreams,
}

impl MutationTestObservation {
    /// Structural bounds only. Verdict rules are separate on purpose: this must
    /// stay usable to validate a stored task result long after the run.
    pub fn validate(&self) -> Result<(), InspectionError> {
        if self.mutants.len() > MUTATION_MAX_ROWS
            || self.mutants_version.len() > MUTATION_MAX_VERSION
            || self.artifacts.report_bundle.len() > MUTATION_ARTIFACT_MAX_BYTES
            || self.runtime.platform.is_empty()
            || self.runtime.platform.len() > 128
            || self.runtime.image_id.len() != 71
            || !self.runtime.image_id.starts_with("sha256:")
            || !self
                .runtime
                .image_id
                .as_bytes()
                .get(7..)
                .is_some_and(|digest| {
                    digest
                        .iter()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
                })
            || self.runtime.execution_fingerprint != self.execution_fingerprint
            || self.runtime.rust_version.is_empty()
            || self.runtime.rust_version.len() > 128
            || self.runtime.cargo_version.is_empty()
            || self.runtime.cargo_version.len() > 128
            || self
                .runtime
                .declared_toolchain
                .as_ref()
                .is_some_and(|value| value.is_empty() || value.len() > 32)
            || !self.counts.consistent() && self.completeness == MutationCompleteness::Complete
            || (self.validation_complete
                && (self.completeness != MutationCompleteness::Complete
                    || self.cap_exceeded
                    || self.mutants_omitted != 0
                    || self.artifacts.outcomes_truncated
                    || self.artifacts.bundle_unavailable
                    || self.mutants_version.is_empty()))
        {
            return Err(InspectionError::InvalidMetadata);
        }
        Ok(())
    }

    /// A clean mutation result. Every clause is required; none of them can be
    /// satisfied by human-readable tool output.
    pub fn clean(&self) -> bool {
        self.validation_complete
            && self.completeness == MutationCompleteness::Complete
            && self.baseline == MutationBaseline::Passed
            && self.counts.clean()
            && !self.cap_exceeded
            && self.termination == ExecutionTermination::Exited
            && self.guest_identity == MutationGuestIdentity::Guest
    }

    /// A conclusive negative result: a surviving mutant or a failing baseline.
    /// This holds on the parsed report alone, so absent secondary evidence
    /// cannot turn a proven failure back into "incomplete".
    pub fn conclusive_failure(&self) -> bool {
        self.termination == ExecutionTermination::Exited
            && self.completeness != MutationCompleteness::Unavailable
            && self.completeness != MutationCompleteness::Invalid
            && (self.baseline == MutationBaseline::Failed || self.counts.missed >= 1)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationTaskResultError {
    InvalidObservation,
    InvalidDuration,
    TooManyArtifacts,
}

impl std::fmt::Display for MutationTaskResultError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidObservation => "invalid mutation observation",
            Self::InvalidDuration => "invalid mutation duration",
            Self::TooManyArtifacts => "too many mutation artifacts",
        })
    }
}

impl std::error::Error for MutationTaskResultError {}

/// Registry-safe terminal value: the byte streams are erased once publication
/// has produced references, so a retained task record holds bounded facts only.
#[derive(Clone, Debug)]
pub struct MutationTestTaskResult {
    observation: MutationTestObservation,
    artifacts: Vec<MutationArtifactReference>,
    expected_artifacts: u8,
    duration_ms: u64,
}

impl MutationTestTaskResult {
    pub fn new(
        mut observation: MutationTestObservation,
        artifacts: Vec<MutationArtifactReference>,
        duration_ms: u64,
    ) -> Result<Self, MutationTaskResultError> {
        observation
            .validate()
            .map_err(|_| MutationTaskResultError::InvalidObservation)?;
        if artifacts.len() > 128 {
            return Err(MutationTaskResultError::TooManyArtifacts);
        }
        if duration_ms > 3_840_000 {
            return Err(MutationTaskResultError::InvalidDuration);
        }
        let expected_artifacts = u8::from(!observation.artifacts.outcomes_json.is_empty())
            + u8::from(!observation.artifacts.report_bundle.is_empty())
            + u8::from(!observation.artifacts.stdout.is_empty())
            + u8::from(!observation.artifacts.stderr.is_empty());
        observation.artifacts.outcomes_json.clear();
        observation.artifacts.report_bundle.clear();
        observation.artifacts.stdout.clear();
        observation.artifacts.stderr.clear();
        Ok(Self {
            observation,
            artifacts,
            expected_artifacts,
            duration_ms,
        })
    }

    pub fn artifacts(&self) -> &[MutationArtifactReference] {
        &self.artifacts
    }

    pub fn into_parts(
        self,
    ) -> (
        MutationTestObservation,
        Vec<MutationArtifactReference>,
        u8,
        u64,
    ) {
        (
            self.observation,
            self.artifacts,
            self.expected_artifacts,
            self.duration_ms,
        )
    }
}

/// Execute `cargo mutants` over the supplied captured `SourceBundle` only.
///
/// Implementations must use the single closed Execution Gateway with a
/// mandatory baseline, a private writable copy that is never exported, a
/// read-only `/source`, network-denied containment and joined cancellation.
/// Counts come only from bounded machine-readable `mutants.out` evidence; the
/// returned byte streams are untrusted bounded artifacts. This port neither
/// opens host paths nor publishes artifacts.
pub trait ProjectMutationTestPort: Send + Sync {
    fn run(
        &self,
        source: &SourceBundle,
        options: &MutationTestCommandOptions,
        control: &dyn InspectionControl,
    ) -> Result<MutationTestObservation, InspectionError>;
}

/// Durable publication boundary, mirroring the nextest and coverage verticals.
/// A durable publisher must charge `artifacts.bundle_entries` against the
/// ADR-061 members/job budget before storing the bundle as one member.
pub trait MutationTestDurablePublisher: Send {
    fn publish(
        &mut self,
        project: &ProjectRef,
        captured_at: UnixSeconds,
        source: &SourceBundle,
        observation: &mut MutationTestObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<MutationArtifactReference>, InspectionError>;
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

fn artifact_error(error: ArtifactError) -> InspectionError {
    match error {
        ArtifactError::QuotaExceeded => InspectionError::OutputLimit,
        _ => InspectionError::Internal,
    }
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    /// Capture, execute and publish ADR-061 Stage-0 evidence under one live
    /// ProjectRef authorization boundary.
    pub fn mutation_test(
        &mut self,
        reference: &ProjectRef,
        options: &MutationTestCommandOptions,
        runner: &impl ProjectMutationTestPort,
        artifacts: &mut impl ArtifactStore,
        clock: &impl rust_engineering_domain::Clock,
        control: &dyn InspectionControl,
    ) -> Result<(MutationTestObservation, Vec<MutationArtifactReference>), InspectionError> {
        let captured = self.capture_validation(reference, artifacts, clock, control)?;
        let mut observation = runner.run(&captured.source, options, control)?;
        let published = self.publish_mutation_test_stage0(
            &captured.reference,
            &mut observation,
            artifacts,
            control,
        )?;
        Ok((observation, published))
    }

    /// Stage-0 publication. The report bundle is published only when it fits
    /// whole: an ArchiveBundle truncated by the store's per-artifact ceiling
    /// would be an unusable tar stream, so it is dropped and recorded as
    /// unavailable instead.
    pub(crate) fn publish_mutation_test_stage0(
        &mut self,
        reference: &ProjectRef,
        observation: &mut MutationTestObservation,
        artifacts: &mut impl ArtifactStore,
        control: &dyn InspectionControl,
    ) -> Result<Vec<MutationArtifactReference>, InspectionError> {
        if observation.artifacts.report_bundle.len() > MUTATION_STAGE0_MAX_BYTES {
            observation.artifacts.report_bundle.clear();
            observation.artifacts.bundle_unavailable = true;
            observation.validation_complete = false;
        }
        let streams = [
            (
                MutationArtifactKind::OutcomesJson,
                observation.artifacts.outcomes_json.as_slice(),
                observation.artifacts.outcomes_truncated,
            ),
            (
                MutationArtifactKind::ArchiveBundle,
                observation.artifacts.report_bundle.as_slice(),
                false,
            ),
            (
                MutationArtifactKind::StdoutLog,
                observation.artifacts.stdout.as_slice(),
                observation.artifacts.stdout_truncated,
            ),
            (
                MutationArtifactKind::StderrLog,
                observation.artifacts.stderr.as_slice(),
                observation.artifacts.stderr_truncated,
            ),
        ];
        self.reap_artifacts(artifacts).map_err(artifact_error)?;
        let mut published = Vec::new();
        let mut degraded = false;
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
                    if metadata.truncated {
                        degraded = true;
                        if kind == MutationArtifactKind::OutcomesJson {
                            observation.artifacts.outcomes_truncated = true;
                        }
                    }
                    published.push(MutationArtifactReference::Ephemeral { kind, metadata });
                }
                Err(ArtifactError::QuotaExceeded) => degraded = true,
                Err(error) => return Err(artifact_error(error)),
            }
        }
        if degraded {
            observation.validation_complete = false;
            if observation.completeness == MutationCompleteness::Complete {
                observation.completeness = MutationCompleteness::Partial;
            }
        }
        if let Err(error) = self.resolve_inner(reference, control, true) {
            for artifact in &published {
                let MutationArtifactReference::Ephemeral { metadata, .. } = artifact else {
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
}

impl<B, G, C> ProjectRegistry<B, G, C>
where
    B: ProjectSourceBackend + QualityProjectBackend,
    G: ReferenceGenerator,
    C: RegistryClock,
{
    /// Capture and execute as usual, but commit artifacts through ADR-061's
    /// durable publisher. The live grant is rechecked at the commit point.
    #[allow(clippy::too_many_arguments)] // One explicit port per authority/publication boundary.
    pub fn mutation_test_durable(
        &mut self,
        reference: &ProjectRef,
        options: &MutationTestCommandOptions,
        runner: &impl ProjectMutationTestPort,
        stage0: &mut impl ArtifactStore,
        publisher: &mut dyn MutationTestDurablePublisher,
        clock: &impl rust_engineering_domain::Clock,
        control: &dyn InspectionControl,
    ) -> Result<(MutationTestObservation, Vec<MutationArtifactReference>), InspectionError> {
        let captured = self.capture_validation(reference, stage0, clock, control)?;
        let mut observation = runner.run(&captured.source, options, control)?;
        let mut revalidate = || {
            self.quality_owner_facts(reference, control)
                .map_err(InspectionError::from)
        };
        let published = publisher.publish(
            &captured.reference,
            captured.created_at,
            &captured.source,
            &mut observation,
            &mut revalidate,
        )?;
        self.resolve_inner(reference, control, true)?;
        Ok((observation, published))
    }
}
