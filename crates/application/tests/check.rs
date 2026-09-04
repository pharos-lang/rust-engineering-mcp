use rust_engineering_application::*;
use rust_engineering_domain::*;
use std::{
    cell::Cell,
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct Control(AtomicBool);
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.is_cancelled() {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
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
struct Backend(Rc<Cell<bool>>);
impl ProjectBackend for Backend {
    type Lease = ();
    fn open(
        &self,
        _: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<()>, ProjectError> {
        Ok(ValidatedProject {
            identity: self.revalidate(&(), &Control::default())?,
            lease: (),
        })
    }
    fn revalidate(
        &self,
        _: &(),
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        Ok(ProjectIdentity {
            workspace_root: "/trusted".into(),
            fingerprint: format!("sha256:{:064x}", u8::from(self.0.get()))
                .parse()
                .map_err(|_| ProjectError::Internal)?,
        })
    }
}
impl ProjectSourceBackend for Backend {
    fn source(&self, _: &(), _: &dyn OperationControl) -> Result<SourceBundle, ProjectError> {
        SourceBundle::new(vec![
            SourceFile::new("Cargo.toml".into(), b"[workspace]\n".to_vec())
                .map_err(|_| ProjectError::Internal)?,
        ])
        .map_err(|_| ProjectError::Internal)
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
struct Checker<F>(F, Cell<usize>);
impl<F: Fn() -> Result<CheckObservation, InspectionError>> ProjectCheckPort for Checker<F> {
    fn check(
        &self,
        source: &SourceBundle,
        _: &CheckOptions,
        _: &dyn InspectionControl,
    ) -> Result<CheckObservation, InspectionError> {
        self.1.set(self.1.get() + 1);
        assert_eq!(source.files()[0].bytes(), b"[workspace]\n");
        (self.0)()
    }
}
fn observation() -> Result<CheckObservation, InspectionError> {
    let fp = format!("sha256:{:064x}", 42);
    Ok(CheckObservation {
        outcome: CheckOutcome::Failed,
        termination: ExecutionTermination::Exited,
        exit_code: Some(101),
        validation_complete: true,
        diagnostics: vec![],
        diagnostics_omitted: 0,
        stdout: "compiler record".into(),
        stderr: "compiler failed".into(),
        stdout_truncated: true,
        stderr_truncated: false,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: fp.clone(),
            configuration_fingerprint: fp.parse().map_err(|_| InspectionError::Internal)?,
            execution_fingerprint: fp.parse().map_err(|_| InspectionError::Internal)?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        source_fingerprint: fp.parse().map_err(|_| InspectionError::Internal)?,
    })
}
struct Store<'a> {
    entries: Vec<(ArtifactMetadata, Vec<u8>)>,
    clock: TestClock,
    after_capture: Option<Box<dyn Fn() + 'a>>,
    captures: usize,
    removes: usize,
    capture_error: Option<ArtifactError>,
    remove_error: bool,
}
impl Store<'_> {
    fn new(clock: TestClock) -> Self {
        Self {
            entries: vec![],
            clock,
            after_capture: None,
            captures: 0,
            removes: 0,
            capture_error: None,
            remove_error: false,
        }
    }
}
impl ArtifactStore for Store<'_> {
    fn capture(
        &mut self,
        owner: &ProjectRef,
        input: &mut dyn ArtifactInput,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        self.captures += 1;
        if let Some(error) = self.capture_error {
            if let Some(callback) = &self.after_capture {
                callback();
            }
            return Err(error);
        }
        let mut bytes = vec![];
        let mut buffer = [0; 4096];
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..n]);
        }
        let now = self.clock.seconds();
        let metadata = ArtifactMetadata {
            owner: owner.clone(),
            id: format!("art_{:032x}", self.captures).parse()?,
            sha256: [0; 32],
            size_bytes: bytes.len() as u32,
            truncated: input.truncated(),
            created_seconds: now,
            expires_seconds: now + 20,
        };
        self.entries.push((metadata.clone(), bytes));
        if let Some(callback) = &self.after_capture {
            callback();
        }
        Ok(metadata)
    }
    fn read<'a>(
        &'a mut self,
        owner: &ProjectRef,
        id: &ArtifactId,
    ) -> Result<ArtifactView<'a>, ArtifactError> {
        let entry = self
            .entries
            .iter()
            .find(|(m, _)| {
                &m.owner == owner && &m.id == id && m.expires_seconds > self.clock.seconds()
            })
            .ok_or(ArtifactError::NotFound)?;
        Ok(ArtifactView {
            metadata: &entry.0,
            content: &entry.1,
        })
    }
    fn remove(&mut self, owner: &ProjectRef, id: &ArtifactId) -> Result<bool, ArtifactError> {
        self.removes += 1;
        if self.remove_error {
            return Err(ArtifactError::ClockRegression);
        }
        let n = self.entries.len();
        self.entries
            .retain(|(m, _)| &m.owner != owner || &m.id != id);
        Ok(n != self.entries.len())
    }
    fn retain_owners(&mut self, owners: &[ProjectRef]) -> Result<usize, ArtifactError> {
        let before = self.entries.len();
        self.entries.retain(|(m, _)| owners.contains(&m.owner));
        Ok(before - self.entries.len())
    }
    fn revoke_owner(&mut self, _: &ProjectRef) -> Result<usize, ArtifactError> {
        Err(ArtifactError::InputFailure)
    }
    fn cleanup(&mut self) -> Result<usize, ArtifactError> {
        Ok(0)
    }
}
fn options() -> Result<CheckOptions, InspectionError> {
    CheckSelection::default()
        .try_into()
        .map_err(|_| InspectionError::Internal)
}

