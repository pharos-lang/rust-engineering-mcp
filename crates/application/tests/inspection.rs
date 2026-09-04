use rust_engineering_application::{
    ExecutionCancellation, ExecutionError, InspectionControl, InspectionError, OperationControl,
    ProjectBackend, ProjectError, ProjectIdentity, ProjectInspectionPort, ProjectRegistry,
    ProjectSourceBackend, ReferenceGenerator, RegistryClock, ValidatedProject,
};
use rust_engineering_domain::{
    CargoConfiguration, Clock, FreshnessState, IntegrityStatus, OperationalErrorCode,
    ProjectConfigPolicy, ProjectPackage, ProjectRef, ProjectStructure, RuntimeIdentity,
    RustEdition, SourceBundle, SourceFile, SourceKind, UnixSeconds,
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
fn source() -> Result<SourceBundle, ProjectError> {
    SourceBundle::with_directories(
        vec![
            SourceFile::new("Cargo.toml".into(), b"[workspace]\nmembers=[]\n".to_vec())
                .map_err(|_| ProjectError::Internal)?,
            SourceFile::new("data/raw.bin".into(), vec![0, 255, 13, 10])
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
        source()
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

#[test]
fn success_preserves_capture_and_reports_aged_latest_known_evidence() -> Result<(), InspectionError>
{
    let backend = Backend::default();
    let idle = TestClock::default();
    let wall = TestClock::default();
    wall.0.set(1_000);
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, idle.clone(), 10, 1)?;
    let opened = registry.open("/trusted/project", &control)?;
    idle.0.set(8);
    let port = inspector(|| {
        idle.0.set(9);
        wall.0.set(1_120);
        Ok(())
    });
    let result = registry.inspect(&opened.project_ref, &port, &wall, &control)?;
    assert_eq!(port.seen.borrow().as_slice(), &[source()?]);
    assert_eq!(backend.captures.get(), 1);
    assert_eq!(result.project_ref, opened.project_ref);
    assert_eq!(
        result.project_identity_fingerprint,
        opened.identity.fingerprint
    );
    assert_eq!(
        serde_json::to_value(result.semantics).map_err(|_| InspectionError::Internal)?,
        "latest_known"
    );
    assert_eq!(
        serde_json::to_value(&result.structure).map_err(|_| InspectionError::Internal)?,
        serde_json::to_value(structure()?).map_err(|_| InspectionError::Internal)?
    );
    let provenance = result.evidence.provenance();
    assert_eq!(provenance.source_kind(), SourceKind::ProjectSnapshot);
    assert_eq!(
        provenance.source_id().to_string(),
        result.structure.source_fingerprint.to_string()
    );
    assert_eq!(provenance.created_at(), Some(UnixSeconds(1_000)));
    assert_eq!(provenance.observed_at(), Some(UnixSeconds(1_120)));
    assert_eq!(provenance.integrity(), IntegrityStatus::Verified);
    assert!(!provenance.network_used());
    let freshness = result.evidence.freshness();
    assert_eq!(freshness.state(), FreshnessState::Aging);
    assert_eq!(freshness.age_seconds(), Some(120));
    assert_eq!(freshness.assessed_at(), UnixSeconds(1_120));
    assert_eq!(freshness.policy().id().to_string(), "captured-project-v1");
    assert_eq!(freshness.policy().fresh_for_seconds(), 60);
    assert_eq!(freshness.policy().stale_after_seconds(), 300);
    // Renewal occurs after the inspector completes, not at capture (second 8).
    idle.0.set(18);
    registry.resolve(&opened.project_ref, &control)?;
    assert_eq!(backend.releases.get(), 0);
    Ok(())
}

#[test]
fn invalid_reference_and_precancel_never_reach_capture_or_inspector() -> Result<(), InspectionError>
{
    let backend = Backend::default();
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let port = inspector(|| Ok(()));
    assert_eq!(
        registry
            .inspect(&Generator.generate()?, &port, &clock, &control)
            .err(),
        Some(missing().into())
    );
    let reference = registry.open("/trusted/project", &control)?.project_ref;
    control.0.store(true, Ordering::Relaxed);
    assert_eq!(
        registry.inspect(&reference, &port, &clock, &control).err(),
        Some(ProjectError::Cancelled.into())
    );
    assert_eq!(backend.validations.get(), 0);
    assert_eq!(backend.captures.get(), 0);
    assert!(port.seen.borrow().is_empty());
    Ok(())
}

#[test]
fn ttl_expiring_inside_inspector_releases_lease_without_publication() -> Result<(), InspectionError>
{
    let backend = Backend::default();
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/trusted/project", &control)?.project_ref;
    clock.0.set(9);
    let port = inspector(|| {
        clock.0.set(10);
        Ok(())
    });
    assert_eq!(
        registry.inspect(&reference, &port, &clock, &control).err(),
        Some(missing().into())
    );
    assert_eq!(port.seen.borrow().as_slice(), &[source()?]);
    assert_eq!(backend.releases.get(), 1);
    let validations = backend.validations.get();
    assert_eq!(registry.resolve(&reference, &control), Err(missing()));
    assert_eq!(backend.validations.get(), validations);
    Ok(())
}

#[test]
fn identity_change_inside_inspector_permanently_removes_reference() -> Result<(), InspectionError> {
    let backend = Backend::default();
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/trusted/project", &control)?.project_ref;
    let port = inspector(|| {
        backend.changed.set(true);
        Ok(())
    });
    assert_eq!(
        registry.inspect(&reference, &port, &clock, &control).err(),
        Some(ProjectError::Rejected(OperationalErrorCode::InvalidProject).into())
    );
    assert_eq!(port.seen.borrow().as_slice(), &[source()?]);
    assert_eq!(backend.releases.get(), 1);
    backend.changed.set(false);
    assert_eq!(registry.resolve(&reference, &control), Err(missing()));
    Ok(())
}

#[test]
fn inspector_failure_preserves_error_without_renewing_capture_ttl() -> Result<(), InspectionError> {
    let backend = Backend::default();
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/trusted/project", &control)?.project_ref;
    clock.0.set(9);
    let error = InspectionError::Execution(ExecutionError::CleanupUncertain);
    let port = inspector(|| Err(error));
    assert_eq!(
        registry.inspect(&reference, &port, &clock, &control).err(),
        Some(error)
    );
    assert_eq!(port.seen.borrow().as_slice(), &[source()?]);
    assert_eq!(backend.releases.get(), 0);
    clock.0.set(10);
    assert_eq!(registry.resolve(&reference, &control), Err(missing()));
    assert_eq!(backend.releases.get(), 1);
    Ok(())
}

#[test]
fn cancellation_inside_successful_inspector_suppresses_result_and_ttl_renewal()
-> Result<(), InspectionError> {
    let backend = Backend::default();
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/trusted/project", &control)?.project_ref;
    clock.0.set(9);
    let port = inspector(|| {
        control.0.store(true, Ordering::Relaxed);
        Ok(())
    });
    assert_eq!(
        registry.inspect(&reference, &port, &clock, &control).err(),
        Some(ProjectError::Cancelled.into())
    );
    assert_eq!(port.seen.borrow().as_slice(), &[source()?]);
    assert_eq!(backend.releases.get(), 0);
    control.0.store(false, Ordering::Relaxed);
    clock.0.set(10);
    assert_eq!(registry.resolve(&reference, &control), Err(missing()));
    assert_eq!(backend.releases.get(), 1);
    Ok(())
}
