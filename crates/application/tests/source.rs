use rust_engineering_application::{
    OperationControl, ProjectBackend, ProjectError, ProjectIdentity, ProjectRegistry,
    ProjectSourceBackend, ReferenceGenerator, RegistryClock, ValidatedProject,
};
use rust_engineering_domain::{OperationalErrorCode, ProjectRef, SourceBundle, SourceFile};
use std::{cell::Cell, rc::Rc};
struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
#[derive(Clone, Default)]
struct Backend {
    calls: Rc<Cell<usize>>,
    changed: Rc<Cell<bool>>,
    change_during_capture: Rc<Cell<bool>>,
    cancel: Rc<Cell<bool>>,
    time: Rc<Cell<u64>>,
    advance: Rc<Cell<bool>>,
}
fn identity(changed: bool) -> Result<ProjectIdentity, ProjectError> {
    Ok(ProjectIdentity {
        workspace_root: "/authority".into(),
        fingerprint: format!("sha256:{:064x}", u8::from(changed))
            .parse()
            .map_err(|_| ProjectError::Internal)?,
    })
}
impl ProjectBackend for Backend {
    type Lease = ();
    fn open(
        &self,
        _: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<()>, ProjectError> {
        Ok(ValidatedProject {
            identity: identity(false)?,
            lease: (),
        })
    }
    fn revalidate(
        &self,
        _: &(),
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        identity(self.changed.get())
    }
}
impl ProjectSourceBackend for Backend {
    fn source(&self, _: &(), _: &dyn OperationControl) -> Result<SourceBundle, ProjectError> {
        self.calls.set(self.calls.get() + 1);
        if self.cancel.get() {
            return Err(ProjectError::Cancelled);
        }
        self.changed.set(self.change_during_capture.get());
        if self.advance.get() {
            self.time.set(20);
        }
        SourceBundle::new(vec![
            SourceFile::new("file".into(), vec![42]).map_err(|_| ProjectError::Internal)?,
        ])
        .map_err(|_| ProjectError::Internal)
    }
}
#[derive(Clone, Default)]
struct Clock(Rc<Cell<u64>>);
impl RegistryClock for Clock {
    fn seconds(&self) -> u64 {
        self.0.get()
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
#[test]
fn source_requires_live_reference_and_revalidates_after_capture() -> Result<(), ProjectError> {
    let backend = Backend::default();
    let clock = Clock::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let reference = Generator.generate()?;
    assert_eq!(
        registry.source(&reference, &Continue),
        Err(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound
        ))
    );
    registry.open("/authority", &Continue)?;
    assert_eq!(
        registry.source(&reference, &Continue)?.files()[0].bytes(),
        &[42]
    );
    backend.change_during_capture.set(true);
    assert_eq!(
        registry.source(&reference, &Continue),
        Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject))
    );
    assert_eq!(
        registry.source(&reference, &Continue),
        Err(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound
        ))
    );
    assert_eq!(backend.calls.get(), 2);
    backend.changed.set(false);
    backend.change_during_capture.set(false);
    registry.open("/authority", &Continue)?;
    clock.0.set(10);
    assert_eq!(
        registry.source(&reference, &Continue),
        Err(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound
        ))
    );
    assert_eq!(backend.calls.get(), 2);
    Ok(())
}
#[test]
fn cancelled_capture_preserves_reference_and_precancel_skips_backend() -> Result<(), ProjectError> {
    let backend = Backend::default();
    let clock = Clock::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/authority", &Continue)?.project_ref;
    backend.cancel.set(true);
    assert_eq!(
        registry.source(&reference, &Continue),
        Err(ProjectError::Cancelled)
    );
    backend.cancel.set(false);
    registry.source(&reference, &Continue)?;
    struct Cancel;
    impl OperationControl for Cancel {
        fn check(&self) -> Result<(), ProjectError> {
            Err(ProjectError::Cancelled)
        }
    }
    assert_eq!(
        registry.source(&reference, &Cancel),
        Err(ProjectError::Cancelled)
    );
    assert_eq!(backend.calls.get(), 2);
    Ok(())
}

#[test]
fn expiry_during_capture_prevents_releasing_bytes() -> Result<(), ProjectError> {
    let backend = Backend::default();
    let clock = Clock(backend.time.clone());
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock, 10, 1)?;
    let reference = registry.open("/authority", &Continue)?.project_ref;
    backend.advance.set(true);
    assert_eq!(
        registry.source(&reference, &Continue),
        Err(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound
        ))
    );
    assert_eq!(backend.calls.get(), 1);
    Ok(())
}

#[test]
fn cancelled_capture_does_not_renew_idle_ttl() -> Result<(), ProjectError> {
    let backend = Backend::default();
    let clock = Clock::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/authority", &Continue)?.project_ref;
    clock.0.set(9);
    backend.cancel.set(true);
    assert_eq!(
        registry.source(&reference, &Continue),
        Err(ProjectError::Cancelled)
    );
    backend.cancel.set(false);
    clock.0.set(10);
    assert_eq!(
        registry.source(&reference, &Continue),
        Err(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound
        ))
    );
    assert_eq!(backend.calls.get(), 1);
    Ok(())
}
