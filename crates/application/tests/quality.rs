use rust_engineering_application::*;
use rust_engineering_domain::*;
use std::{
    cell::{Cell, RefCell},
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
#[derive(Clone, Default)]
struct Backend {
    changed: Rc<Cell<bool>>,
    captures: Rc<Cell<usize>>,
}
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
            fingerprint: format!("sha256:{:064x}", u8::from(self.changed.get()))
                .parse()
                .map_err(|_| ProjectError::Internal)?,
        })
    }
}
impl ProjectSourceBackend for Backend {
    fn source(&self, _: &(), _: &dyn OperationControl) -> Result<SourceBundle, ProjectError> {
        self.captures.set(self.captures.get() + 1);
        SourceBundle::new(vec![
            SourceFile::new(
                "Cargo.toml".into(),
                format!("[workspace]\n# generation {}\n", self.captures.get()).into_bytes(),
            )
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
fn observation() -> Result<CheckObservation, InspectionError> {
    let fp = format!("sha256:{:064x}", 42);
    Ok(CheckObservation {
        outcome: CheckOutcome::Passed,
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        validation_complete: true,
        diagnostics: vec![],
        diagnostics_omitted: 0,
        stdout: "compiler record".into(),
        stderr: "compiler failed".into(),
        stdout_truncated: false,
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
    after_capture: Option<Box<dyn Fn(usize) + 'a>>,
    captures: usize,
    removes: usize,
    reads: usize,
    after_read: Option<Box<dyn Fn(usize) + 'a>>,
    fail_capture: Option<usize>,
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
            reads: 0,
            after_read: None,
            fail_capture: None,
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
        if self.fail_capture == Some(self.captures) {
            return Err(ArtifactError::InputFailure);
        }
        if let Some(error) = self.capture_error {
            if let Some(callback) = &self.after_capture {
                callback(self.captures);
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
            callback(self.captures);
        }
        Ok(metadata)
    }
    fn read<'a>(
        &'a mut self,
        owner: &ProjectRef,
        id: &ArtifactId,
    ) -> Result<ArtifactView<'a>, ArtifactError> {
        self.reads += 1;
        if let Some(callback) = &self.after_read {
            callback(self.reads);
        }
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

struct Suite<F> {
    during: F,
    seen: RefCell<Vec<(&'static str, usize)>>,
}
impl<F: Fn(&str, &mut CheckObservation) -> Result<(), InspectionError>> Suite<F> {
    fn run(
        &self,
        stage: &'static str,
        source: &SourceBundle,
    ) -> Result<CheckObservation, InspectionError> {
        assert_eq!(source.files()[0].bytes(), b"[workspace]\n# generation 1\n");
        self.seen
            .borrow_mut()
            .push((stage, std::ptr::from_ref(source) as usize));
        let mut result = observation()?;
        result.stdout = format!("{stage} stdout");
        (self.during)(stage, &mut result)?;
        Ok(result)
    }
}
impl<F: Fn(&str, &mut CheckObservation) -> Result<(), InspectionError>> ProjectFormatPort
    for Suite<F>
{
    fn format(
        &self,
        source: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<FormatObservation, InspectionError> {
        Ok(FormatObservation {
            execution: self.run("fmt", source)?,
            affected_files: vec![],
            affected_files_omitted: 0,
            diff: None,
            diff_omitted: false,
        })
    }
}
impl<F: Fn(&str, &mut CheckObservation) -> Result<(), InspectionError>> ProjectCheckPort
    for Suite<F>
{
    fn check(
        &self,
        source: &SourceBundle,
        options: &CheckOptions,
        _: &dyn InspectionControl,
    ) -> Result<CheckObservation, InspectionError> {
        assert_eq!(
            options,
            &CheckOptions::try_from(CheckSelection::default())
                .map_err(|_| InspectionError::Internal)?
        );
        self.run("check", source)
    }
}
impl<F: Fn(&str, &mut CheckObservation) -> Result<(), InspectionError>> ProjectClippyPort
    for Suite<F>
{
    fn clippy(
        &self,
        source: &SourceBundle,
        options: &ClippyOptions,
        _: &dyn InspectionControl,
    ) -> Result<CheckObservation, InspectionError> {
        assert_eq!(options.lint_profile(), LintProfile::Strict);
        assert_eq!(options.package(), None);
        assert!(!options.workspace() && !options.all_targets() && options.features().is_empty());
        self.run("clippy", source)
    }
}
impl<F: Fn(&str, &mut CheckObservation) -> Result<(), InspectionError>> ProjectTestPort
    for Suite<F>
{
    fn test(
        &self,
        source: &SourceBundle,
        options: &TestOptions,
        _: &dyn InspectionControl,
    ) -> Result<TestObservation, InspectionError> {
        assert_eq!(
            options,
            &TestOptions::try_from(TestSelection::default())
                .map_err(|_| InspectionError::Internal)?
        );
        assert_eq!(options.timeout(), 30);
        Ok(TestObservation {
            execution: self.run("test", source)?,
            build_succeeded: Some(true),
        })
    }
}
impl<F: Fn(&str, &mut CheckObservation) -> Result<(), InspectionError>> ProjectInspectionPort
    for Suite<F>
{
    fn inspect(
        &self,
        source: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        let result = self.run("metadata", source)?;
        Ok(ProjectStructure {
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
            source_fingerprint: result.source_fingerprint,
            runtime: result.runtime,
        })
    }
}
impl<F: Fn(&str, &mut CheckObservation) -> Result<(), InspectionError>> DependencyAuditPort
    for Suite<F>
{
    fn audit(
        &self,
        source: &SourceBundle,
        structure: &ProjectStructure,
        _: &dyn Clock,
        _: &dyn InspectionControl,
    ) -> Result<AuditObservation, AuditDataError> {
        let result = self
            .run("audit", source)
            .map_err(|_| AuditDataError::Internal)?;
        assert_eq!(structure.source_fingerprint, result.source_fingerprint);
        Ok(AuditObservation::unavailable())
    }
}
fn suite<F: Fn(&str, &mut CheckObservation) -> Result<(), InspectionError>>(during: F) -> Suite<F> {
    Suite {
        during,
        seen: RefCell::new(vec![]),
    }
}
fn seed(
    store: &mut Store<'_>,
    reference: &ProjectRef,
) -> Result<ArtifactMetadata, InspectionError> {
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
    Ok(old)
}

#[test]
fn profiles_use_one_capture_and_closed_options_even_after_validation_failure()
-> Result<(), InspectionError> {
    for profile in [QualityProfile::Fast, QualityProfile::Standard] {
        let clock = TestClock::default();
        let backend = Backend::default();
        let control = Control::default();
        let mut registry = ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        clock.0.set(9);
        let executor = suite(|stage, result| {
            if stage == "check" {
                result.outcome = CheckOutcome::Failed;
                result.exit_code = Some(101);
            }
            Ok(())
        });
        let mut store = Store::new(clock.clone());
        let result = registry.quality_gate(
            &reference,
            profile,
            QualityPorts {
                executor: &executor,
                auditor: &executor,
            },
            &mut store,
            (&clock, &clock),
            &control,
        )?;
        let seen = executor.seen.borrow();
        let expected: &[&str] = match profile {
            QualityProfile::Fast => &["fmt", "check", "clippy"],
            QualityProfile::Standard => &["fmt", "check", "clippy", "test", "metadata", "audit"],
        };
        assert_eq!(
            seen.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            expected
        );
        assert!(seen.iter().all(|(_, pointer)| *pointer == seen[0].1));
        assert_eq!(backend.captures.get(), 1);
        assert_eq!(
            result.source_fingerprint,
            Some(observation()?.source_fingerprint)
        );
        assert_eq!(result.stages[1].status, ToolStatus::Failed);
        assert_eq!(result.stages[2].status, ToolStatus::Passed);
        assert_eq!(
            quality_status(profile, &result.stages),
            if profile == QualityProfile::Fast {
                ToolStatus::Failed
            } else {
                ToolStatus::Unavailable
            }
        );
        assert_eq!(
            store.entries.len(),
            if profile == QualityProfile::Fast {
                3
            } else {
                4
            }
        );
        for stage in &result.stages {
            if let Some(execution) = stage
                .observation
                .as_ref()
                .and_then(QualityObservation::execution)
            {
                assert!(execution.stdout.is_empty() && execution.stderr.is_empty());
                assert!(stage.log.is_some());
                assert_eq!(stage.retention_remaining_seconds, Some(20));
            }
        }
        clock.0.set(18);
        assert!(registry.resolve(&reference, &control).is_ok());
    }
    Ok(())
}

#[test]
fn operational_failure_continues_but_infrastructure_cancel_and_mismatched_source_abort()
-> Result<(), InspectionError> {
    for case in 0..4 {
        let clock = TestClock::default();
        let control = Control::default();
        let mut registry =
            ProjectRegistry::new(Backend::default(), Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        clock.0.set(9);
        let executor = suite(|stage, observation| {
            if stage == "check" {
                match case {
                    0 => return Err(InspectionError::Execution(ExecutionError::Unavailable)),
                    1 => return Err(InspectionError::Execution(ExecutionError::CleanupUncertain)),
                    2 => control.0.store(true, Ordering::Relaxed),
                    _ => {
                        observation.source_fingerprint = format!("sha256:{:064x}", 99)
                            .parse()
                            .map_err(|_| InspectionError::Internal)?
                    }
                }
            }
            Ok(())
        });
        let mut store = Store::new(clock.clone());
        let result = registry.quality_gate(
            &reference,
            QualityProfile::Fast,
            QualityPorts {
                executor: &executor,
                auditor: &executor,
            },
            &mut store,
            (&clock, &clock),
            &control,
        );
        if case == 0 {
            let result = result?;
            assert_eq!(result.stages[1].status, ToolStatus::Unavailable);
            assert_eq!(result.stages[2].status, ToolStatus::Passed);
            assert_eq!(executor.seen.borrow().len(), 3);
        } else {
            assert_eq!(
                result.err(),
                Some(match case {
                    1 => InspectionError::Execution(ExecutionError::CleanupUncertain),
                    2 => InspectionError::Project(ProjectError::Cancelled),
                    _ => InspectionError::Internal,
                })
            );
            assert_eq!(executor.seen.borrow().len(), 2);
            assert_eq!(store.captures, 0);
            control.0.store(false, Ordering::Relaxed);
            clock.0.set(10);
            assert!(registry.resolve(&reference, &control).is_err());
        }
    }
    Ok(())
}

#[test]
fn batch_rolls_back_new_logs_on_capture_failure_or_expiry_during_later_validation()
-> Result<(), InspectionError> {
    for expires in [false, true] {
        let clock = TestClock::default();
        let artifact_clock = TestClock::default();
        let control = Control::default();
        let mut registry =
            ProjectRegistry::new(Backend::default(), Generator, clock.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        clock.0.set(9);
        let executor = suite(|_, _| Ok(()));
        let mut store = Store::new(artifact_clock.clone());
        let old = seed(&mut store, &reference)?;
        if expires {
            // First log is valid when inspected. Reading a later log advances time
            // past the first retention boundary; final group validation must catch it.
            store.after_capture = Some(Box::new(|n| {
                if n == 1 {
                    artifact_clock.0.set(1);
                }
            }));
            store.after_read = Some(Box::new(|n| {
                if n == 9 {
                    artifact_clock.0.set(20);
                }
            }));
        } else {
            store.fail_capture = Some(2);
        }
        assert!(
            registry
                .quality_gate(
                    &reference,
                    QualityProfile::Fast,
                    QualityPorts {
                        executor: &executor,
                        auditor: &executor
                    },
                    &mut store,
                    (&clock, &artifact_clock),
                    &control
                )
                .is_err()
        );
        assert_eq!(store.entries.len(), 1);
        assert_eq!(store.entries[0], (old, b"old".to_vec()));
        assert_eq!(store.removes, if expires { 3 } else { 1 });
        clock.0.set(10);
        assert!(
            registry.resolve(&reference, &control).is_err(),
            "a failed batch renewed the project"
        );
    }
    Ok(())
}

#[test]
fn quota_omission_never_bypasses_final_identity_expiry_or_cancellation()
-> Result<(), InspectionError> {
    for quota in [false, true] {
        for case in 0..4 {
            let clock = TestClock::default();
            let artifact_clock = TestClock::default();
            let backend = Backend::default();
            let control = Control::default();
            let mut registry =
                ProjectRegistry::new(backend.clone(), Generator, clock.clone(), 10, 1)?;
            let reference = registry.open("/trusted", &control)?.project_ref;
            clock.0.set(9);
            let mut store = Store::new(artifact_clock.clone());
            let old = seed(&mut store, &reference)?;
            if quota {
                store.capture_error = Some(ArtifactError::QuotaExceeded);
            }
            store.after_capture = Some(Box::new(|n| {
                if n == 3 {
                    match case {
                        1 => backend.changed.set(true),
                        2 => clock.0.set(10),
                        3 => control.0.store(true, Ordering::Relaxed),
                        _ => {}
                    }
                }
            }));
            let executor = suite(|_, _| Ok(()));
            let result = registry.quality_gate(
                &reference,
                QualityProfile::Fast,
                QualityPorts {
                    executor: &executor,
                    auditor: &executor,
                },
                &mut store,
                (&clock, &artifact_clock),
                &control,
            );
            if case == 0 {
                let result = result?;
                assert_eq!(
                    quality_status(QualityProfile::Fast, &result.stages),
                    if quota {
                        ToolStatus::Blocked
                    } else {
                        ToolStatus::Passed
                    }
                );
                assert!(
                    result
                        .stages
                        .iter()
                        .all(|stage| stage.log.is_none() == quota)
                );
                if quota {
                    assert!(result.stages.iter().all(|stage| {
                        stage
                            .observation
                            .as_ref()
                            .and_then(QualityObservation::execution)
                            .is_some_and(|execution| {
                                execution.stdout_truncated
                                    && execution.stderr_truncated
                                    && execution.validation_complete
                            })
                    }));
                }
                assert_eq!(store.removes, 0);
            } else {
                assert!(result.is_err(), "quota={quota}, case={case}");
                assert_eq!(store.removes, if quota { 0 } else { 3 });
                // Cancellation preserves the live owner's previous artifacts.
                if case == 3 {
                    assert_eq!(store.entries, vec![(old, b"old".to_vec())]);
                }
                control.0.store(false, Ordering::Relaxed);
                clock.0.set(10);
                assert!(registry.resolve(&reference, &control).is_err());
            }
        }
    }
    Ok(())
}

#[test]
fn rollback_attempts_every_new_id_even_when_one_removal_fails() -> Result<(), InspectionError> {
    let clock = TestClock::default();
    let control = Control::default();
    let mut registry = ProjectRegistry::new(Backend::default(), Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/trusted", &control)?.project_ref;
    clock.0.set(9);
    let mut store = Store::new(clock.clone());
    let old = seed(&mut store, &reference)?;
    store.remove_error = true;
    store.after_capture = Some(Box::new(|n| {
        if n == 3 {
            control.0.store(true, Ordering::Relaxed);
        }
    }));
    let executor = suite(|_, _| Ok(()));
    assert_eq!(
        registry
            .quality_gate(
                &reference,
                QualityProfile::Fast,
                QualityPorts {
                    executor: &executor,
                    auditor: &executor
                },
                &mut store,
                (&clock, &clock),
                &control
            )
            .err(),
        Some(InspectionError::Internal)
    );
    assert_eq!(store.removes, 3);
    assert_eq!(store.entries[0], (old, b"old".to_vec()));
    control.0.store(false, Ordering::Relaxed);
    clock.0.set(10);
    assert!(registry.resolve(&reference, &control).is_err());
    Ok(())
}

struct FreshAudit(AuditObservation);
impl DependencyAuditPort for FreshAudit {
    fn audit(
        &self,
        _: &SourceBundle,
        _: &ProjectStructure,
        _: &dyn Clock,
        _: &dyn InspectionControl,
    ) -> Result<AuditObservation, AuditDataError> {
        Ok(self.0.clone())
    }
}
#[test]
fn audit_fresh_at_execution_is_reassessed_after_log_publication() -> Result<(), InspectionError> {
    let clock = TestClock::default();
    let wall = TestClock::default();
    wall.0.set(1);
    let control = Control::default();
    let mut registry = ProjectRegistry::new(Backend::default(), Generator, clock.clone(), 10, 1)?;
    let reference = registry.open("/trusted", &control)?.project_ref;
    let fp = format!("sha256:{:064x}", 42);
    let provenance = Provenance::new(
        SourceKind::RustsecSnapshot,
        "advisory-fixture"
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        Some(UnixSeconds(0)),
        Some(UnixSeconds(1)),
        IntegrityStatus::Verified,
        false,
    )
    .map_err(|_| InspectionError::Internal)?;
    let policy = FreshnessPolicy::new(
        "fixture-v1"
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        60,
        300,
    )
    .map_err(|_| InspectionError::Internal)?;
    let auditor = FreshAudit(AuditObservation {
        state: AuditState::Passed,
        issue: None,
        validation_complete: true,
        lock_fingerprint: Some(fp.parse().map_err(|_| InspectionError::Internal)?),
        snapshot_fingerprint: Some(fp.parse().map_err(|_| InspectionError::Internal)?),
        snapshot: Some(SnapshotEvidence::assess(provenance, policy, &wall)),
        snapshot_record_count: Some(1),
        snapshot_sequence: Some(1),
        packages_total: 1,
        crates_io_scanned: 1,
        workspace_packages_excluded: 0,
        unsupported_packages: vec![],
        findings: vec![],
        informational: vec![],
        findings_omitted: 0,
    });
    let mut store = Store::new(clock.clone());
    store.after_capture = Some(Box::new(|n| {
        if n == 4 {
            wall.0.set(301);
        }
    }));
    let executor = suite(|_, _| Ok(()));
    let result = registry.quality_gate(
        &reference,
        QualityProfile::Standard,
        QualityPorts {
            executor: &executor,
            auditor: &auditor,
        },
        &mut store,
        (&wall, &clock),
        &control,
    )?;
    assert_eq!(result.stages[4].status, ToolStatus::Unavailable);
    assert_eq!(
        quality_status(QualityProfile::Standard, &result.stages),
        ToolStatus::Unavailable
    );
    let Some(QualityObservation::Audit { observation, .. }) = &result.stages[4].observation else {
        return Err(InspectionError::Internal);
    };
    assert_eq!(observation.issue, Some(AuditIssue::SnapshotStale));
    assert!(!observation.validation_complete);
    assert_eq!(
        observation
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.freshness().assessed_at()),
        Some(UnixSeconds(301))
    );
    Ok(())
}

#[test]
fn inconsistent_runtime_aborts_before_logs_and_audit_but_command_identity_may_differ()
-> Result<(), InspectionError> {
    for changed_stage in ["check", "metadata"] {
        for field in 0..7 {
            let clock = TestClock::default();
            let control = Control::default();
            let mut registry =
                ProjectRegistry::new(Backend::default(), Generator, clock.clone(), 10, 1)?;
            let reference = registry.open("/trusted", &control)?.project_ref;
            clock.0.set(9);
            let executor = suite(|stage, observation| {
                if stage == changed_stage {
                    match field {
                        0 => observation.runtime.image_id = "different-image".into(),
                        1 => {
                            observation.runtime.configuration_fingerprint =
                                format!("sha256:{:064x}", 99)
                                    .parse()
                                    .map_err(|_| InspectionError::Internal)?
                        }
                        2 => observation.runtime.platform = "linux/x86_64".into(),
                        3 => observation.runtime.rust_version = "1.98.0".into(),
                        4 => observation.runtime.cargo_version = "1.98.0".into(),
                        5 => observation.runtime.declared_toolchain = Some("stable".into()),
                        _ => {
                            observation.runtime.execution_fingerprint =
                                format!("sha256:{:064x}", 99)
                                    .parse()
                                    .map_err(|_| InspectionError::Internal)?
                        }
                    }
                }
                Ok(())
            });
            let mut store = Store::new(clock.clone());
            let result = registry.quality_gate(
                &reference,
                QualityProfile::Standard,
                QualityPorts {
                    executor: &executor,
                    auditor: &executor,
                },
                &mut store,
                (&clock, &clock),
                &control,
            );
            if field == 6 {
                let result = result?;
                assert_eq!(store.captures, 4);
                assert_eq!(executor.seen.borrow().len(), 6);
                assert!(
                    result.stages[..4]
                        .iter()
                        .all(|stage| stage.status == ToolStatus::Passed)
                );
                clock.0.set(18);
                assert!(registry.resolve(&reference, &control).is_ok());
            } else {
                assert_eq!(
                    result.err(),
                    Some(InspectionError::Internal),
                    "{changed_stage}, field {field}"
                );
                assert_eq!(store.captures, 0);
                let seen = executor.seen.borrow();
                assert_eq!(seen.len(), if changed_stage == "check" { 2 } else { 5 });
                assert!(seen.iter().all(|(stage, _)| *stage != "audit"));
                clock.0.set(10);
                assert!(
                    registry.resolve(&reference, &control).is_err(),
                    "runtime mismatch renewed the lease"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn outer_capture_age_includes_execution_and_log_publication_without_changing_verdict()
-> Result<(), InspectionError> {
    for (published_at, expected_state) in
        [(1120, FreshnessState::Aging), (1301, FreshnessState::Stale)]
    {
        let idle = TestClock::default();
        let wall = TestClock::default();
        wall.0.set(1000);
        let control = Control::default();
        let mut registry =
            ProjectRegistry::new(Backend::default(), Generator, idle.clone(), 10, 1)?;
        let reference = registry.open("/trusted", &control)?.project_ref;
        let executor = suite(|stage, _| {
            if stage == "fmt" {
                wall.0.set(1010);
            }
            Ok(())
        });
        let mut store = Store::new(idle.clone());
        store.after_capture = Some(Box::new(|n| {
            if n == 3 {
                wall.0.set(published_at);
            }
        }));
        let result = registry.quality_gate(
            &reference,
            QualityProfile::Fast,
            QualityPorts {
                executor: &executor,
                auditor: &executor,
            },
            &mut store,
            (&wall, &idle),
            &control,
        )?;
        assert_eq!(
            quality_status(QualityProfile::Fast, &result.stages),
            ToolStatus::Passed
        );
        assert!(matches!(result.semantics, InspectionSemantics::LatestKnown));
        let Evidence::Snapshot(evidence) = &result.evidence else {
            return Err(InspectionError::Internal);
        };
        assert_eq!(
            evidence.provenance().source_kind(),
            SourceKind::ProjectSnapshot
        );
        assert_eq!(evidence.provenance().created_at(), Some(UnixSeconds(1000)));
        assert_eq!(evidence.provenance().observed_at(), Some(UnixSeconds(1000)));
        assert_eq!(
            evidence.freshness().assessed_at(),
            UnixSeconds(published_at)
        );
        assert_eq!(evidence.freshness().state(), expected_state);
        assert_eq!(
            evidence.freshness().age_seconds(),
            Some(published_at - 1000)
        );
        assert!(!evidence.provenance().network_used());
    }
    Ok(())
}
