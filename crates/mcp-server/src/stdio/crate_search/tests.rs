use super::*;
use rust_engineering_application::{ExecutionCancellation, OperationControl};
use serde_json::{Value, json};
use std::sync::atomic::AtomicUsize;

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
impl ExecutionCancellation for Continue {
    fn is_cancelled(&self) -> bool {
        false
    }
}
struct CancelAfterFirst(AtomicUsize);
impl OperationControl for CancelAfterFirst {
    fn check(&self) -> Result<(), ProjectError> {
        if self.0.fetch_add(1, Ordering::SeqCst) > 0 {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl ExecutionCancellation for CancelAfterFirst {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst) > 1
    }
}
struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(110)
    }
}

fn evidence(kind: SourceKind, id: &str) -> Result<SnapshotEvidence, Box<dyn std::error::Error>> {
    Ok(SnapshotEvidence::assess(
        Provenance::new(
            kind,
            id.parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(101)),
            IntegrityStatus::Verified,
            false,
        )?,
        FreshnessPolicy::new("search-fixture-v1".parse()?, 60, 300)?,
        &FixedClock,
    ))
}
fn result(count: u32, description: &str) -> Result<CrateSearchResult, Box<dyn std::error::Error>> {
    let snapshot_fingerprint: CatalogFingerprint = format!("sha256:{}", "1".repeat(64)).parse()?;
    let model_evidence = evidence(SourceKind::EmbeddingModel, "fixture:model")?;
    let identity = EmbeddingIdentity {
        model: "fixture-model".into(),
        revision: "immutable-fixture-1".into(),
        artifact_fingerprint: format!("sha256:{}", "2".repeat(64)).parse()?,
        runtime: "fixture-runtime".into(),
        provenance: model_evidence.provenance().clone(),
        dimension: 2,
        max_tokens: 512,
        intra_threads: 2,
        pooling: PoolingKind::Mean,
        normalization: Normalization::L2,
    };
    let index = IndexMetadata {
        schema_version: 1,
        snapshot_fingerprint: snapshot_fingerprint.clone(),
        model: identity,
    };
    Ok(CrateSearchResult {
        requested_mode: CrateSearchMode::Hybrid,
        effective_mode: CrateSearchMode::Hybrid,
        fallback: None,
        snapshot_fingerprint,
        evidence: evidence(SourceKind::RegistrySnapshot, "fixture:catalog")?,
        semantic_index: Some(index),
        model_evidence: Some(model_evidence),
        results: (0..count)
            .map(|i| RankedCrate {
                facts: SearchCrateFacts {
                    name: format!("crate_{i:02}"),
                    description: description.into(),
                    repository: None,
                    latest_known_stable: Some(KnownVersion {
                        version: "2.0.0".into(),
                        yanked: true,
                        rust_version: Some("1.90".into()),
                        license: None,
                    }),
                    selected_version: SearchVersionFacts {
                        version: "1.0.0".into(),
                        yanked: false,
                        rust_version: Some("1.80".into()),
                        license: Some("MIT".into()),
                        published_at: None,
                        known_advisory_ids: vec!["RUSTSEC-2024-0001".into()],
                    },
                    version_count: 2,
                },
                lexical: Some(LexicalScore {
                    rank: i + 1,
                    bm25: -1.0 / f64::from(i + 1),
                }),
                semantic: Some(SemanticScore {
                    rank: i + 1,
                    squared_l2: i as f32 / 50.0,
                }),
                fusion_score: Some(2.0 / f64::from(61 + i)),
            })
            .collect(),
        window: SearchWindow {
            candidate_limit_per_channel: 50,
            lexical_candidates: count,
            semantic_candidates: count,
            examined: count,
            filtered_out: 0,
            eligible: count,
            returned: count,
            limit_truncated: 0,
            omitted_by_output: 0,
        },
    })
}
fn request(
    contract: &Contract<Input, Output>,
    value: Value,
) -> Result<CrateSearchRequest, ErrorData> {
    contract.decode(value.as_object().cloned())?.request()
}

#[test]
fn closed_input_defaults_and_semantic_query_bounds() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let default = request(&contract, json!({"query":"serde"}))?;
    assert_eq!(default.mode, CrateSearchMode::Hybrid);
    assert_eq!(default.query.limit(), 10);
    assert_eq!(default.filters, CrateSearchFilters::default());
    for value in [
        json!({}),
        json!({"query":"serde","refresh":true}),
        json!({"query":"serde","path":"/tmp"}),
        json!({"query":"serde","fts":"OR"}),
        json!({"query":"serde","filters":{"sql":"SELECT 1"}}),
        json!({"query":"serde","mode":"nearest"}),
        json!({"query":"serde","limit":0}),
        json!({"query":"serde","limit":51}),
        json!({"query":"serde","filters":null}),
        json!({"query":""}),
        json!({"query":"  "}),
        json!({"query":"line\nbreak"}),
        json!({"query":"a".repeat(257)}),
        json!({"query":"é".repeat(129)}),
        json!({"query":vec!["term";17].join(" ")}),
    ] {
        assert!(request(&contract, value.clone()).is_err(), "{value}");
    }
    for text in [
        "é".repeat(128),
        vec!["term"; 16].join(" "),
        "serde OR tokio".into(),
        "name:serde*".into(),
    ] {
        // FTS-looking text remains plain query text: there is no separate FTS/SQL authority input.
        assert_eq!(
            request(&contract, json!({"query":text}))?.query.text(),
            text
        );
    }
    for mode in ["lexical", "semantic", "hybrid"] {
        request(&contract, json!({"query":"serde","mode":mode,"limit":50}))?;
    }
    Ok(())
}

