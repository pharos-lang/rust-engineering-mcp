//! Adapter from the application semver port to the closed dual-volume gateway.

use crate::RustGateway;
use rust_engineering_application::semver_check::{SemverObservation, SemverOptions};
use rust_engineering_application::{InspectionControl, InspectionError, ProjectError};
use rust_engineering_domain::{
    ExecutionLimits, ExecutionTermination, RuntimeIdentity, SourceBundle,
    semver_check::{SemverExit, SemverFindingCompleteness},
};

pub(super) fn run(
    gateway: &RustGateway,
    baseline: &SourceBundle,
    candidate: &SourceBundle,
    options: &SemverOptions,
    control: &dyn InspectionControl,
) -> Result<SemverObservation, InspectionError> {
    let wall_ms = options
        .timeout_seconds()
        .checked_mul(1000)
        .ok_or(InspectionError::Internal)?;
    let limits = ExecutionLimits::new_job(wall_ms, 512 * 1024).ok_or(InspectionError::Internal)?;
    let result = gateway
        .execute_semver(baseline, candidate, options.selection(), limits, control)
        .map_err(InspectionError::Execution)?;
    if result.termination == ExecutionTermination::Cancelled {
        return Err(InspectionError::Project(ProjectError::Cancelled));
    }
    let mut parser_input = String::with_capacity(result.stdout.len() + result.stderr.len() + 1);
    parser_input.push_str(&result.stdout);
    parser_input.push('\n');
    parser_input.push_str(&result.stderr);
    let parsed = super::semver_output::parse(&parser_input);
    let completeness = if result.stdout_truncated
        || result.stderr_truncated
        || parsed.completeness == super::semver_output::SemverParseCompleteness::Incomplete
    {
        SemverFindingCompleteness::Incomplete
    } else {
        SemverFindingCompleteness::Partial
    };
    let exit = if result.termination == ExecutionTermination::Exited {
        result
            .exit_code
            .map_or(SemverExit::Uncalibrated, SemverExit::classify)
    } else {
        SemverExit::Incomplete
    };
    let runtime = RuntimeIdentity {
        platform: result.platform.into(),
        image_id: result.image_id.clone(),
        configuration_fingerprint: gateway
            .configuration_fingerprint()
            .map_err(InspectionError::Execution)?,
        execution_fingerprint: result.execution_fingerprint.clone(),
        rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
        cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
        declared_toolchain: super::project_metadata::declared_toolchain(candidate)?,
    };
    Ok(SemverObservation {
        options: options.clone(),
        exit,
        counts: parsed.counts,
        findings: parsed.findings,
        findings_omitted: parsed.findings_omitted,
        completeness,
        termination: result.termination,
        exit_code: result.exit_code,
        runtime,
        execution_fingerprint: result.execution_fingerprint,
        stdout: result.stdout.into_bytes(),
        stderr: result.stderr.into_bytes(),
        stdout_truncated: result.stdout_truncated,
        stderr_truncated: result.stderr_truncated,
    })
}
