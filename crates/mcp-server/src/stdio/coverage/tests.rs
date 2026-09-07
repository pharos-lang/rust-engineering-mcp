//! ADR-062 `rust.coverage` contract closure, metric projection and operational
//! mapping. The instrumented run itself is qualified by
//! `docs/validation/M3-runtime.json`; everything asserted here is portable.
use super::*;
use rust_engineering_application::coverage::{
    CoverageArtifactStreams, CoverageIdentity, CoverageObservation,
};
use rust_engineering_domain::{
    ArtifactMetadata, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    ArtifactSource, ExecutionFingerprint, PayloadFormatVersion, PluginIdentity,
    QualityArtifactDescriptor, QualityArtifactDraft, QualityArtifactId, QualityArtifactKind,
    QualityJobId, QualityMimeType, RuntimeIdentity, UtcInstant,
    coverage::{CoverageFile, CoverageMetrics, CoveragePackage, CoverageSummary},
};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;

const PROJECT_REF: &str = "prj_00000000000000000000000000000001";

fn digest(value: u8) -> String {
    format!("sha256:{}", format!("{value:02x}").repeat(32))
}

fn input(timeout_seconds: u64) -> Result<Input, Box<dyn std::error::Error>> {
    Ok(Input {
        project_ref: PROJECT_REF.parse()?,
        package: Some("member".into()),
        workspace: false,
        features: vec![],
        all_features: false,
        no_default_features: false,
        target: None,
        timeout_seconds,
        execution_mode: ExecutionModeDto::Auto,
    })
}

fn metrics() -> Result<CoverageMetrics, Box<dyn std::error::Error>> {
    Ok(CoverageMetrics::new((10, 5), (8, 4), (4, 3))?)
}

fn observation(files: usize) -> Result<CoverageObservation, Box<dyn std::error::Error>> {
    let fingerprint: ExecutionFingerprint = digest(6).parse()?;
    Ok(CoverageObservation {
        options: input(300)?.options()?,
        summary: CoverageSummary {
            aggregate: metrics()?,
            packages: vec![CoveragePackage {
                name: "member".into(),
                metrics: metrics()?,
            }],
            files: (0..files)
                .map(|index| {
                    Ok(CoverageFile {
                        path: format!("src/file_{index}.rs"),
                        package: "member".into(),
                        metrics: metrics()?,
                    })
                })
                .collect::<Result<_, Box<dyn std::error::Error>>>()?,
            files_omitted: 0,
        },
        identity: CoverageIdentity {
            cargo_llvm_cov_version: "0.9.0".into(),
            manifest_path: "/source/Cargo.toml".into(),
            llvm_tools_version: "1.98.1".into(),
        },
        doctests_run: false,
        cfg_coverage_enabled: true,
        target: "aarch64-unknown-linux-gnu",
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        parse_complete: true,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: digest(4),
            configuration_fingerprint: fingerprint.clone(),
            execution_fingerprint: fingerprint.clone(),
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        execution_fingerprint: fingerprint,
        artifacts: CoverageArtifactStreams::default(),
    })
}

fn ephemeral() -> Result<ArtifactMetadata, Box<dyn std::error::Error>> {
    Ok(ArtifactMetadata {
        owner: PROJECT_REF.parse()?,
        id: "art_00000000000000000000000000000002".parse()?,
        sha256: [0xa5; 32],
        size_bytes: 40,
        truncated: false,
        created_seconds: 0,
        expires_seconds: 3600,
    })
}

