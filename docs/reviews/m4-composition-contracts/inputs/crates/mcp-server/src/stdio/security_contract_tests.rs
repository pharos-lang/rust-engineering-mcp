use super::{
    miri::MiriTool, quality_v2::QualityV2Tool, supply_chain::SupplyTool, unsafe_scan::UnsafeTool,
};
use rmcp::model::{CallToolResult, Tool};
use rust_engineering_domain::{
    AuditSource, AuditState, CatalogFingerprint, Clock, ExecutionFingerprint, ExecutionTermination,
    FreshnessPolicy, IntegrityStatus, Provenance, QualityIssue, RuntimeIdentity, SnapshotEvidence,
    SourceFingerprint, SourceKind, ToolStatus, UnixSeconds,
    miri::{MiriCategory, MiriCounts, MiriFinding, MiriObservation, MiriReport},
    quality_v2::{
        QualityV2Details, QualityV2Observation, QualityV2Profile, QualityV2Report, QualityV2Stage,
        QualityV2StageKind,
    },
    security::{
        FindingDisposition, SecurityEngine, SecurityFinding, SecurityPackage, SecurityPolicyState,
        SecuritySeverity, SecuritySource,
    },
    supply_chain::{
        SupplyAdvisory, SupplyAudit, SupplyAvailability, SupplyCatalog, SupplyDeny,
        SupplyObservation, SupplyPackage, SupplyReport, SupplySource, YankedFact,
    },
    unsafe_scan::{
        UnsafeCoverage, UnsafeFinding, UnsafeKind, UnsafeObservation, UnsafeOrigin,
        UnsafeScanReport,
    },
};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;
const MAX_RESULT_BYTES: usize = 512 * 1024;
const PROJECT_REF: &str = "prj_00000000000000000000000000000001";

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(102)
    }
}

fn source_fingerprint(digit: u8) -> Result<SourceFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
}

fn execution_fingerprint(digit: u8) -> Result<ExecutionFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
}

fn catalog_fingerprint(digit: u8) -> Result<CatalogFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
}

fn runtime() -> Result<RuntimeIdentity, Box<dyn std::error::Error>> {
    Ok(RuntimeIdentity {
        platform: "linux/aarch64".into(),
        image_id: format!("sha256:{}", "1".repeat(64)),
        configuration_fingerprint: execution_fingerprint(2)?,
        execution_fingerprint: execution_fingerprint(3)?,
        rust_version: "1.98.1".into(),
        cargo_version: "1.98.1".into(),
        declared_toolchain: Some("1.98".into()),
    })
}

fn snapshot(kind: SourceKind, id: &str) -> Result<SnapshotEvidence, Box<dyn std::error::Error>> {
    Ok(SnapshotEvidence::assess(
        Provenance::new(
            kind,
            id.parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(101)),
            IntegrityStatus::Verified,
            false,
        )?,
        FreshnessPolicy::new("mcp-security-v1".parse()?, 60, 300)?,
        &FixedClock,
    ))
}

fn artifact() -> Value {
    json!({
        "uri": format!(
            "rust-quality-artifact://{PROJECT_REF}/art_00000000000000000000000000000001?offset=0&length=512"
        ),
        "sha256": "4".repeat(64),
        "size_bytes": 512,
        "completeness": "complete"
    })
}

fn passed(data: Value, summary: &str) -> Value {
    json!({
        "status": "passed",
        "error_code": null,
        "error_message": null,
        "data": data,
        "summary": summary,
        "duration_ms": 7
    })
}

fn failed(data: Value, code: &str, message: &str, summary: &str) -> Value {
    json!({
        "status": "failed",
        "error_code": code,
        "error_message": message,
        "data": data,
        "summary": summary,
        "duration_ms": 7
    })
}

fn blocked(data: Value, code: &str, message: &str, summary: &str) -> Value {
    json!({
        "status": "blocked",
        "error_code": code,
        "error_message": message,
        "data": data,
        "summary": summary,
        "duration_ms": 7
    })
}