#[test]
fn msrv_input_accepts_only_canonical_bounded_versions() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    for msrv in ["1.80", "1.80.0", "0.0", "0.0.0"] {
        let parsed = request(
            &contract,
            json!({"query":"serde","filters":{"msrv_lte":msrv,"allow_yanked":true,"include_prerelease":true}}),
        )?;
        assert!(parsed.filters.msrv_lte.is_some());
        assert!(parsed.filters.allow_yanked && parsed.filters.include_prerelease);
    }
    for msrv in [
        "",
        "1",
        "01.80",
        "1.080",
        "1.80.00",
        "1.80.0.1",
        "1.80-nightly",
        "1.80.0+meta",
        " 1.80",
        "1.80 ",
        "18446744073709551616.0",
        "99999999999999999999999999999999.0",
    ] {
        assert!(
            request(
                &contract,
                json!({"query":"serde","filters":{"msrv_lte":msrv}})
            )
            .is_err(),
            "{msrv}"
        );
    }
    Ok(())
}

#[test]
fn hybrid_wire_matches_schema_and_closes_every_nested_object() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let search = result(1, "Small serialization crate")?;
    let expected = serde_json::to_value(&search)?;
    let encoded = encode_bounded(&contract, output(Ok(search), 7)?, &Continue)?;
    assert_eq!(encoded.is_error, Some(false));
    let wire = serde_json::to_value(encoded)?;
    let content = &wire["structuredContent"];
    assert_eq!(content["data"]["search"], expected);
    assert_eq!(
        serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().ok_or("text")?)?,
        *content
    );
    assert_eq!(content["data"]["coverage"], "candidate_window_only");
    assert_eq!(
        content["data"]["advisory_interpretation"],
        "snapshot_listed_ids_only"
    );
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    for pointer in [
        "",
        "/data",
        "/data/search",
        "/data/search/window",
        "/data/search/results/0",
        "/data/search/results/0/facts",
        "/data/search/results/0/facts/selected_version",
        "/data/search/results/0/facts/latest_known_stable",
        "/data/search/results/0/lexical",
        "/data/search/results/0/semantic",
        "/data/search/evidence",
        "/data/search/evidence/provenance",
        "/data/search/evidence/freshness",
        "/data/search/evidence/freshness/policy",
        "/data/search/semantic_index",
        "/data/search/semantic_index/model",
        "/data/search/semantic_index/model/provenance",
        "/data/search/model_evidence",
        "/truncation",
        "/evidence",
    ] {
        let mut bad = content.clone();
        bad.pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .insert("unexpected".into(), json!(true));
        assert!(!validator.is_valid(&bad), "open {pointer}");
    }
    for (pointer, field) in [
        ("/data/search", "fallback"),
        ("/data/search", "semantic_index"),
        ("/data/search", "model_evidence"),
        ("/data/search/results/0", "lexical"),
        ("/data/search/results/0", "semantic"),
        ("/data/search/results/0", "fusion_score"),
        ("/data/search/results/0/facts", "repository"),
        ("/data/search/results/0/facts", "latest_known_stable"),
        (
            "/data/search/results/0/facts/selected_version",
            "published_at",
        ),
        ("/data/search/results/0/facts/selected_version", "license"),
        ("/data/search/evidence/provenance", "created_at"),
        ("/data/search/evidence/freshness", "age_seconds"),
    ] {
        let mut missing = content.clone();
        missing
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .remove(field);
        assert!(
            !validator.is_valid(&missing),
            "missing nullable {pointer}/{field}"
        );
        let mut nullable = content.clone();
        nullable
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or("object")?
            .insert(field.into(), Value::Null);
        assert!(
            validator.is_valid(&nullable),
            "not nullable {pointer}/{field}"
        );
    }
    Ok(())
}