fn durable(
    kind: QualityArtifactKind,
    mime: QualityMimeType,
    payload: PayloadFormatVersion,
    guest: GuestArtifactName,
) -> Result<QualityArtifactDescriptor, Box<dyn std::error::Error>> {
    let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
    Ok(QualityArtifactDraft {
        artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
        member_index: 0,
        kind,
        mime_type: mime,
        payload_format_version: payload,
        completeness: ArtifactCompleteness::Complete,
        sensitivity: ArtifactSensitivity::SourceDerived,
        expires_at_utc: created.checked_add_seconds(3_600)?,
        created_at_utc: created,
        source: ArtifactSource {
            captured_source_sha256: [2; 32],
            guest_name: guest,
            selection: ArtifactSelection::Workspace,
        },
        runtime: ArtifactRuntime {
            image_digest: [3; 32],
            toolchain_identity: [4; 32],
            plugin: ArtifactPlugin {
                identity: PluginIdentity::Coverage,
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
        640 * 1024,
    )?)
}

fn wire(result: CallToolResult) -> Result<Value, Box<dyn std::error::Error>> {
    let value = serde_json::to_value(result)?;
    Ok(value["structuredContent"].clone())
}

#[test]
fn rejects_package_workspace_contradiction() -> TestResult {
    let mut contradiction = input(300)?;
    contradiction.workspace = true;
    assert!(contradiction.options().is_err());
    Ok(())
}

#[test]
fn closed_input_rejects_flags_and_unqualified_selections() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    assert!(contract.decode(None).is_err());
    for patch in [
        json!({"doctests": true}),
        json!({"ignore_filename_regex": ".*"}),
        json!({"package": "--manifest-path"}),
        json!({"package": "member", "workspace": true}),
        json!({"features": ["x"], "all_features": true}),
        json!({"target": "x86_64-unknown-linux-gnu"}),
        json!({"timeout_seconds": 0}),
        json!({"timeout_seconds": 3601}),
    ] {
        let mut value = json!({"project_ref": PROJECT_REF});
        value
            .as_object_mut()
            .ok_or("object")?
            .extend(patch.as_object().ok_or("patch")?.clone());
        assert!(
            contract
                .decode(Some(serde_json::from_value(value)?))
                .and_then(|input| input.options())
                .is_err()
        );
    }
    let accepted = contract.decode(Some(serde_json::from_value(json!({
        "project_ref": PROJECT_REF,
        "workspace": true,
        "target": "aarch64-unknown-linux-gnu",
        "timeout_seconds": 45,
        "execution_mode": "synchronous",
    }))?))?;
    assert_eq!(accepted.options()?.timeout_seconds(), 45);
    assert!(matches!(
        accepted.execution_mode,
        ExecutionModeDto::Synchronous
    ));
    Ok(())
}

#[test]
fn a_work_budget_above_the_synchronous_bound_requires_tasks() -> TestResult {
    let tool = CoverageTool::new()?;
    let value = wire(tool.tasks_required()?)?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["error_code"], "TASKS_REQUIRED");
    assert_eq!(value["data"], Value::Null);
    // Discovery that has not completed blocks with the sandbox code instead.
    let denied = wire(tool.blocked(Code::SandboxDenied, "not ready", None, 5)?)?;
    assert_eq!(denied["status"], "blocked");
    assert_eq!(denied["error_code"], "SANDBOX_DENIED");
    assert_eq!(denied["summary"], "not ready");
    assert_eq!(denied["duration_ms"], 5);
    Ok(())
}

#[test]
fn every_inspection_error_maps_to_one_declared_operational_outcome() -> TestResult {
    let tool = CoverageTool::new()?;
    for (error, status, code) in [
        (
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ToolNotInstalled,
            )),
            "unavailable",
            json!("TOOL_NOT_INSTALLED"),
        ),
        (
            InspectionError::Execution(ExecutionError::Unavailable),
            "unavailable",
            json!("TOOL_NOT_INSTALLED"),
        ),
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
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            )),
            "blocked",
            json!("PROJECT_NOT_FOUND"),
        ),
        (
            InspectionError::InvalidMetadata,
            "blocked",
            json!("INVALID_PROJECT"),
        ),
        (
            InspectionError::Internal,
            "blocked",
            json!("INVALID_PROJECT"),
        ),
    ] {
        let value = wire(tool.encode_error(error, 9)?)?;
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], code);
        assert_eq!(value["duration_ms"], 9);
        assert_eq!(value["data"], Value::Null);
    }
    Ok(())
}

