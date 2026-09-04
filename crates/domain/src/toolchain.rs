//! Observed installed inventory of the explicitly selected runtime.
use crate::{
    ExecutionFingerprint, InspectionSemantics, ProjectIdentityFingerprint, ProjectRef,
    SnapshotEvidence, SourceFingerprint,
};
use serde::Serialize;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolchainChannel {
    Stable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstalledComponentKind {
    Cargo,
    Clippy,
    RustStd,
    Rustc,
    Rustfmt,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstalledComponent {
    pub component: InstalledComponentKind,
    pub target: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ToolchainInventory {
    pub rustc_version: String,
    pub cargo_version: String,
    pub channel: ToolchainChannel,
    pub host_triple: String,
    pub installed_targets: Vec<String>,
    pub installed_components: Vec<InstalledComponent>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolchainObservationCommand {
    CompilerVersion,
    CargoVersion,
    InstalledComponents,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolchainExecution {
    pub command: ToolchainObservationCommand,
    pub execution_fingerprint: ExecutionFingerprint,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolchainRuntime {
    pub platform: String,
    pub image_id: String,
    pub configuration_fingerprint: ExecutionFingerprint,
    pub executions: Vec<ToolchainExecution>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolchainObservation {
    pub inventory: ToolchainInventory,
    pub runtime: ToolchainRuntime,
    pub source_fingerprint: SourceFingerprint,
    pub declared_toolchain: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ToolchainInspection {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub semantics: InspectionSemantics,
    pub observation: ToolchainObservation,
    pub evidence: SnapshotEvidence,
}
