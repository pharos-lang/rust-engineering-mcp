//! Application boundary for one `rust.benchmark.run` execution (M5-01).
//!
//! The shape is exactly [`crate::miri`]'s: capture one source generation,
//! execute through a port, revalidate the observation, publish, then re-check
//! project identity. The difference is what "revalidate" means here. A
//! benchmark dataset is evidence a later comparison will quote verbatim, so the
//! adapter does not get to tell us a dataset describes a run it did not
//! describe: the selection, the harness version, the vendor tree and the
//! execution identity are all re-derived from what *we* asked for and compared
//! against what the dataset claims, before a byte is published.
use crate::security::{SecurityCapture, SecurityError};
use crate::{
    InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts,
    QualityProjectBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::benchmark::{APPROVED_CRITERION_VERSION, BenchmarkSelection};
use rust_engineering_domain::benchmark_run::HarnessDetection;
use rust_engineering_domain::{
    CargoVendorSnapshot, Clock, ProjectRef, QualityArtifactDescriptor, SourceBundle,
};

pub use rust_engineering_domain::benchmark_run::BenchmarkObservation;

/// The only build profile a benchmark run selects (ADR-073 §2).
pub const BENCHMARK_PROFILE: &str = "bench";
/// Ceiling on peer-supplied features. Sixteen is a bound, not a capability.
pub const BENCHMARK_MAX_FEATURES: usize = 16;
pub const BENCHMARK_MIN_RUN_COUNT: u8 = 1;
pub const BENCHMARK_MAX_RUN_COUNT: u8 = 3;
pub const BENCHMARK_DEFAULT_RUN_COUNT: u8 = 3;
pub const BENCHMARK_MIN_TIMEOUT_SECONDS: u64 = 1;
/// ADR-076 §7 and spec §44: the whole `benchmark` budget.
pub const BENCHMARK_MAX_TIMEOUT_SECONDS: u64 = 900;
pub const BENCHMARK_DEFAULT_TIMEOUT_SECONDS: u64 = 900;

/// Every reason a benchmark request is refused before anything runs. Closed: a
/// new rejection is a deliberate contract change, never an opaque string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchmarkOptionsError {
    InvalidPackage,
    InvalidBenchTarget,
    InvalidFeature,
    TooManyFeatures,
    InvalidRunCount,
    InvalidTimeout,
}
impl std::fmt::Display for BenchmarkOptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPackage => "benchmark package name is not a bounded cargo name",
            Self::InvalidBenchTarget => "benchmark target name is not a bounded cargo name",
            Self::InvalidFeature => "benchmark feature name is not a bounded cargo name",
            Self::TooManyFeatures => "benchmark request exceeds the feature ceiling",
            Self::InvalidRunCount => "benchmark run count is outside its closed range",
            Self::InvalidTimeout => "benchmark timeout is outside its closed range",
        })
    }
}
impl std::error::Error for BenchmarkOptionsError {}

/// `^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}$`. A path, an argument and a flag all fail
/// it — the leading `-` is excluded on purpose, because an alphabet that admits
/// `-noplot` admits something that reads as an option wherever this name is
/// later placed on an argv. A hyphen inside the name stays legal: that is an
/// ordinary cargo target.
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// A validated benchmark request.
///
/// Fields are private because two of them carry an invariant the type owns:
/// `features` is stored already sorted and deduplicated, and `profile` is not
/// selectable at all. ADR-073 §5 compares [`BenchmarkSelection`] verbatim
/// between two datasets, so the normalization happens once, here, rather than
/// in each producer that might disagree about ordering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchmarkRunOptions {
    package: Option<String>,
    bench_target: Option<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    run_count: u8,
    timeout_seconds: u64,
}

