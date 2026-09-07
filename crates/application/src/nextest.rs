//! Application boundary for one captured cargo-nextest run.

use crate::{
    ArtifactStore, ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts, QualityProjectBackend,
    ReferenceGenerator, RegistryClock,
};
use crate::{InspectionControl, InspectionError};
use rust_engineering_domain::{
    ArtifactMetadata, CheckOptions, CheckSelection, ExecutionFingerprint, ExecutionTermination,
    InvalidCheckOptions, ProjectRef, QualityArtifactDescriptor, RuntimeIdentity, SourceBundle,
    UnixSeconds,
};
use std::{error::Error, fmt};

pub const NEXTEST_DEFAULT_TIMEOUT_SECONDS: u64 = 300;
pub const NEXTEST_MAX_TIMEOUT_SECONDS: u64 = 3_600;
pub const NEXTEST_MAX_TEST_ROWS: usize = 128;
pub const NEXTEST_PROFILE: &str = "rust-mcp";

#[derive(Clone, Debug, Default)]
pub struct NextestSelection {
    pub package: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub target: Option<String>,
    pub test_filter: Option<String>,
    pub timeout_seconds: u64,
    pub retries: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextestOptions {
    package: Option<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    target: Option<String>,
    test_filter: Option<String>,
    timeout_seconds: u64,
    retries: u8,
}

impl TryFrom<NextestSelection> for NextestOptions {
    type Error = InvalidCheckOptions;

    fn try_from(mut selection: NextestSelection) -> Result<Self, Self::Error> {
        if selection.timeout_seconds == 0 {
            selection.timeout_seconds = NEXTEST_DEFAULT_TIMEOUT_SECONDS;
        }
        if selection.timeout_seconds > NEXTEST_MAX_TIMEOUT_SECONDS
            || selection.retries > 2
            || selection.test_filter.as_ref().is_some_and(|filter| {
                filter.is_empty()
                    || filter.len() > 128
                    || !filter
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_:".contains(&byte))
                    || !filter
                        .as_bytes()
                        .first()
                        .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            })
        {
            return Err(InvalidCheckOptions);
        }
        let cargo = CheckOptions::try_from(CheckSelection {
            package: selection.package,
            features: selection.features,
            all_features: selection.all_features,
            no_default_features: selection.no_default_features,
            target: selection.target,
            ..Default::default()
        })?;
        Ok(Self {
            package: cargo.package().map(str::to_owned),
            features: cargo.features().to_vec(),
            all_features: cargo.all_features(),
            no_default_features: cargo.no_default_features(),
            target: cargo.target().map(str::to_owned),
            test_filter: selection.test_filter,
            timeout_seconds: selection.timeout_seconds,
            retries: selection.retries,
        })
    }
}

impl NextestOptions {
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }

    pub fn features(&self) -> &[String] {
        &self.features
    }

    pub fn all_features(&self) -> bool {
        self.all_features
    }

