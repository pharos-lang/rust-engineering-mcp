use rust_engineering_application::{
    DependencyAuditPort, ExecutionCancellation, ExecutionError, InspectionControl, InspectionError,
    OperationControl, ProjectAuditError, ProjectBackend, ProjectError, ProjectIdentity,
    ProjectInspectionPort, ProjectRegistry, ProjectSourceBackend, ReferenceGenerator,
    RegistryClock, ValidatedProject,
};
use rust_engineering_domain::{
    AuditDataError, AuditFinding, AuditIssue, AuditObservation, AuditPackage, AuditSource,
    AuditState, CargoConfiguration, Clock, FreshnessPolicy, FreshnessState, IntegrityStatus,
    OperationalErrorCode, ProjectConfigPolicy, ProjectPackage, ProjectRef, ProjectStructure,
    Provenance, RuntimeIdentity, RustEdition, SnapshotEvidence, SourceBundle, SourceFile,
    SourceKind, UnixSeconds,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct Control(AtomicBool);
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.is_cancelled() {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
#[derive(Clone, Default)]
struct TestClock(Rc<Cell<u64>>);
impl RegistryClock for TestClock {
    fn seconds(&self) -> u64 {
        self.0.get()
    }
}
impl Clock for TestClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0.get())
    }
}
#[derive(Clone, Default)]
struct Backend {
    changed: Rc<Cell<bool>>,
    captures: Rc<Cell<usize>>,
    validations: Rc<Cell<usize>>,
    releases: Rc<Cell<usize>>,
    lock_changed: Rc<Cell<bool>>,
}
struct Lease(Rc<Cell<usize>>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn identity(changed: bool) -> Result<ProjectIdentity, ProjectError> {
    Ok(ProjectIdentity {
        workspace_root: "/trusted/project".into(),
        fingerprint: format!("sha256:{:064x}", u8::from(changed))
            .parse()
            .map_err(|_| ProjectError::Internal)?,
    })
}
fn source(changed: bool) -> Result<SourceBundle, ProjectError> {
    SourceBundle::with_directories(
        vec![
            SourceFile::new("Cargo.toml".into(), b"[workspace]\nmembers=[]\n".to_vec())
                .map_err(|_| ProjectError::Internal)?,
            SourceFile::new(
                "Cargo.lock".into(),
                if changed {
                    b"new-lock".to_vec()
                } else {
                    b"captured-lock".to_vec()
                },
            )
            .map_err(|_| ProjectError::Internal)?,
        ],
        vec!["empty".into()],
    )
    .map_err(|_| ProjectError::Internal)
}
impl ProjectBackend for Backend {
    type Lease = Lease;
    fn open(
        &self,
        _: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<Lease>, ProjectError> {
        Ok(ValidatedProject {
            identity: identity(false)?,
            lease: Lease(self.releases.clone()),
        })
    }
    fn revalidate(
        &self,
        _: &Lease,
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        self.validations.set(self.validations.get() + 1);
        identity(self.changed.get())
    }
}
impl ProjectSourceBackend for Backend {
    fn source(&self, _: &Lease, _: &dyn OperationControl) -> Result<SourceBundle, ProjectError> {
        self.captures.set(self.captures.get() + 1);
        source(self.lock_changed.get())
    }
}
struct Generator;
impl ReferenceGenerator for Generator {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        "prj_00000000000000000000000000000001"
            .parse()
            .map_err(|_| ProjectError::Internal)
    }
}
fn structure() -> Result<ProjectStructure, InspectionError> {
    let fingerprint = format!("sha256:{:064x}", 42);
    Ok(ProjectStructure {
        workspace_members: vec![0],
        workspace_default_members: vec![0],
        packages: vec![ProjectPackage {
            package_index: 0,
            name: "captured-member".into(),
            version: "0.1.0".into(),
            manifest_path: "Cargo.toml".into(),
            edition: RustEdition::E2024,
            rust_version: None,
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
        runtime: RuntimeIdentity {
            platform: "linux/arm64".into(),
            image_id: "captured-image".into(),
            configuration_fingerprint: fingerprint
                .parse()
                .map_err(|_| InspectionError::Internal)?,
            execution_fingerprint: fingerprint.parse().map_err(|_| InspectionError::Internal)?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        source_fingerprint: fingerprint.parse().map_err(|_| InspectionError::Internal)?,
    })
}
struct Inspector<F> {
    during: F,
    seen: RefCell<Vec<SourceBundle>>,
}
impl<F: Fn() -> Result<(), InspectionError>> ProjectInspectionPort for Inspector<F> {
    fn inspect(
        &self,
        source: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        self.seen.borrow_mut().push(source.clone());
        (self.during)()?;
        structure()
    }
}
fn inspector<F: Fn() -> Result<(), InspectionError>>(during: F) -> Inspector<F> {
    Inspector {
        during,
        seen: RefCell::new(vec![]),
    }
}
fn missing() -> ProjectError {
    ProjectError::Rejected(OperationalErrorCode::ProjectNotFound)
}

struct Auditor<F> {
    during: F,
    seen: RefCell<Vec<SourceBundle>>,
}
impl<F: Fn() -> Result<AuditObservation, AuditDataError>> DependencyAuditPort for Auditor<F> {
    fn audit(
        &self,
        source: &SourceBundle,
        structure: &ProjectStructure,
        _: &dyn Clock,
        _: &dyn InspectionControl,
    ) -> Result<AuditObservation, AuditDataError> {
        self.seen.borrow_mut().push(source.clone());
        assert_eq!(structure.packages[0].name, "captured-member");
        (self.during)()
    }
}
fn auditor<F: Fn() -> Result<AuditObservation, AuditDataError>>(during: F) -> Auditor<F> {
    Auditor {
        during,
        seen: RefCell::new(vec![]),
    }
}
fn project_error(value: ProjectError) -> ProjectAuditError {
    InspectionError::Project(value).into()
}

#[test]
fn lock_mutation_between_ports_keeps_one_generation_and_conservative_evidence()
-> Result<(), ProjectAuditError> {
    let backend = Backend::default();
    let idle = TestClock::default();
    let wall = TestClock::default();
    wall.0.set(1_000);
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, idle.clone(), 10, 1)
        .map_err(project_error)?;
    let opened = registry
        .open("/trusted/project", &control)
        .map_err(project_error)?;
    idle.0.set(8);
    let inspect = inspector(|| {
        backend.lock_changed.set(true);
        Ok(())
    });
    let audit = auditor(|| {
        idle.0.set(9);
        wall.0.set(1_120);
        Ok(AuditObservation::unavailable())
    });
    let result = registry.audit(&opened.project_ref, &inspect, &audit, &wall, &control)?;
    let captured = source(false).map_err(project_error)?;
    assert_ne!(captured, source(true).map_err(project_error)?);
    assert_eq!(
        inspect.seen.borrow().as_slice(),
        std::slice::from_ref(&captured)
    );
    assert_eq!(audit.seen.borrow().as_slice(), &[captured]);
    assert_eq!(backend.captures.get(), 1);
    assert_eq!(result.project_ref, opened.project_ref);
    assert_eq!(
        result.project_identity_fingerprint,
        opened.identity.fingerprint
    );
    assert_eq!(result.source_fingerprint, structure()?.source_fingerprint);
    assert_eq!(result.runtime.image_id, "captured-image");
    assert!(matches!(
        result.semantics,
        rust_engineering_domain::InspectionSemantics::LatestKnown
    ));
    let provenance = result.evidence.provenance();
    assert_eq!(provenance.source_kind(), SourceKind::ProjectSnapshot);
    assert_eq!(
        provenance.source_id().to_string(),
        result.source_fingerprint.to_string()
    );
    assert_eq!(provenance.created_at(), Some(UnixSeconds(1_000)));
    assert_eq!(provenance.observed_at(), Some(UnixSeconds(1_120)));
    assert_eq!(provenance.integrity(), IntegrityStatus::Verified);
    assert!(!provenance.network_used());
    assert_eq!(result.evidence.freshness().state(), FreshnessState::Aging);
    assert_eq!(result.evidence.freshness().age_seconds(), Some(120));
    assert_eq!(
        result.evidence.freshness().policy().id().to_string(),
        "captured-project-v1"
    );
    assert_eq!(result.evidence.freshness().policy().fresh_for_seconds(), 60);
    assert_eq!(
        result.evidence.freshness().policy().stale_after_seconds(),
        300
    );
    idle.0.set(18);
    registry
        .resolve(&opened.project_ref, &control)
        .map_err(project_error)?;
    Ok(())
}

#[test]
fn invalid_reference_and_precancel_reach_neither_port() -> Result<(), ProjectAuditError> {
    let backend = Backend::default();
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)
        .map_err(project_error)?;
    let inspect = inspector(|| Ok(()));
    let audit = auditor(|| Ok(AuditObservation::unavailable()));
    assert_eq!(
        registry
            .audit(
                &Generator.generate().map_err(project_error)?,
                &inspect,
                &audit,
                &clock,
                &control
            )
            .err(),
        Some(project_error(missing()))
    );
    let reference = registry
        .open("/trusted/project", &control)
        .map_err(project_error)?
        .project_ref;
    control.0.store(true, Ordering::Relaxed);
    assert_eq!(
        registry
            .audit(&reference, &inspect, &audit, &clock, &control)
            .err(),
        Some(project_error(ProjectError::Cancelled))
    );
    assert_eq!(backend.captures.get(), 0);
    assert_eq!(backend.validations.get(), 0);
    assert!(inspect.seen.borrow().is_empty());
    assert!(audit.seen.borrow().is_empty());
    Ok(())
}

#[test]
fn identity_expiry_and_cancellation_after_audit_deny_publication_and_renewal()
-> Result<(), ProjectAuditError> {
    for scenario in 0..3 {
        let backend = Backend::default();
        let clock = TestClock::default();
        let control = Control::default();
        let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)
            .map_err(project_error)?;
        let reference = registry
            .open("/trusted/project", &control)
            .map_err(project_error)?
            .project_ref;
        clock.0.set(9);
        let inspect = inspector(|| Ok(()));
        let audit = auditor(|| {
            match scenario {
                0 => backend.changed.set(true),
                1 => clock.0.set(10),
                _ => control.0.store(true, Ordering::Relaxed),
            }
            Ok(AuditObservation::unavailable())
        });
        let expected = match scenario {
            0 => ProjectError::Rejected(OperationalErrorCode::InvalidProject),
            1 => missing(),
            _ => ProjectError::Cancelled,
        };
        assert_eq!(
            registry
                .audit(&reference, &inspect, &audit, &clock, &control)
                .err(),
            Some(project_error(expected))
        );
        assert_eq!(audit.seen.borrow().len(), 1);
        backend.changed.set(false);
        control.0.store(false, Ordering::Relaxed);
        clock.0.set(10);
        assert_eq!(registry.resolve(&reference, &control), Err(missing()));
        assert_eq!(backend.releases.get(), 1);
    }
    Ok(())
}

#[test]
fn inspection_failure_or_cancellation_never_runs_auditor() -> Result<(), ProjectAuditError> {
    for cancel in [false, true] {
        let backend = Backend::default();
        let clock = TestClock::default();
        let control = Control::default();
        let mut registry = ProjectRegistry::new(backend, Generator, clock.clone(), 10, 1)
            .map_err(project_error)?;
        let reference = registry
            .open("/trusted/project", &control)
            .map_err(project_error)?
            .project_ref;
        clock.0.set(9);
        let inspect = inspector(|| {
            if cancel {
                control.0.store(true, Ordering::Relaxed);
                Ok(())
            } else {
                Err(InspectionError::Execution(ExecutionError::CleanupUncertain))
            }
        });
        let audit = auditor(|| Ok(AuditObservation::unavailable()));
        let expected = if cancel {
            project_error(ProjectError::Cancelled)
        } else {
            InspectionError::Execution(ExecutionError::CleanupUncertain).into()
        };
        assert_eq!(
            registry
                .audit(&reference, &inspect, &audit, &clock, &control)
                .err(),
            Some(expected)
        );
        assert!(audit.seen.borrow().is_empty());
        control.0.store(false, Ordering::Relaxed);
        clock.0.set(10);
        assert_eq!(registry.resolve(&reference, &control), Err(missing()));
    }
    Ok(())
}

#[test]
fn audit_data_errors_preserve_typed_failure_without_renewing_lease() -> Result<(), ProjectAuditError>
{
    for error in [
        AuditDataError::InvalidSnapshot,
        AuditDataError::MissingLockfile,
        AuditDataError::Budget,
        AuditDataError::Cancelled,
    ] {
        let clock = TestClock::default();
        let control = Control::default();
        let mut registry =
            ProjectRegistry::new(Backend::default(), Generator, clock.clone(), 10, 1)
                .map_err(project_error)?;
        let reference = registry
            .open("/trusted/project", &control)
            .map_err(project_error)?
            .project_ref;
        clock.0.set(9);
        let inspect = inspector(|| Ok(()));
        let audit = auditor(|| Err(error));
        assert_eq!(
            registry
                .audit(&reference, &inspect, &audit, &clock, &control)
                .err(),
            Some(ProjectAuditError::Data(error))
        );
        clock.0.set(10);
        assert_eq!(registry.resolve(&reference, &control), Err(missing()));
    }
    Ok(())
}

fn snapshot(clock: &TestClock) -> Result<SnapshotEvidence, ProjectAuditError> {
    let provenance = Provenance::new(
        SourceKind::RustsecSnapshot,
        "snapshot-fixture"
            .parse()
            .map_err(|_| AuditDataError::Internal)?,
        Some(UnixSeconds(0)),
        Some(UnixSeconds(1)),
        IntegrityStatus::Verified,
        false,
    )
    .map_err(|_| AuditDataError::Internal)?;
    let policy = FreshnessPolicy::new(
        "fixture-v1".parse().map_err(|_| AuditDataError::Internal)?,
        60,
        300,
    )
    .map_err(|_| AuditDataError::Internal)?;
    Ok(SnapshotEvidence::assess(provenance, policy, clock))
}

#[test]
fn unavailable_stale_and_finding_failure_are_observations_with_preserved_evidence()
-> Result<(), ProjectAuditError> {
    for scenario in 0..3 {
        let clock = TestClock::default();
        clock.0.set(1_000);
        let control = Control::default();
        let mut registry =
            ProjectRegistry::new(Backend::default(), Generator, clock.clone(), 10, 1)
                .map_err(project_error)?;
        let reference = registry
            .open("/trusted/project", &control)
            .map_err(project_error)?
            .project_ref;
        let mut observation = AuditObservation::unavailable();
        if scenario > 0 {
            observation.snapshot = Some(snapshot(&clock)?);
            observation.snapshot_fingerprint = Some(
                format!("sha256:{:064x}", 11)
                    .parse()
                    .map_err(|_| AuditDataError::Internal)?,
            );
            observation.state = AuditState::Incomplete;
            observation.issue = Some(AuditIssue::SnapshotStale);
        }
        if scenario == 2 {
            observation.state = AuditState::Failed;
            observation.findings.push(AuditFinding {
                advisory_id: "RUSTSEC-2020-0001".into(),
                url: "https://rustsec.org/advisories/RUSTSEC-2020-0001.html".into(),
                title: "Fixture finding".into(),
                package: AuditPackage {
                    name: "affected".into(),
                    version: "1.0.0".into(),
                    source: AuditSource::CratesIo,
                    source_fingerprint: None,
                },
                patched_requirements: vec![">=1.0.1".into()],
                unaffected_requirements: vec![],
                severity: None,
                informational: None,
                paths: vec![],
                paths_omitted: 0,
            });
        }
        let inspect = inspector(|| Ok(()));
        let audit = auditor(|| Ok(observation.clone()));
        let result = registry.audit(&reference, &inspect, &audit, &clock, &control)?;
        assert_eq!(
            serde_json::to_value(&result.observation).map_err(|_| AuditDataError::Internal)?,
            serde_json::to_value(&observation).map_err(|_| AuditDataError::Internal)?
        );
        assert_eq!(
            result.evidence.provenance().source_kind(),
            SourceKind::ProjectSnapshot
        );
        if let Some(evidence) = result.observation.snapshot {
            assert_eq!(evidence.provenance().created_at(), Some(UnixSeconds(0)));
            assert_eq!(evidence.freshness().state(), FreshnessState::Stale);
        }
        assert_eq!(inspect.seen.borrow().len(), 1);
        assert_eq!(audit.seen.borrow().len(), 1);
    }
    Ok(())
}
