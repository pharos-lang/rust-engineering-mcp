use super::*;
use crate::stdio::workers::WorkerError;
use rust_engineering_domain::{
    AuditFinding, AuditPackage, AuditPath, AuditSource, FreshnessPolicy, InspectionSemantics,
    Provenance, SnapshotEvidence,
};
use rust_engineering_domain::{Clock, UnixSeconds};
use serde_json::{Value, json};
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(102)
    }
}
fn evidence(
    kind: SourceKind,
    created: Option<UnixSeconds>,
    integrity: IntegrityStatus,
) -> Result<SnapshotEvidence, Box<dyn std::error::Error>> {
    Ok(SnapshotEvidence::assess(
        Provenance::new(
            kind,
            format!("sha256:{:064x}", 42).parse()?,
            created,
            Some(UnixSeconds(101)),
            integrity,
            false,
        )?,
        FreshnessPolicy::new("snapshot-v1".parse()?, 10, 20)?,
        &FixedClock,
    ))
}
fn project() -> Result<ProjectAudit, Box<dyn std::error::Error>> {
    let fingerprint = format!("sha256:{:064x}", 42);
    Ok(ProjectAudit {
        project_ref: "prj_00000000000000000000000000000001".parse()?,
        project_identity_fingerprint: fingerprint.parse()?,
        semantics: InspectionSemantics::LatestKnown,
        source_fingerprint: fingerprint.parse()?,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: fingerprint.clone(),
            configuration_fingerprint: fingerprint.parse()?,
            execution_fingerprint: fingerprint.parse()?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        observation: AuditObservation {
            state: AuditState::Passed,
            issue: None,
            validation_complete: true,
            lock_fingerprint: Some(fingerprint.parse()?),
            snapshot_fingerprint: Some(fingerprint.parse()?),
            snapshot: Some(evidence(
                SourceKind::RustsecSnapshot,
                Some(UnixSeconds(100)),
                IntegrityStatus::Verified,
            )?),
            snapshot_record_count: Some(1),
            snapshot_sequence: Some(1),
            packages_total: 2,
            crates_io_scanned: 1,
            workspace_packages_excluded: 1,
            unsupported_packages: vec![],
            findings: vec![],
            informational: vec![],
            findings_omitted: 0,
        },
        evidence: evidence(
            SourceKind::ProjectSnapshot,
            Some(UnixSeconds(100)),
            IntegrityStatus::Verified,
        )?,
    })
}
fn finding() -> AuditFinding {
    let package = AuditPackage {
        name: "example".into(),
        version: "1.0.0".into(),
        source: AuditSource::CratesIo,
        source_fingerprint: None,
    };
    AuditFinding {
        advisory_id: "RUSTSEC-2026-0001".into(),
        url: "https://rustsec.org/advisories/RUSTSEC-2026-0001.html".into(),
        title: "Example vulnerability".into(),
        package: package.clone(),
        patched_requirements: vec![">=1.0.1".into()],
        unaffected_requirements: vec![],
        severity: None,
        informational: None,
        paths: vec![AuditPath {
            workspace_root: package.clone(),
            packages: vec![package],
        }],
        paths_omitted: 0,
    }
}
fn encoded(project: ProjectAudit) -> Result<CallToolResult, ErrorData> {
    encode_bounded(&Contract::<Input, Output>::new()?, output(Ok(project), 1)?)
}
fn data(result: &CallToolResult) -> Result<&Value, Box<dyn std::error::Error>> {
    result
        .structured_content
        .as_ref()
        .ok_or_else(|| "missing structured content".into())
}
#[test]
fn closed_input_only_accepts_live_reference_shape() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for patch in [
        json!({"path":"/secret"}),
        json!({"snapshot":"/secret"}),
        json!({"download":true}),
        json!({"package":"example"}),
        json!({"features":[]}),
        json!({"args":[]}),
        json!({"project_ref":"secret"}),
    ] {
        let mut input = json!({"project_ref":"prj_00000000000000000000000000000001"});
        input
            .as_object_mut()
            .ok_or("object")?
            .extend(patch.as_object().ok_or("object")?.clone());
        assert!(
            contract
                .decode(Some(serde_json::from_value(input)?))
                .is_err()
        );
    }
    assert!(contract.decode(None).is_err());
    Ok(())
}
#[test]
fn passed_and_vulnerable_results_keep_both_evidence_sources() -> TestResult {
    let result = encoded(project()?)?;
    assert_eq!(result.is_error, Some(false));
    let value = data(&result)?;
    assert_eq!(value["status"], "passed");
    assert_eq!(value["data"]["semantics"], "latest_known");
    assert_eq!(
        value["evidence"]["details"]["provenance"]["source_kind"],
        "project_snapshot"
    );
    assert_eq!(
        value["data"]["observation"]["snapshot"]["provenance"]["source_kind"],
        "rustsec_snapshot"
    );
    let mut project = project()?;
    // The adapter must not trust an inconsistent Passed returned by a port.
    project.observation.findings.push(finding());
    let result = encoded(project)?;
    assert_eq!(result.is_error, Some(false));
    assert_eq!(data(&result)?["status"], "failed");
    Ok(())
}
#[test]
fn incomplete_port_facts_never_pass() -> TestResult {
    for change in 0..8 {
        let mut project = project()?;
        match change {
            0 => project.observation.validation_complete = false,
            1 => project.observation.lock_fingerprint = None,
            2 => project.observation.findings_omitted = 1,
            3 => project
                .observation
                .unsupported_packages
                .push(finding().package),
            4 => {
                let mut finding = finding();
                finding.paths_omitted = 1;
                project.observation.informational.push(finding);
            }
            5 => project.observation.state = AuditState::Incomplete,
            6 => project.observation.issue = Some(AuditIssue::UnsupportedSources),
            _ => project.observation.crates_io_scanned = 0,
        }
        let result = encoded(project)?;
        assert_ne!(data(&result)?["status"], "passed", "case {change}");
        assert_eq!(
            data(&result)?["data"]["observation"]["validation_complete"],
            false
        );
    }
    Ok(())
}
#[test]
fn absent_stale_unknown_and_unverified_snapshots_cannot_pass() -> TestResult {
    for (created, integrity, code) in [
        (
            Some(UnixSeconds(1)),
            IntegrityStatus::Verified,
            "AUDIT_SNAPSHOT_STALE",
        ),
        (
            None,
            IntegrityStatus::Verified,
            "AUDIT_SNAPSHOT_UNKNOWN_AGE",
        ),
        (
            Some(UnixSeconds(100)),
            IntegrityStatus::Unverified,
            "AUDIT_INTEGRITY_FAILED",
        ),
    ] {
        let mut project = project()?;
        project.observation.snapshot =
            Some(evidence(SourceKind::RustsecSnapshot, created, integrity)?);
        let result = encoded(project)?;
        assert_eq!(result.is_error, Some(true));
        assert_eq!(data(&result)?["error_code"], code);
        assert!(data(&result)?["data"]["observation"]["snapshot"].is_object());
    }
    let mut project = project()?;
    project.observation = AuditObservation::unavailable();
    let result = encoded(project)?;
    assert_eq!(data(&result)?["status"], "unavailable");
    assert!(data(&result)?["data"].is_object());
    Ok(())
}
#[test]
fn informational_records_do_not_fail_clean_audit() -> TestResult {
    let mut project = project()?;
    let mut finding = finding();
    finding.informational = Some("unmaintained".into());
    project.observation.informational.push(finding);
    assert_eq!(data(&encoded(project)?)?["status"], "passed");
    Ok(())
}
#[test]
fn audit_error_codes_are_explicit_and_schema_valid() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for (error, code) in [
        (AuditDataError::InvalidSnapshot, "AUDIT_SNAPSHOT_INVALID"),
        (AuditDataError::Integrity, "AUDIT_INTEGRITY_FAILED"),
        (AuditDataError::MissingLockfile, "AUDIT_LOCKFILE_MISSING"),
        (AuditDataError::InvalidLockfile, "AUDIT_LOCKFILE_INVALID"),
        (AuditDataError::Budget, "AUDIT_BUDGET_EXCEEDED"),
    ] {
        let result = contract.encode(output(Err(ProjectAuditError::Data(error)), 1)?)?;
        assert_eq!(result.is_error, Some(true));
        assert_eq!(data(&result)?["status"], "blocked");
        assert_eq!(data(&result)?["error_code"], code);
    }
    assert!(output(Err(ProjectAuditError::Data(AuditDataError::Internal)), 1).is_err());
    Ok(())
}
#[test]
fn bounded_response_marks_omissions_and_downgrades_pass() -> TestResult {
    let mut project = project()?;
    let mut finding = finding();
    finding.title = "\\\"".repeat(1000);
    finding.paths[0].packages = vec![finding.package.clone(); 100];
    finding.informational = Some("unmaintained".into());
    project.observation.informational = vec![finding; 150];
    let result = encoded(project)?;
    assert!(serde_json::to_vec(&result)?.len() <= MAX_RESULT);
    let value = data(&result)?;
    assert_ne!(value["status"], "passed");
    assert_eq!(value["data"]["observation"]["validation_complete"], false);
    assert!(
        value["truncation"]["paths_omitted"]
            .as_u64()
            .ok_or("count")?
            > 0
    );
    assert!(
        value["truncation"]["findings_omitted"]
            .as_u64()
            .ok_or("count")?
            > 0
    );
    Ok(())
}
#[test]
fn joined_hard_errors_survive_interrupts() {
    let joined = Joined::<(), _> {
        result: Err(ProjectAuditError::Inspection(InspectionError::Execution(
            ExecutionError::CleanupUncertain,
        ))),
        interrupted: Some(WorkerError::Cancelled),
    };
    assert_eq!(
        joined_result(joined),
        Err(ProjectAuditError::Inspection(InspectionError::Execution(
            ExecutionError::CleanupUncertain
        )))
    );
    let joined = Joined::<(), _> {
        result: Err(ProjectAuditError::Data(AuditDataError::Cancelled)),
        interrupted: Some(WorkerError::TimedOut),
    };
    assert_eq!(
        joined_result(joined),
        Err(ProjectAuditError::Inspection(InspectionError::Project(
            ProjectError::Rejected(OperationalErrorCode::CommandTimeout)
        )))
    );
}