    pub fn no_default_features(&self) -> bool {
        self.no_default_features
    }

    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    pub fn test_filter(&self) -> Option<&str> {
        self.test_filter.as_deref()
    }

    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }

    pub fn retries(&self) -> u8 {
        self.retries
    }

    pub fn profile(&self) -> &'static str {
        NEXTEST_PROFILE
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NextestCounts {
    pub selected: u64,
    pub passed: u64,
    pub failed: u64,
    pub ignored: u64,
    pub retried: u64,
    pub flaky: u64,
    pub leaked: u64,
    pub timed_out: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextestTestStatus {
    Passed,
    Failed,
    Ignored,
    Flaky,
    Leaked,
    TimedOut,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NextestTestRow {
    pub test_id: String,
    pub status: NextestTestStatus,
    pub attempts: u16,
    pub duration_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextestCompleteness {
    Complete,
    Partial,
    Invalid,
    Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArtifactStreams {
    pub junit_xml: Vec<u8>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub junit_truncated: bool,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextestArtifactKind {
    JunitXml,
    StdoutLog,
    StderrLog,
}

/// Publication result from either ADR-061 Stage 0 or the durable Stage 1 port.
/// Locators remain typed here; URI construction stays at the MCP boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NextestArtifactReference {
    Ephemeral {
        kind: NextestArtifactKind,
        metadata: ArtifactMetadata,
    },
    Durable(Box<QualityArtifactDescriptor>),
    EphemeralUnavailable {
        kind: NextestArtifactKind,
        metadata: ArtifactMetadata,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextestTaskResultError {
    InvalidObservation,
    InvalidDuration,
    TooManyArtifacts,
}

impl fmt::Display for NextestTaskResultError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidObservation => "invalid nextest observation",
            Self::InvalidDuration => "invalid nextest duration",
            Self::TooManyArtifacts => "too many nextest artifacts",
        })
    }
}

impl Error for NextestTaskResultError {}

/// Registry-safe terminal value. Raw streams are deliberately erased after
/// publication so the task record contains references and bounded facts only.
#[derive(Clone, Debug)]
pub struct NextestTaskResult {
    observation: NextestObservation,
    artifacts: Vec<NextestArtifactReference>,
    expected_artifacts: u8,
    duration_ms: u64,
}

impl NextestTaskResult {
    pub fn artifacts(&self) -> &[NextestArtifactReference] {
        &self.artifacts
    }
    pub fn new(
        mut observation: NextestObservation,
        artifacts: Vec<NextestArtifactReference>,
        duration_ms: u64,
    ) -> Result<Self, NextestTaskResultError> {
        observation
            .validate()
            .map_err(|_| NextestTaskResultError::InvalidObservation)?;
        if artifacts.len() > 128 {
            return Err(NextestTaskResultError::TooManyArtifacts);
        }
        if duration_ms > 3_840_000 {
            return Err(NextestTaskResultError::InvalidDuration);
        }
        let expected_artifacts = u8::from(!observation.artifacts.junit_xml.is_empty())
            + u8::from(!observation.artifacts.stdout.is_empty())
            + u8::from(!observation.artifacts.stderr.is_empty());
        observation.artifacts.junit_xml.clear();
        observation.artifacts.stdout.clear();
        observation.artifacts.stderr.clear();
        Ok(Self {
            observation,
            artifacts,
            expected_artifacts,
            duration_ms,
        })
    }

    pub fn into_parts(self) -> (NextestObservation, Vec<NextestArtifactReference>, u8, u64) {
        (
            self.observation,
            self.artifacts,
            self.expected_artifacts,
            self.duration_ms,
        )
    }

    pub fn replace_artifacts(
        mut self,
        artifacts: Vec<NextestArtifactReference>,
    ) -> Result<Self, NextestTaskResultError> {
        if artifacts.len() > 128 {
            return Err(NextestTaskResultError::TooManyArtifacts);
        }
        self.artifacts = artifacts;
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub struct NextestObservation {
    pub options: NextestOptions,
    pub validation_complete: bool,
    pub completeness: NextestCompleteness,
    pub counts: NextestCounts,
    pub tests: Vec<NextestTestRow>,
    pub tests_omitted: u64,
    pub doctests_run: bool,
    pub termination: ExecutionTermination,
    pub exit_code: Option<i32>,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub artifacts: ArtifactStreams,
}

impl NextestObservation {
    pub fn validate(&self) -> Result<(), InspectionError> {
        let accounted = self
            .counts
            .passed
            .checked_add(self.counts.failed)
            .and_then(|value| value.checked_add(self.counts.ignored))
            .and_then(|value| value.checked_add(self.counts.flaky))
            .and_then(|value| value.checked_add(self.counts.leaked))
            .and_then(|value| value.checked_add(self.counts.timed_out))
            .ok_or(InspectionError::OutputLimit)?;
        if self.tests.len() > NEXTEST_MAX_TEST_ROWS
            || self
                .tests
                .iter()
                .any(|row| row.test_id.is_empty() || row.test_id.len() > 256 || row.attempts == 0)
            || accounted > self.counts.selected
            || self.counts.flaky > self.counts.retried
            || self.doctests_run
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
            || (self.validation_complete
                && (self.completeness != NextestCompleteness::Complete
                    || accounted != self.counts.selected
                    || self.tests_omitted != 0
                    || self.artifacts.junit_truncated
                    || self.artifacts.stdout_truncated
                    || self.artifacts.stderr_truncated))
        {
            return Err(InspectionError::InvalidMetadata);
        }
        Ok(())
    }
}

/// Execute cargo-nextest over the supplied captured `SourceBundle` only.
///
/// Implementations must use the single closed Execution Gateway, fixed product
/// profile `rust-mcp`, network-denied containment and joined cancellation. Counts
/// come only from bounded machine-readable evidence. The returned JUnit/stdout/
/// stderr bytes are untrusted bounded artifact streams; this port neither opens
/// host paths nor publishes artifacts. A missing/mismatched plugin is
/// `ExecutionError::Unavailable`, parse uncertainty is incomplete, and no
/// partial/unavailable/skip observation may be promoted to a pass.
pub trait ProjectNextestPort: Send + Sync {
    fn run(
        &self,
        source: &SourceBundle,
        options: &NextestOptions,
        control: &dyn InspectionControl,
    ) -> Result<NextestObservation, InspectionError>;
}

/// Durable publication boundary. The application supplies a live revalidation
/// callback which does not renew the project lease; adapters never derive
/// authority from a resource URI or a stored descriptor.
pub trait NextestDurablePublisher: Send {
    fn publish(
        &mut self,
        project: &ProjectRef,
        captured_at: UnixSeconds,
        source: &SourceBundle,
        observation: &mut NextestObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<NextestArtifactReference>, InspectionError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    /// Capture, execute and publish ADR-061 Stage-0 evidence under one live
    /// ProjectRef authorization boundary. Durable Stage 1 remains a separate
    /// integration point through `NextestArtifactReference::Durable`.
    pub fn nextest(
        &mut self,
        reference: &rust_engineering_domain::ProjectRef,
        options: &NextestOptions,
        runner: &impl ProjectNextestPort,
        artifacts: &mut impl ArtifactStore,
        clock: &impl rust_engineering_domain::Clock,
        control: &dyn InspectionControl,
    ) -> Result<(NextestObservation, Vec<NextestArtifactReference>), InspectionError> {
        let captured = self.capture_validation(reference, artifacts, clock, control)?;
        let mut observation = runner.run(&captured.source, options, control)?;
        let published =
            self.publish_nextest_stage0(captured, &mut observation, artifacts, control)?;
        Ok((observation, published))
    }
}

impl<B, G, C> ProjectRegistry<B, G, C>
where
    B: ProjectSourceBackend + QualityProjectBackend,
    G: ReferenceGenerator,
    C: RegistryClock,
{
    /// Capture and execute as usual, but commit artifacts through ADR-061's
    /// durable publisher. The live grant is checked immediately before every
    /// member stream and once more at the publication commit point.
    #[allow(clippy::too_many_arguments)] // One explicit port per authority/publication boundary.
    pub fn nextest_durable(
        &mut self,
        reference: &ProjectRef,
        options: &NextestOptions,
        runner: &impl ProjectNextestPort,
        stage0: &mut impl ArtifactStore,
        publisher: &mut dyn NextestDurablePublisher,
        clock: &impl rust_engineering_domain::Clock,
        control: &dyn InspectionControl,
    ) -> Result<(NextestObservation, Vec<NextestArtifactReference>), InspectionError> {
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
