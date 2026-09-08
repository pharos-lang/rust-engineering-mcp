//! Schema-only mirrors; domain serializes the actual facts.
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnsafeOrigin {
    Workspace,
    Dependency,
    WorkspaceUnowned,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UnsafeKind {
    UnsafeAttribute,
    UnsafeBlock,
    UnsafeExternBlock,
    UnsafeFn,
    UnsafeImpl,
    UnsafeMod,
    UnsafeStatic,
    UnsafeTrait,
    ExternBlock,
    ExternCrate,
    ExternFn,
}
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnsafeFinding {
    pub path: String,
    pub origin: UnsafeOrigin,
    pub package: Option<super::super::deny::schemas::Package>,
    pub file_fingerprint: String,
    pub kind: UnsafeKind,
    pub byte_start: u32,
    pub byte_end: u32,
    pub line: u32,
    pub column: u32,
    pub conditional: bool,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnsafeCoverage {
    pub files_total: u32,
    #[schemars(range(max = 4096))]
    pub files_selected: u32,
    pub files_omitted: u32,
    pub files_parsed: u32,
    pub files_parse_error: u32,
    pub files_crashed: u32,
    pub files_timed_out: u32,
    pub files_unavailable: u32,
    pub files_invalid_utf8: u32,
    pub files_too_large: u32,
    pub files_budget_exhausted: u32,
    pub workspace_files: u32,
    pub dependency_files: u32,
    pub workspace_unowned_files: u32,
    pub source_bytes: u64,
    pub macro_boundaries_omitted: u64,
    pub opaque_syntax_omitted: u64,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UnsafeScanReport {
    pub coverage: UnsafeCoverage,
    #[schemars(length(max = 128))]
    pub findings: Vec<UnsafeFinding>,
    pub findings_total: u64,
    pub findings_omitted: u64,
    /// Complete only for the declared syntactic input selection; excludes macros/cfg/generated.
    pub syntax_complete: bool,
    pub cfg_evaluated: bool,
    pub macros_expanded: bool,
    pub generated_sources_scanned: bool,
}

#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub report: UnsafeScanReport,
    pub source_fingerprint: String,
    pub vendor_fingerprint: String,
    pub vendor_archive_fingerprint: String,
    pub metadata_fingerprint: String,
    pub manifest_fingerprint: String,
    pub runtime: super::super::inspection::schemas::RuntimeIdentity,
    pub execution_fingerprint: String,
}