fn data(semantics: &str, observation: Value) -> Value {
    json!({
        "project_ref": PROJECT_REF,
        "semantics": semantics,
        "observation": observation,
        "artifacts": [artifact()]
    })
}

fn output_schema(tool: Tool) -> Result<Value, Box<dyn std::error::Error>> {
    let wire = serde_json::to_value(tool)?;
    wire.get("outputSchema")
        .cloned()
        .ok_or_else(|| "tool has no output schema".into())
}

fn assert_valid(schema: &Value, value: &Value) -> TestResult {
    let validator = jsonschema::validator_for(schema)?;
    if !validator.is_valid(value) {
        return Err("fixture does not satisfy advertised output schema".into());
    }
    Ok(())
}

fn assert_invalid(schema: &Value, value: &Value) -> TestResult {
    let validator = jsonschema::validator_for(schema)?;
    if validator.is_valid(value) {
        return Err("adversarial fixture unexpectedly satisfies advertised output schema".into());
    }
    Ok(())
}

fn assert_mirrored_and_bounded(value: &Value) -> TestResult {
    let operational_error = matches!(
        value["status"].as_str(),
        Some("blocked" | "unavailable" | "cancelled")
    );
    let result = if operational_error {
        CallToolResult::structured_error(value.clone())
    } else {
        CallToolResult::structured(value.clone())
    };
    let wire = serde_json::to_value(&result)?;
    let text = wire["content"][0]["text"]
        .as_str()
        .ok_or("structured result has no text mirror")?;
    assert_eq!(
        serde_json::from_str::<Value>(text)?,
        wire["structuredContent"]
    );
    assert_eq!(wire["structuredContent"], *value);
    assert_eq!(wire["isError"], operational_error);
    assert!(serde_json::to_vec(&result)?.len() <= MAX_RESULT_BYTES);
    Ok(())
}

fn unsafe_value() -> Result<Value, Box<dyn std::error::Error>> {
    let report = UnsafeScanReport {
        coverage: UnsafeCoverage {
            files_total: 2,
            files_selected: 2,
            files_omitted: 0,
            files_parsed: 2,
            files_parse_error: 0,
            files_crashed: 0,
            files_timed_out: 0,
            files_unavailable: 0,
            files_invalid_utf8: 0,
            files_too_large: 0,
            files_budget_exhausted: 0,
            workspace_files: 2,
            dependency_files: 0,
            workspace_unowned_files: 0,
            source_bytes: 96,
            macro_boundaries_omitted: 0,
            opaque_syntax_omitted: 0,
        },
        findings: vec![
            UnsafeFinding {
                path: "src/trait.rs".into(),
                origin: UnsafeOrigin::Workspace,
                package: None,
                file_fingerprint: source_fingerprint(5)?,
                kind: UnsafeKind::UnsafeTrait,
                byte_start: 0,
                byte_end: 6,
                line: 1,
                column: 1,
                conditional: false,
            },
            UnsafeFinding {
                path: "src/static.rs".into(),
                origin: UnsafeOrigin::Workspace,
                package: None,
                file_fingerprint: source_fingerprint(6)?,
                kind: UnsafeKind::UnsafeStatic,
                byte_start: 0,
                byte_end: 6,
                line: 1,
                column: 1,
                conditional: true,
            },
        ],
        findings_total: 2,
        findings_omitted: 0,
        syntax_complete: true,
        cfg_evaluated: false,
        macros_expanded: false,
        generated_sources_scanned: false,
    };
    assert!(report.validate());
    let observation = UnsafeObservation {
        report,
        source_fingerprint: source_fingerprint(7)?,
        vendor_fingerprint: source_fingerprint(8)?,
        vendor_archive_fingerprint: source_fingerprint(9)?,
        metadata_fingerprint: source_fingerprint(1)?,
        manifest_fingerprint: source_fingerprint(2)?,
        runtime: runtime()?,
        execution_fingerprint: execution_fingerprint(3)?,
    };
    Ok(passed(
        data(
            "syntactic_evidence_only_not_memory_safety",
            serde_json::to_value(observation)?,
        ),
        "Syntactic unsafe and extern evidence; no inference of memory safety",
    ))
}

