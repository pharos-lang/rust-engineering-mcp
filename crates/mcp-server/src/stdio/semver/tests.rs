//! ADR-062 `rust.semver.check` contract closure, projections and operational
//! mapping. Everything here is portable: the gateway itself is qualified by
//! `docs/validation/M3-runtime.json`, not by this module.
use super::*;
use crate::stdio::workers::{Joined, WorkerError};
use rust_engineering_application::semver_check::SemverObservation;
use rust_engineering_domain::{
    ArtifactMetadata, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    ArtifactSource, ExecutionTermination, FreshnessPolicy, IntegrityStatus, PayloadFormatVersion,
    PluginIdentity, Provenance, QualityArtifactDraft, QualityArtifactId, QualityArtifactKind,
    QualityJobId, QualityMimeType, RuntimeIdentity, SnapshotEvidence, SourceKind, UtcInstant,
    semver_check::{SemverExit, SemverFinding, SemverFindingCompleteness, SemverFindingCounts},
};
use rust_engineering_domain::{Clock, UnixSeconds};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const BASELINE_REF: &str = "prj_00000000000000000000000000000001";
const CANDIDATE_REF: &str = "prj_00000000000000000000000000000002";

fn digest(value: u8) -> String {
    format!("sha256:{}", format!("{value:02x}").repeat(32))
}

fn evidence() -> Result<SnapshotEvidence, Box<dyn std::error::Error>> {
    struct At;
    impl Clock for At {
        fn now(&self) -> UnixSeconds {
            UnixSeconds(102)
        }
    }
    Ok(SnapshotEvidence::assess(
        Provenance::new(
            SourceKind::ProjectSnapshot,
            digest(3).parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(101)),
            IntegrityStatus::Verified,
            false,
        )?,
        FreshnessPolicy::new("captured-project-v1".parse()?, 60, 300)?,
        &At,
    ))
}

fn options() -> Result<SemverOptions, Box<dyn std::error::Error>> {
    let selection = SemverCommandOptions::try_from(SemverProjectSelection {
        package: Some("member".into()),
        features: vec!["std".into()],
        all_features: false,
        no_default_features: true,
        target: Some("aarch64-unknown-linux-gnu".into()),
    })?;
    Ok(SemverOptions::new(
        selection.clone(),
        selection,
        SEMVER_DEFAULT_TIMEOUT_SECONDS,
    )?)
}

fn observation(findings: usize) -> Result<SemverObservation, Box<dyn std::error::Error>> {
    Ok(SemverObservation {
        options: options()?,
        exit: SemverExit::Breaking,
        counts: SemverFindingCounts {
            deny: u32::try_from(findings)?,
            warn: 0,
        },
        findings: (0..findings)
            .map(|index| {
                SemverFinding::new(
                    format!("pub fn removed_{index}"),
                    "function_missing".into(),
                    SemverFindingLevel::Deny,
                    Some(SemverRequiredUpdate::Major),
                    Some("src/lib.rs:1".into()),
                )
            })
            .collect::<Result<_, _>>()?,
        findings_omitted: 4,
        completeness: SemverFindingCompleteness::Partial,
        termination: ExecutionTermination::Exited,
        exit_code: Some(1),
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: digest(4),
            configuration_fingerprint: digest(5).parse()?,
            execution_fingerprint: digest(6).parse()?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        execution_fingerprint: digest(6).parse()?,
        stdout: Vec::new(),
        stderr: Vec::new(),
        stdout_truncated: false,
        stderr_truncated: false,
    })
}

fn result(
    outcome: SemverOutcome,
    findings: usize,
) -> Result<SemverProjectResult, Box<dyn std::error::Error>> {
    Ok(SemverProjectResult {
        baseline_project_ref: BASELINE_REF.parse()?,
        baseline_project_identity_fingerprint: digest(1).parse()?,
        baseline_evidence: evidence()?,
        candidate_project_ref: CANDIDATE_REF.parse()?,
        candidate_project_identity_fingerprint: digest(2).parse()?,
        candidate_evidence: evidence()?,
        outcome,
        observation: Some(observation(findings)?),
        raw_output: None,
        raw_output_omitted: false,
    })
}

