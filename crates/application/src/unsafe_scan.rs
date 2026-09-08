//! One captured source generation and the existing owner-bound artifact lifecycle.
use crate::security::{SecurityCapture, SecurityError};
use crate::{
    InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts,
    QualityProjectBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::unsafe_scan::UnsafeScanOptions;
use rust_engineering_domain::{
    CargoVendorSnapshot, Clock, ProjectRef, QualityArtifactDescriptor, SourceBundle,
};

pub use rust_engineering_domain::unsafe_scan::UnsafeObservation;

pub trait ProjectUnsafeScanPort: Send + Sync {
    fn unsafe_scan(
        &self,
        source: &SourceBundle,
        vendor: &CargoVendorSnapshot,
        options: &UnsafeScanOptions,
        control: &dyn InspectionControl,
    ) -> Result<UnsafeObservation, SecurityError>;
}
pub trait UnsafePublisher: Send {
    fn publish_unsafe(
        &mut self,
        capture: &SecurityCapture,
        observation: &UnsafeObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<QualityArtifactDescriptor, InspectionError>;
}
pub struct UnsafePorts<'a, E, P> {
    pub executor: &'a E,
    pub publisher: &'a mut P,
}
pub struct PublishedUnsafe {
    pub observation: UnsafeObservation,
    pub artifact: QualityArtifactDescriptor,
}

impl<B: ProjectSourceBackend + QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock>
    ProjectRegistry<B, G, C>
{
    pub fn unsafe_scan_durable(
        &mut self,
        reference: &ProjectRef,
        vendor: &CargoVendorSnapshot,
        options: &UnsafeScanOptions,
        ports: UnsafePorts<'_, impl ProjectUnsafeScanPort, impl UnsafePublisher>,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<PublishedUnsafe, SecurityError> {
        let capture = self.capture_security(reference, clock, control)?;
        let observation = ports
            .executor
            .unsafe_scan(&capture.source, vendor, options, control)?;
        control.check()?;
        if !observation.report.validate()
            || observation.vendor_fingerprint != vendor.tree_fingerprint
            || observation.runtime.execution_fingerprint != observation.execution_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        let mut revalidate = || {
            self.quality_owner_facts(reference, control)
                .map_err(InspectionError::from)
        };
        let artifact = ports
            .publisher
            .publish_unsafe(&capture, &observation, &mut revalidate)?;
        control.check()?;
        if self.resolve_inner(reference, control, true)?.fingerprint
            != capture.project_identity_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        Ok(PublishedUnsafe {
            observation,
            artifact,
        })
    }
}
