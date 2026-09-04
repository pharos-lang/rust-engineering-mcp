//! A complete source inspection owns its live lease through final publication.
use crate::{
    InspectionControl, InspectionError, ProjectRegistry, ProjectSourceBackend, ReferenceGenerator,
    RegistryClock,
};
use rust_engineering_domain::{
    Clock, FreshnessPolicy, InspectionSemantics, IntegrityStatus, ProjectRef, Provenance,
    SnapshotEvidence, SourceBundle, SourceKind, ToolchainInspection, ToolchainObservation,
};

/// The adapter owns execution/JSON parsing and returns validated structural facts.
pub trait ToolchainInspectionPort {
    fn inspect_toolchain(
        &self,
        source: &SourceBundle,
        control: &dyn InspectionControl,
    ) -> Result<ToolchainObservation, InspectionError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn inspect_toolchain(
        &mut self,
        reference: &ProjectRef,
        inspector: &impl ToolchainInspectionPort,
        clock: &impl Clock,
        control: &dyn InspectionControl,
    ) -> Result<ToolchainInspection, InspectionError> {
        let identity = self.resolve_inner(reference, control, false)?;
        // Capture begins after this timestamp; age is deliberately conservative.
        let created_at = clock.now();
        let source = self.source_inner(reference, control, false)?;
        let observation = inspector.inspect_toolchain(&source, control)?;
        control.check()?;
        let provenance = Provenance::new(
            SourceKind::ProjectSnapshot,
            observation
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
        Ok(ToolchainInspection {
            project_ref: reference.clone(),
            project_identity_fingerprint: identity.fingerprint,
            semantics: InspectionSemantics::LatestKnown,
            observation,
            evidence,
        })
    }
}
