use rust_engineering_domain::{
    ExecutionFingerprint, RuntimeIdentity, SourceFingerprint, ToolStatus,
    miri::{MiriCategory, MiriCounts, MiriFinding, MiriObservation, MiriReport},
    quality_v2::{QualityV2Observation, QualityV2Profile, QualityV2Report, QualityV2Stage},
    supply_chain::{
        SupplyAvailability, SupplyCatalog, SupplyObservation, SupplyPackage, SupplyReport,
        SupplySource, YankedFact,
    },
    unsafe_scan::{
        UnsafeCoverage, UnsafeFinding, UnsafeKind, UnsafeObservation, UnsafeOrigin,
        UnsafeScanReport,
    },
};
use rust_engineering_execution::{
    safe_gate_v2_log, safe_miri_log, safe_supply_log, safe_unsafe_log, security_runtime_inventory,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn source(value: char) -> Result<SourceFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{}", value.to_string().repeat(64)).parse()?)
}

fn execution(value: char) -> Result<ExecutionFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{}", value.to_string().repeat(64)).parse()?)
}

fn runtime() -> Result<RuntimeIdentity, Box<dyn std::error::Error>> {
    Ok(RuntimeIdentity {
        platform: "linux/aarch64".into(),
        image_id: rust_engineering_execution::APPROVED_M4_IMAGE.into(),
        configuration_fingerprint: execution('1')?,
        execution_fingerprint: execution('2')?,
        rust_version: "1.98.1".into(),
        cargo_version: "1.98.1".into(),
        declared_toolchain: None,
    })
}

#[test]
fn runtime_inventory_is_passive_complete_and_pinned() -> TestResult {
    let value = serde_json::to_value(security_runtime_inventory())?;
    assert_eq!(value["operation"], "security_runtime_inventory");
    assert_eq!(value["installation_observed"], false);
    assert_eq!(
        value["image_id"],
        rust_engineering_execution::APPROVED_M4_IMAGE
    );
    assert_eq!(value["cargo_deny"]["version"], "0.19.7");
    assert_eq!(value["nightly"], "nightly-2026-09-07");
    assert_eq!(value["target"], "aarch64-unknown-linux-gnu");
    Ok(())
}

#[test]
fn miri_log_is_valid_json_and_trims_whole_findings() -> TestResult {
    let finding = MiriFinding {
        category: MiriCategory::TestFailure,
        test_name: Some("x".repeat(4_096)),
        test_binary: Some("fixture".into()),
    };
    let mut observation = MiriObservation {
        report: MiriReport {
            counts: MiriCounts {
                tests: 128,
                failed: 128,
                test_failures: 128,
                ..Default::default()
            },
            findings: vec![finding; 128],
            findings_omitted: 0,
            complete: true,
            clean: false,
            junit_present: true,
            exit_code: Some(101),
        },
        source_fingerprint: source('3')?,
        vendor_fingerprint: source('4')?,
        metadata_fingerprint: source('5')?,
        config_fingerprint: source('6')?,
        junit_fingerprint: Some(source('7')?),
        runtime: runtime()?,
        execution_fingerprint: execution('8')?,
        nightly_commit: "5a2be9f5f075d31e3ca5526b5b029881ce441253".into(),
        sysroot_fingerprint: source('9')?,
    };
    let log = safe_miri_log(&observation).map_err(|error| format!("{error:?}"))?;
    assert!(log.bytes.len() <= 256 * 1024);
    assert!(log.findings_removed > 0);
    let value: serde_json::Value = serde_json::from_slice(&log.bytes)?;
    assert_eq!(value["report"]["complete"], false);
    observation.report.clean = true;
    assert!(safe_miri_log(&observation).is_err());
    Ok(())
}

