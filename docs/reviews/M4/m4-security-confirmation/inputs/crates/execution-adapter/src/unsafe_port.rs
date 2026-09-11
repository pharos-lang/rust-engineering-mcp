//! Syntactic scanner execution through the existing isolated metadata lifecycle.
use crate::RustGateway;
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::unsafe_scan::UnsafeObservation;
use rust_engineering_application::{ExecutionError, InspectionControl};
use rust_engineering_domain::unsafe_scan::UnsafeScanOptions;
use rust_engineering_domain::{
    CargoVendorSnapshot, ExecutionLimits, RuntimeIdentity, SourceBundle,
};

/// No image admission until the helper's exact bytes and containment are qualified.
pub const SCANNER_IMAGE: Option<&str> = Some(crate::APPROVED_M4_IMAGE);

pub(super) fn run(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    options: &UnsafeScanOptions,
    control: &dyn InspectionControl,
) -> Result<UnsafeObservation, SecurityError> {
    if Some(gateway.image_id()) != SCANNER_IMAGE {
        return Err(ExecutionError::Unavailable.into());
    }
    let limits = ExecutionLimits::new_job(options.timeout_seconds() * 1000, 512 * 1024)
        .ok_or(SecurityError::InvalidMetadata)?;
    let execution =
        crate::security_gateway::execute_scan(gateway, source, vendor, limits, control)?;
    let plan = execution.scan_plan.ok_or(SecurityError::InvalidMetadata)?;
    let report = plan.parse(
        &execution.capture.stdout,
        &execution.capture.stderr,
        execution
            .capture
            .code
            .ok_or(SecurityError::InvalidMetadata)?,
    )?;
    if !report.validate() {
        return Err(SecurityError::InvalidMetadata);
    }
    Ok(UnsafeObservation {
        report,
        source_fingerprint: execution.source_fingerprint,
        vendor_fingerprint: execution.vendor_fingerprint,
        vendor_archive_fingerprint: execution.vendor_archive_fingerprint,
        metadata_fingerprint: execution.metadata.original_fingerprint,
        manifest_fingerprint: execution.manifest_fingerprint,
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
    })
}
