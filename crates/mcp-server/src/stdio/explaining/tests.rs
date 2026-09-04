use super::*;
use rust_engineering_domain::*;
use serde_json::{Value, json};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn explanation() -> Result<DiagnosticExplanation, Box<dyn std::error::Error>> {
    let fp = format!("sha256:{:064x}", 42);
    struct FixedClock;
    impl Clock for FixedClock {
        fn now(&self) -> UnixSeconds {
            UnixSeconds(102)
        }
    }
    let evidence = SnapshotEvidence::assess(
        Provenance::new(
            SourceKind::Artifact,
            fp.parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(101)),
            IntegrityStatus::Verified,
            false,
        )?,
        FreshnessPolicy::new("compiler-explanation-v1".parse()?, 60, 300)?,
        &FixedClock,
    );
    Ok(DiagnosticExplanation {
        semantics: InspectionSemantics::LatestKnown,
        evidence,
        observation: ExplainObservation {
            code: "E0502".parse()?,
            explanation: Some(
                "A value was borrowed mutably while already borrowed immutably.\n".into(),
            ),
            complete: true,
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            stdout_truncated: false,
            stderr_truncated: false,
            content_fingerprint: fp.parse()?,
            runtime: RuntimeIdentity {
                platform: "linux/aarch64".into(),
                image_id: fp.clone(),
                configuration_fingerprint: fp.parse()?,
                execution_fingerprint: fp.parse()?,
                rust_version: "1.98.1".into(),
                cargo_version: "1.98.1".into(),
                declared_toolchain: None,
            },
        },
    })
}
fn successful() -> Result<Output, Box<dyn std::error::Error>> {
    Ok(output(Ok(explanation()?), 2)?)
}
#[test]
fn code_only_input_rejects_injections_and_authority() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    assert!(contract.decode(None).is_err());
    for code in [
        "E502",
        "E05022",
        "e0502",
        "E٠٥٠٢",
        "E0502\n",
        "E0502 --help",
        ";id",
        "$(id)",
        "--version",
        "../E0502",
        " E0502",
    ] {
        let error = contract
            .decode(Some(serde_json::from_value(json!({"code":code}))?))
            .err()
            .ok_or("accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert!(error.data.is_none());
    }
    for value in [
        json!({}),
        json!({"code": null}),
        json!({"code": 502}),
        json!({"code":"E0502","project_ref":"prj_00000000000000000000000000000001"}),
        json!({"code":"E0502","command":"id"}),
        json!({"code":"E0502","network":true}),
    ] {
        assert!(
            contract
                .decode(Some(serde_json::from_value(value)?))
                .is_err()
        );
    }
    for code in ["E0502", "E0000", "E9999"] {
        contract.decode(Some(serde_json::from_value(json!({"code":code}))?))?;
    }
    Ok(())
}
#[test]
fn explanation_contract_is_closed_and_carries_artifact_evidence() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    let encoded = encode_bounded(&contract, successful()?)?;
    assert_eq!(encoded.is_error, Some(false));
    let wire = serde_json::to_value(encoded)?;
    let value = &wire["structuredContent"];
    let fallback: Value = serde_json::from_str(wire["content"][0]["text"].as_str().ok_or("text")?)?;
    assert_eq!(&fallback, value);
    assert_eq!(value["data"]["semantics"], "latest_known");
    assert_eq!(value["data"]["observation"]["code"], "E0502");
    assert_eq!(
        value["evidence"]["details"]["provenance"]["source_kind"],
        "artifact"
    );
    assert_eq!(
        value["evidence"]["details"]["provenance"]["source_id"],
        value["data"]["observation"]["content_fingerprint"]
    );
    for pointer in [
        "",
        "/data",
        "/data/observation",
        "/data/observation/runtime",
        "/evidence",
        "/evidence/details",
        "/evidence/details/provenance",
        "/evidence/details/freshness",
        "/truncation",
    ] {
        let mut extra = value.clone();
        extra
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .insert("unexpected".into(), json!(true));
        assert!(!validator.is_valid(&extra), "open object {pointer}");
    }
    Ok(())
}
#[test]
fn incomplete_or_inconsistent_observations_never_pass() -> TestResult {
    for (complete, termination, exit_code, text, stdout, stderr) in [
        (
            false,
            ExecutionTermination::Exited,
            Some(0),
            Some("text"),
            false,
            false,
        ),
        (
            true,
            ExecutionTermination::Exited,
            None,
            Some("text"),
            false,
            false,
        ),
        (
            true,
            ExecutionTermination::Exited,
            Some(1),
            Some("text"),
            false,
            false,
        ),
        (
            true,
            ExecutionTermination::Exited,
            Some(0),
            Some(""),
            false,
            false,
        ),
        (
            true,
            ExecutionTermination::Exited,
            Some(0),
            Some("  \n"),
            false,
            false,
        ),
        (
            true,
            ExecutionTermination::Exited,
            Some(0),
            None,
            false,
            false,
        ),
        (
            true,
            ExecutionTermination::Exited,
            Some(0),
            Some("text"),
            true,
            false,
        ),
        (
            true,
            ExecutionTermination::Exited,
            Some(0),
            Some("text"),
            false,
            true,
        ),
        (
            true,
            ExecutionTermination::TimedOut,
            Some(0),
            Some("text"),
            false,
            false,
        ),
        (
            true,
            ExecutionTermination::Cancelled,
            Some(0),
            Some("text"),
            false,
            false,
        ),
        (
            true,
            ExecutionTermination::OutputLimit,
            Some(0),
            Some("text"),
            false,
            false,
        ),
    ] {
        let mut data = explanation()?;
        data.observation.complete = complete;
        data.observation.termination = termination;
        data.observation.exit_code = exit_code;
        data.observation.explanation = text.map(String::from);
        data.observation.stdout_truncated = stdout;
        data.observation.stderr_truncated = stderr;
        if let Ok(value) = output(Ok(data), 0) {
            assert_ne!(value.status(), ToolStatus::Passed);
        }
    }
    Ok(())
}
#[test]
fn unknown_valid_code_has_no_fabricated_explanation() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let mut data = explanation()?;
    data.observation.code = "E9999".parse()?;
    data.observation.explanation = None;
    data.observation.exit_code = Some(1);
    let encoded = contract.encode(output(Ok(data), 0)?)?;
    assert_eq!(encoded.is_error, Some(true));
    let value = encoded.structured_content.ok_or("content")?;
    assert_eq!(value["status"], "unavailable");
    assert_eq!(value["error_code"], "DIAGNOSTIC_EXPLANATION_UNAVAILABLE");
    assert!(value["data"]["observation"]["explanation"].is_null());
    assert_eq!(value["data"]["observation"]["code"], "E9999");
    assert_eq!(
        value["data"]["observation"]["runtime"]["rust_version"],
        "1.98.1"
    );
    assert_eq!(value["evidence"]["kind"], "snapshot");
    Ok(())
}
#[test]
fn explanation_byte_limit_is_independent_of_json_character_limit() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let mut data = explanation()?;
    data.observation.explanation = Some("é".repeat(MAX_EXPLANATION / 2 + 1));
    let value = contract
        .encode(output(Ok(data), 0)?)?
        .structured_content
        .ok_or("content")?;
    assert_eq!(value["error_code"], "OUTPUT_LIMIT_EXCEEDED");
    assert!(value["data"].is_null());
    Ok(())
}
#[test]
fn operational_results_have_no_partial_inventory_and_infrastructure_is_not_project_error()
-> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for (error, status, code) in [
        (
            InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::InvalidProject)),
            "blocked",
            Some("INVALID_PROJECT"),
        ),
        (
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            )),
            "blocked",
            Some("PROJECT_NOT_FOUND"),
        ),
        (
            InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::CommandTimeout)),
            "blocked",
            Some("COMMAND_TIMEOUT"),
        ),
        (
            InspectionError::OutputLimit,
            "blocked",
            Some("OUTPUT_LIMIT_EXCEEDED"),
        ),
        (
            InspectionError::Execution(ExecutionError::Denied),
            "blocked",
            Some("SANDBOX_DENIED"),
        ),
        (
            InspectionError::Execution(ExecutionError::Busy),
            "blocked",
            Some("SANDBOX_DENIED"),
        ),
        (
            InspectionError::Execution(ExecutionError::InvalidConfiguration),
            "blocked",
            Some("SANDBOX_DENIED"),
        ),
        (
            InspectionError::Execution(ExecutionError::Unavailable),
            "unavailable",
            Some("TOOL_NOT_INSTALLED"),
        ),
        (
            InspectionError::Project(ProjectError::Cancelled),
            "cancelled",
            None,
        ),
        (
            InspectionError::Execution(ExecutionError::Cancelled),
            "cancelled",
            None,
        ),
    ] {
        let encoded = contract.encode(output(Err(error), 1)?)?;
        assert_eq!(encoded.is_error, Some(true));
        let value = encoded.structured_content.ok_or("output")?;
        assert!(value["data"].is_null());
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], json!(code));
        assert_eq!(value["evidence"], json!({"kind":"local"}));
    }
    for error in [
        InspectionError::InvalidMetadata,
        InspectionError::Internal,
        InspectionError::Project(ProjectError::Internal),
        InspectionError::Execution(ExecutionError::Infrastructure),
        InspectionError::Execution(ExecutionError::CleanupUncertain),
    ] {
        let invalid_metadata = matches!(error, InspectionError::InvalidMetadata);
        let error = output(Err(error), 1).err().ok_or("infrastructure hidden")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(error.data.is_none());
        if invalid_metadata {
            assert_eq!(error.message, "Compiler explanation validation failed");
        }
    }
    Ok(())
}

