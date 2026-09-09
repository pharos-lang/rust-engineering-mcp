use super::tests::{TestResult, fixtures};
use super::*;
use rust_engineering_domain::benchmark::{
    APPROVED_CRITERION_VERSION, BenchmarkHarness, BenchmarkIdentity, BenchmarkProvenance,
    HardwareProfile, RawSample, ResourceQuotas, SampleUnit,
};

const MAX_RESULT_BYTES: usize = 512 * 1024;

/// A dataset whose bounded text fields are all at the domain's 512-byte
/// ceiling, so the complete response is larger than the wire budget while every
/// value in it stays valid.
fn wide_dataset(keys: usize) -> TestResult<BenchmarkDataset> {
    let filler = "a".repeat(508);
    let measurements = (0..keys)
        .map(|index| {
            let identity = BenchmarkIdentity::new(
                format!("{filler}{index:04}"),
                Some(format!("{filler}{index:04}")),
                Some(format!("{filler}{index:04}")),
                format!("{filler}{index:04}"),
                format!("bench-{index:04}"),
            )?;
            let samples = (0..24)
                .map(|sample| RawSample::new(1, 1_000.0 + f64::from(index as u32 + sample), 1))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(BenchmarkMeasurement::new(
                identity,
                SamplingMode::Flat,
                samples,
                3_000,
                5_000,
                24,
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
            selection: fixtures::selection(),
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

fn published(keys: usize) -> TestResult<PublishedBenchmark> {
    fixtures::published(
        fixtures::observation(
            HarnessDetection::Criterion {
                version: APPROVED_CRITERION_VERSION.into(),
            },
            BenchmarkExit::Passed,
            Some(wide_dataset(keys)?),
            None,
        )?,
        ArtifactCompleteness::Complete,
    )
}

#[test]
fn encode_result_trims_the_lowest_ranked_rows_into_the_wire_budget() -> TestResult {
    let tool = BenchmarkTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let result = published(MAX_RESPONSE_BENCHMARKS)?;
    let artifacts = result
        .artifacts
        .iter()
        .map(|descriptor| artifact(&reference, descriptor))
        .collect::<Result<Vec<_>, _>>()?;
    let untrimmed = tool.contract.encode(Output {
        outcome: Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(Data {
                project_ref: reference.to_string(),
                semantics: "observed_measurement_of_one_execution_on_one_host_without_attribution",
                observation: observation(&result.observation, true),
                artifacts,
            }),
        },
        summary: "Observed benchmark measurement; no causal or portable claim",
        duration_ms: 7,
    })?;
    assert!(serde_json::to_vec(&untrimmed)?.len() > MAX_RESULT_BYTES);

    let encoded = tool.encode_result(&reference, result, 7)?;
    let wire = serde_json::to_vec(&encoded)?;
    assert!(wire.len() <= MAX_RESULT_BYTES, "{}", wire.len());
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["error_code"], "EVIDENCE_INCOMPLETE");
    assert_eq!(value["data"]["observation"]["complete"], false);
    let omitted = value["data"]["observation"]["benchmarks_omitted"]
        .as_u64()
        .ok_or("benchmarks_omitted")?;
    assert!(omitted > 0);
    let kept = value["data"]["observation"]["benchmarks"]
        .as_array()
        .map(Vec::len)
        .ok_or("benchmarks")?;
    assert_eq!(kept as u64 + omitted, MAX_RESPONSE_BENCHMARKS as u64);
    // The survivors are the highest-ranked rows: trimming drops from the tail.
    let medians: Vec<f64> = value["data"]["observation"]["benchmarks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["median_per_iteration_ns"].as_f64())
        .collect();
    assert!(medians.windows(2).all(|pair| pair[0] >= pair[1]));
    Ok(())
}

#[test]
fn a_small_complete_measurement_stays_passed_and_untrimmed() -> TestResult {
    let tool = BenchmarkTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let encoded = tool.encode_result(&reference, published(2)?, 7)?;
    assert!(serde_json::to_vec(&encoded)?.len() <= MAX_RESULT_BYTES);
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "passed");
    assert_eq!(value["data"]["observation"]["complete"], true);
    assert_eq!(value["data"]["observation"]["benchmarks_omitted"], 0);
    Ok(())
}
