use super::*;
use rust_engineering_application::quality_v2::PublishedQualityV2;
use rust_engineering_domain::coverage::CoverageMetrics;
use rust_engineering_domain::quality_v2::{
    QualityV2Details, QualityV2Report, QualityV2Stage, QualityV2StageKind,
};
use rust_engineering_domain::security::{
    FindingDisposition, SecurityEngine, SecurityFinding, SecurityPackage, SecurityPolicyState,
    SecuritySeverity, SecuritySource, SecuritySuppression,
};
use rust_engineering_domain::supply_chain::{SupplyAdvisory, SupplyAudit, SupplyDeny};
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    AuditSource, AuditState, ExecutionFingerprint, ExecutionTermination, GuestArtifactName,
    PayloadFormatVersion, PluginIdentity, QualityArtifactDescriptor, QualityArtifactDraft,
    QualityArtifactId, QualityArtifactKind, QualityJobId, QualityMimeType, RuntimeIdentity,
    SourceFingerprint, ToolStatus, UtcInstant,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const MAX_RESULT_BYTES: usize = 512 * 1024;

fn source_fingerprint(digit: char) -> TestResult<SourceFingerprint> {
    Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
}

fn execution_fingerprint(digit: char) -> TestResult<ExecutionFingerprint> {
    Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
}

fn runtime() -> TestResult<RuntimeIdentity> {
    Ok(RuntimeIdentity {
        platform: "linux/aarch64".into(),
        image_id: format!("sha256:{}", "1".repeat(64)),
        configuration_fingerprint: execution_fingerprint('2')?,
        execution_fingerprint: execution_fingerprint('3')?,
        rust_version: "1.98.1".into(),
        cargo_version: "1.98.1".into(),
        declared_toolchain: Some("1.98".into()),
    })
}