#[test]
fn joined_hard_errors_survive_both_deadline_and_cancellation() {
    for interrupted in [WorkerError::Cancelled, WorkerError::TimedOut] {
        let result: Result<(), _> = joined_result(Joined {
            result: Err(InspectionError::Execution(ExecutionError::CleanupUncertain)),
            interrupted: Some(interrupted),
        });
        assert!(matches!(
            result,
            Err(InspectionError::Execution(ExecutionError::CleanupUncertain))
        ));
    }
    for error in [
        InspectionError::InvalidMetadata,
        InspectionError::Internal,
        InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::InvalidProject)),
    ] {
        let result: Result<(), _> = joined_result(Joined {
            result: Err(error),
            interrupted: Some(WorkerError::TimedOut),
        });
        assert!(!matches!(
            result,
            Err(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout
            )))
        ));
        assert!(result.is_err());
    }
    let cancelled: Result<(), _> = joined_result(Joined {
        result: Err(InspectionError::Project(ProjectError::Cancelled)),
        interrupted: Some(WorkerError::TimedOut),
    });
    assert!(matches!(
        cancelled,
        Err(InspectionError::Project(ProjectError::Rejected(
            OperationalErrorCode::CommandTimeout
        )))
    ));
    let late = joined_result(Joined {
        result: Ok(()),
        interrupted: Some(WorkerError::Cancelled),
    });
    assert!(matches!(
        late,
        Err(InspectionError::Project(ProjectError::Cancelled))
    ));
    assert!(
        joined_result(Joined {
            result: Ok(()),
            interrupted: None
        })
        .is_ok()
    );
}

