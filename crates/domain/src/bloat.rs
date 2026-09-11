//! Observation contract for one `rust.binary.bloat` execution (M5-04).
//!
//! The central invariant of this module is the separation the plan demands: the
//! file's size is measured by this product and is exact; every per-function and
//! per-crate number is an estimate produced by `cargo-bloat` and is labelled as
//! such. The two are never merged into one number.
use crate::{ExecutionFingerprint, ExecutionTermination, RuntimeIdentity, SourceFingerprint};
use serde::Serialize;

pub const APPROVED_CARGO_BLOAT_VERSION: &str = "0.12.1";
pub const BLOAT_REPORT_FORMAT: &str = "rust-engineering-mcp.bloat-report.v1";
pub const BLOAT_MAX_ROWS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BloatError {
    InvalidTarget,
    InvalidPackage,
}

/// The two closed build profiles. `release_lto` exists so an LTO binary can be
/// exercised; nothing else is selectable and no free profile name is accepted.
///
/// Neither is expressed with the analyzer's `--profile` flag. What was observed,
/// and all that is claimed as observed, is that `--profile release-lto` exits 1
/// with `error in environment variable CARGO_PROFILE_RELEASE: could not load
/// config key profile.release / invalid type: Option value, expected a boolean
/// or string`, recorded in `docs/validation/M5/04-bloat-calibration.json`. The
/// mechanism — `cargo-bloat` 0.12.1 derives an environment key from the profile
/// name, and a hyphenated name yields one Cargo re-splits over `profile.release`
/// — is inferred from the analyzer's source, not measured.
/// `release_lto` is therefore requested as `--release` plus the product-owned
/// environment variable `CARGO_PROFILE_RELEASE_LTO=fat`, which is closed argv
/// and closed environment, never a caller-supplied profile name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BloatProfile {
    #[default]
    Release,
    ReleaseLto,
}
impl BloatProfile {
    pub fn cargo_name(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::ReleaseLto => "release-lto",
        }
    }
    pub fn link_time_optimized(self) -> bool {
        matches!(self, Self::ReleaseLto)
    }
}

/// `^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}$`. A path and an argument fail it, and so
/// does a flag: the leading `-` is excluded on purpose, because an alphabet that
/// admits `-noplot` admits something that reads as an option wherever this name
/// is later placed on an argv.
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct BloatOptions {
    binary_target: String,
    package: Option<String>,
    profile: BloatProfile,
}
impl BloatOptions {
    pub fn new(
        binary_target: String,
        package: Option<String>,
        profile: BloatProfile,
    ) -> Result<Self, BloatError> {
        if !valid_name(&binary_target) {
            return Err(BloatError::InvalidTarget);
        }
        if package.as_deref().is_some_and(|name| !valid_name(name)) {
            return Err(BloatError::InvalidPackage);
        }
        Ok(Self {
            binary_target,
            package,
            profile,
        })
    }
    pub fn binary_target(&self) -> &str {
        &self.binary_target
    }
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
    pub fn profile(&self) -> BloatProfile {
        self.profile
    }
}

/// The binary format the analyzer recognised. WASM is rejected because the
/// pinned backend does not support it; Mach-O and PE are not qualified by the
/// ELF positive and are reported, never silently treated as equivalent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryFormat {
    Elf64Aarch64,
    OtherElf,
    MachO,
    Pe,
    Wasm,
    Unknown,
}

/// Facts this product measured itself inside the guest. Independent of anything
/// `cargo-bloat` reported, and therefore usable as an oracle against it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MeasuredBinary {
    pub size_bytes: u64,
    pub sha256: String,
    pub format: BinaryFormat,
    /// Always true, and serialized so no consumer can miss it: the analyzer
    /// pushes `CARGO_PROFILE_<PROFILE>_STRIP=false` on every build because it
    /// needs the symbol table (`cargo-bloat` 0.12.1, `src/main.rs:694-696`).
    /// The measured file is therefore an **analysis build**. Its size is exact
    /// for that file, and it is not the file a project that asks for stripping
    /// would ship. A stripped report is unreachable through this analyzer.
    pub analysis_build_symbols_forced: bool,
}

