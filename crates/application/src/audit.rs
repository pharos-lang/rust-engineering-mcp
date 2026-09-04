//! Audit and metadata consume one captured generation under one live project lease.
use crate::{
    InspectionControl, InspectionError, ProjectInspectionPort, ProjectRegistry,
    ProjectSourceBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    AuditDataError, AuditObservation, Clock, FreshnessPolicy, InspectionSemantics, IntegrityStatus,
    ProjectAudit, ProjectRef, ProjectStructure, Provenance, SnapshotEvidence, SourceBundle,
    SourceKind,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectAuditError {
    Inspection(InspectionError),
    Data(AuditDataError),
}
impl From<InspectionError> for ProjectAuditError {
    fn from(value: InspectionError) -> Self {
        Self::Inspection(value)
    }
}
impl From<AuditDataError> for ProjectAuditError {
    fn from(value: AuditDataError) -> Self {
        Self::Data(value)
    }
}

/// Correlate owned lock bytes and captured workspace facts with local advisory data.
/// Implementations must not recapture project files or refresh advisory snapshots.
pub trait DependencyAuditPort {
    fn audit(
        &self,
        source: &SourceBundle,
        structure: &ProjectStructure,
        clock: &dyn Clock,
        control: &dyn InspectionControl,
    ) -> Result<AuditObservation, AuditDataError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn audit(
        &mut self,
        reference: &ProjectRef,
        inspector: &impl ProjectInspectionPort,
        auditor: &impl DependencyAuditPort,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<ProjectAudit, ProjectAuditError> {
        let identity = self
            .resolve_inner(reference, control, false)
            .map_err(InspectionError::from)?;
        // Conservative age begins before the single capture used by both ports.
        let created_at = clock.now();
        let source = self
            .source_inner(reference, control, false)
            .map_err(InspectionError::from)?;
        let structure = inspector.inspect(&source, control)?;
        control.check().map_err(InspectionError::from)?;
        let observation = auditor.audit(&source, &structure, clock, control)?;
        control.check().map_err(InspectionError::from)?;
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
        // Failed operations never renew a lease. Changed identity, TTL expiry and
        // cancellation must deny publication even after successful correlation.
        self.resolve_inner(reference, control, true)
            .map_err(InspectionError::from)?;
        Ok(ProjectAudit {
            project_ref: reference.clone(),
            project_identity_fingerprint: identity.fingerprint,
            semantics: InspectionSemantics::LatestKnown,
            source_fingerprint: structure.source_fingerprint,
            runtime: structure.runtime,
            observation,
            evidence,
        })
    }
}
