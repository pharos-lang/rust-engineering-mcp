use super::*;
use rust_engineering_domain::benchmark::{
    APPROVED_CRITERION_VERSION, BenchmarkHarness, BenchmarkIdentity, BenchmarkMeasurement,
    BenchmarkProvenance, BenchmarkSelection, HardwareProfile, MeasurementCompleteness, RawSample,
    ResourceQuotas, SampleUnit, SamplingMode, Virtualization,
};

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) const BASELINE: &str = "qart_00000000000000000000000000000001";
pub(super) const CANDIDATE: &str = "qart_00000000000000000000000000000002";

pub(super) fn dataset(keys: usize) -> TestResult<BenchmarkDataset> {
    let measurements = (0..keys)
        .map(|index| {
            let key = format!("bench/{index:04}");
            let identity = BenchmarkIdentity::new(
                "group".into(),
                Some("function".into()),
                None,
                key.clone(),
                key.replace('/', "_"),
            )?;
            let samples = (0..32)
                .map(|sample| RawSample::new(1, 1_000.0 + f64::from(sample)))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(BenchmarkMeasurement::new(
                identity,
                SamplingMode::Flat,
                samples,
                3_000,
                5_000,
                32,
                MeasurementCompleteness::Complete,
            )?)
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    Ok(BenchmarkDataset::new(
        SampleUnit::Nanoseconds,
        measurements,
        BenchmarkProvenance {
            source_fingerprint: format!("sha256:{}", "a".repeat(64)),
            harness: BenchmarkHarness::Criterion,
            harness_version: APPROVED_CRITERION_VERSION.into(),
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: Some("1.98.1".into()),
            image_digest: format!("sha256:{}", "c".repeat(64)),
            platform: "aarch64-unknown-linux-gnu".into(),
            configuration_fingerprint: format!("sha256:{}", "d".repeat(64)),
            execution_fingerprint: format!("sha256:{}", "3".repeat(64)),
            selection: BenchmarkSelection {
                package: Some("member".into()),
                bench_target: Some("throughput".into()),
                features: vec!["std".into()],
                all_features: false,
                no_default_features: false,
                profile: "bench".into(),
            },
            hardware: HardwareProfile {
                cpu_model: Some("Neoverse-N1".into()),
                cpu_cores: Some(4),
                os_kernel: Some("Linux 6.6.0".into()),
                arch: "aarch64".into(),
                virtualization: Virtualization::Container,
                cpu_governor: Some("performance".into()),
                quotas: ResourceQuotas {
                    cpu_quota_millicores: Some(2_000),
                    memory_bytes: Some(2 << 30),
                    pids: Some(256),
                },
            },
            run_index: 1,
            run_count: 3,
            captured_at_unix: 1_757_000_000,
        },
    )?)
}

pub(super) fn comparison_row(key: &str, verdict: ComparisonVerdict, ratio: f64)
-> BenchmarkComparison {
    BenchmarkComparison {
        key: key.to_owned(),
        verdict,
        effect_ratio: ratio,
        confidence_interval: (ratio - 0.01, ratio + 0.01),
        baseline_median_ns: 1_000.0,
        candidate_median_ns: 1_000.0 * (1.0 + ratio),
        baseline_samples: 32,
        candidate_samples: 32,
        baseline_outliers: 1,
        candidate_outliers: 2,
        minimum_detectable_ratio: 0.02,
        inconclusive_reasons: if verdict == ComparisonVerdict::Inconclusive {
            vec![InconclusiveReason::IntervalSpansThreshold]
        } else {
            Vec::new()
        },
    }
}

pub(super) fn comparison_report(rows: Vec<BenchmarkComparison>) -> ComparisonReport {
    let compared = rows.len();
    ComparisonReport {
        method: ComparisonMethod::frozen(u32::try_from(compared).unwrap_or(u32::MAX)),
        comparisons: rows,
        compared,
        baseline_only: vec!["bench/only-baseline".into()],
        candidate_only: vec!["bench/only-candidate".into()],
    }
}

pub(super) fn input() -> TestResult<Input> {
    Ok(Input {
        project_ref: "prj_00000000000000000000000000000001".parse()?,
        baseline_artifact_id: BASELINE.parse()?,
        candidate_artifact_id: CANDIDATE.parse()?,
        timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
    })
}

fn arguments(extra: &[(&str, serde_json::Value)]) -> TestResult<rmcp::model::JsonObject> {
    let mut value = serde_json::json!({
        "project_ref": "prj_00000000000000000000000000000001",
        "baseline_artifact_id": BASELINE,
        "candidate_artifact_id": CANDIDATE,
    })
    .as_object()
    .cloned()
    .ok_or("arguments")?;
    for (key, item) in extra {
        value.insert((*key).into(), item.clone());
    }
    Ok(value)
}

#[test]
fn closed_input_rejects_composed_identifiers_and_out_of_range_numbers() -> TestResult {
    let tool = ComparisonTool::new()?;
    assert!(tool.contract.decode(Some(arguments(&[])?)).is_ok());
    assert!(
        tool.contract
            .decode(Some(arguments(&[(
                "timeout_seconds",
                serde_json::json!(1)
            )])?))
            .is_ok()
    );
    for (key, value) in [
        ("baseline_artifact_id", serde_json::json!("qart_")),
        (
            "baseline_artifact_id",
            serde_json::json!("qart_0000000000000000000000000000000G"),
        ),
        (
            "baseline_artifact_id",
            serde_json::json!("prj_00000000000000000000000000000001"),
        ),
        (
            "candidate_artifact_id",
            serde_json::json!("../qart_00000000000000000000000000000002"),
        ),
        (
            "candidate_artifact_id",
            serde_json::json!("qart_00000000000000000000000000000002/../secret"),
        ),
        ("timeout_seconds", serde_json::json!(0)),
        ("timeout_seconds", serde_json::json!(31)),
        // No process runs, so no execution mode is accepted.
        ("execution_mode", serde_json::json!("synchronous")),
        ("path", serde_json::json!("/tmp/dataset.json")),
    ] {
        assert!(
            tool.contract
                .decode(Some(arguments(&[(key, value.clone())])?))
                .is_err(),
            "accepted {key}={value}"
        );
    }
    Ok(())
}

#[test]
fn the_decoder_fails_closed_on_anything_that_is_not_this_dataset_format() -> TestResult {
    let decoder = JsonDatasetDecoder;
    let dataset = dataset(1)?;
    let bytes = serde_json::to_vec(&dataset)?;
    assert!(decoder.decode(&bytes).is_ok());

    // Not a dataset at all.
    for payload in [
        &b"{}"[..],
        &b"[]"[..],
        &b"not json"[..],
        br#"{"testsuite":{"tests":1}}"#,
    ] {
        assert!(matches!(
            decoder.decode(payload),
            Err(BenchmarkCompareError::InvalidDataset(_))
        ));
    }

    // A payload that claims a format version this reader does not implement,
    // and one that claims another producer's format entirely.
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
    value["format_version"] = serde_json::json!(2);
    assert!(matches!(
        decoder.decode(&serde_json::to_vec(&value)?),
        Err(BenchmarkCompareError::InvalidDataset(
            BenchmarkError::UnknownFormat
        ))
    ));
    let mut value: serde_json::Value = serde_json::from_slice(&bytes)?;
    value["format"] = serde_json::json!("rust-engineering-mcp.benchmark-dataset.v2");
    assert!(matches!(
        decoder.decode(&serde_json::to_vec(&value)?),
        Err(BenchmarkCompareError::InvalidDataset(
            BenchmarkError::UnknownFormat
        ))
    ));

    // Oversize, refused before a single field is believed.
    let oversize = vec![b' '; usize::try_from(COMPARE_MAX_DATASET_BYTES)? + 1];
    assert!(matches!(
        decoder.decode(&oversize),
        Err(BenchmarkCompareError::ArtifactTooLarge)
    ));
    Ok(())
}

#[test]
fn call_boundary_requires_a_runtime_and_declares_an_absent_store() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        let tool = ComparisonTool::new()?;
        let error = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .err()
            .ok_or("runtime must be required")?;
        assert_eq!(error.message, "Comparison runtime is not configured");

        let ready = Arc::new(AtomicBool::new(false));
        let tool = ComparisonTool::new()?.with_runtime(Runtime {
            registry: Arc::new(Mutex::new(
                Registry::new(
                    rust_engineering_project::SecureProjects::new(&[]).map_err(|_| "backend")?,
                    rust_engineering_project::OsReferences,
                    rust_engineering_project::MonotonicClock::default(),
                    10,
                    1,
                )
                .map_err(|_| "registry")?,
            )),
            workers: Workers::new(),
            ready: Arc::clone(&ready),
            store: None,
        });
        let blocked = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?;
        assert_eq!(
            blocked
                .structured_content
                .clone()
                .ok_or("content")?["error_code"],
            "SANDBOX_DENIED"
        );
        ready.store(true, Ordering::Release);
        let unavailable = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?;
        let value = unavailable.structured_content.ok_or("content")?;
        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["error_code"], "ARTIFACT_UNAVAILABLE");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn every_failure_shape_has_a_closed_status_and_code() -> TestResult {
    let tool = ComparisonTool::new()?;
    let cases: Vec<(Failure, &str, serde_json::Value)> = vec![
        (Failure::Cancelled, "cancelled", serde_json::Value::Null),
        (
            Failure::Timeout,
            "blocked",
            serde_json::json!("COMMAND_TIMEOUT"),
        ),
        (
            Failure::Worker,
            "blocked",
            serde_json::json!("SANDBOX_DENIED"),
        ),
        (
            Failure::Body(BenchmarkCompareError::Cancelled),
            "cancelled",
            serde_json::Value::Null,
        ),
        (
            Failure::Body(BenchmarkCompareError::ArtifactNotFound),
            "blocked",
            serde_json::json!("ARTIFACT_NOT_FOUND"),
        ),
        (
            Failure::Body(BenchmarkCompareError::ArtifactUnreadable),
            "blocked",
            serde_json::json!("ARTIFACT_UNREADABLE"),
        ),
        (
            Failure::Body(BenchmarkCompareError::ArtifactTooLarge),
            "blocked",
            serde_json::json!("ARTIFACT_TOO_LARGE"),
        ),
        (
            Failure::Body(BenchmarkCompareError::NotADataset),
            "blocked",
            serde_json::json!("NOT_A_DATASET"),
        ),
        (
            Failure::Body(BenchmarkCompareError::InvalidDataset(
                BenchmarkError::UnknownFormat,
            )),
            "blocked",
            serde_json::json!("INVALID_DATASET"),
        ),
        (
            Failure::Body(BenchmarkCompareError::NoCommonBenchmark),
            "blocked",
            serde_json::json!("NO_COMMON_BENCHMARK"),
        ),
    ];
    for (failure, status, code) in cases {
        let value = tool
            .error(failure, 7)?
            .structured_content
            .ok_or("content")?;
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], code);
        assert_eq!(value["duration_ms"], 7);
    }
    assert!(tool.error(Failure::Body(BenchmarkCompareError::Internal), 7).is_err());
    Ok(())
}

