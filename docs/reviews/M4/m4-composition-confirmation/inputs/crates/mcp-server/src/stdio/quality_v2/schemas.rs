use super::super::check::schemas::ExecutionTermination;
use super::super::quality::schemas::{QualityIssue, ToolStatus};
use super::super::supply_chain::schemas::{SupplyAudit, SupplyDeny};
use serde::Serialize;
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QualityV2Profile {
    Strict,
    Release,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum QualityV2StageKind {
    Format,
    Check,
    Clippy,
    Test,
    Audit,
    Deny,
    Coverage,
    Semver,
    Mutation,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QualityV2Details {
    Validation {
        termination: ExecutionTermination,
        exit_code: Option<i32>,
        validation_complete: bool,
        diagnostics: u32,
        diagnostics_omitted: u64,
        affected_files: Option<u64>,
        build_succeeded: Option<bool>,
    },
    Audit {
        observation: SupplyAudit,
    },
    Deny {
        observation: SupplyDeny,
    },
    Coverage {
        aggregate: CoverageMetrics,
        parse_complete: bool,
        exit_code: Option<i32>,
        doctests_run: bool,
    },
    Semver {
        counts: SemverFindingCounts,
        findings_completeness: SemverFindingCompleteness,
        exit_code: Option<i32>,
    },
    Mutation {
        baseline: MutationBaseline,
        counts: MutationCounts,
        validation_complete: bool,
        cap_exceeded: bool,
    },
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityV2Stage {
    pub stage: QualityV2StageKind,
    pub status: ToolStatus,
    pub issue: Option<QualityIssue>,
    pub duration_ms: u64,
    pub execution_fingerprint: Option<String>,
    pub details: Option<QualityV2Details>,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityV2Baseline {
    pub project_ref: String,
    pub identity_fingerprint: String,
    pub source_fingerprint: String,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QualityV2Report {
    pub profile: QualityV2Profile,
    pub mutation_requested: bool,
    pub source_fingerprint: String,
    pub baseline: Option<QualityV2Baseline>,
    pub stages: Vec<QualityV2Stage>,
    pub complete: bool,
    pub status: ToolStatus,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub report: QualityV2Report,
    pub runtime: super::super::inspection::schemas::RuntimeIdentity,
    pub execution_fingerprint: String,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoverageMetrics {
    lines: Option<CoverageMetric>,
    regions: Option<CoverageMetric>,
    functions: Option<CoverageMetric>,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoverageMetric {
    count: u64,
    covered: u64,
    percent_millionths: u32,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemverFindingCounts {
    deny: u32,
    warn: u32,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SemverFindingCompleteness {
    Partial,
    Incomplete,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MutationBaseline {
    Passed,
    Failed,
    Missing,
}
#[derive(Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MutationCounts {
    generated: u32,
    tested: u32,
    caught: u32,
    missed: u32,
    timeout: u32,
    unviable: u32,
    other: u32,
}
#[derive(schemars::JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct MutationSelection {
    #[schemars(default)]
    package: Option<String>,
    #[schemars(default, length(max = 64))]
    features: Vec<String>,
    #[schemars(default)]
    all_features: bool,
    #[schemars(default)]
    no_default_features: bool,
    #[schemars(default)]
    target: Option<String>,
    #[schemars(default = "default_mutants", range(min = 1, max = 100))]
    max_mutants: u32,
    #[schemars(default = "default_mutant_timeout", range(min = 1, max = 60))]
    mutant_timeout_seconds: u64,
}
fn default_mutants() -> u32 {
    100
}
fn default_mutant_timeout() -> u64 {
    60
}
