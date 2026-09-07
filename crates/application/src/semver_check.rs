//! Application boundary for one local-baseline semantic-version comparison.

use crate::{
    ArtifactInput, ArtifactStore, InspectionControl, InspectionError, ProjectInspectionPort,
    ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts, QualityProjectBackend,
    ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    ArtifactError, ArtifactMetadata, Clock, ExecutionFingerprint, ExecutionTermination,
    FreshnessPolicy, IntegrityStatus, ProjectIdentityFingerprint, ProjectRef, ProjectStructure,
    Provenance, QualityArtifactDescriptor, RuntimeIdentity, SnapshotEvidence, SourceBundle,
    SourceKind, TargetKind, UnixSeconds,
    semver_check::{
        SemverCommandOptions, SemverExit, SemverFinding, SemverFindingCompleteness,
        SemverFindingCounts,
    },
};

pub const SEMVER_DEFAULT_TIMEOUT_SECONDS: u64 = 300;
pub const SEMVER_MAX_TIMEOUT_SECONDS: u64 = 3_600;
pub const SEMVER_MAX_FINDINGS: usize = 512;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemverOptions {
    baseline_selection: SemverCommandOptions,
    candidate_selection: SemverCommandOptions,
    timeout_seconds: u64,
}

impl SemverOptions {
    pub fn new(
        baseline_selection: SemverCommandOptions,
        candidate_selection: SemverCommandOptions,
        mut timeout_seconds: u64,
    ) -> Result<Self, SemverOptionsError> {
        if baseline_selection != candidate_selection {
            return Err(SemverOptionsError::DivergentSelection);
        }
        if timeout_seconds == 0 {
            timeout_seconds = SEMVER_DEFAULT_TIMEOUT_SECONDS;
        }
        if timeout_seconds > SEMVER_MAX_TIMEOUT_SECONDS {
            return Err(SemverOptionsError::InvalidTimeout);
        }
        Ok(Self {
            baseline_selection,
            candidate_selection,
            timeout_seconds,
        })
    }

    pub fn selection(&self) -> &SemverCommandOptions {
        &self.candidate_selection
    }
    pub fn baseline_selection(&self) -> &SemverCommandOptions {
        &self.baseline_selection
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemverOptionsError {
    DivergentSelection,
    InvalidTimeout,
}
impl std::fmt::Display for SemverOptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DivergentSelection => "baseline and candidate selections differ",
            Self::InvalidTimeout => "invalid semver timeout",
        })
    }
}
impl std::error::Error for SemverOptionsError {}

#[derive(Clone, Debug)]
pub struct SemverObservation {
    pub options: SemverOptions,
    pub exit: SemverExit,
    pub counts: SemverFindingCounts,
    pub findings: Vec<SemverFinding>,
    pub findings_omitted: u64,
    pub completeness: SemverFindingCompleteness,
    pub termination: ExecutionTermination,
    pub exit_code: Option<i32>,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

impl SemverObservation {
    pub fn validate(&self) -> Result<(), InspectionError> {
        if self.findings.len() > SEMVER_MAX_FINDINGS
            || self.runtime.execution_fingerprint != self.execution_fingerprint
            || self.runtime.platform.is_empty()
            || self.runtime.image_id.len() != 71
            || !self.runtime.image_id.starts_with("sha256:")
            || self.runtime.rust_version.is_empty()
            || self.runtime.cargo_version.is_empty()
        {
            return Err(InspectionError::InvalidMetadata);
        }
        Ok(())
    }

