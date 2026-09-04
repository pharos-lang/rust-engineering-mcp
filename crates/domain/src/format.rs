//! Configured workspace formatting evidence; display diffs are never edit commands.
use crate::{
    ArtifactMetadata, CheckObservation, InspectionSemantics, ProjectIdentityFingerprint,
    ProjectRef, SnapshotEvidence,
};
#[derive(Clone, Debug)]
pub struct FormatObservation {
    pub execution: CheckObservation,
    pub affected_files: Vec<String>,
    pub affected_files_omitted: u64,
    pub diff: Option<String>,
    pub diff_omitted: bool,
}
#[derive(Clone, Debug)]
pub struct ProjectFormat {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub semantics: InspectionSemantics,
    pub observation: FormatObservation,
    pub evidence: SnapshotEvidence,
    pub log: Option<ArtifactMetadata>,
    pub retention_remaining_seconds: Option<u64>,
}
