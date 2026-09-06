//! Closed Rust runtime operations. Adapters alone map these to programs and argv.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustCommand {
    Metadata,
    FormatCheck,
    TestProject(crate::TestOptions),
    TestNextest(crate::nextest::NextestCommandOptions),
    CoverageRun(crate::coverage::CoverageOptions),
    CoverageReport(crate::coverage::CoverageReportFormat),
    ClippyProject(crate::ClippyOptions),
    Check,
    CheckProject(crate::CheckOptions),
    CompilerVersion,
    Explain(crate::DiagnosticCode),
    CargoVersion,
    LlvmCovVersion,
    InstalledComponents,
    /// Two live captures (baseline, candidate) run through the dual read-only
    /// mount extension (ADR-062 §8): `/source` is the candidate, `/baseline`
    /// is the baseline. Argument-free identity-only probe is the separate
    /// `SemverChecksVersion` variant below.
    SemverCheck(crate::semver_check::SemverCommandOptions),
    SemverChecksVersion,
    /// One `cargo mutants` run over a private writable copy of the captured
    /// source (M3-05). `/source` stays read-only for the mutators; the mutated
    /// copy and the `mutants.out` report live only in guest-private storage.
    MutationTest(crate::mutation_test::MutationTestCommandOptions),
    /// Argument-free cargo-mutants identity probe, analogous to `CargoVersion`.
    MutantsVersion,
}