fn ephemeral() -> Result<ArtifactMetadata, Box<dyn std::error::Error>> {
    Ok(ArtifactMetadata {
        owner: CANDIDATE_REF.parse()?,
        id: "art_00000000000000000000000000000003".parse()?,
        sha256: [0x5a; 32],
        size_bytes: 12,
        truncated: true,
        created_seconds: 0,
        expires_seconds: 3600,
    })
}

fn durable_log()
-> Result<rust_engineering_domain::QualityArtifactDescriptor, Box<dyn std::error::Error>> {
    let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
    Ok(QualityArtifactDraft {
        artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
        member_index: 0,
        kind: QualityArtifactKind::ToolLog,
        mime_type: QualityMimeType::TextPlain,
        payload_format_version: PayloadFormatVersion::Utf8LogV1,
        completeness: ArtifactCompleteness::Truncated,
        sensitivity: ArtifactSensitivity::SourceDerived,
        expires_at_utc: created.checked_add_seconds(3_600)?,
        created_at_utc: created,
        source: ArtifactSource {
            captured_source_sha256: [2; 32],
            guest_name: GuestArtifactName::ToolLog,
            selection: ArtifactSelection::Workspace,
        },
        runtime: ArtifactRuntime {
            image_digest: [3; 32],
            toolchain_identity: [4; 32],
            plugin: ArtifactPlugin {
                identity: PluginIdentity::Semver,
                version: 1,
                digest: [5; 32],
            },
            implementation_digest: [6; 32],
        },
    }
    .into_descriptor(
        QualityJobId::from_random_bytes([7; 16]),
        [8; 32],
        [9; 32],
        512 * 1024,
    )?)
}

fn wire(result: CallToolResult) -> Result<Value, Box<dyn std::error::Error>> {
    let value = serde_json::to_value(result)?;
    Ok(value["structuredContent"].clone())
}

#[test]
fn schema_is_closed_and_selection_is_single() -> TestResult {
    let tool = SemverTool::new()?;
    let definition = serde_json::to_value(tool.definition)?;
    assert_eq!(definition["name"], NAME);
    assert_eq!(definition["inputSchema"]["additionalProperties"], false);
    assert!(
        definition["inputSchema"]["properties"]
            .get("baseline_features")
            .is_none()
    );
    assert!(
        definition["inputSchema"]["properties"]
            .get("candidate_features")
            .is_none()
    );
    assert_eq!(
        definition["outputSchema"]["$defs"]["Data"]["properties"]["findings"]["maxItems"],
        MAX_RESPONSE_FINDINGS
    );
    Ok(())
}

#[test]
fn maximally_escaped_itemized_findings_keep_the_mirrored_result_below_512_kib() -> TestResult {
    let tool = SemverTool::new()?;
    let side = |suffix: char| SideEvidence {
        project_ref: format!("prj_{}", suffix.to_string().repeat(32)),
        project_identity_fingerprint: format!("sha256:{}", suffix.to_string().repeat(64)),
        captured_source_sha256: format!("sha256:{}", suffix.to_string().repeat(64)),
        captured_at_unix_seconds: Some(1),
        assessed_at_unix_seconds: 1,
    };
    let selection = || Selection {
        package: Some("p".repeat(128)),
        features: vec!["f".repeat(128); 32],
        all_features: false,
        no_default_features: false,
        target: Some("aarch64-unknown-linux-gnu".into()),
    };
    let escaped = "\0".repeat(512);
    let result = tool.contract.encode(Output {
        outcome: Outcome::Failed {
            error_code: (),
            error_message: (),
            data: Box::new(Data {
                baseline: side('1'),
                candidate: side('2'),
                baseline_selection: selection(),
                candidate_selection: selection(),
                counts: Counts {
                    deny: MAX_RESPONSE_FINDINGS as u32,
                    warn: 0,
                },
                findings: (0..MAX_RESPONSE_FINDINGS)
                    .map(|_| Finding {
                        item: escaped.clone(),
                        lint: escaped.clone(),
                        level: FindingLevel::Deny,
                        required_update: Some(RequiredUpdate::Major),
                        span: Some(escaped.clone()),
                    })
                    .collect(),
                completeness: Completeness::Partial,
                termination: rust_engineering_domain::ExecutionTermination::Exited,
                exit_code: Some(100),
                runtime: RuntimeEvidence {
                    platform: "linux/aarch64".into(),
                    image_id: format!("sha256:{}", "3".repeat(64)),
                    configuration_fingerprint: format!("sha256:{}", "4".repeat(64)),
                    execution_fingerprint: format!("sha256:{}", "5".repeat(64)),
                    rust_version: "1.98.1".into(),
                    cargo_version: "1.98.1".into(),
                },
                raw_output: None,
                findings_omitted: 1,
                raw_output_omitted: false,
            }),
        },
        summary: "Semantic-version breaking changes were observed",
        duration_ms: 1,
    })?;
    assert!(serde_json::to_vec(&result)?.len() <= 512 * 1024);
    Ok(())
}

