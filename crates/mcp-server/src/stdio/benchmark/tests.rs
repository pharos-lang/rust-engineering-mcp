use super::super::security_tool::assert_common_error_contract;
use super::*;
use rust_engineering_domain::benchmark::{
    APPROVED_CRITERION_VERSION, BenchmarkHarness, BenchmarkIdentity, BenchmarkMeasurement,
    BenchmarkProvenance, HardwareProfile, RawSample, ResourceQuotas, SampleUnit,
};

pub(super) type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

pub(super) mod fixtures {
    use super::*;
    use rust_engineering_domain::ExecutionTermination;
    use rust_engineering_domain::benchmark_run::CriterionArchive;

    pub(in crate::stdio::benchmark) fn selection() -> BenchmarkSelection {
        BenchmarkSelection {
            package: Some("member".into()),
            bench_target: Some("throughput".into()),
            features: vec!["std".into()],
            all_features: false,
            no_default_features: false,
            profile: "bench".into(),
        }
    }

    pub(in crate::stdio::benchmark) fn measurement(
        key: &str,
        samples: usize,
        completeness: MeasurementCompleteness,
    ) -> TestResult<BenchmarkMeasurement> {
        let identity = BenchmarkIdentity::new(
            "group".into(),
            Some("function".into()),
            None,
            key.into(),
            key.replace('/', "_"),
        )?;
        let samples = (0..samples)
            .map(|index| RawSample::new(1, 1_000.0 + index as f64, 1))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BenchmarkMeasurement::new(
            identity,
            SamplingMode::Flat,
            samples,
            3_000,
            5_000,
            32,
            completeness,
        )?)
    }

    pub(in crate::stdio::benchmark) fn dataset(
        keys: usize,
        samples: usize,
        completeness: MeasurementCompleteness,
    ) -> TestResult<BenchmarkDataset> {
        let measurements = (0..keys)
            .map(|index| measurement(&format!("bench/{index:04}"), samples, completeness))
            .collect::<Result<Vec<_>, _>>()?;
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
                selection: selection(),
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

    pub(in crate::stdio::benchmark) fn observation(
        harness: HarnessDetection,
        exit: BenchmarkExit,
        dataset: Option<BenchmarkDataset>,
        omission: Option<DatasetOmission>,
    ) -> TestResult<BenchmarkObservation> {
        use super::super::super::security_tool::test_fixtures as fixture;
        Ok(BenchmarkObservation {
            selection: selection(),
            harness,
            exit,
            exit_code: Some(0),
            termination: ExecutionTermination::Exited,
            // The tree travels with the dataset or not at all: a run that
            // published no dataset exported no criterion tree either, and the
            // observation must declare which of the two it is.
            archive: dataset.as_ref().map(|_| CriterionArchive {
                run_index: 3,
                bytes: b"ustar-bytes".to_vec(),
            }),
            archive_omission: match dataset {
                Some(_) => None,
                None => omission.or(Some(DatasetOmission::OutputMissing)),
            },
            dataset,
            omission,
            runs_completed: 3,
            runs_requested: 3,
            runtime: fixture::runtime()?,
            execution_fingerprint: fixture::execution_fingerprint('3')?,
            vendor_fingerprint: fixture::source_fingerprint('5')?,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        })
    }

    /// A descriptor of the requested kind. The store's own kind/version/mime
    /// pairing is restated here so a fixture cannot claim a combination the
    /// descriptor validator rejects.
    pub(in crate::stdio::benchmark) fn descriptor(
        kind: QualityArtifactKind,
        completeness: ArtifactCompleteness,
    ) -> TestResult<QualityArtifactDescriptor> {
        use rust_engineering_domain::{
            ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
            ArtifactSource, GuestArtifactName, PayloadFormatVersion, PluginIdentity,
            QualityArtifactDraft, QualityArtifactId, QualityJobId, QualityMimeType, UtcInstant,
        };
        let (payload_format_version, mime_type, guest_name, identity) = match kind {
            QualityArtifactKind::BenchmarkDataset => (
                PayloadFormatVersion::BenchmarkDatasetV1,
                QualityMimeType::ApplicationJson,
                GuestArtifactName::BenchmarkDataset,
                PluginIdentity::Criterion,
            ),
            _ => (
                PayloadFormatVersion::UstarV1,
                QualityMimeType::ApplicationXTar,
                GuestArtifactName::CriterionArchive,
                PluginIdentity::Criterion,
            ),
        };
        let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
        Ok(QualityArtifactDraft {
            artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
            member_index: 0,
            kind,
            mime_type,
            payload_format_version,
            completeness,
            sensitivity: ArtifactSensitivity::PotentiallySensitive,
            created_at_utc: created.clone(),
            expires_at_utc: created.checked_add_seconds(60)?,
            source: ArtifactSource {
                captured_source_sha256: [2; 32],
                guest_name,
                selection: ArtifactSelection::Workspace,
            },
            runtime: ArtifactRuntime {
                image_digest: [3; 32],
                toolchain_identity: [4; 32],
                plugin: ArtifactPlugin {
                    identity,
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

    pub(in crate::stdio::benchmark) fn published(
        observation: BenchmarkObservation,
        completeness: ArtifactCompleteness,
    ) -> TestResult<PublishedBenchmark> {
        let artifacts = if observation.dataset.is_some() {
            vec![
                descriptor(QualityArtifactKind::BenchmarkDataset, completeness)?,
                descriptor(QualityArtifactKind::CriterionArchive, completeness)?,
            ]
        } else {
            Vec::new()
        };
        Ok(PublishedBenchmark {
            observation,
            artifacts,
        })
    }
}

fn arguments(extra: &[(&str, serde_json::Value)]) -> TestResult<rmcp::model::JsonObject> {
    let mut value = serde_json::json!({"project_ref":"prj_00000000000000000000000000000001"})
        .as_object()
        .cloned()
        .ok_or("arguments")?;
    for (key, item) in extra {
        value.insert((*key).into(), item.clone());
    }
    Ok(value)
}

#[test]
fn closed_input_rejects_paths_free_flags_and_out_of_range_numbers() -> TestResult {
    let tool = BenchmarkTool::new()?;
    assert!(tool.contract.decode(Some(arguments(&[])?)).is_ok());
    assert!(
        tool.contract
            .decode(Some(arguments(&[
                ("package", serde_json::json!("member")),
                ("bench_target", serde_json::json!("throughput")),
                ("features", serde_json::json!(["std", "std"])),
                ("run_count", serde_json::json!(1)),
                ("timeout_seconds", serde_json::json!(900)),
            ])?))
            .is_ok()
    );
    for (key, value) in [
        ("path", serde_json::json!("/tmp/secret")),
        ("flags", serde_json::json!(["--verbose"])),
        ("profile", serde_json::json!("release")),
        ("warm_up_ms", serde_json::json!(1)),
        ("sample_size", serde_json::json!(10)),
        ("bench_target", serde_json::json!("../escape")),
        ("bench_target", serde_json::json!("a bench")),
        ("bench_target", serde_json::json!("")),
        ("package", serde_json::json!("member/../other")),
        ("features", serde_json::json!(["a/b"])),
        ("features", serde_json::json!(vec!["f"; 17])),
        ("run_count", serde_json::json!(0)),
        ("run_count", serde_json::json!(4)),
        ("timeout_seconds", serde_json::json!(0)),
        ("timeout_seconds", serde_json::json!(901)),
        ("execution_mode", serde_json::json!("background")),
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
fn call_boundary_covers_the_task_gate_and_the_missing_runtime() -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    runtime.block_on(async {
        let tool = BenchmarkTool::new()?;
        let task = tool
            .call_with_token(
                CallToolRequestParams::new(NAME)
                    .with_arguments(arguments(&[("execution_mode", serde_json::json!("task"))])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await?;
        assert_eq!(
            task.structured_content.ok_or("content")?["error_code"],
            "TASKS_REQUIRED"
        );
        let error = tool
            .call_with_token(
                CallToolRequestParams::new(NAME).with_arguments(arguments(&[])?),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
            .err()
            .ok_or("runtime must be required")?;
        assert_eq!(error.message, "Benchmark runtime is not configured");
        Ok::<_, Box<dyn std::error::Error>>(())
    })
}

#[test]
fn operational_errors_have_closed_status_and_codes() -> TestResult {
    assert_common_error_contract!(BenchmarkTool::new()?, error);
    Ok(())
}

#[test]
fn result_encoding_separates_measurement_failure_and_partial_evidence() -> TestResult {
    let tool = BenchmarkTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let criterion = || HarnessDetection::Criterion {
        version: APPROVED_CRITERION_VERSION.into(),
    };
    let cases: Vec<(PublishedBenchmark, &str, serde_json::Value)> = vec![
        (
            fixtures::published(
                fixtures::observation(
                    criterion(),
                    BenchmarkExit::Passed,
                    Some(fixtures::dataset(2, 16, MeasurementCompleteness::Complete)?),
                    None,
                )?,
                ArtifactCompleteness::Complete,
            )?,
            "passed",
            serde_json::Value::Null,
        ),
        (
            fixtures::published(
                fixtures::observation(
                    criterion(),
                    BenchmarkExit::BenchmarkFailed,
                    None,
                    Some(DatasetOmission::ExecutionFailed),
                )?,
                ArtifactCompleteness::Complete,
            )?,
            "failed",
            serde_json::json!("OBSERVED_FAILURE"),
        ),
        (
            fixtures::published(
                fixtures::observation(
                    HarnessDetection::Unrecognized,
                    BenchmarkExit::Passed,
                    None,
                    Some(DatasetOmission::HarnessUnrecognized),
                )?,
                ArtifactCompleteness::Complete,
            )?,
            "failed",
            serde_json::json!("HARNESS_UNRECOGNIZED"),
        ),
        (
            fixtures::published(
                fixtures::observation(
                    HarnessDetection::CriterionUnapproved {
                        version: "0.5.1".into(),
                    },
                    BenchmarkExit::Passed,
                    None,
                    Some(DatasetOmission::HarnessUnapproved),
                )?,
                ArtifactCompleteness::Complete,
            )?,
            "failed",
            serde_json::json!("HARNESS_UNAPPROVED"),
        ),
        (
            fixtures::published(
                fixtures::observation(
                    criterion(),
                    BenchmarkExit::Passed,
                    Some(fixtures::dataset(
                        1,
                        16,
                        MeasurementCompleteness::Truncated,
                    )?),
                    None,
                )?,
                ArtifactCompleteness::Partial,
            )?,
            "blocked",
            serde_json::json!("EVIDENCE_INCOMPLETE"),
        ),
    ];
    for (published, status, code) in cases {
        let encoded = tool.encode_result(&reference, published, 7)?;
        let value = encoded.structured_content.ok_or("content")?;
        assert_eq!(value["status"], status);
        assert_eq!(value["error_code"], code);
        assert_eq!(value["duration_ms"], 7);
        // A raw sample never crosses this boundary; the dataset artifact owns it.
        let text = serde_json::to_string(&value)?;
        assert!(!text.contains("total_ns"), "raw samples leaked");
        assert!(!text.contains("iterations"), "raw samples leaked");
    }
    Ok(())
}

#[test]
fn published_measurements_are_ranked_and_described_without_raw_samples() -> TestResult {
    let tool = BenchmarkTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let published = fixtures::published(
        fixtures::observation(
            HarnessDetection::Criterion {
                version: APPROVED_CRITERION_VERSION.into(),
            },
            BenchmarkExit::Passed,
            Some(fixtures::dataset(3, 16, MeasurementCompleteness::Complete)?),
            None,
        )?,
        ArtifactCompleteness::Complete,
    )?;
    let value = tool
        .encode_result(&reference, published, 7)?
        .structured_content
        .ok_or("content")?;
    let benchmarks = value["data"]["observation"]["benchmarks"]
        .as_array()
        .ok_or("benchmarks")?;
    assert_eq!(benchmarks.len(), 3);
    let medians: Vec<f64> = benchmarks
        .iter()
        .filter_map(|row| row["median_per_iteration_ns"].as_f64())
        .collect();
    assert_eq!(medians.len(), 3);
    assert!(medians.windows(2).all(|pair| pair[0] >= pair[1]));
    assert_eq!(benchmarks[0]["samples"], 16);
    assert_eq!(benchmarks[0]["median_per_iteration_ns"], 1007.5);
    assert_eq!(benchmarks[0]["minimum_per_iteration_ns"], 1000.0);
    assert_eq!(benchmarks[0]["maximum_per_iteration_ns"], 1015.0);
    assert_eq!(benchmarks[0]["median_absolute_deviation_ns"], 4.0);
    assert_eq!(benchmarks[0]["outliers_counted"], 0);
    assert_eq!(value["data"]["artifacts"].as_array().map(Vec::len), Some(2));
    assert_eq!(value["data"]["artifacts"][0]["kind"], "benchmark_dataset");
    assert_eq!(value["data"]["artifacts"][1]["kind"], "criterion_archive");
    assert_eq!(
        value["data"]["observation"]["provenance"]["dataset_format"],
        rust_engineering_domain::benchmark::BENCHMARK_DATASET_FORMAT
    );
    Ok(())
}

#[test]
fn descriptive_statistics_describe_the_sample_set_they_are_given() {
    assert_eq!(median(&[]), 0.0);
    assert_eq!(median(&[1.0]), 1.0);
    assert_eq!(median(&[1.0, 3.0]), 2.0);
    assert_eq!(median(&[1.0, 2.0, 30.0]), 2.0);
    assert_eq!(median_absolute_deviation(&[]), 0.0);
    assert_eq!(median_absolute_deviation(&[1.0, 2.0, 3.0]), 1.0);
    assert_eq!(quantile(&[], 0.5), 0.0);
    assert_eq!(quantile(&[1.0, 2.0, 3.0, 4.0], 0.25), 1.75);
    // Fewer than four values cannot place a fence.
    assert_eq!(tukey_outliers(&[1.0, 2.0, 3.0]), 0);
    let mut values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 1_000.0];
    values.sort_unstable_by(f64::total_cmp);
    assert_eq!(tukey_outliers(&values), 1);
}
