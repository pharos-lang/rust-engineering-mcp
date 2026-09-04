//! A joined formatting check publishes a single retained log only after live authorization.
use crate::{
    ArtifactStore, InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend,
    ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    Clock, FormatObservation, InspectionSemantics, ProjectFormat, ProjectRef, SourceBundle,
};

pub trait ProjectFormatPort {
    fn format(
        &self,
        source: &SourceBundle,
        control: &dyn InspectionControl,
    ) -> Result<FormatObservation, InspectionError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn format(
        &mut self,
        reference: &ProjectRef,
        formatter: &impl ProjectFormatPort,
        artifacts: &mut impl ArtifactStore,
        clocks: (&impl Clock, &impl RegistryClock),
        control: &dyn InspectionControl,
    ) -> Result<ProjectFormat, InspectionError> {
        let captured = self.capture_validation(reference, artifacts, clocks.0, control)?;
        let mut observation = formatter.format(&captured.source, control)?;
        let published = self.publish_validation(
            captured,
            &mut observation.execution,
            artifacts,
            clocks,
            control,
        )?;
        Ok(ProjectFormat {
            project_ref: published.project_ref,
            project_identity_fingerprint: published.project_identity_fingerprint,
            semantics: InspectionSemantics::LatestKnown,
            observation,
            evidence: published.evidence,
            log: published.log,
            retention_remaining_seconds: published.retention_remaining_seconds,
        })
    }
}
