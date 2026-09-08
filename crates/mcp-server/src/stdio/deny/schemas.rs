//! Mirrors only for JsonSchema; domain owns serialization.
use schemars::JsonSchema;
use serde::Serialize;
#[derive(JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    Rustsec,
    Licenses,
    Bans,
    Sources,
}
#[derive(JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Workspace,
    CratesIo,
    Unverified,
}
#[derive(JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}
#[derive(JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
    Invalid,
    Unavailable,
}
#[derive(JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyState {
    Satisfied,
    SatisfiedWithSuppressions,
    Violated,
    Undetermined,
}
#[derive(JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Package {
    #[schemars(length(min = 1, max = 64))]
    pub name: String,
    #[schemars(length(min = 1, max = 128))]
    pub version: String,
    pub source: Source,
    pub source_fingerprint: Option<String>,
}
#[derive(JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Suppression {
    pub id: String,
    pub engine: Engine,
    pub rule: String,
    pub package: String,
    pub package_source: Source,
    pub version_requirement: String,
    pub reason: String,
    pub owner: String,
    pub expires_at: u64,
    pub rules_digest: String,
}
#[derive(JsonSchema, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Disposition {
    Active,
    Suppressed { suppression: Suppression },
}
#[derive(JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub engine: Engine,
    #[schemars(length(min = 1, max = 96))]
    pub rule: String,
    pub package: Option<Package>,
    pub severity: Severity,
    #[schemars(length(max = 512))]
    pub message: String,
    pub disposition: Disposition,
}
#[derive(JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub errors: u32,
    pub warnings: u32,
    pub notes: u32,
    pub helps: u32,
}

#[derive(JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactCompleteness {
    Complete,
    Truncated,
    Partial,
    Invalid,
    Unavailable,
}