#[test]
fn compiler_failure_is_valid_and_publishes_one_log_with_no_raw_streams()
-> Result<(), InspectionError> {
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(
        Backend(Rc::new(Cell::new(false))),
        Generator,
        clock.clone(),
        10,
        1,
    )?;
    let reference = registry.open("/trusted", &control)?.project_ref;
    clock.0.set(9);
    let mut store = Store::new(clock.clone());
    let checker = Checker(observation, Cell::new(0));
    let checked = registry.check(
        &reference,
        &options()?,
        &checker,
        &mut store,
        (&clock, &clock),
        &control,
    )?;
    assert_eq!(checked.observation.outcome, CheckOutcome::Failed);
    assert_eq!(checked.observation.exit_code, Some(101));
    assert!(checked.observation.stdout.is_empty() && checked.observation.stderr.is_empty());
    assert!(checked.observation.stdout_truncated);
    assert_eq!(checked.retention_remaining_seconds, Some(20));
    assert_eq!(store.entries.len(), 1);
    assert_eq!(
        store.entries[0].1,
        b"=== stdout ===\ncompiler record\n[stream truncated]\n=== stderr ===\ncompiler failed"
    );
    assert_eq!(
        checked.evidence.provenance().source_kind(),
        SourceKind::ProjectSnapshot
    );
    clock.0.set(18);
    assert!(registry.resolve(&reference, &control).is_ok());
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Failure {
    ExpiredDuringCheck,
    IdentityDuringCheck,
    CancelAfterCapture,
    RetentionAfterCapture,
    RollbackError,
    Internal,
    Output,
}
#[test]
fn failed_publications_rollback_only_new_artifact_and_do_not_renew_lease()
-> Result<(), InspectionError> {
    for case in [
        Failure::ExpiredDuringCheck,
        Failure::IdentityDuringCheck,
        Failure::CancelAfterCapture,
        Failure::RetentionAfterCapture,
        Failure::RollbackError,
        Failure::Internal,
        Failure::Output,
    ] {
        let clock = TestClock::default();
        let artifact_clock = TestClock::default();
        let changed = Rc::new(Cell::new(false));
        let control = Control::default();
        let mut registry =
            ProjectRegistry::new(Backend(changed.clone()), Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        clock.0.set(9);
        artifact_clock.0.set(9);
        let checker = Checker(
            || {
                match case {
                    Failure::ExpiredDuringCheck => clock.0.set(10),
                    Failure::IdentityDuringCheck => changed.set(true),
                    _ => {}
                }
                let mut value = observation()?;
                if matches!(case, Failure::Output) {
                    value.stdout = "x".repeat(256 * 1024 + 1);
                }
                Ok(value)
            },
            Cell::new(0),
        );
        let mut store = Store::new(artifact_clock.clone());
        let old = ArtifactMetadata {
            owner: reference.clone(),
            id: "art_00000000000000000000000000000000"
                .parse()
                .map_err(|_| InspectionError::Internal)?,
            sha256: [1; 32],
            size_bytes: 3,
            truncated: false,
            created_seconds: 0,
            expires_seconds: 100,
        };
        store.entries.push((old.clone(), b"old".to_vec()));
        store.after_capture = Some(Box::new(|| match case {
            Failure::CancelAfterCapture | Failure::RollbackError => {
                control.0.store(true, Ordering::Relaxed)
            }
            Failure::RetentionAfterCapture => artifact_clock.0.set(29),
            _ => {}
        }));
        store.remove_error = matches!(case, Failure::RollbackError);
        store.capture_error = match case {
            Failure::Internal => Some(ArtifactError::InputFailure),
            _ => None,
        };
        let result = registry.check(
            &reference,
            &options()?,
            &checker,
            &mut store,
            (&clock, &artifact_clock),
            &control,
        );
        let expected = match case {
            Failure::CancelAfterCapture => InspectionError::Project(ProjectError::Cancelled),
            Failure::RollbackError | Failure::Internal => InspectionError::Internal,
            Failure::Output => InspectionError::OutputLimit,
            _ => InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            )),
        };
        assert_eq!(result.err(), Some(expected), "{case:?}");
        let retired = matches!(
            case,
            Failure::ExpiredDuringCheck | Failure::IdentityDuringCheck
        );
        assert_eq!(
            store.entries.iter().any(|(m, b)| m == &old && b == b"old"),
            !retired
        );
        if !matches!(case, Failure::RollbackError) {
            assert_eq!(store.entries.len(), usize::from(!retired), "{case:?}");
        }
        assert_eq!(
            store.removes,
            usize::from(matches!(
                case,
                Failure::ExpiredDuringCheck
                    | Failure::IdentityDuringCheck
                    | Failure::CancelAfterCapture
                    | Failure::RetentionAfterCapture
                    | Failure::RollbackError
            )),
            "{case:?}"
        );
        control.0.store(false, Ordering::Relaxed);
        clock.0.set(10);
        assert!(
            registry.resolve(&reference, &control).is_err(),
            "lease renewed: {case:?}"
        );
    }
    Ok(())
}
#[test]
fn precancel_and_unknown_reference_never_invoke_checker_or_store() -> Result<(), InspectionError> {
    let clock = TestClock::default();
    let control = Control::default();
    let checker = Checker(observation, Cell::new(0));
    let mut store = Store::new(clock.clone());
    let mut registry = ProjectRegistry::new(
        Backend(Rc::new(Cell::new(false))),
        Generator,
        clock.clone(),
        10,
        1,
    )?;
    let reference = Generator.generate()?;
    assert!(
        registry
            .check(
                &reference,
                &options()?,
                &checker,
                &mut store,
                (&clock, &clock),
                &control
            )
            .is_err()
    );
    registry.open("/trusted", &control)?;
    control.0.store(true, Ordering::Relaxed);
    assert_eq!(
        registry
            .check(
                &reference,
                &options()?,
                &checker,
                &mut store,
                (&clock, &clock),
                &control
            )
            .err(),
        Some(InspectionError::Project(ProjectError::Cancelled))
    );
    assert_eq!(checker.1.get(), 0);
    assert_eq!(store.captures, 0);
    Ok(())
}

