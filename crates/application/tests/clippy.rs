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
struct Clippy<F>(F, Cell<usize>);
impl<F: Fn() -> Result<CheckObservation, InspectionError>> ProjectClippyPort for Clippy<F> {
    fn clippy(
        &self,
        source: &SourceBundle,
        options: &ClippyOptions,
        _: &dyn InspectionControl,
    ) -> Result<CheckObservation, InspectionError> {
        self.1.set(self.1.get() + 1);
        assert_eq!(source.files()[0].bytes(), b"[workspace]\n");
        assert_eq!(options, &selection()?);
        (self.0)()
    }
}
fn observation() -> Result<CheckObservation, InspectionError> {
    let fp = format!("sha256:{:064x}", 42);
    Ok(CheckObservation {
        outcome: CheckOutcome::Failed,
        termination: ExecutionTermination::Exited,
        exit_code: Some(1),
        validation_complete: true,
        diagnostics: vec![],
        diagnostics_omitted: 0,
        stdout: "lint finding".into(),
        stderr: "lint warning".into(),
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

fn selection() -> Result<ClippyOptions, InspectionError> {
    ClippyOptions::try_from(ClippySelection {
        workspace: true,
        all_targets: true,
        features: vec!["std".into(), "member/extra".into()],
        lint_profile: LintProfile::Strict,
        ..Default::default()
    })
    .map_err(|_| InspectionError::Internal)
}

#[test]
fn options_forwarded_and_failed_result_retained_with_one_authorized_log()
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
    let mut store = Store::new(clock.clone());
    let clippy = Clippy(observation, Cell::new(0));
    let options = selection()?;
    let result = registry.clippy(
        &reference,
        &options,
        &clippy,
        &mut store,
        (&clock, &clock),
        &control,
    )?;
    assert_eq!(clippy.1.get(), 1);
    assert_eq!(result.options, options);
    assert_eq!(result.observation.outcome, CheckOutcome::Failed);
    assert_eq!(result.observation.exit_code, Some(1));
    assert!(result.observation.validation_complete);
    assert!(result.observation.stdout.is_empty() && result.observation.stderr.is_empty());
    assert!(matches!(result.semantics, InspectionSemantics::LatestKnown));
    assert_eq!(
        result.evidence.provenance().source_kind(),
        SourceKind::ProjectSnapshot
    );
    assert_eq!(result.retention_remaining_seconds, Some(20));
    assert_eq!(store.entries.len(), 1);
    assert_eq!(
        store.entries[0].1,
        b"=== stdout ===\nlint finding\n[stream truncated]\n=== stderr ===\nlint warning"
    );
    Ok(())
}

#[test]
fn final_revocation_and_cancellation_rollback_new_log_without_renewal()
-> Result<(), InspectionError> {
    for cancelled in [false, true] {
        let clock = TestClock::default();
        let control = Control::default();
        let changed = Rc::new(Cell::new(false));
        let mut registry =
            ProjectRegistry::new(Backend(changed.clone()), Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        let mut store = Store::new(clock.clone());
        store.after_capture = Some(Box::new(|| {
            if cancelled {
                control.0.store(true, Ordering::Relaxed);
            } else {
                changed.set(true);
            }
        }));
        clock.0.set(9);
        let result = registry.clippy(
            &reference,
            &selection()?,
            &Clippy(observation, Cell::new(0)),
            &mut store,
            (&clock, &clock),
            &control,
        );
        assert_eq!(
            result.err(),
            Some(InspectionError::Project(if cancelled {
                ProjectError::Cancelled
            } else {
                ProjectError::Rejected(OperationalErrorCode::ProjectNotFound)
            }))
        );
        assert_eq!(store.removes, 1);
        assert!(store.entries.is_empty());
        control.0.store(false, Ordering::Relaxed);
        clock.0.set(10);
        assert!(registry.resolve(&reference, &control).is_err());
    }
    Ok(())
}

#[test]
fn quota_fallback_preserves_failure_and_still_requires_live_authorization()
-> Result<(), InspectionError> {
    for revoke in [false, true] {
        let clock = TestClock::default();
        let control = Control::default();
        let changed = Rc::new(Cell::new(false));
        let mut registry =
            ProjectRegistry::new(Backend(changed.clone()), Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        let mut store = Store::new(clock.clone());
        store.capture_error = Some(ArtifactError::QuotaExceeded);
        store.after_capture = Some(Box::new(|| changed.set(revoke)));
        let result = registry.clippy(
            &reference,
            &selection()?,
            &Clippy(observation, Cell::new(0)),
            &mut store,
            (&clock, &clock),
            &control,
        );
        if revoke {
            assert!(result.is_err());
        } else {
            let result = result?;
            assert_eq!(result.observation.outcome, CheckOutcome::Failed);
            assert!(result.observation.validation_complete);
            assert!(result.log.is_none() && result.retention_remaining_seconds.is_none());
            assert!(result.observation.stdout_truncated && result.observation.stderr_truncated);
            assert!(result.observation.stdout.is_empty() && result.observation.stderr.is_empty());
        }
    }
    Ok(())
}
