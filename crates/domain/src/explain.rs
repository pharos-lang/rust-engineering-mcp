//! Facts captured from the approved compiler, independent of any project authority.
use crate::{
    DiagnosticCode, ExecutionTermination, InspectionSemantics, RuntimeIdentity, SnapshotEvidence,
    SourceFingerprint,
};
#[derive(Clone, Debug, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExplainObservation {
    pub code: DiagnosticCode,
    pub explanation: Option<String>,
    pub complete: bool,
    pub termination: ExecutionTermination,
    pub exit_code: Option<i32>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// SHA-256 of the returned explanation bytes, or empty bytes when unavailable.
    pub content_fingerprint: SourceFingerprint,
    pub runtime: RuntimeIdentity,
}
#[derive(Clone, Debug)]
pub struct DiagnosticExplanation {
    pub semantics: InspectionSemantics,
    pub observation: ExplainObservation,
    pub evidence: SnapshotEvidence,
}
