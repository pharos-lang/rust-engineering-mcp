use super::*;
use rust_engineering_application::{ExecutionCancellation, OperationControl};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Control(bool);
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.0 {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        self.0
    }
}
struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(110)
    }
}
fn version(n: u32) -> InspectVersion {
    InspectVersion {
        version: format!("1.0.{n}"),
        yanked: n == 2,
        rust_version: None,
        license: Some("MIT".into()),
        published_at: None,
        feature_count: 3,
        dependency_count: 3,
        advisory_count: 3,
    }
}
fn page(data: InspectPageData, count: u32) -> InspectPage {
    InspectPage {
        overview: InspectOverview {
            name: "serde".into(),
            description: "Serialization".into(),
            repository: None,
            updated_at: None,
            latest_known_stable: Some(KnownVersion {
                version: "1.0.2".into(),
                yanked: true,
                rust_version: None,
                license: None,
            }),
            version_count: 3,
            documentation: InspectUnknown::default(),
            source: InspectUnknown::default(),
        },
        data,
        pagination: InspectPagination {
            offset: 0,
            total: count,
            returned: count,
            next_offset: None,
            omitted_by_output: 0,
        },
    }
}
fn result(lookup: InspectLookup) -> Result<CrateInspectResult, Box<dyn std::error::Error>> {
    Ok(CrateInspectResult {
        name: "serde".into(),
        snapshot_fingerprint: format!("sha256:{}", "1".repeat(64)).parse()?,
        sequence: 1,
        evidence: SnapshotEvidence::assess(
            Provenance::new(
                SourceKind::RegistrySnapshot,
                "fixture:catalog".parse()?,
                Some(UnixSeconds(100)),
                Some(UnixSeconds(101)),
                IntegrityStatus::Verified,
                false,
            )?,
            FreshnessPolicy::new("inspect-fixture-v1".parse()?, 60, 300)?,
            &FixedClock,
        ),
        lookup,
    })
}
fn found(page: InspectPage) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(output(
        Ok(result(InspectLookup::Found {
            page: Box::new(page),
        })?),
        1,
    )?)
}
fn sections() -> Vec<InspectPageData> {
    vec![
        InspectPageData::Overview {
            selected_version: Some(version(2)),
        },
        InspectPageData::Overview {
            selected_version: None,
        },
        InspectPageData::Versions {
            items: (0..3).rev().map(version).collect(),
        },
        InspectPageData::Features {
            version: version(2),
            items: vec!["a".into(), "b".into(), "c".into()],
        },
        InspectPageData::Dependencies {
            version: version(2),
            items: [
                DependencyKind::Normal,
                DependencyKind::Build,
                DependencyKind::Dev,
            ]
            .into_iter()
            .map(|kind| DependencyRecord {
                name: "serde".into(),
                requirement: "^1".into(),
                kind,
                optional: false,
            })
            .collect(),
        },
        InspectPageData::Advisories {
            version: version(2),
            items: (1..4).map(|n| format!("RUSTSEC-2024-000{n}")).collect(),
        },
    ]
}
fn decode(
    contract: &Contract<Input, Output>,
    value: Value,
) -> Result<CrateInspectRequest, ErrorData> {
    contract.decode(value.as_object().cloned())?.request()
}
#[test]
fn input_defaults_closure_and_section_pagination_rules() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let r = decode(&contract, json!({"name":"serde"}))?;
    assert_eq!(r.section, InspectSection::Overview);
    assert_eq!((r.limit, r.offset), (20, 0));
    assert!(r.version.is_none() && r.snapshot_fingerprint.is_none());
    for value in [
        json!({}),
        json!({"name":""}),
        json!({"name":"é"}),
        json!({"name":"a/b"}),
        json!({"name":"a".repeat(65)}),
        json!({"name":"serde","path":"/tmp"}),
        json!({"name":"serde","limit":0}),
        json!({"name":"serde","limit":51}),
        json!({"name":"serde","section":"invalid"}),
        json!({"name":"serde","offset":1}),
        json!({"name":"serde","section":"versions","version":"1.0.0"}),
        json!({"name":"serde","version":""}),
        json!({"name":"serde","version":"a".repeat(129)}),
        json!({"name":"serde","version":"1.0.0\n"}),
        json!({"name":"serde","snapshot_fingerprint":"bad"}),
    ] {
        assert!(decode(&contract, value.clone()).is_err(), "{value}");
    }
    let fingerprint = format!("sha256:{}", "1".repeat(64));
    for section in ["features", "dependencies", "advisories"] {
        assert!(decode(&contract, json!({"name":"serde","section":section})).is_err());
        decode(
            &contract,
            json!({"name":"serde","section":section,"version":"1.0.0","offset":128,"snapshot_fingerprint":fingerprint,"limit":50}),
        )?;
    }
    for section in ["versions", "features", "dependencies", "advisories"] {
        let mut v =
            json!({"name":"A_b-1","section":section,"offset":1,"snapshot_fingerprint":fingerprint});
        if section != "versions" {
            v["version"] = json!("1.0.0");
        }
        decode(&contract, v.clone())?;
        v["offset"] = json!(129);
        assert!(decode(&contract, v.clone()).is_err());
        v["offset"] = json!(1);
        v.as_object_mut()
            .ok_or("object")?
            .remove("snapshot_fingerprint");
        assert!(decode(&contract, v).is_err());
    }
    assert!(
        decode(
            &contract,
            json!({"name":"serde","offset":1,"snapshot_fingerprint":fingerprint})
        )
        .is_err()
    );
    decode(&contract, json!({"name":"a".repeat(64),"version":"1.0.0"}))?;
    Ok(())
}
fn assert_closed(
    validator: &jsonschema::Validator,
    root: &Value,
    value: &Value,
    path: &str,
) -> TestResult {
    match value {
        Value::Object(object) => {
            let mut bad = root.clone();
            bad.pointer_mut(path)
                .and_then(Value::as_object_mut)
                .ok_or("object")?
                .insert("unexpected".into(), json!(true));
            assert!(!validator.is_valid(&bad), "open object {path}");
            for (key, child) in object {
                assert_closed(validator, root, child, &format!("{path}/{key}"))?;
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                assert_closed(validator, root, child, &format!("{path}/{i}"))?;
            }
        }
        _ => {}
    }
    Ok(())
}
#[test]
fn all_sections_unknown_facts_and_nested_objects_match_announced_schema() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    for data in sections() {
        let encoded = encode_bounded(&contract, found(page(data, 3))?, &Control(false))?;
        let wire = serde_json::to_value(encoded)?;
        let value = &wire["structuredContent"];
        assert_eq!(
            serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().ok_or("text")?)?,
            *value
        );
        assert!(validator.is_valid(value));
        assert_eq!(value["data"]["coverage"], "snapshot_page_only");
        assert_eq!(
            value["data"]["inspection"]["lookup"]["page"]["overview"]["documentation"],
            json!({"status":"unknown","reason":"not_recorded_in_snapshot"})
        );
        assert_closed(&validator, value, value, "")?;
        for pointer in [
            "/data/inspection/lookup/page/data/selected_version",
            "/data/inspection/lookup/page/data/version",
            "/data/inspection/lookup/page/data/items/0",
        ] {
            if !value.pointer(pointer).is_some_and(Value::is_object) {
                continue;
            }
            for field in ["rust_version", "license", "published_at"] {
                if value.pointer(pointer).and_then(|v| v.get(field)).is_none() {
                    continue;
                }
                let mut missing = value.clone();
                missing
                    .pointer_mut(pointer)
                    .and_then(Value::as_object_mut)
                    .ok_or("version")?
                    .remove(field);
                assert!(!validator.is_valid(&missing), "missing {pointer}/{field}");
                let mut nullable = value.clone();
                nullable
                    .pointer_mut(pointer)
                    .and_then(Value::as_object_mut)
                    .ok_or("version")?
                    .insert(field.into(), Value::Null);
                assert!(validator.is_valid(&nullable));
            }
        }
        if value["data"]["inspection"]["lookup"]["page"]["data"]["section"] == "overview" {
            let mut missing = value.clone();
            missing["data"]["inspection"]["lookup"]["page"]["data"]
                .as_object_mut()
                .ok_or("data")?
                .remove("selected_version");
            assert!(!validator.is_valid(&missing));
        }

        for (pointer, field) in [
            ("/data/inspection/lookup/page/overview", "repository"),
            ("/data/inspection/lookup/page/overview", "updated_at"),
            (
                "/data/inspection/lookup/page/overview",
                "latest_known_stable",
            ),
            ("/data/inspection/lookup/page/pagination", "next_offset"),
            ("/data/inspection/evidence/provenance", "created_at"),
            ("/data/inspection/evidence/provenance", "observed_at"),
            ("/data/inspection/evidence/freshness", "age_seconds"),
        ] {
            let mut missing = value.clone();
            missing
                .pointer_mut(pointer)
                .and_then(Value::as_object_mut)
                .ok_or("object")?
                .remove(field);
            assert!(!validator.is_valid(&missing), "missing {pointer}/{field}");
            let mut nullable = value.clone();
            nullable
                .pointer_mut(pointer)
                .and_then(Value::as_object_mut)
                .ok_or("object")?
                .insert(field.into(), Value::Null);
            assert!(validator.is_valid(&nullable));
        }
    }
    Ok(())
}
#[test]
fn missing_lookup_and_operational_errors_remain_distinct() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for (lookup, kind) in [
        (InspectLookup::CrateNotFound, "crate_not_found"),
        (InspectLookup::VersionNotFound, "version_not_found"),
    ] {
        let encoded = contract.encode(output(Ok(result(lookup)?), 0)?)?;
        assert_eq!(encoded.is_error, Some(false));
        assert_eq!(
            encoded.structured_content.ok_or("content")?["data"]["inspection"]["lookup"],
            json!({"kind":kind})
        );
    }
    for (error, status, code) in [
        (
            CatalogInspectError::SnapshotMismatch,
            "blocked",
            Some("SNAPSHOT_MISMATCH"),
        ),
        (
            CatalogInspectError::Unavailable(CatalogComponentUnavailable::Missing),
            "unavailable",
            Some("CATALOG_UNAVAILABLE"),
        ),
        (
            CatalogInspectError::Catalog(CatalogError::Integrity),
            "unavailable",
            Some("CATALOG_INVALID"),
        ),
        (
            CatalogInspectError::Catalog(CatalogError::Budget),
            "blocked",
            Some("OUTPUT_LIMIT_EXCEEDED"),
        ),
        (
            CatalogInspectError::Project(ProjectError::Rejected(
                OperationalErrorCode::SandboxDenied,
            )),
            "blocked",
            Some("SANDBOX_DENIED"),
        ),
        (
            CatalogInspectError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout,
            )),
            "blocked",
            Some("COMMAND_TIMEOUT"),
        ),
        (
            CatalogInspectError::Project(ProjectError::Cancelled),
            "cancelled",
            None,
        ),
    ] {
        let encoded = contract.encode(output(Err(error), 0)?)?;
        assert_eq!(encoded.is_error, Some(true));
        let v = encoded.structured_content.ok_or("content")?;
        assert_eq!(v["status"], status);
        assert_eq!(v["error_code"], json!(code));
        assert!(v["data"].is_null());
    }
    assert!(output(Err(CatalogInspectError::Project(ProjectError::Internal)), 0).is_err());
    assert!(
        output(
            Err(CatalogInspectError::Catalog(CatalogError::InvalidInput)),
            0
        )
        .is_err()
    );
    Ok(())
}
#[test]
fn full_mcp_budget_preserves_valid_maximum_version_facts_and_counts() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let mut p = page(
        InspectPageData::Versions {
            items: (0..50)
                .rev()
                .map(|n| {
                    let mut v = version(n);
                    v.license = Some("\u{1}".repeat(512));
                    v
                })
                .collect(),
        },
        50,
    );
    p.overview.version_count = 64;
    p.pagination.total = 64;
    p.pagination.next_offset = Some(50);
    let expected = serde_json::to_value(&p)?;
    let encoded = encode_bounded(&contract, found(p)?, &Control(false))?;
    assert!(serde_json::to_vec(&encoded)?.len() <= MAX_RESULT);
    let v = encoded.structured_content.ok_or("content")?;
    assert_eq!(v["status"], "passed");
    assert_eq!(v["data"]["inspection"]["lookup"]["page"], expected);
    Ok(())
}
#[test]
fn trimming_keeps_prefix_and_progress_without_skipping_or_emptying_page() -> TestResult {
    for data in sections() {
        let mut p = page(data, 3);
        p.pagination.offset = 2;
        p.pagination.total = 5;
        let original = serde_json::to_value(&p.data)?;
        if matches!(p.data, InspectPageData::Overview { .. }) {
            assert!(!trim_page(&mut p));
            assert_eq!(serde_json::to_value(&p.data)?, original);
            continue;
        }
        assert!(trim_page(&mut p));
        assert!(trim_page(&mut p));
        assert!(!trim_page(&mut p));
        assert_eq!(p.pagination.returned, 1);
        assert_eq!(p.pagination.omitted_by_output, 2);
        assert_eq!(p.pagination.next_offset, Some(3));
        assert_eq!(p.pagination.total, 5);
        let after = serde_json::to_value(&p.data)?;
        assert_eq!(after["items"], json!([original["items"][0]]));
    }
    Ok(())
}
#[test]
fn cancellation_before_encoding_cannot_publish_success() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let encoded = encode_bounded(
        &contract,
        found(page(
            InspectPageData::Overview {
                selected_version: None,
            },
            0,
        ))?,
        &Control(true),
    )?;
    assert_eq!(encoded.is_error, Some(true));
    let v = encoded.structured_content.ok_or("content")?;
    assert_eq!(v["status"], "cancelled");
    assert!(v["data"].is_null());
    Ok(())
}
