//! Closed wire schema; the domain observation owns serialization.
use schemars::JsonSchema;
use serde::Serialize;
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExplainObservation {
    #[schemars(regex(pattern = "^E[0-9]{4}$"))]
    pub code: String,
    #[schemars(length(min = 1, max = 65536))]
    pub explanation: Option<String>,
    pub complete: bool,
    pub termination: super::super::check::schemas::ExecutionTermination,
    pub exit_code: Option<i32>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub content_fingerprint: super::super::toolchain::schemas::Fingerprint,
    pub runtime: super::super::inspection::schemas::RuntimeIdentity,
}
#[derive(Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "details",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Evidence {
    Local,
    Snapshot(SnapshotEvidence),
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotEvidence {
    pub provenance: Provenance,
    pub freshness: super::super::inspection::schemas::Freshness,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Artifact,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source_kind: SourceKind,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub source_id: String,
    pub created_at: Option<u64>,
    pub observed_at: Option<u64>,
    pub integrity: super::super::inspection::schemas::IntegrityStatus,
    pub network_used: bool,
}
