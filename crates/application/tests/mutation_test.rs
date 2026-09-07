//! Application-level contract for `rust.mutation.test` (M3-05).

use rust_engineering_application::mutation_test::{
    MUTATION_STAGE0_MAX_BYTES, MutationArtifactKind, MutationArtifactReference,
    MutationArtifactStreams, MutationCompleteness, MutationTestObservation, MutationTestTaskResult,
    ProjectMutationTestPort, synchronous_qualified, total_budget_seconds,
};
use rust_engineering_application::{
    ArtifactInput, ArtifactStore, ExecutionCancellation, InspectionControl, InspectionError,
    OperationControl, ProjectBackend, ProjectError, ProjectIdentity, ProjectRegistry,
    ProjectSourceBackend, ReferenceGenerator, RegistryClock, ValidatedProject,
};
use rust_engineering_domain::mutation_test::{
    MutationBaseline, MutationCounts, MutationGuestIdentity, MutationTestCommandOptions,
    MutationTestSelection,
};
use rust_engineering_domain::{
    ArtifactError, ArtifactId, ArtifactMetadata, ArtifactView, Clock, ExecutionFingerprint,
    ExecutionTermination, ProjectRef, RuntimeIdentity, SourceBundle, SourceFile, UnixSeconds,
};

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

struct Ticks;
impl RegistryClock for Ticks {
    fn seconds(&self) -> u64 {
        1
    }
}
struct Wall;
impl Clock for Wall {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(1)
    }
}

struct Backend;
impl ProjectBackend for Backend {
    type Lease = ();
    fn open(
        &self,
        _: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<()>, ProjectError> {
        Ok(ValidatedProject {
            identity: identity()?,
            lease: (),
        })
    }
    fn revalidate(
        &self,
        _: &(),
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        identity()
    }
}
impl ProjectSourceBackend for Backend {
    fn source(&self, _: &(), _: &dyn OperationControl) -> Result<SourceBundle, ProjectError> {
        SourceBundle::new(vec![
            SourceFile::new(
                "src/lib.rs".into(),
                b"pub fn answer() -> u8 { 42 }\n".to_vec(),
            )
            .map_err(|_| ProjectError::Internal)?,
        ])
        .map_err(|_| ProjectError::Internal)
    }
}
fn identity() -> Result<ProjectIdentity, ProjectError> {
    Ok(ProjectIdentity {
        workspace_root: "/trusted".into(),
        fingerprint: format!("sha256:{:064x}", 1)
            .parse()
            .map_err(|_| ProjectError::Internal)?,
    })
}

struct Generator;
impl ReferenceGenerator for Generator {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        "prj_00000000000000000000000000000001"
            .parse()
            .map_err(|_| ProjectError::Internal)
    }
}

