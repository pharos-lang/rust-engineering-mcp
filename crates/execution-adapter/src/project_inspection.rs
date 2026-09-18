//! Lazy approved runtime composition, invoked only from an admitted MCP worker.
use crate::{HostDockerConfig, RustGateway};
use rust_engineering_application::{
    DiagnosticExplainPort, ExecutionError, InspectionControl, InspectionError, ProjectError,
    ProjectInspectionPort, ProjectMutationPort, ProjectResolutionPort, ResolutionError,
    ToolchainInspectionPort,
};
use rust_engineering_domain::{
    CargoVendorSnapshot, DiagnosticCode, ExecutionFingerprint, ExecutionLimits, ExecutionResult,
    ExecutionTermination, ExplainObservation, MutationResolutionObservation, OperationalErrorCode,
    ProjectStructure, RuntimeIdentity, RustCommand, RustMutationCommand, RustMutationObservation,
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
            metadata_structure(source, result, || {
                gateway
                    .configuration_fingerprint()
                    .map_err(InspectionError::Execution)
            })
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

/// Classifies one `cargo metadata` execution and converts its output into the
/// project structure, bound to the runtime that produced it. `configuration`
/// is only consulted once the execution is known to be a clean exit.
fn metadata_structure(
    source: &SourceBundle,
    result: ExecutionResult,
    configuration: impl FnOnce() -> Result<ExecutionFingerprint, InspectionError>,
) -> Result<ProjectStructure, InspectionError> {
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
        configuration_fingerprint: configuration()?,
        execution_fingerprint: result.execution_fingerprint,
        // Both approved images preserve this stable toolchain, verified during
        // explicit provisioning. These are facts of that immutable identity.
        rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
        cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
        declared_toolchain: None,
    };
    super::project_metadata::parse(result.stdout.as_bytes(), source, runtime)
}

/// Bounds, classifies and parses one Cargo validation execution. Test runs
/// additionally accept a finished build with failing tests as complete.
fn cargo_observation(
    source: &SourceBundle,
    mut result: ExecutionResult,
    test_output: bool,
    configuration: impl FnOnce() -> Result<ExecutionFingerprint, InspectionError>,
) -> Result<(rust_engineering_domain::CheckObservation, Option<bool>), InspectionError> {
    use rust_engineering_domain::{CheckObservation, CheckOutcome};
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
    let archive = super::source_archive::encode(source).map_err(InspectionError::Execution)?;
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
                configuration_fingerprint: configuration()?,
                execution_fingerprint: result.execution_fingerprint,
                rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
                cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
                declared_toolchain: super::project_metadata::declared_toolchain(source)?,
            },
        },
        parsed.build_finished,
    ))
}

/// Bounds and classifies one `cargo fmt --check` execution. Only an empty
/// clean exit or a parsed non-empty diff with exit 1 is complete.
fn format_observation(
    source: &SourceBundle,
    mut result: ExecutionResult,
    configuration: impl FnOnce() -> Result<ExecutionFingerprint, InspectionError>,
) -> Result<rust_engineering_domain::FormatObservation, InspectionError> {
    use rust_engineering_domain::{CheckObservation, CheckOutcome, FormatObservation};
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
    let archive = super::source_archive::encode(source).map_err(InspectionError::Execution)?;
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
                configuration_fingerprint: configuration()?,
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
}

/// Parses the three accepted probe outputs (rustc, cargo, components, in that
/// order) into the toolchain inventory bound to the runtime that produced it.
fn toolchain_observation(
    source: &SourceBundle,
    outputs: Vec<String>,
    executions: Vec<ToolchainExecution>,
    image_id: &str,
    configuration: impl FnOnce() -> Result<ExecutionFingerprint, InspectionError>,
) -> Result<ToolchainObservation, InspectionError> {
    let [rustc, cargo, components]: [String; 3] =
        outputs.try_into().map_err(|_| InspectionError::Internal)?;
    let inventory = super::toolchain_metadata::parse(
        rustc.as_bytes(),
        cargo.as_bytes(),
        components.as_bytes(),
    )?;
    let archive = super::source_archive::encode(source).map_err(InspectionError::Execution)?;
    Ok(ToolchainObservation {
        inventory,
        declared_toolchain: super::project_metadata::declared_toolchain(source)?,
        source_fingerprint: super::digest(&archive)
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        runtime: ToolchainRuntime {
            platform: "linux/aarch64".into(),
            image_id: image_id.into(),
            configuration_fingerprint: configuration()?,
            executions,
        },
    })
}

