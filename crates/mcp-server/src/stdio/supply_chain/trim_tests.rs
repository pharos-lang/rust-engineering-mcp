use super::*;
use rust_engineering_application::supply_chain::PublishedSupply;
use rust_engineering_domain::security::SecurityPolicyState;
use rust_engineering_domain::supply_chain::{
    SupplyAudit, SupplyAvailability, SupplyCatalog, SupplyDeny, SupplyPackage, SupplyReport,
    SupplySource, YankedFact,
};
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    AuditState, CatalogFingerprint, Clock, ExecutionFingerprint, FreshnessPolicy,
    GuestArtifactName, IntegrityStatus, PayloadFormatVersion, PluginIdentity, Provenance,
    QualityArtifactDescriptor, QualityArtifactDraft, QualityArtifactId, QualityArtifactKind,
    QualityJobId, QualityMimeType, RuntimeIdentity, SnapshotEvidence, SourceFingerprint,
    SourceKind, UnixSeconds, UtcInstant,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const MAX_RESULT_BYTES: usize = 512 * 1024;

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(102)
    }
}

fn source_fingerprint(digit: char) -> TestResult<SourceFingerprint> {
    Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
}

fn execution_fingerprint(digit: char) -> TestResult<ExecutionFingerprint> {
    Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
}

fn catalog_fingerprint(digit: char) -> TestResult<CatalogFingerprint> {
    Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
}

fn snapshot(kind: SourceKind, id: &str) -> TestResult<SnapshotEvidence> {
    Ok(SnapshotEvidence::assess(
        Provenance::new(
            kind,
            id.parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(101)),
            IntegrityStatus::Verified,
            false,
        )?,
        FreshnessPolicy::new("supply-trim-test-v1".parse()?, 60, 300)?,
        &FixedClock,
    ))
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

fn large_complete_observation() -> TestResult<SupplyObservation> {
    let feature = format!("feature_{}", "x".repeat(55));
    let packages = (0..128)
        .map(|index| SupplyPackage {
            name: format!("package_{index:03}"),
            version: "1.0.0".into(),
            source: SupplySource::Workspace,
            source_fingerprint: None,
            declared_checksum: None,
            checksum_verified: false,
            duplicate_name: false,
            declared_features: Some(vec![feature.clone(); 40]),
            active_features: Some(vec![feature.clone(); 40]),
            yanked: YankedFact::NotApplicable,
        })
        .collect::<Vec<_>>();
    let report = SupplyReport {
        source_fingerprint: source_fingerprint('4')?,
        lock_fingerprint: source_fingerprint('5')?,
        packages,
        packages_total: 128,
        packages_omitted: 0,
        audit_availability: SupplyAvailability::Available,
        audit: Some(SupplyAudit {
            state: AuditState::Passed,
            issue: None,
            validation_complete: true,
            lock_fingerprint: Some(source_fingerprint('5')?),
            snapshot_fingerprint: Some(catalog_fingerprint('9')?),
            snapshot: Some(snapshot(
                SourceKind::RustsecSnapshot,
                "rustsec-supply-trim-v1",
            )?),
            snapshot_sequence: Some(1),
            packages_total: 128,
            crates_io_scanned: 0,
            workspace_packages_excluded: 128,
            unsupported_packages: 0,
            findings: vec![],
            findings_omitted: 0,
        }),
        deny_availability: SupplyAvailability::Available,
        deny: Some(SupplyDeny {
            complete: true,
            policy_state: SecurityPolicyState::Satisfied,
            findings: vec![],
            findings_omitted: 0,
            policy_fingerprint: source_fingerprint('6')?,
            execution_fingerprint: execution_fingerprint('7')?,
        }),
        catalog: SupplyCatalog {
            availability: SupplyAvailability::Available,
            snapshot_fingerprint: Some(catalog_fingerprint('9')?),
            bundle_fingerprint: Some(source_fingerprint('9')?),
            sequence: Some(1),
            evidence: Some(snapshot(
                SourceKind::RegistrySnapshot,
                "registry-supply-trim-v1",
            )?),
            lookups: 0,
        },
        complete: true,
    };
    assert!(report.validate());
    Ok(SupplyObservation {
        report,
        runtime: runtime()?,
        execution_fingerprint: execution_fingerprint('8')?,
    })
}

#[test]
fn encode_result_trims_large_valid_report_within_complete_wire_budget() -> TestResult {
    let tool = SupplyTool::new()?;
    let reference: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let observation = large_complete_observation()?;
    let descriptor = descriptor()?;
    assert!(observation.report.complete);
    assert_eq!(observation.report.packages_omitted, 0);
    let mut missing_audit = observation.report.clone();
    missing_audit.audit = None;
    assert!(!missing_audit.validate());
    let mut missing_catalog_evidence = observation.report.clone();
    missing_catalog_evidence.catalog.evidence = None;
    assert!(!missing_catalog_evidence.validate());
    let untrimmed = tool.contract.encode(Output {
        outcome: Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(Data {
                project_ref: reference.to_string(),
                semantics: "recorded_facts_not_a_security_score_or_legal_approval",
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
        summary: "Recorded supply chain facts with independent source coverage",
        duration_ms: 7,
    })?;
    assert!(serde_json::to_vec(&untrimmed)?.len() > MAX_RESULT_BYTES);

    let encoded = tool.encode_result(
        &reference,
        PublishedSupply {
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
    assert!(
        value["data"]["observation"]["report"]["packages_omitted"]
            .as_u64()
            .ok_or("packages_omitted")?
            > 0
    );
    Ok(())
}

#[test]
fn encode_result_keeps_small_complete_evidence_passed() -> TestResult {
    let tool = SupplyTool::new()?;
    let reference: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let mut observation = large_complete_observation()?;
    observation.report.packages.truncate(1);
    observation.report.packages_total = 1;
    if let Some(audit) = observation.report.audit.as_mut() {
        audit.packages_total = 1;
        audit.workspace_packages_excluded = 1;
    }
    let encoded = tool.encode_result(
        &reference,
        PublishedSupply {
            observation,
            artifact: descriptor()?,
        },
        7,
    )?;
    let value = encoded.structured_content.ok_or("content")?;
    assert_eq!(value["status"], "passed");
    assert_eq!(value["data"]["observation"]["report"]["complete"], true);
    Ok(())
}
