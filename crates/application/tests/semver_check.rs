use rust_engineering_application::semver_check::{
    ProjectSemverPort, SemverArtifactReference, SemverDurablePublisher, SemverObservation,
    SemverOptions, SemverOutcome,
};
use rust_engineering_application::{
    ArtifactInput, ArtifactStore, ExecutionCancellation, InspectionControl, InspectionError,
    OperationControl, ProjectBackend, ProjectError, ProjectIdentity, ProjectInspectionPort,
    ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts, QualityProjectBackend,
    ReferenceGenerator, RegistryClock, ValidatedProject,
};
use rust_engineering_domain::{
    ArtifactError, ArtifactId, ArtifactMetadata, ArtifactView, CargoConfiguration, Clock,
    ExecutionFingerprint, ExecutionTermination, ProjectConfigPolicy, ProjectPackage, ProjectRef,
    ProjectStructure, ProjectTarget, RuntimeIdentity, RustEdition, SourceBundle, SourceFile,
    SourceFingerprint, TargetKind, UnixSeconds,
    semver_check::{
        SemverCommandOptions, SemverExit, SemverFindingCompleteness, SemverFindingCounts,
        SemverProjectSelection,
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
};

type TestResult<T = ()> = Result<T, InspectionError>;

#[derive(Default)]
struct Control;
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        false
    }
}

#[derive(Clone, Default)]
struct TestClock(Arc<AtomicU64>);
impl RegistryClock for TestClock {
    fn seconds(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}
impl Clock for TestClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.seconds())
    }
}