fn miri_value(complete: bool) -> Result<Value, Box<dyn std::error::Error>> {
    let (report, status) = if complete {
        (
            MiriReport {
                counts: MiriCounts {
                    tests: 4,
                    passed: 0,
                    failed: 4,
                    skipped: 0,
                    undefined_behavior: 1,
                    unsupported_operation: 1,
                    test_failures: 1,
                    compile_failures: 1,
                    timeouts: 0,
                    unclassified: 0,
                },
                findings: [
                    MiriCategory::UndefinedBehavior,
                    MiriCategory::UnsupportedOperation,
                    MiriCategory::TestFailure,
                    MiriCategory::CompileFailure,
                ]
                .into_iter()
                .enumerate()
                .map(|(index, category)| MiriFinding {
                    category,
                    test_name: Some(format!("fixture::case_{index}")),
                    test_binary: Some("fixture-tests".into()),
                })
                .collect(),
                findings_omitted: 0,
                complete: true,
                clean: false,
                junit_present: true,
                exit_code: Some(101),
            },
            "failed",
        )
    } else {
        (
            MiriReport {
                counts: MiriCounts {
                    timeouts: 1,
                    unclassified: 1,
                    ..Default::default()
                },
                findings: vec![
                    MiriFinding {
                        category: MiriCategory::Timeout,
                        test_name: None,
                        test_binary: None,
                    },
                    MiriFinding {
                        category: MiriCategory::Unclassified,
                        test_name: None,
                        test_binary: None,
                    },
                ],
                findings_omitted: 0,
                complete: false,
                clean: false,
                junit_present: false,
                exit_code: None,
            },
            "blocked",
        )
    };
    assert!(report.validate());
    let observation = MiriObservation {
        report,
        source_fingerprint: source_fingerprint(5)?,
        vendor_fingerprint: source_fingerprint(6)?,
        metadata_fingerprint: source_fingerprint(7)?,
        config_fingerprint: source_fingerprint(8)?,
        junit_fingerprint: complete.then(|| source_fingerprint(9)).transpose()?,
        runtime: runtime()?,
        execution_fingerprint: execution_fingerprint(3)?,
        nightly_commit: "5a2be9f5f075d31e3ca5526b5b029881ce441253".into(),
        sysroot_fingerprint: source_fingerprint(4)?,
    };
    let result_data = data(
        "observed_interpreter_evidence_not_a_proof_of_memory_safety",
        serde_json::to_value(observation)?,
    );
    Ok(if status == "failed" {
        failed(
            result_data,
            "OBSERVED_FAILURE",
            "Interpreter or compilation failure observed; inspect categories and coverage",
            "Observed interpreter evidence; no proof of universal memory safety",
        )
    } else {
        blocked(
            result_data,
            "EVIDENCE_INCOMPLETE",
            "Interpreter evidence is partial",
            "Observed interpreter evidence; no proof of universal memory safety",
        )
    })
}

