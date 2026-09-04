//! Fixed trusted calibration inputs only. No host filesystem fixture execution.
use super::rust_gateway::RustGateway;
use rust_engineering_application::{ExecutionCancellation, ExecutionError};
use rust_engineering_domain::{
    ExecutionLimits, ExecutionResult, ExecutionTermination, RustCommand, SourceBundle, SourceFile,
};
const CHECKS: &str = include_str!("../../../fixtures/security/rust-containment/checks.rs");
const BUILD: &str = include_str!("../../../fixtures/security/rust-containment/build.rs");
const MACRO: &str = include_str!("../../../fixtures/security/rust-containment/proc_macro.rs");
const DESCENDANTS: &str =
    include_str!("../../../fixtures/security/rust-containment/descendants.rs");
const TIMEOUT: &str = include_str!("../../../fixtures/security/rust-containment/build_timeout.rs");
const RESOURCES: &str =
    include_str!("../../../fixtures/security/rust-containment/build_resources.rs");
const OVERFLOW: &str =
    include_str!("../../../fixtures/security/rust-containment/build_overflow.rs");
#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustCalibrationScenario {
    BuildScript,
    ProcMacro,
    Timeout,
    Overflow,
    Resources,
}
fn source(scenario: RustCalibrationScenario) -> Result<SourceBundle, ExecutionError> {
    let manifest = "[package]\nname='rust_calibration'\nversion='0.1.0'\nedition='2024'\n";
    let lock = "version = 4\n[[package]]\nname = 'rust_calibration'\nversion = '0.1.0'\n";
    let mut files = vec![
        ("Cargo.toml", manifest.to_owned()),
        ("Cargo.lock", lock.to_owned()),
        ("src/lib.rs", "pub fn benign() {}\n".into()),
    ];
    match scenario {
        RustCalibrationScenario::ProcMacro => {
            files[0]
                .1
                .push_str("[dependencies]\ncalibration_macro={path='macros'}\n");
            files[1].1="version = 4\n[[package]]\nname = 'rust_calibration'\nversion = '0.1.0'\ndependencies=['calibration_macro']\n[[package]]\nname='calibration_macro'\nversion='0.1.0'\n".into();
            files[2].1 = "calibration_macro::verify_containment!();\n".into();
            files.extend([("macros/Cargo.toml","[package]\nname='calibration_macro'\nversion='0.1.0'\nedition='2024'\n[lib]\nproc-macro=true\n".into()),("macros/src/lib.rs",MACRO.into()),("macros/src/checks.rs",CHECKS.into())]);
        }
        _ => {
            files.push(("checks.rs", CHECKS.into()));
            let build = match scenario {
                RustCalibrationScenario::BuildScript => BUILD,
                RustCalibrationScenario::Timeout => TIMEOUT,
                RustCalibrationScenario::Overflow => OVERFLOW,
                RustCalibrationScenario::Resources => RESOURCES,
                _ => return Err(ExecutionError::Infrastructure),
            };
            files.push(("build.rs", build.into()));
            if !matches!(scenario, RustCalibrationScenario::BuildScript) {
                files.push(("descendants.rs", DESCENDANTS.into()));
            }
        }
    }
    let files = files
        .into_iter()
        .map(|(p, b)| SourceFile::new(p.into(), b.into_bytes()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ExecutionError::Infrastructure)?;
    SourceBundle::new(files).map_err(|_| ExecutionError::Infrastructure)
}
impl RustGateway {
    pub(super) fn calibration_scenario(
        &self,
        scenario: RustCalibrationScenario,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<ExecutionResult, ExecutionError> {
        self.execute_calibration(&source(scenario)?, RustCommand::Check, limits, cancel)
    }
}
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::{APPROVED_RUST_IMAGE, HostDockerConfig};
    use rust_engineering_application::NeverCancel;
    use std::path::PathBuf;
    #[test]
    #[ignore = "explicit Docker socket; bounded resource stress only inside gateway"]
    fn resource_limits_are_actually_enforced() -> Result<(), String> {
        let root = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-resource-test-{}",
            crate::state::nonce().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        struct Root(PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _root = Root(root.clone());
        let gateway = RustGateway::new(HostDockerConfig {
            executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
            socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
                .ok_or("socket")?
                .into(),
            state_root: root,
            image_id: APPROVED_RUST_IMAGE.into(),
        })
        .map_err(|e| format!("{e:?}"))?;
        let result = gateway
            .calibration_scenario(
                RustCalibrationScenario::Resources,
                ExecutionLimits::new(30_000, 256 * 1024).ok_or("limits")?,
                &NeverCancel,
            )
            .map_err(|e| format!("resource: {e:?}"))?;
        println!(
            "{}",
            serde_json::to_string(&result).map_err(|e| e.to_string())?
        );
        assert_eq!(
            result.termination,
            ExecutionTermination::Exited,
            "{}",
            result.stderr
        );
        assert_eq!(result.exit_code, Some(0), "{}", result.stderr);
        assert!(result.stderr.contains("RUST_CONTAINMENT_RESOURCES_PASSED"));
        Ok(())
    }
    #[test]
    #[ignore = "explicit Docker socket; fixed hostile descendants only inside gateway"]
    fn observed_descendants_are_cleaned_on_timeout_cancel_and_overflow() -> Result<(), String> {
        let root = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-calibration-tree-{}",
            crate::state::nonce().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        struct Root(PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _root = Root(root.clone());
        let gateway = RustGateway::new(HostDockerConfig {
            executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
            socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
                .ok_or("socket")?
                .into(),
            state_root: root,
            image_id: APPROVED_RUST_IMAGE.into(),
        })
        .map_err(|e| format!("{e:?}"))?;
        let report = gateway
            .calibrate(&NeverCancel)
            .map_err(|e| format!("calibration: {e:?}"))?;
        assert_eq!(report.observations.len(), 6);
        assert!(report.verified);
        let inspected = gateway
            .execute(
                &source(RustCalibrationScenario::BuildScript).map_err(|e| format!("{e:?}"))?,
                RustCommand::Metadata,
                ExecutionLimits::default(),
                &NeverCancel,
            )
            .map_err(|e| format!("post calibration: {e:?}"))?;
        assert_eq!(inspected.exit_code, Some(0), "{}", inspected.stderr);
        struct Cancelled;
        impl ExecutionCancellation for Cancelled {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        assert!(matches!(
            gateway.calibrate(&Cancelled),
            Err(ExecutionError::Cancelled)
        ));
        assert!(matches!(
            gateway.execute(
                &source(RustCalibrationScenario::BuildScript).map_err(|e| format!("{e:?}"))?,
                RustCommand::Metadata,
                ExecutionLimits::default(),
                &NeverCancel,
            ),
            Err(ExecutionError::Denied)
        ));
        assert!(
            !gateway
                .calibrating
                .load(std::sync::atomic::Ordering::Acquire)
        );
        assert!(!gateway.is_quarantined());
        println!(
            "{}",
            serde_json::to_string(&report).map_err(|e| e.to_string())?
        );
        Ok(())
    }
    #[test]
    #[ignore = "explicit Docker socket and approved image; hostile code runs only inside gateway"]
    fn actual_build_script_and_proc_macro_containment() -> Result<(), String> {
        let root = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-calibration-{}",
            crate::state::nonce().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        struct Root(PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _root = Root(root.clone());
        let gateway = RustGateway::new(HostDockerConfig {
            executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
            socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
                .ok_or("explicit socket required")?
                .into(),
            state_root: root,
            image_id: APPROVED_RUST_IMAGE.into(),
        })
        .map_err(|e| format!("{e:?}"))?;
        let limits = ExecutionLimits::new(30_000, 256 * 1024).ok_or("limits")?;
        for (scenario, marker) in [
            (
                RustCalibrationScenario::BuildScript,
                "RUST_CONTAINMENT_BUILD_CHECKS_PASSED",
            ),
            (
                RustCalibrationScenario::ProcMacro,
                "RUST_CONTAINMENT_PROC_MACRO_CHECKS_PASSED",
            ),
        ] {
            let result = gateway
                .calibration_scenario(scenario, limits, &NeverCancel)
                .map_err(|e| format!("{scenario:?}: {e:?}"))?;
            println!(
                "{}",
                serde_json::to_string(&result).map_err(|e| e.to_string())?
            );
            assert_eq!(
                result.termination,
                ExecutionTermination::Exited,
                "{}",
                result.stderr
            );
            assert_eq!(result.exit_code, Some(0), "{}", result.stderr);
            assert!(
                result.stdout.contains(marker) || result.stderr.contains(marker),
                "missing marker: {} {}",
                result.stdout,
                result.stderr
            );
        }
        Ok(())
    }
    #[test]
    #[ignore = "explicit approved Docker socket; actual Clippy hostile fixtures only inside calibrated gateway"]
    fn actual_clippy_build_script_and_proc_macro_containment() -> Result<(), String> {
        use rust_engineering_domain::ClippySelection;
        let root = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-clippy-containment-{}",
            crate::state::nonce().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        // Retain private evidence on every failure. Cleanup success is proven by
        // execute's joined cleanup before a result is returned, plus quarantine.
        let gateway = RustGateway::new(HostDockerConfig {
            executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
            socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
                .ok_or("explicit socket required")?
                .into(),
            state_root: root.clone(),
            image_id: APPROVED_RUST_IMAGE.into(),
        })
        .map_err(|e| format!("{e:?}"))?;
        let calibration = gateway
            .calibrate(&NeverCancel)
            .map_err(|e| format!("calibration: {e:?}; retained {}", root.display()))?;
        assert!(calibration.verified);
        assert_eq!(calibration.observations.len(), 6);
        let configuration = gateway
            .configuration_fingerprint()
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(configuration, calibration.configuration_fingerprint);
        let options: rust_engineering_domain::ClippyOptions = ClippySelection::default()
            .try_into()
            .map_err(|e| format!("{e:?}"))?;
        let mut executions = Vec::new();
        for (scenario, marker) in [
            (
                RustCalibrationScenario::BuildScript,
                "RUST_CONTAINMENT_BUILD_CHECKS_PASSED",
            ),
            (
                RustCalibrationScenario::ProcMacro,
                "RUST_CONTAINMENT_PROC_MACRO_CHECKS_PASSED",
            ),
        ] {
            let result = gateway
                .execute(
                    &source(scenario).map_err(|e| format!("{e:?}"))?,
                    RustCommand::ClippyProject(options.clone()),
                    ExecutionLimits::new(30_000, 256 * 1024).ok_or("limits")?,
                    &NeverCancel,
                )
                .map_err(|e| format!("{scenario:?}: {e:?}; retained {}", root.display()))?;
            assert_eq!(
                result.termination,
                ExecutionTermination::Exited,
                "{}",
                result.stderr
            );
            assert_eq!(
                result.exit_code,
                Some(0),
                "{} {}",
                result.stdout,
                result.stderr
            );
            assert!(!result.stdout_truncated && !result.stderr_truncated);
            assert!(
                result.stdout.contains(marker) || result.stderr.contains(marker),
                "missing actual Clippy containment marker: {} {}",
                result.stdout,
                result.stderr
            );
            assert!(result.stdout.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
                .any(|record| record["reason"] == "build-finished" && record["success"] == true));
            assert!(!gateway.is_quarantined());
            executions.push(serde_json::json!({
                "scenario":scenario, "marker":marker, "termination":result.termination,
                "exit_code":result.exit_code, "execution_fingerprint":result.execution_fingerprint,
                "joined_cleanup_verified":true
            }));
        }
        println!("M1_CLIPPY_CONTAINMENT_RECEIPT {}", serde_json::to_string(&serde_json::json!({
            "status":"passed", "cases":2, "calibration_verified":true,
            "calibration_cases":calibration.observations.len(),
            "configuration_fingerprint":configuration,
            "image_id":APPROVED_RUST_IMAGE, "fixture_fingerprint":calibration.fixture_fingerprint,
            "build_script_and_proc_macro_checks":true,
            "network_env_filesystem_process_cgroup_assertions":true,
            "cleanup":true, "quarantined":gateway.is_quarantined(), "executions":executions
        })).map_err(|e| e.to_string())?);
        std::fs::remove_dir_all(root).map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[derive(Debug, serde::Serialize)]
pub struct RustCalibrationObservation {
    pub scenario: &'static str,
    pub execution: ExecutionResult,
    pub detached_processes: Option<String>,
}
#[derive(Debug, serde::Serialize)]
pub struct RustCalibrationReport {
    pub scope: &'static str,
    pub image_id: String,
    pub observed_at_unix_ms: u64,
    pub fixture_fingerprint: String,
    pub configuration_fingerprint: rust_engineering_domain::ExecutionFingerprint,
    pub observations: Vec<RustCalibrationObservation>,
    pub verified: bool,
}
struct Latch<'a>(std::sync::atomic::AtomicBool, &'a dyn ExecutionCancellation);
impl ExecutionCancellation for Latch<'_> {
    fn is_cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Acquire) || self.1.is_cancelled()
    }
}
impl RustGateway {
    fn interrupted_calibration(
        &self,
        scenario: RustCalibrationScenario,
        cancel_after_observation: bool,
        parent: &dyn ExecutionCancellation,
    ) -> Result<(ExecutionResult, String), ExecutionError> {
        let latch = Latch(std::sync::atomic::AtomicBool::new(false), parent);
        let name = std::sync::Mutex::new(None);
        let source = source(scenario)?;
        let limits = ExecutionLimits::new(
            5_000,
            if matches!(scenario, RustCalibrationScenario::Overflow) {
                16 * 1024
            } else {
                256 * 1024
            },
        )
        .ok_or(ExecutionError::Infrastructure)?;
        std::thread::scope(|scope| {
            let job = scope.spawn(|| {
                self.execute_observed(
                    &source,
                    RustCommand::Check,
                    limits,
                    &latch,
                    super::rust_gateway::Admission::Calibration(Some(&name)),
                )
            });
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
            let mut observed = None;
            let mut observer_error = None;
            while !job.is_finished()
                && std::time::Instant::now() < deadline
                && !parent.is_cancelled()
            {
                let current = name
                    .lock()
                    .map_err(|_| ExecutionError::Infrastructure)?
                    .clone();
                if let Some(name) = current {
                    match self.detached_observation(&name) {
                        Ok(Some(top)) => {
                            observed = Some(top);
                            break;
                        }
                        Ok(None) => (),
                        Err(error) => {
                            observer_error = Some(error);
                            break;
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            if cancel_after_observation || observed.is_none() {
                latch.0.store(true, std::sync::atomic::Ordering::Release);
            }
            let result = job.join().map_err(|_| ExecutionError::Infrastructure)??;
            if parent.is_cancelled() {
                return Err(ExecutionError::Cancelled);
            }
            if let Some(error) = observer_error {
                return Err(error);
            }
            Ok((result, observed.ok_or(ExecutionError::Denied)?))
        })
    }
    /// Explicit trusted-host operation; no caller source or evidence document can
    /// substitute for executing these fixed fixtures under this actual gateway.
    pub fn calibrate(
        &self,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<RustCalibrationReport, ExecutionError> {
        use std::sync::atomic::Ordering;
        if self
            .calibrating
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(ExecutionError::Busy);
        }
        struct Guard<'a>(&'a std::sync::atomic::AtomicBool);
        impl Drop for Guard<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _guard = Guard(&self.calibrating);
        self.set_verified(false);
        let mut observations = Vec::new();
        let limits =
            ExecutionLimits::new(30_000, 256 * 1024).ok_or(ExecutionError::Infrastructure)?;
        for (scenario, name, marker) in [
            (
                RustCalibrationScenario::BuildScript,
                "build_script",
                "RUST_CONTAINMENT_BUILD_CHECKS_PASSED",
            ),
            (
                RustCalibrationScenario::Resources,
                "resources",
                "RUST_CONTAINMENT_RESOURCES_PASSED",
            ),
            (
                RustCalibrationScenario::ProcMacro,
                "proc_macro",
                "RUST_CONTAINMENT_PROC_MACRO_CHECKS_PASSED",
            ),
        ] {
            let execution = self.calibration_scenario(scenario, limits, cancel)?;
            if cancel.is_cancelled() {
                return Err(ExecutionError::Cancelled);
            }
            if execution.termination != ExecutionTermination::Exited
                || execution.exit_code != Some(0)
                || !(execution.stdout.contains(marker) || execution.stderr.contains(marker))
            {
                return Err(ExecutionError::Denied);
            }
            observations.push(RustCalibrationObservation {
                scenario: name,
                execution,
                detached_processes: None,
            });
        }
        for (scenario, name, cancel_after, expected) in [
            (
                RustCalibrationScenario::Timeout,
                "timeout",
                false,
                ExecutionTermination::TimedOut,
            ),
            (
                RustCalibrationScenario::Timeout,
                "cancel",
                true,
                ExecutionTermination::Cancelled,
            ),
            (
                RustCalibrationScenario::Overflow,
                "overflow",
                false,
                ExecutionTermination::OutputLimit,
            ),
        ] {
            let (execution, top) = self.interrupted_calibration(scenario, cancel_after, cancel)?;
            if execution.termination != expected || execution.exit_code.is_some() {
                return Err(ExecutionError::Denied);
            }
            observations.push(RustCalibrationObservation {
                scenario: name,
                execution,
                detached_processes: Some(top),
            });
        }
        let fixture_bytes = serde_json::to_vec(&(
            CHECKS,
            BUILD,
            MACRO,
            DESCENDANTS,
            TIMEOUT,
            OVERFLOW,
            RESOURCES,
        ))
        .map_err(|_| ExecutionError::Infrastructure)?;
        let fixture_fingerprint = crate::digest(&fixture_bytes);
        let observed_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| ExecutionError::Infrastructure)?
            .as_millis()
            .try_into()
            .map_err(|_| ExecutionError::Infrastructure)?;
        let configuration_fingerprint = self.configuration_fingerprint()?;
        if cancel.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        self.set_verified(true);
        Ok(RustCalibrationReport {
            scope: "rust-cargo-source-profile-v1",
            image_id: self.image_id().into(),
            observed_at_unix_ms,
            fixture_fingerprint,
            configuration_fingerprint,
            observations,
            verified: true,
        })
    }
}
