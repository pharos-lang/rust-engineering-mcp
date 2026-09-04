//! Closed schema mirror for domain-owned test selections.
use schemars::JsonSchema;
use serde::Serialize;
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TestOptions {
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
    )]
    pub package: Option<String>,
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_:]*$")
    )]
    pub test_filter: Option<String>,
    #[schemars(length(max = 32))]
    pub features: Vec<super::super::check::schemas::Feature>,
    pub all_features: bool,
    #[schemars(regex(pattern = "^aarch64-unknown-linux-gnu$"))]
    pub target: Option<String>,
    #[schemars(range(min = 1, max = 60))]
    pub timeout: u64,
}
