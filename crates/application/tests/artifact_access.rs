use rust_engineering_application::{
    ArtifactAccessError, ArtifactInput, ArtifactStore, OperationControl, ProjectBackend,
    ProjectError, ProjectIdentity, ProjectRegistry, ReferenceGenerator, RegistryClock,
    ValidatedProject,
};
use rust_engineering_domain::{
    ArtifactError, ArtifactId, ArtifactMetadata, ArtifactView, ProjectRef,
};
use std::{
    cell::Cell,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct Control(AtomicBool);
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.0.load(Ordering::Relaxed) {
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
fn identity(changed: bool) -> Result<ProjectIdentity, ProjectError> {
    Ok(ProjectIdentity {
        workspace_root: "/trusted".into(),
        fingerprint: format!("sha256:{:064x}", u8::from(changed))
            .parse()
            .map_err(|_| ProjectError::Internal)?,
    })
}
struct Lease(Rc<Cell<usize>>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
struct Backend<F> {
    validate: F,
    drops: Rc<Cell<usize>>,
}
impl<F: Fn() -> Result<ProjectIdentity, ProjectError>> ProjectBackend for Backend<F> {
    type Lease = Lease;
    fn open(
        &self,
        _: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<Lease>, ProjectError> {
        Ok(ValidatedProject {
            identity: identity(false)?,
            lease: Lease(self.drops.clone()),
        })
    }
    fn revalidate(
        &self,
        _: &Lease,
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        (self.validate)()
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
struct Store<F> {
    metadata: ArtifactMetadata,
    bytes: Vec<u8>,
    reads: usize,
    during: F,
}
impl<F: Fn(usize) -> Result<(), ArtifactError>> ArtifactStore for Store<F> {
    fn capture(
        &mut self,
        _: &ProjectRef,
        _: &mut dyn ArtifactInput,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        Err(ArtifactError::InputFailure)
    }
    fn read<'a>(
        &'a mut self,
        owner: &ProjectRef,
        id: &ArtifactId,
    ) -> Result<ArtifactView<'a>, ArtifactError> {
        self.reads += 1;
        (self.during)(self.reads)?;
        if &self.metadata.owner != owner || &self.metadata.id != id {
            return Err(ArtifactError::NotFound);
        }
        Ok(ArtifactView {
            metadata: &self.metadata,
            content: &self.bytes,
        })
    }
    fn remove(&mut self, _: &ProjectRef, _: &ArtifactId) -> Result<bool, ArtifactError> {
        Ok(false)
    }
    fn retain_owners(&mut self, owners: &[ProjectRef]) -> Result<usize, ArtifactError> {
        if !owners.contains(&self.metadata.owner) {
            self.bytes.clear();
        }
        Ok(0)
    }
    fn revoke_owner(&mut self, _: &ProjectRef) -> Result<usize, ArtifactError> {
        Ok(0)
    }
    fn cleanup(&mut self) -> Result<usize, ArtifactError> {
        Ok(0)
    }
}
fn store<F: Fn(usize) -> Result<(), ArtifactError>>(
    size: usize,
    during: F,
) -> Result<Store<F>, ArtifactAccessError> {
    Ok(Store {
        metadata: ArtifactMetadata {
            owner: Generator.generate()?,
            id: "art_00000000000000000000000000000001".parse()?,
            sha256: [7; 32],
            size_bytes: size as u32,
            truncated: false,
            created_seconds: 0,
            expires_seconds: 20,
        },
        bytes: vec![255; size],
        reads: 0,
        during,
    })
}
#[test]
fn bounded_reads_preserve_bytes_metadata_and_never_renew_artifact_retention()
-> Result<(), ArtifactAccessError> {
    for size in [0, 256 * 1024] {
        let clock = Clock::default();
        let control = Control::default();
        let drops = Rc::new(Cell::new(0));
        let backend = Backend {
            validate: || identity(false),
            drops,
        };
        let mut registry = ProjectRegistry::new(backend, Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        let mut store = store(size, |_| Ok(()))?;
        let metadata = store.metadata.clone();
        clock.0.set(9);
        let result =
            registry.read_artifact(&reference, &metadata.id, &mut store, &clock, &control)?;
        assert_eq!(result.content, vec![255; size]);
        assert_eq!(result.metadata, metadata);
        assert_eq!(result.retention_remaining_seconds, 11);
        clock.0.set(18);
        let result =
            registry.read_artifact(&reference, &metadata.id, &mut store, &clock, &control)?;
        assert_eq!(result.retention_remaining_seconds, 2);
        assert_eq!(store.metadata, metadata);
        clock.0.set(20);
        assert_eq!(
            registry
                .read_artifact(&reference, &metadata.id, &mut store, &clock, &control)
                .err(),
            Some(ArtifactAccessError::NotFound)
        );
    }
    Ok(())
}
#[derive(Clone, Copy, Debug)]
enum Case {
    IdentityBefore,
    IdentityAfter,
    ProjectBefore,
    ProjectAfter,
    ArtifactBefore,
    ArtifactAfter,
    ProjectDuringRead,
    Missing,
    Owner,
    Infrastructure,
    Cancel,
    Oversize,
}
#[test]
fn every_failed_authorization_or_copy_preserves_project_ttl_and_error_class()
-> Result<(), ArtifactAccessError> {
    for case in [
        Case::IdentityBefore,
        Case::IdentityAfter,
        Case::ProjectBefore,
        Case::ProjectAfter,
        Case::ArtifactBefore,
        Case::ArtifactAfter,
        Case::ProjectDuringRead,
        Case::Missing,
        Case::Owner,
        Case::Infrastructure,
        Case::Cancel,
        Case::Oversize,
    ] {
        let clock = Clock::default();
        let artifact_clock = Clock::default();
        let control = Control::default();
        let validations = Cell::new(0);
        let drops = Rc::new(Cell::new(0));
        let backend = Backend {
            drops: drops.clone(),
            validate: || {
                validations.set(validations.get() + 1);
                if validations.get() == 2 {
                    match case {
                        Case::ProjectAfter => clock.0.set(10),
                        Case::ArtifactAfter => artifact_clock.0.set(20),
                        _ => {}
                    }
                }
                identity(
                    matches!(case, Case::IdentityBefore)
                        || (matches!(case, Case::IdentityAfter) && validations.get() == 2),
                )
            },
        };
        let mut registry = ProjectRegistry::new(backend, Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        clock.0.set(if matches!(case, Case::ProjectBefore) {
            10
        } else {
            9
        });
        artifact_clock
            .0
            .set(if matches!(case, Case::ArtifactBefore) {
                20
            } else {
                9
            });
        let mut store = store(
            if matches!(case, Case::Oversize) {
                256 * 1024 + 1
            } else {
                3
            },
            |reads| {
                match case {
                    Case::Missing => return Err(ArtifactError::NotFound),
                    Case::Infrastructure => return Err(ArtifactError::InputFailure),
                    Case::ProjectDuringRead if reads == 2 => clock.0.set(10),
                    Case::Cancel if reads == 2 => control.0.store(true, Ordering::Relaxed),
                    _ => {}
                }
                Ok(())
            },
        )?;
        let id = store.metadata.id.clone();
        if matches!(case, Case::Owner) {
            store.metadata.owner = "prj_00000000000000000000000000000002"
                .parse()
                .map_err(|_| ArtifactAccessError::Internal)?;
        }
        let expected = match case {
            Case::Infrastructure | Case::Oversize => ArtifactAccessError::Internal,
            Case::Cancel => ArtifactAccessError::Cancelled,
            _ => ArtifactAccessError::NotFound,
        };
        assert_eq!(
            registry
                .read_artifact(&reference, &id, &mut store, &artifact_clock, &control)
                .err(),
            Some(expected),
            "{case:?}"
        );
        if matches!(case, Case::IdentityBefore | Case::ProjectBefore) {
            assert_eq!(store.reads, 0);
        }
        if matches!(case, Case::IdentityAfter) {
            assert_eq!(drops.get(), 1);
        }
        control.0.store(false, Ordering::Relaxed);
        clock.0.set(10);
        assert!(
            registry.resolve(&reference, &control).is_err(),
            "failure renewed lease: {case:?}"
        );
        assert_eq!(drops.get(), 1, "{case:?}");
    }
    Ok(())
}
#[test]
fn unknown_reference_and_precancel_do_not_read_storage() -> Result<(), ArtifactAccessError> {
    let clock = Clock::default();
    let control = Control::default();
    let backend = Backend {
        validate: || identity(false),
        drops: Rc::new(Cell::new(0)),
    };
    let mut registry = ProjectRegistry::new(backend, Generator, clock.clone(), 10, 1)?;
    let reference = Generator.generate()?;
    let mut store = store(1, |_| Ok(()))?;
    let id = store.metadata.id.clone();
    assert_eq!(
        registry
            .read_artifact(&reference, &id, &mut store, &clock, &control)
            .err(),
        Some(ArtifactAccessError::NotFound)
    );
    registry.open("/trusted", &control)?;
    control.0.store(true, Ordering::Relaxed);
    assert_eq!(
        registry
            .read_artifact(&reference, &id, &mut store, &clock, &control)
            .err(),
        Some(ArtifactAccessError::Cancelled)
    );
    assert_eq!(store.reads, 0);
    Ok(())
}

#[test]
fn invalidated_owner_is_purged_without_reading_its_artifact() -> Result<(), ArtifactAccessError> {
    let clock = Clock::default();
    let control = Control::default();
    let changed = Cell::new(false);
    let backend = Backend {
        validate: || identity(changed.get()),
        drops: Rc::new(Cell::new(0)),
    };
    let mut registry = ProjectRegistry::new(backend, Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/trusted", &control)?.project_ref;
    let mut store = store(8, |_| Ok(()))?;
    let id = store.metadata.id.clone();
    changed.set(true);
    assert_eq!(
        registry
            .read_artifact(&reference, &id, &mut store, &clock, &control)
            .err(),
        Some(ArtifactAccessError::NotFound)
    );
    assert!(store.bytes.is_empty());
    assert_eq!(store.reads, 0);
    Ok(())
}
