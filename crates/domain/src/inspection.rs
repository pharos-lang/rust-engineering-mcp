//! Captured project facts; no Cargo, filesystem, protocol or database API.
use crate::{
    ExecutionFingerprint, ProjectIdentityFingerprint, ProjectRef, SnapshotEvidence,
    SourceFingerprint,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredDependencyKind {
    Normal,
    Build,
    Dev,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencySourceKind {
    Path,
    Registry,
    Git,
}
#[derive(Clone, Debug, Serialize)]
pub struct DependencyOrigin {
    pub kind: DependencySourceKind,
    /// Identity of the declared source, never a credential-bearing URL.
    pub identity: SourceFingerprint,
    pub relative_path: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DirectDependency {
    pub name: String,
    pub rename: Option<String>,
    pub version_requirement: String,
    pub kind: DeclaredDependencyKind,
    pub optional: bool,
    pub uses_default_features: bool,
    pub features: Vec<String>,
    pub target_condition: Option<String>,
    pub origin: DependencyOrigin,
}
#[derive(Clone, Debug, Serialize)]
pub struct DeclaredFeature {
    pub name: String,
    pub activations: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProjectTarget {
    pub name: String,
    pub kinds: Vec<TargetKind>,
    pub crate_types: Vec<TargetKind>,
    pub source_path: String,
    pub edition: RustEdition,
    pub required_features: Vec<String>,
    pub test: bool,
    pub doctest: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProjectPackage {
    pub package_index: u32,
    pub name: String,
    pub version: String,
    pub manifest_path: String,
    pub edition: RustEdition,
    pub rust_version: Option<String>,
    pub targets: Vec<ProjectTarget>,
    pub features: Vec<DeclaredFeature>,
    pub direct_dependencies: Vec<DirectDependency>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProfileValue {
    Boolean(bool),
    Integer(u32),
    Text(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
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
#[derive(Clone, Debug, Serialize)]
pub struct ProfileSetting {
    pub name: ProfileSettingName,
    pub value: ProfileValue,
}
#[derive(Clone, Debug, Serialize)]
pub struct PackageProfile {
    pub package: String,
    pub settings: Vec<ProfileSetting>,
}
#[derive(Clone, Debug, Serialize)]
pub struct DeclaredProfile {
    pub name: String,
    pub inherits: Option<String>,
    pub settings: Vec<ProfileSetting>,
    pub package_overrides: Vec<PackageProfile>,
    pub build_override: Vec<ProfileSetting>,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectConfigPolicy {
    Rejected,
}
#[derive(Clone, Debug, Serialize)]
pub struct CargoConfiguration {
    pub project_config_policy: ProjectConfigPolicy,
    pub frozen: bool,
    pub offline: bool,
    pub incremental: bool,
    pub target_directory_ephemeral: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeIdentity {
    pub platform: String,
    pub image_id: String,
    pub configuration_fingerprint: ExecutionFingerprint,
    pub execution_fingerprint: ExecutionFingerprint,
    pub rust_version: String,
    pub cargo_version: String,
    pub declared_toolchain: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProjectStructure {
    pub workspace_members: Vec<u32>,
    pub workspace_default_members: Vec<u32>,
    pub packages: Vec<ProjectPackage>,
    pub profiles: Vec<DeclaredProfile>,
    pub cargo_configuration: CargoConfiguration,
    pub runtime: RuntimeIdentity,
    pub source_fingerprint: SourceFingerprint,
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectionSemantics {
    LatestKnown,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProjectInspection {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub semantics: InspectionSemantics,
    pub structure: ProjectStructure,
    pub evidence: SnapshotEvidence,
}
