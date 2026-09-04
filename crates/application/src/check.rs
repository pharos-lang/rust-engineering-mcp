//! A joined check publishes a single retained log only after live authorization.
use crate::{
    ArtifactStore, InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend,
    ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    CheckObservation, CheckOptions, Clock, InspectionSemantics, ProjectCheck, ProjectRef,
    SourceBundle,
};

pub trait ProjectCheckPort {
    fn check(
        &self,
        source: &SourceBundle,
        options: &CheckOptions,
        control: &dyn InspectionControl,
    ) -> Result<CheckObservation, InspectionError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn check(
        &mut self,
        reference: &ProjectRef,
        options: &CheckOptions,
        checker: &impl ProjectCheckPort,
        artifacts: &mut impl ArtifactStore,
        clocks: (&impl Clock, &impl RegistryClock),
        control: &dyn InspectionControl,
    ) -> Result<ProjectCheck, InspectionError> {
        let captured = self.capture_validation(reference, artifacts, clocks.0, control)?;
        let mut observation = checker.check(&captured.source, options, control)?;
        let published =
            self.publish_validation(captured, &mut observation, artifacts, clocks, control)?;
        Ok(ProjectCheck {
            project_ref: published.project_ref,
            project_identity_fingerprint: published.project_identity_fingerprint,
            semantics: InspectionSemantics::LatestKnown,
            options: options.clone(),
            observation,
            evidence: published.evidence,
            log: published.log,
            retention_remaining_seconds: published.retention_remaining_seconds,
        })
    }
}
