use super::*;
use crate::stdio::workers::WorkerError;
use rust_engineering_domain::UnixSeconds;
use rust_engineering_domain::{
    ArtifactMetadata, AuditIssue, CheckOutcome, DiagnosticSource, FormatObservation,
    FreshnessPolicy, InspectionSemantics, IntegrityStatus, Provenance, Severity, SnapshotEvidence,
    SourceKind, TestObservation,
};
use serde_json::{Value, json};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn project(profile: QualityProfile) -> Result<ProjectQualityGate, Box<dyn std::error::Error>> {
    let fingerprint = format!("sha256:{:064x}", 42);
    let reference: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    struct Clock;
    impl rust_engineering_domain::Clock for Clock {
        fn now(&self) -> UnixSeconds {
            UnixSeconds(102)
        }
    }
    let evidence = SnapshotEvidence::assess(
        Provenance::new(
            SourceKind::ProjectSnapshot,
            fingerprint.parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(101)),
            IntegrityStatus::Verified,
            false,
        )?,
        FreshnessPolicy::new("captured-project-v1".parse()?, 60, 300)?,
        &Clock,
    );
    let execution = CheckObservation {
        outcome: CheckOutcome::Passed,
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        validation_complete: true,
        diagnostics: vec![Diagnostic {
            source: DiagnosticSource::Rustc,
            severity: Severity::Note,
            code: Some("test".parse()?),
            message: "repair detail".parse()?,
            spans: vec![],
            rendered: None,
            suggestions: vec![],
            truncated: false,
        }],
        diagnostics_omitted: 0,
        stdout: "RAW_SECRET_STDOUT".into(),
        stderr: "RAW_SECRET_STDERR".into(),
        stdout_truncated: false,
        stderr_truncated: false,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: fingerprint.clone(),
            configuration_fingerprint: fingerprint.parse()?,
            execution_fingerprint: fingerprint.parse()?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        source_fingerprint: fingerprint.parse()?,
    };
    let stages = profile
        .stages()
        .iter()
        .enumerate()
        .map(|(index, &stage)| {
            let observation = match stage {
                QualityStage::Format => QualityObservation::Format(FormatObservation {
                    execution: execution.clone(),
                    affected_files: vec![],
                    affected_files_omitted: 0,
                    diff: None,
                    diff_omitted: false,
                }),
                QualityStage::Check => QualityObservation::Check(execution.clone()),
                QualityStage::Clippy => QualityObservation::Clippy(execution.clone()),
                QualityStage::Test => QualityObservation::Test(TestObservation {
                    execution: execution.clone(),
                    build_succeeded: Some(true),
                }),
                QualityStage::Audit => QualityObservation::Audit {
                    runtime: execution.runtime.clone(),
                    observation: AuditObservation::unavailable(),
                },
            };
            let log = if stage == QualityStage::Audit {
                None
            } else {
                Some(ArtifactMetadata {
                    owner: reference.clone(),
                    id: format!("art_{:032x}", index + 1).parse()?,
                    sha256: [42; 32],
                    size_bytes: 3,
                    truncated: false,
                    created_seconds: 0,
                    expires_seconds: 3600,
                })
            };
            Ok(QualityStageReport {
                stage,
                duration_ms: 1,
                status: ToolStatus::Passed,
                issue: None,
                observation: Some(observation),
                retention_remaining_seconds: log.as_ref().map(|_| 3599),
                log,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    Ok(ProjectQualityGate {
        project_ref: reference,
        project_identity_fingerprint: format!("sha256:{:064x}", 1).parse()?,
        semantics: InspectionSemantics::LatestKnown,
        profile,
        source_fingerprint: Some(fingerprint.parse()?),
        stages,
        evidence: Evidence::Snapshot(evidence),
    })
}
fn encoded(project: ProjectQualityGate) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(
        encode_bounded(&Contract::<Input, Output>::new()?, output(Ok(project), 12)?)?
            .structured_content
            .ok_or("content")?,
    )
}
#[test]
fn only_reference_and_explicit_closed_profile_are_accepted() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    assert!(contract.decode(None).is_err());
    for invalid in [
        json!({"project_ref":"prj_00000000000000000000000000000001"}),
        json!({"project_ref":"/secret","profile":"fast"}),
        json!({"project_ref":"prj_00000000000000000000000000000001","profile":"full"}),
        json!({"project_ref":"prj_00000000000000000000000000000001","profile":null}),
    ] {
        assert!(
            contract
                .decode(Some(serde_json::from_value(invalid)?))
                .is_err()
        );
    }
    for field in [
        "args",
        "command",
        "workspace",
        "features",
        "target",
        "package",
        "timeout",
        "snapshot_path",
    ] {
        let mut value =
            json!({"project_ref":"prj_00000000000000000000000000000001","profile":"fast"});
        value[field] = json!("arbitrary");
        let error = contract
            .decode(Some(serde_json::from_value(value)?))
            .err()
            .ok_or("input accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(error.data.is_none());
    }
    for profile in ["fast", "standard"] {
        assert!(
            contract
                .decode(Some(serde_json::from_value(
                    json!({"project_ref":"prj_00000000000000000000000000000001","profile":profile})
                )?))
                .is_ok()
        );
    }
    Ok(())
}
#[test]
fn profiles_keep_rows_repair_facts_resources_and_exact_text_fallback() -> TestResult {
    for (profile, count, status) in [
        (QualityProfile::Fast, 3, "passed"),
        (QualityProfile::Standard, 5, "unavailable"),
    ] {
        let contract = Contract::<Input, Output>::new()?;
        let result = encode_bounded(&contract, output(Ok(project(profile)?), 9)?)?;
        let wire = serde_json::to_value(&result)?;
        let value = &wire["structuredContent"];
        assert_eq!(value["status"], status);
        assert_eq!(
            value["data"]["stages"].as_array().ok_or("rows")?.len(),
            count
        );
        assert_eq!(
            serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().ok_or("fallback")?)?,
            *value
        );
        assert_eq!(
            value["data"]["stages"][0]["execution"]["diagnostics"][0]["message"],
            "repair detail"
        );
        assert_eq!(
            value["data"]["stages"][0]["log"]["uri"],
            "rust-artifact://prj_00000000000000000000000000000001/art_00000000000000000000000000000001"
        );
        assert_eq!(value["data"]["stages"][0]["log"]["sha256"], "2a".repeat(32));
        assert_eq!(
            value["data"]["stages"][0]["log"]["retention_remaining_seconds"],
            3599
        );
        assert!(!serde_json::to_string(&result)?.contains("RAW_SECRET"));
        assert_eq!(
            value["data"]["stages"][2]["applied_selection"],
            "clippy_strict_cargo_defaults"
        );
        assert_eq!(
            value["data"]["source_fingerprint"],
            value["data"]["stages"][1]["execution"]["source_fingerprint"]
        );
    }
    Ok(())
}
#[test]
fn forged_pass_partial_execution_test_build_and_audit_unavailability_never_pass() -> TestResult {
    for field in 0..5 {
        let mut p = project(QualityProfile::Fast)?;
        let e = p.stages[1]
            .observation
            .as_mut()
            .and_then(QualityObservation::execution_mut)
            .ok_or("execution")?;
        match field {
            0 => e.validation_complete = false,
            1 => e.diagnostics_omitted = 1,
            2 => e.exit_code = None,
            3 => e.stdout_truncated = true,
            _ => e.termination = ExecutionTermination::OutputLimit,
        };
        let value = encoded(p)?;
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["data"]["stages"][1]["status"], "blocked");
    }
    let mut p = project(QualityProfile::Standard)?;
    if let Some(QualityObservation::Test(o)) = &mut p.stages[3].observation {
        o.build_succeeded = None;
    }
    if let Some(QualityObservation::Audit { observation, .. }) = &mut p.stages[4].observation {
        observation.state = AuditState::Passed;
        observation.validation_complete = true;
        observation.issue = Some(AuditIssue::SnapshotStale);
    }
    let value = encoded(p)?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["data"]["stages"][3]["status"], "blocked");
    assert_eq!(value["data"]["stages"][4]["status"], "unavailable");
    Ok(())
}
#[test]
fn aggregate_precedence_preserves_failed_unavailable_and_blocked_rows() -> TestResult {
    let mut p = project(QualityProfile::Standard)?;
    let e = p.stages[1]
        .observation
        .as_mut()
        .and_then(QualityObservation::execution_mut)
        .ok_or("execution")?;
    e.outcome = CheckOutcome::Failed;
    e.exit_code = Some(101);
    let value = encoded(p.clone())?;
    assert_eq!(value["status"], "unavailable");
    assert_eq!(value["data"]["stages"][1]["status"], "failed");
    p.stages[2].observation = None;
    p.stages[2].log = None;
    p.stages[2].retention_remaining_seconds = None;
    p.stages[2].issue = Some(QualityIssue::Operational(
        OperationalErrorCode::SandboxDenied,
    ));
    let value = encoded(p)?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["data"]["stages"][1]["status"], "failed");
    assert_eq!(value["data"]["stages"][4]["status"], "unavailable");
    Ok(())
}
#[test]
fn inconsistent_order_variant_fingerprint_or_log_metadata_fails_closed() -> TestResult {
    for case in 0..6 {
        let mut p = project(QualityProfile::Fast)?;
        match case {
            0 => {
                p.stages.pop();
            }
            1 => p.stages.swap(0, 1),
            2 => p.source_fingerprint = None,
            3 => p.stages[1].retention_remaining_seconds = Some(0),
            4 => {
                p.stages[1].log.as_mut().ok_or("log")?.owner =
                    "prj_00000000000000000000000000000002".parse()?
            }
            _ => p.stages[1].observation = p.stages[0].observation.clone(),
        }
        let error = output(Ok(p), 1).err().ok_or("inconsistency accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(error.data.is_none());
    }
    Ok(())
}
#[test]
fn all_operational_stage_failures_retain_rows_with_local_evidence() -> TestResult {
    let mut p = project(QualityProfile::Fast)?;
    p.source_fingerprint = None;
    p.evidence = Evidence::Local;
    for row in &mut p.stages {
        row.observation = None;
        row.log = None;
        row.retention_remaining_seconds = None;
        row.issue = Some(QualityIssue::Operational(
            OperationalErrorCode::ToolNotInstalled,
        ));
    }
    let value = encoded(p)?;
    assert_eq!(value["status"], "unavailable");
    assert_eq!(value["evidence"]["kind"], "local");
    assert!(value["data"]["source_fingerprint"].is_null());
    assert_eq!(value["data"]["stages"].as_array().ok_or("rows")?.len(), 3);
    Ok(())
}
#[test]
fn response_budget_preserves_all_stage_rows_and_resources_with_visible_omissions() -> TestResult {
    let mut p = project(QualityProfile::Standard)?;
    for row in &mut p.stages {
        if let Some(e) = row
            .observation
            .as_mut()
            .and_then(QualityObservation::execution_mut)
        {
            let mut d = e.diagnostics[0].clone();
            d.message = "\\\"".repeat(2048).parse()?;
            e.diagnostics = vec![d; 128];
        }
    }
    if let Some(QualityObservation::Format(o)) = &mut p.stages[0].observation {
        o.diff = Some("\\\"".repeat(70000));
        o.affected_files = vec!["src/lib.rs".into(); 128];
    }
    let result = encode_bounded(&Contract::<Input, Output>::new()?, output(Ok(p), 2)?)?;
    assert!(serde_json::to_vec(&result)?.len() <= MAX_RESULT);
    let value = result.structured_content.ok_or("content")?;
    assert_eq!(value["status"], "blocked");
    assert!(
        value["truncation"]["diagnostics_omitted"]
            .as_u64()
            .ok_or("omissions")?
            > 0
    );
    let rows = value["data"]["stages"].as_array().ok_or("rows")?;
    assert_eq!(rows.len(), 5);
    for row in &rows[..4] {
        assert!(row["log"]["uri"].as_str().is_some());
    }
    assert_eq!(rows[0]["format"]["diff_omitted"], true);
    Ok(())
}
#[test]
fn nested_schema_is_closed_and_required_nullable_facts_are_present() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    let value = encoded(project(QualityProfile::Fast)?)?;
    for pointer in [
        "",
        "/data",
        "/data/stages/0",
        "/data/stages/0/execution",
        "/data/stages/0/execution/runtime",
        "/data/stages/0/format",
        "/data/stages/0/log",
        "/truncation",
    ] {
        let mut extra = value.clone();
        extra
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .insert("unexpected".into(), json!(true));
        assert!(!validator.is_valid(&extra), "open {pointer}");
    }
    for (pointer, field) in [
        ("/data", "source_fingerprint"),
        ("/data/stages/0", "audit"),
        ("/data/stages/0", "issue"),
        ("/data/stages/0", "log"),
        ("/data/stages/0/execution", "exit_code"),
    ] {
        let mut omitted = value.clone();
        omitted
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .remove(field);
        assert!(!validator.is_valid(&omitted), "optional {pointer}/{field}");
    }
    Ok(())
}
#[test]
fn cleanup_uncertainty_and_infrastructure_outrank_worker_interruption() -> TestResult {
    for timed_out in [false, true] {
        for error in [
            ExecutionError::CleanupUncertain,
            ExecutionError::Infrastructure,
        ] {
            let result: Result<ProjectQualityGate, _> = joined_result(Joined {
                result: Err(InspectionError::Execution(error)),
                interrupted: Some(if timed_out {
                    WorkerError::TimedOut
                } else {
                    WorkerError::Cancelled
                }),
            });
            let error = output(result, 1).err().ok_or("hard failure hidden")?;
            assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
            assert!(error.data.is_none());
        }
    }
    for (signal, status, code) in [
        (WorkerError::TimedOut, "blocked", json!("COMMAND_TIMEOUT")),
        (WorkerError::Cancelled, "cancelled", Value::Null),
    ] {
        let result = joined_result(Joined {
            result: Err(InspectionError::Project(ProjectError::Cancelled)),
            interrupted: Some(signal),
        });
        let value = Contract::<Input, Output>::new()?
            .encode(output(result, 1)?)?
            .structured_content
            .ok_or("content")?;
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], code);
        assert!(value["data"].is_null());
    }
    Ok(())
}
#[test]
#[ignore = "prints schemas for owner-reviewed versioned snapshot generation; writes no files"]
fn emit_quality_contract_snapshot() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    println!(
        "M1_QUALITY_SCHEMA_SNAPSHOT {}",
        serde_json::to_string(&definition(&contract))?
    );
    Ok(())
}

