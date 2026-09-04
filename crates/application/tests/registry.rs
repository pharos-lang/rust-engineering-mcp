use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use rust_engineering_application::{
    OperationControl, ProjectBackend, ProjectError, ProjectIdentity, ProjectRegistry,
    ReferenceGenerator, RegistryClock, ValidatedProject,
};
use rust_engineering_domain::{OperationalErrorCode, ProjectRef};

type TestResult = Result<(), ProjectError>;

fn rejected(code: OperationalErrorCode) -> ProjectError {
    ProjectError::Rejected(code)
}

fn reference(number: u128) -> Result<ProjectRef, ProjectError> {
    format!("prj_{number:032x}")
        .parse()
        .map_err(|_| ProjectError::Internal)
}

fn identity(path: &str, revision: u8) -> Result<ProjectIdentity, ProjectError> {
    Ok(ProjectIdentity {
        workspace_root: path.to_owned(),
        fingerprint: format!("sha256:{revision:064x}")
            .parse()
            .map_err(|_| ProjectError::Internal)?,
    })
}

#[derive(Clone, Default)]
struct Control(Arc<AtomicBool>);

impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.0.load(Ordering::SeqCst) {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Default)]
struct Clock(Rc<Cell<u64>>);

impl RegistryClock for Clock {
    fn seconds(&self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone, Default)]
struct Generator {
    next: Rc<Cell<u128>>,
    queued: Rc<RefCell<VecDeque<Result<ProjectRef, ProjectError>>>>,
    calls: Rc<Cell<usize>>,
}

impl ReferenceGenerator for Generator {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        self.calls.set(self.calls.get() + 1);
        if let Some(value) = self.queued.borrow_mut().pop_front() {
            return value;
        }
        let value = self.next.get();
        self.next.set(value + 1);
        reference(value)
    }
}

#[derive(Clone, Default)]
struct Backend {
    opened: Rc<RefCell<Vec<String>>>,
    validated: Rc<RefCell<Vec<ProjectIdentity>>>,
    observation: Rc<RefCell<Option<Result<ProjectIdentity, ProjectError>>>>,
    open_error: Rc<Cell<Option<ProjectError>>>,
    cancel_after_open: Rc<RefCell<Option<Control>>>,
    cancel_after_revalidate: Rc<RefCell<Option<Control>>>,
}

impl ProjectBackend for Backend {
    // An owned snapshot stands in for the adapter's owned directory capability.
    type Lease = ProjectIdentity;

