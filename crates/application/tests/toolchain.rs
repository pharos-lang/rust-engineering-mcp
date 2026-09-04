use rust_engineering_application::{
    ExecutionCancellation, ExecutionError, InspectionControl, InspectionError, OperationControl,
    ProjectBackend, ProjectError, ProjectIdentity, ProjectRegistry, ProjectSourceBackend,
    ReferenceGenerator, RegistryClock, ToolchainInspectionPort, ValidatedProject,
};
use rust_engineering_domain::{
    Clock, FreshnessState, InstalledComponent, InstalledComponentKind, IntegrityStatus,
    OperationalErrorCode, ProjectRef, SourceBundle, SourceFile, SourceKind, ToolchainChannel,
    ToolchainExecution, ToolchainInventory, ToolchainObservation, ToolchainObservationCommand,
    ToolchainRuntime, UnixSeconds,
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
fn observation() -> Result<ToolchainObservation, InspectionError> {
    let commands = [
        ToolchainObservationCommand::CompilerVersion,
        ToolchainObservationCommand::CargoVersion,
        ToolchainObservationCommand::InstalledComponents,
    ];
    let executions = commands
        .into_iter()
        .enumerate()
        .map(|(index, command)| {
            Ok(ToolchainExecution {
                command,
                execution_fingerprint: format!("sha256:{:064x}", index + 10)
                    .parse()
                    .map_err(|_| InspectionError::Internal)?,
            })
        })
        .collect::<Result<Vec<_>, InspectionError>>()?;
    Ok(ToolchainObservation {
        inventory: ToolchainInventory {
            rustc_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            channel: ToolchainChannel::Stable,
            host_triple: "aarch64-unknown-linux-gnu".into(),
            installed_targets: vec!["aarch64-unknown-linux-gnu".into()],
            installed_components: vec![InstalledComponent {
                component: InstalledComponentKind::Rustc,
                target: Some("aarch64-unknown-linux-gnu".into()),
            }],
        },
        runtime: ToolchainRuntime {
            platform: "linux/arm64".into(),
            image_id: "captured-image".into(),
            configuration_fingerprint: format!("sha256:{:064x}", 20)
                .parse()
                .map_err(|_| InspectionError::Internal)?,
            executions,
        },
        source_fingerprint: format!("sha256:{:064x}", 30)
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        declared_toolchain: Some("1.98.1".into()),
    })
}
struct Inspector<F> {
    during: F,
    seen: RefCell<Vec<SourceBundle>>,
}
impl<F: Fn() -> Result<(), InspectionError>> ToolchainInspectionPort for Inspector<F> {
    fn inspect_toolchain(
        &self,
        source: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ToolchainObservation, InspectionError> {
        self.seen.borrow_mut().push(source.clone());
        // Models a complete observation held privately until all probes succeed.
        let observed = observation()?;
        (self.during)()?;
        Ok(observed)
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
fn complete_inventory_and_distinct_execution_evidence_survive_publication()
-> Result<(), InspectionError> {
    let backend = Backend::default();
    let idle = TestClock::default();
    let wall = TestClock::default();
    wall.0.set(1000);
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, idle.clone(), 10, 1)?;
    let opened = registry.open("/trusted/project", &control)?;
    idle.0.set(8);
    let port = inspector(|| {
        idle.0.set(9);
        wall.0.set(1120);
        Ok(())
    });
    let result = registry.inspect_toolchain(&opened.project_ref, &port, &wall, &control)?;
    assert_eq!(port.seen.borrow().as_slice(), &[source()?]);
    assert_eq!(result.project_ref, opened.project_ref);
    assert_eq!(
        result.project_identity_fingerprint,
        opened.identity.fingerprint
    );
    let expected = observation()?;
    assert_eq!(result.observation.inventory, expected.inventory);
    assert_eq!(
        serde_json::to_value(&result.observation).map_err(|_| InspectionError::Internal)?,
        serde_json::to_value(expected).map_err(|_| InspectionError::Internal)?
    );
    let executions = &result.observation.runtime.executions;
    assert_eq!(executions.len(), 3);
    for (index, command) in [
        ToolchainObservationCommand::CompilerVersion,
        ToolchainObservationCommand::CargoVersion,
        ToolchainObservationCommand::InstalledComponents,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(executions[index].command, command);
        assert_eq!(
            executions[index].execution_fingerprint.to_string(),
            format!("sha256:{:064x}", index + 10)
        );
    }
    assert_eq!(
        serde_json::to_value(result.semantics).map_err(|_| InspectionError::Internal)?,
        "latest_known"
    );
    let provenance = result.evidence.provenance();
    assert_eq!(provenance.source_kind(), SourceKind::ProjectSnapshot);
    assert_eq!(
        provenance.source_id().to_string(),
        result.observation.source_fingerprint.to_string()
    );
    assert_eq!(provenance.created_at(), Some(UnixSeconds(1000)));
    assert_eq!(provenance.observed_at(), Some(UnixSeconds(1120)));
    assert_eq!(provenance.integrity(), IntegrityStatus::Verified);
    assert!(!provenance.network_used());
    assert_eq!(result.evidence.freshness().state(), FreshnessState::Aging);
    assert_eq!(result.evidence.freshness().age_seconds(), Some(120));
    // Capturing at t=8 would expire at t=18; publication at t=9 renews instead.
    idle.0.set(18);
    registry.resolve(&opened.project_ref, &control)?;
    assert_eq!(backend.releases.get(), 0);
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Failure {
    Partial,
    Cleanup,
    CancelledPort,
    CancelledControl,
    Expired,
    Changed,
}
#[test]
fn incomplete_or_invalidated_observation_never_publishes_or_renews() -> Result<(), InspectionError>
{
    for case in [
        Failure::Partial,
        Failure::Cleanup,
        Failure::CancelledPort,
        Failure::CancelledControl,
        Failure::Expired,
        Failure::Changed,
    ] {
        let backend = Backend::default();
        let clock = TestClock::default();
        let control = Control::default();
        let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted/project", &control)?.project_ref;
        clock.0.set(9);
        let port = inspector(|| match case {
            Failure::Partial => Err(InspectionError::InvalidMetadata),
            Failure::Cleanup => Err(InspectionError::Execution(ExecutionError::CleanupUncertain)),
            Failure::CancelledPort => Err(InspectionError::Execution(ExecutionError::Cancelled)),
            Failure::CancelledControl => {
                control.0.store(true, Ordering::Relaxed);
                Ok(())
            }
            Failure::Expired => {
                clock.0.set(10);
                Ok(())
            }
            Failure::Changed => {
                backend.changed.set(true);
                Ok(())
            }
        });
        let expected = match case {
            Failure::Partial => InspectionError::InvalidMetadata,
            Failure::Cleanup => InspectionError::Execution(ExecutionError::CleanupUncertain),
            Failure::CancelledPort => InspectionError::Execution(ExecutionError::Cancelled),
            Failure::CancelledControl => ProjectError::Cancelled.into(),
            Failure::Expired => missing().into(),
            Failure::Changed => ProjectError::Rejected(OperationalErrorCode::InvalidProject).into(),
        };
        assert_eq!(
            registry
                .inspect_toolchain(&reference, &port, &clock, &control)
                .err(),
            Some(expected),
            "{case:?}"
        );
        assert_eq!(port.seen.borrow().as_slice(), &[source()?], "{case:?}");
        assert_eq!(backend.captures.get(), 1);
        let invalidated = matches!(case, Failure::Expired | Failure::Changed);
        assert_eq!(backend.releases.get(), usize::from(invalidated), "{case:?}");
        control.0.store(false, Ordering::Relaxed);
        backend.changed.set(false);
        // Restoring identity before TTL expiry cannot resurrect the old lease.
        if matches!(case, Failure::Changed) {
            assert_eq!(registry.resolve(&reference, &control), Err(missing()));
        }
        clock.0.set(10);
        let validations = backend.validations.get();
        assert_eq!(
            registry.resolve(&reference, &control),
            Err(missing()),
            "{case:?}"
        );
        assert_eq!(backend.validations.get(), validations);
        assert_eq!(backend.releases.get(), 1);
    }
    Ok(())
}

#[test]
fn missing_reference_and_precancellation_skip_both_effect_ports() -> Result<(), InspectionError> {
    let backend = Backend::default();
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
    let port = inspector(|| Ok(()));
    assert_eq!(
        registry
            .inspect_toolchain(&Generator.generate()?, &port, &clock, &control)
            .err(),
        Some(missing().into())
    );
    let reference = registry.open("/trusted/project", &control)?.project_ref;
    control.0.store(true, Ordering::Relaxed);
    assert_eq!(
        registry
            .inspect_toolchain(&reference, &port, &clock, &control)
            .err(),
        Some(ProjectError::Cancelled.into())
    );
    assert!(port.seen.borrow().is_empty());
    assert_eq!(backend.captures.get(), 0);
    assert_eq!(backend.validations.get(), 0);
    Ok(())
}