#[test]
fn an_incompatible_pair_is_an_observed_result_and_not_a_protocol_error() -> TestResult {
    let tool = ComparisonTool::new()?;
    let input = input()?;
    let encoded = tool.encode_result(
        &input,
        CompareOutcome::Incompatible(vec![
            IncompatibilityReason::SameArtifact,
            IncompatibilityReason::CpuModel,
            IncompatibilityReason::CpuModel,
            IncompatibilityReason::RustVersion,
        ]),
        7,
    )?;
    assert_eq!(encoded.is_error, Some(false));
    let value = encoded.structured_content.ok_or("content")?;
    assert_eq!(value["status"], "failed");
    assert_eq!(value["error_code"], "INCOMPATIBLE_DATASETS");
    assert_eq!(
        value["data"]["report"]["incompatibility_reasons"],
        serde_json::json!(["rust_version", "cpu_model", "same_artifact"])
    );
    assert_eq!(value["data"]["report"]["method"], serde_json::Value::Null);
    assert_eq!(value["data"]["baseline_artifact_id"], BASELINE);
    assert_eq!(value["data"]["candidate_artifact_id"], CANDIDATE);
    Ok(())
}

#[test]
fn a_report_publishes_the_whole_frozen_method_and_ranks_its_rows() -> TestResult {
    let tool = ComparisonTool::new()?;
    let input = input()?;
    let report = comparison_report(vec![
        comparison_row("bench/c", ComparisonVerdict::Inconclusive, 0.30),
        comparison_row("bench/a", ComparisonVerdict::NoMaterialChange, 0.01),
        comparison_row("bench/b", ComparisonVerdict::Regression, 0.12),
        comparison_row("bench/d", ComparisonVerdict::Improvement, -0.20),
    ]);
    let encoded = tool.encode_result(&input, CompareOutcome::Report(Box::new(report)), 7)?;
    assert_eq!(encoded.is_error, Some(false));
    let value = encoded.structured_content.ok_or("content")?;
    assert_eq!(value["status"], "passed");
    assert_eq!(value["data"]["report"]["compared"], 4);
    let method = &value["data"]["report"]["method"];
    assert_eq!(method["statistic"], "median_per_iteration_nanoseconds");
    assert_eq!(method["bootstrap_resamples"], 10_000);
    assert_eq!(method["confidence_level"], 0.95);
    assert_eq!(method["material_threshold_ratio"], 0.05);
    assert_eq!(method["multiplicity"], "bonferroni");
    assert_eq!(method["family_size"], 4);
    assert_eq!(method["outlier_policy"], "reported_not_removed");
    assert_eq!(
        method["seed"],
        rust_engineering_domain::benchmark_compare::BOOTSTRAP_SEED
    );
    let keys: Vec<&str> = value["data"]["report"]["comparisons"]
        .as_array()
        .ok_or("comparisons")?
        .iter()
        .filter_map(|row| row["key"].as_str())
        .collect();
    // Material verdicts rank ahead of a withheld one, largest effect first.
    assert_eq!(keys, ["bench/d", "bench/b", "bench/a", "bench/c"]);
    let row = &value["data"]["report"]["comparisons"][3];
    assert_eq!(row["verdict"], "inconclusive");
    assert_eq!(
        row["inconclusive_reasons"],
        serde_json::json!(["interval_spans_threshold"])
    );
    assert_eq!(row["minimum_detectable_ratio"], 0.02);
    assert_eq!(row["baseline_outliers"], 1);
    assert_eq!(row["candidate_outliers"], 2);
    assert!(row["confidence_interval"]["low"].is_number());
    assert!(row["confidence_interval"]["high"].is_number());
    assert_eq!(
        value["data"]["report"]["baseline_only"],
        serde_json::json!(["bench/only-baseline"])
    );
    assert_eq!(
        value["data"]["report"]["candidate_only"],
        serde_json::json!(["bench/only-candidate"])
    );
    // No field name, enum spelling or summary in this contract names a cause,
    // a reason for a value, or an action the reader ought to take.
    let mut vocabulary = Vec::new();
    collect_vocabulary(&value, &mut vocabulary);
    vocabulary.push(
        value["summary"]
            .as_str()
            .ok_or("summary")?
            .to_ascii_lowercase(),
    );
    for word in &vocabulary {
        for forbidden in [
            "cause",
            "because",
            "due_to",
            "due to",
            "recommend",
            "suggest",
            "should",
            "optimi",
            "improve_by",
            "explain",
            "why",
        ] {
            assert!(!word.contains(forbidden), "causal language {forbidden:?} in {word:?}");
        }
    }
    Ok(())
}

/// Every object key and every string value the payload publishes, lowercased.
/// The vocabulary a peer reads is exactly this set.
fn collect_vocabulary(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                into.push(key.to_ascii_lowercase());
                collect_vocabulary(item, into);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_vocabulary(item, into);
            }
        }
        serde_json::Value::String(text) => into.push(text.to_ascii_lowercase()),
        _ => {}
    }
}

#[test]
fn a_non_finite_statistic_is_published_as_an_absence_rather_than_failing_to_encode() {
    assert_eq!(finite(1.5), 1.5);
    assert_eq!(finite(f64::NAN), 0.0);
    assert_eq!(finite(f64::INFINITY), 0.0);
    assert_eq!(finite(f64::NEG_INFINITY), 0.0);
}
