//! Wire types for `rust.binary.bloat`.
//!
//! Every enum this tool publishes is declared here and nowhere else, so a
//! variant added elsewhere cannot widen this frozen schema (ADR-076 §2).
//!
//! ADR-076 §6 draws one line through this whole module and nothing here may
//! blur it: [`MeasuredBinary`] is a fact this product measured itself inside
//! the guest, and [`BloatAttribution`] is `cargo-bloat`'s estimate. The two
//! never merge into one number, and no field name, enum spelling or
//! description here may let a reader mistake the estimate for the
//! measurement, or mistake the measured file for the artifact a project
//! asking for stripping would ship.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactCompleteness {
    Complete,
    Truncated,
    Partial,
    Invalid,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTermination {
    Exited,
    TimedOut,
    Cancelled,
    OutputLimit,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIdentity {
    #[schemars(length(max = 128))]
    pub platform: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub image_id: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub configuration_fingerprint: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub execution_fingerprint: String,
    #[schemars(length(max = 128))]
    pub rust_version: String,
    #[schemars(length(max = 128))]
    pub cargo_version: String,
    #[schemars(length(max = 128))]
    pub declared_toolchain: Option<String>,
}

/// The two closed build profiles this tool accepts, echoed back from the
/// request so a response is self-describing. Neither is requested from the
/// analyzer with `--profile`: `release_lto` is `--release` plus the
/// product-owned `CARGO_PROFILE_RELEASE_LTO=fat` environment variable, never a
/// caller-supplied profile name (`docs/validation/M5-04-bloat-calibration.json`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BloatProfile {
    #[default]
    Release,
    ReleaseLto,
}

/// The binary format the analyzer recognised. WASM is rejected because the
/// pinned backend does not support it; Mach-O and PE are not qualified by the
/// ELF positive and are reported, never silently treated as equivalent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BinaryFormat {
    Elf64Aarch64,
    OtherElf,
    MachO,
    Pe,
    Wasm,
    Unknown,
}

/// The size analyzer's own terminal state. `compilation_failed` means the
/// analyzed project did not build; `analysis_failed` means the analyzer
/// itself refused the request (an unresolvable target, an unsupported
/// profile derivation). Neither is an infrastructure fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BloatExit {
    Passed,
    AnalysisFailed,
    CompilationFailed,
    Uncalibrated,
    Incomplete,
}

/// How complete the published evidence is. Anything but `complete` is visible
/// to the caller and is never smoothed into a clean result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BloatCompleteness {
    Complete,
    /// The analyzer's own reported file size disagreed with the size this
    /// product measured. The attribution rows below describe the analyzer's
    /// run, but they are not published as a description of the measured file.
    SizeMismatch,
    Truncated,
    UnsupportedFormat,
    Unavailable,
}

/// Facts this product measured itself inside the guest, independent of
/// anything the analyzer reported. This is an exact measurement of one file.
///
/// It is not, on its own, the artifact a project asking for stripping would
/// ship: see `analysis_build_symbols_forced`.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeasuredBinary {
    /// The exact size, in bytes, of the file this product measured.
    pub size_bytes: u64,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub sha256: String,
    pub format: BinaryFormat,
    /// Always `true`. The size analyzer forces symbol stripping off on every
    /// build it performs, because it needs the symbol table to attribute
    /// size to functions and crates. The measured file is therefore an
    /// **analysis build**: its size above is exact for that file, but it is
    /// not byte-identical to what a project that asks for stripping would
    /// ship. A stripped report cannot be produced by this analyzer.
    pub analysis_build_symbols_forced: bool,
}

/// One estimated function-level row from the analyzer's ranking. `crate_name`
/// and `name` are display text taken from a compiled symbol; they are not
/// sanitized to a closed alphabet and are never a filesystem path.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BloatFunction {
    #[schemars(length(min = 1, max = 512))]
    pub crate_name: String,
    #[schemars(length(min = 1, max = 512))]
    pub name: String,
    /// Estimated bytes attributed to this function by the analyzer.
    pub size_bytes: u64,
}

/// One estimated crate-level row from the analyzer's ranking. `[Unknown]` is
/// the analyzer's own bucket for bytes it could not attribute to a crate, and
/// is preserved verbatim rather than treated as a parse failure.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BloatCrate {
    #[schemars(length(min = 1, max = 512))]
    pub name: String,
    /// Estimated bytes attributed to this crate by the analyzer.
    pub size_bytes: u64,
}

/// Estimated attribution. Every field here comes from `cargo-bloat`, never
/// from this product's own measurement, and none of it is exact.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BloatAttribution {
    /// Always `true`. Serialized so no consumer can read the rows below as an
    /// exact measurement: everything in this object is the analyzer's own
    /// estimate of where size went, not a fact this product verified.
    pub estimated: bool,
    /// The file size the analyzer itself reported, in its own estimate. Not
    /// the exact size: see `measured.size_bytes` in the sibling object for
    /// that, and `completeness` for whether the two agreed.
    pub reported_file_size_bytes: Option<u64>,
    /// The analyzer's estimate of the `.text` section size.
    pub text_section_size_bytes: Option<u64>,
    /// The analyzer's per-function estimate, largest first.
    #[schemars(length(max = 256))]
    pub functions: Vec<BloatFunction>,
    /// The analyzer's per-crate estimate, largest first.
    #[schemars(length(max = 256))]
    pub crates: Vec<BloatCrate>,
    /// Function rows dropped from this response, either by the analyzer's own
    /// row cap or by this tool's response budget. The dropped rows were the
    /// smallest; every row that remains is estimated at least as large.
    pub functions_omitted: u32,
    /// Crate rows dropped from this response, for the same reasons.
    pub crates_omitted: u32,
}

/// What this analysis measured and estimated for one binary target.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]{1,64}$"))]
    pub binary_target: String,
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]{1,64}$"))]
    pub package: Option<String>,
    pub profile: BloatProfile,
    #[schemars(length(min = 1, max = 64))]
    pub analyzer_version: String,
    pub exit: BloatExit,
    pub exit_code: Option<i32>,
    pub termination: ExecutionTermination,
    /// Facts this product measured itself. Absent only when no binary was
    /// produced to measure (an unbuilt project, an unavailable analyzer).
    pub measured: Option<MeasuredBinary>,
    /// The analyzer's estimated attribution. Absent under the same
    /// conditions as `measured`, or when the analyzer produced no usable
    /// ranking.
    pub attribution: Option<BloatAttribution>,
    pub completeness: BloatCompleteness,
    pub runtime: RuntimeIdentity,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub execution_fingerprint: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub vendor_fingerprint: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// `false` whenever this response describes less than the analysis
    /// produced: a build or analysis failure, a size disagreement, an
    /// incomplete artifact, or rows trimmed to fit the response budget.
    pub complete: bool,
}
