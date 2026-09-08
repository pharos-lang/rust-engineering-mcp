#![allow(clippy::unwrap_used)] // Fixed fixtures must fail immediately when malformed.

use rust_engineering_application::coverage::{
    CoverageArtifactStreams, CoverageIdentity, CoverageObservation, ProjectCoveragePort,
};
use rust_engineering_application::mutation_test::{
    MutationArtifactStreams, MutationCompleteness, MutationTestObservation, ProjectMutationTestPort,
};
use rust_engineering_application::quality_v2::{
    QualityV2Inputs, QualityV2Options, QualityV2Ports, QualityV2Publisher,
};
use rust_engineering_application::security::{
    DenyObservation, ProjectDenyPort, SecurityArtifactStreams, SecurityCapture, SecurityError,
};
use rust_engineering_application::semver_check::{
    ProjectSemverPort, SemverObservation, SemverOptions,
};
use rust_engineering_application::{
    DependencyAuditPort, ExecutionCancellation, InspectionControl, InspectionError,
    OperationControl, ProjectBackend, ProjectCheckPort, ProjectClippyPort, ProjectError,
    ProjectFormatPort, ProjectIdentity, ProjectInspectionPort, ProjectRegistry,
    ProjectSourceBackend, ProjectTestPort, QualityOwnerFacts, QualityProjectBackend,
    ReferenceGenerator, RegistryClock, ValidatedProject,
};
use rust_engineering_domain::coverage::{CoverageMetrics, CoverageOptions, CoverageSummary};
use rust_engineering_domain::mutation_test::{
    MutationBaseline, MutationCounts, MutationGuestIdentity, MutationTestCommandOptions,
    MutationTestSelection,
};
use rust_engineering_domain::quality_v2::{QualityV2Profile, QualityV2StageKind};
use rust_engineering_domain::security::{
    DenyOptions, SecurityCounts, SecurityLint, SecurityPackage, SecurityPolicy,
    SecurityPolicyDocument, SecurityRules, SecuritySource,
};
use rust_engineering_domain::semver_check::{
    SemverExit, SemverFindingCompleteness, SemverFindingCounts,
};
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    AuditDataError, AuditObservation, AuditState, CargoConfiguration, CargoVendorPackage,
    CargoVendorSnapshot, CheckObservation, CheckOptions, CheckOutcome, ClippyOptions, Clock,
    ExecutionFingerprint, ExecutionTermination, FormatObservation, FreshnessPolicy,
    GuestArtifactName, IntegrityStatus, PayloadFormatVersion, PluginIdentity, ProjectConfigPolicy,
    ProjectIdentityFingerprint, ProjectRef, ProjectStructure, Provenance,
    QualityArtifactDescriptor, QualityArtifactDraft, QualityArtifactId, QualityArtifactKind,
    QualityJobId, QualityMimeType, RuntimeIdentity, SnapshotEvidence, SourceBundle, SourceFile,
    SourceFingerprint, SourceKind, TestObservation, TestOptions, ToolStatus, UnixSeconds,
    UtcInstant,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

fn source_fingerprint(value: u8) -> SourceFingerprint {
    format!("sha256:{value:064x}").parse().unwrap()
}

fn execution_fingerprint(value: u8) -> ExecutionFingerprint {
    format!("sha256:{value:064x}").parse().unwrap()
}

fn identity_fingerprint(value: u8) -> ProjectIdentityFingerprint {
    format!("sha256:{value:064x}").parse().unwrap()
}

fn source(candidate: bool) -> SourceBundle {
    let label = if candidate { "candidate" } else { "baseline" };
    SourceBundle::new(vec![
        SourceFile::new(
            "Cargo.toml".into(),
            format!("[package]\nname=\"{label}\"\nversion=\"1.0.0\"\n").into_bytes(),
        )
        .unwrap(),
        SourceFile::new(
            "Cargo.lock".into(),
            format!("version=4\n# {label}\n").into_bytes(),
        )
        .unwrap(),
    ])
    .unwrap()
}

fn runtime() -> RuntimeIdentity {
    RuntimeIdentity {
        platform: "linux/aarch64".into(),
        image_id: format!("sha256:{}", "6".repeat(64)),
        configuration_fingerprint: execution_fingerprint(10),
        execution_fingerprint: execution_fingerprint(12),
        rust_version: "1.98.1".into(),
        cargo_version: "1.98.1".into(),
        declared_toolchain: None,
    }
}