/// Records every published stream so the test can assert exactly which
/// artifacts Stage 0 accepted, and truncates like the real bounded store.
#[derive(Default)]
struct Store {
    captured: Vec<(usize, bool)>,
    limit: usize,
    next: u8,
}
impl ArtifactStore for Store {
    fn capture(
        &mut self,
        owner: &ProjectRef,
        input: &mut dyn ArtifactInput,
    ) -> Result<ArtifactMetadata, ArtifactError> {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 512];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..read]);
        }
        let truncated = bytes.len() > self.limit;
        if truncated {
            bytes.truncate(self.limit);
        }
        self.captured.push((bytes.len(), truncated));
        self.next += 1;
        Ok(ArtifactMetadata {
            owner: owner.clone(),
            id: format!("art_{:032x}", self.next)
                .parse::<ArtifactId>()
                .map_err(|_| ArtifactError::InvalidLimits)?,
            sha256: [0u8; 32],
            size_bytes: u32::try_from(bytes.len()).map_err(|_| ArtifactError::InvalidLimits)?,
            truncated,
            created_seconds: 1,
            expires_seconds: 3_600,
        })
    }
    fn read<'a>(
        &'a mut self,
        _: &ProjectRef,
        _: &ArtifactId,
    ) -> Result<ArtifactView<'a>, ArtifactError> {
        Err(ArtifactError::NotFound)
    }
    fn remove(&mut self, _: &ProjectRef, _: &ArtifactId) -> Result<bool, ArtifactError> {
        Ok(true)
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

struct Port(MutationTestObservation);
impl ProjectMutationTestPort for Port {
    fn run(
        &self,
        _: &SourceBundle,
        _: &MutationTestCommandOptions,
        _: &dyn InspectionControl,
    ) -> Result<MutationTestObservation, InspectionError> {
        Ok(self.0.clone())
    }
}

fn observation() -> Result<MutationTestObservation, Box<dyn std::error::Error>> {
    let fingerprint: ExecutionFingerprint = format!("sha256:{:064x}", 2).parse()?;
    Ok(MutationTestObservation {
        options: MutationTestCommandOptions::try_from(MutationTestSelection::default())?,
        completeness: MutationCompleteness::Complete,
        validation_complete: true,
        baseline: MutationBaseline::Passed,
        counts: MutationCounts {
            generated: 2,
            tested: 2,
            caught: 2,
            ..Default::default()
        },
        mutants: Vec::new(),
        mutants_omitted: 0,
        cap_exceeded: false,
        mutants_version: "27.1.0".into(),
        guest_identity: MutationGuestIdentity::Guest,
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: format!("sha256:{:064x}", 3),
            configuration_fingerprint: fingerprint.clone(),
            execution_fingerprint: fingerprint.clone(),
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        execution_fingerprint: fingerprint,
        artifacts: MutationArtifactStreams::default(),
    })
}

fn opened() -> Result<ProjectRegistry<Backend, Generator, Ticks>, ProjectError> {
    let mut registry = ProjectRegistry::new(Backend, Generator, Ticks, 3_600, 4)?;
    registry.open("/trusted", &Control)?;
    Ok(registry)
}

#[test]
fn a_clean_report_requires_every_containment_and_completeness_clause()
-> Result<(), Box<dyn std::error::Error>> {
    let base = observation()?;
    assert!(base.clean());
    assert!(!base.conclusive_failure());
    type Mutate = fn(&mut MutationTestObservation);
    let mutations: [(&str, Mutate); 7] = [
        ("partial evidence", |value| {
            value.completeness = MutationCompleteness::Partial;
        }),
        ("incomplete validation", |value| {
            value.validation_complete = false;
        }),
        ("failing baseline", |value| {
            value.baseline = MutationBaseline::Failed;
        }),
        ("missing baseline", |value| {
            value.baseline = MutationBaseline::Missing;
        }),
        ("untested mutants", |value| value.counts.generated = 3),
        ("capped selection", |value| value.cap_exceeded = true),
        ("host-shaped identity", |value| {
            value.guest_identity = MutationGuestIdentity::Redacted;
        }),
    ];
    for (label, mutate) in mutations {
        let mut value = base.clone();
        mutate(&mut value);
        assert!(!value.clean(), "{label}");
    }
    // Timeout and unviable classes never credit a clean result either.
    for counts in [
        MutationCounts {
            generated: 2,
            tested: 2,
            caught: 1,
            timeout: 1,
            ..Default::default()
        },
        MutationCounts {
            generated: 2,
            tested: 2,
            caught: 1,
            unviable: 1,
            ..Default::default()
        },
    ] {
        let mut value = base.clone();
        value.counts = counts;
        assert!(!value.clean());
        assert!(!value.conclusive_failure());
    }
    Ok(())
}

#[test]
fn a_missed_mutant_or_failing_baseline_is_conclusive_without_extra_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let mut missed = observation()?;
    missed.counts = MutationCounts {
        generated: 2,
        tested: 2,
        caught: 1,
        missed: 1,
        ..Default::default()
    };
    // Deliberately degraded secondary evidence: the failure still stands.
    missed.completeness = MutationCompleteness::Partial;
    missed.validation_complete = false;
    missed.artifacts.bundle_unavailable = true;
    assert!(missed.conclusive_failure());
    assert!(!missed.clean());

    let mut baseline = observation()?;
    baseline.baseline = MutationBaseline::Failed;
    baseline.counts = MutationCounts::default();
    baseline.validation_complete = false;
    assert!(baseline.conclusive_failure());

    // Evidence that could not be parsed at all is never conclusive.
    for completeness in [
        MutationCompleteness::Invalid,
        MutationCompleteness::Unavailable,
    ] {
        let mut unusable = missed.clone();
        unusable.completeness = completeness;
        assert!(!unusable.conclusive_failure());
    }
    // Neither is a run that never terminated normally.
    let mut cancelled = missed.clone();
    cancelled.termination = ExecutionTermination::TimedOut;
    assert!(!cancelled.conclusive_failure());
    Ok(())
}

#[test]
fn validation_bounds_reject_inconsistent_or_oversized_observations()
-> Result<(), Box<dyn std::error::Error>> {
    let base = observation()?;
    assert!(base.validate().is_ok());
    for mutate in [
        (|value: &mut MutationTestObservation| {
            // Class counts that do not add up while claiming completeness.
            value.counts.caught = 1;
        }) as fn(&mut MutationTestObservation),
        |value| value.mutants_version = "v".repeat(65),
        |value| value.runtime.image_id = "sha256:zz".into(),
        |value| value.runtime.rust_version = String::new(),
        |value| {
            // `validation_complete` may not be claimed alongside an omission.
            value.mutants_omitted = 1;
        },
        |value| value.cap_exceeded = true,
        |value| value.artifacts.bundle_unavailable = true,
        |value| value.artifacts.outcomes_truncated = true,
        |value| value.mutants_version = String::new(),
        |value| value.completeness = MutationCompleteness::Partial,
    ] {
        let mut value = base.clone();
        mutate(&mut value);
        assert!(value.validate().is_err());
    }
    Ok(())
}