#[test]
fn large_streams_keep_both_sections_and_utf8_without_invalidating_completion()
-> Result<(), InspectionError> {
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(
        Backend(Rc::new(Cell::new(false))),
        Generator,
        clock.clone(),
        10,
        1,
    )?;
    let reference = registry.open("/trusted", &control)?.project_ref;
    let checker = Checker(
        || {
            let mut observed = observation()?;
            observed.stdout = "€".repeat(80_000);
            observed.stderr = "診".repeat(80_000);
            observed.stdout_truncated = false;
            Ok(observed)
        },
        Cell::new(0),
    );
    let mut store = Store::new(clock.clone());
    let checked = registry.check(
        &reference,
        &options()?,
        &checker,
        &mut store,
        (&clock, &clock),
        &control,
    )?;
    let bytes = &store.entries[0].1;
    let log = std::str::from_utf8(bytes).map_err(|_| InspectionError::Internal)?;
    assert!(bytes.len() < 256 * 1024);
    assert!(log.starts_with("=== stdout ===\n€"));
    assert!(log.contains("\n=== stderr ===\n診"));
    assert_eq!(log.matches("[stream truncated]").count(), 2);
    assert!(checked.log.as_ref().is_some_and(|log| log.truncated));
    assert!(checked.observation.stdout_truncated && checked.observation.stderr_truncated);
    assert!(checked.observation.validation_complete);
    assert_eq!(
        checked.log.as_ref().map(|log| log.size_bytes as usize),
        Some(bytes.len())
    );
    Ok(())
}

