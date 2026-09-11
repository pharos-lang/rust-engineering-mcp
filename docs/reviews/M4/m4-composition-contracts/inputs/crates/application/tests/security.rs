// Fixed security fixtures and poisoned test mutexes must fail the test immediately.
#![allow(clippy::unwrap_used)]

use rust_engineering_application::security::{
    DenyObservation, ProjectDenyPort, SecurityArtifactStreams, SecurityCapture, SecurityError,
    SecurityObservation, SecurityPorts, SecurityPublisher, inspect_security,
};
use rust_engineering_application::{
    DependencyAuditPort, ExecutionCancellation, InspectionControl, InspectionError,
    OperationControl, ProjectBackend, ProjectError, ProjectIdentity, ProjectInspectionPort,
    ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts, QualityProjectBackend,
    ReferenceGenerator, RegistryClock, ValidatedProject,
};
use rust_engineering_domain::security::{
    DenyOptions, DenySelection, FindingDisposition, SecurityCompleteness, SecurityCounts,
    SecurityEngine, SecurityFinding, SecurityLint, SecurityPackage, SecurityPolicy,
    SecurityPolicyDocument, SecurityPolicyState, SecurityRules, SecuritySeverity, SecuritySource,
    SecuritySuppression,
};
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    AuditDataError, AuditFinding, AuditIssue, AuditObservation, AuditPackage, AuditSeverity,
    AuditSource, AuditState, CargoConfiguration, CargoVendorPackage, CargoVendorSnapshot, Clock,
    ExecutionFingerprint, ExecutionTermination, FreshnessPolicy, GuestArtifactName,
    IntegrityStatus, OperationalErrorCode, PayloadFormatVersion, PluginIdentity,
    ProjectConfigPolicy, ProjectIdentityFingerprint, ProjectPackage, ProjectRef, ProjectStructure,
    Provenance, QualityArtifactDescriptor, QualityArtifactDraft, QualityArtifactId,
    QualityArtifactKind, QualityJobId, QualityMimeType, RuntimeIdentity, RustEdition,
    SnapshotEvidence, SourceBundle, SourceFile, SourceFingerprint, SourceKind, UnixSeconds,
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

fn source() -> SourceBundle {
    SourceBundle::new(vec![
        SourceFile::new(
            "Cargo.toml".into(),
            b"[package]\nname = \"captured-member\"\nversion = \"0.1.0\"\nedition = \"2024\"\nlicense = \"MIT\"\n"
                .to_vec(),
        )
        .unwrap(),
        SourceFile::new(
            "Cargo.lock".into(),
            b"version = 4\n\n[[package]]\nname = \"captured-member\"\nversion = \"0.1.0\"\n"
                .to_vec(),
        )
        .unwrap(),
        SourceFile::new("LICENSE-MIT".into(), b"MIT License\nfixture terms\n".to_vec()).unwrap(),
        SourceFile::new("src/lib.rs".into(), b"pub fn answer() -> u8 { 42 }\n".to_vec())
            .unwrap(),
    ])
    .unwrap()
}

fn runtime(execution: u8) -> RuntimeIdentity {
    RuntimeIdentity {
        platform: "linux/arm64".into(),
        image_id: "rust-m4-fixture@sha256:verified".into(),
        configuration_fingerprint: execution_fingerprint(10),
        execution_fingerprint: execution_fingerprint(execution),
        rust_version: "1.98.1".into(),
        cargo_version: "1.98.1".into(),
        declared_toolchain: Some("1.98.1".into()),
    }
}

fn structure() -> ProjectStructure {
    ProjectStructure {
        workspace_members: vec![0],
        workspace_default_members: vec![0],
        packages: vec![ProjectPackage {
            package_index: 0,
            name: "captured-member".into(),
            version: "0.1.0".into(),
            manifest_path: "Cargo.toml".into(),
            edition: RustEdition::E2024,
            rust_version: Some("1.98.1".into()),
            targets: vec![],
            features: vec![],
            direct_dependencies: vec![],
        }],
        profiles: vec![],
        cargo_configuration: CargoConfiguration {
            project_config_policy: ProjectConfigPolicy::Rejected,
            frozen: true,
            offline: true,
            incremental: false,
            target_directory_ephemeral: true,
        },
        runtime: runtime(11),
        source_fingerprint: source_fingerprint(20),
    }
}

fn vendor() -> CargoVendorSnapshot {
    CargoVendorSnapshot {
        source: SourceBundle::new(vec![
            SourceFile::new(
                "affected-crate-1.2.5/.cargo-checksum.json".into(),
                br#"{"files":{"LICENSE-MIT":"fixture"},"package":"fixture"}"#.to_vec(),
            )
            .unwrap(),
            SourceFile::new(
                "affected-crate-1.2.5/Cargo.toml".into(),
                b"[package]\nname = \"affected-crate\"\nversion = \"1.2.5\"\n".to_vec(),
            )
            .unwrap(),
            SourceFile::new(
                "affected-crate-1.2.5/LICENSE-MIT".into(),
                b"MIT License\nfixture terms\n".to_vec(),
            )
            .unwrap(),
            SourceFile::new(
                "affected-crate-1.2.5/src/lib.rs".into(),
                b"pub fn dependency() {}\n".to_vec(),
            )
            .unwrap(),
        ])
        .unwrap(),
        tree_fingerprint: source_fingerprint(21),
        packages: vec![CargoVendorPackage {
            name: "affected-crate".into(),
            version: "1.2.5".into(),
            package_checksum: source_fingerprint(29),
        }],
    }
}

fn package() -> SecurityPackage {
    SecurityPackage {
        name: "affected-crate".into(),
        version: "1.2.5".into(),
        source: SecuritySource::CratesIo,
        source_fingerprint: Some(source_fingerprint(29)),
    }
}