#[derive(Clone)]
struct Backend {
    log: Arc<Mutex<Vec<String>>>,
}
#[derive(Clone)]
struct Lease {
    side: String,
}
fn identity(side: &str) -> Result<ProjectIdentity, ProjectError> {
    let digit = if side == "baseline" { '1' } else { '2' };
    Ok(ProjectIdentity {
        workspace_root: format!("/{side}"),
        fingerprint: format!("sha256:{}", digit.to_string().repeat(64))
            .parse()
            .map_err(|_| ProjectError::Internal)?,
    })
}
impl ProjectBackend for Backend {
    type Lease = Lease;
    fn open(
        &self,
        path: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<Self::Lease>, ProjectError> {
        let side = path.trim_start_matches('/').to_owned();
        Ok(ValidatedProject {
            identity: identity(&side)?,
            lease: Lease { side },
        })
    }
    fn revalidate(
        &self,
        lease: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        self.log
            .lock()
            .map_err(|_| ProjectError::Internal)?
            .push(format!("revalidate:{}", lease.side));
        identity(&lease.side)
    }
}
impl ProjectSourceBackend for Backend {
    fn source(
        &self,
        lease: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<SourceBundle, ProjectError> {
        self.log
            .lock()
            .map_err(|_| ProjectError::Internal)?
            .push(format!("capture:{}", lease.side));
        SourceBundle::new(vec![
            SourceFile::new(format!("{}.marker", lease.side), Vec::new())
                .map_err(|_| ProjectError::Internal)?,
        ])
        .map_err(|_| ProjectError::Internal)
    }
}
impl QualityProjectBackend for Backend {
    fn revalidate_quality_owner(
        &self,
        lease: &Self::Lease,
        control: &dyn OperationControl,
    ) -> Result<QualityOwnerFacts, ProjectError> {
        let identity = self.revalidate(lease, control)?;
        Ok(QualityOwnerFacts {
            granted_root_device: 1,
            granted_root_inode: if lease.side == "baseline" { 11 } else { 12 },
            workspace_root: identity.workspace_root,
        })
    }
}
struct Generator(AtomicUsize);
impl ReferenceGenerator for Generator {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        let next = self.0.fetch_add(1, Ordering::Relaxed) + 1;
        format!("prj_{next:032x}")
            .parse()
            .map_err(|_| ProjectError::Internal)
    }
}

struct Inspector {
    log: Arc<Mutex<Vec<String>>>,
    candidate_has_lib: bool,
}
impl ProjectInspectionPort for Inspector {
    fn inspect(
        &self,
        source: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        let side = if source
            .files()
            .iter()
            .any(|file| file.path() == "baseline.marker")
        {
            "baseline"
        } else {
            "candidate"
        };
        self.log
            .lock()
            .map_err(|_| InspectionError::Internal)?
            .push(format!("inspect:{side}"));
        structure(side == "baseline" || self.candidate_has_lib)
    }
}
fn structure(has_lib: bool) -> Result<ProjectStructure, InspectionError> {
    let fingerprint: ExecutionFingerprint = format!("sha256:{}", "3".repeat(64))
        .parse()
        .map_err(|_| InspectionError::Internal)?;
    let source_fingerprint: SourceFingerprint =
        format!("sha256:{}", if has_lib { "4" } else { "5" }.repeat(64))
            .parse()
            .map_err(|_| InspectionError::Internal)?;
    let targets = if has_lib {
        vec![ProjectTarget {
            name: "fixture".into(),
            kinds: vec![TargetKind::Lib],
            crate_types: vec![TargetKind::Lib],
            source_path: "src/lib.rs".into(),
            edition: RustEdition::E2024,
            required_features: vec![],
            test: true,
            doctest: true,
        }]
    } else {
        vec![ProjectTarget {
            name: "fixture".into(),
            kinds: vec![TargetKind::Bin],
            crate_types: vec![TargetKind::Bin],
            source_path: "src/main.rs".into(),
            edition: RustEdition::E2024,
            required_features: vec![],
            test: true,
            doctest: false,
        }]
    };
    Ok(ProjectStructure {
        workspace_members: vec![0],
        workspace_default_members: vec![0],
        packages: vec![ProjectPackage {
            package_index: 0,
            name: "fixture".into(),
            version: "1.0.0".into(),
            manifest_path: "Cargo.toml".into(),
            edition: RustEdition::E2024,
            rust_version: None,
            targets,
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
            platform: "linux/aarch64".into(),
            image_id: format!("sha256:{}", "6".repeat(64)),
            configuration_fingerprint: fingerprint.clone(),
            execution_fingerprint: fingerprint,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        source_fingerprint,
    })
}

struct Runner {
    log: Arc<Mutex<Vec<String>>>,
    calls: Arc<AtomicUsize>,
}
impl ProjectSemverPort for Runner {
    fn run(
        &self,
        _: &SourceBundle,
        _: &SourceBundle,
        options: &SemverOptions,
        _: &dyn InspectionControl,
    ) -> Result<SemverObservation, InspectionError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.log
            .lock()
            .map_err(|_| InspectionError::Internal)?
            .push("run".into());
        let fingerprint: ExecutionFingerprint = format!("sha256:{}", "7".repeat(64))
            .parse()
            .map_err(|_| InspectionError::Internal)?;
        Ok(SemverObservation {
            options: options.clone(),
            exit: SemverExit::NoBreak,
            counts: SemverFindingCounts::default(),
            findings: vec![],
            findings_omitted: 0,
            completeness: SemverFindingCompleteness::Partial,
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            runtime: RuntimeIdentity {
                platform: "linux/aarch64".into(),
                image_id: format!("sha256:{}", "8".repeat(64)),
                configuration_fingerprint: fingerprint.clone(),
                execution_fingerprint: fingerprint.clone(),
                rust_version: "1.98.1".into(),
                cargo_version: "1.98.1".into(),
                declared_toolchain: None,
            },
            execution_fingerprint: fingerprint,
            stdout: b"No semver-breaking changes detected.\n".to_vec(),
            stderr: vec![],
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }
}

#[derive(Default)]
struct Store {
    entry: Option<(ArtifactMetadata, Vec<u8>)>,
    cancel_after_capture: Option<Arc<AtomicBool>>,
}
impl ArtifactStore for Store {
    fn capture(
        &mut self,
        owner: &ProjectRef,
        input: &mut dyn ArtifactInput,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        loop {
            let count = input.read(&mut chunk)?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        let metadata = ArtifactMetadata {
            owner: owner.clone(),
            id: "art_00000000000000000000000000000001".parse()?,
            sha256: [9; 32],
            size_bytes: bytes
                .len()
                .try_into()
                .map_err(|_| ArtifactError::QuotaExceeded)?,
            truncated: input.truncated(),
            created_seconds: 0,
            expires_seconds: 60,
        };
        self.entry = Some((metadata.clone(), bytes));
        if let Some(cancelled) = &self.cancel_after_capture {
            cancelled.store(true, Ordering::Release);
        }
        Ok(metadata)
    }
    fn read<'a>(
        &'a mut self,
        owner: &ProjectRef,
        id: &ArtifactId,
    ) -> Result<ArtifactView<'a>, ArtifactError> {
        let (metadata, content) = self.entry.as_ref().ok_or(ArtifactError::NotFound)?;
        if &metadata.owner != owner || &metadata.id != id {
            return Err(ArtifactError::NotFound);
        }
        Ok(ArtifactView { metadata, content })
    }
    fn remove(&mut self, _: &ProjectRef, _: &ArtifactId) -> Result<bool, ArtifactError> {
        Ok(self.entry.take().is_some())
    }
    fn retain_owners(&mut self, _: &[ProjectRef]) -> Result<usize, ArtifactError> {
        Ok(0)
    }
    fn revoke_owner(&mut self, _: &ProjectRef) -> Result<usize, ArtifactError> {
        Ok(0)
    }
    fn cleanup(&mut self) -> Result<usize, ArtifactError> {
        Ok(0)
    }
}

struct CancelAfterCapture(Arc<AtomicBool>);
impl OperationControl for CancelAfterCapture {
    fn check(&self) -> Result<(), ProjectError> {
        if self.0.load(Ordering::Acquire) {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl ExecutionCancellation for CancelAfterCapture {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

struct QuotaStore;
impl ArtifactStore for QuotaStore {
    fn capture(
        &mut self,
        _: &ProjectRef,
        _: &mut dyn ArtifactInput,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        Err(ArtifactError::QuotaExceeded)
    }
    fn read<'a>(
        &'a mut self,
        _: &ProjectRef,
        _: &ArtifactId,
    ) -> Result<ArtifactView<'a>, ArtifactError> {
        Err(ArtifactError::NotFound)
    }
    fn remove(&mut self, _: &ProjectRef, _: &ArtifactId) -> Result<bool, ArtifactError> {
        Ok(false)
    }
    fn retain_owners(&mut self, _: &[ProjectRef]) -> Result<usize, ArtifactError> {
        Ok(0)
    }
    fn revoke_owner(&mut self, _: &ProjectRef) -> Result<usize, ArtifactError> {
        Ok(0)
    }
    fn cleanup(&mut self) -> Result<usize, ArtifactError> {
        Ok(0)
    }
}

struct DurablePublisher {
    calls: Arc<AtomicUsize>,
}
impl SemverDurablePublisher for DurablePublisher {
    fn publish(
        &mut self,
        project: &ProjectRef,
        _: UnixSeconds,
        baseline: &SourceBundle,
        candidate: &SourceBundle,
        observation: &mut SemverObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<Option<SemverArtifactReference>, InspectionError> {
        assert!(
            baseline
                .files()
                .iter()
                .any(|file| file.path() == "baseline.marker")
        );
        assert!(
            candidate
                .files()
                .iter()
                .any(|file| file.path() == "candidate.marker")
        );
        let first = revalidate()?;
        let second = revalidate()?;
        assert_eq!(first, second);
        assert_eq!(first.workspace_root, "/candidate");
        self.calls.fetch_add(1, Ordering::Relaxed);
        observation.stdout.clear();
        observation.stderr.clear();
        Ok(Some(SemverArtifactReference::Ephemeral(ArtifactMetadata {
            owner: project.clone(),
            id: "art_00000000000000000000000000000002"
                .parse()
                .map_err(|_| InspectionError::Internal)?,
            sha256: [10; 32],
            size_bytes: 1,
            truncated: false,
            created_seconds: 0,
            expires_seconds: 60,
        })))
    }
}

fn options() -> TestResult<SemverOptions> {
    let selection = SemverCommandOptions::try_from(SemverProjectSelection::default())
        .map_err(|_| InspectionError::Internal)?;
    SemverOptions::new(selection.clone(), selection, 60).map_err(|_| InspectionError::Internal)
}

#[test]
fn cancellation_after_raw_capture_rolls_back_the_new_artifact() -> TestResult {
    let log = Arc::new(Mutex::new(Vec::new()));
    let backend = Backend {
        log: Arc::clone(&log),
    };
    let clock = TestClock::default();
    let cancelled = Arc::new(AtomicBool::new(false));
    let control = CancelAfterCapture(Arc::clone(&cancelled));
    let mut registry = ProjectRegistry::new(
        backend,
        Generator(AtomicUsize::new(0)),
        clock.clone(),
        60,
        2,
    )?;
    let baseline = registry.open("/baseline", &control)?.project_ref;
    let candidate = registry.open("/candidate", &control)?.project_ref;
    let inspector = Inspector {
        log: Arc::clone(&log),
        candidate_has_lib: true,
    };
    let runner = Runner {
        log,
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut store = Store {
        entry: None,
        cancel_after_capture: Some(Arc::clone(&cancelled)),
    };
    assert!(matches!(
        registry.semver_check(
            &baseline,
            &candidate,
            &options()?,
            &inspector,
            &runner,
            &mut store,
            &clock,
            &control,
        ),
        Err(InspectionError::Project(ProjectError::Cancelled))
    ));
    assert!(store.entry.is_none());
    Ok(())
}

#[test]
fn durable_publication_receives_both_captures_and_revalidates_both_roots() -> TestResult {
    let log = Arc::new(Mutex::new(Vec::new()));
    let clock = TestClock::default();
    let control = Control;
    let mut registry = ProjectRegistry::new(
        Backend {
            log: Arc::clone(&log),
        },
        Generator(AtomicUsize::new(0)),
        clock.clone(),
        60,
        2,
    )?;
    let baseline = registry.open("/baseline", &control)?.project_ref;
    let candidate = registry.open("/candidate", &control)?.project_ref;
    let inspector = Inspector {
        log: Arc::clone(&log),
        candidate_has_lib: true,
    };
    let runner = Runner {
        log,
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let mut publisher = DurablePublisher {
        calls: Arc::clone(&calls),
    };
    let result = registry.semver_check_durable(
        &baseline,
        &candidate,
        &options()?,
        &inspector,
        &runner,
        &mut Store::default(),
        &mut publisher,
        &clock,
        &control,
    )?;
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    assert!(matches!(
        result.raw_output,
        Some(SemverArtifactReference::Ephemeral(_))
    ));
    Ok(())
}

#[test]
fn capture_order_and_final_revalidation_are_stable() -> TestResult {
    let log = Arc::new(Mutex::new(Vec::new()));
    let backend = Backend {
        log: Arc::clone(&log),
    };
    let clock = TestClock::default();
    let control = Control;
    let calls = Arc::new(AtomicUsize::new(0));
    let mut registry = ProjectRegistry::new(
        backend,
        Generator(AtomicUsize::new(0)),
        clock.clone(),
        60,
        2,
    )?;
    let baseline = registry.open("/baseline", &control)?.project_ref;
    let candidate = registry.open("/candidate", &control)?.project_ref;
    let inspector = Inspector {
        log: Arc::clone(&log),
        candidate_has_lib: true,
    };
    let runner = Runner {
        log: Arc::clone(&log),
        calls: Arc::clone(&calls),
    };
    let mut store = Store::default();
    let result = registry.semver_check(
        &baseline,
        &candidate,
        &options()?,
        &inspector,
        &runner,
        &mut store,
        &clock,
        &control,
    )?;
    assert_eq!(result.outcome, SemverOutcome::NoBreak);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let events = log.lock().map_err(|_| InspectionError::Internal)?;
    let baseline_capture = events
        .iter()
        .position(|event| event == "capture:baseline")
        .ok_or(InspectionError::Internal)?;
    let candidate_capture = events
        .iter()
        .position(|event| event == "capture:candidate")
        .ok_or(InspectionError::Internal)?;
    let run = events
        .iter()
        .position(|event| event == "run")
        .ok_or(InspectionError::Internal)?;
    assert!(baseline_capture < candidate_capture && candidate_capture < run);
    assert!(
        events[run + 1..]
            .iter()
            .filter(|event| *event == "revalidate:baseline")
            .count()
            >= 2
    );
    assert!(
        events[run + 1..]
            .iter()
            .filter(|event| *event == "revalidate:candidate")
            .count()
            >= 2
    );
    assert!(
        store
            .entry
            .as_ref()
            .is_some_and(|(_, bytes)| bytes.starts_with(b"=== stdout ==="))
    );
    Ok(())
}

#[test]
fn missing_candidate_library_is_unavailable_before_execution() -> TestResult {
    let log = Arc::new(Mutex::new(Vec::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let clock = TestClock::default();
    let control = Control;
    let mut registry = ProjectRegistry::new(
        Backend {
            log: Arc::clone(&log),
        },
        Generator(AtomicUsize::new(0)),
        clock.clone(),
        60,
        2,
    )?;
    let baseline = registry.open("/baseline", &control)?.project_ref;
    let candidate = registry.open("/candidate", &control)?.project_ref;
    let result = registry.semver_check(
        &baseline,
        &candidate,
        &options()?,
        &Inspector {
            log: Arc::clone(&log),
            candidate_has_lib: false,
        },
        &Runner {
            log,
            calls: Arc::clone(&calls),
        },
        &mut Store::default(),
        &clock,
        &control,
    )?;
    assert_eq!(result.outcome, SemverOutcome::Unavailable);
    assert!(result.observation.is_none());
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn stage0_quota_omits_raw_output_without_changing_the_coarse_outcome() -> TestResult {
    let log = Arc::new(Mutex::new(Vec::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let clock = TestClock::default();
    let control = Control;
    let mut registry = ProjectRegistry::new(
        Backend {
            log: Arc::clone(&log),
        },
        Generator(AtomicUsize::new(0)),
        clock.clone(),
        60,
        2,
    )?;
    let baseline = registry.open("/baseline", &control)?.project_ref;
    let candidate = registry.open("/candidate", &control)?.project_ref;
    let result = registry.semver_check(
        &baseline,
        &candidate,
        &options()?,
        &Inspector {
            log: Arc::clone(&log),
            candidate_has_lib: true,
        },
        &Runner { log, calls },
        &mut QuotaStore,
        &clock,
        &control,
    )?;
    assert_eq!(result.outcome, SemverOutcome::NoBreak);
    assert!(result.raw_output.is_none());
    assert!(result.raw_output_omitted);
    Ok(())
}