#[test]
fn contradictory_snapshot_provenance_is_blocked() -> TestResult {
    let mut project = project()?;
    project.observation.snapshot = Some(evidence(
        SourceKind::RegistrySnapshot,
        Some(UnixSeconds(100)),
        IntegrityStatus::Verified,
    )?);
    let result = encoded(project)?;
    assert_eq!(data(&result)?["status"], "blocked");
    assert_eq!(data(&result)?["error_code"], "AUDIT_SNAPSHOT_INVALID");
    assert_eq!(data(&result)?["evidence"]["kind"], "snapshot");
    Ok(())
}

#[test]
fn snapshot_coverage_count_and_sequence_are_required_for_pass() -> TestResult {
    for (count, sequence) in [
        (None, Some(1)),
        (Some(0), Some(1)),
        (Some(1), None),
        (Some(1), Some(0)),
    ] {
        let mut project = project()?;
        project.observation.snapshot_record_count = count;
        project.observation.snapshot_sequence = sequence;
        let result = encoded(project)?;
        let value = data(&result)?;
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["data"]["observation"]["state"], "incomplete");
        assert_eq!(value["data"]["observation"]["validation_complete"], false);
        assert_eq!(
            value["data"]["observation"]["snapshot_record_count"],
            json!(count)
        );
        assert_eq!(
            value["data"]["observation"]["snapshot_sequence"],
            json!(sequence)
        );
    }
    let result = encoded(project()?)?;
    assert_eq!(data(&result)?["status"], "passed");
    assert_eq!(
        data(&result)?["data"]["observation"]["snapshot_sequence"],
        1
    );
    assert_eq!(
        data(&result)?["data"]["observation"]["snapshot_record_count"],
        1
    );
    Ok(())
}

