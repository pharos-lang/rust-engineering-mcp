//! Schema mirrors for domain facts; real wire serialization uses the domain types.
//! Contract tests validate produced facts against these closed nested definitions.
use schemars::JsonSchema;
use serde::Serialize;
type SourceFingerprint = String;
type ExecutionFingerprint = String;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RustEdition {
    #[serde(rename = "2015")]
    E2015,
    #[serde(rename = "2018")]
    E2018,
    #[serde(rename = "2021")]
    E2021,
    #[serde(rename = "2024")]
    E2024,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
    Lib,
    Bin,
    Example,
    Test,
    Bench,
    CustomBuild,
    ProcMacro,
    Rlib,
    Dylib,
    Cdylib,
    Staticlib,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredDependencyKind {
    Normal,
    Build,
    Dev,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DependencySourceKind {
    Path,
    Registry,
    Git,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DependencyOrigin {
    pub kind: DependencySourceKind,
    /// Identity of the declared source, never a credential-bearing URL.
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub identity: SourceFingerprint,
    #[schemars(length(max = 100))]
    pub relative_path: Option<String>,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DirectDependency {
    #[schemars(length(max = 4096))]
    pub name: String,
    #[schemars(length(max = 128))]
    pub rename: Option<String>,
    #[schemars(length(max = 4096))]
    pub version_requirement: String,
    pub kind: DeclaredDependencyKind,
    pub optional: bool,
    pub uses_default_features: bool,
    #[schemars(length(max = 256))]
    pub features: Vec<String>,
    #[schemars(length(max = 1024))]
    pub target_condition: Option<String>,
    pub origin: DependencyOrigin,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclaredFeature {
    #[schemars(length(max = 4096))]
    pub name: String,
    #[schemars(length(max = 256))]
    pub activations: Vec<String>,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectTarget {
    #[schemars(length(max = 4096))]
    pub name: String,
    #[schemars(length(max = 512))]
    pub kinds: Vec<TargetKind>,
    #[schemars(length(max = 512))]
    pub crate_types: Vec<TargetKind>,
    #[schemars(length(max = 4096))]
    pub source_path: String,
    pub edition: RustEdition,
    #[schemars(length(max = 512))]
    pub required_features: Vec<String>,
    pub test: bool,
    pub doctest: bool,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectPackage {
    pub package_index: u32,
    #[schemars(length(max = 4096))]
    pub name: String,
    #[schemars(length(max = 4096))]
    pub version: String,
    #[schemars(length(max = 4096))]
    pub manifest_path: String,
    pub edition: RustEdition,
    #[schemars(length(max = 32))]
    pub rust_version: Option<String>,
    #[schemars(length(max = 512))]
    pub targets: Vec<ProjectTarget>,
    #[schemars(length(max = 256))]
    pub features: Vec<DeclaredFeature>,
    #[schemars(length(max = 512))]
    pub direct_dependencies: Vec<DirectDependency>,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ProfileValue {
    Boolean(bool),
    Integer(u32),
    Text(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileSettingName {
    OptLevel,
    Debug,
    SplitDebuginfo,
    Strip,
    DebugAssertions,
    OverflowChecks,
    Lto,
    Panic,
    Incremental,
    CodegenUnits,
    Rpath,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileSetting {
    pub name: ProfileSettingName,
    pub value: ProfileValue,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PackageProfile {
    #[schemars(length(max = 4096))]
    pub package: String,
    #[schemars(length(max = 512))]
    pub settings: Vec<ProfileSetting>,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclaredProfile {
    #[schemars(length(max = 4096))]
    pub name: String,
    #[schemars(length(max = 128))]
    pub inherits: Option<String>,
    #[schemars(length(max = 512))]
    pub settings: Vec<ProfileSetting>,
    #[schemars(length(max = 128))]
    pub package_overrides: Vec<PackageProfile>,
    #[schemars(length(max = 512))]
    pub build_override: Vec<ProfileSetting>,
}
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectConfigPolicy {
    Rejected,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CargoConfiguration {
    pub project_config_policy: ProjectConfigPolicy,
    pub frozen: bool,
    pub offline: bool,
    pub incremental: bool,
    pub target_directory_ephemeral: bool,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIdentity {
    #[schemars(length(max = 4096))]
    pub platform: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub image_id: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub configuration_fingerprint: ExecutionFingerprint,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub execution_fingerprint: ExecutionFingerprint,
    #[schemars(length(max = 4096))]
    pub rust_version: String,
    #[schemars(length(max = 4096))]
    pub cargo_version: String,
    #[schemars(length(max = 6))]
    pub declared_toolchain: Option<String>,
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectStructure {
    #[schemars(length(max = 128))]
    pub workspace_members: Vec<u32>,
    #[schemars(length(max = 128))]
    pub workspace_default_members: Vec<u32>,
    #[schemars(length(max = 128))]
    pub packages: Vec<ProjectPackage>,
    #[schemars(length(max = 64))]
    pub profiles: Vec<DeclaredProfile>,
    pub cargo_configuration: CargoConfiguration,
    pub runtime: RuntimeIdentity,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub source_fingerprint: SourceFingerprint,
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
    pub freshness: Freshness,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    ProjectSnapshot,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    Verified,
    Unverified,
    Failed,
    Unknown,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub source_kind: SourceKind,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub source_id: String,
    pub created_at: Option<u64>,
    pub observed_at: Option<u64>,
    pub integrity: IntegrityStatus,
    pub network_used: bool,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Freshness {
    pub state: FreshnessState,
    pub age_seconds: Option<u64>,
    pub assessed_at: u64,
    pub policy: FreshnessPolicy,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState {
    Live,
    Fresh,
    Aging,
    Stale,
    Unknown,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FreshnessPolicy {
    #[schemars(length(min = 1, max = 128))]
    pub id: String,
    pub fresh_for_seconds: u64,
    pub stale_after_seconds: u64,
}