#[test]
fn complete_wire_budget_counts_both_structured_content_and_text() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let mut value = successful()?;
    // The otherwise-valid schema does not bound the server-authored summary.
    // Each representation individually fits; their complete MCP result does not.
    static LONG_SUMMARY: [u8; MAX_RESULT / 2] = [b'x'; MAX_RESULT / 2];
    value.summary = std::str::from_utf8(&LONG_SUMMARY)?;
    assert!(serde_json::to_vec(&value)?.len() < MAX_RESULT);
    assert!(serde_json::to_vec(&serde_json::to_string(&value)?)?.len() < MAX_RESULT);
    let encoded = encode_bounded(&contract, value)?;
    assert_eq!(encoded.is_error, Some(true));
    assert!(serde_json::to_vec(&encoded)?.len() <= MAX_RESULT);
    let value = encoded.structured_content.ok_or("output")?;
    assert_eq!(value["error_code"], "OUTPUT_LIMIT_EXCEEDED");
    assert!(value["data"].is_null());
    assert_eq!(value["truncation"]["stdout_truncated"], true);
    Ok(())
}

#[test]
fn nested_schema_rejects_invalid_facts_and_missing_nullable_fields() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    let valid = contract
        .encode(successful()?)?
        .structured_content
        .ok_or("content")?;
    for (pointer, replacement) in [
        ("/data/semantics", json!("latest")),
        ("/data/observation/code", json!("E0502 --help")),
        (
            "/data/observation/explanation",
            json!("x".repeat(MAX_EXPLANATION + 1)),
        ),
        ("/data/observation/termination", json!("unknown")),
        ("/data/observation/runtime/image_id", json!("rust:latest")),
        ("/data/observation/content_fingerprint", json!("sha256:bad")),
        (
            "/evidence/details/provenance/source_kind",
            json!("project_snapshot"),
        ),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).ok_or("pointer")? = replacement;
        assert!(!validator.is_valid(&invalid), "accepted {pointer}");
    }
    for (pointer, field) in [
        ("/data/observation", "explanation"),
        ("/data/observation", "exit_code"),
        ("/data/observation/runtime", "declared_toolchain"),
    ] {
        let mut value = valid.clone();
        let object = value
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?;
        object.insert(field.into(), Value::Null);
        assert!(validator.is_valid(&value));
        value
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .remove(field);
        assert!(!validator.is_valid(&value));
    }
    Ok(())
}
