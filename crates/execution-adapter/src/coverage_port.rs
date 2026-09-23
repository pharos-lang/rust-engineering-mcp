//! Adapter from the closed coverage gateway to the application port.
use crate::{RustGateway, coverage_json};
use rust_engineering_application::coverage::{
    CoverageArtifactStreams, CoverageIdentity, CoverageObservation,
};
use rust_engineering_application::{InspectionControl, InspectionError, ProjectError};
use rust_engineering_domain::{
    ExecutionLimits, ExecutionTermination, OperationalErrorCode, RuntimeIdentity, RustCommand,
    SourceBundle,
};

const APPROVED_LLVM_COV_VERSION: &str = "0.9.0";

fn unavailable() -> InspectionError {
    InspectionError::Project(ProjectError::Rejected(
        OperationalErrorCode::ToolNotInstalled,
    ))
}

fn version_available(result: &rust_engineering_domain::ExecutionResult) -> bool {
    result.termination == ExecutionTermination::Exited
        && result.exit_code == Some(0)
        && result
            .stdout
            .trim()
            .starts_with(&format!("cargo-llvm-cov {APPROVED_LLVM_COV_VERSION}"))
}

fn components_available(result: &rust_engineering_domain::ExecutionResult) -> bool {
    result.termination == ExecutionTermination::Exited
        && result.exit_code == Some(0)
        && result
            .stdout
            .lines()
            .any(|line| line.trim() == "llvm-tools-preview")
}

pub(super) fn run(
    gateway: &RustGateway,
    source: &SourceBundle,
    options: &rust_engineering_domain::coverage::CoverageOptions,
    control: &dyn InspectionControl,
) -> Result<CoverageObservation, InspectionError> {
    let probe_limits =
        ExecutionLimits::new_job(30_000, 256 * 1024).ok_or(InspectionError::Internal)?;
    let version = gateway
        .execute(source, RustCommand::LlvmCovVersion, probe_limits, control)
        .map_err(InspectionError::Execution)?;
    if !version_available(&version) {
        return Err(unavailable());
    }
    let components = gateway
        .execute(
            source,
            RustCommand::InstalledComponents,
            probe_limits,
            control,
        )
        .map_err(InspectionError::Execution)?;
    if !components_available(&components) {
        return Err(unavailable());
    }
    let wall_ms = options
        .timeout_seconds()
        .checked_mul(1000)
        .ok_or(InspectionError::Internal)?;
    let execution = gateway
        .execute_coverage(
            source,
            options,
            ExecutionLimits::new_job(wall_ms, 256 * 1024).ok_or(InspectionError::Internal)?,
            control,
        )
        .map_err(InspectionError::Execution)?;
    let result = execution.result;
    if result.termination == ExecutionTermination::Cancelled {
        return Err(InspectionError::Project(ProjectError::Cancelled));
    }
    let parsed = execution
        .json
        .as_deref()
        .and_then(|bytes| coverage_json::parse(bytes).ok());
    let Some(parsed) = parsed else {
        return Err(InspectionError::OutputLimit);
    };
    if parsed.cargo_llvm_cov_version != APPROVED_LLVM_COV_VERSION {
        return Err(InspectionError::InvalidMetadata);
    }
    let runtime = RuntimeIdentity {
        platform: result.platform.into(),
        image_id: result.image_id.clone(),
        configuration_fingerprint: gateway
            .configuration_fingerprint()
            .map_err(InspectionError::Execution)?,
        execution_fingerprint: result.execution_fingerprint.clone(),
        rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
        cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
        declared_toolchain: super::project_metadata::declared_toolchain(source)?,
    };
    Ok(CoverageObservation {
        options: options.clone(),
        summary: parsed.summary,
        identity: CoverageIdentity {
            cargo_llvm_cov_version: parsed.cargo_llvm_cov_version,
            manifest_path: parsed.manifest_path,
            llvm_tools_version: "1.98.1".into(),
        },
        doctests_run: false,
        cfg_coverage_enabled: true,
        target: "aarch64-unknown-linux-gnu",
        termination: result.termination,
        exit_code: result.exit_code,
        parse_complete: true,
        runtime,
        execution_fingerprint: result.execution_fingerprint,
        artifacts: CoverageArtifactStreams {
            json: execution.json.unwrap_or_default(),
            lcov: execution.lcov.unwrap_or_default(),
            html_bundle: execution.html.unwrap_or_default(),
            stdout: result.stdout.into_bytes(),
            stderr: result.stderr.into_bytes(),
            json_truncated: execution.json_truncated,
            lcov_truncated: execution.lcov_truncated,
            html_truncated: execution.html_truncated,
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::{ExecutionFingerprint, ExecutionResult};

    fn result(stdout: &str) -> Result<ExecutionResult, String> {
        Ok(ExecutionResult {
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            oom_killed: Some(false),
            stdout: stdout.into(),
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            duration_ms: 1,
            total_duration_ms: 1,
            execution_fingerprint: format!("sha256:{}", "0".repeat(64))
                .parse::<ExecutionFingerprint>()
                .map_err(|error| format!("{error:?}"))?,
            platform: "linux/aarch64",
            image_id: format!("sha256:{}", "1".repeat(64)),
        })
    }

    #[test]
    fn coverage_version_probe_requires_the_pinned_prefix_and_clean_exit() -> Result<(), String> {
        assert!(version_available(&result("cargo-llvm-cov 0.9.0\n")?));
        assert!(version_available(&result(
            "  cargo-llvm-cov 0.9.0 (llvm-cov)  "
        )?));
        assert!(!version_available(&result("cargo-llvm-cov 0.8.2\n")?));
        let mut failed = result("cargo-llvm-cov 0.9.0\n")?;
        failed.exit_code = Some(1);
        assert!(!version_available(&failed));
        failed.exit_code = Some(0);
        failed.termination = ExecutionTermination::TimedOut;
        assert!(!version_available(&failed));
        Ok(())
    }

    #[test]
    fn component_probe_matches_one_exact_trimmed_line() -> Result<(), String> {
        assert!(components_available(&result(
            "rust-src\n  llvm-tools-preview  \nrustfmt\n"
        )?));
        assert!(!components_available(&result(
            "llvm-tools-preview-extra\n"
        )?));
        let mut failed = result("llvm-tools-preview\n")?;
        failed.termination = ExecutionTermination::OutputLimit;
        assert!(!components_available(&failed));
        Ok(())
    }

    #[test]
    fn missing_coverage_tool_is_a_closed_operational_error() {
        assert_eq!(
            unavailable(),
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ToolNotInstalled
            ))
        );
    }
}