fn structure(candidate: bool) -> ProjectStructure {
    ProjectStructure {
        workspace_members: vec![],
        workspace_default_members: vec![],
        packages: vec![],
        profiles: vec![],
        cargo_configuration: CargoConfiguration {
            project_config_policy: ProjectConfigPolicy::Rejected,
            frozen: true,
            offline: true,
            incremental: false,
            target_directory_ephemeral: true,
        },
        runtime: runtime(),
        source_fingerprint: source_fingerprint(if candidate { 20 } else { 30 }),
    }
}

fn vendor() -> CargoVendorSnapshot {
    CargoVendorSnapshot {
        source: SourceBundle::new(vec![
            SourceFile::new(
                "dep-1.0.0/.cargo-checksum.json".into(),
                br#"{"files":{},"package":"fixture"}"#.to_vec(),
            )
            .unwrap(),
            SourceFile::new(
                "dep-1.0.0/Cargo.toml".into(),
                b"[package]\nname=\"dep\"\nversion=\"1.0.0\"\n".to_vec(),
            )
            .unwrap(),
        ])
        .unwrap(),
        tree_fingerprint: source_fingerprint(21),
        packages: vec![CargoVendorPackage {
            name: "dep".into(),
            version: "1.0.0".into(),
            package_checksum: source_fingerprint(29),
        }],
    }
}

fn policy() -> SecurityPolicy {
    SecurityPolicy::new(
        SecurityPolicyDocument {
            schema_version: 1,
            rules: SecurityRules {
                allowed_licenses: vec!["MIT".into()],
                banned_packages: vec![],
                multiple_versions: SecurityLint::Deny,
                wildcards: SecurityLint::Deny,
            },
            suppressions: vec![],
        },
        source_fingerprint(22),
        source_fingerprint(23),
        100,
    )
    .unwrap()
}

fn deny_observation() -> DenyObservation {
    DenyObservation {
        source_fingerprint: source_fingerprint(20),
        vendor_fingerprint: source_fingerprint(21),
        vendor_archive_fingerprint: source_fingerprint(35),
        policy_fingerprint: source_fingerprint(22),
        deny_config_fingerprint: source_fingerprint(36),
        cargo_config_fingerprint: source_fingerprint(37),
        metadata_original_fingerprint: source_fingerprint(24),
        metadata_derived_fingerprint: source_fingerprint(25),
        lock_fingerprint: source_fingerprint(26),
        runtime: runtime(),
        execution_fingerprint: execution_fingerprint(12),
        packages: vec![SecurityPackage {
            name: "dep".into(),
            version: "1.0.0".into(),
            source: SecuritySource::CratesIo,
            source_fingerprint: Some(source_fingerprint(29)),
        }],
        declared_licenses: vec![Some("MIT".into())],
        license_files: vec![vec![]],
        enabled_features: vec![vec![]],
        dependency_indices: vec![vec![]],
        workspace_members: vec![0],
        findings: vec![],
        findings_omitted: 0,
        licenses: SecurityCounts::default(),
        bans: SecurityCounts::default(),
        sources: SecurityCounts::default(),
        parse_complete: true,
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        artifacts: SecurityArtifactStreams::default(),
    }
}

#[derive(Clone, Default)]
struct TestClock(Arc<AtomicU64>);

impl TestClock {
    fn at(value: u64) -> Self {
        let clock = Self::default();
        clock.0.store(value, Ordering::SeqCst);
        clock
    }
}

impl Clock for TestClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0.load(Ordering::SeqCst))
    }
}

