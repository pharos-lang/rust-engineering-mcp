use super::*;
use rust_engineering_domain::*;
use serde_json::{Value, json};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn structure() -> Result<ProjectStructure, Box<dyn std::error::Error>> {
    let fingerprint = format!("sha256:{:064x}", 42);
    Ok(ProjectStructure {
        workspace_members: vec![0],
        workspace_default_members: vec![0],
        packages: vec![ProjectPackage {
            package_index: 0,
            name: "captured-member".into(),
            version: "0.1.0".into(),
            manifest_path: "Cargo.toml".into(),
            edition: RustEdition::E2024,
            rust_version: None,
            targets: vec![ProjectTarget {
                name: "lib".into(),
                kinds: vec![TargetKind::Lib],
                crate_types: vec![TargetKind::Lib],
                source_path: "src/lib.rs".into(),
                edition: RustEdition::E2024,
                required_features: vec![],
                test: true,
                doctest: true,
            }],
            features: vec![DeclaredFeature {
                name: "default".into(),
                activations: vec![],
            }],
            direct_dependencies: vec![DirectDependency {
                name: "local".into(),
                rename: None,
                version_requirement: "*".into(),
                kind: DeclaredDependencyKind::Normal,
                optional: false,
                uses_default_features: true,
                features: vec![],
                target_condition: None,
                origin: DependencyOrigin {
                    kind: DependencySourceKind::Path,
                    identity: fingerprint.parse()?,
                    relative_path: Some("local".into()),
                },
            }],
        }],
        profiles: vec![DeclaredProfile {
            name: "dev".into(),
            inherits: None,
            settings: vec![ProfileSetting {
                name: ProfileSettingName::OptLevel,
                value: ProfileValue::Integer(0),
            }],
            package_overrides: vec![PackageProfile {
                package: "*".into(),
                settings: vec![],
            }],
            build_override: vec![],
        }],
        cargo_configuration: CargoConfiguration {
            project_config_policy: ProjectConfigPolicy::Rejected,
            frozen: true,
            offline: true,
            incremental: false,
            target_directory_ephemeral: true,
        },
        runtime: RuntimeIdentity {
            platform: "linux/arm64".into(),
            image_id: fingerprint.clone(),
            configuration_fingerprint: fingerprint.parse()?,
            execution_fingerprint: fingerprint.parse()?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        source_fingerprint: fingerprint.parse()?,
    })
}

fn successful() -> Result<Output, Box<dyn std::error::Error>> {
    let structure = structure()?;
    let provenance = Provenance::new(
        SourceKind::ProjectSnapshot,
        structure.source_fingerprint.to_string().parse()?,
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
        Ok(ProjectInspection {
            project_ref: "prj_00000000000000000000000000000001".parse()?,
            project_identity_fingerprint: format!("sha256:{:064x}", 1).parse()?,
            semantics: InspectionSemantics::LatestKnown,
            structure,
            evidence,
        }),
        2,
    )?)
}
#[test]
fn domain_output_matches_closed_schema_and_required_nullable_facts() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let schema = serde_json::to_value(contract.output_schema.as_ref())?;
    let validator = jsonschema::validator_for(&schema)?;
    let encoded = contract.encode(successful()?)?;
    assert_eq!(encoded.is_error, Some(false));
    let value = encoded.structured_content.ok_or("missing data")?;
    assert_eq!(
        value["data"]["structure"]["packages"][0]["rust_version"],
        Value::Null
    );
    assert_eq!(
        value["evidence"]["details"]["provenance"]["source_kind"],
        "project_snapshot"
    );
    assert_eq!(value["evidence"]["details"]["freshness"]["age_seconds"], 2);
    for pointer in [
        "",
        "/data",
        "/data/structure",
        "/data/structure/packages/0",
        "/data/structure/runtime",
        "/data/structure/packages/0/targets/0",
        "/data/structure/packages/0/features/0",
        "/data/structure/packages/0/direct_dependencies/0",
        "/data/structure/packages/0/direct_dependencies/0/origin",
        "/data/structure/profiles/0",
        "/data/structure/profiles/0/settings/0",
        "/data/structure/profiles/0/settings/0/value",
        "/data/structure/profiles/0/package_overrides/0",
        "/data/structure/cargo_configuration",
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
    for (pointer, field) in [
        ("/data/structure/packages/0", "rust_version"),
        ("/data/structure/runtime", "declared_toolchain"),
        ("/evidence/details/provenance", "observed_at"),
    ] {
        let mut missing = value.clone();
        missing
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .remove(field);
        assert!(
            !validator.is_valid(&missing),
            "optional omission: {pointer}/{field}"
        );
    }
    for incorrect in [
        json!({"kind":"boolean","value":5}),
        json!({"kind":"integer","value":true}),
        json!({"kind":"text","value":false}),
    ] {
        let mut mismatched = value.clone();
        mismatched["data"]["structure"]["profiles"][0]["settings"][0]["value"] = incorrect;
        assert!(!validator.is_valid(&mismatched));
    }
    let mut forged = value;
    forged["data"]["semantics"] = json!("latest");
    assert!(!validator.is_valid(&forged));
    Ok(())
}
#[test]
fn operational_results_and_infrastructure_failures_remain_distinct() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for error in [
        InspectionError::InvalidMetadata,
        InspectionError::OutputLimit,
        InspectionError::Execution(ExecutionError::Denied),
        InspectionError::Execution(ExecutionError::Unavailable),
        InspectionError::Project(ProjectError::Cancelled),
    ] {
        let encoded = contract.encode(output(Err(error), 1)?)?;
        assert_eq!(encoded.is_error, Some(true));
        let value = encoded.structured_content.ok_or("output")?;
        assert!(value["data"].is_null());
        assert_eq!(value["evidence"], json!({"kind":"local"}));
    }
    for error in [
        InspectionError::Internal,
        InspectionError::Execution(ExecutionError::Infrastructure),
        InspectionError::Execution(ExecutionError::CleanupUncertain),
    ] {
        let error = output(Err(error), 1).err().ok_or("infrastructure hidden")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert!(error.data.is_none());
    }
    Ok(())
}
#[test]
fn project_ref_only_input_rejects_extra_fields_and_malformed_tokens() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for value in [
        json!({}),
        json!({"project_ref":null}),
        json!({"project_ref":"prj_00000000000000000000000000000001", "features":["a"]}),
        json!({"project_ref":"/etc/passwd"}),
        json!({"project_ref":true}),
    ] {
        let error = contract
            .decode(Some(serde_json::from_value(value)?))
            .err()
            .ok_or("accepted")?;
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, "Invalid tool arguments");
    }
    Ok(())
}

#[test]
fn complete_wire_budget_rejects_large_structured_and_text_payload() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let mut value = successful()?;
    let Outcome::Passed { data, .. } = &mut value.outcome else {
        return Err("fixture".into());
    };
    // Valid individual schema fields can collectively exceed the envelope budget.
    data.structure.packages[0].features = (0..256)
        .map(|index| DeclaredFeature {
            name: format!("feature_{index}"),
            activations: vec!["x".repeat(2048)],
        })
        .collect();
    let encoded = encode_bounded(&contract, value)?;
    assert_eq!(encoded.is_error, Some(true));
    let bytes = serde_json::to_vec(&encoded)?;
    assert!(bytes.len() <= MAX_RESULT);
    let value = encoded.structured_content.ok_or("output")?;
    assert_eq!(value["error_code"], "OUTPUT_LIMIT_EXCEEDED");
    assert!(value["data"].is_null());
    assert_eq!(value["truncation"]["stdout_truncated"], true);
    Ok(())
}