#[test]
fn closed_input_rejects_asymmetric_and_contradictory_selections() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    assert!(contract.decode(None).is_err());
    for patch in [
        json!({"baseline_features": ["std"]}),
        json!({"package": "--help"}),
        json!({"features": ["x", "x"]}),
        json!({"features": ["x"], "all_features": true}),
        json!({"target": "x86_64-unknown-linux-gnu"}),
        json!({"timeout_seconds": 0}),
        json!({"timeout_seconds": 3601}),
        json!({"execution_mode": "background"}),
    ] {
        let mut value = json!({
            "baseline_project_ref": BASELINE_REF,
            "candidate_project_ref": CANDIDATE_REF,
        });
        value
            .as_object_mut()
            .ok_or("object")?
            .extend(patch.as_object().ok_or("patch")?.clone());
        let error = contract
            .decode(Some(serde_json::from_value(value)?))
            .and_then(|input| input.options())
            .err()
            .ok_or("invalid selection accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    }
    let input = contract.decode(Some(serde_json::from_value(json!({
        "baseline_project_ref": BASELINE_REF,
        "candidate_project_ref": CANDIDATE_REF,
        "features": ["std", "dep/derive"],
        "no_default_features": true,
        "target": "aarch64-unknown-linux-gnu",
        "timeout_seconds": 45,
    }))?))?;
    let options = input.options()?;
    // One decoded selection is applied identically to both sides.
    assert_eq!(options.baseline_selection(), options.selection());
    assert_eq!(options.selection().features(), &["dep/derive", "std"]);
    assert_eq!(options.timeout_seconds(), 45);
    Ok(())
}

#[test]
fn a_work_budget_above_the_synchronous_bound_requires_tasks() -> TestResult {
    let tool = SemverTool::new()?;
    let value = wire(tool.tasks_required()?)?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["error_code"], "TASKS_REQUIRED");
    assert_eq!(value["data"], Value::Null);
    assert_eq!(value["duration_ms"], 0);
    Ok(())
}

