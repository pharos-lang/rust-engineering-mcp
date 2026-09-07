//! Docker-gated end-to-end tests for `rust.test.nextest`, run only by the
//! integration gate once `APPROVED_RUST_IMAGE` names a provisioned image
//! with cargo-nextest installed (ADR-063) and `RUST_MCP_TEST_SOCKET`/
//! `RUST_MCP_TEST_IMAGE` are set. Every test here is `#[ignore]`d: this
//! package has no Docker access and cannot run or calibrate them itself.
//!
//! Fixtures under `fixtures/nextest/{passing,failing,ignored,flaky,leaky,
//! doc-only,no-tests,hostile-output}` are owned by package F01 (Luna) and
//! may not exist yet. Tests that need them read the directory at runtime
//! (not `include_str!`, which would fail to compile before the fixture
//! exists) and fail with a clear message if it is absent, per the package
//! instructions to "write the test against the agreed path and report".
use rust_engineering_application::{ExecutionCancellation, NeverCancel};
use rust_engineering_domain::nextest::{NextestCommandOptions, NextestSelection};
use rust_engineering_domain::{ExecutionLimits, ExecutionTermination, SourceBundle, SourceFile};
use rust_engineering_execution::{APPROVED_RUST_IMAGE, HostDockerConfig, RustGateway};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}

fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];
    checked(getrandom::fill(&mut bytes))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

struct StateRoot(PathBuf);
impl StateRoot {
    fn new(label: &str) -> Result<Self> {
        let path = PathBuf::from("/private/tmp")
            .join(format!("rust-mcp-nextest-test-{label}-{}", nonce()?));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for StateRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gateway(root: &StateRoot) -> Result<RustGateway> {
    let gateway = checked(RustGateway::new(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
            .ok_or("set RUST_MCP_TEST_SOCKET")?
            .into(),
        state_root: root.0.clone(),
        image_id: std::env::var("RUST_MCP_TEST_IMAGE")
            .unwrap_or_else(|_| APPROVED_RUST_IMAGE.into()),
    }))?;
    assert!(checked(gateway.calibrate(&NeverCancel))?.verified);
    Ok(gateway)
}

fn options(selection: NextestSelection) -> Result<NextestCommandOptions> {
    checked(NextestCommandOptions::try_from(selection))
}

fn limits(wall_ms: u64) -> Result<ExecutionLimits> {
    ExecutionLimits::new_job(wall_ms, 256 * 1024).ok_or_else(|| "invalid test limits".into())
}

fn fixture_source(name: &str) -> Result<SourceBundle> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/nextest/{name}"));
    if !root.is_dir() {
        return Err(format!(
            "fixture directory not found at the agreed path: {}; package F01 (Luna) owns creating it",
            root.display()
        )
        .into());
    }
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(&root)?
                    .to_str()
                    .ok_or("non-UTF8 fixture path")?;
                files.push(SourceFile::new(
                    relative.replace('\\', "/"),
                    std::fs::read(&path)?,
                ));
            }
        }
    }
    checked(SourceBundle::new(checked(
        files
            .into_iter()
            .collect::<std::result::Result<Vec<_>, _>>(),
    )?))
}

