//! Translate the qualified interpreter boundary into bounded typed observations.
use crate::RustGateway;
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::{ExecutionError, InspectionControl};
use rust_engineering_domain::miri::{MiriObservation, MiriOptions};
use rust_engineering_domain::{
    CargoVendorSnapshot, ExecutionLimits, RuntimeIdentity, SourceBundle,
};
pub const MIRI_IMAGE: Option<&str> = Some(crate::APPROVED_M4_IMAGE);

pub(super) fn run(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    options: &MiriOptions,
    control: &dyn InspectionControl,
) -> Result<MiriObservation, SecurityError> {
    if Some(gateway.image_id()) != MIRI_IMAGE {
        return Err(ExecutionError::Unavailable.into());
    }
    let limits = ExecutionLimits::new_job(options.timeout_seconds() * 1000, 1024 * 1024)
        .ok_or(SecurityError::InvalidMetadata)?;
    let execution =
        crate::security_gateway::execute_miri(gateway, source, vendor, limits, control)?;
    let report = crate::miri_output::parse(
        execution.junit.as_deref(),
        &execution.capture.stdout,
        &execution.capture.stderr,
        execution
            .capture
            .code
            .ok_or(SecurityError::InvalidMetadata)?,
    )
    .map_err(|error| match error {
        crate::miri_output::MiriParseError::InputLimit => SecurityError::OutputLimit,
        _ => SecurityError::InvalidMetadata,
    })?;
    if !report.validate() {
        return Err(SecurityError::InvalidMetadata);
    }
    Ok(MiriObservation {
        report,
        source_fingerprint: execution.source_fingerprint,
        vendor_fingerprint: execution.vendor_fingerprint,
        metadata_fingerprint: execution.metadata.original_fingerprint,
        config_fingerprint: crate::digest(crate::miri_admission::CONFIG)
            .parse()
            .map_err(|_| SecurityError::InvalidMetadata)?,
        junit_fingerprint: execution
            .junit
            .as_ref()
            .map(|b| crate::digest(b).parse())
            .transpose()
            .map_err(|_| SecurityError::InvalidMetadata)?,
        runtime: RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: gateway.image_id().into(),
            configuration_fingerprint: gateway.configuration_fingerprint()?,
            execution_fingerprint: execution.execution_fingerprint.clone(),
            rust_version: "1.100.0-nightly (5a2be9f5f 2026-09-06)".into(),
            cargo_version: "1.100.0-nightly (3c0b53475 2026-09-04)".into(),
            declared_toolchain: crate::project_metadata::declared_toolchain(source)?,
        },
        execution_fingerprint: execution.execution_fingerprint,
        nightly_commit: crate::miri_admission::NIGHTLY_COMMIT.into(),
        sysroot_fingerprint: crate::miri_admission::SYSROOT_HASH
            .parse()
            .map_err(|_| SecurityError::InvalidMetadata)?,
    })
}