#[test]
fn a_complete_run_projects_bounded_metrics_and_artifacts() -> TestResult {
    let tool = CoverageTool::new()?;
    let project: ProjectRef = PROJECT_REF.parse()?;
    let references = vec![
        CoverageArtifactReference::Ephemeral {
            kind: CoverageArtifactKind::Json,
            metadata: ephemeral()?,
        },
        CoverageArtifactReference::Ephemeral {
            kind: CoverageArtifactKind::Lcov,
            metadata: ephemeral()?,
        },
        CoverageArtifactReference::Ephemeral {
            kind: CoverageArtifactKind::StdoutLog,
            metadata: ephemeral()?,
        },
        CoverageArtifactReference::Durable(Box::new(durable(
            QualityArtifactKind::ArchiveBundle,
            QualityMimeType::ApplicationXTar,
            PayloadFormatVersion::UstarV1,
            GuestArtifactName::ReportArchive,
        )?)),
    ];
    let result =
        CoverageTaskResult::new(observation(3)?, references, 12).map_err(|_| "task result")?;
    let value = wire(tool.encode_result(&project, result)?)?;
    assert_eq!(value["status"], "passed");
    assert_eq!(value["data"]["project_ref"], PROJECT_REF);
    assert_eq!(value["data"]["aggregate"]["lines"]["count"], 10);
    assert_eq!(value["data"]["aggregate"]["lines"]["covered"], 5);
    assert_eq!(
        value["data"]["aggregate"]["lines"]["percent_millionths"],
        50_000_000
    );
    assert_eq!(value["data"]["packages"][0]["name"], "member");
    assert_eq!(value["data"]["files_page_rows"], 3);
    assert_eq!(value["data"]["files_omitted"], false);
    assert_eq!(value["data"]["doctests_run"], false);
    assert_eq!(value["data"]["termination"], "exited");
    assert_eq!(value["data"]["artifacts"][0]["kind"], "json");
    assert_eq!(
        value["data"]["artifacts"][0]["uri"],
        format!("rust-artifact://{project}/art_00000000000000000000000000000002")
    );
    assert_eq!(value["data"]["artifacts"][0]["sha256"], "a5".repeat(32));
    assert_eq!(value["data"]["artifacts"][0]["completeness"], "complete");
    assert_eq!(value["data"]["artifacts"][1]["kind"], "lcov");
    assert_eq!(value["data"]["artifacts"][2]["kind"], "tool_log");
    assert_eq!(value["data"]["artifacts"][3]["kind"], "archive_bundle");
    // A durable member is read back through a bounded quality-artifact window.
    assert_eq!(
        value["data"]["artifacts"][3]["uri"],
        format!(
            "rust-quality-artifact://{project}/{}?offset=0&length={}",
            durable(
                QualityArtifactKind::ArchiveBundle,
                QualityMimeType::ApplicationXTar,
                PayloadFormatVersion::UstarV1,
                GuestArtifactName::ReportArchive,
            )?
            .artifact_id,
            320 * 1024
        )
    );
    assert_eq!(value["data"]["artifacts"][3]["size_bytes"], 640 * 1024);
    Ok(())
}

#[test]
fn incomplete_evidence_is_blocked_and_still_carries_its_data() -> TestResult {
    let tool = CoverageTool::new()?;
    let project: ProjectRef = PROJECT_REF.parse()?;
    for (parse_complete, termination, exit_code) in [
        (false, ExecutionTermination::Exited, Some(0)),
        (true, ExecutionTermination::TimedOut, None),
        (true, ExecutionTermination::Exited, Some(101)),
    ] {
        let mut observation = observation(1)?;
        observation.parse_complete = parse_complete;
        observation.termination = termination;
        observation.exit_code = exit_code;
        let result =
            CoverageTaskResult::new(observation, Vec::new(), 12).map_err(|_| "task result")?;
        let value = wire(tool.encode_result(&project, result)?)?;
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["error_code"], "INVALID_PROJECT");
        assert_eq!(value["data"]["project_ref"], PROJECT_REF);
    }
    Ok(())
}

#[test]
fn the_file_page_is_bounded_and_reports_its_omission() -> TestResult {
    let tool = CoverageTool::new()?;
    let project: ProjectRef = PROJECT_REF.parse()?;
    let mut observation = observation(2)?;
    observation.summary.files_omitted = 9;
    let result = CoverageTaskResult::new(observation, Vec::new(), 12).map_err(|_| "task result")?;
    let value = wire(tool.encode_result(&project, result)?)?;
    assert_eq!(value["data"]["files_page_rows"], 2);
    assert_eq!(value["data"]["files_omitted"], true);
    Ok(())
}

