use std::error::Error;

use rust_engineering_domain::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    count: u32,
}

fn report() -> Result<Report<Payload>, ContractError> {
    Ok(Report {
        summary: "Compilation checked".parse()?,
        duration_ms: 12,
        data: Payload { count: 0 },
        diagnostics: vec![],
        truncation: Truncation::default(),
        evidence: Evidence::Local,
    })
}

#[test]
fn canonical_references_roundtrip_but_reject_paths_and_noncanonical_input() -> TestResult {
    let canonical = format!("prj_{}", "abcdef01".repeat(4));
    let reference: ProjectRef = canonical.parse()?;
    assert_eq!(reference.as_str(), canonical);
    assert_eq!(serde_json::to_value(&reference)?, json!(canonical));
    assert_eq!(
        serde_json::from_value::<ProjectRef>(json!(canonical))?,
        reference
    );
    for input in [
        "",
        "prj_abc",
        "../../project",
        "/tmp/project",
        "prj_é",
        " prj_123",
    ] {
        assert!(input.parse::<ProjectRef>().is_err());
        assert!(serde_json::from_value::<ProjectRef>(json!(input)).is_err());
    }
    for input in [
        canonical.to_uppercase(),
        format!("{canonical}0"),
        format!("{canonical}\n"),
    ] {
        assert!(input.parse::<ProjectRef>().is_err());
    }
    assert!(serde_json::from_value::<ProjectRef>(json!(128)).is_err());
    Ok(())
}

#[test]
fn fingerprints_validate_canonical_digests_without_computing_them() -> TestResult {
    let digest = format!("sha256:{}", "0123456789abcdef".repeat(4));
    let identity: ProjectIdentityFingerprint = digest.parse()?;
    let execution: ExecutionFingerprint = digest.parse()?;
    assert_eq!(identity.as_str(), execution.as_str());
    assert_eq!(
        serde_json::from_str::<ExecutionFingerprint>(&serde_json::to_string(&execution)?)?,
        execution
    );
    for invalid in [
        digest.to_uppercase(),
        format!("{digest}f"),
        "sha256:bad".into(),
        "a".repeat(64),
    ] {
        assert!(invalid.parse::<ProjectIdentityFingerprint>().is_err());
        assert!(serde_json::from_value::<ExecutionFingerprint>(json!(invalid)).is_err());
    }
    Ok(())
}

#[test]
fn error_text_does_not_reflect_invalid_input() {
    let error = "private-secret".parse::<ProjectRef>();
    assert_eq!(error, Err(ContractError::InvalidProjectRef));
    assert!(
        !ContractError::InvalidProjectRef
            .to_string()
            .contains("private-secret")
    );
    assert!(" \n\t".parse::<NonEmptyText>().is_err());
}

#[test]
fn project_failure_is_a_normal_result_and_keeps_diagnostics() -> TestResult {
    let mut report = report()?;
    report.diagnostics.push(Diagnostic {
        source: DiagnosticSource::Rustc,
        severity: Severity::Error,
        code: Some("E0502".parse()?),
        message: "cannot borrow café".parse()?,
        spans: vec![],
        rendered: None,
        suggestions: vec![],
        truncated: false,
    });
    report.truncation.diagnostics_omitted = 2;
    let result = OutputEnvelope::failed(report);
    assert_eq!(result.status(), ToolStatus::Failed);
    assert!(!result.is_operational_error());
    let value = serde_json::to_value(&result)?;
    assert_eq!(value["status"], "failed");
    assert_eq!(value["error_code"], Value::Null);
    assert_eq!(value["error_message"], Value::Null);
    assert_eq!(value["diagnostics"][0]["code"], "E0502");
    assert_eq!(
        serde_json::from_value::<OutputEnvelope<Payload>>(value)?,
        result
    );
    Ok(())
}