#[test]
fn unsafe_log_is_valid_json_and_marks_trimmed_syntax_incomplete() -> TestResult {
    let finding = UnsafeFinding {
        path: "x".repeat(4_096),
        origin: UnsafeOrigin::Workspace,
        package: None,
        file_fingerprint: source('a')?,
        kind: UnsafeKind::UnsafeBlock,
        byte_start: 0,
        byte_end: 6,
        line: 1,
        column: 1,
        conditional: false,
    };
    let mut observation = UnsafeObservation {
        report: UnsafeScanReport {
            coverage: UnsafeCoverage {
                files_total: 1,
                files_selected: 1,
                files_parsed: 1,
                workspace_files: 1,
                source_bytes: 6,
                ..Default::default()
            },
            findings: vec![finding; 128],
            findings_total: 128,
            findings_omitted: 0,
            syntax_complete: true,
            cfg_evaluated: false,
            macros_expanded: false,
            generated_sources_scanned: false,
        },
        source_fingerprint: source('b')?,
        vendor_fingerprint: source('c')?,
        vendor_archive_fingerprint: source('d')?,
        metadata_fingerprint: source('e')?,
        manifest_fingerprint: source('f')?,
        runtime: runtime()?,
        execution_fingerprint: execution('3')?,
    };
    let log = safe_unsafe_log(&observation).map_err(|error| format!("{error:?}"))?;
    assert!(log.bytes.len() <= 256 * 1024);
    assert!(log.findings_removed > 0);
    let value: serde_json::Value = serde_json::from_slice(&log.bytes)?;
    assert_eq!(value["report"]["syntax_complete"], false);
    observation.report.cfg_evaluated = true;
    assert!(safe_unsafe_log(&observation).is_err());
    Ok(())
}

#[test]
fn supply_and_quality_logs_validate_before_serializing() -> TestResult {
    let package = SupplyPackage {
        name: "x".repeat(4_096),
        version: "1.0.0".into(),
        source: SupplySource::CratesIo,
        source_fingerprint: Some(source('4')?),
        declared_checksum: Some(source('5')?),
        checksum_verified: true,
        duplicate_name: false,
        declared_features: Some(vec![]),
        active_features: Some(vec![]),
        yanked: YankedFact::NotConsulted,
    };
    let mut supply = SupplyObservation {
        report: SupplyReport {
            source_fingerprint: source('6')?,
            lock_fingerprint: source('7')?,
            packages: vec![package; 128],
            packages_total: 128,
            packages_omitted: 0,
            audit_availability: SupplyAvailability::NotConfigured,
            audit: None,
            deny_availability: SupplyAvailability::NotConfigured,
            deny: None,
            catalog: SupplyCatalog {
                availability: SupplyAvailability::NotConfigured,
                snapshot_fingerprint: None,
                bundle_fingerprint: None,
                sequence: None,
                evidence: None,
                lookups: 0,
            },
            complete: false,
        },
        runtime: runtime()?,
        execution_fingerprint: execution('8')?,
    };
    let log = safe_supply_log(&supply).map_err(|error| format!("{error:?}"))?;
    assert!(log.bytes.len() <= 256 * 1024);
    assert!(log.findings_removed > 0);
    supply.report.packages_total = 127;
    assert!(safe_supply_log(&supply).is_err());

    let stages = QualityV2Profile::Strict
        .stages(false)
        .into_iter()
        .map(|stage| QualityV2Stage {
            stage,
            status: ToolStatus::Blocked,
            issue: None,
            duration_ms: 1,
            execution_fingerprint: None,
            details: None,
        })
        .collect();
    let mut quality = QualityV2Observation {
        report: QualityV2Report {
            profile: QualityV2Profile::Strict,
            mutation_requested: false,
            source_fingerprint: source('9')?,
            baseline: None,
            stages,
            complete: false,
            status: ToolStatus::Blocked,
        },
        runtime: runtime()?,
        execution_fingerprint: execution('a')?,
    };
    let log = safe_gate_v2_log(&quality).map_err(|error| format!("{error:?}"))?;
    assert_eq!(log.findings_removed, 0);
    assert!(serde_json::from_slice::<serde_json::Value>(&log.bytes).is_ok());
    quality.report.status = ToolStatus::Passed;
    assert!(safe_gate_v2_log(&quality).is_err());
    Ok(())
}