    fn open(
        &self,
        path: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<Self::Lease>, ProjectError> {
        self.opened.borrow_mut().push(path.to_owned());
        if let Some(error) = self.open_error.get() {
            return Err(error);
        }
        if let Some(control) = self.cancel_after_open.borrow().as_ref() {
            control.0.store(true, Ordering::SeqCst);
        }
        let identity = identity(path, 0)?;
        Ok(ValidatedProject {
            lease: identity.clone(),
            identity,
        })
    }

    fn revalidate(
        &self,
        lease: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        self.validated.borrow_mut().push(lease.clone());
        if let Some(control) = self.cancel_after_revalidate.borrow().as_ref() {
            control.0.store(true, Ordering::SeqCst);
        }
        self.observation
            .borrow()
            .clone()
            .unwrap_or_else(|| Ok(lease.clone()))
    }
}

type Registry = ProjectRegistry<Backend, Generator, Clock>;

fn registry(
    backend: &Backend,
    generator: &Generator,
    clock: &Clock,
    ttl: u64,
    capacity: usize,
) -> Result<Registry, ProjectError> {
    ProjectRegistry::new(
        backend.clone(),
        generator.clone(),
        clock.clone(),
        ttl,
        capacity,
    )
}

#[test]
fn colliding_entropy_never_replaces_an_existing_capability() -> TestResult {
    let backend = Backend::default();
    let generator = Generator::default();
    let mut registry = registry(&backend, &generator, &Clock::default(), 30, 64)?;
    let first = registry.open("/host/first", &Control::default())?;
    generator
        .queued
        .borrow_mut()
        .extend([Ok(first.project_ref.clone()), Ok(reference(100)?)]);
    let second = registry.open("/host/second", &Control::default())?;
    assert_eq!(second.project_ref, reference(100)?);
    assert_eq!(
        registry.resolve(&first.project_ref, &Control::default())?,
        first.identity
    );
    assert_eq!(
        registry.resolve(&second.project_ref, &Control::default())?,
        second.identity
    );

    generator
        .queued
        .borrow_mut()
        .extend((0..4).map(|_| Ok(first.project_ref.clone())));
    assert_eq!(
        registry.open("/host/rejected", &Control::default()),
        Err(ProjectError::Internal)
    );
    assert_eq!(generator.calls.get(), 7);
    assert_eq!(
        registry.resolve(&first.project_ref, &Control::default())?,
        first.identity
    );
    assert_eq!(
        registry.resolve(&second.project_ref, &Control::default())?,
        second.identity
    );
    Ok(())
}

#[test]
fn sixty_four_live_handles_fill_capacity_before_backend_access_and_expiry_reclaims_it() -> TestResult
{
    let backend = Backend::default();
    let generator = Generator::default();
    let clock = Clock::default();
    let mut registry = registry(&backend, &generator, &clock, 10, 64)?;
    for index in 0..64 {
        registry.open(&format!("/host/{index}"), &Control::default())?;
    }
    assert_eq!(
        registry.open("/host/overflow", &Control::default()),
        Err(rejected(OperationalErrorCode::SandboxDenied))
    );
    assert_eq!(backend.opened.borrow().len(), 64);
    assert_eq!(generator.calls.get(), 64);
    clock.0.set(10);
    registry.open("/host/after-expiry", &Control::default())?;
    assert_eq!(backend.opened.borrow().len(), 65);
    Ok(())
}

#[test]
fn successful_resolution_refreshes_idle_ttl_and_exact_deadline_expires() -> TestResult {
    let backend = Backend::default();
    let clock = Clock::default();
    let mut registry = registry(&backend, &Generator::default(), &clock, 10, 2)?;
    let active = registry.open("/host/active", &Control::default())?;
    let idle = registry.open("/host/idle", &Control::default())?;
    clock.0.set(9);
    assert_eq!(
        registry.resolve(&active.project_ref, &Control::default())?,
        active.identity
    );
    clock.0.set(10);
    assert_eq!(
        registry.resolve(&idle.project_ref, &Control::default()),
        Err(rejected(OperationalErrorCode::ProjectNotFound))
    );
    clock.0.set(18);
    assert_eq!(
        registry.resolve(&active.project_ref, &Control::default())?,
        active.identity
    );
    clock.0.set(28);
    assert_eq!(
        registry.resolve(&active.project_ref, &Control::default()),
        Err(rejected(OperationalErrorCode::ProjectNotFound))
    );
    assert_eq!(backend.validated.borrow().len(), 2);
    Ok(())
}

#[test]
fn references_have_no_authority_in_another_registry_even_with_identical_backend() -> TestResult {
    let backend = Backend::default();
    let generator = Generator::default();
    let clock = Clock::default();
    let mut first = registry(&backend, &generator, &clock, 30, 1)?;
    let opened = first.open("/host/project", &Control::default())?;
    assert_eq!(
        first.resolve(&reference(999)?, &Control::default()),
        Err(rejected(OperationalErrorCode::ProjectNotFound))
    );
    drop(first);
    let mut restarted = registry(&backend, &generator, &clock, 30, 1)?;
    assert_eq!(
        restarted.resolve(&opened.project_ref, &Control::default()),
        Err(rejected(OperationalErrorCode::ProjectNotFound))
    );
    assert!(backend.validated.borrow().is_empty());
    Ok(())
}

#[test]
fn root_or_fingerprint_change_revokes_reference_permanently() -> TestResult {
    for changed in [
        identity("/host/replaced", 0)?,
        identity("/host/original", 1)?,
    ] {
        let backend = Backend::default();
        let mut registry = registry(&backend, &Generator::default(), &Clock::default(), 30, 1)?;
        let opened = registry.open("/host/original", &Control::default())?;
        *backend.observation.borrow_mut() = Some(Ok(changed));
        assert_eq!(
            registry.resolve(&opened.project_ref, &Control::default()),
            Err(rejected(OperationalErrorCode::InvalidProject))
        );
        *backend.observation.borrow_mut() = None;
        assert_eq!(
            registry.resolve(&opened.project_ref, &Control::default()),
            Err(rejected(OperationalErrorCode::ProjectNotFound))
        );
        assert_eq!(*backend.validated.borrow(), vec![opened.identity]);
        registry.open("/host/replacement", &Control::default())?;
    }
    Ok(())
}

#[test]
fn cancellation_before_open_never_calls_ports_and_after_backend_never_registers() -> TestResult {
    let backend = Backend::default();
    let generator = Generator::default();
    let mut registry = registry(&backend, &generator, &Clock::default(), 30, 1)?;
    let control = Control::default();
    control.0.store(true, Ordering::SeqCst);
    assert_eq!(
        registry.open("/host/project", &control),
        Err(ProjectError::Cancelled)
    );
    assert!(backend.opened.borrow().is_empty());
    assert_eq!(generator.calls.get(), 0);

    control.0.store(false, Ordering::SeqCst);
    *backend.cancel_after_open.borrow_mut() = Some(control.clone());
    assert_eq!(
        registry.open("/host/project", &control),
        Err(ProjectError::Cancelled)
    );
    assert_eq!(backend.opened.borrow().len(), 1);
    control.0.store(false, Ordering::SeqCst);
    assert_eq!(
        registry.resolve(&reference(0)?, &control),
        Err(rejected(OperationalErrorCode::ProjectNotFound))
    );
    *backend.cancel_after_open.borrow_mut() = None;
    registry.open("/host/accepted", &control)?;
    Ok(())
}

#[test]
fn cancelled_resolution_does_not_refresh_idle_deadline() -> TestResult {
    for cancel_in_backend in [false, true] {
        let backend = Backend::default();
        let clock = Clock::default();
        let mut registry = registry(&backend, &Generator::default(), &clock, 10, 1)?;
        let opened = registry.open("/host/project", &Control::default())?;
        let control = Control::default();
        if cancel_in_backend {
            *backend.cancel_after_revalidate.borrow_mut() = Some(control.clone());
        } else {
            *backend.observation.borrow_mut() = Some(Err(ProjectError::Cancelled));
        }
        clock.0.set(9);
        assert_eq!(
            registry.resolve(&opened.project_ref, &control),
            Err(ProjectError::Cancelled)
        );
        control.0.store(false, Ordering::SeqCst);
        *backend.observation.borrow_mut() = None;
        *backend.cancel_after_revalidate.borrow_mut() = None;
        clock.0.set(10);
        assert_eq!(
            registry.resolve(&opened.project_ref, &control),
            Err(rejected(OperationalErrorCode::ProjectNotFound))
        );
        assert_eq!(backend.validated.borrow().len(), 1);
    }
    Ok(())
}

#[test]
fn cancelled_resolve_preserves_lease_and_pre_cancelled_resolve_skips_backend() -> TestResult {
    let backend = Backend::default();
    let mut registry = registry(&backend, &Generator::default(), &Clock::default(), 30, 1)?;
    let opened = registry.open("/host/project", &Control::default())?;
    let control = Control::default();
    control.0.store(true, Ordering::SeqCst);
    assert_eq!(
        registry.resolve(&opened.project_ref, &control),
        Err(ProjectError::Cancelled)
    );
    assert!(backend.validated.borrow().is_empty());
    control.0.store(false, Ordering::SeqCst);
    *backend.observation.borrow_mut() = Some(Err(ProjectError::Cancelled));
    assert_eq!(
        registry.resolve(&opened.project_ref, &control),
        Err(ProjectError::Cancelled)
    );
    *backend.observation.borrow_mut() = None;
    assert_eq!(
        registry.resolve(&opened.project_ref, &control)?,
        opened.identity
    );
    assert_eq!(
        *backend.validated.borrow(),
        vec![opened.identity.clone(), opened.identity]
    );
    Ok(())
}

#[test]
fn rejected_backend_and_failed_entropy_cannot_consume_registry_slots() -> TestResult {
    let backend = Backend::default();
    let generator = Generator::default();
    let mut registry = registry(&backend, &generator, &Clock::default(), 30, 1)?;
    backend
        .open_error
        .set(Some(rejected(OperationalErrorCode::SandboxDenied)));
    assert_eq!(
        registry.open("/outside/root", &Control::default()),
        Err(rejected(OperationalErrorCode::SandboxDenied))
    );
    assert_eq!(generator.calls.get(), 0);
    backend.open_error.set(None);
    generator
        .queued
        .borrow_mut()
        .push_back(Err(ProjectError::Internal));
    assert_eq!(
        registry.open("/host/first", &Control::default()),
        Err(ProjectError::Internal)
    );
    registry.open("/host/second", &Control::default())?;
    Ok(())
}

#[test]
fn revalidation_failure_revokes_lease_and_releases_capacity() -> TestResult {
    let backend = Backend::default();
    let mut registry = registry(&backend, &Generator::default(), &Clock::default(), 30, 1)?;
    let opened = registry.open("/host/project", &Control::default())?;
    *backend.observation.borrow_mut() = Some(Err(rejected(OperationalErrorCode::SandboxDenied)));
    assert_eq!(
        registry.resolve(&opened.project_ref, &Control::default()),
        Err(rejected(OperationalErrorCode::SandboxDenied))
    );
    assert_eq!(
        registry.resolve(&opened.project_ref, &Control::default()),
        Err(rejected(OperationalErrorCode::ProjectNotFound))
    );
    assert_eq!(backend.validated.borrow().len(), 1);
    registry.open("/host/replacement", &Control::default())?;
    Ok(())
}

#[test]
fn malformed_or_oversized_paths_never_reach_backend_and_boundary_is_bytes() -> TestResult {
    let backend = Backend::default();
    let generator = Generator::default();
    let mut registry = registry(&backend, &generator, &Clock::default(), 30, 2)?;
    for path in [
        String::new(),
        "a\0b".to_owned(),
        "a".repeat(4097),
        "é".repeat(2049),
    ] {
        assert_eq!(
            registry.open(&path, &Control::default()),
            Err(rejected(OperationalErrorCode::InvalidProject))
        );
    }
    assert!(backend.opened.borrow().is_empty());
    assert_eq!(generator.calls.get(), 0);
    registry.open(&"a".repeat(4096), &Control::default())?;
    registry.open(&"é".repeat(2048), &Control::default())?;
    assert_eq!(backend.opened.borrow().len(), 2);
    Ok(())
}

#[test]
fn configuration_requires_positive_bounded_ttl_and_capacity() -> TestResult {
    let backend = Backend::default();
    let generator = Generator::default();
    let clock = Clock::default();
    for (ttl, capacity) in [
        (0, 1),
        (86_401, 1),
        (u64::MAX, 1),
        (1, 0),
        (1, 65),
        (1, usize::MAX),
    ] {
        assert!(matches!(
            registry(&backend, &generator, &clock, ttl, capacity),
            Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied))
        ));
    }
    for (ttl, capacity) in [(1, 1), (86_400, 64)] {
        registry(&backend, &generator, &clock, ttl, capacity)?;
    }
    assert!(backend.opened.borrow().is_empty());
    assert_eq!(generator.calls.get(), 0);
    Ok(())
}