#[test]
fn every_operational_code_has_one_mapping_and_is_not_success() -> TestResult {
    use OperationalErrorCode::*;
    for (code, status, name) in [
        (ProjectNotFound, ToolStatus::Blocked, "PROJECT_NOT_FOUND"),
        (InvalidProject, ToolStatus::Blocked, "INVALID_PROJECT"),
        (
            ToolNotInstalled,
            ToolStatus::Unavailable,
            "TOOL_NOT_INSTALLED",
        ),
        (
            LockfileUpdateRequired,
            ToolStatus::Blocked,
            "LOCKFILE_UPDATE_REQUIRED",
        ),
        (CommandTimeout, ToolStatus::Blocked, "COMMAND_TIMEOUT"),
        (SandboxDenied, ToolStatus::Blocked, "SANDBOX_DENIED"),
        (NetworkDenied, ToolStatus::Blocked, "NETWORK_DENIED"),
        (
            UnsupportedPlatform,
            ToolStatus::Unavailable,
            "UNSUPPORTED_PLATFORM",
        ),
        (
            OutputLimitExceeded,
            ToolStatus::Blocked,
            "OUTPUT_LIMIT_EXCEEDED",
        ),
    ] {
        let result = OutputEnvelope::operational_error(
            OperationalError::new(code, "Operation could not complete".parse()?),
            report()?,
        );
        assert_eq!(result.status(), status);
        assert!(result.is_operational_error());
        assert!(!result.status().is_success());
        let wire = serde_json::to_value(&result)?;
        assert_eq!(wire["error_code"], name);
        assert_eq!(
            serde_json::from_value::<OutputEnvelope<Payload>>(wire)?,
            result
        );
    }
    assert!(serde_json::from_value::<OperationalErrorCode>(json!("INTERNAL_ERROR")).is_err());
    assert!(serde_json::from_value::<ToolStatus>(json!("success")).is_err());
    Ok(())
}

#[test]
fn cancellation_does_not_invent_an_error_code() -> TestResult {
    let cancelled = OutputEnvelope::cancelled(report()?);
    assert_eq!(cancelled.status(), ToolStatus::Cancelled);
    assert!(cancelled.is_operational_error());
    let wire = serde_json::to_value(&cancelled)?;
    assert_eq!(wire["error_code"], Value::Null);
    assert_eq!(
        serde_json::from_value::<OutputEnvelope<Payload>>(wire)?,
        cancelled
    );
    for status in [
        ToolStatus::Failed,
        ToolStatus::Blocked,
        ToolStatus::Unavailable,
        ToolStatus::Cancelled,
    ] {
        assert!(!status.is_success());
    }
    assert!(ToolStatus::Passed.is_success());
    Ok(())
}

#[test]
fn deserialization_cannot_bypass_outcome_invariants() -> TestResult {
    let valid = serde_json::to_value(OutputEnvelope::passed(report()?))?;
    for field in [
        "status",
        "error_code",
        "error_message",
        "data",
        "evidence",
        "truncation",
    ] {
        let mut candidate = valid.clone();
        candidate
            .as_object_mut()
            .ok_or("object expected")?
            .remove(field);
        assert!(
            serde_json::from_value::<OutputEnvelope<Payload>>(candidate).is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("error_code", json!("COMMAND_TIMEOUT")),
        ("error_message", json!("partial error")),
        ("status", json!("unavailable")),
        ("extra", json!(true)),
    ] {
        let mut candidate = valid.clone();
        candidate[field] = value;
        assert!(
            serde_json::from_value::<OutputEnvelope<Payload>>(candidate).is_err(),
            "{field}"
        );
    }
    let operational = OutputEnvelope::operational_error(
        OperationalError::new(OperationalErrorCode::CommandTimeout, "Timed out".parse()?),
        report()?,
    );
    let mut wrong_status = serde_json::to_value(operational)?;
    wrong_status["status"] = json!("unavailable");
    assert!(serde_json::from_value::<OutputEnvelope<Payload>>(wrong_status).is_err());
    let mut nested = valid;
    nested["data"]["unknown"] = json!(1);
    assert!(serde_json::from_value::<OutputEnvelope<Payload>>(nested).is_err());
    Ok(())
}
