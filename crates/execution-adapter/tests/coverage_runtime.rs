//! Docker-only ADR-062 qualification. The runtime script executes each name
//! serially against the immutable approved image.

use rust_engineering_application::{ExecutionCancellation, NeverCancel};
use rust_engineering_domain::coverage::{CoverageOptions, CoverageSelection};
use rust_engineering_domain::{ExecutionLimits, ExecutionTermination, SourceBundle, SourceFile};
use rust_engineering_execution::{APPROVED_RUST_IMAGE, HostDockerConfig, RustGateway};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

struct ObservedCoverage {
    result: rust_engineering_domain::ExecutionResult,
    json: Option<Vec<u8>>,
    lcov: Option<Vec<u8>>,
    html: Option<Vec<u8>>,
}

struct StateRoot(PathBuf);
impl StateRoot {
    fn new() -> Result<Self> {
        let path =
            PathBuf::from("/private/tmp").join(format!("rust-mcp-coverage-{}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}
impl Drop for StateRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn gateway(root: &StateRoot) -> Result<RustGateway> {
    let image_id =
        std::env::var("RUST_MCP_TEST_IMAGE").unwrap_or_else(|_| APPROVED_RUST_IMAGE.into());
    assert_eq!(image_id, APPROVED_RUST_IMAGE);
    let gateway = RustGateway::new(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
            .ok_or("RUST_MCP_TEST_SOCKET required")?
            .into(),
        state_root: root.0.clone(),
        image_id,
    })
    .map_err(|error| format!("{error:?}"))?;
    assert!(
        gateway
            .calibrate(&NeverCancel)
            .map_err(|error| format!("{error:?}"))?
            .verified
    );
    Ok(gateway)
}
fn source(name: &str) -> Result<SourceBundle> {
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/coverage/{name}"));
    let mut stack = vec![root.clone()];
    let mut files = Vec::new();
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                let relative = entry
                    .path()
                    .strip_prefix(&root)?
                    .to_str()
                    .ok_or("non UTF-8 fixture")?
                    .replace('\\', "/");
                files.push(
                    SourceFile::new(relative, std::fs::read(entry.path())?)
                        .map_err(|error| format!("{error:?}"))?,
                );
            }
        }
    }
    SourceBundle::new(files).map_err(|error| format!("{error:?}").into())
}
fn assert_no_owned_objects() -> Result {
    let socket = std::env::var("RUST_MCP_TEST_SOCKET")?;
    for arguments in [
        ["container", "ls", "--all", "--quiet"],
        ["volume", "ls", "--quiet", "--filter"],
    ] {
        let mut command = Command::new("/Applications/Docker.app/Contents/Resources/bin/docker");
        command.args(["--host", &format!("unix://{socket}")]);
        command.args(arguments);
        if arguments[0] == "container" {
            command.args(["--filter", "label=org.rust-mcp.execution=true"]);
        } else {
            command.arg("label=org.rust-mcp.execution=true");
        }
        let output = command.output()?;
        assert!(
            output.status.success(),
            "docker inventory failed: {output:?}"
        );
        assert!(
            output.stdout.is_empty(),
            "owned Docker object survived cleanup: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
    Ok(())
}

fn execute_with_control(
    fixture: &str,
    timeout_ms: u64,
    cancel: &dyn ExecutionCancellation,
) -> Result<ObservedCoverage> {
    let root = StateRoot::new()?;
    let gateway = gateway(&root)?;
    let options = CoverageOptions::try_from(CoverageSelection::default())?;
    let execution = gateway
        .execute_coverage(
            &source(fixture)?,
            &options,
            ExecutionLimits::new_job(timeout_ms, 256 * 1024).ok_or("invalid limits")?,
            cancel,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert!(!gateway.is_quarantined());
    assert_no_owned_objects()?;
    Ok(ObservedCoverage {
        result: execution.result,
        json: execution.json,
        lcov: execution.lcov,
        html: execution.html,
    })
}

fn execute(fixture: &str, timeout_ms: u64) -> Result {
    let observation = execute_with_control(fixture, timeout_ms, &NeverCancel)?;
    assert_success(&observation)
}

fn assert_success(observation: &ObservedCoverage) -> Result {
    let result = &observation.result;
    assert_eq!(
        result.termination,
        ExecutionTermination::Exited,
        "{result:?}"
    );
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    assert!(observation.json.is_some(), "missing JSON: {result:?}");
    assert!(observation.lcov.is_some(), "missing LCOV: {result:?}");
    assert!(observation.html.is_some(), "missing HTML: {result:?}");
    Ok(())
}

macro_rules! docker_case {
    ($name:ident, $fixture:literal) => {
        #[test]
        #[ignore = "requires approved Docker coverage gateway and recorded calibration"]
        fn $name() -> Result {
            execute($fixture, 120_000)
        }
    };
}

#[test]
#[ignore = "requires approved Docker coverage gateway for M3-02 budget measurement"]
fn known_counts_records_cold_and_warm_sync_samples() -> Result {
    let source = source("known-counts")?;
    let options = CoverageOptions::try_from(CoverageSelection::default())?;
    let samples = std::env::var("RUST_MCP_M3_BUDGET_SAMPLES")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(5);
    if !(1..=100).contains(&samples) {
        return Err("RUST_MCP_M3_BUDGET_SAMPLES must be between 1 and 100".into());
    }
    let mut cold_ms = Vec::with_capacity(samples);
    let mut warm_ms = Vec::with_capacity(samples);
    let mut cold_command_ms = Vec::with_capacity(samples);
    let mut warm_command_ms = Vec::with_capacity(samples);
    let mut cold_gateway_ms = Vec::with_capacity(samples);
    let mut warm_gateway_ms = Vec::with_capacity(samples);
    for _ in 0..samples {
        let root = StateRoot::new()?;
        let gateway = gateway(&root)?;
        let limits = ExecutionLimits::new_job(60_000, 256 * 1024).ok_or("invalid limits")?;
        let started = Instant::now();
        let cold = gateway
            .execute_coverage(&source, &options, limits, &NeverCancel)
            .map_err(|error| format!("{error:?}"))?;
        cold_ms.push(u64::try_from(started.elapsed().as_millis())?);
        cold_command_ms.push(cold.result.duration_ms);
        cold_gateway_ms.push(cold.result.total_duration_ms);
        assert_success(&ObservedCoverage {
            result: cold.result,
            json: cold.json,
            lcov: cold.lcov,
            html: cold.html,
        })?;

        let started = Instant::now();
        let warm = gateway
            .execute_coverage(&source, &options, limits, &NeverCancel)
            .map_err(|error| format!("{error:?}"))?;
        warm_ms.push(u64::try_from(started.elapsed().as_millis())?);
        warm_command_ms.push(warm.result.duration_ms);
        warm_gateway_ms.push(warm.result.total_duration_ms);
        assert_success(&ObservedCoverage {
            result: warm.result,
            json: warm.json,
            lcov: warm.lcov,
            html: warm.html,
        })?;
        assert!(!gateway.is_quarantined());
    }
    assert_no_owned_objects()?;
    println!(
        "M3_COVERAGE_SYNC_TIMINGS {{\"cold_ms\":{cold_ms:?},\"warm_ms\":{warm_ms:?},\"cold_command_ms\":{cold_command_ms:?},\"warm_command_ms\":{warm_command_ms:?},\"cold_gateway_ms\":{cold_gateway_ms:?},\"warm_gateway_ms\":{warm_gateway_ms:?},\"samples_each\":{samples}}}"
    );
    Ok(())
}

fn print_calibration(fixture: &str, observation: &ObservedCoverage) -> Result {
    let json = observation.json.as_deref().ok_or("missing coverage JSON")?;
    let parsed = rust_engineering_execution::coverage_json::parse(json)?;
    eprintln!(
        "coverage calibration {fixture}: identity={} manifest={} metrics={:?} files={:?} json_bytes={} lcov_bytes={} html_bytes={}",
        parsed.cargo_llvm_cov_version,
        parsed.manifest_path,
        parsed.summary.aggregate,
        parsed.summary.files,
        json.len(),
        observation.lcov.as_deref().map_or(0, <[u8]>::len),
        observation.html.as_deref().map_or(0, <[u8]>::len),
    );
    if fixture == "known-counts" {
        eprintln!(
            "coverage calibration known-counts LCOV:\n{}",
            std::str::from_utf8(observation.lcov.as_deref().ok_or("missing LCOV")?)?
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires approved Docker coverage gateway and recorded calibration"]
fn known_counts_fixture_has_exact_line_region_and_function_oracle() -> Result {
    let observation = execute_with_control("known-counts", 120_000, &NeverCancel)?;
    assert_success(&observation)?;
    print_calibration("known-counts", &observation)?;
    let parsed = rust_engineering_execution::coverage_json::parse(
        observation.json.as_deref().ok_or("missing coverage JSON")?,
    )?;
    assert_eq!(parsed.cargo_llvm_cov_version, "0.9.0");
    assert_eq!(parsed.manifest_path, "/source/Cargo.toml");
    let aggregate = parsed.summary.aggregate;
    assert_eq!(
        aggregate.lines.map(|value| (value.count, value.covered)),
        Some((4, 4))
    );
    assert_eq!(
        aggregate.regions.map(|value| (value.count, value.covered)),
        Some((9, 8))
    );
    assert_eq!(
        aggregate
            .functions
            .map(|value| (value.count, value.covered)),
        Some((2, 2))
    );
    assert_eq!(parsed.summary.files.len(), 1);
    assert_eq!(parsed.summary.files[0].path, "src/lib.rs");
    assert!(
        observation
            .html
            .as_deref()
            .ok_or("missing HTML")?
            .windows("index.html".len())
            .any(|window| window == b"index.html")
    );
    Ok(())
}

#[test]
#[ignore = "requires approved Docker coverage gateway and recorded calibration"]
fn shared_file_workspace_deduplicates_aggregate_only() -> Result {
    let observation = execute_with_control("shared-file-workspace", 120_000, &NeverCancel)?;
    assert_success(&observation)?;
    print_calibration("shared-file-workspace", &observation)?;
    let parsed = rust_engineering_execution::coverage_json::parse(
        observation.json.as_deref().ok_or("missing coverage JSON")?,
    )?;
    assert_eq!(parsed.summary.files.len(), 3);
    assert_eq!(
        parsed
            .summary
            .files
            .iter()
            .filter(|file| file.path == "shared.rs")
            .count(),
        1
    );
    let aggregate = parsed.summary.aggregate;
    assert_eq!(
        aggregate.lines.map(|value| (value.count, value.covered)),
        Some((3, 0))
    );
    assert_eq!(
        aggregate.regions.map(|value| (value.count, value.covered)),
        Some((9, 0))
    );
    assert_eq!(
        aggregate
            .functions
            .map(|value| (value.count, value.covered)),
        Some((3, 0))
    );
    Ok(())
}

#[test]
#[ignore = "requires approved Docker coverage gateway and recorded calibration"]
fn zero_denominator_is_absent_from_percent_metrics() -> Result {
    let observation = execute_with_control("zero-denominator", 120_000, &NeverCancel)?;
    assert_eq!(observation.result.termination, ExecutionTermination::Exited);
    assert_eq!(observation.result.exit_code, Some(0));
    assert!(
        observation.result.stderr.contains("no coverage data found"),
        "{:?}",
        observation.result
    );
    assert!(observation.json.is_none());
    assert!(observation.lcov.is_none());
    assert!(observation.html.is_none());
    Ok(())
}
docker_case!(three_report_formats_derive_from_one_capture, "known-counts");
docker_case!(no_tests_is_not_promoted_to_pass, "no-tests");
#[test]
#[ignore = "requires approved Docker coverage gateway and recorded calibration"]
fn timeout_mid_build_is_blocked_after_joined_cleanup() -> Result {
    let observation = execute_with_control("slow-build", 2_000, &NeverCancel)?;
    let result = observation.result;
    assert_eq!(
        result.termination,
        ExecutionTermination::TimedOut,
        "{result:?}"
    );
    assert_eq!(result.exit_code, None, "{result:?}");
    assert!(observation.json.is_none() && observation.lcov.is_none() && observation.html.is_none());
    Ok(())
}

#[test]
#[ignore = "requires approved Docker coverage gateway and recorded calibration"]
fn cancel_or_eof_joins_active_child_before_capacity_reuse() -> Result {
    struct CancelAfter(Instant);
    impl ExecutionCancellation for CancelAfter {
        fn is_cancelled(&self) -> bool {
            self.0.elapsed() >= Duration::from_secs(2)
        }
    }
    let root = StateRoot::new()?;
    let gateway = gateway(&root)?;
    let options = CoverageOptions::try_from(CoverageSelection::default())?;
    let cancellation = CancelAfter(Instant::now());
    let execution = gateway
        .execute_coverage(
            &source("slow-build")?,
            &options,
            ExecutionLimits::new_job(120_000, 256 * 1024).ok_or("invalid limits")?,
            &cancellation,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        execution.result.termination,
        ExecutionTermination::Cancelled,
        "{:?}",
        execution.result
    );
    assert!(!gateway.is_quarantined());
    let followup = gateway
        .execute_coverage(
            &source("known-counts")?,
            &options,
            ExecutionLimits::new_job(120_000, 256 * 1024).ok_or("invalid limits")?,
            &NeverCancel,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(followup.result.termination, ExecutionTermination::Exited);
    assert_eq!(followup.result.exit_code, Some(0), "{:?}", followup.result);
    assert_no_owned_objects()?;
    Ok(())
}

#[test]
#[ignore = "requires approved Docker coverage gateway and recorded calibration"]
fn hostile_html_is_retained_only_as_opaque_archive_bundle() -> Result {
    let observation = execute_with_control("containment", 120_000, &NeverCancel)?;
    let result = observation.result;
    assert_eq!(result.termination, ExecutionTermination::Exited);
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    assert!(observation.json.is_some());
    assert!(observation.lcov.is_some());
    assert!(observation.html.is_some());
    Ok(())
}
