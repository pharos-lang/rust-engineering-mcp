//! Closed schema mirrors; domain types alone serialize actual observation data.
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Identifier(#[schemars(length(min = 1, max = 128), regex(pattern = "^[ -~]+$"))] String);
#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Fingerprint(#[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))] String);
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolchainChannel {
    Stable,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InstalledComponentKind {
    Cargo,
    Clippy,
    RustStd,
    Rustc,
    Rustfmt,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstalledComponent {
    pub component: InstalledComponentKind,
    pub target: Option<Identifier>,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolchainInventory {
    pub rustc_version: Identifier,
    pub cargo_version: Identifier,
    pub channel: ToolchainChannel,
    pub host_triple: Identifier,
    #[schemars(length(min = 1, max = 32))]
    pub installed_targets: Vec<Identifier>,
    #[schemars(length(min = 1, max = 32))]
    pub installed_components: Vec<InstalledComponent>,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolchainObservationCommand {
    CompilerVersion,
    CargoVersion,
    InstalledComponents,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolchainExecution {
    pub command: ToolchainObservationCommand,
    pub execution_fingerprint: Fingerprint,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolchainRuntime {
    pub platform: Identifier,
    pub image_id: Fingerprint,
    pub configuration_fingerprint: Fingerprint,
    #[schemars(length(min = 3, max = 3))]
    pub executions: Vec<ToolchainExecution>,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolchainObservation {
    pub inventory: ToolchainInventory,
    pub runtime: ToolchainRuntime,
    pub source_fingerprint: Fingerprint,
    #[schemars(regex(pattern = "^1\\.98\\.1$"))]
    pub declared_toolchain: Option<String>,
}