#[test]
fn every_termination_and_artifact_kind_has_one_declared_spelling() -> TestResult {
    for (value, expected) in [
        (ExecutionTermination::Exited, "exited"),
        (ExecutionTermination::TimedOut, "timed_out"),
        (ExecutionTermination::Cancelled, "cancelled"),
        (ExecutionTermination::OutputLimit, "output_limit"),
    ] {
        assert_eq!(
            serde_json::to_value(termination(value))?,
            Value::String(expected.into())
        );
    }
    for (value, expected) in [
        (CoverageArtifactKind::Json, "json"),
        (CoverageArtifactKind::Lcov, "lcov"),
        (CoverageArtifactKind::ArchiveBundle, "archive_bundle"),
        (CoverageArtifactKind::StdoutLog, "tool_log"),
        (CoverageArtifactKind::StderrLog, "tool_log"),
    ] {
        assert_eq!(
            serde_json::to_value(kind_dto(value))?,
            Value::String(expected.into())
        );
    }
    assert_eq!(hex(&[0x0f; 32]), "0f".repeat(32));
    assert_eq!(hex(&[0xf0; 32]), "f0".repeat(32));
    Ok(())
}

#[test]
fn a_durable_member_that_fails_validation_is_not_projected() -> TestResult {
    let project: ProjectRef = PROJECT_REF.parse()?;
    let mut invalid = durable(
        QualityArtifactKind::CoverageJson,
        QualityMimeType::ApplicationJson,
        PayloadFormatVersion::CoverageJsonV1,
        GuestArtifactName::CoverageJson,
    )?;
    invalid.format_version = 2;
    assert!(
        artifacts(
            &project,
            vec![CoverageArtifactReference::Durable(Box::new(invalid))]
        )
        .is_err()
    );
    // The valid guest names map onto the declared kinds, and anything else is
    // reported as an opaque tool log rather than as coverage data.
    for (guest, kind, mime, payload, expected) in [
        (
            GuestArtifactName::CoverageJson,
            QualityArtifactKind::CoverageJson,
            QualityMimeType::ApplicationJson,
            PayloadFormatVersion::CoverageJsonV1,
            "json",
        ),
        (
            GuestArtifactName::Lcov,
            QualityArtifactKind::Lcov,
            QualityMimeType::TextPlain,
            PayloadFormatVersion::LcovV1,
            "lcov",
        ),
        (
            GuestArtifactName::ToolLog,
            QualityArtifactKind::ToolLog,
            QualityMimeType::TextPlain,
            PayloadFormatVersion::Utf8LogV1,
            "tool_log",
        ),
    ] {
        let projected = artifacts(
            &project,
            vec![CoverageArtifactReference::Durable(Box::new(durable(
                kind, mime, payload, guest,
            )?))],
        )?;
        assert_eq!(
            serde_json::to_value(&projected[0].kind)?,
            Value::String(expected.into())
        );
    }
    Ok(())
}

#[test]
fn joined_cleanup_signals_are_reported_over_a_completed_body() {
    assert!(matches!(
        joined_result(Joined {
            result: Ok(7_u8),
            interrupted: None
        }),
        Ok(7)
    ));
    assert!(matches!(
        joined_result(Joined {
            result: Ok(7_u8),
            interrupted: Some(WorkerError::Cancelled)
        }),
        Err(InspectionError::Project(ProjectError::Cancelled))
    ));
    assert!(matches!(
        joined_result(Joined {
            result: Ok(7_u8),
            interrupted: Some(WorkerError::TimedOut)
        }),
        Err(InspectionError::Project(ProjectError::Rejected(
            OperationalErrorCode::CommandTimeout
        )))
    ));
    assert!(matches!(
        joined_result(Joined {
            result: Ok(7_u8),
            interrupted: Some(WorkerError::Busy)
        }),
        Err(InspectionError::Internal)
    ));
    assert!(matches!(
        joined_result::<u8>(Joined {
            result: Err(InspectionError::OutputLimit),
            interrupted: None
        }),
        Err(InspectionError::OutputLimit)
    ));
}