fn suppression() -> SecuritySuppression {
    SecuritySuppression {
        id: "temporary-rustsec-exception".into(),
        engine: SecurityEngine::Rustsec,
        rule: "RUSTSEC-2026-0001".into(),
        package: "affected-crate".into(),
        package_source: SecuritySource::CratesIo,
        version_requirement: ">=1.2.0, <1.3.0".into(),
        reason: "Migration is owned and tracked".into(),
        owner: "security-team".into(),
        expires_at: 200,
        rules_digest: source_fingerprint(23),
    }
}

fn policy(with_suppression: bool) -> SecurityPolicy {
    SecurityPolicy::new(
        SecurityPolicyDocument {
            schema_version: 1,
            rules: SecurityRules {
                allowed_licenses: vec!["MIT".into(), "Apache-2.0".into()],
                banned_packages: vec![],
                multiple_versions: SecurityLint::Deny,
                wildcards: SecurityLint::Deny,
            },
            suppressions: if with_suppression {
                vec![suppression()]
            } else {
                vec![]
            },
        },
        source_fingerprint(22),
        source_fingerprint(23),
        100,
    )
    .unwrap()
}

#[derive(Clone, Default)]
struct TestClock(Arc<AtomicU64>);

impl TestClock {
    fn at(value: u64) -> Self {
        let clock = Self::default();
        clock.set(value);
        clock
    }

