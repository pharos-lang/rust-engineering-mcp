//! Closed Rust runtime operations. Adapters alone map these to programs and argv.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustCommand {
    Metadata,
    FormatCheck,
    TestProject(crate::TestOptions),
    ClippyProject(crate::ClippyOptions),
    Check,
    CheckProject(crate::CheckOptions),
    CompilerVersion,
    Explain(crate::DiagnosticCode),
    CargoVersion,
    InstalledComponents,
}
