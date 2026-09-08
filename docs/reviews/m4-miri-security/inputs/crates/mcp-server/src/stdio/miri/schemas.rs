use serde::Serialize;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MiriCategory {
    UndefinedBehavior,
    UnsupportedOperation,
    TestFailure,
    CompileFailure,
    Timeout,
    Unclassified,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MiriFinding {
    pub category: MiriCategory,
    pub test_name: Option<String>,
    pub test_binary: Option<String>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MiriCounts {
    pub tests: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub undefined_behavior: u32,
    pub unsupported_operation: u32,
    pub test_failures: u32,
    pub compile_failures: u32,
    pub timeouts: u32,
    pub unclassified: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MiriReport {
    pub counts: MiriCounts,
    #[schemars(length(max = 128))]
    pub findings: Vec<MiriFinding>,
    pub findings_omitted: u64,
    pub complete: bool,
    pub clean: bool,
    pub junit_present: bool,
    pub exit_code: Option<i32>,
}
#[derive(Clone, Debug, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub report: MiriReport,
    pub source_fingerprint: String,
    pub vendor_fingerprint: String,
    pub metadata_fingerprint: String,
    pub config_fingerprint: String,
    pub junit_fingerprint: Option<String>,
    pub runtime: super::super::inspection::schemas::RuntimeIdentity,
    pub execution_fingerprint: String,
    pub nightly_commit: String,
    pub sysroot_fingerprint: String,
}