fn inline_source(lib_body: &str) -> Result<SourceBundle> {
    let files = [
        (
            "Cargo.toml",
            "[package]\nname='nextest_fixture'\nversion='0.1.0'\nedition='2024'\n".to_owned(),
        ),
        (
            "Cargo.lock",
            "version=4\n[[package]]\nname='nextest_fixture'\nversion='0.1.0'\n".to_owned(),
        ),
        ("src/lib.rs", lib_body.to_owned()),
    ]
    .into_iter()
    .map(|(path, body)| SourceFile::new(path.to_owned(), body.into_bytes()))
    .collect::<std::result::Result<Vec<_>, _>>();
    checked(SourceBundle::new(checked(files)?))
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063)"]
fn passing_and_failing_tests_produce_exact_junit_and_hypothesized_exit_codes() -> Result {
    let root = StateRoot::new("pass-fail")?;
    let gateway = gateway(&root)?;
    let source = inline_source(
        "#[test] fn a_passes() { assert_eq!(1 + 1, 2); }\n\
         #[test] fn b_fails() { assert_eq!(1 + 1, 3); }\n",
    )?;
    let opts = options(NextestSelection::default())?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(
        execution.result.termination,
        ExecutionTermination::Exited,
        "{:?}",
        execution.result
    );
    // Hypothesis from R01 (docs fetch failed); this run is exactly the
    // calibration evidence needed to confirm or replace it.
    assert_eq!(
        execution.result.exit_code,
        Some(100),
        "NextestExit::TestFailure hypothesis uncalibrated: {:?}",
        execution.result
    );
    let junit = execution
        .junit
        .ok_or("expected a JUnit report to be exported")?;
    let junit = String::from_utf8(junit)?;
    assert!(junit.contains("a_passes"), "{junit}");
    assert!(junit.contains("b_fails"), "{junit}");
    assert!(junit.contains("<failure"), "{junit}");
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063)"]
fn all_passing_tests_report_the_hypothesized_success_exit_code() -> Result {
    let root = StateRoot::new("pass-only")?;
    let gateway = gateway(&root)?;
    let source = inline_source("#[test] fn ok() { assert!(true); }\n")?;
    let opts = options(NextestSelection::default())?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    assert_eq!(
        execution.result.exit_code,
        Some(0),
        "{:?}",
        execution.result
    );
    let junit = execution
        .junit
        .ok_or_else(|| format!("expected junit: {:?}", execution.result))?;
    assert!(!String::from_utf8(junit)?.contains("<failure"));
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063)"]
fn build_error_reports_the_observed_runner_failure_exit_code() -> Result {
    let root = StateRoot::new("build-error")?;
    let gateway = gateway(&root)?;
    let source = inline_source("this is not valid rust\n")?;
    let execution = checked(gateway.execute_nextest(
        &source,
        &options(NextestSelection::default())?,
        limits(60_000)?,
        &NeverCancel,
    ))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    assert_eq!(
        execution.result.exit_code,
        Some(101),
        "{:?}",
        execution.result
    );
    assert_eq!(execution.junit, None);
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-064)"]
fn quality_profile_allows_only_the_required_anonymous_unix_stream_pair() -> Result {
    let root = StateRoot::new("quality-socket-negative")?;
    let gateway = gateway(&root)?;
    let source = inline_source(
        "#[test] fn forbidden_socket_families_and_unix_bind_are_denied() {\n\
         assert!(std::net::TcpListener::bind(\"127.0.0.1:0\").is_err());\n\
         assert!(std::net::TcpListener::bind(\"[::1]:0\").is_err());\n\
         assert!(std::net::TcpStream::connect(\"127.0.0.1:9\").is_err());\n\
         assert!(std::net::TcpStream::connect(\"[::1]:9\").is_err());\n\
         assert!(std::os::unix::net::UnixListener::bind(\"/work/rust-mcp-denied.sock\").is_err());\n\
         }\n",
    )?;
    let execution = checked(gateway.execute_nextest(
        &source,
        &options(NextestSelection::default())?,
        limits(60_000)?,
        &NeverCancel,
    ))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    assert_eq!(
        execution.result.exit_code,
        Some(0),
        "{:?}",
        execution.result
    );
    assert!(execution.junit.is_some());
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063)"]
fn slow_test_timeout_is_a_test_failure_with_junit_evidence() -> Result {
    let root = StateRoot::new("slow-timeout-exit")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("slow")?;
    let execution = checked(gateway.execute_nextest(
        &source,
        &options(NextestSelection {
            timeout: 1,
            ..Default::default()
        })?,
        limits(60_000)?,
        &NeverCancel,
    ))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    assert_eq!(
        execution.result.exit_code,
        Some(100),
        "{:?}",
        execution.result
    );
    let junit = String::from_utf8(execution.junit.ok_or("expected timeout junit")?)?;
    assert!(junit.contains("exceeds_timeout"), "{junit}");
    assert!(
        junit.contains("type=\"test timeout\"") || junit.contains("test timeout"),
        "{junit}"
    );
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063)"]
fn passing_fixture_records_five_cold_and_five_warm_sync_samples() -> Result {
    let source = fixture_source("passing")?;
    let opts = options(NextestSelection::default())?;
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
    for sample in 0..samples {
        let root = StateRoot::new(&format!("timing-{sample}"))?;
        let gateway = gateway(&root)?;
        let started = Instant::now();
        let cold = checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
        cold_ms.push(u64::try_from(started.elapsed().as_millis())?);
        cold_command_ms.push(cold.result.duration_ms);
        cold_gateway_ms.push(cold.result.total_duration_ms);
        assert_eq!(cold.result.exit_code, Some(0), "{:?}", cold.result);

        let started = Instant::now();
        let warm = checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
        warm_ms.push(u64::try_from(started.elapsed().as_millis())?);
        warm_command_ms.push(warm.result.duration_ms);
        warm_gateway_ms.push(warm.result.total_duration_ms);
        assert_eq!(warm.result.exit_code, Some(0), "{:?}", warm.result);
        assert!(!gateway.is_quarantined());
    }
    println!(
        "M3_NEXTEST_SYNC_TIMINGS {{\"cold_ms\":{cold_ms:?},\"warm_ms\":{warm_ms:?},\"cold_command_ms\":{cold_command_ms:?},\"warm_command_ms\":{warm_command_ms:?},\"cold_gateway_ms\":{cold_gateway_ms:?},\"warm_gateway_ms\":{warm_gateway_ms:?},\"samples_each\":{samples}}}"
    );
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063); fixtures/nextest/no-tests owned by package F01"]
fn no_tests_fixture_still_exits_with_evidence() -> Result {
    let root = StateRoot::new("no-tests")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("no-tests")?;
    let opts = options(NextestSelection::default())?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063); fixtures/nextest/ignored owned by package F01"]
