//! A complete source inspection owns its live lease through final publication.
use crate::{
    ExecutionCancellation, ExecutionError, OperationControl, ProjectError, ProjectRegistry,
    ProjectSourceBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    Clock, FreshnessPolicy, InspectionSemantics, IntegrityStatus, ProjectInspection, ProjectRef,
    ProjectStructure, Provenance, SnapshotEvidence, SourceBundle, SourceKind,
};

pub trait InspectionControl: OperationControl + ExecutionCancellation {}
impl<T: OperationControl + ExecutionCancellation> InspectionControl for T {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InspectionError {
    Project(ProjectError),
    Execution(ExecutionError),
    InvalidMetadata,
    OutputLimit,
    Internal,
}
impl From<ProjectError> for InspectionError {
    fn from(value: ProjectError) -> Self {
        Self::Project(value)
    }
}

/// The adapter owns execution/JSON parsing and returns validated structural facts.
pub trait ProjectInspectionPort {
    fn inspect(
        &self,
        source: &SourceBundle,
        control: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn inspect(
        &mut self,
        reference: &ProjectRef,
        inspector: &impl ProjectInspectionPort,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<ProjectInspection, InspectionError> {
        let identity = self.resolve_inner(reference, control, false)?;
        // Capture begins after this timestamp; age is deliberately conservative.
        let created_at = clock.now();
        let source = self.source_inner(reference, control, false)?;
        let structure = inspector.inspect(&source, control)?;
        control.check()?;
        let provenance = Provenance::new(
            SourceKind::ProjectSnapshot,
            structure
                .source_fingerprint
                .to_string()
                .parse()
                .map_err(|_| InspectionError::Internal)?,
            Some(created_at),
            Some(clock.now()),
            IntegrityStatus::Verified,
            false,
        )
        .map_err(|_| InspectionError::Internal)?;
        let policy = FreshnessPolicy::new(
            "captured-project-v1"
                .parse()
                .map_err(|_| InspectionError::Internal)?,
            60,
            300,
        )
        .map_err(|_| InspectionError::Internal)?;
        let evidence = SnapshotEvidence::assess(provenance, policy, clock);
        // A long-running job may outlive its TTL or observe invalidated identity.
        // No snapshot is published or lease renewed after either condition.
        // resolve_inner rejects changed fingerprints and drops the stale lease;
        // it never returns a replacement identity for an existing reference.
        self.resolve_inner(reference, control, true)?;
        Ok(ProjectInspection {
            project_ref: reference.clone(),
            project_identity_fingerprint: identity.fingerprint,
            semantics: InspectionSemantics::LatestKnown,
            structure,
            evidence,
        })
    }
}
