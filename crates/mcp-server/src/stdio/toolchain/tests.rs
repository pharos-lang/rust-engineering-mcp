use super::*;
use crate::stdio::workers::WorkerError;
use rust_engineering_domain::*;
use serde_json::{Value, json};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn successful() -> Result<Output, Box<dyn std::error::Error>> {
    let fingerprint = format!("sha256:{:064x}", 42);
    let host = "aarch64-unknown-linux-gnu";
    let observation = ToolchainObservation {
        inventory: ToolchainInventory {
            rustc_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            channel: ToolchainChannel::Stable,
            host_triple: host.into(),
            installed_targets: vec![host.into()],
            installed_components: [
                InstalledComponentKind::Cargo,
                InstalledComponentKind::Clippy,
                InstalledComponentKind::RustStd,
                InstalledComponentKind::Rustc,
                InstalledComponentKind::Rustfmt,
            ]
            .into_iter()
            .map(|component| InstalledComponent {
                component,
                target: (component == InstalledComponentKind::RustStd).then(|| host.into()),
            })
            .collect(),
        },
        runtime: ToolchainRuntime {
            platform: "linux/aarch64".into(),
            image_id: fingerprint.clone(),
            configuration_fingerprint: fingerprint.parse()?,
            executions: [
                ToolchainObservationCommand::CompilerVersion,
                ToolchainObservationCommand::CargoVersion,
                ToolchainObservationCommand::InstalledComponents,
            ]
            .into_iter()
            .map(|command| {
                Ok(ToolchainExecution {
                    command,
                    execution_fingerprint: fingerprint.parse()?,
                })
            })
            .collect::<Result<Vec<_>, ContractError>>()?,
        },
        source_fingerprint: fingerprint.parse()?,
        declared_toolchain: None,
    };
    let provenance = Provenance::new(
        SourceKind::ProjectSnapshot,
        fingerprint.parse()?,
        Some(UnixSeconds(100)),
        Some(UnixSeconds(101)),
        IntegrityStatus::Verified,
        false,
    )?;
    struct Clock;
    impl rust_engineering_domain::Clock for Clock {
        fn now(&self) -> UnixSeconds {
            UnixSeconds(102)
        }
    }
    let evidence = SnapshotEvidence::assess(
        provenance,
        FreshnessPolicy::new("captured-project-v1".parse()?, 60, 300)?,
        &Clock,
    );
    Ok(output(
        Ok(ToolchainInspection {
            project_ref: "prj_00000000000000000000000000000001".parse()?,
            project_identity_fingerprint: format!("sha256:{:064x}", 1).parse()?,
            semantics: InspectionSemantics::LatestKnown,
            observation,
            evidence,
        }),
        2,
    )?)
}

#[test]
fn domain_output_matches_closed_nested_schema_and_text_fallback() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let schema = serde_json::to_value(contract.output_schema.as_ref())?;
    let validator = jsonschema::validator_for(&schema)?;
    let encoded = encode_bounded(&contract, successful()?)?;
    assert_eq!(encoded.is_error, Some(false));
    let wire = serde_json::to_value(&encoded)?;
    let value = &wire["structuredContent"];
    let text: Value =
        serde_json::from_str(wire["content"][0]["text"].as_str().ok_or("missing text")?)?;
    assert_eq!(&text, value);
    assert_eq!(value["data"]["semantics"], "latest_known");
    assert_eq!(
        value["data"]["observation"]["inventory"]["host_triple"],
        "aarch64-unknown-linux-gnu"
    );
    assert_eq!(
        value["data"]["observation"]["inventory"]["installed_components"][2]["target"],
        "aarch64-unknown-linux-gnu"
    );
    assert_eq!(
        value["evidence"]["details"]["provenance"]["source_kind"],
        "project_snapshot"
    );
    assert_eq!(
        value["evidence"]["details"]["provenance"]["source_id"],
        value["data"]["observation"]["source_fingerprint"]
    );
    assert_eq!(value["evidence"]["details"]["freshness"]["age_seconds"], 2);
    for pointer in [
        "",
        "/data",
        "/data/observation",
        "/data/observation/inventory",
        "/data/observation/inventory/installed_components/0",
        "/data/observation/runtime",
        "/data/observation/runtime/executions/0",
        "/evidence",
        "/evidence/details",
        "/evidence/details/provenance",
        "/evidence/details/freshness",
        "/evidence/details/freshness/policy",
        "/truncation",
    ] {
        let mut extra = value.clone();
        extra
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("missing object")?
            .insert("unexpected".into(), json!(true));
        assert!(!validator.is_valid(&extra), "open object: {pointer}");
    }
    Ok(())
}

