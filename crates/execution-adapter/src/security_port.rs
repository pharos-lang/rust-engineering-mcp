//! Map a joined, confined deny execution into typed application evidence.
use crate::{RustGateway, deny_json, security_gateway};
use rust_engineering_application::security::{
    DenyObservation, SecurityArtifactStreams, SecurityError,
};
use rust_engineering_application::{ExecutionError, InspectionControl};
use rust_engineering_domain::security::{DenyOptions, SecurityCounts, SecurityPolicy};
use rust_engineering_domain::{
    CargoVendorSnapshot, ExecutionLimits, ExecutionTermination, RuntimeIdentity, SourceBundle,
};

pub const M4_IMAGE: &str =
    "sha256:95dddeb5305f10b09b441e3cc4018ebb1a8a296d365c65106327d59f933c64e7";

pub(super) fn run(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    policy: &SecurityPolicy,
    options: &DenyOptions,
    control: &dyn InspectionControl,
) -> Result<DenyObservation, SecurityError> {
    if gateway.image_id() != M4_IMAGE && gateway.image_id() != crate::APPROVED_M4_IMAGE {
        return Err(ExecutionError::Unavailable.into());
    }
    let limits = ExecutionLimits::new_job(options.timeout_seconds() * 1000, 1024 * 1024)
        .ok_or(SecurityError::InvalidPolicy)?;
    let execution = security_gateway::execute(gateway, source, vendor, policy, limits, control)?;
    let capture = execution.capture;
    let parsed = capture.code.and_then(|code| {
        deny_json::parse(
            &capture.stderr,
            &capture.stdout,
            code,
            capture.stdout_truncated || capture.stderr_truncated,
            &execution.metadata.packages,
        )
        .ok()
    });
    let (findings, findings_omitted, licenses, bans, sources, parse_complete) = parsed.map_or_else(
        || {
            (
                vec![],
                0,
                SecurityCounts::default(),
                SecurityCounts::default(),
                SecurityCounts::default(),
                false,
            )
        },
        |p| {
            (
                p.findings,
                p.findings_omitted,
                p.licenses,
                p.bans,
                p.sources,
                p.parse_complete,
            )
        },
    );
    let observation = DenyObservation {
        source_fingerprint: execution.source_fingerprint,
        vendor_fingerprint: execution.vendor_fingerprint,
        vendor_archive_fingerprint: execution.vendor_archive_fingerprint,
        policy_fingerprint: execution
            .policy_fingerprint
            .ok_or(SecurityError::InvalidPolicy)?,
        deny_config_fingerprint: execution.deny_config_fingerprint,
        cargo_config_fingerprint: execution.cargo_config_fingerprint,
        metadata_original_fingerprint: execution.metadata.original_fingerprint,
        metadata_derived_fingerprint: execution.metadata.derived_fingerprint,
        lock_fingerprint: execution.metadata.lock_fingerprint,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: gateway.image_id().into(),
            configuration_fingerprint: gateway.configuration_fingerprint()?,
            execution_fingerprint: execution.execution_fingerprint.clone(),
            rust_version: crate::rust_gateway::APPROVED_RUST_VERSION.into(),
            cargo_version: crate::rust_gateway::APPROVED_CARGO_VERSION.into(),
            declared_toolchain: crate::project_metadata::declared_toolchain(source)?,
        },
        execution_fingerprint: execution.execution_fingerprint,
        packages: execution.metadata.packages,
        declared_licenses: execution.metadata.declared_licenses,
        license_files: execution.metadata.license_files,
        enabled_features: execution.metadata.enabled_features,
        dependency_indices: execution.metadata.dependency_indices,
        workspace_members: execution.metadata.workspace_members,
        findings,
        findings_omitted,
        licenses,
        bans,
        sources,
        parse_complete,
        termination: match capture.stop {
            crate::supervisor::Stop::Exited => ExecutionTermination::Exited,
            crate::supervisor::Stop::TimedOut => ExecutionTermination::TimedOut,
            crate::supervisor::Stop::Cancelled => ExecutionTermination::Cancelled,
            crate::supervisor::Stop::OutputLimit => ExecutionTermination::OutputLimit,
        },
        exit_code: capture.code,
        artifacts: SecurityArtifactStreams {
            stdout: capture.stdout,
            stderr: capture.stderr,
            stdout_truncated: capture.stdout_truncated,
            stderr_truncated: capture.stderr_truncated,
        },
    };
    observation.validate()?;
    Ok(observation)
}