#[test]
fn committed_publication_survives_late_cancellation_and_timeout() -> TestResult {
    for signal in [WorkerError::Cancelled, WorkerError::TimedOut] {
        let result = joined_result(Joined {
            result: Ok(project(QualityProfile::Fast)?),
            interrupted: Some(signal),
        });
        let encoded = encode_bounded(&Contract::<Input, Output>::new()?, output(result, 1)?)?;
        assert_eq!(encoded.is_error, Some(false));
        let value = encoded.structured_content.ok_or("content")?;
        assert_eq!(value["status"], "passed");
        assert!(value["error_code"].is_null());
        assert_eq!(value["data"]["stages"].as_array().ok_or("stages")?.len(), 3);
        for row in value["data"]["stages"].as_array().ok_or("stages")? {
            assert!(row["log"]["uri"].as_str().is_some());
        }
    }
    Ok(())
}
#[test]
fn divergent_audit_runtime_is_rejected_but_command_execution_fingerprints_may_differ() -> TestResult
{
    for field in 0..6 {
        let mut p = project(QualityProfile::Standard)?;
        let Some(QualityObservation::Audit { runtime, .. }) = &mut p.stages[4].observation else {
            return Err("audit observation".into());
        };
        match field {
            0 => runtime.platform = "other-platform".into(),
            1 => runtime.image_id = format!("sha256:{:064x}", 99),
            2 => runtime.configuration_fingerprint = format!("sha256:{:064x}", 99).parse()?,
            3 => runtime.rust_version = "1.99.0".into(),
            4 => runtime.cargo_version = "1.99.0".into(),
            _ => runtime.declared_toolchain = Some("different-approved-toolchain".into()),
        }
        let error = output(Ok(p), 1).err().ok_or("different runtime accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(error.data.is_none());
    }
    let mut p = project(QualityProfile::Standard)?;
    for (index, row) in p.stages.iter_mut().enumerate() {
        let observation = row.observation.as_mut().ok_or("observation")?;
        let runtime = match observation {
            QualityObservation::Audit { runtime, .. } => runtime,
            other => &mut other.execution_mut().ok_or("execution")?.runtime,
        };
        runtime.execution_fingerprint = format!("sha256:{:064x}", 200 + index).parse()?;
    }
    let value = encoded(p)?;
    assert_eq!(value["status"], "unavailable");
    assert_ne!(
        value["data"]["stages"][0]["execution"]["runtime"]["execution_fingerprint"],
        value["data"]["stages"][4]["audit"]["runtime"]["execution_fingerprint"]
    );
    Ok(())
}
#[test]
fn per_stage_diagnostic_count_is_bounded_even_below_byte_budget() -> TestResult {
    for already_omitted in [0, 7] {
        let mut p = project(QualityProfile::Fast)?;
        let execution = p.stages[1]
            .observation
            .as_mut()
            .and_then(QualityObservation::execution_mut)
            .ok_or("execution")?;
        execution.diagnostics = vec![execution.diagnostics[0].clone(); 129];
        execution.diagnostics_omitted = already_omitted;
        let value = output(Ok(p), 1)?;
        assert!(serde_json::to_vec(&value)?.len() < MAX_RESULT / 4);
        let result = encode_bounded(&Contract::<Input, Output>::new()?, value)?;
        assert_eq!(result.is_error, Some(true));
        let value = result.structured_content.ok_or("content")?;
        assert_eq!(value["status"], "blocked");
        let row = &value["data"]["stages"][1];
        assert_eq!(
            row["execution"]["diagnostics"]
                .as_array()
                .ok_or("diagnostics")?
                .len(),
            128
        );
        assert_eq!(row["execution"]["diagnostics_omitted"], already_omitted + 1);
        assert_eq!(row["execution"]["validation_complete"], false);
        assert_eq!(row["status"], "blocked");
        assert_eq!(
            value["truncation"]["diagnostics_omitted"],
            already_omitted + 1
        );
        assert_eq!(value["data"]["stages"][0]["status"], "passed");
        assert!(row["log"]["uri"].as_str().is_some());
    }
    Ok(())
}