fn descriptor() -> TestResult<QualityArtifactDescriptor> {
    let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
    Ok(QualityArtifactDraft {
        artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
        member_index: 0,
        kind: QualityArtifactKind::ToolLog,
        mime_type: QualityMimeType::TextPlain,
        payload_format_version: PayloadFormatVersion::Utf8LogV1,
        completeness: ArtifactCompleteness::Complete,
        sensitivity: ArtifactSensitivity::PotentiallySensitive,
        created_at_utc: created.clone(),
        expires_at_utc: created.checked_add_seconds(60)?,
        source: rust_engineering_domain::ArtifactSource {
            captured_source_sha256: [2; 32],
            guest_name: GuestArtifactName::ToolLog,
            selection: ArtifactSelection::Workspace,
        },
        runtime: ArtifactRuntime {
            image_digest: [3; 32],
            toolchain_identity: [4; 32],
            plugin: ArtifactPlugin {
                identity: PluginIdentity::Builtin,
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
        512,
    )?)
}

fn validation(stage: QualityV2StageKind, execution: &ExecutionFingerprint) -> QualityV2Stage {
    QualityV2Stage {
        stage,
        status: ToolStatus::Passed,
        issue: None,
        duration_ms: 1,
        execution_fingerprint: Some(execution.clone()),
        details: Some(QualityV2Details::Validation {
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            validation_complete: true,
            diagnostics: 0,
            diagnostics_omitted: 0,
            affected_files: None,
            build_succeeded: None,
        }),
    }
}

fn large_complete_observation() -> TestResult<QualityV2Observation> {
    let execution = execution_fingerprint('4')?;
    let rules_digest = source_fingerprint('9')?;
    let package_source_fingerprint = source_fingerprint('5')?;
    let findings = (0..128)
        .map(|index| {
            let rule = format!("license-rule-{index:03}-{}", "r".repeat(75));
            SecurityFinding {
                engine: SecurityEngine::Licenses,
                rule: rule.clone(),
                package: Some(SecurityPackage {
                    name: format!("package-{index:03}-{}", "p".repeat(52)),
                    version: format!("1.0.0+{}", "v".repeat(120)),
                    source: SecuritySource::CratesIo,
                    source_fingerprint: Some(package_source_fingerprint.clone()),
                }),
                severity: SecuritySeverity::Note,
                message: format!("cargo-deny reported rule '{rule}'"),
                disposition: FindingDisposition::Suppressed(SecuritySuppression {
                    id: format!("suppression-{index:03}-{}", "i".repeat(48)),
                    engine: SecurityEngine::Licenses,
                    rule: format!("license-rule-{index:03}-{}", "r".repeat(75)),
                    package: format!("package-{index:03}-{}", "p".repeat(52)),
                    package_source: SecuritySource::CratesIo,
                    version_requirement: format!("=1.0.0+{}", "v".repeat(119)),
                    reason: "\"".repeat(512),
                    owner: "\\".repeat(128),
                    expires_at: 1_900_000_000,
                    rules_digest: rules_digest.clone(),
                }),
            }
        })
        .collect();
    let audit_findings = (0..128)
        .map(|index| SupplyAdvisory {
            advisory_id: format!("RUSTSEC-2026-{index:04}"),
            package_name: format!("package-{index:03}-{}", "p".repeat(52)),
            package_version: format!("1.0.0+{}", "v".repeat(120)),
            package_source: AuditSource::CratesIo,
            source_fingerprint: Some(package_source_fingerprint.clone()),
            informational: true,
        })
        .collect();
    let audit = SupplyAudit {
        state: AuditState::Passed,
        issue: None,
        validation_complete: true,
        lock_fingerprint: Some(source_fingerprint('5')?),
        snapshot_fingerprint: None,
        snapshot: None,
        snapshot_sequence: None,
        packages_total: 0,
        crates_io_scanned: 0,
        workspace_packages_excluded: 0,
        unsupported_packages: 0,
        findings: audit_findings,
        findings_omitted: 0,
    };
    let deny = SupplyDeny {
        complete: true,
        policy_state: SecurityPolicyState::Satisfied,
        findings,
        findings_omitted: 0,
        policy_fingerprint: source_fingerprint('6')?,
        execution_fingerprint: execution.clone(),
    };
    let mut stages = vec![
        validation(QualityV2StageKind::Format, &execution),
        validation(QualityV2StageKind::Check, &execution),
        validation(QualityV2StageKind::Clippy, &execution),
        validation(QualityV2StageKind::Test, &execution),
        QualityV2Stage {
            stage: QualityV2StageKind::Audit,
            status: ToolStatus::Passed,
            issue: None,
            duration_ms: 1,
            execution_fingerprint: Some(execution.clone()),
            details: Some(QualityV2Details::Audit { observation: audit }),
        },
        QualityV2Stage {
            stage: QualityV2StageKind::Deny,
            status: ToolStatus::Passed,
            issue: None,
            duration_ms: 1,
            execution_fingerprint: Some(execution.clone()),
            details: Some(QualityV2Details::Deny { observation: deny }),
        },
        QualityV2Stage {
            stage: QualityV2StageKind::Coverage,
            status: ToolStatus::Passed,
            issue: None,
            duration_ms: 1,
            execution_fingerprint: Some(execution.clone()),
            details: Some(QualityV2Details::Coverage {
                aggregate: CoverageMetrics::new((10, 10), (10, 10), (1, 1))?,
                parse_complete: true,
                exit_code: Some(0),
                doctests_run: false,
            }),
        },
    ];
    let mut report = QualityV2Report {
        profile: QualityV2Profile::Strict,
        mutation_requested: false,
        source_fingerprint: source_fingerprint('7')?,
        baseline: None,
        stages: std::mem::take(&mut stages),
        complete: false,
        status: ToolStatus::Blocked,
    };
    report.refresh();
    assert!(report.validate());
    assert!(report.complete);
    assert_eq!(report.status, ToolStatus::Passed);
    Ok(QualityV2Observation {
        report,
        runtime: runtime()?,
        execution_fingerprint: execution_fingerprint('8')?,
    })
}

#[test]
fn encode_result_trims_large_valid_report_without_preserving_pass() -> TestResult {
    let tool = QualityV2Tool::new()?;
    let reference: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let observation = large_complete_observation()?;
    let descriptor = descriptor()?;
    let untrimmed = tool.contract.encode(Output {
        outcome: Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(Data {
                project_ref: reference.to_string(),
                semantics: "complete_required_quality_stages_over_one_capture",
                observation: observation.clone(),
                artifacts: vec![Artifact {
                    uri: format!(
                        "rust-quality-artifact://{reference}/{}?offset=0&length={}",
                        descriptor.artifact_id,
                        descriptor.size_bytes.min(320 * 1024)
                    ),
                    sha256: super::super::resources::hex(&descriptor.sha256),
                    size_bytes: descriptor.size_bytes,
                    completeness: descriptor.completeness,
                }],
            }),
        },
        summary: "Recorded extended quality facts with independent source coverage",
        duration_ms: 7,
    })?;
    assert!(serde_json::to_vec(&untrimmed)?.len() > MAX_RESULT_BYTES);

    let encoded = tool.encode_result(
        &reference,
        PublishedQualityV2 {
            observation,
            artifact: descriptor,
        },
        7,
    )?;
    let wire = serde_json::to_vec(&encoded)?;
    assert!(wire.len() <= MAX_RESULT_BYTES, "{}", wire.len());
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "blocked");
    assert_ne!(value["status"], "passed");
    assert_eq!(value["error_code"], "EVIDENCE_INCOMPLETE");
    assert_eq!(value["data"]["observation"]["report"]["complete"], false);
    assert_ne!(value["data"]["observation"]["report"]["status"], "passed");
    let deny = &value["data"]["observation"]["report"]["stages"][5]["details"]["observation"];
    assert_eq!(deny["complete"], false);
    assert!(
        deny["findings_omitted"]
            .as_u64()
            .ok_or("findings_omitted")?
            > 0
    );
    Ok(())
}
