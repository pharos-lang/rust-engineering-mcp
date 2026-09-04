//! Closed schema mirrors for domain-owned Clippy serialization.
use schemars::JsonSchema;
use serde::Serialize;
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LintProfile {
    Default,
    Strict,
    Pedantic,
    Project,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClippyOptions {
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
    )]
    pub package: Option<String>,
    pub workspace: bool,
    #[schemars(length(max = 32))]
    pub features: Vec<super::super::check::schemas::Feature>,
    pub all_targets: bool,
    pub lint_profile: LintProfile,
}