    pub fn raw_output_bytes(&self) -> Vec<u8> {
        let mut raw = Vec::with_capacity(self.stdout.len() + self.stderr.len() + 64);
        raw.extend_from_slice(b"=== stdout ===\n");
        raw.extend_from_slice(&self.stdout);
        raw.extend_from_slice(b"\n=== stderr ===\n");
        raw.extend_from_slice(&self.stderr);
        raw
    }
}

/// Execute cargo-semver-checks over exactly two captured byte bundles. The
/// implementation owns the fixed mounts, containment and joined cleanup.
pub trait ProjectSemverPort: Send + Sync {
    fn run(
        &self,
        baseline: &SourceBundle,
        candidate: &SourceBundle,
        options: &SemverOptions,
        control: &dyn InspectionControl,
    ) -> Result<SemverObservation, InspectionError>;
}

pub trait SemverDurablePublisher: Send {
    fn publish(
        &mut self,
        project: &ProjectRef,
        captured_at: UnixSeconds,
        baseline: &SourceBundle,
        candidate: &SourceBundle,
        observation: &mut SemverObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Option<SemverArtifactReference>, InspectionError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SemverArtifactReference {
    Ephemeral(ArtifactMetadata),
    Durable(Box<QualityArtifactDescriptor>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemverOutcome {
    NoBreak,
    Breaking,
    Incomplete,
    Blocked,
    Unavailable,
}

#[derive(Clone, Debug)]
pub struct SemverProjectResult {
    pub baseline_project_ref: ProjectRef,
    pub baseline_project_identity_fingerprint: ProjectIdentityFingerprint,
    pub baseline_evidence: SnapshotEvidence,
    pub candidate_project_ref: ProjectRef,
    pub candidate_project_identity_fingerprint: ProjectIdentityFingerprint,
    pub candidate_evidence: SnapshotEvidence,
    pub outcome: SemverOutcome,
    pub observation: Option<SemverObservation>,
    pub raw_output: Option<SemverArtifactReference>,
    pub raw_output_omitted: bool,
}

fn has_library(structure: &ProjectStructure, package: Option<&str>) -> bool {
    structure.packages.iter().any(|candidate| {
        package.is_none_or(|name| candidate.name == name)
            && candidate.targets.iter().any(|target| {
                target.kinds.iter().chain(&target.crate_types).any(|kind| {
                    matches!(
                        kind,
                        TargetKind::Lib
                            | TargetKind::ProcMacro
                            | TargetKind::Rlib
                            | TargetKind::Dylib
                            | TargetKind::Cdylib
                            | TargetKind::Staticlib
                    )
                })
            })
    })
}

fn evidence(
    structure: &ProjectStructure,
    created_at: rust_engineering_domain::UnixSeconds,
    clock: &impl Clock,
) -> Result<SnapshotEvidence, InspectionError> {
    let provenance = Provenance::new(
        SourceKind::ProjectSnapshot,
        structure
            .source_fingerprint
            .to_string()
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        Some(created_at),
        Some(clock.now()),
        IntegrityStatus::Verified,
        false,
    )
    .map_err(|_| InspectionError::Internal)?;
    let policy = FreshnessPolicy::new(
        "captured-semver-side-v1"
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        60,
        300,
    )
    .map_err(|_| InspectionError::Internal)?;
    Ok(SnapshotEvidence::assess(provenance, policy, clock))
}

struct Bytes<'a> {
    bytes: &'a [u8],
    truncated: bool,
}
impl ArtifactInput for Bytes<'_> {
    fn truncated(&self) -> bool {
        self.truncated
    }
    fn read(&mut self, out: &mut [u8]) -> Result<usize, ArtifactError> {
        let count = out.len().min(self.bytes.len());
        out[..count].copy_from_slice(&self.bytes[..count]);
        self.bytes = &self.bytes[count..];
        Ok(count)
    }
}

fn classify(observation: &SemverObservation) -> SemverOutcome {
    if !matches!(observation.termination, ExecutionTermination::Exited) {
        return SemverOutcome::Incomplete;
    }
    if observation.exit == SemverExit::Breaking && observation.counts.deny == 0 {
        return SemverOutcome::Blocked;
    }
    if observation.exit == SemverExit::NoBreak && observation.counts.deny != 0 {
        return SemverOutcome::Blocked;
    }
    if observation.exit == SemverExit::Uncalibrated {
        return SemverOutcome::Blocked;
    }
    if observation.completeness == SemverFindingCompleteness::Incomplete {
        return SemverOutcome::Incomplete;
    }
    match observation.exit {
        SemverExit::NoBreak => SemverOutcome::NoBreak,
        SemverExit::Breaking => SemverOutcome::Breaking,
        SemverExit::Incomplete => SemverOutcome::Incomplete,
        SemverExit::Uncalibrated => SemverOutcome::Blocked,
    }
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    /// Captures baseline then candidate and produces two independent evidence
    /// records. No atomicity across the two roots is claimed.
    #[allow(clippy::too_many_arguments)]
    pub fn semver_check(
        &mut self,
        baseline_ref: &ProjectRef,
        candidate_ref: &ProjectRef,
        options: &SemverOptions,
        inspector: &impl ProjectInspectionPort,
        runner: &impl ProjectSemverPort,
        artifacts: &mut impl ArtifactStore,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<SemverProjectResult, InspectionError> {
        self.reap_artifacts(artifacts)
            .map_err(|_| InspectionError::Internal)?;
        let baseline = self.capture_validation(baseline_ref, artifacts, clock, control)?;
        let baseline_structure = inspector.inspect(&baseline.source, control)?;
        self.resolve_inner(baseline_ref, control, false)?;
        let candidate = self.capture_validation(candidate_ref, artifacts, clock, control)?;
        let candidate_structure = inspector.inspect(&candidate.source, control)?;
        self.resolve_inner(candidate_ref, control, false)?;

        let baseline_evidence = evidence(&baseline_structure, baseline.created_at, clock)?;
        let candidate_evidence = evidence(&candidate_structure, candidate.created_at, clock)?;
        let common = |outcome, observation, raw_output, raw_output_omitted| SemverProjectResult {
            baseline_project_ref: baseline.reference.clone(),
            baseline_project_identity_fingerprint: baseline.identity.clone(),
            baseline_evidence: baseline_evidence.clone(),
            candidate_project_ref: candidate.reference.clone(),
            candidate_project_identity_fingerprint: candidate.identity.clone(),
            candidate_evidence: candidate_evidence.clone(),
            outcome,
            observation,
            raw_output,
            raw_output_omitted,
        };

        if !has_library(&baseline_structure, options.selection().package())
            || !has_library(&candidate_structure, options.selection().package())
        {
            self.resolve_inner(baseline_ref, control, false)?;
            self.resolve_inner(candidate_ref, control, true)?;
            return Ok(common(SemverOutcome::Unavailable, None, None, false));
        }

        let observation = runner.run(&baseline.source, &candidate.source, options, control)?;
        observation.validate()?;
        let outcome = classify(&observation);
        self.resolve_inner(baseline_ref, control, false)?;
        self.resolve_inner(candidate_ref, control, false)?;
        control.check()?;

        let raw = observation.raw_output_bytes();
        let producer_truncated = observation.stdout_truncated || observation.stderr_truncated;
        let (raw_output, raw_output_omitted) = match artifacts.capture(
            candidate_ref,
            &mut Bytes {
                bytes: &raw,
                truncated: producer_truncated,
            },
        ) {
            Ok(metadata) => (Some(SemverArtifactReference::Ephemeral(metadata)), false),
            Err(ArtifactError::QuotaExceeded) => (None, true),
            Err(_) => return Err(InspectionError::Internal),
        };
        let final_authorization = self
            .resolve_inner(baseline_ref, control, false)
            .and_then(|_| self.resolve_inner(candidate_ref, control, true));
        if let Err(error) = final_authorization {
            if let Some(SemverArtifactReference::Ephemeral(metadata)) = &raw_output {
                artifacts
                    .remove(candidate_ref, &metadata.id)
                    .map_err(|_| InspectionError::Internal)?;
            }
            return Err(error.into());
        }
        Ok(common(
            outcome,
            Some(observation),
            raw_output,
            raw_output_omitted,
        ))
    }
}

impl<B, G, C> ProjectRegistry<B, G, C>
where
    B: ProjectSourceBackend + QualityProjectBackend,
    G: ReferenceGenerator,
    C: RegistryClock,
{
    /// Stage 1 variant. The durable artifact is owned by the candidate root,
    /// while the publisher receives both captures and revalidates both roots
    /// at each authority checkpoint. The descriptor's source digest is a
    /// domain-separated digest of the ordered baseline/candidate pair.
    #[allow(clippy::too_many_arguments)]
    pub fn semver_check_durable(
        &mut self,
        baseline_ref: &ProjectRef,
        candidate_ref: &ProjectRef,
        options: &SemverOptions,
        inspector: &impl ProjectInspectionPort,
        runner: &impl ProjectSemverPort,
        stage0: &mut impl ArtifactStore,
        publisher: &mut dyn SemverDurablePublisher,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<SemverProjectResult, InspectionError> {
        self.reap_artifacts(stage0)
            .map_err(|_| InspectionError::Internal)?;
        let baseline = self.capture_validation(baseline_ref, stage0, clock, control)?;
        let baseline_structure = inspector.inspect(&baseline.source, control)?;
        self.resolve_inner(baseline_ref, control, false)?;
        let candidate = self.capture_validation(candidate_ref, stage0, clock, control)?;
        let candidate_structure = inspector.inspect(&candidate.source, control)?;
        self.resolve_inner(candidate_ref, control, false)?;
        let baseline_evidence = evidence(&baseline_structure, baseline.created_at, clock)?;
        let candidate_evidence = evidence(&candidate_structure, candidate.created_at, clock)?;

        if !has_library(&baseline_structure, options.selection().package())
            || !has_library(&candidate_structure, options.selection().package())
        {
            self.resolve_inner(baseline_ref, control, false)?;
            self.resolve_inner(candidate_ref, control, true)?;
            return Ok(SemverProjectResult {
                baseline_project_ref: baseline.reference,
                baseline_project_identity_fingerprint: baseline.identity,
                baseline_evidence,
                candidate_project_ref: candidate.reference,
                candidate_project_identity_fingerprint: candidate.identity,
                candidate_evidence,
                outcome: SemverOutcome::Unavailable,
                observation: None,
                raw_output: None,
                raw_output_omitted: false,
            });
        }

        let mut observation = runner.run(&baseline.source, &candidate.source, options, control)?;
        observation.validate()?;
        let outcome = classify(&observation);
        self.resolve_inner(baseline_ref, control, false)?;
        self.resolve_inner(candidate_ref, control, false)?;
        control.check()?;
        let mut revalidate = || {
            self.resolve_inner(baseline_ref, control, false)?;
            self.quality_owner_facts(candidate_ref, control)
                .map_err(InspectionError::from)
        };
        let raw_output = publisher.publish(
            candidate_ref,
            candidate.created_at,
            &baseline.source,
            &candidate.source,
            &mut observation,
            &mut revalidate,
        )?;
        self.resolve_inner(baseline_ref, control, false)?;
        self.resolve_inner(candidate_ref, control, true)?;
        Ok(SemverProjectResult {
            baseline_project_ref: baseline.reference,
            baseline_project_identity_fingerprint: baseline.identity,
            baseline_evidence,
            candidate_project_ref: candidate.reference,
            candidate_project_identity_fingerprint: candidate.identity,
            candidate_evidence,
            outcome,
            observation: Some(observation),
            raw_output_omitted: raw_output.is_none(),
            raw_output,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::semver_check::SemverProjectSelection;

    fn observation(
        exit: SemverExit,
        deny: u32,
        completeness: SemverFindingCompleteness,
    ) -> Result<SemverObservation, Box<dyn std::error::Error>> {
        let fingerprint: ExecutionFingerprint = format!("sha256:{}", "1".repeat(64)).parse()?;
        let selection = SemverCommandOptions::try_from(SemverProjectSelection::default())?;
        Ok(SemverObservation {
            options: SemverOptions::new(selection.clone(), selection, 60)?,
            exit,
            counts: SemverFindingCounts { deny, warn: 0 },
            findings: Vec::new(),
            findings_omitted: 0,
            completeness,
            termination: ExecutionTermination::Exited,
            exit_code: Some(match exit {
                SemverExit::NoBreak => 0,
                SemverExit::Breaking => 100,
                SemverExit::Incomplete => 101,
                SemverExit::Uncalibrated => 99,
            }),
            runtime: RuntimeIdentity {
                platform: "linux/aarch64".into(),
                image_id: format!("sha256:{}", "2".repeat(64)),
                configuration_fingerprint: fingerprint.clone(),
                execution_fingerprint: fingerprint.clone(),
                rust_version: "1.98.1".into(),
                cargo_version: "1.98.1".into(),
                declared_toolchain: None,
            },
            execution_fingerprint: fingerprint,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }

    #[test]
    fn divergent_selections_are_rejected_before_execution() -> Result<(), Box<dyn std::error::Error>>
    {
        let baseline = SemverCommandOptions::try_from(SemverProjectSelection::default())?;
        let candidate = SemverCommandOptions::try_from(SemverProjectSelection {
            features: vec!["extra".into()],
            ..Default::default()
        })?;
        assert_eq!(
            SemverOptions::new(baseline, candidate, 60),
            Err(SemverOptionsError::DivergentSelection)
        );
        Ok(())
    }

    #[test]
    fn timeout_defaults_and_is_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let selection = SemverCommandOptions::try_from(SemverProjectSelection::default())?;
        assert_eq!(
            SemverOptions::new(selection.clone(), selection.clone(), 0)?.timeout_seconds(),
            SEMVER_DEFAULT_TIMEOUT_SECONDS
        );
        assert_eq!(
            SemverOptions::new(selection.clone(), selection, SEMVER_MAX_TIMEOUT_SECONDS + 1),
            Err(SemverOptionsError::InvalidTimeout)
        );
        Ok(())
    }

    #[test]
    fn parser_uncertainty_and_breaking_without_a_deny_row_fail_closed()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            classify(&observation(
                SemverExit::NoBreak,
                0,
                SemverFindingCompleteness::Incomplete
            )?),
            SemverOutcome::Incomplete
        );
        assert_eq!(
            classify(&observation(
                SemverExit::Breaking,
                0,
                SemverFindingCompleteness::Incomplete
            )?),
            SemverOutcome::Blocked
        );
        assert_eq!(
            classify(&observation(
                SemverExit::Uncalibrated,
                0,
                SemverFindingCompleteness::Incomplete
            )?),
            SemverOutcome::Blocked
        );
        assert_eq!(
            classify(&observation(
                SemverExit::Breaking,
                1,
                SemverFindingCompleteness::Partial
            )?),
            SemverOutcome::Breaking
        );
        assert_eq!(
            classify(&observation(
                SemverExit::NoBreak,
                1,
                SemverFindingCompleteness::Partial
            )?),
            SemverOutcome::Blocked
        );
        Ok(())
    }
}