impl RegistryClock for TestClock {
    fn seconds(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Control(AtomicBool);

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
struct Backend {
    candidate_captures: Arc<AtomicUsize>,
    baseline_captures: Arc<AtomicUsize>,
    owner_validations: Arc<AtomicUsize>,
    revoked: Arc<AtomicBool>,
}

impl ProjectBackend for Backend {
    type Lease = String;

    fn open(
        &self,
        path: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<Self::Lease>, ProjectError> {
        let candidate = path.contains("candidate");
        Ok(ValidatedProject {
            identity: ProjectIdentity {
                workspace_root: path.into(),
                fingerprint: identity_fingerprint(if candidate { 1 } else { 2 }),
            },
            lease: path.into(),
        })
    }

    fn revalidate(
        &self,
        lease: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        let candidate = lease.contains("candidate");
        Ok(ProjectIdentity {
            workspace_root: lease.clone(),
            fingerprint: identity_fingerprint(
                if candidate && self.revoked.load(Ordering::SeqCst) {
                    9
                } else if candidate {
                    1
                } else {
                    2
                },
            ),
        })
    }
}

impl ProjectSourceBackend for Backend {
    fn source(
        &self,
        lease: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<SourceBundle, ProjectError> {
        let candidate = lease.contains("candidate");
        if candidate {
            self.candidate_captures.fetch_add(1, Ordering::SeqCst);
        } else {
            self.baseline_captures.fetch_add(1, Ordering::SeqCst);
        }
        Ok(source(candidate))
    }
}

impl QualityProjectBackend for Backend {
    fn revalidate_quality_owner(
        &self,
        lease: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<QualityOwnerFacts, ProjectError> {
        self.owner_validations.fetch_add(1, Ordering::SeqCst);
        Ok(QualityOwnerFacts {
            granted_root_device: 7,
            granted_root_inode: if lease.contains("candidate") { 11 } else { 12 },
            workspace_root: lease.clone(),
        })
    }
}

#[derive(Default)]
struct Generator(AtomicUsize);

impl ReferenceGenerator for Generator {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        let value = self.0.fetch_add(1, Ordering::SeqCst) + 1;
        format!("prj_{value:032x}")
            .parse()
            .map_err(|_| ProjectError::Internal)
    }
}

struct Executor {
    events: Arc<Mutex<Vec<&'static str>>>,
    partial_coverage: bool,
    mutation: Option<MutationTestObservation>,
}

impl Executor {
    fn new(partial_coverage: bool) -> Self {
        Self {
            events: Arc::new(Mutex::new(vec![])),
            partial_coverage,
            mutation: None,
        }
    }

    fn with_mutation(observation: MutationTestObservation) -> Self {
        Self {
            events: Arc::new(Mutex::new(vec![])),
            partial_coverage: false,
            mutation: Some(observation),
        }
    }

    fn record(&self, event: &'static str) {
        self.events.lock().unwrap().push(event);
    }

    fn validation(&self, event: &'static str) -> CheckObservation {
        self.record(event);
        CheckObservation {
            outcome: CheckOutcome::Passed,
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            validation_complete: true,
            diagnostics: vec![],
            diagnostics_omitted: 0,
            stdout: String::new(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            runtime: runtime(),
            source_fingerprint: source_fingerprint(20),
        }
    }
}

impl ProjectInspectionPort for Executor {
    fn inspect(
        &self,
        captured: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        let candidate = captured == &source(true);
        self.record(if candidate {
            "inspect_candidate"
        } else {
            "inspect_baseline"
        });
        Ok(structure(candidate))
    }
}

impl ProjectFormatPort for Executor {
    fn format(
        &self,
        _: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<FormatObservation, InspectionError> {
        Ok(FormatObservation {
            execution: self.validation("format"),
            affected_files: vec![],
            affected_files_omitted: 0,
            diff: None,
            diff_omitted: false,
        })
    }
}

impl ProjectCheckPort for Executor {
    fn check(
        &self,
        _: &SourceBundle,
        _: &CheckOptions,
        _: &dyn InspectionControl,
    ) -> Result<CheckObservation, InspectionError> {
        Ok(self.validation("check"))
    }
}

impl ProjectClippyPort for Executor {
    fn clippy(
        &self,
        _: &SourceBundle,
        _: &ClippyOptions,
        _: &dyn InspectionControl,
    ) -> Result<CheckObservation, InspectionError> {
        Ok(self.validation("clippy"))
    }
}

impl ProjectTestPort for Executor {
    fn test(
        &self,
        _: &SourceBundle,
        _: &TestOptions,
        _: &dyn InspectionControl,
    ) -> Result<TestObservation, InspectionError> {
        Ok(TestObservation {
            execution: self.validation("test"),
            build_succeeded: Some(true),
        })
    }
}

impl ProjectDenyPort for Executor {
    fn deny(
        &self,
        _: &SourceBundle,
        _: &CargoVendorSnapshot,
        _: &SecurityPolicy,
        _: &DenyOptions,
        _: &dyn InspectionControl,
    ) -> Result<DenyObservation, SecurityError> {
        self.record("deny");
        Ok(deny_observation())
    }
}

impl ProjectCoveragePort for Executor {
    fn run(
        &self,
        _: &SourceBundle,
        options: &CoverageOptions,
        _: &dyn InspectionControl,
    ) -> Result<CoverageObservation, InspectionError> {
        self.record("coverage");
        Ok(CoverageObservation {
            options: options.clone(),
            summary: CoverageSummary {
                aggregate: CoverageMetrics::new((10, 10), (10, 10), (1, 1))
                    .map_err(|_| InspectionError::InvalidMetadata)?,
                packages: vec![],
                files: vec![],
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
            parse_complete: !self.partial_coverage,
            runtime: runtime(),
            execution_fingerprint: execution_fingerprint(12),
            artifacts: CoverageArtifactStreams::default(),
        })
    }
}

impl ProjectSemverPort for Executor {
    fn run(
        &self,
        baseline: &SourceBundle,
        candidate: &SourceBundle,
        options: &SemverOptions,
        _: &dyn InspectionControl,
    ) -> Result<SemverObservation, InspectionError> {
        assert_eq!(baseline, &source(false));
        assert_eq!(candidate, &source(true));
        self.record("semver");
        Ok(SemverObservation {
            options: options.clone(),
            exit: SemverExit::NoBreak,
            counts: SemverFindingCounts::default(),
            findings: vec![],
            findings_omitted: 0,
            completeness: SemverFindingCompleteness::Partial,
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            runtime: runtime(),
            execution_fingerprint: execution_fingerprint(12),
            stdout: vec![],
            stderr: vec![],
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }
}

impl ProjectMutationTestPort for Executor {
    fn run(
        &self,
        _: &SourceBundle,
        _: &MutationTestCommandOptions,
        _: &dyn InspectionControl,
    ) -> Result<MutationTestObservation, InspectionError> {
        self.record("mutation");
        self.mutation.clone().ok_or(InspectionError::Internal)
    }
}

struct Auditor {
    calls: AtomicUsize,
    clock: TestClock,
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl DependencyAuditPort for Auditor {
    fn audit(
        &self,
        _: &SourceBundle,
        structure: &ProjectStructure,
        _: &dyn Clock,
        _: &dyn InspectionControl,
    ) -> Result<AuditObservation, AuditDataError> {
        assert_eq!(structure.source_fingerprint, source_fingerprint(20));
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.events.lock().unwrap().push("audit");
        let evidence = SnapshotEvidence::assess(
            Provenance::new(
                SourceKind::RustsecSnapshot,
                "rustsec-quality-v2".parse().unwrap(),
                Some(UnixSeconds(99)),
                Some(UnixSeconds(100)),
                IntegrityStatus::Verified,
                false,
            )
            .unwrap(),
            FreshnessPolicy::new("quality-v2".parse().unwrap(), 60, 300).unwrap(),
            &self.clock,
        );
        Ok(AuditObservation {
            state: AuditState::Passed,
            issue: None,
            validation_complete: true,
            lock_fingerprint: Some(source_fingerprint(26)),
            snapshot_fingerprint: Some(format!("sha256:{:064x}", 30).parse().unwrap()),
            snapshot: Some(evidence),
            snapshot_record_count: Some(1),
            snapshot_sequence: Some(1),
            packages_total: 1,
            crates_io_scanned: 1,
            workspace_packages_excluded: 0,
            unsupported_packages: vec![],
            findings: vec![],
            informational: vec![],
            findings_omitted: 0,
        })
    }
}

fn descriptor() -> QualityArtifactDescriptor {
    let created = UtcInstant::from_unix_seconds(1_788_000_000).unwrap();
    QualityArtifactDraft {
        artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
        member_index: 0,
        kind: QualityArtifactKind::ToolLog,
        mime_type: QualityMimeType::TextPlain,
        payload_format_version: PayloadFormatVersion::Utf8LogV1,
        completeness: ArtifactCompleteness::Complete,
        sensitivity: ArtifactSensitivity::PotentiallySensitive,
        created_at_utc: created.clone(),
        expires_at_utc: created.checked_add_seconds(60).unwrap(),
        source: rust_engineering_domain::ArtifactSource {
            captured_source_sha256: [2; 32],
            guest_name: GuestArtifactName::ToolLog,
            selection: ArtifactSelection::Workspace,
        },
        runtime: ArtifactRuntime {
            image_digest: [3; 32],
            toolchain_identity: [4; 32],
            plugin: ArtifactPlugin {
                identity: PluginIdentity::Builtin,
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
        3,
    )
    .unwrap()
}

#[derive(Default)]
struct Publisher {
    calls: usize,
    revalidations: usize,
    fail: bool,
    revoke_after_revalidate: Option<Arc<AtomicBool>>,
}

impl QualityV2Publisher for Publisher {
    fn publish_gate_v2(
        &mut self,
        capture: &SecurityCapture,
        observation: &rust_engineering_domain::quality_v2::QualityV2Observation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<QualityArtifactDescriptor, InspectionError> {
        self.calls += 1;
        assert_eq!(capture.source, source(true));
        assert!(observation.report.validate());
        if self.fail {
            return Err(InspectionError::Internal);
        }
        self.revalidations += 1;
        let owner = revalidate()?;
        assert_eq!(owner.workspace_root, "/trusted/candidate");
        if let Some(revoked) = &self.revoke_after_revalidate {
            revoked.store(true, Ordering::SeqCst);
        }
        Ok(descriptor())
    }
}

fn opened(
    backend: Backend,
    clock: &TestClock,
    release: bool,
) -> (
    ProjectRegistry<Backend, Generator, TestClock>,
    ProjectRef,
    Option<ProjectRef>,
) {
    let mut registry =
        ProjectRegistry::new(backend, Generator::default(), clock.clone(), 600, 4).unwrap();
    let candidate = registry
        .open("/trusted/candidate", &Control::default())
        .unwrap()
        .project_ref;
    let baseline = release.then(|| {
        registry
            .open("/trusted/baseline", &Control::default())
            .unwrap()
            .project_ref
    });
    (registry, candidate, baseline)
}

fn execute(
    profile: QualityV2Profile,
    partial_coverage: bool,
    backend: Backend,
    publisher: &mut Publisher,
) -> Result<
    (
        rust_engineering_domain::quality_v2::QualityV2Observation,
        Vec<&'static str>,
        usize,
    ),
    SecurityError,
> {
    let clock = TestClock::at(100);
    let release = profile == QualityV2Profile::Release;
    let (mut registry, candidate, baseline) = opened(backend, &clock, release);
    let executor = Executor::new(partial_coverage);
    let auditor = Auditor {
        calls: AtomicUsize::new(0),
        clock: clock.clone(),
        events: Arc::clone(&executor.events),
    };
    let options = QualityV2Options {
        profile,
        baseline,
        timeout_seconds: 3_600,
        mutation: None,
    };
    let vendor = vendor();
    let policy = policy();
    let result = registry.quality_gate_v2(
        &candidate,
        QualityV2Inputs {
            vendor: Some(&vendor),
            policy: Some(&policy),
            options: &options,
        },
        QualityV2Ports {
            executor: &executor,
            auditor: &auditor,
            publisher,
        },
        &clock,
        &Control::default(),
    )?;
    let events = executor.events.lock().unwrap().clone();
    Ok((result.observation, events, auditor.calls.into_inner()))
}

#[test]
fn strict_and_release_use_one_candidate_capture_one_audit_and_exact_stage_order() {
    for (profile, expected, baseline_captures) in [
        (
            QualityV2Profile::Strict,
            vec![
                "inspect_candidate",
                "format",
                "check",
                "clippy",
                "test",
                "audit",
                "deny",
                "coverage",
            ],
            0,
        ),
        (
            QualityV2Profile::Release,
            vec![
                "inspect_candidate",
                "inspect_baseline",
                "format",
                "check",
                "clippy",
                "test",
                "audit",
                "deny",
                "coverage",
                "semver",
            ],
            1,
        ),
    ] {
        let backend = Backend::default();
        let mut publisher = Publisher::default();
        let (observation, events, audits) =
            execute(profile, false, backend.clone(), &mut publisher).unwrap();
        assert_eq!(backend.candidate_captures.load(Ordering::SeqCst), 1);
        assert_eq!(
            backend.baseline_captures.load(Ordering::SeqCst),
            baseline_captures
        );
        assert_eq!(audits, 1);
        assert_eq!(events, expected);
        assert_eq!(
            observation
                .report
                .stages
                .iter()
                .map(|stage| stage.stage)
                .collect::<Vec<_>>(),
            profile.stages(false)
        );
        assert!(
            observation.report.complete,
            "stages: {:#?}",
            observation.report.stages
        );
        assert_eq!(observation.report.status, ToolStatus::Passed);
        assert_eq!(publisher.calls, 1);
        assert_eq!(publisher.revalidations, 1);
        assert_eq!(
            backend.owner_validations.load(Ordering::SeqCst),
            if profile == QualityV2Profile::Release {
                2
            } else {
                1
            }
        );
    }
}

#[test]
fn partial_required_stage_never_produces_a_passed_gate() {
    let backend = Backend::default();
    let mut publisher = Publisher::default();
    let (observation, _, _) =
        execute(QualityV2Profile::Strict, true, backend, &mut publisher).unwrap();
    let coverage = observation
        .report
        .stages
        .iter()
        .find(|stage| stage.stage == QualityV2StageKind::Coverage)
        .unwrap();
    assert_eq!(coverage.status, ToolStatus::Blocked);
    assert!(!observation.report.complete);
    assert_eq!(observation.report.status, ToolStatus::Blocked);
}

#[test]
fn missing_vendor_or_policy_makes_the_deny_stage_unavailable_and_gate_incomplete() {
    for missing_vendor in [true, false] {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let (mut registry, candidate, _) = opened(backend, &clock, false);
        let executor = Executor::new(false);
        let auditor = Auditor {
            calls: AtomicUsize::new(0),
            clock: clock.clone(),
            events: Arc::clone(&executor.events),
        };
        let mut publisher = Publisher::default();
        let options = QualityV2Options {
            profile: QualityV2Profile::Strict,
            baseline: None,
            timeout_seconds: 3_600,
            mutation: None,
        };
        let vendor = vendor();
        let policy = policy();

        let published = registry
            .quality_gate_v2(
                &candidate,
                QualityV2Inputs {
                    vendor: (!missing_vendor).then_some(&vendor),
                    policy: missing_vendor.then_some(&policy),
                    options: &options,
                },
                QualityV2Ports {
                    executor: &executor,
                    auditor: &auditor,
                    publisher: &mut publisher,
                },
                &clock,
                &Control::default(),
            )
            .unwrap();

        let deny = published
            .observation
            .report
            .stages
            .iter()
            .find(|stage| stage.stage == QualityV2StageKind::Deny)
            .unwrap();
        assert_eq!(deny.status, ToolStatus::Unavailable);
        assert!(deny.details.is_none());
        assert!(!published.observation.report.complete);
        assert_eq!(published.observation.report.status, ToolStatus::Unavailable);
        assert!(!executor.events.lock().unwrap().contains(&"deny"));
        assert_eq!(publisher.calls, 1);
    }
}

fn mutation_options() -> MutationTestCommandOptions {
    MutationTestCommandOptions::try_from(MutationTestSelection {
        max_mutants: 1,
        mutant_timeout_seconds: 1,
        ..Default::default()
    })
    .unwrap()
}

fn mutation_observation(
    baseline: MutationBaseline,
    counts: MutationCounts,
    validation_complete: bool,
) -> MutationTestObservation {
    MutationTestObservation {
        options: mutation_options(),
        completeness: if validation_complete {
            MutationCompleteness::Complete
        } else {
            MutationCompleteness::Partial
        },
        validation_complete,
        baseline,
        counts,
        mutants: vec![],
        mutants_omitted: u64::from(!validation_complete),
        cap_exceeded: false,
        mutants_version: "27.1.0".into(),
        guest_identity: MutationGuestIdentity::Guest,
        termination: ExecutionTermination::Exited,
        exit_code: Some(if counts.missed > 0 { 2 } else { 0 }),
        runtime: runtime(),
        execution_fingerprint: execution_fingerprint(12),
        artifacts: MutationArtifactStreams::default(),
    }
}

fn execute_mutation(
    mutation: MutationTestObservation,
) -> rust_engineering_domain::quality_v2::QualityV2Observation {
    let backend = Backend::default();
    let clock = TestClock::at(100);
    let (mut registry, candidate, _) = opened(backend, &clock, false);
    let executor = Executor::with_mutation(mutation);
    let auditor = Auditor {
        calls: AtomicUsize::new(0),
        clock: clock.clone(),
        events: Arc::clone(&executor.events),
    };
    let mut publisher = Publisher::default();
    let options = QualityV2Options {
        profile: QualityV2Profile::Strict,
        baseline: None,
        timeout_seconds: 3_600,
        mutation: Some(mutation_options()),
    };

    registry
        .quality_gate_v2(
            &candidate,
            QualityV2Inputs {
                vendor: Some(&vendor()),
                policy: Some(&policy()),
                options: &options,
            },
            QualityV2Ports {
                executor: &executor,
                auditor: &auditor,
                publisher: &mut publisher,
            },
            &clock,
            &Control::default(),
        )
        .unwrap()
        .observation
}

#[test]
fn mutation_clean_conclusive_and_inconclusive_results_keep_distinct_classifications() {
    let cases = [
        (
            mutation_observation(
                MutationBaseline::Passed,
                MutationCounts {
                    generated: 1,
                    tested: 1,
                    caught: 1,
                    ..Default::default()
                },
                true,
            ),
            ToolStatus::Passed,
            MutationBaseline::Passed,
            true,
        ),
        (
            mutation_observation(
                MutationBaseline::Passed,
                MutationCounts {
                    generated: 1,
                    tested: 1,
                    missed: 1,
                    ..Default::default()
                },
                true,
            ),
            ToolStatus::Failed,
            MutationBaseline::Passed,
            true,
        ),
        (
            mutation_observation(
                MutationBaseline::Missing,
                MutationCounts {
                    generated: 1,
                    ..Default::default()
                },
                false,
            ),
            ToolStatus::Blocked,
            MutationBaseline::Missing,
            false,
        ),
    ];

    for (input, expected_status, expected_baseline, expected_complete) in cases {
        let observation = execute_mutation(input);
        let mutation = observation
            .report
            .stages
            .iter()
            .find(|stage| stage.stage == QualityV2StageKind::Mutation)
            .unwrap();
        assert_eq!(mutation.status, expected_status);
        assert_eq!(mutation.evidence_complete(), expected_complete);
        assert!(matches!(
            mutation.details.as_ref(),
            Some(rust_engineering_domain::quality_v2::QualityV2Details::Mutation { .. })
        ));
        if let rust_engineering_domain::quality_v2::QualityV2Details::Mutation {
            baseline,
            validation_complete,
            ..
        } = mutation.details.as_ref().unwrap()
        {
            assert_eq!(*baseline, expected_baseline);
            assert_eq!(*validation_complete, expected_complete);
        }
        assert_eq!(observation.report.complete, expected_complete);
        assert_eq!(observation.report.status, expected_status);
    }
}

#[test]
fn invalid_mutation_budget_is_rejected_before_candidate_capture() {
    let backend = Backend::default();
    let clock = TestClock::at(100);
    let (mut registry, candidate, _) = opened(backend.clone(), &clock, false);
    let executor = Executor::new(false);
    let auditor = Auditor {
        calls: AtomicUsize::new(0),
        clock: clock.clone(),
        events: Arc::new(Mutex::new(vec![])),
    };
    let mut publisher = Publisher::default();
    let options = QualityV2Options {
        profile: QualityV2Profile::Strict,
        baseline: None,
        timeout_seconds: 3_600,
        mutation: Some(
            MutationTestCommandOptions::try_from(MutationTestSelection::default()).unwrap(),
        ),
    };
    let result = registry.quality_gate_v2(
        &candidate,
        QualityV2Inputs {
            vendor: Some(&vendor()),
            policy: Some(&policy()),
            options: &options,
        },
        QualityV2Ports {
            executor: &executor,
            auditor: &auditor,
            publisher: &mut publisher,
        },
        &clock,
        &Control::default(),
    );
    assert!(matches!(result, Err(SecurityError::InvalidMetadata)));
    assert_eq!(backend.candidate_captures.load(Ordering::SeqCst), 0);
    assert!(executor.events.lock().unwrap().is_empty());
    assert_eq!(auditor.calls.load(Ordering::SeqCst), 0);
    assert_eq!(publisher.calls, 0);
}

#[test]
fn publication_error_and_revocation_after_publisher_revalidation_never_return_evidence() {
    let backend = Backend::default();
    let mut failing = Publisher {
        fail: true,
        ..Default::default()
    };
    assert!(execute(QualityV2Profile::Strict, false, backend, &mut failing).is_err());
    assert_eq!(failing.calls, 1);
    assert_eq!(failing.revalidations, 0);

    let backend = Backend::default();
    let mut revoking = Publisher {
        revoke_after_revalidate: Some(Arc::clone(&backend.revoked)),
        ..Default::default()
    };
    assert!(execute(QualityV2Profile::Strict, false, backend, &mut revoking).is_err());
    assert_eq!(revoking.calls, 1);
    assert_eq!(revoking.revalidations, 1);
}
