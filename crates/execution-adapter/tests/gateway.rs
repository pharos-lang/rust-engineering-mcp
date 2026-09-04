use rust_engineering_application::{ExecutionCancellation, ExecutionPort, NeverCancel};
use rust_engineering_domain::{
    ExecutionLimits, ExecutionSpec, ExecutionTermination, ProbeScenario,
};
use rust_engineering_execution::{DockerGateway, HostDockerConfig};
use std::path::PathBuf;
use std::time::{Duration, Instant};

type Result<T = ()> = std::result::Result<T, String>;
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|e| format!("{e:?}"))
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self> {
        let mut bytes = [0; 16];
        checked(getrandom::fill(&mut bytes))?;
        let path = checked(std::env::temp_dir().canonicalize())?.join(format!(
            "rust-mcp-gateway-test-{:x}",
            u128::from_le_bytes(bytes)
        ));
        checked(std::fs::create_dir(&path))?;
        Ok(Self(path))
    }
    fn gateway(&self) -> Result<DockerGateway> {
        checked(DockerGateway::new(HostDockerConfig {
            executable: PathBuf::from("/Applications/Docker.app/Contents/Resources/bin/docker"),
            socket: PathBuf::from(
                std::env::var("RUST_MCP_TEST_SOCKET").map_err(|_| "set RUST_MCP_TEST_SOCKET")?,
            ),
            state_root: self.0.clone(),
            image_id: std::env::var("RUST_MCP_TEST_IMAGE")
                .map_err(|_| "set RUST_MCP_TEST_IMAGE")?,
        }))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn spec(scenario: ProbeScenario, wall: u64, output: usize) -> Result<ExecutionSpec> {
    Ok(ExecutionSpec {
        scenario,
        limits: ExecutionLimits::new(wall, output).ok_or("invalid test limits")?,
    })
}

#[test]
fn configuration_rejects_noncanonical_image_and_paths_before_io() {
    for image in [
        "rust:latest",
        "sha256:bad",
        &format!("sha256:{}", "A".repeat(64)),
    ] {
        let result = DockerGateway::new(HostDockerConfig {
            executable: "relative".into(),
            socket: "/missing".into(),
            state_root: "/missing".into(),
            image_id: image.into(),
        });
        assert!(matches!(
            result,
            Err(rust_engineering_application::ExecutionError::InvalidConfiguration)
        ));
    }
}

#[test]
#[ignore = "explicit local Docker image/socket and macOS/APFS required"]
fn successful_and_failed_processes_are_normal_results_with_stable_identity() -> Result {
    let fixture = Fixture::new()?;
    let gateway = fixture.gateway()?;
    let first =
        checked(gateway.execute(&spec(ProbeScenario::Success, 10000, 65536)?, &NeverCancel))?;
    assert_eq!(first.termination, ExecutionTermination::Exited);
    assert_eq!(first.exit_code, Some(0));
    assert!(first.stdout.contains("completed"), "{first:?}");
    assert!(first.stderr.is_empty(), "{first:?}");
    let second =
        checked(gateway.execute(&spec(ProbeScenario::Success, 10000, 65536)?, &NeverCancel))?;
    assert_eq!(first.execution_fingerprint, second.execution_fingerprint);
    let failed =
        checked(gateway.execute(&spec(ProbeScenario::Exit7, 10000, 65536)?, &NeverCancel))?;
    assert_eq!(failed.termination, ExecutionTermination::Exited);
    assert_eq!(failed.exit_code, Some(7));
    assert_ne!(first.execution_fingerprint, failed.execution_fingerprint);
    assert!(!gateway.is_quarantined());
    drop(gateway);
    assert_eq!(checked(std::fs::read_dir(&fixture.0))?.count(), 0);
    Ok(())
}

#[test]
#[ignore = "explicit local Docker image/socket and macOS/APFS required"]
fn timeout_cancellation_and_stream_limits_clean_up_the_container() -> Result {
    let fixture = Fixture::new()?;
    let gateway = fixture.gateway()?;
    let timed =
        checked(gateway.execute(&spec(ProbeScenario::Descendants, 700, 65536)?, &NeverCancel))?;
    assert_eq!(timed.termination, ExecutionTermination::TimedOut);
    assert!(timed.stdout.contains("descendant_started"), "{timed:?}");
    let records: Vec<serde_json::Value> = timed
        .stdout
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| e.to_string())?;
    let child = records
        .iter()
        .find(|r| r["event"] == "descendant_started")
        .ok_or("missing descendant")?;
    assert_eq!(child["details"]["double_fork"], true);
    assert!(records.iter().any(|r| r["event"] == "heartbeat"
        && r["details"]["parent_pid"] == 1
        && r["details"]["process_group"] == child["details"]["child_pid"]
        && r["details"]["process_group"] != child["details"]["parent_process_group"]));
    struct CancelAfter(Instant);
    impl ExecutionCancellation for CancelAfter {
        fn is_cancelled(&self) -> bool {
            self.0.elapsed() > Duration::from_millis(1000)
        }
    }
    let cancelled = checked(gateway.execute(
        &spec(ProbeScenario::Sleep, 10000, 65536)?,
        &CancelAfter(Instant::now()),
    ))?;
    assert_eq!(cancelled.termination, ExecutionTermination::Cancelled);
    let flood = checked(gateway.execute(&spec(ProbeScenario::Output, 10000, 4096)?, &NeverCancel))?;
    assert_eq!(flood.termination, ExecutionTermination::OutputLimit);
    assert!(flood.stdout_truncated || flood.stderr_truncated);
    assert!(flood.stdout.len() <= 4096 && flood.stderr.len() <= 4096);
    assert!(!gateway.is_quarantined());
    let recovered =
        checked(gateway.execute(&spec(ProbeScenario::Success, 10000, 4096)?, &NeverCancel))?;
    assert_eq!(recovered.exit_code, Some(0));
    Ok(())
}