#[test]
fn fresh_findings_fail_even_when_source_or_output_coverage_is_incomplete() -> TestResult {
    for issue in [AuditIssue::UnsupportedSources, AuditIssue::OutputBudget] {
        let mut project = project()?;
        project.observation.issue = Some(issue);
        project.observation.validation_complete = false;
        project.observation.findings.push(finding());
        let result = encoded(project)?;
        let value = data(&result)?;
        assert_eq!(result.is_error, Some(false));
        assert_eq!(value["status"], "failed");
        assert_eq!(value["data"]["observation"]["state"], "failed");
        assert_eq!(value["data"]["observation"]["validation_complete"], false);
        assert_eq!(value["data"]["observation"]["issue"], json!(issue));
    }
    Ok(())
}

#[test]
fn stale_and_unknown_snapshot_take_precedence_over_retained_findings() -> TestResult {
    for created in [Some(UnixSeconds(1)), None] {
        let mut project = project()?;
        project.observation.snapshot = Some(evidence(
            SourceKind::RustsecSnapshot,
            created,
            IntegrityStatus::Verified,
        )?);
        project.observation.findings.push(finding());
        let result = encoded(project)?;
        let value = data(&result)?;
        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["data"]["observation"]["state"], "unavailable");
        assert_eq!(
            value["data"]["observation"]["findings"]
                .as_array()
                .ok_or("findings")?
                .len(),
            1
        );
        assert!(value["data"]["observation"]["snapshot"].is_object());
    }
    Ok(())
}

#[test]
fn present_but_invalid_integrity_is_not_reported_as_missing_snapshot() -> TestResult {
    let mut project = project()?;
    project.observation.snapshot = Some(evidence(
        SourceKind::RustsecSnapshot,
        Some(UnixSeconds(100)),
        IntegrityStatus::Failed,
    )?);
    let result = encoded(project)?;
    let value = data(&result)?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["error_code"], "AUDIT_INTEGRITY_FAILED");
    assert_eq!(value["data"]["observation"]["state"], "incomplete");
    assert_eq!(value["data"]["observation"]["issue"], Value::Null);
    assert!(value["data"]["observation"]["snapshot"].is_object());
    Ok(())
}

