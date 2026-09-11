//! One captured source generation and the existing owner-bound artifact lifecycle.
use crate::security::{SecurityCapture, SecurityError};
use crate::{
    InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend, QualityOwnerFacts,
    QualityProjectBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::miri::MiriOptions;
use rust_engineering_domain::{
    CargoVendorSnapshot, Clock, ProjectRef, QualityArtifactDescriptor, SourceBundle,
};

pub use rust_engineering_domain::miri::MiriObservation;

pub trait ProjectMiriPort: Send + Sync {
    fn miri(
        &self,
        source: &SourceBundle,
        vendor: &CargoVendorSnapshot,
        options: &MiriOptions,
        control: &dyn InspectionControl,
    ) -> Result<MiriObservation, SecurityError>;
}
pub trait MiriPublisher: Send {
    fn publish_miri(
        &mut self,
        capture: &SecurityCapture,
        observation: &MiriObservation,
        revalidate: &mut dyn FnMut() -> Result<QualityOwnerFacts, InspectionError>,
    ) -> Result<QualityArtifactDescriptor, InspectionError>;
}
pub struct MiriPorts<'a, E, P> {
    pub executor: &'a E,
    pub publisher: &'a mut P,
}
pub struct PublishedMiri {
    pub observation: MiriObservation,
    pub artifact: QualityArtifactDescriptor,
}

impl<B: ProjectSourceBackend + QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock>
    ProjectRegistry<B, G, C>
{
    pub fn miri_durable(
        &mut self,
        reference: &ProjectRef,
        vendor: &CargoVendorSnapshot,
        options: &MiriOptions,
        ports: MiriPorts<'_, impl ProjectMiriPort, impl MiriPublisher>,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<PublishedMiri, SecurityError> {
        let capture = self.capture_security(reference, clock, control)?;
        let observation = ports
            .executor
            .miri(&capture.source, vendor, options, control)?;
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
            .publish_miri(&capture, &observation, &mut revalidate)?;
        control.check()?;
        if self.resolve_inner(reference, control, true)?.fingerprint
            != capture.project_identity_fingerprint
        {
            return Err(SecurityError::InvalidMetadata);
        }
        Ok(PublishedMiri {
            observation,
            artifact,
        })
    }
}