#[test]
fn every_inspection_error_maps_to_one_declared_operational_outcome() -> TestResult {
    let tool = SemverTool::new()?;
    for (error, status, code) in [
        (
            InspectionError::Project(ProjectError::Cancelled),
            "cancelled",
            Value::Null,
        ),
        (
            InspectionError::Execution(ExecutionError::Cancelled),
            "cancelled",
            Value::Null,
        ),
        (
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            )),
            "blocked",
            json!("PROJECT_NOT_FOUND"),
        ),
        (
            InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::InvalidProject)),
            "blocked",
            json!("INVALID_PROJECT"),
        ),
        (
            InspectionError::InvalidMetadata,
            "blocked",
            json!("INVALID_PROJECT"),
        ),
        (
            InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::CommandTimeout)),
            "blocked",
            json!("COMMAND_TIMEOUT"),
        ),
        (
            InspectionError::OutputLimit,
            "blocked",
            json!("OUTPUT_LIMIT_EXCEEDED"),
        ),
        (
            InspectionError::Execution(ExecutionError::Unavailable),
            "unavailable",
            json!("TOOL_NOT_INSTALLED"),
        ),
        (
            InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::NetworkDenied)),
            "blocked",
            json!("SANDBOX_DENIED"),
        ),
        (
            InspectionError::Execution(ExecutionError::Denied),
            "blocked",
            json!("SANDBOX_DENIED"),
        ),
        (
            InspectionError::Execution(ExecutionError::Busy),
            "blocked",
            json!("SANDBOX_DENIED"),
        ),
        (
            InspectionError::Execution(ExecutionError::InvalidConfiguration),
            "blocked",
            json!("SANDBOX_DENIED"),
        ),
    ] {
        let value = wire(tool.encode_error(error, 7)?)?;
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], code);
        assert_eq!(value["duration_ms"], 7);
    }
    // Uncertain cleanup and infrastructure failures are protocol errors: they
    // never become a tool result a peer could read as an assessment.
    for error in [
        InspectionError::Execution(ExecutionError::CleanupUncertain),
        InspectionError::Execution(ExecutionError::Infrastructure),
        InspectionError::Project(ProjectError::Internal),
        InspectionError::Internal,
    ] {
        let failure = tool.encode_error(error, 7).err().ok_or("tool result")?;
        assert_eq!(failure.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
    }
    Ok(())
}

#[test]
fn worker_signals_and_joined_cleanup_map_to_inspection_errors() {
    assert!(matches!(
        worker_error(WorkerError::Busy),
        InspectionError::Execution(ExecutionError::Busy)
    ));
    assert!(matches!(
        worker_error(WorkerError::Cancelled),
        InspectionError::Project(ProjectError::Cancelled)
    ));
    assert!(matches!(
        worker_error(WorkerError::TimedOut),
        InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
    ));
    assert!(matches!(
        worker_error(WorkerError::Internal),
        InspectionError::Internal
    ));
    // A completed body that was interrupted during cleanup reports the signal,
    // and a cancellation observed by the body defers to the signal that caused it.
    assert!(matches!(
        joined_result(Joined {
            result: Ok(1_u8),
            interrupted: None
        }),
        Ok(1)
    ));
    assert!(matches!(
        joined_result(Joined {
            result: Ok(1_u8),
            interrupted: Some(WorkerError::TimedOut)
        }),
        Err(InspectionError::Project(ProjectError::Rejected(
            OperationalErrorCode::CommandTimeout
        )))
    ));
    assert!(matches!(
        joined_result::<u8>(Joined {
            result: Err(InspectionError::Project(ProjectError::Cancelled)),
            interrupted: Some(WorkerError::TimedOut)
        }),
        Err(InspectionError::Project(ProjectError::Rejected(
            OperationalErrorCode::CommandTimeout
        )))
    ));
    assert!(matches!(
        joined_result::<u8>(Joined {
            result: Err(InspectionError::OutputLimit),
            interrupted: Some(WorkerError::Cancelled)
        }),
        Err(InspectionError::OutputLimit)
    ));
}