/// Estimated attribution. Every field here comes from the analyzer.
///
/// The two omission counters are **ranking coverage**, which ADR-079 §1 keeps
/// apart from measurement validity: they say how much of the analyzer's ranking
/// this product's own row cap ([`BLOAT_MAX_ROWS`]) left out. A capped ranking is
/// the attribution the contract promises, not incomplete evidence, so nothing
/// here may feed [`BloatObservation::analysis_validated`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BloatAttribution {
    /// Always true. Serialized so no consumer can read these rows as exact.
    pub estimated: bool,
    pub reported_file_size_bytes: Option<u64>,
    pub text_section_size_bytes: Option<u64>,
    pub functions: Vec<BloatFunction>,
    pub crates: Vec<BloatCrate>,
    /// Function rows [`BLOAT_MAX_ROWS`] left out of the ranking. The dropped
    /// rows were the smallest; every row kept is estimated at least as large.
    pub functions_omitted_by_row_cap: u32,
    /// Crate rows [`BLOAT_MAX_ROWS`] left out of the ranking.
    pub crates_omitted_by_row_cap: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BloatFunction {
    pub crate_name: String,
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BloatCrate {
    pub name: String,
    pub size_bytes: u64,
}

/// Whether the measurement itself is valid, and nothing else (ADR-079 §1).
///
/// This vocabulary answers one question: did the analyzer run, produce a report
/// this product could parse, and report a file size that agrees with the size
/// this product measured on its own? It deliberately has no variant for a
/// ranking the product's own cap bounded, because that is coverage rather than
/// validity: it is declared in [`BloatAttribution`]'s omission counters and it
/// never downgrades this enum. A `Truncated` variant lived here until ADR-079
/// and was the whole defect — a declared product cap read as invalid evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BloatCompleteness {
    Complete,
    /// The analyzer's file size disagreed with the size this product measured.
    /// The attribution then describes some other file and is not published as
    /// if it described this one.
    SizeMismatch,
    UnsupportedFormat,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BloatExit {
    Passed,
    AnalysisFailed,
    CompilationFailed,
    Uncalibrated,
    Incomplete,
}
impl BloatExit {
    /// Not yet confirmed by a Docker calibration receipt.
    pub const CALIBRATED: bool = false;
    pub fn classify(code: i32) -> Self {
        match code {
            0 => Self::Passed,
            1 => Self::AnalysisFailed,
            101 => Self::CompilationFailed,
            _ => Self::Uncalibrated,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BloatObservation {
    pub options: BloatOptions,
    pub analyzer_version: String,
    pub exit: BloatExit,
    pub exit_code: Option<i32>,
    pub termination: ExecutionTermination,
    pub measured: Option<MeasuredBinary>,
    pub attribution: Option<BloatAttribution>,
    pub completeness: BloatCompleteness,
    pub report: Vec<u8>,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub vendor_fingerprint: SourceFingerprint,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}
impl BloatObservation {
    /// Measurement validity, which is the only one of ADR-079 §1's three
    /// concepts allowed to decide a status.
    ///
    /// True means exactly what ADR-079 §2 lets `passed` mean: the analyzer
    /// executed, this product could parse what it produced, and **the exact
    /// measured file size is present and the analyzer agrees with it**. It does
    /// not mean the binary is optimized, and it does not mean the attribution is
    /// exhaustive — a ranking bounded by [`BLOAT_MAX_ROWS`] is still a valid
    /// measurement, and how many rows that cap left out is declared beside the
    /// rows instead of being folded in here.
    ///
    /// The exact size is load-bearing on purpose: without a measurement of our
    /// own there is nothing to check the analyzer against, and its estimate
    /// would be publishable as if it described a file we never measured.
    pub fn analysis_validated(&self) -> bool {
        if self.exit != BloatExit::Passed || self.completeness != BloatCompleteness::Complete {
            return false;
        }
        match (&self.measured, &self.attribution) {
            (Some(measured), Some(attribution)) => {
                attribution.estimated
                    && attribution.reported_file_size_bytes == Some(measured.size_bytes)
            }
            _ => false,
        }
    }

    /// `Complete` requires both an exact measurement and an attribution whose
    /// reported size matches it. Anything else downgrades completeness.
    pub fn consistent(&self) -> bool {
        match (&self.measured, &self.attribution, self.completeness) {
            (Some(measured), Some(attribution), BloatCompleteness::Complete) => {
                attribution.estimated
                    && attribution.reported_file_size_bytes == Some(measured.size_bytes)
            }
            (Some(measured), Some(attribution), BloatCompleteness::SizeMismatch) => {
                attribution.estimated
                    && attribution.reported_file_size_bytes != Some(measured.size_bytes)
            }
            (_, Some(attribution), _) => attribution.estimated,
            (_, None, BloatCompleteness::Complete) => false,
            _ => true,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;

    fn attribution(reported: Option<u64>) -> BloatAttribution {
        BloatAttribution {
            estimated: true,
            reported_file_size_bytes: reported,
            text_section_size_bytes: Some(1024),
            functions: Vec::new(),
            crates: Vec::new(),
            functions_omitted_by_row_cap: 0,
            crates_omitted_by_row_cap: 0,
        }
    }
    fn measured(size: u64) -> MeasuredBinary {
        MeasuredBinary {
            size_bytes: size,
            sha256: format!("sha256:{}", "0".repeat(64)),
            format: BinaryFormat::Elf64Aarch64,
            analysis_build_symbols_forced: true,
        }
    }
    fn observation(
        measured_binary: Option<MeasuredBinary>,
        attributed: Option<BloatAttribution>,
        completeness: BloatCompleteness,
    ) -> BloatObservation {
        BloatObservation {
            options: BloatOptions::new("fixture".into(), None, BloatProfile::Release)
                .expect("options"),
            analyzer_version: APPROVED_CARGO_BLOAT_VERSION.into(),
            exit: BloatExit::Passed,
            exit_code: Some(0),
            termination: ExecutionTermination::Exited,
            measured: measured_binary,
            attribution: attributed,
            completeness,
            report: Vec::new(),
            runtime: RuntimeIdentity {
                platform: "linux/aarch64".into(),
                image_id: "sha256:0".into(),
                configuration_fingerprint: format!("sha256:{}", "0".repeat(64))
                    .parse()
                    .expect("fingerprint"),
                execution_fingerprint: format!("sha256:{}", "1".repeat(64))
                    .parse()
                    .expect("fingerprint"),
                rust_version: "1.98.1".into(),
                cargo_version: "1.98.1".into(),
                declared_toolchain: None,
            },
            execution_fingerprint: format!("sha256:{}", "1".repeat(64))
                .parse()
                .expect("fingerprint"),
            vendor_fingerprint: format!("sha256:{}", "2".repeat(64))
                .parse()
                .expect("fingerprint"),
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
        }
    }

    #[test]
    fn profiles_are_closed_and_declare_their_lto() {
        assert_eq!(BloatProfile::Release.cargo_name(), "release");
        assert_eq!(BloatProfile::ReleaseLto.cargo_name(), "release-lto");
        assert!(!BloatProfile::Release.link_time_optimized());
        assert!(BloatProfile::ReleaseLto.link_time_optimized());
    }

    #[test]
    fn options_reject_paths_and_free_text() {
        assert!(
            BloatOptions::new(
                "fixture".into(),
                Some("inner".into()),
                BloatProfile::Release
            )
            .is_ok()
        );
        assert!(
            BloatOptions::new("fix-ture_2".into(), None, BloatProfile::Release).is_ok(),
            "a hyphen inside the name is a legitimate cargo target"
        );
        // A leading `-` reads as an option wherever the name lands on an argv.
        for target in [
            "",
            "/bin/sh",
            "a b",
            "a;b",
            "-release",
            "--bin",
            "-",
            &"a".repeat(65),
        ] {
            assert_eq!(
                BloatOptions::new(target.into(), None, BloatProfile::Release).unwrap_err(),
                BloatError::InvalidTarget
            );
        }
        assert_eq!(
            BloatOptions::new("ok".into(), Some("-p".into()), BloatProfile::Release).unwrap_err(),
            BloatError::InvalidPackage
        );
        assert_eq!(
            BloatOptions::new("ok".into(), Some("bad name".into()), BloatProfile::Release)
                .unwrap_err(),
            BloatError::InvalidPackage
        );
    }

    #[test]
    fn a_disagreeing_reported_size_can_never_be_complete() {
        assert!(
            observation(
                Some(measured(4096)),
                Some(attribution(Some(4096))),
                BloatCompleteness::Complete
            )
            .consistent()
        );
        assert!(
            !observation(
                Some(measured(4096)),
                Some(attribution(Some(4095))),
                BloatCompleteness::Complete
            )
            .consistent()
        );
        assert!(
            observation(
                Some(measured(4096)),
                Some(attribution(Some(4095))),
                BloatCompleteness::SizeMismatch
            )
            .consistent()
        );
        assert!(
            !observation(
                Some(measured(4096)),
                Some(attribution(None)),
                BloatCompleteness::Complete
            )
            .consistent()
        );
        assert!(!observation(None, None, BloatCompleteness::Complete).consistent());
    }

    /// ADR-079 §2. The product's own row cap is not a defect in the
    /// measurement: a ranking it bounded is still a validated analysis, and the
    /// number of rows it dropped travels beside the rows. This is the assertion
    /// that fails the moment ranking coverage is folded back into validity.
    #[test]
    fn a_ranking_the_product_s_own_cap_bounded_is_still_a_validated_analysis() {
        let mut capped = attribution(Some(4096));
        capped.functions_omitted_by_row_cap = 378;
        capped.crates_omitted_by_row_cap = 4;
        let observed = observation(
            Some(measured(4096)),
            Some(capped),
            BloatCompleteness::Complete,
        );
        assert!(observed.analysis_validated());
        assert!(observed.consistent());
        let declared = observed.attribution.as_ref().expect("attribution");
        assert_eq!(declared.functions_omitted_by_row_cap, 378);
        assert_eq!(declared.crates_omitted_by_row_cap, 4);
        // And the exact measured size survives the capped ranking untouched.
        assert_eq!(
            observed.measured.as_ref().map(|binary| binary.size_bytes),
            Some(4096)
        );
    }

    /// The four states ADR-079 §3 keeps out of `passed`, plus the two shapes
    /// that leave nothing to check the analyzer against.
    #[test]
    fn validity_still_refuses_every_state_adr_079_keeps_blocking() {
        // A size disagreement: the attribution describes some other file.
        assert!(
            !observation(
                Some(measured(4096)),
                Some(attribution(Some(4095))),
                BloatCompleteness::SizeMismatch
            )
            .analysis_validated()
        );
        for refused in [
            BloatCompleteness::Unavailable,
            BloatCompleteness::UnsupportedFormat,
        ] {
            assert!(
                !observation(Some(measured(4096)), Some(attribution(Some(4096))), refused)
                    .analysis_validated(),
                "{refused:?}"
            );
        }
        for failure in [BloatExit::CompilationFailed, BloatExit::AnalysisFailed] {
            let mut failed = observation(None, None, BloatCompleteness::Unavailable);
            failed.exit = failure;
            failed.exit_code = Some(1);
            assert!(!failed.analysis_validated(), "{failure:?}");
        }
        // No exact measurement of our own: nothing checks the analyzer.
        assert!(
            !observation(
                None,
                Some(attribution(Some(4096))),
                BloatCompleteness::Complete
            )
            .analysis_validated()
        );
        // A measurement the analyzer never named a size for.
        assert!(
            !observation(
                Some(measured(4096)),
                Some(attribution(None)),
                BloatCompleteness::Complete
            )
            .analysis_validated()
        );
        // Rows that do not declare themselves estimated.
        let mut exact_looking = attribution(Some(4096));
        exact_looking.estimated = false;
        assert!(
            !observation(
                Some(measured(4096)),
                Some(exact_looking),
                BloatCompleteness::Complete
            )
            .analysis_validated()
        );
    }

    #[test]
    fn exit_codes_classify_without_claiming_calibration() {
        // The exit table is a hypothesis until a Docker receipt records it.
        let calibrated: bool = BloatExit::CALIBRATED;
        assert!(!calibrated, "exit codes are not calibrated yet");
        assert_eq!(BloatExit::classify(0), BloatExit::Passed);
        assert_eq!(BloatExit::classify(1), BloatExit::AnalysisFailed);
        assert_eq!(BloatExit::classify(101), BloatExit::CompilationFailed);
        assert_eq!(BloatExit::classify(7), BloatExit::Uncalibrated);
    }
}