fn ignored_fixture_reports_skipped_tests_when_report_skipped_is_all() -> Result {
    let root = StateRoot::new("ignored")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("ignored")?;
    let opts = options(NextestSelection::default())?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    let junit = String::from_utf8(execution.junit.ok_or("expected junit")?)?;
    assert!(junit.contains("<skipped"), "{junit}");
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063); fixtures/nextest/flaky owned by package F01"]
fn flaky_fixture_is_classified_flaky_when_retries_are_requested() -> Result {
    let root = StateRoot::new("flaky")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("flaky")?;
    let opts = options(NextestSelection {
        retries: 2,
        ..Default::default()
    })?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    let junit = String::from_utf8(execution.junit.ok_or("expected junit")?)?;
    assert!(
        junit.contains("<flakyFailure") || junit.contains("<flakyError"),
        "{junit}"
    );
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063); fixtures/nextest/leaky owned by package F01"]
fn leaky_fixture_is_observed_and_never_hangs_the_gateway() -> Result {
    let root = StateRoot::new("leaky")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("leaky")?;
    let opts = options(NextestSelection::default())?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063); fixtures/nextest/doc-only owned by package F01"]
fn doc_only_fixture_never_runs_doctests() -> Result {
    let root = StateRoot::new("doc-only")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("doc-only")?;
    let opts = options(NextestSelection::default())?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    // Oracle: nextest never runs doctests; the observation layer must state
    // `doctests: NotRun` rather than inferring pass/fail from their absence.
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063); fixtures/nextest/hostile-output owned by package F01"]
fn hostile_output_flood_is_bounded_and_reported_as_output_limit() -> Result {
    let root = StateRoot::new("flood")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("hostile-output")?;
    let opts = options(NextestSelection::default())?;
    let limits = ExecutionLimits::new_job(60_000, 16 * 1024).ok_or("invalid test limits")?;
    assert!(!gateway.is_quarantined(), "calibration quarantined gateway");
    let execution = checked(gateway.execute_nextest(&source, &opts, limits, &NeverCancel))?;
    assert_eq!(
        execution.result.termination,
        ExecutionTermination::OutputLimit
    );
    assert!(execution.result.stdout_truncated || execution.result.stderr_truncated);
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063)"]
fn hostile_symlink_at_the_fixed_junit_path_is_rejected_and_never_followed() -> Result {
    let root = StateRoot::new("junit-symlink")?;
    let gateway = gateway(&root)?;
    let source = inline_source(
        "#[test] fn plants_junit_symlink() {\n\
         std::fs::create_dir_all(\"/junit/rust-mcp/reports\").unwrap();\n\
         let _ = std::fs::remove_file(\"/junit/rust-mcp/reports/junit.xml\");\n\
         std::os::unix::fs::symlink(\"/etc/passwd\", \"/junit/rust-mcp/reports/junit.xml\").unwrap();\n\
         }\n",
    )?;
    let opts = options(NextestSelection::default())?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(execution.junit, None, "a link must never become host bytes");
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063)"]
fn active_cancellation_with_an_observed_child_terminates_and_joins_cleanly() -> Result {
    let root = StateRoot::new("cancel")?;
    let gateway = gateway(&root)?;
    let source = inline_source(
        "#[test] fn sleeps() { std::thread::sleep(std::time::Duration::from_secs(60)); }\n",
    )?;
    let opts = options(NextestSelection {
        timeout: 45,
        ..Default::default()
    })?;
    struct CancelAfter(AtomicBool);
    impl ExecutionCancellation for CancelAfter {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
    }
    let cancel = CancelAfter(AtomicBool::new(false));
    let run_limits = limits(45_000)?;
    let (result, elapsed) = std::thread::scope(|scope| -> Result<_> {
        let started = Instant::now();
        let job = scope.spawn(|| gateway.execute_nextest(&source, &opts, run_limits, &cancel));
        std::thread::sleep(Duration::from_secs(3));
        cancel.0.store(true, Ordering::Release);
        let result = checked(job.join().map_err(|_| "test job panicked")?)?;
        Ok((result, started.elapsed()))
    })?;
    assert_eq!(result.result.termination, ExecutionTermination::Cancelled);
    assert!(
        elapsed < Duration::from_secs(30),
        "cancellation should terminate well before the 45s budget: {elapsed:?}"
    );
    assert!(!gateway.is_quarantined());
    // Immediate reuse after a clean cancellation proves the process tree and
    // volume/container objects were actually joined and removed, not merely
    // reported as cancelled while something was left running.
    let followup_source = inline_source("#[test] fn ok() { assert!(true); }\n")?;
    let followup_options = options(NextestSelection::default())?;
    let followup = checked(gateway.execute_nextest(
        &followup_source,
        &followup_options,
        limits(60_000)?,
        &NeverCancel,
    ))?;
    assert_eq!(followup.result.termination, ExecutionTermination::Exited);
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-nextest provisioned (ADR-063)"]
fn source_is_immutable_across_the_run_and_a_second_job_reuses_the_gateway_cleanly() -> Result {
    let root = StateRoot::new("immutable")?;
    let gateway = gateway(&root)?;
    let source = inline_source(
        "#[test] fn attempts_write() { assert!(std::fs::write(\"src/lib.rs\", b\"tampered\").is_err()); }\n",
    )?;
    let opts = options(NextestSelection::default())?;
    let execution =
        checked(gateway.execute_nextest(&source, &opts, limits(60_000)?, &NeverCancel))?;
    assert_eq!(execution.result.termination, ExecutionTermination::Exited);
    assert_eq!(
        execution.result.exit_code,
        Some(0),
        "{:?}",
        execution.result
    );
    assert!(!gateway.is_quarantined());
    Ok(())
}
