//! Compiler explanations do not require a project capability or artifact resource.
use crate::{InspectionControl, InspectionError};
use rust_engineering_domain::*;
pub trait DiagnosticExplainPort {
    fn explain(
        &self,
        code: &DiagnosticCode,
        control: &dyn InspectionControl,
    ) -> Result<ExplainObservation, InspectionError>;
}
pub fn explain_diagnostic(
    port: &impl DiagnosticExplainPort,
    code: &DiagnosticCode,
    clock: &impl Clock,
    control: &dyn InspectionControl,
) -> Result<DiagnosticExplanation, InspectionError> {
    control.check()?;
    let created = clock.now();
    let observation = port.explain(code, control)?;
    control.check()?;
    if &observation.code != code
        || observation
            .explanation
            .as_ref()
            .is_some_and(|text| text.len() > 64 * 1024)
    {
        return Err(InspectionError::Internal);
    }
    let provenance = Provenance::new(
        SourceKind::Artifact,
        observation
            .content_fingerprint
            .to_string()
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        Some(created),
        Some(clock.now()),
        IntegrityStatus::Verified,
        false,
    )
    .map_err(|_| InspectionError::Internal)?;
    let policy = FreshnessPolicy::new(
        "compiler-explanation-v1"
            .parse()
            .map_err(|_| InspectionError::Internal)?,
        60,
        300,
    )
    .map_err(|_| InspectionError::Internal)?;
    let evidence = SnapshotEvidence::assess(provenance, policy, clock);
    control.check()?;
    Ok(DiagnosticExplanation {
        semantics: InspectionSemantics::LatestKnown,
        observation,
        evidence,
    })
}
