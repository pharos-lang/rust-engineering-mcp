//! A joined test run publishes a single retained log only after live authorization.
use crate::{
    ArtifactStore, InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend,
    ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    Clock, InspectionSemantics, ProjectRef, ProjectTest, SourceBundle, TestObservation, TestOptions,
};

pub trait ProjectTestPort {
    fn test(
        &self,
        source: &SourceBundle,
        options: &TestOptions,
        control: &dyn InspectionControl,
    ) -> Result<TestObservation, InspectionError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn test(
        &mut self,
        reference: &ProjectRef,
        options: &TestOptions,
        runner: &impl ProjectTestPort,
        artifacts: &mut impl ArtifactStore,
        clocks: (&impl Clock, &impl RegistryClock),
        control: &dyn InspectionControl,
    ) -> Result<ProjectTest, InspectionError> {
        let captured = self.capture_validation(reference, artifacts, clocks.0, control)?;
        let mut observation = runner.test(&captured.source, options, control)?;
        let published = self.publish_validation(
            captured,
            &mut observation.execution,
            artifacts,
            clocks,
            control,
        )?;
        Ok(ProjectTest {
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