fn supply_value(complete: bool) -> Result<Value, Box<dyn std::error::Error>> {
    let package = SupplyPackage {
        name: "unicode-ident".into(),
        version: "1.0.24".into(),
        source: SupplySource::CratesIo,
        source_fingerprint: Some(source_fingerprint(5)?),
        declared_checksum: Some(source_fingerprint(6)?),
        checksum_verified: true,
        duplicate_name: false,
        declared_features: Some(vec![]),
        active_features: Some(vec![]),
        yanked: YankedFact::NotYanked,
    };
    let rustsec = snapshot(SourceKind::RustsecSnapshot, "rustsec-fixture-v1")?;
    let catalog = snapshot(SourceKind::RegistrySnapshot, "registry-fixture-v1")?;
    let audit = SupplyAudit {
        state: AuditState::Passed,
        issue: None,
        validation_complete: true,
        lock_fingerprint: Some(source_fingerprint(8)?),
        snapshot_fingerprint: Some(catalog_fingerprint(9)?),
        snapshot: Some(rustsec),
        snapshot_sequence: Some(4),
        packages_total: 1,
        crates_io_scanned: 1,
        workspace_packages_excluded: 0,
        unsupported_packages: 0,
        findings: vec![SupplyAdvisory {
            advisory_id: "RUSTSEC-2026-0001".into(),
            package_name: "unicode-ident".into(),
            package_version: "1.0.24".into(),
            package_source: AuditSource::CratesIo,
            source_fingerprint: Some(source_fingerprint(5)?),
            informational: true,
        }],
        findings_omitted: 0,
    };
    let deny = SupplyDeny {
        complete: true,
        policy_state: SecurityPolicyState::Satisfied,
        findings: vec![SecurityFinding {
            engine: SecurityEngine::Licenses,
            rule: "license-allowlist".into(),
            package: Some(SecurityPackage {
                name: "unicode-ident".into(),
                version: "1.0.24".into(),
                source: SecuritySource::CratesIo,
                source_fingerprint: Some(source_fingerprint(5)?),
            }),
            severity: SecuritySeverity::Note,
            message: "License evidence recorded".into(),
            disposition: FindingDisposition::Active,
        }],
        findings_omitted: 0,
        policy_fingerprint: source_fingerprint(1)?,
        execution_fingerprint: execution_fingerprint(3)?,
    };
    let report = SupplyReport {
        source_fingerprint: source_fingerprint(7)?,
        lock_fingerprint: source_fingerprint(8)?,
        packages: vec![package],
        packages_total: 1,
        packages_omitted: 0,
        audit_availability: SupplyAvailability::Available,
        audit: Some(audit),
        deny_availability: SupplyAvailability::Available,
        deny: Some(deny),
        catalog: SupplyCatalog {
            availability: if complete {
                SupplyAvailability::Available
            } else {
                SupplyAvailability::Partial
            },
            snapshot_fingerprint: Some(catalog_fingerprint(9)?),
            bundle_fingerprint: Some(source_fingerprint(9)?),
            sequence: Some(4),
            evidence: Some(catalog),
            lookups: 1,
        },
        complete,
    };
    assert!(report.validate());
    let observation = SupplyObservation {
        report,
        runtime: runtime()?,
        execution_fingerprint: execution_fingerprint(3)?,
    };
    let result_data = data(
        "recorded_facts_not_a_security_score_or_legal_approval",
        serde_json::to_value(observation)?,
    );
    Ok(if complete {
        passed(
            result_data,
            "Recorded supply chain facts with independent source coverage",
        )
    } else {
        blocked(
            result_data,
            "EVIDENCE_INCOMPLETE",
            "Supply chain evidence is partial; inspect independent sources",
            "Recorded supply chain facts with independent source coverage",
        )
    })
}