/// Accepts one toolchain probe only as a clean, complete exit and pairs its
/// output with the execution that produced it.
fn toolchain_step(
    execution: ExecutionResult,
    command: ToolchainObservationCommand,
) -> Result<(ToolchainExecution, String), InspectionError> {
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
    Ok((
        ToolchainExecution {
            command,
            execution_fingerprint: execution.execution_fingerprint,
        },
        execution.stdout,
    ))
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
                let (execution, output) = toolchain_step(execution, observation_command)?;
                executions.push(execution);
                outputs.push(output);
            }
            control.check().map_err(InspectionError::Project)?;
            toolchain_observation(source, outputs, executions, gateway.image_id(), || {
                gateway
                    .configuration_fingerprint()
                    .map_err(InspectionError::Execution)
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
        let result = self.with_gateway(control, |gateway| {
            let result = gateway
                .execute(
                    source,
                    command,
                    ExecutionLimits::new(wall_ms, 256 * 1024).ok_or(InspectionError::Internal)?,
                    control,
                )
                .map_err(InspectionError::Execution)?;
            cargo_observation(source, result, test_output, || {
                gateway
                    .configuration_fingerprint()
                    .map_err(InspectionError::Execution)
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

impl rust_engineering_application::nextest::ProjectNextestPort for RustProjectInspector {
    fn run(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_application::nextest::NextestOptions,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_application::nextest::NextestObservation, InspectionError> {
        let result = self.with_gateway(control, |gateway| {
            super::nextest_port::run(gateway, source, options, control)
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

impl rust_engineering_application::mutation_test::ProjectMutationTestPort for RustProjectInspector {
    fn run(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_domain::mutation_test::MutationTestCommandOptions,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_application::mutation_test::MutationTestObservation, InspectionError>
    {
        let result = self.with_gateway(control, |gateway| {
            super::mutation_test_port::run(gateway, source, options, control)
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

impl rust_engineering_application::semver_check::ProjectSemverPort for RustProjectInspector {
    fn run(
        &self,
        baseline: &SourceBundle,
        candidate: &SourceBundle,
        options: &rust_engineering_application::semver_check::SemverOptions,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_application::semver_check::SemverObservation, InspectionError>
    {
        let result = self.with_gateway(control, |gateway| {
            super::semver_port::run(gateway, baseline, candidate, options, control)
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

impl rust_engineering_application::coverage::ProjectCoveragePort for RustProjectInspector {
    fn run(
        &self,
        source: &SourceBundle,
        options: &rust_engineering_domain::coverage::CoverageOptions,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_application::coverage::CoverageObservation, InspectionError> {
        let result = self.with_gateway(control, |gateway| {
            super::coverage_port::run(gateway, source, options, control)
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

impl rust_engineering_application::security::ProjectDenyPort for RustProjectInspector {
    fn deny(
        &self,
        source: &SourceBundle,
        vendor: &rust_engineering_domain::CargoVendorSnapshot,
        policy: &rust_engineering_domain::security::SecurityPolicy,
        options: &rust_engineering_domain::security::DenyOptions,
        control: &dyn InspectionControl,
    ) -> Result<
        rust_engineering_application::security::DenyObservation,
        rust_engineering_application::security::SecurityError,
    > {
        use rust_engineering_application::security::SecurityError;
        let result = self
            .with_gateway(control, |gateway| {
                Ok(super::security_port::run(
                    gateway, source, vendor, policy, options, control,
                ))
            })
            .map_err(SecurityError::from)
            .and_then(|result| result);
        if matches!(
            result,
            Err(SecurityError::Inspection(
                InspectionError::Execution(ExecutionError::CleanupUncertain)
                    | InspectionError::Internal
            ))
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl rust_engineering_application::unsafe_scan::ProjectUnsafeScanPort for RustProjectInspector {
    fn unsafe_scan(
        &self,
        source: &SourceBundle,
        vendor: &rust_engineering_domain::CargoVendorSnapshot,
        options: &rust_engineering_domain::unsafe_scan::UnsafeScanOptions,
        control: &dyn InspectionControl,
    ) -> Result<
        rust_engineering_application::unsafe_scan::UnsafeObservation,
        rust_engineering_application::security::SecurityError,
    > {
        use rust_engineering_application::security::SecurityError;
        let result = self
            .with_gateway(control, |gateway| {
                Ok(super::unsafe_port::run(
                    gateway, source, vendor, options, control,
                ))
            })
            .map_err(SecurityError::from)
            .and_then(|result| result);
        if matches!(
            result,
            Err(SecurityError::Inspection(
                InspectionError::Execution(ExecutionError::CleanupUncertain)
                    | InspectionError::Internal
            ))
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl rust_engineering_application::miri::ProjectMiriPort for RustProjectInspector {
    fn miri(
        &self,
        source: &SourceBundle,
        vendor: &rust_engineering_domain::CargoVendorSnapshot,
        options: &rust_engineering_domain::miri::MiriOptions,
        control: &dyn InspectionControl,
    ) -> Result<
        rust_engineering_application::miri::MiriObservation,
        rust_engineering_application::security::SecurityError,
    > {
        use rust_engineering_application::security::SecurityError;
        let result = self
            .with_gateway(control, |gateway| {
                Ok(super::miri_port::run(
                    gateway, source, vendor, options, control,
                ))
            })
            .map_err(SecurityError::from)
            .and_then(|result| result);
        if matches!(
            result,
            Err(SecurityError::Inspection(
                InspectionError::Execution(ExecutionError::CleanupUncertain)
                    | InspectionError::Internal
            ))
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl rust_engineering_application::benchmark::ProjectBenchmarkPort for RustProjectInspector {
    fn benchmark(
        &self,
        source: &SourceBundle,
        vendor: rust_engineering_application::vendor_capture::BenchmarkVendor<'_>,
        options: &rust_engineering_application::benchmark::BenchmarkRunOptions,
        control: &dyn InspectionControl,
    ) -> Result<
        rust_engineering_application::benchmark::BenchmarkObservation,
        rust_engineering_application::security::SecurityError,
    > {
        use rust_engineering_application::security::SecurityError;
        let result = self
            .with_gateway(control, |gateway| {
                Ok(super::performance_port::benchmark(
                    gateway, source, vendor, options, control,
                ))
            })
            .map_err(SecurityError::from)
            .and_then(|result| result);
        if matches!(
            result,
            Err(SecurityError::Inspection(
                InspectionError::Execution(ExecutionError::CleanupUncertain)
                    | InspectionError::Internal
            ))
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl rust_engineering_application::profile::ProjectProfilePort for RustProjectInspector {
    fn profile(
        &self,
        source: &SourceBundle,
        vendor: &rust_engineering_domain::CargoVendorSnapshot,
        options: &rust_engineering_domain::profile::ProfileOptions,
        control: &dyn InspectionControl,
    ) -> Result<
        rust_engineering_application::profile::ProfileObservation,
        rust_engineering_application::security::SecurityError,
    > {
        use rust_engineering_application::security::SecurityError;
        let result = self
            .with_gateway(control, |gateway| {
                Ok(super::performance_port::profile(
                    gateway, source, vendor, options, control,
                ))
            })
            .map_err(SecurityError::from)
            .and_then(|result| result);
        if matches!(
            result,
            Err(SecurityError::Inspection(
                InspectionError::Execution(ExecutionError::CleanupUncertain)
                    | InspectionError::Internal
            ))
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl rust_engineering_application::bloat::ProjectBloatPort for RustProjectInspector {
    fn bloat(
        &self,
        source: &SourceBundle,
        vendor: &rust_engineering_domain::CargoVendorSnapshot,
        options: &rust_engineering_domain::bloat::BloatOptions,
        control: &dyn InspectionControl,
    ) -> Result<
        rust_engineering_application::bloat::BloatObservation,
        rust_engineering_application::security::SecurityError,
    > {
        use rust_engineering_application::security::SecurityError;
        let result = self
            .with_gateway(control, |gateway| {
                Ok(super::performance_port::bloat(
                    gateway, source, vendor, options, control,
                ))
            })
            .map_err(SecurityError::from)
            .and_then(|result| result);
        if matches!(
            result,
            Err(SecurityError::Inspection(
                InspectionError::Execution(ExecutionError::CleanupUncertain)
                    | InspectionError::Internal
            ))
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
        result
    }
}

impl rust_engineering_application::ProjectFormatPort for RustProjectInspector {
    fn format(
        &self,
        source: &SourceBundle,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::FormatObservation, InspectionError> {
        let result = self.with_gateway(control, |gateway| {
            let result = gateway
                .execute(
                    source,
                    RustCommand::FormatCheck,
                    ExecutionLimits::new(30_000, 256 * 1024).ok_or(InspectionError::Internal)?,
                    control,
                )
                .map_err(InspectionError::Execution)?;
            format_observation(source, result, || {
                gateway
                    .configuration_fingerprint()
                    .map_err(InspectionError::Execution)
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

fn require_successful_mutation_execution(result: &ExecutionResult) -> Result<(), InspectionError> {
    match result.termination {
        ExecutionTermination::TimedOut => Err(InspectionError::Project(ProjectError::Rejected(
            OperationalErrorCode::CommandTimeout,
        ))),
        ExecutionTermination::Cancelled => Err(InspectionError::Project(ProjectError::Cancelled)),
        ExecutionTermination::OutputLimit => Err(InspectionError::OutputLimit),
        ExecutionTermination::Exited if result.stdout_truncated || result.stderr_truncated => {
            Err(InspectionError::OutputLimit)
        }
        ExecutionTermination::Exited
            if result.exit_code != Some(0)
                || result.oom_killed != Some(false)
                || !result.stdout.is_empty()
                || !result.stderr.is_empty() =>
        {
            Err(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::InvalidProject,
            )))
        }
        ExecutionTermination::Exited => Ok(()),
    }
}

fn invalid_mutation() -> InspectionError {
    InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::InvalidProject))
}

fn require_successful_fix_execution(
    result: &ExecutionResult,
    candidate: &SourceBundle,
) -> Result<(), InspectionError> {
    require_successful_fix_envelope(result)?;
    let parsed = super::cargo_diagnostics::parse(&result.stdout, candidate, true)
        .map_err(|_| invalid_mutation())?;
    if parsed.complete && parsed.build_finished == Some(true) {
        Ok(())
    } else {
        Err(invalid_mutation())
    }
}

fn require_successful_fix_envelope(result: &ExecutionResult) -> Result<(), InspectionError> {
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
    if result.stdout_truncated || result.stderr_truncated {
        return Err(InspectionError::OutputLimit);
    }
    if result.exit_code != Some(0) || result.oom_killed != Some(false) {
        return Err(invalid_mutation());
    }
    Ok(())
}

fn require_successful_check_execution(
    result: &ExecutionResult,
    candidate: &SourceBundle,
) -> Result<(), InspectionError> {
    require_successful_fix_execution(result, candidate)
}

fn mutation_execution_error(error: ExecutionError) -> InspectionError {
    if error == ExecutionError::Cancelled {
        InspectionError::Project(ProjectError::Cancelled)
    } else {
        InspectionError::Execution(error)
    }
}

/// Binds an accepted mutation candidate to the post-check runtime that
/// verified it and to the mutation execution that produced it.
fn mutation_observation(
    candidate: SourceBundle,
    postcheck: ExecutionResult,
    mutation_execution_fingerprint: ExecutionFingerprint,
    configuration: impl FnOnce() -> Result<ExecutionFingerprint, InspectionError>,
) -> Result<RustMutationObservation, InspectionError> {
    let archive = super::source_archive::encode(&candidate).map_err(InspectionError::Execution)?;
    let declared_toolchain = super::project_metadata::declared_toolchain(&candidate)?;
    Ok(RustMutationObservation {
        candidate,
        runtime: RuntimeIdentity {
            platform: postcheck.platform.into(),
            image_id: postcheck.image_id,
            configuration_fingerprint: configuration()?,
            execution_fingerprint: postcheck.execution_fingerprint,
            rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
            cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
            declared_toolchain,
        },
        mutation_execution_fingerprint,
        candidate_source_fingerprint: super::digest(&archive)
            .parse()
            .map_err(|_| InspectionError::Internal)?,
    })
}

impl ProjectMutationPort for RustProjectInspector {
    fn mutate(
        &self,
        source: &SourceBundle,
        command: RustMutationCommand,
        control: &dyn InspectionControl,
    ) -> Result<RustMutationObservation, InspectionError> {
        let result = self.with_gateway(control, |gateway| {
            let mutation = gateway
                .execute_mutation(
                    source,
                    command.clone(),
                    ExecutionLimits::new(30_000, 256 * 1024).ok_or(InspectionError::Internal)?,
                    control,
                )
                .map_err(mutation_execution_error)?;
            match command {
                RustMutationCommand::Format => {
                    require_successful_mutation_execution(&mutation.result)?;
                }
                RustMutationCommand::Fix => {
                    require_successful_fix_envelope(&mutation.result)?;
                }
            }
            let candidate =
                mutation
                    .candidate
                    .ok_or(InspectionError::Project(ProjectError::Rejected(
                        OperationalErrorCode::InvalidProject,
                    )))?;
            if matches!(command, RustMutationCommand::Fix) {
                require_successful_fix_execution(&mutation.result, &candidate)?;
            }
            let postcheck_command = match command {
                RustMutationCommand::Format => RustCommand::FormatCheck,
                RustMutationCommand::Fix => RustCommand::Check,
            };
            let postcheck = gateway
                .execute(
                    &candidate,
                    postcheck_command,
                    ExecutionLimits::new(30_000, 256 * 1024).ok_or(InspectionError::Internal)?,
                    control,
                )
                .map_err(mutation_execution_error)?;
            match command {
                RustMutationCommand::Format => {
                    require_successful_mutation_execution(&postcheck)?;
                }
                RustMutationCommand::Fix => {
                    require_successful_check_execution(&postcheck, &candidate)?;
                }
            }
            control.check().map_err(InspectionError::Project)?;
            mutation_observation(
                candidate,
                postcheck,
                mutation.result.execution_fingerprint,
                || {
                    gateway
                        .configuration_fingerprint()
                        .map_err(InspectionError::Execution)
                },
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

impl ProjectResolutionPort for RustProjectInspector {
    fn resolve(
        &self,
        edited: &SourceBundle,
        dataset: &CargoVendorSnapshot,
        control: &dyn InspectionControl,
    ) -> Result<MutationResolutionObservation, ResolutionError> {
        let semantic_error = std::cell::Cell::new(None);
        let inspected = self.with_gateway(control, |gateway| {
            super::resolution_gateway::execute(gateway, edited, dataset, control).map_err(|error| {
                match error {
                    ResolutionError::Inspection(error) => error,
                    error => {
                        semantic_error.set(Some(error));
                        InspectionError::Internal
                    }
                }
            })
        });
        let result = if let Some(error) = semantic_error.get() {
            Err(error)
        } else {
            inspected.map_err(ResolutionError::Inspection)
        };
        if matches!(
            result,
            Err(ResolutionError::Inspection(
                InspectionError::Execution(ExecutionError::CleanupUncertain)
                    | InspectionError::Internal
            ))
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

impl rust_engineering_application::supply_chain::SupplyFactsPort for RustProjectInspector {
    fn supply_facts(
        &self,
        source: &rust_engineering_domain::SourceBundle,
        vendor: Option<&rust_engineering_domain::CargoVendorSnapshot>,
        deny: Option<&rust_engineering_application::security::DenyObservation>,
        control: &dyn rust_engineering_application::InspectionControl,
    ) -> Result<
        rust_engineering_domain::supply_chain::SupplyGraph,
        rust_engineering_application::security::SecurityError,
    > {
        crate::supply_facts::facts(source, vendor, deny, control)
    }
}

/// M6-01: the one door from `rust.analyzer.symbols` into the guest analyzer
/// session. Reuses [`RustProjectInspector::with_gateway`] exactly like every
/// other port on this type; no separate lend method is needed because this
/// `impl` already lives beside it in the same file.
impl rust_engineering_application::analyzer::AnalyzerPort for RustProjectInspector {
    fn analyze(
        &self,
        source: &SourceBundle,
        query: &rust_engineering_domain::AnalyzerQuery,
        limits: ExecutionLimits,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_application::analyzer::AnalyzerObservation, InspectionError> {
        // The same bundle digest `check`/`cargo_run` already compute for a
        // `SourceBundle`: no second hashing scheme for M6. Computed before the
        // single-flight gateway lock is taken (V05 P3): hashing bytes already
        // captured needs no exclusivity, and holding the lock only for
        // `execute_analyzer` shortens every other call's wait.
        let archive = super::source_archive::encode(source).map_err(InspectionError::Execution)?;
        let source_fingerprint = super::digest(&archive)
            .parse()
            .map_err(|_| InspectionError::Internal)?;
        let result = self.with_gateway(control, |gateway| {
            let execution = gateway
                .execute_analyzer(source, query, limits, control)
                .map_err(InspectionError::Execution)?;
            Ok(
                rust_engineering_application::analyzer::AnalyzerObservation {
                    source_fingerprint,
                    execution,
                },
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

    /// M6-04: one `CodeActions` session, with every applicable action's
    /// digest computed against the same bundle digest `analyze` publishes.
    fn resolve_actions(
        &self,
        source: &SourceBundle,
        file: &rust_engineering_domain::AnalyzerFile,
        range: rust_engineering_domain::TextRange,
        only: &[rust_engineering_domain::CodeActionKind],
        limits: ExecutionLimits,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_application::analyzer::ActionsObservation, InspectionError> {
        let query = rust_engineering_domain::AnalyzerQuery::CodeActions {
            file: file.clone(),
            range,
            only: only.to_vec(),
        };
        // V07 P3-6: the bundle digest is computed inside the quarantine scope,
        // so its failure quarantines exactly as a gateway failure does.
        let result = analyzer_source_fingerprint(source).and_then(|source_fingerprint| {
            self.with_gateway(control, |gateway| {
                let execution = gateway
                    .execute_analyzer(source, &query, limits, control)
                    .map_err(InspectionError::Execution)?;
                let action_digests =
                    super::analyzer_gateway::action_digests(&execution, &source_fingerprint)
                        .map_err(InspectionError::Execution)?;
                Ok(rust_engineering_application::analyzer::ActionsObservation {
                    observation: rust_engineering_application::analyzer::AnalyzerObservation {
                        source_fingerprint: source_fingerprint.clone(),
                        execution,
                    },
                    action_digests,
                })
            })
        });
        self.quarantine_if_uncertain(&result);
        result
    }

    /// M6-04: the apply-preview resolution. The gateway resolves the action
    /// by digest in a fresh session; nothing is applied here.
    fn resolve_action_candidate(
        &self,
        source: &SourceBundle,
        file: &rust_engineering_domain::AnalyzerFile,
        range: rust_engineering_domain::TextRange,
        action_digest: &rust_engineering_domain::SourceFingerprint,
        limits: ExecutionLimits,
        control: &dyn InspectionControl,
    ) -> Result<rust_engineering_application::analyzer::ActionApplyObservation, InspectionError>
    {
        // V07 P3-6: inside the quarantine scope, as in `resolve_actions`.
        let result = analyzer_source_fingerprint(source).and_then(|source_fingerprint| {
            self.with_gateway(control, |gateway| {
                let resolved = super::analyzer_gateway::resolve_action_candidate(
                    gateway,
                    source,
                    &source_fingerprint,
                    super::analyzer_gateway::ActionLookup {
                        file,
                        range,
                        action_digest,
                    },
                    limits,
                    control,
                )
                .map_err(InspectionError::Execution)?;
                Ok(
                    rust_engineering_application::analyzer::ActionApplyObservation {
                        observation: rust_engineering_application::analyzer::AnalyzerObservation {
                            source_fingerprint: source_fingerprint.clone(),
                            execution: resolved.execution,
                        },
                        runtime: resolved.runtime,
                        resolution: resolved.resolution,
                    },
                )
            })
        });
        self.quarantine_if_uncertain(&result);
        result
    }
}

/// The bundle digest every analyzer answer publishes, computed before the
/// single-flight gateway lock is taken (V05 P3).
fn analyzer_source_fingerprint(
    source: &SourceBundle,
) -> Result<rust_engineering_domain::SourceFingerprint, InspectionError> {
    let archive = super::source_archive::encode(source).map_err(InspectionError::Execution)?;
    super::digest(&archive)
        .parse()
        .map_err(|_| InspectionError::Internal)
}

impl RustProjectInspector {
    /// The quarantine rule every port on this type applies to its result.
    fn quarantine_if_uncertain<T>(&self, result: &Result<T, InspectionError>) {
        if matches!(
            result,
            Err(InspectionError::Execution(ExecutionError::CleanupUncertain)
                | InspectionError::Internal)
        ) {
            self.quarantined.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::SourceFile;
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
    fn successful_empty_execution() -> Result<ExecutionResult, Box<dyn std::error::Error>> {
        let mut execution = explanation_execution()?;
        execution.stdout.clear();
        Ok(execution)
    }
    fn source_file(path: &str, bytes: &[u8]) -> Result<SourceFile, Box<dyn std::error::Error>> {
        SourceFile::new(path.into(), bytes.to_vec())
            .map_err(|error| std::io::Error::other(format!("{error:?}")).into())
    }
    fn source_bundle(files: Vec<SourceFile>) -> Result<SourceBundle, Box<dyn std::error::Error>> {
        SourceBundle::new(files).map_err(|error| std::io::Error::other(format!("{error:?}")).into())
    }

    #[test]
    fn mutation_and_postcheck_require_complete_clean_success()
    -> Result<(), Box<dyn std::error::Error>> {
        let success = successful_empty_execution()?;
        assert_eq!(require_successful_mutation_execution(&success), Ok(()));
        for termination in [
            ExecutionTermination::TimedOut,
            ExecutionTermination::Cancelled,
            ExecutionTermination::OutputLimit,
        ] {
            let mut changed = success.clone();
            changed.termination = termination;
            let error = require_successful_mutation_execution(&changed);
            match termination {
                ExecutionTermination::TimedOut => assert!(matches!(
                    error,
                    Err(InspectionError::Project(ProjectError::Rejected(
                        OperationalErrorCode::CommandTimeout
                    )))
                )),
                ExecutionTermination::Cancelled => assert_eq!(
                    error,
                    Err(InspectionError::Project(ProjectError::Cancelled))
                ),
                ExecutionTermination::OutputLimit => {
                    assert_eq!(error, Err(InspectionError::OutputLimit));
                }
                ExecutionTermination::Exited => unreachable!(),
            }
        }
        for stdout_stream in [true, false] {
            let mut changed = success.clone();
            if stdout_stream {
                changed.stdout_truncated = true;
            } else {
                changed.stderr_truncated = true;
            }
            assert_eq!(
                require_successful_mutation_execution(&changed),
                Err(InspectionError::OutputLimit)
            );
        }
        for mutation in 0..4 {
            let mut changed = success.clone();
            match mutation {
                0 => changed.exit_code = Some(1),
                1 => changed.oom_killed = Some(true),
                2 => changed.stdout = "unexpected".into(),
                _ => changed.stderr = "unexpected".into(),
            }
            assert!(matches!(
                require_successful_mutation_execution(&changed),
                Err(InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::InvalidProject
                )))
            ));
        }
        Ok(())
    }

    #[test]
    fn fix_accepts_progress_stderr_only_with_complete_successful_cargo_json()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = source_bundle(vec![source_file(
            "src/lib.rs",
            b"pub fn answer() -> u32 { 42 }\n",
        )?])?;
        let mut execution = successful_empty_execution()?;
        execution.stdout = "{\"reason\":\"build-finished\",\"success\":true}\n".into();
        execution.stderr = "    Checking fixture v0.1.0 (/source)\n".into();
        assert_eq!(
            require_successful_fix_execution(&execution, &source),
            Ok(())
        );
        for stdout in [
            "{\"reason\":\"build-finished\",\"success\":false}\n",
            "{\"reason\":\"build-finished\",\"success\":true}",
            "{\"reason\":\"unknown\"}\n",
        ] {
            let mut invalid = execution.clone();
            invalid.stdout = stdout.into();
            assert_eq!(
                require_successful_fix_execution(&invalid, &source),
                Err(InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::InvalidProject
                )))
            );
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit approved Docker socket/image; production inspector mutation fixture"]
    fn production_inspector_formats_postchecks_rejects_and_cancels()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_application::{ExecutionCancellation, OperationControl};
        struct Control {
            started: std::time::Instant,
            cancel_after_ms: Option<u64>,
        }
        impl ExecutionCancellation for Control {
            fn is_cancelled(&self) -> bool {
                self.cancel_after_ms
                    .is_some_and(|millis| self.started.elapsed().as_millis() >= u128::from(millis))
            }
        }
        impl OperationControl for Control {
            fn check(&self) -> Result<(), ProjectError> {
                if self.is_cancelled() {
                    Err(ProjectError::Cancelled)
                } else {
                    Ok(())
                }
            }
        }
        let suffix = super::super::state::nonce().map_err(|error| format!("nonce: {error:?}"))?;
        let state_root = std::path::PathBuf::from("/private/tmp")
            .join(format!("rust-mcp-inspector-mutation-{suffix}"));
        std::fs::create_dir(&state_root)?;
        let config = HostDockerConfig {
            executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
            socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
            state_root: state_root.clone(),
            image_id: crate::APPROVED_RUST_IMAGE.into(),
        };
        let gateway =
            RustGateway::new(config.clone()).map_err(|error| format!("gateway: {error:?}"))?;
        gateway.set_verified(true);
        let inspector = RustProjectInspector {
            config: Some(config),
            gateway: Mutex::new(Some(gateway)),
            calibrated: AtomicBool::new(true),
            calibration_failed: AtomicBool::new(false),
            quarantined: AtomicBool::new(false),
        };
        let source =
            SourceBundle::new(vec![
            SourceFile::new(
                "Cargo.toml".into(),
                b"[package]\nname = \"inspector_fmt\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
                    .to_vec(),
            )
            .map_err(|error| format!("source: {error:?}"))?,
            SourceFile::new("src/main.rs".into(), b"fn main( ){println!(\"ok\");}\n".to_vec())
                .map_err(|error| format!("source: {error:?}"))?,
        ])
            .map_err(|error| format!("bundle: {error:?}"))?;
        let control = Control {
            started: std::time::Instant::now(),
            cancel_after_ms: None,
        };
        let observation = inspector
            .mutate(&source, RustMutationCommand::Format, &control)
            .map_err(|error| format!("mutate: {error:?}"))?;
        assert_eq!(
            observation.candidate.files()[1].bytes(),
            b"fn main() {\n    println!(\"ok\");\n}\n"
        );
        assert_ne!(
            observation.mutation_execution_fingerprint,
            observation.runtime.execution_fingerprint
        );
        let archive = super::super::source_archive::encode(&observation.candidate)
            .map_err(|error| format!("archive: {error:?}"))?;
        assert_eq!(
            observation.candidate_source_fingerprint.to_string(),
            super::super::digest(&archive)
        );
        let invalid = SourceBundle::new(vec![
            SourceFile::new("Cargo.toml".into(), b"[package\n".to_vec())
                .map_err(|error| format!("source: {error:?}"))?,
            SourceFile::new("src/main.rs".into(), b"fn main() {}\n".to_vec())
                .map_err(|error| format!("source: {error:?}"))?,
        ])
        .map_err(|error| format!("bundle: {error:?}"))?;
        assert!(matches!(
            inspector.mutate(&invalid, RustMutationCommand::Format, &control),
            Err(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::InvalidProject
            )))
        ));
        let fix_manifest = b"[package]\nname = \"fix_probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\ndefault = [\"enabled\"]\nenabled = []\n";
        let fix_lock = b"# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"fix_probe\"\nversion = \"0.1.0\"\n";
        let fix_source = source_bundle(vec![
            source_file("Cargo.toml", fix_manifest)?,
            source_file("Cargo.lock", fix_lock)?,
            source_file(
                "src/lib.rs",
                b"#[cfg(not(feature = \"enabled\"))]\ncompile_error!(\"default feature disabled\");\n\npub fn answer() -> u32 {\n    let mut value = 42;\n    value\n}\n"
            )?,
        ])?;
        let fixed = inspector
            .mutate(&fix_source, RustMutationCommand::Fix, &control)
            .map_err(|error| format!("fix: {error:?}"))?;
        assert_eq!(fixed.candidate.files()[0].bytes(), fix_lock);
        assert_eq!(fixed.candidate.files()[1].bytes(), fix_manifest);
        assert!(
            std::str::from_utf8(fixed.candidate.files()[2].bytes())?.contains("let value = 42;")
        );
        assert_ne!(
            fixed.mutation_execution_fingerprint,
            fixed.runtime.execution_fingerprint
        );
        let missing_lock = source_bundle(vec![
            source_file("Cargo.toml", fix_manifest)?,
            source_file("src/lib.rs", b"pub fn answer() {}\n")?,
        ])?;
        assert!(matches!(
            inspector.mutate(&missing_lock, RustMutationCommand::Fix, &control),
            Err(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::InvalidProject
            )))
        ));
        let hostile_manifest = b"[package]\nname = \"hostile_fix\"\nversion = \"0.1.0\"\nedition = \"2024\"\nbuild = \"build.rs\"\n";
        let hostile_lock = b"# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"hostile_fix\"\nversion = \"0.1.0\"\n";
        let hostile = source_bundle(vec![
            source_file("Cargo.toml", hostile_manifest)?,
            source_file("Cargo.lock", hostile_lock)?,
            source_file(
                "build.rs",
                b"fn main() { std::fs::write(\"Cargo.toml\", b\"[package]\\nname='changed'\\nversion='0.1.0'\\n\").unwrap(); }\n",
            )?,
            source_file(
                "src/lib.rs",
                b"pub fn answer() -> u32 { let mut value = 42; value }\n",
            )?,
        ])?;
        assert!(matches!(
            inspector.mutate(&hostile, RustMutationCommand::Fix, &control),
            Err(InspectionError::Execution(ExecutionError::Denied))
        ));
        let cancelled = Control {
            started: std::time::Instant::now(),
            cancel_after_ms: Some(100),
        };
        assert!(matches!(
            inspector.mutate(&source, RustMutationCommand::Format, &cancelled),
            Err(InspectionError::Project(ProjectError::Cancelled))
        ));
        let cancelled_fix = Control {
            started: std::time::Instant::now(),
            cancel_after_ms: Some(100),
        };
        assert!(matches!(
            inspector.mutate(&fix_source, RustMutationCommand::Fix, &cancelled_fix),
            Err(InspectionError::Project(ProjectError::Cancelled))
        ));
        assert!(!inspector.is_quarantined());
        {
            let gateway = inspector.gateway.lock().map_err(|_| "gateway lock")?;
            let gateway = gateway.as_ref().ok_or("gateway missing")?;
            for kind in ["container", "volume"] {
                let arguments = if kind == "container" {
                    vec![
                        "container".into(),
                        "ls".into(),
                        "--all".into(),
                        "--filter=label=org.rust-mcp.execution=true".into(),
                        "--format={{.ID}}".into(),
                    ]
                } else {
                    vec![
                        "volume".into(),
                        "ls".into(),
                        "--filter=label=org.rust-mcp.execution=true".into(),
                        "--format={{.Name}}".into(),
                    ]
                };
                let inventory = gateway
                    .inner
                    .control(&arguments)
                    .map_err(|error| format!("inventory: {error:?}"))?;
                assert_eq!(inventory.code, Some(0));
                assert!(inventory.stdout.iter().all(u8::is_ascii_whitespace));
            }
        }
        drop(inspector);
        std::fs::remove_dir_all(state_root)?;
        Ok(())
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

#[cfg(test)]
mod observation_tests {
    //! Daemon-free evidence for the observation builders extracted from the
    //! gateway closures: classification, bounds, error order and runtime
    //! binding, plus the fail-closed prologue shared by every port.
    use super::*;
    use rust_engineering_application::{ExecutionCancellation, OperationControl};
    use rust_engineering_domain::{CheckOutcome, SourceFile};
    use std::cell::Cell;

    type TestResult = Result<(), String>;
    trait Checked<T> {
        fn c(self) -> Result<T, String>;
    }
    impl<T, E: std::fmt::Debug> Checked<T> for Result<T, E> {
        fn c(self) -> Result<T, String> {
            self.map_err(|error| format!("{error:?}"))
        }
    }
    const MANIFEST: &[u8] = b"[package]\nname='root'\nversion='1.2.3'\nedition='2024'\n";

    fn fingerprint(fill: char) -> Result<ExecutionFingerprint, String> {
        format!("sha256:{}", fill.to_string().repeat(64))
            .parse()
            .c()
    }
    fn source() -> Result<SourceBundle, String> {
        SourceBundle::new(vec![
            SourceFile::new("Cargo.toml".into(), MANIFEST.to_vec()).c()?,
            SourceFile::new("src/lib.rs".into(), b"pub fn f() {}\n".to_vec()).c()?,
        ])
        .c()
    }
    fn execution(stdout: &str) -> Result<ExecutionResult, String> {
        Ok(ExecutionResult {
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            oom_killed: Some(false),
            stdout: stdout.into(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            duration_ms: 1,
            total_duration_ms: 2,
            execution_fingerprint: fingerprint('e')?,
            platform: "linux/aarch64",
            image_id: crate::APPROVED_RUST_IMAGE.into(),
        })
    }
    fn source_digest(source: &SourceBundle) -> Result<String, String> {
        Ok(super::super::digest(
            &super::super::source_archive::encode(source).c()?,
        ))
    }
    /// A configuration closure that records whether it was consulted.
    fn configuration(
        called: &Cell<bool>,
    ) -> impl FnOnce() -> Result<ExecutionFingerprint, InspectionError> + '_ {
        move || {
            called.set(true);
            format!("sha256:{}", "c".repeat(64))
                .parse()
                .map_err(|_| InspectionError::Internal)
        }
    }
    fn failing_configuration() -> Result<ExecutionFingerprint, InspectionError> {
        Err(InspectionError::Execution(ExecutionError::Unavailable))
    }
    fn terminations() -> [(ExecutionTermination, InspectionError); 3] {
        [
            (
                ExecutionTermination::TimedOut,
                InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::CommandTimeout,
                )),
            ),
            (
                ExecutionTermination::Cancelled,
                InspectionError::Project(ProjectError::Cancelled),
            ),
            (
                ExecutionTermination::OutputLimit,
                InspectionError::OutputLimit,
            ),
        ]
    }

    const METADATA: &str = r#"{"version":1,"resolve":null,"workspace_root":"/source","packages":[{"id":"opaque","name":"root","version":"1.2.3","source":null,"manifest_path":"/source/Cargo.toml","edition":"2024","rust_version":null,"dependencies":[],"features":{},"targets":[{"name":"root","kind":["lib"],"crate_types":["lib"],"src_path":"/source/src/lib.rs","edition":"2024","test":true,"doctest":true}]}],"workspace_members":["opaque"],"workspace_default_members":["opaque"]}"#;

    #[test]
    fn metadata_structure_binds_runtime_only_after_a_clean_exit() -> TestResult {
        let source = source()?;
        let called = Cell::new(false);
        let structure =
            metadata_structure(&source, execution(METADATA)?, configuration(&called)).c()?;
        assert!(called.get());
        assert_eq!(structure.packages.len(), 1);
        assert_eq!(structure.packages[0].manifest_path, "Cargo.toml");
        assert_eq!(
            structure.runtime.configuration_fingerprint,
            fingerprint('c')?
        );
        assert_eq!(structure.runtime.execution_fingerprint, fingerprint('e')?);
        assert_eq!(structure.runtime.rust_version, "1.98.1");
        assert_eq!(structure.runtime.cargo_version, "1.98.1");
        assert_eq!(structure.runtime.declared_toolchain, None);
        for (termination, error) in terminations() {
            let called = Cell::new(false);
            let mut result = execution(METADATA)?;
            result.termination = termination;
            assert_eq!(
                metadata_structure(&source, result, configuration(&called)).err(),
                Some(error)
            );
            assert!(!called.get(), "{termination:?}");
        }
        let called = Cell::new(false);
        let mut failed = execution(METADATA)?;
        failed.exit_code = Some(101);
        assert_eq!(
            metadata_structure(&source, failed, configuration(&called)).err(),
            Some(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::InvalidProject
            )))
        );
        assert!(!called.get());
        assert_eq!(
            metadata_structure(&source, execution(METADATA)?, failing_configuration).err(),
            Some(InspectionError::Execution(ExecutionError::Unavailable))
        );
        assert_eq!(
            metadata_structure(&source, execution("{}")?, configuration(&Cell::new(false))).err(),
            Some(InspectionError::InvalidMetadata)
        );
        Ok(())
    }

    const FINISHED: &str = "{\"reason\":\"build-finished\",\"success\":true}\n";
    const FAILED_BUILD: &str = "{\"reason\":\"build-finished\",\"success\":false}\n";

    #[test]
    fn cargo_observation_classifies_every_outcome() -> TestResult {
        let source = source()?;
        let observe = |stdout: &str, exit: Option<i32>, test_output: bool| -> Result<_, String> {
            let mut result = execution(stdout)?;
            result.exit_code = exit;
            cargo_observation(
                &source,
                result,
                test_output,
                configuration(&Cell::new(false)),
            )
            .c()
        };
        let (passed, finished) = observe(FINISHED, Some(0), false)?;
        assert_eq!(passed.outcome, CheckOutcome::Passed);
        assert!(passed.validation_complete);
        assert_eq!(finished, Some(true));
        assert_eq!(
            passed.source_fingerprint.to_string(),
            source_digest(&source)?
        );
        assert_eq!(passed.runtime.configuration_fingerprint, fingerprint('c')?);
        assert_eq!(passed.runtime.image_id, crate::APPROVED_RUST_IMAGE);
        assert_eq!(passed.runtime.declared_toolchain, None);
        let (failed, finished) = observe(FAILED_BUILD, Some(101), false)?;
        assert_eq!(failed.outcome, CheckOutcome::Failed);
        assert_eq!(finished, Some(false));
        // A finished build with a failing exit is only complete for test runs.
        assert_eq!(
            observe(FINISHED, Some(101), false)?.0.outcome,
            CheckOutcome::Incomplete
        );
        assert_eq!(
            observe(FINISHED, Some(101), true)?.0.outcome,
            CheckOutcome::Failed
        );
        assert_eq!(
            observe("", Some(0), false)?.0.outcome,
            CheckOutcome::Incomplete
        );
        let mut frozen = execution("")?;
        frozen.exit_code = Some(101);
        frozen.stderr = "error: cannot update the lock file /source/Cargo.lock because --frozen was passed to prevent this\n".into();
        let (lock, _) =
            cargo_observation(&source, frozen, false, configuration(&Cell::new(false))).c()?;
        assert_eq!(lock.outcome, CheckOutcome::LockfileUpdateRequired);
        let mut truncated = execution(FINISHED)?;
        truncated.stderr_truncated = true;
        let (incomplete, _) =
            cargo_observation(&source, truncated, false, configuration(&Cell::new(false))).c()?;
        assert_eq!(incomplete.outcome, CheckOutcome::Incomplete);
        let mut oversized = execution(FINISHED)?;
        oversized.stderr = "é".repeat(200 * 1024);
        let (bounded, _) =
            cargo_observation(&source, oversized, false, configuration(&Cell::new(false))).c()?;
        assert!(bounded.stderr_truncated);
        assert!(bounded.stderr.len() <= 256 * 1024);
        assert_eq!(bounded.outcome, CheckOutcome::Incomplete);
        let called = Cell::new(false);
        let mut cancelled = execution(FINISHED)?;
        cancelled.termination = ExecutionTermination::Cancelled;
        assert_eq!(
            cargo_observation(&source, cancelled, false, configuration(&called)).err(),
            Some(InspectionError::Project(ProjectError::Cancelled))
        );
        assert!(!called.get());
        let mut timed_out = execution(FINISHED)?;
        timed_out.termination = ExecutionTermination::TimedOut;
        let (partial, _) =
            cargo_observation(&source, timed_out, false, configuration(&Cell::new(false))).c()?;
        assert_eq!(partial.outcome, CheckOutcome::Incomplete);
        assert_eq!(partial.termination, ExecutionTermination::TimedOut);
        assert_eq!(
            cargo_observation(&source, execution(FINISHED)?, false, failing_configuration).err(),
            Some(InspectionError::Execution(ExecutionError::Unavailable))
        );
        Ok(())
    }

    #[test]
    fn format_observation_accepts_only_clean_or_parsed_diff_exits() -> TestResult {
        let source = source()?;
        let observe = |stdout: &str, stderr: &str, exit: Option<i32>| -> Result<_, String> {
            let mut result = execution(stdout)?;
            result.stderr = stderr.into();
            result.exit_code = exit;
            format_observation(&source, result, configuration(&Cell::new(false))).c()
        };
        let clean = observe("", "", Some(0))?;
        assert_eq!(clean.execution.outcome, CheckOutcome::Passed);
        assert!(clean.affected_files.is_empty() && clean.diff.is_none());
        assert_eq!(
            clean.execution.runtime.configuration_fingerprint,
            fingerprint('c')?
        );
        assert_eq!(
            clean.execution.source_fingerprint.to_string(),
            source_digest(&source)?
        );
        let diff = observe(
            "Diff in /source/src/lib.rs:1:\n-pub fn f() {}\n+pub fn f() {}\n",
            "",
            Some(1),
        )?;
        assert_eq!(diff.execution.outcome, CheckOutcome::Failed);
        assert_eq!(diff.affected_files, ["src/lib.rs"]);
        assert!(diff.diff.is_some());
        for (stdout, stderr, exit) in [
            ("", "", Some(1)),
            ("", "warning", Some(0)),
            ("Diff in /source/src/lib.rs:1:\n-a\n+b\n", "", Some(0)),
            ("", "", Some(2)),
        ] {
            assert_eq!(
                observe(stdout, stderr, exit)?.execution.outcome,
                CheckOutcome::Incomplete,
                "{stdout:?} {stderr:?} {exit:?}"
            );
        }
        let called = Cell::new(false);
        let mut cancelled = execution("")?;
        cancelled.termination = ExecutionTermination::Cancelled;
        assert_eq!(
            format_observation(&source, cancelled, configuration(&called)).err(),
            Some(InspectionError::Project(ProjectError::Cancelled))
        );
        assert!(!called.get());
        Ok(())
    }

    #[test]
    fn toolchain_steps_accept_only_clean_complete_exits() -> TestResult {
        let (step, output) = toolchain_step(
            execution("rustc 1.98.1")?,
            ToolchainObservationCommand::CompilerVersion,
        )
        .c()?;
        assert_eq!(output, "rustc 1.98.1");
        assert_eq!(step.command, ToolchainObservationCommand::CompilerVersion);
        assert_eq!(step.execution_fingerprint, fingerprint('e')?);
        for (termination, error) in terminations() {
            let mut result = execution("x")?;
            result.termination = termination;
            assert_eq!(
                toolchain_step(result, ToolchainObservationCommand::CargoVersion).err(),
                Some(error)
            );
        }
        let mut failed = execution("x")?;
        failed.exit_code = Some(1);
        assert_eq!(
            toolchain_step(failed, ToolchainObservationCommand::InstalledComponents).err(),
            Some(InspectionError::Execution(ExecutionError::Unavailable))
        );
        for stdout_truncated in [true, false] {
            let mut truncated = execution("x")?;
            truncated.stdout_truncated = stdout_truncated;
            truncated.stderr_truncated = !stdout_truncated;
            assert_eq!(
                toolchain_step(truncated, ToolchainObservationCommand::CargoVersion).err(),
                Some(InspectionError::OutputLimit)
            );
        }
        Ok(())
    }

    const RUSTC: &str = "rustc 1.98.1 (48a229cea 2026-09-01)\nbinary: rustc\ncommit-hash: 48a229ceaefd4985c50990b14116b6d856af0985\ncommit-date: 2026-09-01\nhost: aarch64-unknown-linux-gnu\nrelease: 1.98.1\nLLVM version: 22.1.8\n";
    const CARGO: &str = "cargo 1.98.1 (797e8a9bc 2026-08-05)\nrelease: 1.98.1\ncommit-hash: 797e8a9bca276c1c9f9f738d2a20f484fa4eea9d\ncommit-date: 2026-08-05\nhost: aarch64-unknown-linux-gnu\nlibgit2: 1.9.4 (sys:0.21.0 vendored)\nlibcurl: 8.21.0-DEV (sys:0.4.90+curl-8.21.0 vendored ssl:OpenSSL/3.6.3)\nssl: OpenSSL 3.6.3 9 Jun 2026\nos: Debian 12.0.0 (bookworm) [64-bit]\n";
    const COMPONENTS: &str = "rustfmt-preview\nrustc\nrust-std-aarch64-unknown-linux-gnu\nclippy-preview\ncargo\nllvm-tools-preview\n";

    #[test]
    fn toolchain_observation_parses_three_outputs_in_order() -> TestResult {
        let source = source()?;
        let executions = vec![ToolchainExecution {
            command: ToolchainObservationCommand::CompilerVersion,
            execution_fingerprint: fingerprint('e')?,
        }];
        let outputs = || vec![RUSTC.to_owned(), CARGO.to_owned(), COMPONENTS.to_owned()];
        let observation = toolchain_observation(
            &source,
            outputs(),
            executions.clone(),
            "sha256:image",
            configuration(&Cell::new(false)),
        )
        .c()?;
        assert_eq!(observation.runtime.image_id, "sha256:image");
        assert_eq!(observation.runtime.platform, "linux/aarch64");
        assert_eq!(
            observation.runtime.configuration_fingerprint,
            fingerprint('c')?
        );
        assert_eq!(observation.runtime.executions.len(), 1);
        assert_eq!(
            observation.runtime.executions[0].command,
            ToolchainObservationCommand::CompilerVersion
        );
        assert_eq!(
            observation.runtime.executions[0].execution_fingerprint,
            fingerprint('e')?
        );
        assert_eq!(observation.declared_toolchain, None);
        assert_eq!(
            observation.source_fingerprint.to_string(),
            source_digest(&source)?
        );
        // Fewer than three outputs is an internal fault; swapped outputs are
        // rejected by the parser; neither consults the configuration.
        let called = Cell::new(false);
        assert_eq!(
            toolchain_observation(
                &source,
                vec![RUSTC.to_owned()],
                executions.clone(),
                "i",
                configuration(&called)
            )
            .err(),
            Some(InspectionError::Internal)
        );
        assert!(
            toolchain_observation(
                &source,
                vec![CARGO.to_owned(), RUSTC.to_owned(), COMPONENTS.to_owned()],
                executions.clone(),
                "i",
                configuration(&called)
            )
            .is_err()
        );
        assert!(!called.get());
        Ok(())
    }

    #[test]
    fn mutation_observation_binds_candidate_postcheck_and_mutation() -> TestResult {
        let candidate = source()?;
        let expected_digest = source_digest(&candidate)?;
        let observation = mutation_observation(
            candidate.clone(),
            execution("")?,
            fingerprint('d')?,
            configuration(&Cell::new(false)),
        )
        .c()?;
        assert_eq!(observation.candidate, candidate);
        assert_eq!(
            observation.mutation_execution_fingerprint,
            fingerprint('d')?
        );
        assert_eq!(observation.runtime.execution_fingerprint, fingerprint('e')?);
        assert_eq!(
            observation.runtime.configuration_fingerprint,
            fingerprint('c')?
        );
        assert_eq!(
            observation.candidate_source_fingerprint.to_string(),
            expected_digest
        );
        assert_eq!(
            mutation_observation(
                candidate,
                execution("")?,
                fingerprint('d')?,
                failing_configuration
            )
            .err(),
            Some(InspectionError::Execution(ExecutionError::Unavailable))
        );
        Ok(())
    }

    struct Control(bool);
    impl ExecutionCancellation for Control {
        fn is_cancelled(&self) -> bool {
            self.0
        }
    }
    impl OperationControl for Control {
        fn check(&self) -> Result<(), ProjectError> {
            if self.0 {
                Err(ProjectError::Cancelled)
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn ports_fail_closed_without_host_configuration_and_never_quarantine() -> TestResult {
        use rust_engineering_application::{
            ProjectCheckPort, ProjectClippyPort, ProjectFormatPort,
        };
        let source = source()?;
        let inspector = RustProjectInspector::new(None);
        let denied = Some(InspectionError::Execution(ExecutionError::Denied));
        let control = Control(false);
        assert_eq!(inspector.inspect(&source, &control).err(), denied);
        assert_eq!(
            inspector.explain(&"E0502".parse().c()?, &control).err(),
            denied
        );
        assert_eq!(inspector.inspect_toolchain(&source, &control).err(), denied);
        assert_eq!(
            inspector
                .check(
                    &source,
                    &rust_engineering_domain::CheckSelection::default()
                        .try_into()
                        .c()?,
                    &control
                )
                .err(),
            denied
        );
        assert_eq!(
            inspector
                .clippy(
                    &source,
                    &rust_engineering_domain::ClippySelection::default()
                        .try_into()
                        .c()?,
                    &control
                )
                .err(),
            denied
        );
        assert_eq!(inspector.format(&source, &control).err(), denied);
        assert_eq!(
            inspector
                .mutate(&source, RustMutationCommand::Format, &control)
                .err(),
            denied
        );
        assert!(!inspector.is_quarantined());
        // A cancelled control is observed before any configuration is read.
        assert_eq!(
            inspector.inspect(&source, &Control(true)).err(),
            Some(InspectionError::Project(ProjectError::Cancelled))
        );
        // An earlier quarantine is sticky and reported before the missing config.
        inspector.quarantined.store(true, Ordering::Release);
        assert_eq!(
            inspector.format(&source, &control).err(),
            Some(InspectionError::Execution(ExecutionError::CleanupUncertain))
        );
        assert!(inspector.is_quarantined());
        Ok(())
    }

    #[test]
    fn quality_and_security_ports_fail_closed_without_host_configuration() -> TestResult {
        use rust_engineering_application::security::SecurityError;
        use rust_engineering_application::{
            ProjectTestPort, coverage::ProjectCoveragePort, miri::ProjectMiriPort,
            mutation_test::ProjectMutationTestPort, nextest::ProjectNextestPort,
            unsafe_scan::ProjectUnsafeScanPort,
        };
        let source = source()?;
        let inspector = RustProjectInspector::new(None);
        let control = Control(false);
        let denied = InspectionError::Execution(ExecutionError::Denied);
        assert_eq!(
            inspector
                .test(
                    &source,
                    &rust_engineering_domain::TestSelection::default()
                        .try_into()
                        .c()?,
                    &control
                )
                .err(),
            Some(denied)
        );
        let nextest: rust_engineering_application::nextest::NextestOptions =
            rust_engineering_application::nextest::NextestSelection::default()
                .try_into()
                .c()?;
        assert_eq!(
            ProjectNextestPort::run(&inspector, &source, &nextest, &control).err(),
            Some(denied)
        );
        let mutation: rust_engineering_domain::mutation_test::MutationTestCommandOptions =
            rust_engineering_domain::mutation_test::MutationTestSelection::default()
                .try_into()
                .c()?;
        assert_eq!(
            ProjectMutationTestPort::run(&inspector, &source, &mutation, &control).err(),
            Some(denied)
        );
        let coverage: rust_engineering_domain::coverage::CoverageOptions =
            rust_engineering_domain::coverage::CoverageSelection::default()
                .try_into()
                .c()?;
        assert_eq!(
            ProjectCoveragePort::run(&inspector, &source, &coverage, &control).err(),
            Some(denied)
        );
        let vendor = CargoVendorSnapshot {
            source: SourceBundle::new(Vec::new()).c()?,
            tree_fingerprint: format!("sha256:{}", "f".repeat(64)).parse().c()?,
            packages: Vec::new(),
        };
        let scan = rust_engineering_domain::unsafe_scan::UnsafeScanOptions::new(60).c()?;
        assert_eq!(
            inspector
                .unsafe_scan(&source, &vendor, &scan, &control)
                .err(),
            Some(SecurityError::Inspection(denied))
        );
        let miri = rust_engineering_domain::miri::MiriOptions::new(60).c()?;
        assert_eq!(
            inspector.miri(&source, &vendor, &miri, &control).err(),
            Some(SecurityError::Inspection(denied))
        );
        assert!(matches!(
            inspector.resolve(&source, &vendor, &control),
            Err(ResolutionError::Inspection(InspectionError::Execution(
                ExecutionError::Denied
            )))
        ));
        assert!(!inspector.is_quarantined());
        Ok(())
    }
}
