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

fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
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
}

/// Estimated attribution. Every field here comes from the analyzer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BloatAttribution {
    /// Always true. Serialized so no consumer can read these rows as exact.
    pub estimated: bool,
    pub reported_file_size_bytes: Option<u64>,
    pub text_section_size_bytes: Option<u64>,
    pub functions: Vec<BloatFunction>,
    pub crates: Vec<BloatCrate>,
    pub functions_omitted: u32,
    pub crates_omitted: u32,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BloatCompleteness {
    Complete,
    /// The analyzer's file size disagreed with the size this product measured.
    /// The attribution then describes some other file and is not published as
    /// if it described this one.
    SizeMismatch,
    Truncated,
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
            functions_omitted: 0,
            crates_omitted: 0,
        }
    }
    fn measured(size: u64) -> MeasuredBinary {
        MeasuredBinary {
            size_bytes: size,
            sha256: format!("sha256:{}", "0".repeat(64)),
            format: BinaryFormat::Elf64Aarch64,
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
        for target in ["", "/bin/sh", "a b", "a;b", &"a".repeat(65)] {
            assert_eq!(
                BloatOptions::new(target.into(), None, BloatProfile::Release).unwrap_err(),
                BloatError::InvalidTarget
            );
        }
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
