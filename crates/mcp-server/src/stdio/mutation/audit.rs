//! Fixed local stderr event for completed M2 calls.
use super::{Action, Data, Output, Reason, Status};
use rust_engineering_application::MutationAllocationStats;
use serde::Serialize;

const SCHEMA: &str = "rust-mcp-mutation-event-v1";
const EVENT: &str = "mutation_call_completed";

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Preview,
    Commit,
    Receipt,
    Recover,
}

#[derive(Serialize)]
struct Event<'a> {
    schema: &'static str,
    event: &'static str,
    tool: &'static str,
    phase: Phase,
    admitted: bool,
    status: &'static str,
    reason: Option<&'static str>,
    duration_ms: u64,
    cleanup_uncertain: bool,
    result_id: Option<&'a str>,
    files_changed: usize,
    allocated_plans: Option<usize>,
    allocated_plan_bytes: Option<usize>,
}

pub(super) fn phase(action: &Action) -> Phase {
    match action {
        Action::Preview { .. } | Action::FormatPreview { .. } | Action::SemanticPreview { .. } => {
            Phase::Preview
        }
        Action::Commit { .. } => Phase::Commit,
        Action::Receipt { recover: true, .. } => Phase::Recover,
        Action::Receipt { recover: false, .. } => Phase::Receipt,
    }
}

pub(super) fn emit(
    tool: &'static str,
    phase: Phase,
    admitted: bool,
    cleanup_uncertain: bool,
    output: &Output,
    allocation: Option<MutationAllocationStats>,
) {
    if let Ok(encoded) = encode(tool, phase, admitted, cleanup_uncertain, output, allocation) {
        tracing::info!(target: "rust_engineering_mcp", "{encoded}");
    }
}

fn encode(
    tool: &'static str,
    phase: Phase,
    admitted: bool,
    cleanup_uncertain: bool,
    output: &Output,
    allocation: Option<MutationAllocationStats>,
) -> Result<String, serde_json::Error> {
    let (result_id, files_changed) = match output.data.as_ref() {
        Some(Data::Preview { plan_id, files, .. }) => (Some(plan_id.as_str()), files.len()),
        Some(Data::Receipt {
            operation_id,
            files,
            ..
        }) => (Some(operation_id.as_str()), files.len()),
        None => (None, 0),
    };
    serde_json::to_string(&Event {
        schema: SCHEMA,
        event: EVENT,
        tool,
        phase,
        admitted,
        status: status(output.status),
        reason: output.error_code.map(reason),
        duration_ms: output.duration_ms,
        cleanup_uncertain,
        result_id,
        files_changed,
        allocated_plans: allocation.map(|value| value.plans),
        allocated_plan_bytes: allocation.map(|value| value.bytes),
    })
}

fn status(value: Status) -> &'static str {
    match value {
        Status::Passed => "passed",
        Status::Failed => "failed",
        Status::Blocked => "blocked",
        Status::Unavailable => "unavailable",
        Status::Cancelled => "cancelled",
    }
}