fn quality_v2_value(complete: bool) -> Result<Value, Box<dyn std::error::Error>> {
    let execution = execution_fingerprint(3)?;
    let validation = |stage| QualityV2Stage {
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
            affected_files: (stage == QualityV2StageKind::Format).then_some(0),
            build_succeeded: (stage == QualityV2StageKind::Test).then_some(true),
        }),
    };
    let audit = SupplyAudit {
        state: AuditState::Passed,
        issue: None,
        validation_complete: true,
        lock_fingerprint: Some(source_fingerprint(8)?),
        snapshot_fingerprint: Some(catalog_fingerprint(9)?),
        snapshot: Some(snapshot(SourceKind::RustsecSnapshot, "rustsec-quality-v2")?),
        snapshot_sequence: Some(4),
        packages_total: 1,
        crates_io_scanned: 1,
        workspace_packages_excluded: 0,
        unsupported_packages: 0,
        findings: vec![],
        findings_omitted: 0,
    };
    let deny = SupplyDeny {
        complete: true,
        policy_state: SecurityPolicyState::Satisfied,
        findings: vec![],
        findings_omitted: 0,
        policy_fingerprint: source_fingerprint(1)?,
        execution_fingerprint: execution_fingerprint(3)?,
    };
    let mut stages = vec![
        validation(QualityV2StageKind::Format),
        validation(QualityV2StageKind::Check),
        validation(QualityV2StageKind::Clippy),
        validation(QualityV2StageKind::Test),
        QualityV2Stage {
            stage: QualityV2StageKind::Audit,
            status: ToolStatus::Passed,
            issue: None,
            duration_ms: 1,
            execution_fingerprint: Some(execution_fingerprint(3)?),
            details: Some(QualityV2Details::Audit { observation: audit }),
        },
        QualityV2Stage {
            stage: QualityV2StageKind::Deny,
            status: ToolStatus::Passed,
            issue: None,
            duration_ms: 1,
            execution_fingerprint: Some(execution_fingerprint(3)?),
            details: Some(QualityV2Details::Deny { observation: deny }),
        },
        QualityV2Stage {
            stage: QualityV2StageKind::Coverage,
            status: ToolStatus::Passed,
            issue: None,
            duration_ms: 1,
            execution_fingerprint: Some(execution_fingerprint(3)?),
            details: Some(QualityV2Details::Coverage {
                aggregate: rust_engineering_domain::coverage::CoverageMetrics::new(
                    (10, 10),
                    (10, 10),
                    (1, 1),
                )?,
                parse_complete: true,
                exit_code: Some(0),
                doctests_run: false,
            }),
        },
    ];
    if !complete {
        let coverage = stages.last_mut().ok_or("coverage stage")?;
        coverage.status = ToolStatus::Blocked;
        coverage.issue = Some(QualityIssue::Incomplete);
        coverage.details = Some(QualityV2Details::Coverage {
            aggregate: rust_engineering_domain::coverage::CoverageMetrics::new(
                (10, 9),
                (10, 9),
                (1, 1),
            )?,
            parse_complete: false,
            exit_code: Some(0),
            doctests_run: false,
        });
    }
    let mut report = QualityV2Report {
        profile: QualityV2Profile::Strict,
        mutation_requested: false,
        source_fingerprint: source_fingerprint(7)?,
        baseline: None,
        stages,
        complete: false,
        status: ToolStatus::Blocked,
    };
    report.refresh();
    assert!(report.validate());
    assert_eq!(report.complete, complete);
    let observation = QualityV2Observation {
        report,
        runtime: runtime()?,
        execution_fingerprint: execution_fingerprint(3)?,
    };
    let result_data = data(
        "complete_required_quality_stages_over_one_capture",
        serde_json::to_value(observation)?,
    );
    Ok(if complete {
        passed(
            result_data,
            "Recorded extended quality facts with independent source coverage",
        )
    } else {
        blocked(
            result_data,
            "EVIDENCE_INCOMPLETE",
            "Extended quality evidence is partial; inspect independent sources",
            "Recorded extended quality facts with independent source coverage",
        )
    })
}

#[test]
fn real_domain_outputs_validate_and_mirror_within_the_fixed_budget() -> TestResult {
    let fixtures = [
        (
            output_schema(UnsafeTool::new()?.definition)?,
            unsafe_value()?,
        ),
        (
            output_schema(MiriTool::new()?.definition)?,
            miri_value(true)?,
        ),
        (
            output_schema(MiriTool::new()?.definition)?,
            miri_value(false)?,
        ),
        (
            output_schema(SupplyTool::new()?.definition)?,
            supply_value(true)?,
        ),
        (
            output_schema(SupplyTool::new()?.definition)?,
            supply_value(false)?,
        ),
        (
            output_schema(QualityV2Tool::new()?.definition)?,
            quality_v2_value(true)?,
        ),
        (
            output_schema(QualityV2Tool::new()?.definition)?,
            quality_v2_value(false)?,
        ),
    ];
    for (schema, value) in fixtures {
        assert_valid(&schema, &value)?;
        assert_mirrored_and_bounded(&value)?;
    }
    Ok(())
}

