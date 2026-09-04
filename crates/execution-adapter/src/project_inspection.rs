//! Lazy approved runtime composition, invoked only from an admitted MCP worker.
use crate::{HostDockerConfig, RustGateway};
use rust_engineering_application::{
    DiagnosticExplainPort, ExecutionError, InspectionControl, InspectionError, ProjectError,
    ProjectInspectionPort, ToolchainInspectionPort,
};
use rust_engineering_domain::{
    DiagnosticCode, ExecutionFingerprint, ExecutionLimits, ExecutionResult, ExecutionTermination,
    ExplainObservation, OperationalErrorCode, ProjectStructure, RuntimeIdentity, RustCommand,
    SourceBundle, ToolchainExecution, ToolchainObservation, ToolchainObservationCommand,
    ToolchainRuntime,
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

pub struct RustProjectInspector {
    config: Option<HostDockerConfig>,
    gateway: Mutex<Option<RustGateway>>,
    calibrated: AtomicBool,
    calibration_failed: AtomicBool,
    quarantined: AtomicBool,
}
impl RustProjectInspector {
    /// Stores host policy only; startup does not execute Docker or calibration.
    pub fn new(config: Option<HostDockerConfig>) -> Self {
        Self {
            config,
            gateway: Mutex::new(None),
            calibrated: AtomicBool::new(false),
            calibration_failed: AtomicBool::new(false),
            quarantined: AtomicBool::new(false),
        }
    }
    /// A busy/poisoned state is not proof of clean shutdown.
    pub fn is_quarantined(&self) -> bool {
        self.quarantined.load(Ordering::Acquire)
            || match self.gateway.try_lock() {
                Ok(state) => state.as_ref().is_some_and(RustGateway::is_quarantined),
                Err(_) => true,
            }
    }
    fn ensure_calibrated(
        &self,
        calibrate: impl FnOnce() -> Result<(), ExecutionError>,
    ) -> Result<(), ExecutionError> {
        if self.calibration_failed.load(Ordering::Acquire) {
            return Err(ExecutionError::Denied);
        }
        if self.calibrated.load(Ordering::Acquire) {
            return Ok(());
        }
        match calibrate() {
            Ok(()) => {
                self.calibrated.store(true, Ordering::Release);
                Ok(())
            }
            // An interrupted calibration did not establish failed containment;
            // its gateway cleanup still completes before a later retry.
            Err(ExecutionError::Cancelled) => Err(ExecutionError::Cancelled),
            Err(error) => {
                // Never re-run hostile calibration after failed verification.
                // Recovery requires an explicit new host session.
                self.calibration_failed.store(true, Ordering::Release);
                Err(error)
            }
        }
    }
    fn with_gateway<T>(
        &self,
        control: &dyn InspectionControl,
        work: impl FnOnce(&RustGateway) -> Result<T, InspectionError>,
    ) -> Result<T, InspectionError> {
        control.check().map_err(InspectionError::Project)?;
        if self.quarantined.load(Ordering::Acquire) {
            return Err(InspectionError::Execution(ExecutionError::CleanupUncertain));
        }
        let config = self
            .config
            .as_ref()
            .ok_or(InspectionError::Execution(ExecutionError::Denied))?;
        let mut state = self.gateway.lock().map_err(|_| InspectionError::Internal)?;
        if state.is_none() {
            *state = Some(RustGateway::new(config.clone()).map_err(InspectionError::Execution)?);
        }
        let gateway = state.as_ref().ok_or(InspectionError::Internal)?;
        if gateway.is_quarantined() {
            return Err(InspectionError::Execution(ExecutionError::CleanupUncertain));
        }
        self.ensure_calibrated(|| gateway.calibrate(control).map(|_| ()))
            .map_err(InspectionError::Execution)?;
        control.check().map_err(InspectionError::Project)?;
        work(gateway)
    }
    fn inspect_inner(
        &self,
        source: &SourceBundle,
        control: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        self.with_gateway(control, |gateway| {
            let result = gateway
                .execute(
                    source,
                    RustCommand::Metadata,
                    ExecutionLimits::new(30_000, 256 * 1024).ok_or(InspectionError::Internal)?,
                    control,
                )
                .map_err(InspectionError::Execution)?;
            match result.termination {
                ExecutionTermination::TimedOut => {
                    return Err(InspectionError::Project(ProjectError::Rejected(
                        OperationalErrorCode::CommandTimeout,
                    )));
                }
                ExecutionTermination::Cancelled => {
                    return Err(InspectionError::Project(ProjectError::Cancelled));
                }
                ExecutionTermination::OutputLimit => return Err(InspectionError::OutputLimit),
                ExecutionTermination::Exited => (),
            }
            if result.exit_code != Some(0) {
                return Err(InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::InvalidProject,
                )));
            }
            let runtime = RuntimeIdentity {
                platform: result.platform.into(),
                image_id: result.image_id,
                configuration_fingerprint: gateway
                    .configuration_fingerprint()
                    .map_err(InspectionError::Execution)?,
                execution_fingerprint: result.execution_fingerprint,
                // RustGateway::new accepts only APPROVED_RUST_IMAGE, verified during
                // explicit provisioning. These are facts of that immutable identity.
                rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
                cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
                declared_toolchain: None,
            };
            super::project_metadata::parse(result.stdout.as_bytes(), source, runtime)
        })
    }
}
impl ProjectInspectionPort for RustProjectInspector {
    fn inspect(
        &self,
        source: &SourceBundle,
        control: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        let result = self.inspect_inner(source, control);
        if matches!(
            result,
            Err(InspectionError::Execution(ExecutionError::CleanupUncertain)
                | InspectionError::Internal)
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl DiagnosticExplainPort for RustProjectInspector {
    fn explain(
        &self,
        code: &DiagnosticCode,
        control: &dyn InspectionControl,
    ) -> Result<ExplainObservation, InspectionError> {
        let result = self.with_gateway(control, |gateway| {
            // No project handle or host source enters this compiler-only request.
            let source = SourceBundle::new(Vec::new()).map_err(|_| InspectionError::Internal)?;
            let execution = gateway
                .execute(
                    &source,
                    RustCommand::Explain(code.clone()),
                    ExecutionLimits::new(30_000, 64 * 1024).ok_or(InspectionError::Internal)?,
                    control,
                )
                .map_err(InspectionError::Execution)?;
            control.check().map_err(InspectionError::Project)?;
            explain_observation(
                code,
                execution,
                gateway
                    .configuration_fingerprint()
                    .map_err(InspectionError::Execution)?,
            )
        });
        if matches!(
            result,
            Err(InspectionError::Execution(ExecutionError::CleanupUncertain)
                | InspectionError::Internal)
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

fn explain_observation(
    code: &DiagnosticCode,
    execution: ExecutionResult,
    configuration_fingerprint: ExecutionFingerprint,
) -> Result<ExplainObservation, InspectionError> {
    match execution.termination {
        ExecutionTermination::Cancelled => {
            return Err(InspectionError::Project(ProjectError::Cancelled));
        }
        ExecutionTermination::TimedOut => {
            return Err(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout,
            )));
        }
        ExecutionTermination::OutputLimit => return Err(InspectionError::OutputLimit),
        ExecutionTermination::Exited => (),
    }
    if execution.stdout_truncated
        || execution.stderr_truncated
        || execution.stdout.len() > 64 * 1024
        || execution.stderr.len() > 64 * 1024
    {
        return Err(InspectionError::OutputLimit);
    }
    if execution.oom_killed == Some(true) {
        return Err(InspectionError::Execution(ExecutionError::Infrastructure));
    }
    let explanation = if execution.exit_code == Some(0)
        && !execution.stdout.trim().is_empty()
        && execution.stderr.is_empty()
    {
        Some(execution.stdout)
    } else if execution.exit_code == Some(1)
        && execution.stdout.is_empty()
        // Match the installed compiler's entire unknown-code diagnostic. A
        // loader error, panic or different code cannot masquerade as absence.
        && execution.stderr.trim_end_matches('\n') == format!("error: {code} is not a valid error code")
    {
        None
    } else {
        return Err(InspectionError::Execution(ExecutionError::Infrastructure));
    };
    Ok(ExplainObservation {
        code: code.clone(),
        content_fingerprint: super::digest(explanation.as_deref().unwrap_or("").as_bytes())
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        explanation,
        complete: true,
        termination: execution.termination,
        exit_code: execution.exit_code,
        stdout_truncated: false,
        stderr_truncated: false,
        runtime: RuntimeIdentity {
            platform: execution.platform.into(),
            image_id: execution.image_id,
            configuration_fingerprint,
            execution_fingerprint: execution.execution_fingerprint,
            rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
            cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
            declared_toolchain: None,
        },
    })
}

impl ToolchainInspectionPort for RustProjectInspector {
    fn inspect_toolchain(
        &self,
        source: &SourceBundle,
        control: &dyn InspectionControl,
    ) -> Result<ToolchainObservation, InspectionError> {
        let result = self.with_gateway(control, |gateway| {
            let mut outputs = Vec::new();
            let mut executions = Vec::new();
            for (command, observation_command) in [
                (
                    RustCommand::CompilerVersion,
                    ToolchainObservationCommand::CompilerVersion,
                ),
                (
                    RustCommand::CargoVersion,
                    ToolchainObservationCommand::CargoVersion,
                ),
                (
                    RustCommand::InstalledComponents,
                    ToolchainObservationCommand::InstalledComponents,
                ),
            ] {
                control.check().map_err(InspectionError::Project)?;
                let execution = gateway
                    .execute(
                        source,
                        command,
                        ExecutionLimits::new(30_000, 16 * 1024).ok_or(InspectionError::Internal)?,
                        control,
                    )
                    .map_err(InspectionError::Execution)?;
                match execution.termination {
                    ExecutionTermination::Cancelled => {
                        return Err(InspectionError::Project(ProjectError::Cancelled));
                    }
                    ExecutionTermination::TimedOut => {
                        return Err(InspectionError::Project(ProjectError::Rejected(
                            OperationalErrorCode::CommandTimeout,
                        )));
                    }
                    ExecutionTermination::OutputLimit => return Err(InspectionError::OutputLimit),
                    ExecutionTermination::Exited => (),
                }
                if execution.exit_code != Some(0) {
                    return Err(InspectionError::Execution(ExecutionError::Unavailable));
                }
                if execution.stdout_truncated || execution.stderr_truncated {
                    return Err(InspectionError::OutputLimit);
                }
                executions.push(ToolchainExecution {
                    command: observation_command,
                    execution_fingerprint: execution.execution_fingerprint,
                });
                outputs.push(execution.stdout);
            }
            control.check().map_err(InspectionError::Project)?;
            let [rustc, cargo, components]: [String; 3] =
                outputs.try_into().map_err(|_| InspectionError::Internal)?;
            let inventory = super::toolchain_metadata::parse(
                rustc.as_bytes(),
                cargo.as_bytes(),
                components.as_bytes(),
            )?;
            let archive =
                super::source_archive::encode(source).map_err(InspectionError::Execution)?;
            Ok(ToolchainObservation {
                inventory,
                declared_toolchain: super::project_metadata::declared_toolchain(source)?,
                source_fingerprint: super::digest(&archive)
                    .parse()
                    .map_err(|_| InspectionError::Internal)?,
                runtime: ToolchainRuntime {
                    platform: "linux/aarch64".into(),
                    image_id: gateway.image_id().into(),
                    configuration_fingerprint: gateway
                        .configuration_fingerprint()
                        .map_err(InspectionError::Execution)?,
                    executions,
                },
            })
        });
        if matches!(
            result,
            Err(InspectionError::Execution(ExecutionError::CleanupUncertain)
                | InspectionError::Internal)
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl RustProjectInspector {
    fn cargo_validation(
        &self,
        source: &SourceBundle,
        command: RustCommand,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::CheckObservation, InspectionError> {
        self.cargo_run(source, command, 30_000, false, control)
            .map(|(observation, _)| observation)
    }
    fn cargo_run(
        &self,
        source: &SourceBundle,
        command: RustCommand,
        wall_ms: u64,
        test_output: bool,
        control: &dyn InspectionControl,
    ) -> Result<(rust_engineering_domain::CheckObservation, Option<bool>), InspectionError> {
        use rust_engineering_domain::{CheckObservation, CheckOutcome};
        let result = self.with_gateway(control, |gateway| {
            let mut result = gateway
                .execute(
                    source,
                    command,
                    ExecutionLimits::new(wall_ms, 256 * 1024).ok_or(InspectionError::Internal)?,
                    control,
                )
                .map_err(InspectionError::Execution)?;
            // Gateway byte caps precede UTF8-lossy conversion, which can expand
            // hostile bytes. Keep a bounded partial report instead of discarding it.
            bound_check_text(&mut result.stdout, &mut result.stdout_truncated);
            bound_check_text(&mut result.stderr, &mut result.stderr_truncated);
            if result.termination == ExecutionTermination::Cancelled {
                return Err(InspectionError::Project(ProjectError::Cancelled));
            }
            let parser = if test_output {
                super::cargo_diagnostics::parse_test
            } else {
                super::cargo_diagnostics::parse
            };
            let parsed = parser(
                &result.stdout,
                source,
                result.termination == ExecutionTermination::Exited && !result.stdout_truncated,
            )?;
            let validation_complete = parsed.complete
                && !result.stderr_truncated
                && result.termination == ExecutionTermination::Exited
                && (matches!(
                    (result.exit_code, parsed.build_finished),
                    (Some(0), Some(true)) | (Some(1..), Some(false))
                ) || (test_output
                    && matches!(
                        (result.exit_code, parsed.build_finished),
                        (Some(1..), Some(true))
                    )));
            let frozen_lock_error = frozen_lock_error(
                result.termination,
                result.exit_code,
                &result.stdout,
                &result.stderr,
                result.stderr_truncated,
            );
            let outcome = if frozen_lock_error {
                CheckOutcome::LockfileUpdateRequired
            } else if !validation_complete {
                CheckOutcome::Incomplete
            } else if result.exit_code == Some(0) {
                CheckOutcome::Passed
            } else {
                CheckOutcome::Failed
            };
            let archive =
                super::source_archive::encode(source).map_err(InspectionError::Execution)?;
            Ok((
                CheckObservation {
                    outcome,
                    termination: result.termination,
                    exit_code: result.exit_code,
                    validation_complete,
                    diagnostics: parsed.diagnostics,
                    diagnostics_omitted: parsed.diagnostics_omitted,
                    stdout: result.stdout,
                    stderr: result.stderr,
                    stdout_truncated: result.stdout_truncated,
                    stderr_truncated: result.stderr_truncated,
                    source_fingerprint: super::digest(&archive)
                        .parse()
                        .map_err(|_| InspectionError::Internal)?,
                    runtime: RuntimeIdentity {
                        platform: result.platform.into(),
                        image_id: result.image_id,
                        configuration_fingerprint: gateway
                            .configuration_fingerprint()
                            .map_err(InspectionError::Execution)?,
                        execution_fingerprint: result.execution_fingerprint,
                        rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
                        cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
                        declared_toolchain: super::project_metadata::declared_toolchain(source)?,
                    },
                },
                parsed.build_finished,
            ))
        });
        if matches!(
            result,
            Err(InspectionError::Execution(ExecutionError::CleanupUncertain)
                | InspectionError::Internal)
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl rust_engineering_application::ProjectCheckPort for RustProjectInspector {
    fn check(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_domain::CheckOptions,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::CheckObservation, InspectionError> {
        self.cargo_validation(source, RustCommand::CheckProject(options.clone()), control)
    }
}
impl rust_engineering_application::ProjectClippyPort for RustProjectInspector {
    fn clippy(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_domain::ClippyOptions,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::CheckObservation, InspectionError> {
        self.cargo_validation(source, RustCommand::ClippyProject(options.clone()), control)
    }
}

impl rust_engineering_application::ProjectTestPort for RustProjectInspector {
    fn test(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_domain::TestOptions,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::TestObservation, InspectionError> {
        self.cargo_run(
            source,
            RustCommand::TestProject(options.clone()),
            options.timeout() * 1000,
            true,
            control,
        )
        .map(
            |(execution, build_succeeded)| rust_engineering_domain::TestObservation {
                execution,
                build_succeeded,
            },
        )
    }
}

impl rust_engineering_application::ProjectFormatPort for RustProjectInspector {
    fn format(
        &self,
        source: &SourceBundle,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::FormatObservation, InspectionError> {
        use rust_engineering_domain::{CheckObservation, CheckOutcome, FormatObservation};
        let result = self.with_gateway(control, |gateway| {
            let mut result = gateway
                .execute(
                    source,
                    RustCommand::FormatCheck,
                    ExecutionLimits::new(30_000, 256 * 1024).ok_or(InspectionError::Internal)?,
                    control,
                )
                .map_err(InspectionError::Execution)?;
            bound_check_text(&mut result.stdout, &mut result.stdout_truncated);
            bound_check_text(&mut result.stderr, &mut result.stderr_truncated);
            if result.termination == ExecutionTermination::Cancelled {
                return Err(InspectionError::Project(ProjectError::Cancelled));
            }
            let parsed = super::format_output::parse(
                &result.stdout,
                source,
                result.termination == ExecutionTermination::Exited && !result.stdout_truncated,
            );
            let validation_complete = parsed.complete
                && !result.stderr_truncated
                && result.stderr.is_empty()
                && result.termination == ExecutionTermination::Exited
                && ((result.exit_code == Some(0) && result.stdout.is_empty())
                    || (result.exit_code == Some(1) && !parsed.affected_files.is_empty()));
            let outcome = if !validation_complete {
                CheckOutcome::Incomplete
            } else if result.exit_code == Some(0) {
                CheckOutcome::Passed
            } else {
                CheckOutcome::Failed
            };
            let archive =
                super::source_archive::encode(source).map_err(InspectionError::Execution)?;
            Ok(FormatObservation {
                execution: CheckObservation {
                    outcome,
                    termination: result.termination,
                    exit_code: result.exit_code,
                    validation_complete,
                    diagnostics: Vec::new(),
                    diagnostics_omitted: 0,
                    stdout: result.stdout,
                    stderr: result.stderr,
                    stdout_truncated: result.stdout_truncated,
                    stderr_truncated: result.stderr_truncated,
                    source_fingerprint: super::digest(&archive)
                        .parse()
                        .map_err(|_| InspectionError::Internal)?,
                    runtime: RuntimeIdentity {
                        platform: result.platform.into(),
                        image_id: result.image_id,
                        configuration_fingerprint: gateway
                            .configuration_fingerprint()
                            .map_err(InspectionError::Execution)?,
                        execution_fingerprint: result.execution_fingerprint,
                        rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
                        cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
                        declared_toolchain: super::project_metadata::declared_toolchain(source)?,
                    },
                },
                affected_files: parsed.affected_files,
                affected_files_omitted: parsed.affected_files_omitted,
                diff: parsed.diff,
                diff_omitted: parsed.diff_omitted,
            })
        });
        if matches!(
            result,
            Err(InspectionError::Execution(ExecutionError::CleanupUncertain)
                | InspectionError::Internal)
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

fn bound_check_text(text: &mut String, truncated: &mut bool) {
    const LIMIT: usize = 256 * 1024;
    if text.len() > LIMIT {
        let mut end = LIMIT;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        *truncated = true;
    }
}

// Pinned Cargo startup vocabulary; untrusted output classification never grants
// execution authority, modifies a lock or turns an unsuccessful job into passed.
fn frozen_lock_error(
    termination: ExecutionTermination,
    exit: Option<i32>,
    stdout: &str,
    stderr: &str,
    truncated: bool,
) -> bool {
    termination == ExecutionTermination::Exited
        && exit == Some(101)
        && stdout.is_empty()
        && !truncated
        && matches!(
            stderr.lines().next(),
            Some(
                "error: cannot update the lock file /source/Cargo.lock because --frozen was passed to prevent this"
                    | "error: cannot create the lock file /source/Cargo.lock because --frozen was passed to prevent this"
            )
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    fn explanation_execution() -> Result<ExecutionResult, Box<dyn std::error::Error>> {
        Ok(ExecutionResult {
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            oom_killed: Some(false),
            stdout: "Compiler explanation.\n\n```rust\nfn main() {}\n```\n".into(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            duration_ms: 1,
            total_duration_ms: 2,
            execution_fingerprint: format!("sha256:{}", "a".repeat(64)).parse()?,
            platform: "linux/aarch64",
            image_id: crate::APPROVED_RUST_IMAGE.into(),
        })
    }
    #[test]
    fn explain_retains_exact_compiler_bytes_and_binds_runtime_and_content()
    -> Result<(), Box<dyn std::error::Error>> {
        let execution = explanation_execution()?;
        let expected = execution.stdout.clone();
        let configuration: ExecutionFingerprint = format!("sha256:{}", "b".repeat(64)).parse()?;
        let observation =
            explain_observation(&"E0502".parse()?, execution.clone(), configuration.clone())
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(observation.explanation.as_deref(), Some(expected.as_str()));
        assert_eq!(
            observation.content_fingerprint.to_string(),
            super::super::digest(expected.as_bytes())
        );
        assert!(observation.complete);
        assert_eq!(observation.exit_code, Some(0));
        assert_eq!(
            observation.runtime.execution_fingerprint,
            execution.execution_fingerprint
        );
        assert_eq!(
            observation.runtime.configuration_fingerprint,
            configuration.clone()
        );
        assert_eq!(observation.runtime.image_id, crate::APPROVED_RUST_IMAGE);
        assert_eq!(observation.runtime.rust_version, "1.98.1");
        assert_eq!(observation.runtime.declared_toolchain, None);
        Ok(())
    }
    #[test]
    fn explain_unknown_requires_exact_complete_code_specific_compiler_evidence()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = "E9999".parse()?;
        let mut execution = explanation_execution()?;
        execution.exit_code = Some(1);
        execution.stdout.clear();
        execution.stderr = "error: E9999 is not a valid error code\n\n".into();
        let configuration = execution.execution_fingerprint.clone();
        let observation = explain_observation(&code, execution.clone(), configuration.clone())
            .map_err(|error| format!("{error:?}"))?;
        assert!(observation.complete);
        assert!(observation.explanation.is_none());
        assert_eq!(observation.exit_code, Some(1));
        assert_eq!(
            observation.content_fingerprint.to_string(),
            super::super::digest(b"")
        );
        for stderr in [
            "error: E0000 is not a valid error code\n",
            "error: E9999 is not a valid error code\nerror: compiler panic\n",
            "error: couldn't load library\n",
            "",
        ] {
            execution.stderr = stderr.into();
            assert!(matches!(
                explain_observation(&code, execution.clone(), configuration.clone()),
                Err(InspectionError::Execution(ExecutionError::Infrastructure))
            ));
        }
        execution.stderr = "error: E9999 is not a valid error code\n".into();
        for exit in [Some(0), Some(101), Some(137), None] {
            execution.exit_code = exit;
            assert!(matches!(
                explain_observation(&code, execution.clone(), configuration.clone()),
                Err(InspectionError::Execution(ExecutionError::Infrastructure))
            ));
        }
        execution.exit_code = Some(1);
        execution.stdout = "unexpected output".into();
        assert!(matches!(
            explain_observation(&code, execution, configuration.clone()),
            Err(InspectionError::Execution(ExecutionError::Infrastructure))
        ));
        Ok(())
    }
    #[test]
    fn explain_never_promotes_incomplete_empty_or_failed_output()
    -> Result<(), Box<dyn std::error::Error>> {
        let code = "E0502".parse()?;
        let execution = explanation_execution()?;
        let configuration = execution.execution_fingerprint.clone();
        for termination in [
            ExecutionTermination::Cancelled,
            ExecutionTermination::TimedOut,
            ExecutionTermination::OutputLimit,
        ] {
            let mut changed = execution.clone();
            changed.termination = termination;
            let error = explain_observation(&code, changed, configuration.clone()).err();
            match termination {
                ExecutionTermination::Cancelled => assert!(matches!(
                    error,
                    Some(InspectionError::Project(ProjectError::Cancelled))
                )),
                ExecutionTermination::TimedOut => assert!(matches!(
                    error,
                    Some(InspectionError::Project(ProjectError::Rejected(
                        OperationalErrorCode::CommandTimeout
                    )))
                )),
                ExecutionTermination::OutputLimit => {
                    assert!(matches!(error, Some(InspectionError::OutputLimit)))
                }
                ExecutionTermination::Exited => unreachable!(),
            }
        }
        for stream in [true, false] {
            let mut changed = execution.clone();
            if stream {
                changed.stdout_truncated = true;
            } else {
                changed.stderr_truncated = true;
            }
            assert!(matches!(
                explain_observation(&code, changed, configuration.clone()),
                Err(InspectionError::OutputLimit)
            ));
            let mut changed = execution.clone();
            if stream {
                changed.stdout = "x".repeat(64 * 1024 + 1);
            } else {
                changed.stderr = "x".repeat(64 * 1024 + 1);
            }
            assert!(matches!(
                explain_observation(&code, changed, configuration.clone()),
                Err(InspectionError::OutputLimit)
            ));
        }
        let mut changed = execution.clone();
        changed.stdout = "x".repeat(64 * 1024);
        assert!(explain_observation(&code, changed, configuration.clone()).is_ok());
        for stdout in ["", " \n\t"] {
            let mut changed = execution.clone();
            changed.stdout = stdout.into();
            assert!(matches!(
                explain_observation(&code, changed, configuration.clone()),
                Err(InspectionError::Execution(ExecutionError::Infrastructure))
            ));
        }
        let mut changed = execution.clone();
        changed.stderr = "unexpected compiler warning".into();
        assert!(matches!(
            explain_observation(&code, changed, configuration.clone()),
            Err(InspectionError::Execution(ExecutionError::Infrastructure))
        ));
        let mut changed = execution;
        changed.oom_killed = Some(true);
        assert!(matches!(
            explain_observation(&code, changed, configuration.clone()),
            Err(InspectionError::Execution(ExecutionError::Infrastructure))
        ));
        Ok(())
    }
    #[test]
    fn invalid_utf8_expansion_keeps_bounded_partial_text() {
        let mut text = String::from_utf8_lossy(&vec![0xff; 256 * 1024]).into_owned();
        assert!(text.len() > 256 * 1024);
        let mut truncated = false;
        bound_check_text(&mut text, &mut truncated);
        assert!(truncated);
        assert!(text.len() <= 256 * 1024);
        assert!(text.chars().all(|c| c == '\u{fffd}'));
    }
    #[test]
    fn frozen_lock_classification_requires_exact_startup_evidence() {
        let line = "error: cannot create the lock file /source/Cargo.lock because --frozen was passed to prevent this";
        assert!(frozen_lock_error(
            ExecutionTermination::Exited,
            Some(101),
            "",
            line,
            false
        ));
        assert!(frozen_lock_error(
            ExecutionTermination::Exited,
            Some(101),
            "",
            &line.replace("create", "update"),
            false
        ));
        for (termination, exit, stdout, stderr, truncated) in [
            (
                ExecutionTermination::TimedOut,
                Some(101),
                "",
                line.to_owned(),
                false,
            ),
            (
                ExecutionTermination::Exited,
                Some(0),
                "",
                line.to_owned(),
                false,
            ),
            (
                ExecutionTermination::Exited,
                Some(101),
                "{}\n",
                line.to_owned(),
                false,
            ),
            (
                ExecutionTermination::Exited,
                Some(101),
                "",
                line.to_owned(),
                true,
            ),
            (
                ExecutionTermination::Exited,
                Some(101),
                "",
                line.replace("/source/", "/other/"),
                false,
            ),
            (
                ExecutionTermination::Exited,
                Some(101),
                "",
                format!("project output\n{line}"),
                false,
            ),
        ] {
            assert!(!frozen_lock_error(
                termination,
                exit,
                stdout,
                &stderr,
                truncated
            ));
        }
    }
    #[test]
    fn failed_calibration_is_latched_but_clean_cancellation_can_retry() {
        for failure in [
            ExecutionError::Denied,
            ExecutionError::Infrastructure,
            ExecutionError::CleanupUncertain,
            ExecutionError::InvalidConfiguration,
            ExecutionError::Busy,
            ExecutionError::Unavailable,
        ] {
            let inspector = RustProjectInspector::new(None);
            let attempts = Cell::new(0);
            assert_eq!(
                inspector.ensure_calibrated(|| {
                    attempts.set(1);
                    Err(failure)
                }),
                Err(failure)
            );
            assert_eq!(
                inspector.ensure_calibrated(|| {
                    attempts.set(2);
                    Ok(())
                }),
                Err(ExecutionError::Denied)
            );
            assert_eq!(attempts.get(), 1);
        }
        let inspector = RustProjectInspector::new(None);
        assert_eq!(
            inspector.ensure_calibrated(|| Err(ExecutionError::Cancelled)),
            Err(ExecutionError::Cancelled)
        );
        assert_eq!(inspector.ensure_calibrated(|| Ok(())), Ok(()));
        assert_eq!(
            inspector.ensure_calibrated(|| Err(ExecutionError::Denied)),
            Ok(())
        );
    }
}
