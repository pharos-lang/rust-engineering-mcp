//! Closed mirrors for domain-owned audit facts.
use schemars::JsonSchema;
use serde::Serialize;
type SourceFingerprint = String;
type CatalogFingerprint = String;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditState {
    Passed,
    Failed,
    Incomplete,
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditIssue {
    SnapshotUnavailable,
    SnapshotStale,
    SnapshotUnknownAge,
    UnsupportedSources,
    OutputBudget,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditSource {
    CratesIo,
    Workspace,
    Unverified,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditPackage {
    pub name: String,
    pub version: String,
    pub source: AuditSource,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub source_fingerprint: Option<SourceFingerprint>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuditSeverity {
    None,
    Low,
    Medium,
    High,
    Critical,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditPath {
    pub workspace_root: AuditPackage,
    /// Includes workspace root and affected package; represents one captured lock path.
    pub packages: Vec<AuditPackage>,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditFinding {
    pub advisory_id: String,
    pub url: String,
    pub title: String,
    pub package: AuditPackage,
    pub patched_requirements: Vec<String>,
    pub unaffected_requirements: Vec<String>,
    pub severity: Option<AuditSeverity>,
    pub informational: Option<String>,
    pub paths: Vec<AuditPath>,
    pub paths_omitted: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditObservation {
    pub state: AuditState,
    pub issue: Option<AuditIssue>,
    pub validation_complete: bool,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub lock_fingerprint: Option<SourceFingerprint>,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub snapshot_fingerprint: Option<CatalogFingerprint>,
    pub snapshot: Option<RustSecEvidence>,
    pub snapshot_record_count: Option<u32>,
    pub snapshot_sequence: Option<u64>,
    pub packages_total: u32,
    pub crates_io_scanned: u32,
    pub workspace_packages_excluded: u32,
    pub unsupported_packages: Vec<AuditPackage>,
    pub findings: Vec<AuditFinding>,
    pub informational: Vec<AuditFinding>,
    pub findings_omitted: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RustSecEvidence {
    pub provenance: RustSecProvenance,
    pub freshness: super::super::inspection::schemas::Freshness,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RustSecProvenance {
    pub source_kind: RustSecSourceKind,
    pub source_id: String,
    pub created_at: Option<u64>,
    pub observed_at: Option<u64>,
    pub integrity: super::super::inspection::schemas::IntegrityStatus,
    pub network_used: bool,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RustSecSourceKind {
    RustsecSnapshot,
}