    fn set(&self, value: u64) {
        self.0.store(value, Ordering::SeqCst);
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

struct Engine {
    structure: Mutex<ProjectStructure>,
    deny: Mutex<DenyObservation>,
    inspect_sources: Mutex<Vec<SourceBundle>>,
    deny_sources: Mutex<Vec<SourceBundle>>,
    clock: TestClock,
    advance_after_inspect: Option<u64>,
    advance_after_deny: Option<u64>,
}

impl Engine {
    fn new(clock: TestClock, deny: DenyObservation) -> Self {
        Self {
            structure: Mutex::new(structure()),
            deny: Mutex::new(deny),
            inspect_sources: Mutex::new(vec![]),
            deny_sources: Mutex::new(vec![]),
            clock,
            advance_after_inspect: None,
            advance_after_deny: None,
        }
    }
}

impl ProjectInspectionPort for Engine {
    fn inspect(
        &self,
        source: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        self.inspect_sources.lock().unwrap().push(source.clone());
        if let Some(now) = self.advance_after_inspect {
            self.clock.set(now);
        }
        Ok(self.structure.lock().unwrap().clone())
    }
}

impl ProjectDenyPort for Engine {
    fn deny(
        &self,
        source: &SourceBundle,
        _: &CargoVendorSnapshot,
        _: &SecurityPolicy,
        _: &DenyOptions,
        _: &dyn InspectionControl,
    ) -> Result<DenyObservation, SecurityError> {
        self.deny_sources.lock().unwrap().push(source.clone());
        if let Some(now) = self.advance_after_deny {
            self.clock.set(now);
        }
        Ok(self.deny.lock().unwrap().clone())
    }
}

struct Auditor {
    observation: AuditObservation,
    calls: AtomicUsize,
    sources: Mutex<Vec<SourceBundle>>,
    clock: TestClock,
    advance_after_audit: Option<u64>,
}

impl Auditor {
    fn new(clock: TestClock, observation: AuditObservation) -> Self {
        Self {
            observation,
            calls: AtomicUsize::new(0),
            sources: Mutex::new(vec![]),
            clock,
            advance_after_audit: None,
        }
    }
}

impl DependencyAuditPort for Auditor {
    fn audit(
        &self,
        source: &SourceBundle,
        structure: &ProjectStructure,
        _: &dyn Clock,
        _: &dyn InspectionControl,
    ) -> Result<AuditObservation, AuditDataError> {
        assert_eq!(structure.source_fingerprint, source_fingerprint(20));
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.sources.lock().unwrap().push(source.clone());
        if let Some(now) = self.advance_after_audit {
            self.clock.set(now);
        }
        Ok(self.observation.clone())
    }
}

fn snapshot(clock: &TestClock, created_at: Option<u64>) -> SnapshotEvidence {
    let observed_at = created_at.map(|created| UnixSeconds(created.saturating_add(1)));
    let provenance = Provenance::new(
        SourceKind::RustsecSnapshot,
        "rustsec-fixture-2026-09-07".parse().unwrap(),
        created_at.map(UnixSeconds),
        observed_at,
        IntegrityStatus::Verified,
        false,
    )
    .unwrap();
    let freshness = FreshnessPolicy::new("rustsec-m4-v1".parse().unwrap(), 60, 300).unwrap();
    SnapshotEvidence::assess(provenance, freshness, clock)
}

fn audit_observation(clock: &TestClock, created_at: Option<u64>) -> AuditObservation {
    AuditObservation {
        state: AuditState::Passed,
        issue: None,
        validation_complete: true,
        lock_fingerprint: Some(source_fingerprint(26)),
        snapshot_fingerprint: Some(format!("sha256:{:064x}", 30).parse().unwrap()),
        snapshot: Some(snapshot(clock, created_at)),
        snapshot_record_count: Some(1),
        snapshot_sequence: Some(7),
        packages_total: 1,
        crates_io_scanned: 1,
        workspace_packages_excluded: 0,
        unsupported_packages: vec![],
        findings: vec![],
        informational: vec![],
        findings_omitted: 0,
    }
}

fn advisory_finding() -> AuditFinding {
    AuditFinding {
        advisory_id: "RUSTSEC-2026-0001".into(),
        url: "https://rustsec.org/advisories/RUSTSEC-2026-0001.html".into(),
        title: "Fixture advisory".into(),
        package: AuditPackage {
            name: "affected-crate".into(),
            version: "1.2.5".into(),
            source: AuditSource::CratesIo,
            source_fingerprint: Some(source_fingerprint(29)),
        },
        patched_requirements: vec![">=1.3.0".into()],
        unaffected_requirements: vec!["<1.2.0".into()],
        severity: Some(AuditSeverity::High),
        informational: None,
        paths: vec![],
        paths_omitted: 0,
    }
}

fn finding() -> SecurityFinding {
    SecurityFinding {
        engine: SecurityEngine::Rustsec,
        rule: "RUSTSEC-2026-0001".into(),
        package: Some(package()),
        severity: SecuritySeverity::Error,
        message: "Original deny finding remains visible".into(),
        disposition: FindingDisposition::Active,
    }
}

fn clean_deny() -> DenyObservation {
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
        runtime: runtime(12),
        execution_fingerprint: execution_fingerprint(12),
        packages: vec![package()],
        declared_licenses: vec![Some("MIT".into())],
        license_files: vec![vec![("LICENSE-MIT".into(), source_fingerprint(27))]],
        enabled_features: vec![vec!["default".into()]],
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

fn options() -> DenyOptions {
    DenyOptions::try_from(DenySelection::default()).unwrap()
}

fn inspect(
    engine: &Engine,
    auditor: &Auditor,
    policy: &SecurityPolicy,
    clock: &TestClock,
) -> Result<rust_engineering_application::security::SecurityObservation, SecurityError> {
    inspect_security(
        &source(),
        &vendor(),
        policy,
        &options(),
        SecurityPorts {
            executor: engine,
            auditor,
        },
        clock,
        &Control::default(),
    )
}

#[test]
fn one_source_generation_reaches_all_ports_and_audit_is_bitwise_standalone_equivalent() {
    let clock = TestClock::at(1_000);
    let mut audit = audit_observation(&clock, Some(990));
    audit.state = AuditState::Failed;
    audit.findings.push(advisory_finding());
    let expected_port = Auditor::new(clock.clone(), audit.clone());
    let expected = expected_port
        .audit(&source(), &structure(), &clock, &Control::default())
        .unwrap();
    let auditor = Auditor::new(clock.clone(), audit);
    let engine = Engine::new(clock.clone(), clean_deny());

    let result = inspect(&engine, &auditor, &policy(false), &clock).unwrap();

    assert_eq!(auditor.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        engine.inspect_sources.lock().unwrap().as_slice(),
        &[source()]
    );
    assert_eq!(auditor.sources.lock().unwrap().as_slice(), &[source()]);
    assert_eq!(engine.deny_sources.lock().unwrap().as_slice(), &[source()]);
    assert_eq!(
        serde_json::to_value(&result.audit).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert_eq!(result.completeness, SecurityCompleteness::Complete);
    assert_eq!(result.policy_state, SecurityPolicyState::Violated);
}

#[test]
fn stale_or_unknown_audit_cannot_pass_a_clean_deny() {
    for created_at in [Some(0), None] {
        let clock = TestClock::at(1_000);
        let auditor = Auditor::new(clock.clone(), audit_observation(&clock, created_at));
        let engine = Engine::new(clock.clone(), clean_deny());

        let result = inspect(&engine, &auditor, &policy(false), &clock).unwrap();

        assert_eq!(result.completeness, SecurityCompleteness::Partial);
        assert_eq!(result.policy_state, SecurityPolicyState::Undetermined);
        assert_eq!(result.deny.findings.len(), 0);
        assert_eq!(result.deny.exit_code, Some(0));
    }
}

#[test]
fn exact_suppression_preserves_original_and_does_not_repair_missing_audit_data() {
    let clock = TestClock::at(100);
    let mut audit = audit_observation(&clock, None);
    audit.state = AuditState::Unavailable;
    audit.issue = Some(AuditIssue::SnapshotUnknownAge);
    let auditor = Auditor::new(clock.clone(), audit);
    let mut deny = clean_deny();
    let original = finding();
    deny.findings.push(original.clone());
    deny.licenses.errors = 1;
    let engine = Engine::new(clock.clone(), deny);

    let result = inspect(&engine, &auditor, &policy(true), &clock).unwrap();

    assert_eq!(result.findings.len(), 1);
    let retained = &result.findings[0];
    assert_eq!(retained.engine, original.engine);
    assert_eq!(retained.rule, original.rule);
    assert_eq!(retained.package, original.package);
    assert_eq!(retained.severity, original.severity);
    assert_eq!(retained.message, original.message);
    assert_eq!(
        retained.disposition,
        FindingDisposition::Suppressed(suppression())
    );
    assert_eq!(result.completeness, SecurityCompleteness::Partial);
    assert_eq!(result.policy_state, SecurityPolicyState::Undetermined);
}

#[test]
fn suppression_requires_exact_engine_rule_package_source_and_version() {
    for mutation in 0..5 {
        let clock = TestClock::at(100);
        let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
        let mut row = finding();
        match mutation {
            0 => row.engine = SecurityEngine::Bans,
            1 => row.rule = "RUSTSEC-2026-0002".into(),
            2 => row.package.as_mut().unwrap().name = "other-crate".into(),
            3 => row.package.as_mut().unwrap().source = SecuritySource::Workspace,
            4 => row.package.as_mut().unwrap().version = "1.3.0".into(),
            _ => unreachable!(),
        }
        let mut deny = clean_deny();
        deny.findings.push(row);
        deny.licenses.errors = 1;
        let engine = Engine::new(clock.clone(), deny);

        let result = inspect(&engine, &auditor, &policy(true), &clock).unwrap();

        assert_eq!(result.findings[0].disposition, FindingDisposition::Active);
        assert_eq!(result.policy_state, SecurityPolicyState::Violated);
    }
}

#[test]
fn source_runtime_lock_vendor_and_policy_mismatches_are_rejected() {
    for mutation in 0..5 {
        let clock = TestClock::at(100);
        let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
        let mut deny = clean_deny();
        match mutation {
            0 => deny.source_fingerprint = source_fingerprint(31),
            1 => deny.runtime.image_id = "different-image".into(),
            2 => deny.lock_fingerprint = source_fingerprint(32),
            3 => deny.vendor_fingerprint = source_fingerprint(33),
            4 => deny.policy_fingerprint = source_fingerprint(34),
            _ => unreachable!(),
        }
        let engine = Engine::new(clock.clone(), deny);

        assert_eq!(
            inspect(&engine, &auditor, &policy(false), &clock).unwrap_err(),
            SecurityError::InvalidMetadata,
            "mismatch case {mutation}"
        );
    }
}

#[test]
fn policy_expiry_during_any_engine_stage_rejects_publication() {
    for stage in 0..3 {
        let clock = TestClock::at(100);
        let mut auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
        let mut engine = Engine::new(clock.clone(), clean_deny());
        match stage {
            0 => engine.advance_after_inspect = Some(200),
            1 => auditor.advance_after_audit = Some(200),
            2 => engine.advance_after_deny = Some(200),
            _ => unreachable!(),
        }

        assert_eq!(
            inspect(&engine, &auditor, &policy(true), &clock).unwrap_err(),
            SecurityError::InvalidPolicy,
            "expiry stage {stage}"
        );
    }
}

#[test]
fn audit_or_deny_omissions_never_false_pass() {
    for omitted_by_audit in [true, false] {
        let clock = TestClock::at(100);
        let mut audit = audit_observation(&clock, Some(99));
        let mut deny = clean_deny();
        if omitted_by_audit {
            audit.findings_omitted = 1;
        } else {
            deny.findings_omitted = 1;
            deny.licenses.errors = 1;
        }
        let auditor = Auditor::new(clock.clone(), audit);
        let engine = Engine::new(clock.clone(), deny);

        let result = inspect(&engine, &auditor, &policy(false), &clock).unwrap();

        assert_eq!(result.findings_omitted, 1);
        assert_eq!(result.completeness, SecurityCompleteness::Partial);
        assert_eq!(result.policy_state, SecurityPolicyState::Undetermined);
    }
}

#[derive(Clone, Default)]
struct Backend {
    captures: Arc<AtomicUsize>,
    validations: Arc<AtomicUsize>,
    owner_validations: Arc<AtomicUsize>,
    revoked: Arc<AtomicBool>,
    exception_source: Arc<AtomicBool>,
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
        self.validations.fetch_add(1, Ordering::SeqCst);
        if self.revoked.load(Ordering::SeqCst) {
            return Ok(ProjectIdentity {
                workspace_root: "/trusted/project".into(),
                fingerprint: identity_fingerprint(2),
            });
        }
        Ok(ProjectIdentity {
            workspace_root: "/trusted/project".into(),
            fingerprint: identity_fingerprint(1),
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
        if self.exception_source.load(Ordering::SeqCst) {
            let mut files = source().files().to_vec();
            files.push(SourceFile::new("deny.exceptions.toml".into(), vec![]).unwrap());
            return Ok(SourceBundle::new(files).unwrap());
        }
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

struct Generator;

impl ReferenceGenerator for Generator {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        Ok("prj_00000000000000000000000000000001".parse().unwrap())
    }
}

#[test]
fn registry_security_capture_reads_backend_once_under_revalidated_authority() {
    let backend = Backend::default();
    let clock = TestClock::at(100);
    let control = Control::default();
    let mut registry =
        ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1).unwrap();
    let opened = registry.open("/trusted/project", &control).unwrap();

    let capture = registry
        .capture_security(&opened.project_ref, &clock, &control)
        .unwrap();

    assert_eq!(backend.captures.load(Ordering::SeqCst), 1);
    assert_eq!(backend.validations.load(Ordering::SeqCst), 3);
    assert_eq!(capture.project_ref, opened.project_ref);
    assert_eq!(
        capture.project_identity_fingerprint,
        opened.identity.fingerprint
    );
    assert_eq!(capture.captured_at, UnixSeconds(100));
    assert_eq!(capture.source, source());
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
    revalidation_attempts: usize,
    saw_original_streams: bool,
    revoke_before_revalidate: Option<Arc<AtomicBool>>,
    advance_after_revalidate: Option<(TestClock, u64)>,
    fail_before_revalidate: bool,
}

impl SecurityPublisher for Publisher {
    fn publish(
        &mut self,
        capture: &SecurityCapture,
        observation: &SecurityObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<QualityArtifactDescriptor, InspectionError> {
        self.calls += 1;
        assert_eq!(capture.source, source());
        self.saw_original_streams = observation.deny.artifacts.stdout == b"raw stdout"
            && observation.deny.artifacts.stderr == b"raw stderr";
        if self.fail_before_revalidate {
            return Err(InspectionError::Internal);
        }
        if let Some(revoked) = &self.revoke_before_revalidate {
            revoked.store(true, Ordering::SeqCst);
        }
        self.revalidation_attempts += 1;
        let owner = revalidate()?;
        assert_eq!(owner.workspace_root, "/trusted/project");
        assert_eq!(owner.granted_root_device, 7);
        assert_eq!(owner.granted_root_inode, 11);
        if let Some((clock, now)) = &self.advance_after_revalidate {
            clock.set(*now);
        }
        Ok(descriptor())
    }
}

fn raw_deny() -> DenyObservation {
    let mut deny = clean_deny();
    deny.artifacts = SecurityArtifactStreams {
        stdout: b"raw stdout".to_vec(),
        stderr: b"raw stderr".to_vec(),
        stdout_truncated: false,
        stderr_truncated: false,
    };
    deny
}

fn open_registry(
    backend: Backend,
    clock: &TestClock,
) -> (ProjectRegistry<Backend, Generator, TestClock>, ProjectRef) {
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend, Generator, clock.clone(), 10, 1).unwrap();
    let reference = registry
        .open("/trusted/project", &control)
        .unwrap()
        .project_ref;
    (registry, reference)
}

#[test]
fn durable_deny_captures_and_runs_each_engine_once_revalidates_owner_and_scrubs_streams() {
    let backend = Backend::default();
    let clock = TestClock::at(100);
    let (mut registry, reference) = open_registry(backend.clone(), &clock);
    let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
    let engine = Engine::new(clock.clone(), raw_deny());
    let mut publisher = Publisher::default();

    let published = registry
        .deny_durable(
            &reference,
            &vendor(),
            &policy(false),
            &options(),
            SecurityPorts {
                executor: &engine,
                auditor: &auditor,
            },
            &mut publisher,
            &clock,
            &Control::default(),
        )
        .unwrap();

    assert_eq!(backend.captures.load(Ordering::SeqCst), 1);
    assert_eq!(engine.inspect_sources.lock().unwrap().len(), 1);
    assert_eq!(auditor.calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.deny_sources.lock().unwrap().len(), 1);
    assert_eq!(publisher.calls, 1);
    assert_eq!(publisher.revalidation_attempts, 1);
    assert_eq!(backend.owner_validations.load(Ordering::SeqCst), 1);
    assert!(publisher.saw_original_streams);
    assert!(published.observation.deny.artifacts.stdout.is_empty());
    assert!(published.observation.deny.artifacts.stderr.is_empty());
    assert!(!published.observation.deny.artifacts.stdout_truncated);
    assert!(!published.observation.deny.artifacts.stderr_truncated);
    assert_eq!(published.artifact, descriptor());
}

#[test]
fn durable_deny_rejects_revocation_and_policy_expiry_during_publication() {
    for expiry in [false, true] {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let (mut registry, reference) = open_registry(backend.clone(), &clock);
        let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
        let engine = Engine::new(clock.clone(), raw_deny());
        let mut publisher = if expiry {
            Publisher {
                advance_after_revalidate: Some((clock.clone(), 200)),
                ..Publisher::default()
            }
        } else {
            Publisher {
                revoke_before_revalidate: Some(Arc::clone(&backend.revoked)),
                ..Publisher::default()
            }
        };

        let result = registry.deny_durable(
            &reference,
            &vendor(),
            &policy(expiry),
            &options(),
            SecurityPorts {
                executor: &engine,
                auditor: &auditor,
            },
            &mut publisher,
            &clock,
            &Control::default(),
        );

        assert_eq!(publisher.calls, 1);
        assert_eq!(publisher.revalidation_attempts, 1);
        assert!(publisher.saw_original_streams);
        assert_eq!(
            result.err(),
            Some(if expiry {
                SecurityError::InvalidPolicy
            } else {
                SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::InvalidProject,
                )))
            })
        );
    }
}

#[test]
fn publisher_error_is_not_converted_into_a_success() {
    let backend = Backend::default();
    let clock = TestClock::at(100);
    let (mut registry, reference) = open_registry(backend, &clock);
    let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
    let engine = Engine::new(clock.clone(), raw_deny());
    let mut publisher = Publisher {
        fail_before_revalidate: true,
        ..Publisher::default()
    };

    let result = registry.deny_durable(
        &reference,
        &vendor(),
        &policy(false),
        &options(),
        SecurityPorts {
            executor: &engine,
            auditor: &auditor,
        },
        &mut publisher,
        &clock,
        &Control::default(),
    );

    assert_eq!(
        result.err(),
        Some(SecurityError::Inspection(InspectionError::Internal))
    );
    assert_eq!(publisher.calls, 1);
    assert_eq!(publisher.revalidation_attempts, 0);
}

#[test]
fn project_exception_is_rejected_before_inspection_audit_deny_and_publish() {
    let backend = Backend::default();
    backend.exception_source.store(true, Ordering::SeqCst);
    let clock = TestClock::at(100);
    let (mut registry, reference) = open_registry(backend.clone(), &clock);
    let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
    let engine = Engine::new(clock.clone(), clean_deny());
    let mut publisher = Publisher::default();

    let result = registry.deny_durable(
        &reference,
        &vendor(),
        &policy(false),
        &options(),
        SecurityPorts {
            executor: &engine,
            auditor: &auditor,
        },
        &mut publisher,
        &clock,
        &Control::default(),
    );

    assert_eq!(result.err(), Some(SecurityError::InvalidPolicy));
    assert_eq!(backend.captures.load(Ordering::SeqCst), 1);
    assert!(engine.inspect_sources.lock().unwrap().is_empty());
    assert_eq!(auditor.calls.load(Ordering::SeqCst), 0);
    assert!(engine.deny_sources.lock().unwrap().is_empty());
    assert_eq!(publisher.calls, 0);
}

struct Scanner {
    calls: AtomicUsize,
    invalid: bool,
    partial: bool,
}
impl rust_engineering_application::unsafe_scan::ProjectUnsafeScanPort for Scanner {
    fn unsafe_scan(
        &self,
        captured: &SourceBundle,
        vendor: &CargoVendorSnapshot,
        _: &rust_engineering_domain::unsafe_scan::UnsafeScanOptions,
        _: &dyn InspectionControl,
    ) -> Result<rust_engineering_application::unsafe_scan::UnsafeObservation, SecurityError> {
        use rust_engineering_domain::unsafe_scan::{
            UnsafeCoverage, UnsafeObservation, UnsafeScanReport,
        };
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(captured, &source());
        Ok(UnsafeObservation {
            report: UnsafeScanReport {
                coverage: UnsafeCoverage {
                    files_total: 2,
                    files_selected: 2,
                    files_parsed: if self.partial { 1 } else { 2 },
                    files_crashed: u32::from(self.partial),
                    workspace_files: 1,
                    dependency_files: 1,
                    ..Default::default()
                },
                findings: vec![],
                findings_total: 0,
                findings_omitted: 0,
                syntax_complete: !self.partial,
                cfg_evaluated: self.invalid,
                macros_expanded: false,
                generated_sources_scanned: false,
            },
            source_fingerprint: source_fingerprint(20),
            vendor_fingerprint: vendor.tree_fingerprint.clone(),
            vendor_archive_fingerprint: source_fingerprint(21),
            metadata_fingerprint: source_fingerprint(22),
            manifest_fingerprint: source_fingerprint(23),
            runtime: runtime(12),
            execution_fingerprint: execution_fingerprint(12),
        })
    }
}
struct ScanPublisher {
    calls: usize,
    revoke: Option<Arc<AtomicBool>>,
    fail: bool,
}
impl rust_engineering_application::unsafe_scan::UnsafePublisher for ScanPublisher {
    fn publish_unsafe(
        &mut self,
        capture: &SecurityCapture,
        observation: &rust_engineering_application::unsafe_scan::UnsafeObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<QualityArtifactDescriptor, InspectionError> {
        self.calls += 1;
        assert_eq!(capture.source, source());
        assert!(observation.report.validate());
        revalidate()?;
        if let Some(revoked) = &self.revoke {
            revoked.store(true, Ordering::SeqCst);
        }
        if self.fail {
            return Err(InspectionError::Internal);
        }
        Ok(descriptor())
    }
}
#[test]
fn scanner_uses_one_capture_and_keeps_partial_files_without_a_safety_verdict() {
    use rust_engineering_application::unsafe_scan::UnsafePorts;
    for partial in [false, true] {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let control = Control::default();
        let mut registry =
            ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1).unwrap();
        let opened = registry.open("/trusted/project", &control).unwrap();
        let scanner = Scanner {
            calls: AtomicUsize::new(0),
            invalid: false,
            partial,
        };
        let mut publisher = ScanPublisher {
            calls: 0,
            revoke: None,
            fail: false,
        };
        let result = registry
            .unsafe_scan_durable(
                &opened.project_ref,
                &vendor(),
                &rust_engineering_domain::unsafe_scan::UnsafeScanOptions::new(120).unwrap(),
                UnsafePorts {
                    executor: &scanner,
                    publisher: &mut publisher,
                },
                &clock,
                &control,
            )
            .unwrap();
        assert_eq!(result.observation.report.syntax_complete, !partial);
        assert_eq!(
            result.observation.report.coverage.files_crashed,
            u32::from(partial)
        );
        assert_eq!(backend.captures.load(Ordering::SeqCst), 1);
        assert_eq!(scanner.calls.load(Ordering::SeqCst), 1);
        assert_eq!(publisher.calls, 1);
        assert!(backend.owner_validations.load(Ordering::SeqCst) > 0);
    }
}
#[test]
fn scanner_rejects_forged_coverage_publication_failure_and_owner_revocation() {
    use rust_engineering_application::unsafe_scan::UnsafePorts;
    for case in 0..3 {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let control = Control::default();
        let mut registry =
            ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1).unwrap();
        let opened = registry.open("/trusted/project", &control).unwrap();
        let scanner = Scanner {
            calls: AtomicUsize::new(0),
            invalid: case == 0,
            partial: false,
        };
        let mut publisher = ScanPublisher {
            calls: 0,
            revoke: (case == 2).then(|| Arc::clone(&backend.revoked)),
            fail: case == 1,
        };
        let result = registry.unsafe_scan_durable(
            &opened.project_ref,
            &vendor(),
            &rust_engineering_domain::unsafe_scan::UnsafeScanOptions::new(120).unwrap(),
            UnsafePorts {
                executor: &scanner,
                publisher: &mut publisher,
            },
            &clock,
            &control,
        );
        assert!(result.is_err());
        assert_eq!(publisher.calls, usize::from(case != 0));
    }
}

#[derive(Clone)]
enum SupplyDenyAnswer {
    Observation(Box<DenyObservation>),
    Error(SecurityError),
}

struct SupplyEngine {
    structure: Mutex<ProjectStructure>,
    deny: Mutex<SupplyDenyAnswer>,
    graph: Mutex<rust_engineering_domain::supply_chain::SupplyGraph>,
    inspect_calls: AtomicUsize,
    deny_calls: AtomicUsize,
    facts_calls: AtomicUsize,
    facts_received_deny: AtomicBool,
    sources: Mutex<Vec<SourceBundle>>,
}

impl SupplyEngine {
    fn new(
        deny: SupplyDenyAnswer,
        graph: rust_engineering_domain::supply_chain::SupplyGraph,
    ) -> Self {
        Self {
            structure: Mutex::new(structure()),
            deny: Mutex::new(deny),
            graph: Mutex::new(graph),
            inspect_calls: AtomicUsize::new(0),
            deny_calls: AtomicUsize::new(0),
            facts_calls: AtomicUsize::new(0),
            facts_received_deny: AtomicBool::new(false),
            sources: Mutex::new(vec![]),
        }
    }
}

impl ProjectInspectionPort for SupplyEngine {
    fn inspect(
        &self,
        captured: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        self.inspect_calls.fetch_add(1, Ordering::SeqCst);
        self.sources.lock().unwrap().push(captured.clone());
        Ok(self.structure.lock().unwrap().clone())
    }
}

impl ProjectDenyPort for SupplyEngine {
    fn deny(
        &self,
        captured: &SourceBundle,
        _: &CargoVendorSnapshot,
        _: &SecurityPolicy,
        _: &DenyOptions,
        _: &dyn InspectionControl,
    ) -> Result<DenyObservation, SecurityError> {
        self.deny_calls.fetch_add(1, Ordering::SeqCst);
        self.sources.lock().unwrap().push(captured.clone());
        match &*self.deny.lock().unwrap() {
            SupplyDenyAnswer::Observation(observation) => Ok(*observation.clone()),
            SupplyDenyAnswer::Error(error) => Err(*error),
        }
    }
}

impl rust_engineering_application::supply_chain::SupplyFactsPort for SupplyEngine {
    fn supply_facts(
        &self,
        captured: &SourceBundle,
        _: Option<&CargoVendorSnapshot>,
        deny: Option<&DenyObservation>,
        _: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::supply_chain::SupplyGraph, SecurityError> {
        self.facts_calls.fetch_add(1, Ordering::SeqCst);
        self.facts_received_deny
            .store(deny.is_some(), Ordering::SeqCst);
        self.sources.lock().unwrap().push(captured.clone());
        Ok(self.graph.lock().unwrap().clone())
    }
}

fn supply_graph() -> rust_engineering_domain::supply_chain::SupplyGraph {
    use rust_engineering_domain::supply_chain::{SupplyPackage, SupplySource, YankedFact};
    rust_engineering_domain::supply_chain::SupplyGraph {
        source_fingerprint: source_fingerprint(20),
        lock_fingerprint: source_fingerprint(26),
        packages: vec![SupplyPackage {
            name: "captured-member".into(),
            version: "0.1.0".into(),
            source: SupplySource::Workspace,
            source_fingerprint: None,
            declared_checksum: None,
            checksum_verified: false,
            duplicate_name: false,
            declared_features: Some(vec![]),
            active_features: Some(vec![]),
            yanked: YankedFact::NotApplicable,
        }],
    }
}

fn catalog_evidence(clock: &TestClock, created_at: Option<u64>) -> SnapshotEvidence {
    let observed_at = created_at.map(|created| UnixSeconds(created.saturating_add(1)));
    let provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "registry-supply-fixture-v1".parse().unwrap(),
        created_at.map(UnixSeconds),
        observed_at,
        IntegrityStatus::Verified,
        false,
    )
    .unwrap();
    SnapshotEvidence::assess(
        provenance,
        FreshnessPolicy::new("registry-supply-policy-v1".parse().unwrap(), 60, 300).unwrap(),
        clock,
    )
}

struct SupplyCatalog {
    value: rust_engineering_domain::supply_chain::SupplyCatalog,
    calls: AtomicUsize,
    rows_seen: AtomicUsize,
}

impl SupplyCatalog {
    fn available(clock: &TestClock, created_at: Option<u64>) -> Self {
        Self {
            value: rust_engineering_domain::supply_chain::SupplyCatalog {
                availability: rust_engineering_domain::supply_chain::SupplyAvailability::Available,
                snapshot_fingerprint: Some(source_fingerprint(40).to_string().parse().unwrap()),
                bundle_fingerprint: Some(source_fingerprint(41)),
                sequence: Some(3),
                evidence: Some(catalog_evidence(clock, created_at)),
                lookups: 1,
            },
            calls: AtomicUsize::new(0),
            rows_seen: AtomicUsize::new(0),
        }
    }