#[test]
fn fallback_tags_and_operational_errors_are_explicit() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let validator =
        jsonschema::validator_for(&serde_json::to_value(contract.output_schema.as_ref())?)?;
    for fallback in [
        SearchFallback::Unavailable {
            component: SearchComponent::Model,
            reason: CatalogComponentUnavailable::FeatureDisabled,
        },
        SearchFallback::Failed {
            reason: SemanticError::IdentityMismatch,
        },
    ] {
        let mut search = result(1, "fixture")?;
        search.effective_mode = CrateSearchMode::Lexical;
        search.fallback = Some(fallback);
        search.semantic_index = None;
        search.model_evidence = None;
        search.results[0].semantic = None;
        search.results[0].fusion_score = None;
        search.window.semantic_candidates = 0;
        let encoded = contract.encode(output(Ok(search), 0)?)?;
        let mut value = encoded.structured_content.ok_or("content")?;
        value["data"]["search"]["fallback"]
            .as_object_mut()
            .ok_or("fallback")?
            .insert("unexpected".into(), json!(true));
        assert!(!validator.is_valid(&value));
    }
    for (error, status, code) in [
        (
            CatalogSearchError::Unavailable(CatalogComponentUnavailable::Missing),
            "unavailable",
            Some("CATALOG_UNAVAILABLE"),
        ),
        (
            CatalogSearchError::Catalog(CatalogError::Integrity),
            "unavailable",
            Some("CATALOG_INVALID"),
        ),
        (
            CatalogSearchError::Catalog(CatalogError::Budget),
            "blocked",
            Some("OUTPUT_LIMIT_EXCEEDED"),
        ),
        (
            CatalogSearchError::Project(ProjectError::Rejected(
                OperationalErrorCode::SandboxDenied,
            )),
            "blocked",
            Some("SANDBOX_DENIED"),
        ),
        (
            CatalogSearchError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout,
            )),
            "blocked",
            Some("COMMAND_TIMEOUT"),
        ),
        (
            CatalogSearchError::Project(ProjectError::Cancelled),
            "cancelled",
            None,
        ),
    ] {
        let encoded = encode_bounded(&contract, output(Err(error), 1)?, &Continue)?;
        assert_eq!(encoded.is_error, Some(true));
        let value = encoded.structured_content.ok_or("content")?;
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], json!(code));
        assert!(value["data"].is_null());
    }
    for error in [
        CatalogSearchError::Project(ProjectError::Internal),
        CatalogSearchError::Catalog(CatalogError::InvalidInput),
    ] {
        assert!(output(Err(error), 0).is_err());
    }
    Ok(())
}

fn large_result() -> Result<CrateSearchResult, Box<dyn std::error::Error>> {
    // A permitted non-NUL control byte expands to six JSON bytes; facts stay within 4096 bytes.
    let mut search = result(50, &"\u{1}".repeat(4096))?;
    for row in search.results.iter_mut().skip(25) {
        row.semantic = None;
        row.fusion_score = row.lexical.as_ref().map(|s| 1.0 / f64::from(60 + s.rank));
    }
    search.window.examined = 75;
    search.window.eligible = 75;
    search.window.limit_truncated = 25;
    Ok(search)
}

#[test]
fn complete_result_budget_trims_suffix_without_cropping_facts_or_losing_counts() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let search = large_result()?;
    let original = serde_json::to_value(&search)?;
    let full = contract.encode(output(Ok(search.clone()), 1)?)?;
    assert!(serde_json::to_vec(&full)?.len() > MAX_RESULT);
    let encoded = encode_bounded(&contract, output(Ok(search), 1)?, &Continue)?;
    assert!(serde_json::to_vec(&encoded)?.len() <= MAX_RESULT);
    let wire = serde_json::to_value(encoded)?;
    let value = &wire["structuredContent"];
    assert_eq!(
        serde_json::from_str::<Value>(wire["content"][0]["text"].as_str().ok_or("text")?)?,
        *value
    );
    let after = &value["data"]["search"];
    let rows = after["results"].as_array().ok_or("rows")?;
    assert!(!rows.is_empty() && rows.len() < 50);
    assert_eq!(
        rows,
        &original["results"].as_array().ok_or("original rows")?[..rows.len()]
    );
    let returned = after["window"]["returned"].as_u64().ok_or("returned")?;
    let omitted = after["window"]["omitted_by_output"]
        .as_u64()
        .ok_or("omitted")?;
    assert_eq!(returned, rows.len() as u64);
    assert_eq!(returned + omitted, 50);
    assert_eq!(after["window"]["eligible"], 75);
    assert_eq!(after["window"]["limit_truncated"], 25);
    assert_eq!(returned + omitted + 25, 75);
    Ok(())
}

#[test]
fn cancellation_during_budget_trimming_cannot_publish_success() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let control = CancelAfterFirst(AtomicUsize::new(0));
    let encoded = encode_bounded(&contract, output(Ok(large_result()?), 1)?, &control)?;
    assert!(control.0.load(Ordering::SeqCst) >= 2);
    assert_eq!(encoded.is_error, Some(true));
    let value = encoded.structured_content.ok_or("content")?;
    assert_eq!(value["status"], "cancelled");
    assert!(value["data"].is_null());
    Ok(())
}