#[test]
#[ignore = "explicit local Docker image/socket and macOS/APFS required"]
fn new_gateway_refuses_existing_labelled_container_and_cancel_cleans_it() -> Result {
    use rust_engineering_application::ExecutionError;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    let fixture = Fixture::new()?;
    let gateway = fixture.gateway()?;
    struct DuringCreate<'a> {
        fixture: &'a Fixture,
        calls: AtomicUsize,
        refused: AtomicBool,
    }
    impl ExecutionCancellation for DuringCreate<'_> {
        fn is_cancelled(&self) -> bool {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                self.refused.store(
                    matches!(self.fixture.gateway(),Err(e) if e=="CleanupUncertain"),
                    Ordering::SeqCst,
                );
                true
            } else {
                false
            }
        }
    }
    let cancel = DuringCreate {
        fixture: &fixture,
        calls: AtomicUsize::new(0),
        refused: AtomicBool::new(false),
    };
    assert!(matches!(
        gateway.execute(&spec(ProbeScenario::Sleep, 10000, 4096)?, &cancel),
        Err(ExecutionError::Cancelled)
    ));
    assert!(cancel.refused.load(Ordering::SeqCst));
    assert_eq!(
        checked(gateway.execute(&spec(ProbeScenario::Success, 10000, 4096)?, &NeverCancel))?
            .exit_code,
        Some(0)
    );
    Ok(())
}

#[test]
#[ignore = "explicit local Docker image/socket and macOS/APFS required"]
fn active_capabilities_require_positive_controls_and_kernel_evidence() -> Result {
    let fixture = Fixture::new()?;
    let gateway = fixture.gateway()?;
    let report = checked(gateway.probe_capabilities())?;
    assert!(report.strict_available, "{report:?}");
    assert!(report.restricted_available);
    assert!(!report.project_code_available);
    assert_eq!(report.scope, "trusted_probe_image_only");
    assert_eq!(
        report.configuration_fingerprint,
        checked(gateway.configuration_fingerprint())?
    );
    assert_eq!(report.observations.len(), 12);
    assert_eq!(report.observations.iter().filter(|o| o.control).count(), 2);
    assert!(
        report
            .observations
            .iter()
            .any(|o| o.scenario == ProbeScenario::Memory && o.execution.oom_killed == Some(true))
    );
    assert!(!gateway.is_quarantined());
    Ok(())
}