#[test]
fn unsafe_schema_covers_new_coverage_fields_kinds_closure_and_finding_budget() -> TestResult {
    let schema = output_schema(UnsafeTool::new()?.definition)?;
    let value = unsafe_value()?;
    assert_eq!(
        value["data"]["observation"]["report"]["findings"][0]["kind"],
        "unsafe_trait"
    );
    assert_eq!(
        value["data"]["observation"]["report"]["findings"][1]["kind"],
        "unsafe_static"
    );
    for field in [
        "files_invalid_utf8",
        "files_too_large",
        "files_budget_exhausted",
        "opaque_syntax_omitted",
    ] {
        assert!(
            value["data"]["observation"]["report"]["coverage"]
                .get(field)
                .is_some(),
            "missing {field}"
        );
    }

    let mut unknown = value.clone();
    unknown["data"]["observation"]["report"]["coverage"]
        .as_object_mut()
        .ok_or("coverage object")?
        .insert("forged_complete".into(), json!(true));
    assert_invalid(&schema, &unknown)?;

    let mut bad_kind = value.clone();
    bad_kind["data"]["observation"]["report"]["findings"][0]["kind"] = json!("unsafe_magic");
    assert_invalid(&schema, &bad_kind)?;

    let mut too_many_files = value.clone();
    too_many_files["data"]["observation"]["report"]["coverage"]["files_selected"] = json!(4097);
    assert_invalid(&schema, &too_many_files)?;

    let mut too_many = value;
    let finding = too_many["data"]["observation"]["report"]["findings"][0].clone();
    too_many["data"]["observation"]["report"]["findings"] = Value::Array(vec![finding; 129]);
    assert_invalid(&schema, &too_many)
}

#[test]
fn miri_schema_accepts_all_categories_and_rejects_unknowns_and_over_budget_findings() -> TestResult
{
    let schema = output_schema(MiriTool::new()?.definition)?;
    let complete = miri_value(true)?;
    let partial = miri_value(false)?;
    assert_valid(&schema, &complete)?;
    assert_valid(&schema, &partial)?;

    let mut categories = complete["data"]["observation"]["report"]["findings"]
        .as_array()
        .ok_or("findings array")?
        .iter()
        .map(|finding| finding["category"].clone())
        .collect::<Vec<_>>();
    categories.extend(
        partial["data"]["observation"]["report"]["findings"]
            .as_array()
            .ok_or("findings array")?
            .iter()
            .map(|finding| finding["category"].clone()),
    );
    assert_eq!(
        categories,
        [
            "undefined_behavior",
            "unsupported_operation",
            "test_failure",
            "compile_failure",
            "timeout",
            "unclassified",
        ]
    );

    let mut unknown_field = complete.clone();
    unknown_field["data"]["observation"]["report"]
        .as_object_mut()
        .ok_or("report object")?
        .insert("stdout_claimed_clean".into(), json!(true));
    assert_invalid(&schema, &unknown_field)?;

    let mut unknown_category = complete.clone();
    unknown_category["data"]["observation"]["report"]["findings"][0]["category"] =
        json!("forged_ub");
    assert_invalid(&schema, &unknown_category)?;

    let mut too_many = complete;
    let finding = too_many["data"]["observation"]["report"]["findings"][0].clone();
    too_many["data"]["observation"]["report"]["findings"] = Value::Array(vec![finding; 129]);
    assert_invalid(&schema, &too_many)
}