#[test]
fn task_results_erase_streams_and_stay_bounded() -> Result<(), Box<dyn std::error::Error>> {
    let mut value = observation()?;
    value.artifacts.outcomes_json = b"{}".to_vec();
    value.artifacts.report_bundle = vec![0u8; 1024];
    value.artifacts.stdout = b"tool text".to_vec();
    value.artifacts.stderr = b"tool text".to_vec();
    let result = MutationTestTaskResult::new(value.clone(), Vec::new(), 1)?;
    let (stored, artifacts, expected, duration) = result.into_parts();
    assert!(stored.artifacts.outcomes_json.is_empty());
    assert!(stored.artifacts.report_bundle.is_empty());
    assert!(stored.artifacts.stdout.is_empty());
    assert!(stored.artifacts.stderr.is_empty());
    assert_eq!(expected, 4);
    assert!(artifacts.is_empty());
    assert_eq!(duration, 1);
    assert!(MutationTestTaskResult::new(value.clone(), Vec::new(), 3_840_001).is_err());
    let mut invalid = value;
    invalid.mutants_version = String::new();
    assert!(MutationTestTaskResult::new(invalid, Vec::new(), 1).is_err());
    Ok(())
}

#[test]
fn stage0_publishes_the_report_but_never_a_truncated_bundle()
-> Result<(), Box<dyn std::error::Error>> {
    let project: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let options = MutationTestCommandOptions::try_from(MutationTestSelection::default())?;
    let mut value = observation()?;
    value.artifacts.outcomes_json = b"{\"cargo_mutants_version\":\"27.1.0\"}".to_vec();
    value.artifacts.report_bundle = vec![7u8; 4096];
    value.artifacts.stdout = b"bounded tool text".to_vec();
    let mut registry = opened().map_err(|error| format!("{error:?}"))?;
    let mut store = Store {
        limit: MUTATION_STAGE0_MAX_BYTES,
        ..Default::default()
    };
    let (observed, published) = registry
        .mutation_test(
            &project,
            &options,
            &Port(value.clone()),
            &mut store,
            &Wall,
            &Control,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(published.len(), 3);
    assert!(observed.validation_complete);
    assert!(matches!(
        published[0],
        MutationArtifactReference::Ephemeral {
            kind: MutationArtifactKind::OutcomesJson,
            ..
        }
    ));
    assert!(matches!(
        published[1],
        MutationArtifactReference::Ephemeral {
            kind: MutationArtifactKind::ArchiveBundle,
            ..
        }
    ));
    assert!(store.captured.iter().all(|(_, truncated)| !truncated));

    // A bundle above the Stage-0 ceiling is dropped whole: half a tar archive
    // is not an archive, and the observation records the omission.
    let mut oversized = value;
    oversized.artifacts.report_bundle = vec![7u8; MUTATION_STAGE0_MAX_BYTES + 1];
    let mut store = Store {
        limit: MUTATION_STAGE0_MAX_BYTES,
        ..Default::default()
    };
    let mut registry = opened().map_err(|error| format!("{error:?}"))?;
    let (observed, published) = registry
        .mutation_test(
            &project,
            &options,
            &Port(oversized),
            &mut store,
            &Wall,
            &Control,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(published.len(), 2);
    assert!(observed.artifacts.bundle_unavailable);
    assert!(!observed.validation_complete);
    assert!(!observed.clean());
    assert!(!published.iter().any(|artifact| matches!(
        artifact,
        MutationArtifactReference::Ephemeral {
            kind: MutationArtifactKind::ArchiveBundle,
            ..
        }
    )));
    Ok(())
}

#[test]
fn the_derived_job_budget_never_admits_a_synchronous_run() -> Result<(), Box<dyn std::error::Error>>
{
    for (max_mutants, mutant_timeout_seconds, expected) in
        [(1, 1, 300), (10, 30, 450), (100, 60, 3_600)]
    {
        let options = MutationTestCommandOptions::try_from(MutationTestSelection {
            max_mutants,
            mutant_timeout_seconds,
            ..Default::default()
        })?;
        assert_eq!(total_budget_seconds(&options), expected);
        assert!(total_budget_seconds(&options) <= 3_600);
        assert!(!synchronous_qualified(&options));
    }
    Ok(())
}
