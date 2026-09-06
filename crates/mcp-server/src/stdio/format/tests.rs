use super::*;
use crate::stdio::workers::{Joined, WorkerError};
use rust_engineering_domain::UnixSeconds;
use rust_engineering_domain::{
    Applicability, ArtifactMetadata, CheckObservation, DiagnosticSource, FormatObservation,
    FreshnessPolicy, InspectionSemantics, IntegrityStatus, Position, Provenance, Replacement,
    Severity, SnapshotEvidence, SourceKind, SourceSpan, Suggestion,
};
use serde_json::{Value, json};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn project() -> Result<ProjectFormat, Box<dyn std::error::Error>> {
    let fingerprint = format!("sha256:{:064x}", 42);
    let reference: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let span = SourceSpan::new(
        "src/lib.rs".parse()?,
        Position::new(1, 1)?,
        Position::new(1, 2)?,
        Some(rust_engineering_domain::ByteRange::new(0, 1)?),
        true,
        Some("replace this".parse()?),
    )?;
    let diagnostics = vec![Diagnostic {
        source: DiagnosticSource::Rustc,
        severity: Severity::Warning,
        code: Some("unused".parse()?),
        message: "unused variable".parse()?,
        spans: vec![span.clone()],
        rendered: None,
        suggestions: vec![Suggestion::new(
            "remove".parse()?,
            Applicability::MachineApplicable,
            vec![Replacement {
                span,
                replacement: String::new(),
            }],
        )?],
        truncated: false,
    }];
    struct Clock;
    impl rust_engineering_domain::Clock for Clock {
        fn now(&self) -> UnixSeconds {
            UnixSeconds(102)
        }
    }
    Ok(ProjectFormat {
        project_ref: reference.clone(),
        project_identity_fingerprint: format!("sha256:{:064x}", 1).parse()?,
        semantics: InspectionSemantics::LatestKnown,
        observation: FormatObservation {
            affected_files: vec!["src/lib.rs".into()],
            affected_files_omitted: 0,
            diff: Some("Diff in src/lib.rs:1:\n-old\n+new\n".into()),
            diff_omitted: false,
            execution: CheckObservation {
                outcome: CheckOutcome::Passed,
                termination: ExecutionTermination::Exited,
                exit_code: Some(0),
                validation_complete: true,
                diagnostics,
                diagnostics_omitted: 0,
                stdout: String::new(),
                stderr: String::new(),
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
            },
        },
        evidence: SnapshotEvidence::assess(
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
        ),
        log: Some(ArtifactMetadata {
            owner: reference,
            id: "art_00000000000000000000000000000002".parse()?,
            sha256: [42; 32],
            size_bytes: 3,
            truncated: false,
            created_seconds: 0,
            expires_seconds: 3600,
        }),
        retention_remaining_seconds: Some(3599),
    })
}
#[test]
fn input_accepts_only_live_reference_shape() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    assert!(contract.decode(None).is_err());
    for patch in [
        json!({"command":"cargo"}),
        json!({"args":["--check"]}),
        json!({"options":{}}),
        json!({"workspace":true}),
        json!({"package":"member"}),
        json!({"features":[]}),
        json!({"target":"aarch64-unknown-linux-gnu"}),
        json!({"project_ref":"/secret"}),
        json!({"project_ref":42}),
        json!({"project_ref":"prj_A0000000000000000000000000000000"}),
    ] {
        let mut value = json!({"project_ref":"prj_00000000000000000000000000000001"});
        value
            .as_object_mut()
            .ok_or("object")?
            .extend(patch.as_object().ok_or("patch")?.clone());
        let error = contract
            .decode(Some(serde_json::from_value(value)?))
            .err()
            .ok_or("invalid accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(error.data.is_none());
    }
    contract.decode(Some(serde_json::from_value(
        json!({"project_ref":"prj_00000000000000000000000000000001"}),
    )?))?;
    Ok(())
}
#[test]
fn complete_format_outcomes_are_domain_results_and_timeout_retains_partial_log() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for (outcome, termination, exit, complete, status, is_error) in [
        (
            CheckOutcome::Passed,
            ExecutionTermination::Exited,
            Some(0),
            true,
            "passed",
            false,
        ),
        (
            CheckOutcome::Failed,
            ExecutionTermination::Exited,
            Some(101),
            true,
            "failed",
            false,
        ),
        (
            CheckOutcome::Incomplete,
            ExecutionTermination::TimedOut,
            None,
            false,
            "blocked",
            true,
        ),
    ] {
        let mut project = project()?;
        project.observation.execution.outcome = outcome;
        project.observation.execution.termination = termination;
        project.observation.execution.exit_code = exit;
        project.observation.execution.validation_complete = complete;
        let expected_log = {
            let log = project.log.as_ref().ok_or("fixture log")?;
            resources::uri(&log.owner, &log.id)
        };
        let encoded = encode_bounded(&contract, output(Ok(project), 12)?)?;
        assert_eq!(encoded.is_error, Some(is_error));
        let wire = serde_json::to_value(encoded)?;
        let value = &wire["structuredContent"];
        assert_eq!(
            serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().ok_or("fallback")?)?,
            *value
        );
        assert_eq!(value["status"], status);
        assert_eq!(value["data"]["log"]["uri"], expected_log);
        assert_eq!(
            value["data"].get("log_unavailable_reason"),
            Some(&Value::Null)
        );
        assert_eq!(value["data"]["validation_complete"], complete);
        assert_eq!(value["data"]["exit_code"], json!(exit));
        assert_eq!(
            value["evidence"]["details"]["provenance"]["source_id"],
            value["data"]["source_fingerprint"]
        );
        assert_eq!(
            value["error_code"],
            if is_error {
                json!("COMMAND_TIMEOUT")
            } else {
                Value::Null
            }
        );
    }
    Ok(())
}
#[test]
fn nested_schema_is_closed_and_nullable_facts_are_required() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    let value = contract
        .encode(output(Ok(project()?), 1)?)?
        .structured_content
        .ok_or("output")?;
    for pointer in [
        "",
        "/data",
        "/data/runtime",
        "/data/log",
        "/diagnostics/0",
        "/diagnostics/0/spans/0",
        "/diagnostics/0/spans/0/start",
        "/diagnostics/0/spans/0/bytes",
        "/diagnostics/0/suggestions/0",
        "/diagnostics/0/suggestions/0/edits/0",
        "/truncation",
        "/evidence/details/provenance",
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
        ("/data", "exit_code"),
        ("/data", "diff"),
        ("/data", "log"),
        ("/data", "log_unavailable_reason"),
        ("/data/runtime", "declared_toolchain"),
        ("/diagnostics/0", "code"),
        ("/diagnostics/0", "rendered"),
        ("/diagnostics/0/spans/0", "bytes"),
        ("/diagnostics/0/spans/0", "label"),
    ] {
        let mut null = value.clone();
        null.pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .insert(field.into(), Value::Null);
        assert!(validator.is_valid(&null), "null {pointer}/{field}");
        let mut omitted = value.clone();
        omitted
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .remove(field);
        assert!(!validator.is_valid(&omitted), "omission {pointer}/{field}");
    }
    for (pointer, bad) in [
        ("/diagnostics/0/spans/0/start/line", json!(0)),
        ("/diagnostics/0/message", json!("x".repeat(4097))),
        ("/diagnostics/0/suggestions/0/edits", json!([])),
        ("/data/semantics", json!("latest")),
        ("/data/affected_files", json!(vec!["src/lib.rs"; 129])),
        ("/data/affected_files/0", json!("/secret")),
        ("/data/diff", json!("x".repeat(32769))),
    ] {
        let mut invalid = value.clone();
        *invalid.pointer_mut(pointer).ok_or("pointer")? = bad;
        assert!(!validator.is_valid(&invalid), "accepted {pointer}");
    }
    Ok(())
}
#[test]
fn cleanup_uncertainty_outranks_cancellation_and_deadline_without_payload() -> TestResult {
    for signal in [WorkerError::Cancelled, WorkerError::TimedOut] {
        let result: Result<ProjectFormat, _> = joined_result(Joined {
            result: Err(InspectionError::Execution(ExecutionError::CleanupUncertain)),
            interrupted: Some(signal),
        });
        let error = output(result, 1).err().ok_or("cleanup hidden")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(error.data.is_none());
    }
    let contract = Contract::<Input, Output>::new()?;
    let encoded = contract.encode(output(
        Err(InspectionError::Project(ProjectError::Cancelled)),
        1,
    )?)?;
    assert_eq!(encoded.is_error, Some(true));
    let value = encoded.structured_content.ok_or("output")?;
    assert_eq!(value["status"], "cancelled");
    assert!(value["data"].is_null());
    Ok(())
}
#[test]
fn response_budget_drops_diagnostics_but_keeps_log_and_downgrades_passed() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let mut project = project()?;
    let mut diagnostic = project.observation.execution.diagnostics[0].clone();
    diagnostic.message = "\\\"".repeat(2048).parse()?;
    project.observation.execution.diagnostics = vec![diagnostic; 128];
    let uri = {
        let log = project.log.as_ref().ok_or("fixture log")?;
        resources::uri(&log.owner, &log.id)
    };
    let value = output(Ok(project), 1)?;
    assert!(serde_json::to_vec(&value)?.len() > MAX_RESULT / 4);
    let encoded = encode_bounded(&contract, value)?;
    assert_eq!(encoded.is_error, Some(false));
    assert!(serde_json::to_vec(&encoded)?.len() <= MAX_RESULT);
    let value = encoded.structured_content.ok_or("output")?;
    assert_eq!(value["status"], "failed");
    assert_eq!(value["data"]["validation_complete"], false);
    assert_eq!(value["data"]["log"]["uri"], uri);
    let omitted = value["truncation"]["diagnostics_omitted"]
        .as_u64()
        .ok_or("omitted")?;
    assert!(omitted > 0);
    assert_eq!(
        omitted as usize + value["diagnostics"].as_array().ok_or("diagnostics")?.len(),
        128
    );
    Ok(())
}
#[test]
#[ignore = "prints schemas for owner-reviewed versioned snapshot generation; writes no files"]
fn emit_format_contract_snapshot() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    println!(
        "M1_FORMAT_SCHEMA_SNAPSHOT {}",
        serde_json::to_string(
            &json!({"contract_version":"m1-04","inputSchema":contract.input_schema.as_ref(),"outputSchema":contract.output_schema.as_ref()})
        )?
    );
    Ok(())
}