#[test]
fn supply_schema_keeps_rustsec_and_catalog_provenance_distinct_and_bounded() -> TestResult {
    let schema = output_schema(SupplyTool::new()?.definition)?;
    let value = supply_value(true)?;
    let audit_kind =
        &value["data"]["observation"]["report"]["audit"]["snapshot"]["provenance"]["source_kind"];
    let catalog_kind =
        &value["data"]["observation"]["report"]["catalog"]["evidence"]["provenance"]["source_kind"];
    assert_eq!(audit_kind, "rustsec_snapshot");
    assert_eq!(catalog_kind, "registry_snapshot");

    let mut swapped_audit = value.clone();
    swapped_audit["data"]["observation"]["report"]["audit"]["snapshot"]["provenance"]["source_kind"] =
        json!("registry_snapshot");
    assert_invalid(&schema, &swapped_audit)?;

    let mut swapped_catalog = value.clone();
    swapped_catalog["data"]["observation"]["report"]["catalog"]["evidence"]["provenance"]["source_kind"] =
        json!("rustsec_snapshot");
    assert_invalid(&schema, &swapped_catalog)?;

    let mut unknown = value.clone();
    unknown["data"]["observation"]["report"]["deny"]
        .as_object_mut()
        .ok_or("deny object")?
        .insert("score".into(), json!(100));
    assert_invalid(&schema, &unknown)?;

    let mut unknown_disposition = value.clone();
    unknown_disposition["data"]["observation"]["report"]["deny"]["findings"][0]["disposition"]
        .as_object_mut()
        .ok_or("disposition object")?
        .insert("forged_owner".into(), json!("attacker"));
    assert_invalid(&schema, &unknown_disposition)?;

    let mut too_many_packages = value.clone();
    let package = too_many_packages["data"]["observation"]["report"]["packages"][0].clone();
    too_many_packages["data"]["observation"]["report"]["packages"] =
        Value::Array(vec![package; 129]);
    assert_invalid(&schema, &too_many_packages)?;

    let mut too_many_total = value.clone();
    too_many_total["data"]["observation"]["report"]["packages_total"] = json!(4097);
    assert_invalid(&schema, &too_many_total)?;

    let mut too_many_lookups = value.clone();
    too_many_lookups["data"]["observation"]["report"]["catalog"]["lookups"] = json!(129);
    assert_invalid(&schema, &too_many_lookups)?;

    let mut too_many_audit = value.clone();
    let advisory = too_many_audit["data"]["observation"]["report"]["audit"]["findings"][0].clone();
    too_many_audit["data"]["observation"]["report"]["audit"]["findings"] =
        Value::Array(vec![advisory; 129]);
    assert_invalid(&schema, &too_many_audit)?;

    let mut too_many_deny = value;
    let finding = too_many_deny["data"]["observation"]["report"]["deny"]["findings"][0].clone();
    too_many_deny["data"]["observation"]["report"]["deny"]["findings"] =
        Value::Array(vec![finding; 129]);
    assert_invalid(&schema, &too_many_deny)
}

#[test]
fn quality_v2_schema_is_closed_and_preserves_complete_and_partial_results() -> TestResult {
    let definition = serde_json::to_value(QualityV2Tool::new()?.definition)?;
    assert_eq!(definition["inputSchema"]["additionalProperties"], false);
    assert!(
        definition["outputSchema"]["additionalProperties"] == false
            || definition["outputSchema"]["unevaluatedProperties"] == false
    );
    let schema = definition["outputSchema"].clone();
    let complete = quality_v2_value(true)?;
    let partial = quality_v2_value(false)?;
    assert_valid(&schema, &complete)?;
    assert_valid(&schema, &partial)?;
    assert_eq!(complete["status"], "passed");
    assert_eq!(complete["data"]["observation"]["report"]["complete"], true);
    assert_eq!(partial["status"], "blocked");
    assert_eq!(partial["data"]["observation"]["report"]["complete"], false);
    assert_mirrored_and_bounded(&complete)?;
    assert_mirrored_and_bounded(&partial)?;

    let mut unknown_report = complete.clone();
    unknown_report["data"]["observation"]["report"]
        .as_object_mut()
        .ok_or("quality report object")?
        .insert("aggregate_score".into(), json!(100));
    assert_invalid(&schema, &unknown_report)?;

    let mut unknown_details = complete.clone();
    unknown_details["data"]["observation"]["report"]["stages"][0]["details"]
        .as_object_mut()
        .ok_or("quality details object")?
        .insert("forged_complete".into(), json!(true));
    assert_invalid(&schema, &unknown_details)?;

    let mut bad_profile = complete.clone();
    bad_profile["data"]["observation"]["report"]["profile"] = json!("fast");
    assert_invalid(&schema, &bad_profile)?;

    let mut bad_stage = complete.clone();
    bad_stage["data"]["observation"]["report"]["stages"][0]["stage"] = json!("security_score");
    assert_invalid(&schema, &bad_stage)?;

    let mut too_many_artifacts = complete;
    too_many_artifacts["data"]["artifacts"] = Value::Array(vec![artifact(), artifact()]);
    assert_invalid(&schema, &too_many_artifacts)
}
