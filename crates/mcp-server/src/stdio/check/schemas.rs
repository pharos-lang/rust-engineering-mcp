//! Closed schema mirrors; domain types remain the source of wire serialization.
use schemars::JsonSchema;
use serde::Serialize;
use std::num::NonZeroU32;

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub line: NonZeroU32,
    pub column: NonZeroU32,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    #[schemars(length(min = 1, max = 100))]
    pub file: String,
    pub start: Position,
    pub end: Position,
    pub bytes: Option<ByteRange>,
    pub is_primary: bool,
    #[schemars(length(min = 1, max = 4096))]
    pub label: Option<String>,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSource {
    Rustc,
    Cargo,
    Clippy,
    Rustfmt,
    Rustsec,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    MachineApplicable,
    MaybeIncorrect,
    HasPlaceholders,
    Unspecified,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub span: SourceSpan,
    // Empty replacement denotes deletion and must remain valid.
    #[schemars(length(max = 4096))]
    pub replacement: String,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Suggestion {
    #[schemars(length(min = 1, max = 4096))]
    pub message: String,
    pub applicability: Applicability,
    #[schemars(length(min = 1, max = 16))]
    pub edits: Vec<Replacement>,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub source: DiagnosticSource,
    pub severity: Severity,
    #[schemars(length(min = 1, max = 128))]
    pub code: Option<String>,
    #[schemars(length(min = 1, max = 4096))]
    pub message: String,
    #[schemars(length(max = 32))]
    pub spans: Vec<SourceSpan>,
    #[schemars(length(max = 4096))]
    pub rendered: Option<String>,
    #[schemars(length(max = 16))]
    pub suggestions: Vec<Suggestion>,
    pub truncated: bool,
}
#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Feature(
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*(/[A-Za-z0-9_][A-Za-z0-9_-]*)?$")
    )]
    pub String,
);
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckOptions {
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
    )]
    pub package: Option<String>,
    pub workspace: bool,
    #[schemars(length(max = 32))]
    pub features: Vec<Feature>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub all_targets: bool,
    #[schemars(regex(pattern = "^aarch64-unknown-linux-gnu$"))]
    pub target: Option<String>,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTermination {
    Exited,
    TimedOut,
    Cancelled,
    OutputLimit,
}
