use serde::Serialize;
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupplySource {
    Workspace,
    CratesIo,
    Registry,
    Git,
    Unverified,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SupplyAvailability {
    Available,
    Partial,
    Unavailable,
    Invalid,
    NotConfigured,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum YankedFact {
    Yanked,
    NotYanked,
    CrateAbsent,
    VersionAbsent,
    CatalogUnavailable,
    NotApplicable,
    NotConsulted,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplyPackage {
    pub name: String,
    pub version: String,
    pub source: SupplySource,
    /// A source locator is never exposed; its literal bytes are bound by this hash.
    pub source_fingerprint: Option<String>,
    pub declared_checksum: Option<String>,
    pub checksum_verified: bool,
    pub duplicate_name: bool,
    #[schemars(length(max = 256))]
    pub declared_features: Option<Vec<String>>,
    #[schemars(length(max = 256))]
    pub active_features: Option<Vec<String>>,
    pub yanked: YankedFact,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplyGraph {
    pub source_fingerprint: String,
    pub lock_fingerprint: String,
    #[schemars(length(max = 128))]
    pub packages: Vec<SupplyPackage>,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplyCatalog {
    pub availability: SupplyAvailability,
    pub snapshot_fingerprint: Option<String>,
    pub bundle_fingerprint: Option<String>,
    pub sequence: Option<u64>,
    pub evidence: Option<CatalogEvidence>,
    #[schemars(range(max = 128))]
    pub lookups: u32,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplyDeny {
    pub complete: bool,
    pub policy_state: super::super::deny::schemas::PolicyState,
    #[schemars(length(max = 128))]
    pub findings: Vec<super::super::deny::schemas::Finding>,
    pub findings_omitted: u64,
    pub policy_fingerprint: String,
    pub execution_fingerprint: String,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplyReport {
    pub source_fingerprint: String,
    pub lock_fingerprint: String,
    #[schemars(length(max = 128))]
    pub packages: Vec<SupplyPackage>,
    #[schemars(range(max = 4096))]
    pub packages_total: u32,
    pub packages_omitted: u32,
    pub audit_availability: SupplyAvailability,
    pub audit: Option<SupplyAudit>,
    pub deny_availability: SupplyAvailability,
    pub deny: Option<SupplyDeny>,
    pub catalog: SupplyCatalog,
    pub complete: bool,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub report: SupplyReport,
    pub runtime: super::super::inspection::schemas::RuntimeIdentity,
    pub execution_fingerprint: String,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplyAdvisory {
    pub advisory_id: String,
    pub package_name: String,
    pub package_version: String,
    pub package_source: super::super::auditing::schemas::AuditSource,
    pub source_fingerprint: Option<String>,
    pub informational: bool,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SupplyAudit {
    pub state: super::super::auditing::schemas::AuditState,
    pub issue: Option<super::super::auditing::schemas::AuditIssue>,
    pub validation_complete: bool,
    pub lock_fingerprint: Option<String>,
    pub snapshot_fingerprint: Option<String>,
    pub snapshot: Option<super::super::auditing::schemas::RustSecEvidence>,
    pub snapshot_sequence: Option<u64>,
    #[schemars(range(max = 4096))]
    pub packages_total: u32,
    pub crates_io_scanned: u32,
    pub workspace_packages_excluded: u32,
    pub unsupported_packages: u32,
    #[schemars(length(max = 128))]
    pub findings: Vec<SupplyAdvisory>,
    pub findings_omitted: u64,
}

#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogEvidence {
    provenance: CatalogProvenance,
    freshness: super::super::inspection::schemas::Freshness,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CatalogProvenance {
    source_kind: CatalogSourceKind,
    source_id: String,
    created_at: Option<u64>,
    observed_at: Option<u64>,
    integrity: super::super::inspection::schemas::IntegrityStatus,
    network_used: bool,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSourceKind {
    RegistrySnapshot,
}
