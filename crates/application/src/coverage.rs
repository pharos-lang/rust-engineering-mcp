//! Transport-neutral application contract for the ADR-062 coverage run.

use crate::{
    ArtifactStore, InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend,
    QualityOwnerFacts, QualityProjectBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    ArtifactMetadata, ProjectRef, QualityArtifactDescriptor, UnixSeconds,
};
use rust_engineering_domain::{
    ExecutionFingerprint, ExecutionTermination, RuntimeIdentity, SourceBundle,
    coverage::{CoverageFile, CoverageMetrics, CoverageOptions, CoveragePackage, CoverageSummary},
};

pub const COVERAGE_FILE_PAGE_ROWS: usize = 128;
pub const COVERAGE_ARTIFACT_MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoverageArtifactStreams {
    pub json: Vec<u8>,
    pub lcov: Vec<u8>,
    /// A canonical, validated USTAR bundle; never an HTML preview.
    pub html_bundle: Vec<u8>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub json_truncated: bool,
    pub lcov_truncated: bool,
    pub html_truncated: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoverageIdentity {
    pub cargo_llvm_cov_version: String,
    pub manifest_path: String,
    pub llvm_tools_version: String,
}

#[derive(Clone, Debug)]
pub struct CoverageObservation {
    pub options: CoverageOptions,
    pub summary: CoverageSummary,
    pub identity: CoverageIdentity,
    pub doctests_run: bool,
    pub cfg_coverage_enabled: bool,
    pub target: &'static str,
    pub termination: ExecutionTermination,
    pub exit_code: Option<i32>,
    pub parse_complete: bool,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub artifacts: CoverageArtifactStreams,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageArtifactKind {
    Json,
    Lcov,
    ArchiveBundle,
    StdoutLog,
    StderrLog,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoverageArtifactReference {
    Ephemeral {
        kind: CoverageArtifactKind,
        metadata: ArtifactMetadata,
    },
    Durable(Box<QualityArtifactDescriptor>),
}

/// Registry-safe terminal value: byte streams are intentionally discarded once
/// Stage 0/1 publication has produced references.
#[derive(Clone, Debug)]
pub struct CoverageTaskResult {
    observation: CoverageObservation,
    artifacts: Vec<CoverageArtifactReference>,
    duration_ms: u64,
}
impl CoverageTaskResult {
    pub fn new(
        mut observation: CoverageObservation,
        artifacts: Vec<CoverageArtifactReference>,
        duration_ms: u64,
    ) -> Result<Self, InspectionError> {
        observation.validate()?;
        if artifacts.len() > 128 || duration_ms > 3_840_000 {
            return Err(InspectionError::InvalidMetadata);
        }
        observation.artifacts.json.clear();
        observation.artifacts.lcov.clear();
        observation.artifacts.html_bundle.clear();
        observation.artifacts.stdout.clear();
        observation.artifacts.stderr.clear();
        Ok(Self {
            observation,
            artifacts,
            duration_ms,
        })
    }
    pub fn into_parts(self) -> (CoverageObservation, Vec<CoverageArtifactReference>, u64) {
        (self.observation, self.artifacts, self.duration_ms)
    }
}

impl CoverageObservation {
    pub fn bounded_files(&self) -> (&[CoverageFile], bool) {
        let omitted =
            self.summary.files.len() > COVERAGE_FILE_PAGE_ROWS || self.summary.files_omitted != 0;
        (
            &self.summary.files[..self.summary.files.len().min(COVERAGE_FILE_PAGE_ROWS)],
            omitted,
        )
    }

    pub fn package_metrics(&self) -> &[CoveragePackage] {
        &self.summary.packages
    }
    pub fn aggregate_metrics(&self) -> &CoverageMetrics {
        &self.summary.aggregate
    }

    pub fn validate(&self) -> Result<(), InspectionError> {
        if self.doctests_run
            || !self.cfg_coverage_enabled
            || self.target != "aarch64-unknown-linux-gnu"
            || self.identity.cargo_llvm_cov_version.is_empty()
            || self.identity.cargo_llvm_cov_version.len() > 128
            || self.identity.manifest_path.is_empty()
            || self.identity.manifest_path.len() > 4096
            || self.summary.packages.len() > 1024
            || self.summary.files.len() > 16_384
        {
            return Err(InspectionError::InvalidMetadata);
        }
        Ok(())
    }
}

/// The only application boundary for an instrumented coverage invocation.
/// Implementations own the closed two-phase gateway and bounded report egress.
pub trait ProjectCoveragePort: Send + Sync {
    fn run(
        &self,
        source: &SourceBundle,
        options: &CoverageOptions,
        control: &dyn InspectionControl,
    ) -> Result<CoverageObservation, InspectionError>;
}

pub trait CoverageDurablePublisher: Send {
    fn publish(
        &mut self,
        project: &ProjectRef,
        captured_at: UnixSeconds,
        source: &SourceBundle,
        observation: &mut CoverageObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<CoverageArtifactReference>, InspectionError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn coverage(
        &mut self,
        reference: &ProjectRef,
        options: &CoverageOptions,
        runner: &impl ProjectCoveragePort,
        artifacts: &mut impl ArtifactStore,
        clock: &impl rust_engineering_domain::Clock,
        control: &dyn InspectionControl,
    ) -> Result<(CoverageObservation, Vec<CoverageArtifactReference>), InspectionError> {
        let captured = self.capture_validation(reference, artifacts, clock, control)?;
        let mut observation = runner.run(&captured.source, options, control)?;
        let published =
            self.publish_coverage_stage0(captured, &mut observation, artifacts, control)?;
        Ok((observation, published))
    }
}
impl<B, G, C> ProjectRegistry<B, G, C>
where
    B: ProjectSourceBackend + QualityProjectBackend,
    G: ReferenceGenerator,
    C: RegistryClock,
{
    #[allow(clippy::too_many_arguments)] // One explicit port per authority/publication boundary.
    pub fn coverage_durable(
        &mut self,
        reference: &ProjectRef,
        options: &CoverageOptions,
        runner: &impl ProjectCoveragePort,
        stage0: &mut impl ArtifactStore,
        publisher: &mut dyn CoverageDurablePublisher,
        clock: &impl rust_engineering_domain::Clock,
        control: &dyn InspectionControl,
    ) -> Result<(CoverageObservation, Vec<CoverageArtifactReference>), InspectionError> {
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