    fn unavailable() -> Self {
        Self {
            value: rust_engineering_domain::supply_chain::SupplyCatalog {
                availability:
                    rust_engineering_domain::supply_chain::SupplyAvailability::Unavailable,
                snapshot_fingerprint: None,
                bundle_fingerprint: None,
                sequence: None,
                evidence: None,
                lookups: 0,
            },
            calls: AtomicUsize::new(0),
            rows_seen: AtomicUsize::new(0),
        }
    }
}

impl rust_engineering_application::supply_chain::SupplyCatalogPort for SupplyCatalog {
    fn supply_catalog(
        &self,
        packages: &mut [rust_engineering_domain::supply_chain::SupplyPackage],
        _: &impl Clock,
        _: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::supply_chain::SupplyCatalog, SecurityError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.rows_seen.store(packages.len(), Ordering::SeqCst);
        Ok(self.value.clone())
    }
}

#[derive(Default)]
struct SupplyTestPublisher {
    calls: usize,
    revalidation_attempts: usize,
    revoke_before_revalidate: Option<Arc<AtomicBool>>,
    fail: bool,
}

impl rust_engineering_application::supply_chain::SupplyPublisher for SupplyTestPublisher {
    fn publish_supply(
        &mut self,
        capture: &SecurityCapture,
        observation: &rust_engineering_domain::supply_chain::SupplyObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<QualityArtifactDescriptor, InspectionError> {
        self.calls += 1;
        assert_eq!(capture.source, source());
        assert_eq!(
            observation.report.source_fingerprint,
            source_fingerprint(20)
        );
        if self.fail {
            return Err(InspectionError::Internal);
        }
        if let Some(revoked) = &self.revoke_before_revalidate {
            revoked.store(true, Ordering::SeqCst);
        }
        self.revalidation_attempts += 1;
        let owner = revalidate()?;
        assert_eq!(owner.workspace_root, "/trusted/project");
        Ok(descriptor())
    }
}

fn run_supply(
    registry: &mut ProjectRegistry<Backend, Generator, TestClock>,
    reference: &ProjectRef,
    engine: &SupplyEngine,
    auditor: &Auditor,
    catalog: &SupplyCatalog,
    publisher: &mut SupplyTestPublisher,
    clock: &TestClock,
) -> Result<rust_engineering_application::supply_chain::PublishedSupply, SecurityError> {
    use rust_engineering_application::supply_chain::{SupplyInputs, SupplyPorts};
    registry.supply_chain_durable(
        reference,
        SupplyInputs {
            vendor: Some(&vendor()),
            policy: Some(&policy(false)),
            options: &options(),
        },
        SupplyPorts {
            executor: engine,
            auditor,
            catalog,
            publisher,
        },
        clock,
        &Control::default(),
    )
}

#[test]
fn durable_supply_uses_one_capture_audit_deny_and_facts_then_revalidates_owner() {
    let backend = Backend::default();
    let clock = TestClock::at(100);
    let (mut registry, reference) = open_registry(backend.clone(), &clock);
    let engine = SupplyEngine::new(
        SupplyDenyAnswer::Observation(Box::new(clean_deny())),
        supply_graph(),
    );
    let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
    let catalog = SupplyCatalog::available(&clock, Some(99));
    let mut publisher = SupplyTestPublisher::default();

    let published = run_supply(
        &mut registry,
        &reference,
        &engine,
        &auditor,
        &catalog,
        &mut publisher,
        &clock,
    )
    .unwrap();

    assert!(published.observation.report.complete);
    assert_eq!(published.observation.report.packages_total, 1);
    assert_eq!(published.observation.report.packages_omitted, 0);
    assert_eq!(backend.captures.load(Ordering::SeqCst), 1);
    assert_eq!(engine.inspect_calls.load(Ordering::SeqCst), 1);
    assert_eq!(auditor.calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.deny_calls.load(Ordering::SeqCst), 1);
    assert_eq!(engine.facts_calls.load(Ordering::SeqCst), 1);
    assert!(engine.facts_received_deny.load(Ordering::SeqCst));
    assert_eq!(
        engine.sources.lock().unwrap().as_slice(),
        &[source(), source(), source()]
    );
    assert_eq!(catalog.calls.load(Ordering::SeqCst), 1);
    assert_eq!(catalog.rows_seen.load(Ordering::SeqCst), 1);
    assert_eq!(publisher.calls, 1);
    assert_eq!(publisher.revalidation_attempts, 1);
    assert_eq!(backend.owner_validations.load(Ordering::SeqCst), 1);
    assert_eq!(published.artifact, descriptor());
}

#[test]
fn missing_deny_and_catalog_preserve_audit_and_graph_as_partial() {
    let backend = Backend::default();
    let clock = TestClock::at(100);
    let (mut registry, reference) = open_registry(backend, &clock);
    let engine = SupplyEngine::new(
        SupplyDenyAnswer::Error(SecurityError::MissingOfflineData),
        supply_graph(),
    );
    let expected_audit = audit_observation(&clock, Some(99));
    let auditor = Auditor::new(clock.clone(), expected_audit.clone());
    let catalog = SupplyCatalog::unavailable();
    let mut publisher = SupplyTestPublisher::default();

    let published = run_supply(
        &mut registry,
        &reference,
        &engine,
        &auditor,
        &catalog,
        &mut publisher,
        &clock,
    )
    .unwrap();

    use rust_engineering_domain::supply_chain::SupplyAvailability;
    assert!(!published.observation.report.complete);
    assert_eq!(published.observation.report.packages_total, 1);
    assert_eq!(
        published.observation.report.packages[0].name,
        "captured-member"
    );
    assert_eq!(
        published.observation.report.audit_availability,
        SupplyAvailability::Available
    );
    assert_eq!(
        serde_json::to_value(published.observation.report.audit.as_ref().unwrap()).unwrap(),
        serde_json::to_value(rust_engineering_domain::supply_chain::SupplyAudit::from(
            &expected_audit
        ))
        .unwrap()
    );
    assert_eq!(
        published.observation.report.deny_availability,
        SupplyAvailability::Unavailable
    );
    assert!(published.observation.report.deny.is_none());
    assert_eq!(
        published.observation.report.catalog.availability,
        SupplyAvailability::Unavailable
    );
    assert!(!engine.facts_received_deny.load(Ordering::SeqCst));
}

#[test]
fn stale_or_unknown_supply_evidence_never_marks_report_complete() {
    for stale_audit in [true, false] {
        let clock = TestClock::at(1_000);
        let backend = Backend::default();
        let (mut registry, reference) = open_registry(backend, &clock);
        let audit_created = if stale_audit { Some(0) } else { Some(999) };
        let catalog_created = if stale_audit { Some(999) } else { None };
        let engine = SupplyEngine::new(
            SupplyDenyAnswer::Observation(Box::new(clean_deny())),
            supply_graph(),
        );
        let auditor = Auditor::new(clock.clone(), audit_observation(&clock, audit_created));
        let catalog = SupplyCatalog::available(&clock, catalog_created);
        let mut publisher = SupplyTestPublisher::default();

        let published = run_supply(
            &mut registry,
            &reference,
            &engine,
            &auditor,
            &catalog,
            &mut publisher,
            &clock,
        )
        .unwrap();

        assert!(!published.observation.report.complete);
    }
}

#[test]
fn supply_rejects_owner_revocation_and_publication_failure() {
    for publication_failure in [false, true] {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let (mut registry, reference) = open_registry(backend.clone(), &clock);
        let engine = SupplyEngine::new(
            SupplyDenyAnswer::Observation(Box::new(clean_deny())),
            supply_graph(),
        );
        let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
        let catalog = SupplyCatalog::available(&clock, Some(99));
        let mut publisher = SupplyTestPublisher {
            revoke_before_revalidate: (!publication_failure).then(|| Arc::clone(&backend.revoked)),
            fail: publication_failure,
            ..SupplyTestPublisher::default()
        };

        let result = run_supply(
            &mut registry,
            &reference,
            &engine,
            &auditor,
            &catalog,
            &mut publisher,
            &clock,
        );

        assert_eq!(publisher.calls, 1);
        assert_eq!(
            result.err(),
            Some(if publication_failure {
                SecurityError::Inspection(InspectionError::Internal)
            } else {
                SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::InvalidProject,
                )))
            })
        );
    }
}

#[test]
fn supply_rejects_graph_lock_and_deny_runtime_mismatches_before_publication() {
    for mismatch in 0..3 {
        let backend = Backend::default();
        let clock = TestClock::at(100);
        let (mut registry, reference) = open_registry(backend, &clock);
        let mut graph = supply_graph();
        let mut deny = clean_deny();
        match mismatch {
            0 => graph.source_fingerprint = source_fingerprint(50),
            1 => graph.lock_fingerprint = source_fingerprint(51),
            2 => deny.runtime.image_id = "mismatched-runtime".into(),
            _ => unreachable!(),
        }
        let engine = SupplyEngine::new(SupplyDenyAnswer::Observation(Box::new(deny)), graph);
        let auditor = Auditor::new(clock.clone(), audit_observation(&clock, Some(99)));
        let catalog = SupplyCatalog::available(&clock, Some(99));
        let mut publisher = SupplyTestPublisher::default();

        let result = run_supply(
            &mut registry,
            &reference,
            &engine,
            &auditor,
            &catalog,
            &mut publisher,
            &clock,
        );

        assert_eq!(result.err(), Some(SecurityError::InvalidMetadata));
        assert_eq!(publisher.calls, 0);
    }
}