impl BenchmarkRunOptions {
    /// `run_count` and `timeout_seconds` accept `0` as "unspecified" and take
    /// the frozen default, exactly as [`crate::semver_check::SemverOptions`]
    /// does; any other out-of-range value is rejected rather than clamped.
    pub fn new(
        package: Option<String>,
        bench_target: Option<String>,
        features: Vec<String>,
        all_features: bool,
        no_default_features: bool,
        run_count: u8,
        timeout_seconds: u64,
    ) -> Result<Self, BenchmarkOptionsError> {
        if package.as_deref().is_some_and(|name| !valid_name(name)) {
            return Err(BenchmarkOptionsError::InvalidPackage);
        }
        if bench_target
            .as_deref()
            .is_some_and(|name| !valid_name(name))
        {
            return Err(BenchmarkOptionsError::InvalidBenchTarget);
        }
        if features.len() > BENCHMARK_MAX_FEATURES {
            return Err(BenchmarkOptionsError::TooManyFeatures);
        }
        if features.iter().any(|feature| !valid_name(feature)) {
            return Err(BenchmarkOptionsError::InvalidFeature);
        }
        let run_count = if run_count == 0 {
            BENCHMARK_DEFAULT_RUN_COUNT
        } else {
            run_count
        };
        if !(BENCHMARK_MIN_RUN_COUNT..=BENCHMARK_MAX_RUN_COUNT).contains(&run_count) {
            return Err(BenchmarkOptionsError::InvalidRunCount);
        }
        let timeout_seconds = if timeout_seconds == 0 {
            BENCHMARK_DEFAULT_TIMEOUT_SECONDS
        } else {
            timeout_seconds
        };
        if !(BENCHMARK_MIN_TIMEOUT_SECONDS..=BENCHMARK_MAX_TIMEOUT_SECONDS)
            .contains(&timeout_seconds)
        {
            return Err(BenchmarkOptionsError::InvalidTimeout);
        }
        // Deduplication cannot reintroduce a rejected name, so it happens after
        // validation and the stored order is the compared order.
        let mut features = features;
        features.sort();
        features.dedup();
        Ok(Self {
            package,
            bench_target,
            features,
            all_features,
            no_default_features,
            run_count,
            timeout_seconds,
        })
    }

    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
    pub fn bench_target(&self) -> Option<&str> {
        self.bench_target.as_deref()
    }
    /// Already sorted and deduplicated by construction.
    pub fn features(&self) -> &[String] {
        &self.features
    }
    pub fn all_features(&self) -> bool {
        self.all_features
    }
    pub fn no_default_features(&self) -> bool {
        self.no_default_features
    }
    pub fn run_count(&self) -> u8 {
        self.run_count
    }
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }

    /// The domain selection this request must produce. It is a projection of
    /// already-normalized state, so calling it twice cannot disagree.
    pub fn selection(&self) -> BenchmarkSelection {
        BenchmarkSelection {
            package: self.package.clone(),
            bench_target: self.bench_target.clone(),
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            profile: BENCHMARK_PROFILE.to_owned(),
        }
    }
}

/// Runs `cargo bench` over one owned source generation and one approved vendor
/// tree. The implementation owns the containment, the frozen harness argv and
/// the criterion output parsing; it never receives a host path from here.
pub trait ProjectBenchmarkPort: Send + Sync {
    fn benchmark(
        &self,
        source: &SourceBundle,
        vendor: &CargoVendorSnapshot,
        options: &BenchmarkRunOptions,
        control: &dyn InspectionControl,
    ) -> Result<BenchmarkObservation, SecurityError>;
}

/// Publishes the dataset and the criterion output tree as durable artifacts.
///
/// Publication receives owned evidence and must revalidate the owner before
/// committing any descriptor. Order is the producer's; the returned descriptors
/// are whatever it actually committed, and an empty vector is a legitimate
/// answer for a run that produced no dataset.
pub trait BenchmarkPublisher: Send {
    fn publish_benchmark(
        &mut self,
        capture: &SecurityCapture,
        observation: &BenchmarkObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError>;
}

pub struct BenchmarkPorts<'a, E, P> {
    pub executor: &'a E,
    pub publisher: &'a mut P,
}

pub struct PublishedBenchmark {
    pub observation: BenchmarkObservation,
    pub artifacts: Vec<QualityArtifactDescriptor>,
}

/// Everything that must hold before an observation may be published.
///
/// Split out of [`ProjectRegistry::benchmark_durable`] so the rule set is
/// testable without a registry, and so each clause has exactly one site.
pub fn validate_benchmark_observation(
    observation: &BenchmarkObservation,
    options: &BenchmarkRunOptions,
    vendor: &CargoVendorSnapshot,
) -> Result<(), SecurityError> {
    let selection = options.selection();
    if !observation.consistent()
        || observation.selection != selection
        || observation.runs_requested != options.run_count()
        || observation.runs_completed > observation.runs_requested
        || observation.vendor_fingerprint != vendor.tree_fingerprint
        || observation.runtime.execution_fingerprint != observation.execution_fingerprint
    {
        return Err(SecurityError::InvalidMetadata);
    }
    // ADR-076 §3 publishes the harness output tree BESIDE the dataset, never
    // instead of it. A `criterion_archive` on its own would be bytes this
    // server never turned into a measurement, committed where the artifact pair
    // means one. The observation's own bounds cannot decide this — an export
    // without a measurement is a coherent thing to have observed — so the
    // publication precondition is checked here, against the adapter whose claim
    // it is, exactly like `analysis_build_symbols_forced` in the bloat path.
    if observation.archive.is_some() && observation.dataset.is_none() {
        return Err(SecurityError::InvalidMetadata);
    }
    let Some(dataset) = &observation.dataset else {
        return Ok(());
    };
    // `HarnessDetection::Criterion` is documented as the approved harness AT the
    // approved version; ADR-073 §1 makes a different version `unavailable`, not
    // a degraded measurement, so the variant's own claim is checked here.
    let HarnessDetection::Criterion { version } = &observation.harness else {
        return Err(SecurityError::InvalidMetadata);
    };
    if version != APPROVED_CRITERION_VERSION {
        return Err(SecurityError::InvalidMetadata);
    }
    dataset
        .validate()
        .map_err(|_| SecurityError::InvalidMetadata)?;
    let provenance = dataset.provenance();
    if provenance.selection != selection
        || provenance.harness_version != *version
        || provenance.run_count != options.run_count()
        || provenance.execution_fingerprint != observation.execution_fingerprint.as_str()
    {
        return Err(SecurityError::InvalidMetadata);
    }
    Ok(())
}