#[test]
fn frozen_lock_failure_preserves_log_as_operational_error() -> TestResult {
    let mut project = project()?;
    project.observation.execution.outcome = CheckOutcome::LockfileUpdateRequired;
    project.observation.execution.validation_complete = false;
    project.observation.execution.exit_code = Some(101);
    let result = Contract::<Input, Output>::new()?.encode(output(Ok(project), 1)?)?;
    assert_eq!(result.is_error, Some(true));
    let value = result.structured_content.ok_or("output")?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["error_code"], "LOCKFILE_UPDATE_REQUIRED");
    assert_eq!(value["data"]["validation_complete"], false);
    assert!(value["data"]["log"]["uri"].as_str().is_some());
    Ok(())
}

#[test]
fn retention_capacity_preserves_format_semantics_and_diagnostics_without_artifact_uri() -> TestResult
{
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    for (outcome, exit_code, status) in [
        (CheckOutcome::Passed, 0, "passed"),
        (CheckOutcome::Failed, 101, "failed"),
    ] {
        let mut project = project()?;
        project.observation.execution.outcome = outcome;
        project.observation.execution.exit_code = Some(exit_code);
        let diagnostics = serde_json::to_value(&project.observation.execution.diagnostics)?;
        project.log = None;
        project.retention_remaining_seconds = None;
        let encoded = encode_bounded(&contract, output(Ok(project), 1)?)?;
        assert_eq!(encoded.is_error, Some(false));
        let wire = serde_json::to_value(&encoded)?;
        let value = &wire["structuredContent"];
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], Value::Null);
        assert_eq!(value["error_message"], Value::Null);
        assert_eq!(value["data"]["validation_complete"], true);
        assert_eq!(value["data"]["exit_code"], exit_code);
        assert_eq!(value["diagnostics"], diagnostics);
        assert_eq!(value["truncation"]["diagnostics_omitted"], 0);
        assert_eq!(value["data"].get("log"), Some(&Value::Null));
        assert_eq!(
            value["data"]["log_unavailable_reason"],
            "retention_capacity"
        );
        assert_eq!(value["evidence"]["kind"], "snapshot");
        assert!(!serde_json::to_string(&encoded)?.contains("rust-artifact://"));
        let text: Value =
            serde_json::from_str(wire["content"][0]["text"].as_str().ok_or("fallback")?)?;
        assert_eq!(&text, value);
        assert!(validator.is_valid(value));
        for field in ["log", "log_unavailable_reason"] {
            let mut omitted = value.clone();
            omitted["data"].as_object_mut().ok_or("data")?.remove(field);
            assert!(!validator.is_valid(&omitted), "missing {field}");
        }
        let mut unknown_reason = value.clone();
        unknown_reason["data"]["log_unavailable_reason"] = json!("output_limit");
        assert!(!validator.is_valid(&unknown_reason));
    }
    Ok(())
}