fn reason(value: Reason) -> &'static str {
    match value {
        Reason::InvalidOperation => "invalid_operation",
        Reason::PermissionDenied => "permission_denied",
        Reason::Conflict => "conflict",
        Reason::LockBusy => "lock_busy",
        Reason::PlanExpired => "plan_expired",
        Reason::NotFound => "not_found",
        Reason::LimitExceeded => "limit_exceeded",
        Reason::UnsupportedPlatform => "unsupported_platform",
        Reason::Io => "io",
        Reason::RecoveryRequired => "recovery_required",
        Reason::ToolchainUnavailable => "toolchain_unavailable",
        Reason::CandidateInvalid => "candidate_invalid",
        Reason::Cancelled => "cancelled",
        Reason::CommandTimeout => "command_timeout",
        Reason::OfflineDataMissing => "offline_data_missing",
        Reason::OfflineDataInvalid => "offline_data_invalid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stdio::mutation::{
        Change, Concurrency, ExcludedGuarantee, Freshness, MutationEvidence, SnapshotSemantics,
        Truncation, ValidationMethod, ValidationView,
    };
    use serde_json::{Value, json};

    #[test]
    fn event_is_closed_and_excludes_paths_diffs_and_untrusted_values()
    -> Result<(), Box<dyn std::error::Error>> {
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let output = Output {
            status: Status::Passed,
            error_code: None,
            error_message: None,
            summary: "ignored public summary",
            duration_ms: 17,
            data: Some(Data::Preview {
                plan_id: "mut_0123456789abcdef0123456789abcdef".into(),
                plan_digest: fingerprint.clone(),
                expires_in_seconds: 600,
                files: vec![Change {
                    path: "secret-path.rs".into(),
                    before_sha256: fingerprint.clone(),
                    after_sha256: fingerprint.clone(),
                    before_bytes: 1,
                    after_bytes: 2,
                }],
                diff: "credential=synthetic_secret".into(),
                validation: ValidationView {
                    method: ValidationMethod::RustfmtThenFmtCheck,
                    semantics: SnapshotSemantics::LatestKnown,
                    platform: "linux/aarch64".into(),
                    image_id: fingerprint.clone(),
                    configuration_fingerprint: fingerprint.clone(),
                    execution_fingerprint: fingerprint.clone(),
                    rust_version: "1.98.1".into(),
                    cargo_version: "1.98.1".into(),
                    candidate_source_fingerprint: fingerprint.clone(),
                    mutation_execution_fingerprint: Some(fingerprint.clone()),
                    resolution: None,
                },
            }),
            diagnostics: [],
            truncation: Truncation::default(),
            evidence: MutationEvidence::MutationSnapshot {
                plan_digest: fingerprint,
                semantics: SnapshotSemantics::LatestKnown,
                freshness: Freshness::Unknown,
            },
            concurrency_contract: Concurrency::LocalCoordinated,
            guarantees_not_provided: [
                ExcludedGuarantee::OsExclusionOfExternalWriters,
                ExcludedGuarantee::MultiFileAtomicity,
                ExcludedGuarantee::MaliciousHostProtection,
                ExcludedGuarantee::DemonstratedPowerLossSurvival,
            ],
        };
        let encoded = encode(
            "rust.fmt.apply",
            Phase::Preview,
            true,
            false,
            &output,
            Some(MutationAllocationStats { plans: 1, bytes: 3 }),
        )?;
        for excluded in [
            "secret-path.rs",
            "synthetic_secret",
            "credential",
            "ignored public summary",
            "sha256:",
        ] {
            assert!(!encoded.contains(excluded), "leaked {excluded}: {encoded}");
        }
        let value: Value = serde_json::from_str(&encoded)?;
        assert_eq!(
            value,
            json!({
                "schema":"rust-mcp-mutation-event-v1",
                "event":"mutation_call_completed",
                "tool":"rust.fmt.apply",
                "phase":"preview",
                "admitted":true,
                "status":"passed",
                "reason":null,
                "duration_ms":17,
                "cleanup_uncertain":false,
                "result_id":"mut_0123456789abcdef0123456789abcdef",
                "files_changed":1,
                "allocated_plans":1,
                "allocated_plan_bytes":3
            })
        );
        Ok(())
    }

    #[test]
    fn unavailable_allocation_is_explicitly_null() -> Result<(), serde_json::Error> {
        let output = Output::failure(Reason::LockBusy, 4);
        let value: Value = serde_json::from_str(&encode(
            "rust.manifest.patch",
            Phase::Commit,
            false,
            false,
            &output,
            None,
        )?)?;
        assert_eq!(value["allocated_plans"], Value::Null);
        assert_eq!(value["allocated_plan_bytes"], Value::Null);
        Ok(())
    }

    #[test]
    fn phase_and_reason_mapping_are_exhaustive_and_recovery_is_distinct() {
        assert!(matches!(
            phase(&Action::Receipt {
                operation_id: "mut_0123456789abcdef0123456789abcdef".into(),
                recover: true,
            }),
            Phase::Recover
        ));
        for (value, expected) in [
            (Reason::Cancelled, "cancelled"),
            (Reason::CommandTimeout, "command_timeout"),
            (Reason::RecoveryRequired, "recovery_required"),
            (Reason::Io, "io"),
        ] {
            assert_eq!(reason(value), expected);
        }
    }
}