impl<B: ProjectSourceBackend + QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock>
    ProjectRegistry<B, G, C>
{
    pub fn benchmark_durable(
        &mut self,
        reference: &ProjectRef,
        vendor: &CargoVendorSnapshot,
        options: &BenchmarkRunOptions,
        ports: BenchmarkPorts<'_, impl ProjectBenchmarkPort, impl BenchmarkPublisher>,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<PublishedBenchmark, SecurityError> {
        let capture = self.capture_security(reference, clock, control)?;
        let observation = ports
            .executor
            .benchmark(&capture.source, vendor, options, control)?;
        control.check()?;
        validate_benchmark_observation(&observation, options, vendor)?;
        let mut revalidate = || {
            self.quality_owner_facts(reference, control)
                .map_err(InspectionError::from)
        };
        let artifacts =
            ports
                .publisher
                .publish_benchmark(&capture, &observation, &mut revalidate)?;
        control.check()?;
        if self.resolve_inner(reference, control, true)?.fingerprint
            != capture.project_identity_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        Ok(PublishedBenchmark {
            observation,
            artifacts,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
pub(crate) mod tests {
    //! The registry doubles below are the ones `crates/application/tests/security.rs`
    //! uses; the other three M5 modules' test modules import them from here
    //! rather than restating four identical copies.
    use super::*;
    use crate::{
        ExecutionCancellation, OperationControl, ProjectBackend, ProjectError, ProjectIdentity,
        ValidatedProject,
    };
    use rust_engineering_domain::benchmark::{
        BenchmarkDataset, BenchmarkHarness, BenchmarkIdentity, BenchmarkMeasurement,
        BenchmarkProvenance, HardwareProfile, MeasurementCompleteness, RawSample, ResourceQuotas,
        SampleUnit, SamplingMode, Virtualization,
    };
    use rust_engineering_domain::benchmark_run::{
        BenchmarkExit, CriterionArchive, DatasetOmission,
    };
    use rust_engineering_domain::{
        ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection,
        ArtifactSensitivity, ArtifactSource, CargoVendorPackage, ExecutionFingerprint,
        ExecutionTermination, GuestArtifactName, PayloadFormatVersion, PluginIdentity,
        ProjectIdentityFingerprint, QualityArtifactDraft, QualityArtifactId, QualityArtifactKind,
        QualityJobId, QualityMimeType, RuntimeIdentity, SourceFile, SourceFingerprint, UnixSeconds,
        UtcInstant,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    // -- shared value helpers ------------------------------------------------

    pub(crate) fn source_fingerprint(value: u8) -> SourceFingerprint {
        format!("sha256:{value:064x}").parse().unwrap()
    }
    pub(crate) fn execution_fingerprint(value: u8) -> ExecutionFingerprint {
        format!("sha256:{value:064x}").parse().unwrap()
    }
    pub(crate) fn identity_fingerprint(value: u8) -> ProjectIdentityFingerprint {
        format!("sha256:{value:064x}").parse().unwrap()
    }
    pub(crate) fn source() -> SourceBundle {
        SourceBundle::new(vec![
            SourceFile::new(
                "Cargo.toml".into(),
                b"[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n".to_vec(),
            )
            .unwrap(),
            SourceFile::new(
                "src/lib.rs".into(),
                b"pub fn answer() -> u8 { 42 }\n".to_vec(),
            )
            .unwrap(),
        ])
        .unwrap()
    }
    pub(crate) fn vendor() -> CargoVendorSnapshot {
        CargoVendorSnapshot {
            source: SourceBundle::new(vec![
                SourceFile::new(
                    "criterion-0.8.2/Cargo.toml".into(),
                    b"[package]\nname = \"criterion\"\nversion = \"0.8.2\"\n".to_vec(),
                )
                .unwrap(),
            ])
            .unwrap(),
            tree_fingerprint: source_fingerprint(21),
            packages: vec![CargoVendorPackage {
                name: "criterion".into(),
                version: APPROVED_CRITERION_VERSION.into(),
                package_checksum: source_fingerprint(29),
            }],
        }
    }
    pub(crate) fn runtime(execution: u8) -> RuntimeIdentity {
        RuntimeIdentity {
            platform: "linux/arm64".into(),
            image_id: format!("sha256:{}", "c".repeat(64)),
            configuration_fingerprint: execution_fingerprint(10),
            execution_fingerprint: execution_fingerprint(execution),
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: Some("1.98.1".into()),
        }
    }
    /// The store's own kind/version/mime/guest-name pairing, restated once so a
    /// fixture cannot claim a combination the descriptor validator rejects.
    pub(crate) fn descriptor(kind: QualityArtifactKind) -> QualityArtifactDescriptor {
        let (payload_format_version, mime_type, guest_name, identity) = match kind {
            QualityArtifactKind::BenchmarkDataset => (
                PayloadFormatVersion::BenchmarkDatasetV2,
                QualityMimeType::ApplicationJson,
                GuestArtifactName::BenchmarkDataset,
                PluginIdentity::Criterion,
            ),
            QualityArtifactKind::CriterionArchive => (
                PayloadFormatVersion::UstarV1,
                QualityMimeType::ApplicationXTar,
                GuestArtifactName::CriterionArchive,
                PluginIdentity::Criterion,
            ),
            QualityArtifactKind::FlamegraphSvg => (
                PayloadFormatVersion::FlamegraphSvgV1,
                QualityMimeType::ImageSvgXml,
                GuestArtifactName::FlamegraphSvg,
                PluginIdentity::ProfileHelper,
            ),
            QualityArtifactKind::CollapsedStacks => (
                PayloadFormatVersion::CollapsedStacksV1,
                QualityMimeType::TextPlain,
                GuestArtifactName::CollapsedStacks,
                PluginIdentity::ProfileHelper,
            ),
            QualityArtifactKind::BloatJson => (
                PayloadFormatVersion::BloatJsonV1,
                QualityMimeType::ApplicationJson,
                GuestArtifactName::BloatJson,
                PluginIdentity::Bloat,
            ),
            QualityArtifactKind::JunitXml => (
                PayloadFormatVersion::JunitXmlV1,
                QualityMimeType::ApplicationJunitXml,
                GuestArtifactName::JunitXml,
                PluginIdentity::Nextest,
            ),
            // Only the kinds the M5 tests name are modelled; anything else would
            // be a fixture claiming a pairing this helper has not checked.
            _ => (
                PayloadFormatVersion::Utf8LogV1,
                QualityMimeType::TextPlain,
                GuestArtifactName::ToolLog,
                PluginIdentity::Builtin,
            ),
        };
        let created = UtcInstant::from_unix_seconds(1_788_000_000).unwrap();
        QualityArtifactDraft {
            artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
            member_index: 0,
            kind,
            mime_type,
            payload_format_version,
            completeness: ArtifactCompleteness::Complete,
            sensitivity: ArtifactSensitivity::Public,
            created_at_utc: created.clone(),
            expires_at_utc: created.checked_add_seconds(60).unwrap(),
            source: ArtifactSource {
                captured_source_sha256: [2; 32],
                guest_name,
                selection: ArtifactSelection::Workspace,
            },
            runtime: ArtifactRuntime {
                image_digest: [3; 32],
                toolchain_identity: [4; 32],
                plugin: ArtifactPlugin {
                    identity,
                    version: 1,
                    digest: [5; 32],
                },
                implementation_digest: [6; 32],
            },
        }
        .into_descriptor(
            QualityJobId::from_random_bytes([7; 16]),
            [8; 32],
            [9; 32],
            128,
        )
        .unwrap()
    }

    // -- shared registry doubles --------------------------------------------

    #[derive(Clone, Default)]
    pub(crate) struct TestClock(pub(crate) Arc<AtomicU64>);
    impl TestClock {
        pub(crate) fn at(seconds: u64) -> Self {
            Self(Arc::new(AtomicU64::new(seconds)))
        }
        pub(crate) fn set(&self, seconds: u64) {
            self.0.store(seconds, Ordering::SeqCst);
        }
    }
    impl RegistryClock for TestClock {
        fn seconds(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }
    impl Clock for TestClock {
        fn now(&self) -> UnixSeconds {
            UnixSeconds(self.0.load(Ordering::SeqCst))
        }
    }

    #[derive(Default)]
    pub(crate) struct Control(pub(crate) AtomicBool);
    impl Control {
        pub(crate) fn cancel(&self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    impl OperationControl for Control {
        fn check(&self) -> Result<(), ProjectError> {
            if self.0.load(Ordering::SeqCst) {
                Err(ProjectError::Cancelled)
            } else {
                Ok(())
            }
        }
    }
    impl ExecutionCancellation for Control {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }

    #[derive(Clone, Default)]
    pub(crate) struct Backend {
        pub(crate) captures: Arc<AtomicUsize>,
        pub(crate) owner_validations: Arc<AtomicUsize>,
        pub(crate) revoked: Arc<AtomicBool>,
    }
    impl ProjectBackend for Backend {
        type Lease = ();
        fn open(
            &self,
            _: &str,
            _: &dyn OperationControl,
        ) -> Result<ValidatedProject<Self::Lease>, ProjectError> {
            Ok(ValidatedProject {
                identity: ProjectIdentity {
                    workspace_root: "/trusted/project".into(),
                    fingerprint: identity_fingerprint(1),
                },
                lease: (),
            })
        }
        fn revalidate(
            &self,
            _: &Self::Lease,
            _: &dyn OperationControl,
        ) -> Result<ProjectIdentity, ProjectError> {
            Ok(ProjectIdentity {
                workspace_root: "/trusted/project".into(),
                fingerprint: if self.revoked.load(Ordering::SeqCst) {
                    identity_fingerprint(2)
                } else {
                    identity_fingerprint(1)
                },
            })
        }
    }
    impl ProjectSourceBackend for Backend {
        fn source(
            &self,
            _: &Self::Lease,
            _: &dyn OperationControl,
        ) -> Result<SourceBundle, ProjectError> {
            self.captures.fetch_add(1, Ordering::SeqCst);
            Ok(source())
        }
    }
    impl QualityProjectBackend for Backend {
        fn revalidate_quality_owner(
            &self,
            _: &Self::Lease,
            _: &dyn OperationControl,
        ) -> Result<QualityOwnerFacts, ProjectError> {
            self.owner_validations.fetch_add(1, Ordering::SeqCst);
            Ok(QualityOwnerFacts {
                granted_root_device: 7,
                granted_root_inode: 11,
                workspace_root: "/trusted/project".into(),
            })
        }
    }

    pub(crate) struct Generator;
    impl ReferenceGenerator for Generator {
        fn generate(&self) -> Result<ProjectRef, ProjectError> {
            Ok("prj_00000000000000000000000000000001".parse().unwrap())
        }
    }

    pub(crate) type TestRegistry = ProjectRegistry<Backend, Generator, TestClock>;

    pub(crate) fn registry(backend: Backend, clock: TestClock) -> TestRegistry {
        ProjectRegistry::new(backend, Generator, clock, 10, 1).unwrap()
    }

    // -- benchmark fixtures --------------------------------------------------

    fn options() -> BenchmarkRunOptions {
        BenchmarkRunOptions::new(
            Some("member".into()),
            Some("throughput".into()),
            vec!["std".into()],
            false,
            false,
            3,
            900,
        )
        .unwrap()
    }

    fn dataset_with(
        selection: BenchmarkSelection,
        harness_version: &str,
        execution: &str,
        run_count: u8,
    ) -> BenchmarkDataset {
        let samples = (0..12)
            // Dealt over exactly the executions the provenance below declares,
            // so the dataset's own run-index bound is satisfied by construction.
            .map(|index| {
                let run_index = (index % u32::from(run_count.max(1))) as u8 + 1;
                RawSample::new(1, 1_000.0 + f64::from(index), run_index).unwrap()
            })
            .collect();
        let measurement = BenchmarkMeasurement::new(
            BenchmarkIdentity::new(
                "group".into(),
                Some("function".into()),
                None,
                "bench/one".into(),
                "bench_one".into(),
            )
            .unwrap(),
            SamplingMode::Flat,
            samples,
            3_000,
            5_000,
            30,
            MeasurementCompleteness::Complete,
        )
        .unwrap();
        BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            vec![measurement],
            BenchmarkProvenance {
                source_fingerprint: format!("sha256:{}", "a".repeat(64)),
                harness: BenchmarkHarness::Criterion,
                harness_version: harness_version.to_owned(),
                rust_version: "1.98.1".into(),
                cargo_version: "1.98.1".into(),
                declared_toolchain: Some("1.98.1".into()),
                image_digest: format!("sha256:{}", "c".repeat(64)),
                platform: "aarch64-unknown-linux-gnu".into(),
                configuration_fingerprint: format!("sha256:{}", "d".repeat(64)),
                execution_fingerprint: execution.to_owned(),
                selection,
                hardware: HardwareProfile {
                    cpu_model: Some("Neoverse-N1".into()),
                    cpu_cores: Some(4),
                    os_kernel: Some("Linux 6.6.0".into()),
                    arch: "aarch64".into(),
                    virtualization: Virtualization::Container,
                    cpu_governor: Some("performance".into()),
                    quotas: ResourceQuotas {
                        cpu_quota_millicores: Some(2_000),
                        memory_bytes: Some(2 << 30),
                        pids: Some(256),
                    },
                },
                run_index: 1,
                run_count,
                captured_at_unix: 1_757_000_000,
            },
        )
        .unwrap()
    }

    fn observation(options: &BenchmarkRunOptions) -> BenchmarkObservation {
        BenchmarkObservation {
            selection: options.selection(),
            harness: HarnessDetection::Criterion {
                version: APPROVED_CRITERION_VERSION.into(),
            },
            exit: BenchmarkExit::Passed,
            exit_code: Some(0),
            termination: ExecutionTermination::Exited,
            dataset: Some(dataset_with(
                options.selection(),
                APPROVED_CRITERION_VERSION,
                execution_fingerprint(31).as_str(),
                options.run_count(),
            )),
            omission: None,
            // The last repetition's tree, which is the one the observation's
            // own exit and logs describe.
            archive: Some(CriterionArchive {
                run_index: options.run_count(),
                bytes: b"criterion output tree".to_vec(),
            }),
            archive_omission: None,
            runs_completed: 3,
            runs_requested: options.run_count(),
            runtime: runtime(31),
            execution_fingerprint: execution_fingerprint(31),
            vendor_fingerprint: source_fingerprint(21),
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    // -- benchmark port doubles ---------------------------------------------

    struct Executor {
        observation: BenchmarkObservation,
        calls: Arc<AtomicUsize>,
    }
    impl ProjectBenchmarkPort for Executor {
        fn benchmark(
            &self,
            _: &SourceBundle,
            _: &CargoVendorSnapshot,
            _: &BenchmarkRunOptions,
            control: &dyn InspectionControl,
        ) -> Result<BenchmarkObservation, SecurityError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            control.check()?;
            Ok(self.observation.clone())
        }
    }

    #[derive(Default)]
    struct Publisher {
        calls: Arc<AtomicUsize>,
        revalidations: Arc<AtomicUsize>,
    }
    impl BenchmarkPublisher for Publisher {
        fn publish_benchmark(
            &mut self,
            _: &SecurityCapture,
            _: &BenchmarkObservation,
            revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
        ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            revalidate()?;
            self.revalidations.fetch_add(1, Ordering::SeqCst);
            Ok(vec![
                descriptor(QualityArtifactKind::BenchmarkDataset),
                descriptor(QualityArtifactKind::CriterionArchive),
            ])
        }
    }

    fn run(
        observation: BenchmarkObservation,
        options: &BenchmarkRunOptions,
    ) -> (Result<PublishedBenchmark, SecurityError>, Arc<AtomicUsize>) {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let control = Control::default();
        let mut registry = registry(backend, clock.clone());
        let opened = registry.open("/trusted/project", &control).unwrap();
        let executor = Executor {
            observation,
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let mut publisher = Publisher::default();
        let published = publisher.calls.clone();
        let result = registry.benchmark_durable(
            &opened.project_ref,
            &vendor(),
            options,
            BenchmarkPorts {
                executor: &executor,
                publisher: &mut publisher,
            },
            &clock,
            &control,
        );
        (result, published)
    }

    // -- options -------------------------------------------------------------

    #[test]
    fn options_reject_paths_flags_and_out_of_range_numbers() {
        // A hyphen inside the name is an ordinary cargo target and stays legal.
        assert!(
            BenchmarkRunOptions::new(
                Some("my-pkg_2".into()),
                Some("bench-one".into()),
                vec!["feat-a".into()],
                false,
                false,
                3,
                900
            )
            .is_ok()
        );
        // A leading `-` reads as an option wherever the name lands on an argv.
        for name in [
            "",
            "/etc/passwd",
            "../bench",
            "a b",
            "a;b",
            "-noplot",
            "--bench",
            "-",
            &"a".repeat(65),
        ] {
            assert_eq!(
                BenchmarkRunOptions::new(Some(name.into()), None, vec![], false, false, 3, 900),
                Err(BenchmarkOptionsError::InvalidPackage),
                "accepted package {name:?}"
            );
            assert_eq!(
                BenchmarkRunOptions::new(None, Some(name.into()), vec![], false, false, 3, 900),
                Err(BenchmarkOptionsError::InvalidBenchTarget),
                "accepted target {name:?}"
            );
            assert_eq!(
                BenchmarkRunOptions::new(None, None, vec![name.into()], false, false, 3, 900),
                Err(BenchmarkOptionsError::InvalidFeature),
                "accepted feature {name:?}"
            );
        }
        let many: Vec<String> = (0..=BENCHMARK_MAX_FEATURES)
            .map(|index| format!("feature-{index}"))
            .collect();
        assert_eq!(
            BenchmarkRunOptions::new(None, None, many, false, false, 3, 900),
            Err(BenchmarkOptionsError::TooManyFeatures)
        );
        for count in [4, u8::MAX] {
            assert_eq!(
                BenchmarkRunOptions::new(None, None, vec![], false, false, count, 900),
                Err(BenchmarkOptionsError::InvalidRunCount)
            );
        }
        for timeout in [901, u64::MAX] {
            assert_eq!(
                BenchmarkRunOptions::new(None, None, vec![], false, false, 3, timeout),
                Err(BenchmarkOptionsError::InvalidTimeout)
            );
        }
    }

    #[test]
    fn unspecified_run_count_and_timeout_take_the_frozen_defaults() {
        let options =
            BenchmarkRunOptions::new(None, None, vec![], false, false, 0, 0).expect("options");
        assert_eq!(options.run_count(), BENCHMARK_DEFAULT_RUN_COUNT);
        assert_eq!(options.timeout_seconds(), BENCHMARK_DEFAULT_TIMEOUT_SECONDS);
    }

    /// The comparison method refuses a direction below three executions a side
    /// (`MIN_EXECUTIONS_FOR_DIRECTION`), and this is the call that produces
    /// them. The two numbers are one decision seen from two crates: if the
    /// default run count ever drops, the product would ship a protocol whose
    /// own output can never be given a direction, and if the compare threshold
    /// ever rises above it the default would stop being sufficient. Neither is
    /// a change to make silently, so it fails here.
    #[test]
    fn the_default_run_count_is_what_the_comparison_method_requires() {
        assert_eq!(
            u8::try_from(rust_engineering_domain::benchmark_compare::MIN_EXECUTIONS_FOR_DIRECTION)
                .expect("the execution threshold fits the run-count contract"),
            BENCHMARK_DEFAULT_RUN_COUNT
        );
        // And the caller can still ask for fewer -- the published range starts
        // at one -- which is exactly why the comparison gate reads the samples
        // it was given instead of trusting the request that produced them.
        assert_eq!(BENCHMARK_MIN_RUN_COUNT, 1);
        assert_eq!(BENCHMARK_MAX_RUN_COUNT, BENCHMARK_DEFAULT_RUN_COUNT);
    }

    #[test]
    fn the_selection_is_normalized_once_and_never_names_a_profile() {
        let options = BenchmarkRunOptions::new(
            None,
            None,
            vec!["std".into(), "extra".into(), "std".into()],
            false,
            true,
            1,
            30,
        )
        .expect("options");
        assert_eq!(options.features(), ["extra", "std"]);
        let selection = options.selection();
        assert_eq!(selection.features, vec!["extra", "std"]);
        assert_eq!(selection.profile, BENCHMARK_PROFILE);
        // Two producers building the selection separately must agree verbatim.
        assert_eq!(selection, options.selection());
    }

    // -- durable path --------------------------------------------------------

    #[test]
    fn a_validated_run_publishes_both_artifacts_under_a_revalidated_owner() {
        let options = options();
        let (result, published) = run(observation(&options), &options);
        let result = result.expect("published");
        assert_eq!(published.load(Ordering::SeqCst), 1);
        assert_eq!(result.artifacts.len(), 2);
        assert_eq!(
            result.artifacts[0].kind,
            QualityArtifactKind::BenchmarkDataset
        );
        assert_eq!(
            result.artifacts[1].kind,
            QualityArtifactKind::CriterionArchive
        );
        assert!(result.observation.dataset.is_some());
    }

    #[test]
    fn an_unrecognized_harness_publishes_an_observation_without_a_dataset() {
        let options = options();
        let mut observed = observation(&options);
        observed.harness = HarnessDetection::Unrecognized;
        observed.dataset = None;
        observed.omission = Some(DatasetOmission::HarnessUnrecognized);
        // A harness this server does not recognise exports no criterion tree
        // either; both absences are declared.
        observed.archive = None;
        observed.archive_omission = Some(DatasetOmission::HarnessUnrecognized);
        observed.runs_completed = 0;
        let (result, published) = run(observed, &options);
        assert!(result.is_ok());
        assert_eq!(published.load(Ordering::SeqCst), 1);
    }

    /// The two members are published together or the tree is not published at
    /// all. Bytes offered where a measurement is what the pair means would be
    /// an artifact claiming more than the run produced.
    #[test]
    fn a_retained_tree_is_never_published_without_the_dataset_it_belongs_to() {
        let options = options();
        let mut observed = observation(&options);
        observed.dataset = None;
        observed.omission = Some(DatasetOmission::OutputMissing);
        // The tree itself is a perfectly coherent observation, so the domain
        // accepts it; the refusal is the application's publication rule.
        assert!(observed.consistent());
        assert_eq!(
            validate_benchmark_observation(&observed, &options, &vendor()),
            Err(SecurityError::InvalidMetadata)
        );
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    /// A run whose export this server could not retain still publishes its
    /// dataset, and says why the tree is missing rather than omitting it
    /// silently.
    #[test]
    fn a_declared_missing_tree_still_publishes_the_dataset() {
        let options = options();
        let mut observed = observation(&options);
        observed.archive = None;
        observed.archive_omission = Some(DatasetOmission::OutputTooLarge);
        assert_eq!(
            validate_benchmark_observation(&observed, &options, &vendor()),
            Ok(())
        );
        let (result, published) = run(observed, &options);
        let result = result.expect("published");
        assert_eq!(published.load(Ordering::SeqCst), 1);
        assert!(result.observation.dataset.is_some());
        assert!(result.observation.archive.is_none());
        assert_eq!(
            result.observation.archive_omission,
            Some(DatasetOmission::OutputTooLarge)
        );
    }

    /// An undeclared absence is not a result: a run that carries neither a tree
    /// nor a reason never reaches the store.
    #[test]
    fn a_tree_that_is_neither_retained_nor_declared_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.archive = None;
        observed.archive_omission = None;
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    /// A tree naming a repetition outside the requested run set describes some
    /// other execution.
    #[test]
    fn a_tree_from_outside_the_requested_run_set_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.archive = Some(CriterionArchive {
            run_index: options.run_count() + 1,
            bytes: b"criterion output tree".to_vec(),
        });
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_inconsistent_observation_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        // A dataset and an omission at once: the domain's own exclusion.
        observed.omission = Some(DatasetOmission::OutputTooLarge);
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_dataset_describing_another_selection_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        let mut other = options.selection();
        other.features.push("unrequested".into());
        observed.dataset = Some(dataset_with(
            other,
            APPROVED_CRITERION_VERSION,
            execution_fingerprint(31).as_str(),
            options.run_count(),
        ));
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_dataset_claiming_another_harness_version_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.dataset = Some(dataset_with(
            options.selection(),
            "0.5.1",
            execution_fingerprint(31).as_str(),
            options.run_count(),
        ));
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);

        // The same applies when the detection itself is off the approved version.
        let mut observed = observation(&options);
        observed.harness = HarnessDetection::Criterion {
            version: "0.5.1".into(),
        };
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_dataset_describing_another_execution_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.dataset = Some(dataset_with(
            options.selection(),
            APPROVED_CRITERION_VERSION,
            execution_fingerprint(99).as_str(),
            options.run_count(),
        ));
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_dataset_claiming_another_run_count_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.dataset = Some(dataset_with(
            options.selection(),
            APPROVED_CRITERION_VERSION,
            execution_fingerprint(31).as_str(),
            2,
        ));
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);

        let mut observed = observation(&options);
        observed.runs_requested = 2;
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_run_over_another_vendor_tree_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.vendor_fingerprint = source_fingerprint(77);
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_runtime_describing_another_execution_is_never_published() {
        let options = options();
        let mut observed = observation(&options);
        observed.runtime = runtime(88);
        let (result, published) = run(observed, &options);
        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_revoked_project_after_publication_fails_closed() {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let control = Control::default();
        let options = options();
        let mut registry = registry(backend.clone(), clock.clone());
        let opened = registry.open("/trusted/project", &control).unwrap();
        let executor = Executor {
            observation: observation(&options),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let mut publisher = Publisher::default();
        // The identity changes only after the publisher has been asked to commit.
        let revoked = backend.revoked.clone();
        let result = {
            let outcome = registry.benchmark_durable(
                &opened.project_ref,
                &vendor(),
                &options,
                BenchmarkPorts {
                    executor: &executor,
                    publisher: &mut publisher,
                },
                &clock,
                &control,
            );
            assert!(outcome.is_ok());
            revoked.store(true, Ordering::SeqCst);
            registry.benchmark_durable(
                &opened.project_ref,
                &vendor(),
                &options,
                BenchmarkPorts {
                    executor: &executor,
                    publisher: &mut publisher,
                },
                &clock,
                &control,
            )
        };
        // A changed fingerprint retires the lease before any capture succeeds.
        assert!(result.is_err());
    }

    #[test]
    fn cancellation_propagates_from_the_control_and_publishes_nothing() {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let control = Control::default();
        let options = options();
        let mut registry = registry(backend, clock.clone());
        let opened = registry.open("/trusted/project", &control).unwrap();
        let executor = Executor {
            observation: observation(&options),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let executed = executor.calls.clone();
        let mut publisher = Publisher::default();
        let published = publisher.calls.clone();
        control.cancel();
        let result = registry.benchmark_durable(
            &opened.project_ref,
            &vendor(),
            &options,
            BenchmarkPorts {
                executor: &executor,
                publisher: &mut publisher,
            },
            &clock,
            &control,
        );
        assert_eq!(
            result.err(),
            Some(SecurityError::Inspection(InspectionError::Project(
                ProjectError::Cancelled
            )))
        );
        assert_eq!(executed.load(Ordering::SeqCst), 0);
        assert_eq!(published.load(Ordering::SeqCst), 0);
    }
}