#[test]
fn incomplete_format_never_passes_and_display_omission_keeps_execution_facts() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let mut incomplete = project()?;
    incomplete.observation.execution.validation_complete = false;
    let value = encode_bounded(&contract, output(Ok(incomplete), 1)?)?
        .structured_content
        .ok_or("output")?;
    assert_eq!(value["status"], "failed");
    assert_eq!(value["data"]["validation_complete"], false);
    let mut complete = project()?;
    complete.observation.diff = Some("é".repeat(16385));
    complete.observation.affected_files = vec!["src/lib.rs".into(); 129];
    let value = encode_bounded(&contract, output(Ok(complete), 1)?)?
        .structured_content
        .ok_or("output")?;
    assert_eq!(value["status"], "passed");
    assert_eq!(value["data"]["validation_complete"], true);
    assert!(value["data"]["diff"].is_null());
    assert_eq!(value["data"]["diff_omitted"], true);
    assert_eq!(
        value["data"]["affected_files"]
            .as_array()
            .ok_or("files")?
            .len(),
        128
    );
    assert_eq!(value["data"]["affected_files_omitted"], 1);
    Ok(())
}

#[test]
fn operational_outcomes_cover_unavailable_denied_timeout_and_infrastructure() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for (error, status, code) in [
        (
            InspectionError::Execution(ExecutionError::Unavailable),
            "unavailable",
            "TOOL_NOT_INSTALLED",
        ),
        (
            InspectionError::Execution(ExecutionError::Denied),
            "blocked",
            "SANDBOX_DENIED",
        ),
        (
            InspectionError::Execution(ExecutionError::Busy),
            "blocked",
            "SANDBOX_DENIED",
        ),
        (
            InspectionError::InvalidMetadata,
            "blocked",
            "INVALID_PROJECT",
        ),
        (
            InspectionError::OutputLimit,
            "blocked",
            "OUTPUT_LIMIT_EXCEEDED",
        ),
        (
            worker_error(WorkerError::TimedOut),
            "blocked",
            "COMMAND_TIMEOUT",
        ),
    ] {
        let encoded = contract.encode(output(Err(error), 1)?)?;
        assert_eq!(encoded.is_error, Some(true));
        let value = encoded.structured_content.ok_or("output")?;
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], code);
        assert!(value["data"].is_null());
    }
    assert_eq!(
        output(Err(InspectionError::Internal), 1)
            .err()
            .ok_or("internal")?
            .code,
        rmcp::model::ErrorCode::INTERNAL_ERROR
    );
    Ok(())
}