#[test]
fn snapshot_runtime_errors_keep_operational_codes_and_status() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for (error, code, status) in [
        (AuditDataError::Timeout, "COMMAND_TIMEOUT", "blocked"),
        (AuditDataError::SandboxDenied, "SANDBOX_DENIED", "blocked"),
        (
            AuditDataError::UnsupportedPlatform,
            "UNSUPPORTED_PLATFORM",
            "unavailable",
        ),
    ] {
        let result = contract.encode(output(Err(ProjectAuditError::Data(error)), 1)?)?;
        assert_eq!(result.is_error, Some(true));
        assert_eq!(data(&result)?["error_code"], code);
        assert_eq!(data(&result)?["status"], status);
    }
    Ok(())
}

#[test]
fn joined_snapshot_timeouts_preserve_signal_and_hard_errors() {
    for timeout in [false, true] {
        let interrupted = || {
            if timeout {
                WorkerError::TimedOut
            } else {
                WorkerError::Cancelled
            }
        };
        let joined = Joined::<(), _> {
            result: Err(ProjectAuditError::Data(AuditDataError::Timeout)),
            interrupted: Some(interrupted()),
        };
        assert_eq!(
            joined_result(joined),
            Err(ProjectAuditError::Inspection(worker_error(interrupted())))
        );
        for error in [
            AuditDataError::Internal,
            AuditDataError::Integrity,
            AuditDataError::SandboxDenied,
        ] {
            let joined = Joined::<(), _> {
                result: Err(ProjectAuditError::Data(error)),
                interrupted: Some(interrupted()),
            };
            assert_eq!(joined_result(joined), Err(ProjectAuditError::Data(error)));
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn provider_read_checkpoint_errors_reach_the_expected_public_outcome() -> TestResult {
    use rust_engineering_application::{
        DependencyAuditPort, ExecutionCancellation, OperationControl,
    };
    use rust_engineering_domain::{
        CargoConfiguration, ProjectConfigPolicy, ProjectStructure, SourceBundle,
    };
    struct RejectedControl(ProjectError);
    impl OperationControl for RejectedControl {
        fn check(&self) -> Result<(), ProjectError> {
            Err(self.0)
        }
    }
    impl ExecutionCancellation for RejectedControl {
        fn is_cancelled(&self) -> bool {
            matches!(self.0, ProjectError::Cancelled)
        }
    }
    let source = SourceBundle::new(vec![]).map_err(|error| format!("source: {error:?}"))?;
    let captured = project()?;
    let structure = ProjectStructure {
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
        runtime: captured.runtime,
        source_fingerprint: captured.source_fingerprint,
    };
    let provider = provider::AuditProvider(Some(provider::HostAuditConfig {
        path: "/snapshot-checkpoint-never-opens.json".into(),
        fingerprint: format!("sha256:{:064x}", 42).parse()?,
    }));
    for (project_error, expected) in [
        (
            ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
            AuditDataError::Timeout,
        ),
        (
            ProjectError::Rejected(OperationalErrorCode::SandboxDenied),
            AuditDataError::SandboxDenied,
        ),
        (
            ProjectError::Rejected(OperationalErrorCode::OutputLimitExceeded),
            AuditDataError::Budget,
        ),
        (
            ProjectError::Rejected(OperationalErrorCode::UnsupportedPlatform),
            AuditDataError::UnsupportedPlatform,
        ),
        (
            ProjectError::Rejected(OperationalErrorCode::InvalidProject),
            AuditDataError::InvalidSnapshot,
        ),
        (ProjectError::Cancelled, AuditDataError::Cancelled),
        (ProjectError::Internal, AuditDataError::Internal),
    ] {
        let result = provider.audit(
            &source,
            &structure,
            &FixedClock,
            &RejectedControl(project_error),
        );
        assert_eq!(result.err(), Some(expected));
    }
    let observation = provider
        .audit(
            &source,
            &structure,
            &FixedClock,
            &RejectedControl(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            )),
        )
        .map_err(|error| format!("audit: {error:?}"))?;
    let mut project = project()?;
    project.observation = observation;
    let result = encoded(project)?;
    let value = data(&result)?;
    assert_eq!(value["status"], "unavailable");
    assert_eq!(value["error_code"], "AUDIT_SNAPSHOT_UNAVAILABLE");
    assert_eq!(value["data"]["observation"]["state"], "unavailable");
    assert_eq!(value["evidence"]["kind"], "snapshot");
    Ok(())
}