#[test]
fn required_nullable_facts_allow_null_but_never_omission() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    let value = contract
        .encode(successful()?)?
        .structured_content
        .ok_or("output")?;
    for (pointer, field) in [
        ("/data/observation", "declared_toolchain"),
        (
            "/data/observation/inventory/installed_components/0",
            "target",
        ),
        ("/evidence/details/provenance", "observed_at"),
        ("/evidence/details/provenance", "created_at"),
        ("/evidence/details/freshness", "age_seconds"),
    ] {
        let mut null = value.clone();
        null.pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .insert(field.into(), Value::Null);
        assert!(
            validator.is_valid(&null),
            "null rejected: {pointer}/{field}"
        );
        let mut omitted = value.clone();
        omitted
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .remove(field);
        assert!(
            !validator.is_valid(&omitted),
            "omission accepted: {pointer}/{field}"
        );
    }
    let mut selected = value;
    selected["data"]["observation"]["declared_toolchain"] = json!("1.98.1");
    assert!(validator.is_valid(&selected));
    Ok(())
}

#[test]
fn schema_rejects_unknown_inventory_and_runtime_facts_and_overlong_identifiers() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    let valid = contract
        .encode(successful()?)?
        .structured_content
        .ok_or("output")?;
    for (pointer, value) in [
        ("/data/semantics", json!("latest")),
        ("/data/observation/inventory/channel", json!("nightly")),
        (
            "/data/observation/inventory/host_triple",
            json!("x".repeat(129)),
        ),
        (
            "/data/observation/inventory/rustc_version",
            json!("1.98.1\nsecret"),
        ),
        ("/data/observation/inventory/cargo_version", json!(null)),
        (
            "/data/observation/inventory/installed_targets/0",
            json!("x".repeat(129)),
        ),
        (
            "/data/observation/inventory/installed_components/0/component",
            json!("rustup"),
        ),
        (
            "/data/observation/inventory/installed_components/0/target",
            json!("x\u{0}"),
        ),
        (
            "/data/observation/runtime/executions/0/command",
            json!("shell"),
        ),
        (
            "/data/observation/runtime/executions/0/execution_fingerprint",
            json!("sha256:bad"),
        ),
        ("/data/observation/runtime/image_id", json!("rust:latest")),
        ("/data/observation/source_fingerprint", json!("sha256:bad")),
        ("/data/observation/declared_toolchain", json!("nightly")),
        ("/data/observation/declared_toolchain", json!("1.98.1\n")),
        (
            "/data/observation/inventory/installed_targets",
            json!(vec!["target"; 33]),
        ),
        ("/data/observation/runtime/executions", json!([])),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).ok_or("pointer")? = value;
        assert!(!validator.is_valid(&invalid), "accepted: {pointer}");
    }
    Ok(())
}

#[test]
fn project_ref_only_input_rejects_runtime_authority_and_malformed_tokens() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    assert!(contract.decode(None).is_err());
    for value in [
        json!({}),
        json!({"project_ref":null}),
        json!({"project_ref":true}),
        json!({"project_ref":"/etc/passwd"}),
        json!({"project_ref":"prj_0000000000000000000000000000000A"}),
        json!({"project_ref":"prj_00000000000000000000000000000001", "command":"rustup"}),
        json!({"project_ref":"prj_00000000000000000000000000000001", "image":"unapproved"}),
        json!({"project_ref":"prj_00000000000000000000000000000001", "network":true}),
    ] {
        let error = contract
            .decode(Some(serde_json::from_value(value)?))
            .err()
            .ok_or("accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, "Invalid tool arguments");
        assert!(error.data.is_none());
    }
    contract.decode(Some(serde_json::from_value(
        json!({"project_ref":"prj_00000000000000000000000000000001"}),
    )?))?;
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
            assert_eq!(error.message, "Runtime inventory validation failed");
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