#[test]
fn quota_preserves_validation_without_log_and_final_authorization_still_applies()
-> Result<(), InspectionError> {
    for failure in 0..4 {
        let clock = TestClock::default();
        let control = Control::default();
        let changed = Rc::new(Cell::new(false));
        let mut registry =
            ProjectRegistry::new(Backend(changed.clone()), Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        let mut store = Store::new(clock.clone());
        let old = store
            .capture(&reference, &mut TestInput)
            .map_err(|_| InspectionError::Internal)?;
        store.capture_error = Some(ArtifactError::QuotaExceeded);
        store.after_capture = Some(Box::new(|| match failure {
            1 => clock.0.set(10),
            2 => changed.set(true),
            3 => control.0.store(true, Ordering::Relaxed),
            _ => {}
        }));
        clock.0.set(9);
        let checker = Checker(
            || {
                let mut value = observation()?;
                value.diagnostics.push(Diagnostic {
                    source: DiagnosticSource::Rustc,
                    severity: Severity::Error,
                    code: None,
                    message: "compiler failure"
                        .parse()
                        .map_err(|_| InspectionError::Internal)?,
                    spans: vec![],
                    rendered: None,
                    suggestions: vec![],
                    truncated: false,
                });
                Ok(value)
            },
            Cell::new(0),
        );
        let result = registry.check(
            &reference,
            &options()?,
            &checker,
            &mut store,
            (&clock, &clock),
            &control,
        );
        if failure == 0 {
            let checked = result?;
            assert!(checked.log.is_none() && checked.retention_remaining_seconds.is_none());
            assert_eq!(checked.observation.outcome, CheckOutcome::Failed);
            assert!(checked.observation.validation_complete);
            assert_eq!(checked.observation.diagnostics.len(), 1);
            assert!(checked.observation.stdout.is_empty() && checked.observation.stderr.is_empty());
            assert!(checked.observation.stdout_truncated && checked.observation.stderr_truncated);
            assert_eq!(store.entries.len(), 1);
            assert_eq!(store.entries[0].0, old);
            clock.0.set(18);
            assert!(registry.resolve(&reference, &control).is_ok());
        } else {
            assert!(result.is_err());
            control.0.store(false, Ordering::Relaxed);
            clock.0.set(10);
            assert!(registry.resolve(&reference, &control).is_err());
            if failure == 3 {
                assert_eq!(store.entries[0].0, old);
            }
        }
        assert_eq!(store.removes, 0);
    }
    Ok(())
}
struct TestInput;
impl ArtifactInput for TestInput {
    fn read(&mut self, _: &mut [u8]) -> Result<usize, ArtifactError> {
        Ok(0)
    }
}