#[test]
fn raw_output_projects_ephemeral_and_durable_references() -> TestResult {
    let owner: ProjectRef = CANDIDATE_REF.parse()?;
    let projected = artifact(&owner, SemverArtifactReference::Ephemeral(ephemeral()?))?;
    assert_eq!(projected.uri, resources::uri(&owner, &ephemeral()?.id));
    assert_eq!(projected.sha256, "5a".repeat(32));
    assert_eq!(projected.size_bytes, 12);
    assert!(matches!(projected.completeness, Completeness::Truncated));

    let durable = artifact(
        &owner,
        SemverArtifactReference::Durable(Box::new(durable_log()?)),
    )?;
    assert_eq!(
        durable.uri,
        format!(
            "rust-quality-artifact://{owner}/{}?offset=0&length={}",
            durable_log()?.artifact_id,
            256 * 1024
        )
    );
    assert_eq!(durable.sha256, "09".repeat(32));
    assert!(matches!(durable.completeness, Completeness::Truncated));

    // A reference owned by another project, or one that is not the tool log,
    // is an internal failure rather than a readable Resource.
    let other: ProjectRef = BASELINE_REF.parse()?;
    assert!(artifact(&other, SemverArtifactReference::Ephemeral(ephemeral()?)).is_err());
    let mut wrong = durable_log()?;
    wrong.source.guest_name = GuestArtifactName::JunitXml;
    assert!(artifact(&owner, SemverArtifactReference::Durable(Box::new(wrong))).is_err());
    let mut invalid = durable_log()?;
    invalid.format_version = 2;
    assert!(artifact(&owner, SemverArtifactReference::Durable(Box::new(invalid))).is_err());
    Ok(())
}

#[test]
fn every_coarse_outcome_projects_its_declared_status() -> TestResult {
    let tool = SemverTool::new()?;
    for (outcome, status, code) in [
        (SemverOutcome::NoBreak, "passed", Value::Null),
        (SemverOutcome::Breaking, "failed", Value::Null),
        (
            SemverOutcome::Incomplete,
            "blocked",
            json!("INCOMPLETE_EVIDENCE"),
        ),
        (
            SemverOutcome::Blocked,
            "blocked",
            json!("INCOMPLETE_EVIDENCE"),
        ),
    ] {
        let value = wire(tool.encode_result(result(outcome, 2)?, 11)?)?;
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], code);
        assert_eq!(value["duration_ms"], 11);
        assert_eq!(value["data"]["baseline"]["project_ref"], BASELINE_REF);
        assert_eq!(value["data"]["candidate"]["project_ref"], CANDIDATE_REF);
        assert_eq!(
            value["data"]["baseline_selection"],
            value["data"]["candidate_selection"]
        );
        assert_eq!(value["data"]["counts"]["deny"], 2);
        assert_eq!(value["data"]["findings"].as_array().ok_or("rows")?.len(), 2);
        assert_eq!(value["data"]["findings"][0]["level"], "deny");
        assert_eq!(value["data"]["findings"][0]["required_update"], "major");
        assert_eq!(value["data"]["completeness"], "partial");
        assert_eq!(value["data"]["findings_omitted"], 4);
        assert_eq!(value["data"]["raw_output"], Value::Null);
    }
    // A missing library target is unavailable, and it carries no assessment.
    let mut unavailable = result(SemverOutcome::Unavailable, 0)?;
    unavailable.observation = None;
    let value = wire(tool.encode_result(unavailable, 11)?)?;
    assert_eq!(value["status"], "unavailable");
    assert_eq!(value["error_code"], "INVALID_PROJECT");
    assert_eq!(value["data"], Value::Null);
    // Any other outcome without an observation cannot be projected at all.
    let mut missing = result(SemverOutcome::Breaking, 0)?;
    missing.observation = None;
    assert!(tool.encode_result(missing, 11).is_err());
    Ok(())
}

#[test]
fn itemized_findings_stop_at_the_response_bound_and_are_counted_as_omitted() -> TestResult {
    let tool = SemverTool::new()?;
    let mut result = result(SemverOutcome::Breaking, MAX_RESPONSE_FINDINGS + 3)?;
    result.raw_output = Some(SemverArtifactReference::Ephemeral(ephemeral()?));
    result.raw_output_omitted = false;
    let value = wire(tool.encode_result(result, 3)?)?;
    assert_eq!(
        value["data"]["findings"].as_array().ok_or("rows")?.len(),
        MAX_RESPONSE_FINDINGS
    );
    // The parser's own omissions and the response bound are both reported.
    assert_eq!(value["data"]["findings_omitted"], 4 + 3);
    assert_eq!(value["data"]["counts"]["deny"], MAX_RESPONSE_FINDINGS + 3);
    assert_eq!(value["data"]["raw_output"]["completeness"], "truncated");
    Ok(())
}
