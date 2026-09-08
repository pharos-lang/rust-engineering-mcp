//! Observation contract for one `rust.benchmark.run` execution (M5-01).
//!
//! The dataset in [`crate::benchmark`] is the measurement; this module is what
//! the execution adapter observed while producing it. Keeping them apart lets a
//! run report a real execution — its exit, its logs, an unrecognised harness —
//! without implying that a comparable measurement exists.
use crate::benchmark::{BenchmarkDataset, BenchmarkSelection};
use crate::{ExecutionFingerprint, ExecutionTermination, RuntimeIdentity, SourceFingerprint};
use serde::Serialize;

/// What the adapter found in the project's resolved dependency graph.
///
/// A harness is never inferred from a target name or a manifest comment: it is
/// the exact package and version Cargo resolved, or it is unrecognised.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "harness", rename_all = "snake_case")]
pub enum HarnessDetection {
    /// The approved harness at the approved version.
    Criterion { version: String },
    /// Criterion is present at a version this server has not frozen a format
    /// for. Execution and logs are reportable; a dataset is not.
    CriterionUnapproved { version: String },
    /// Some other, or no, benchmark harness. Never a measurement.
    Unrecognized,
}
impl HarnessDetection {
    pub fn measurable(&self) -> bool {
        matches!(self, Self::Criterion { .. })
    }
}

/// Exit classification for `cargo bench`. These are hypotheses until a
/// calibration receipt records the observed codes, exactly like `SemverExit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkExit {
    Passed,
    BenchmarkFailed,
    CompilationFailed,
    Uncalibrated,
    Incomplete,
}
impl BenchmarkExit {
    /// Not yet confirmed by a Docker calibration receipt.
    pub const CALIBRATED: bool = false;
    pub fn classify(code: i32) -> Self {
        match code {
            0 => Self::Passed,
            100 => Self::CompilationFailed,
            101 => Self::BenchmarkFailed,
            _ => Self::Uncalibrated,
        }
    }
}

/// Why a run produced no dataset, or a partial one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DatasetOmission {
    HarnessUnrecognized,
    HarnessUnapproved,
    ExecutionFailed,
    OutputMissing,
    OutputUnparsable,
    OutputTooLarge,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkObservation {
    pub selection: BenchmarkSelection,
    pub harness: HarnessDetection,
    pub exit: BenchmarkExit,
    pub exit_code: Option<i32>,
    pub termination: ExecutionTermination,
    /// Present only when the harness was approved and its output parsed whole.
    pub dataset: Option<BenchmarkDataset>,
    pub omission: Option<DatasetOmission>,
    /// Independent executions actually completed, at most `run_count`.
    pub runs_completed: u8,
    pub runs_requested: u8,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub vendor_fingerprint: SourceFingerprint,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}
impl BenchmarkObservation {
    /// A dataset and an omission are mutually exclusive, and a dataset can only
    /// come from the approved harness. Callers check this before publishing.
    pub fn consistent(&self) -> bool {
        match (&self.dataset, self.omission) {
            (Some(_), None) => self.harness.measurable() && self.runs_completed >= 1,
            (None, Some(_)) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;

    fn identity() -> RuntimeIdentity {
        RuntimeIdentity {
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
        }
    }

    fn observation(
        harness: HarnessDetection,
        dataset: Option<BenchmarkDataset>,
        omission: Option<DatasetOmission>,
        runs_completed: u8,
    ) -> BenchmarkObservation {
        BenchmarkObservation {
            selection: BenchmarkSelection {
                package: None,
                bench_target: Some("perf".into()),
                features: Vec::new(),
                all_features: false,
                no_default_features: false,
                profile: "bench".into(),
            },
            harness,
            exit: BenchmarkExit::Passed,
            exit_code: Some(0),
            termination: ExecutionTermination::Exited,
            dataset,
            omission,
            runs_completed,
            runs_requested: 3,
            runtime: identity(),
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
    fn only_the_approved_harness_is_measurable() {
        assert!(
            HarnessDetection::Criterion {
                version: "0.8.2".into()
            }
            .measurable()
        );
        assert!(
            !HarnessDetection::CriterionUnapproved {
                version: "0.5.1".into()
            }
            .measurable()
        );
        assert!(!HarnessDetection::Unrecognized.measurable());
    }

    #[test]
    fn exit_codes_classify_without_claiming_calibration() {
        // The exit table is a hypothesis until a Docker receipt records it.
        let calibrated: bool = BenchmarkExit::CALIBRATED;
        assert!(!calibrated, "exit codes are not calibrated yet");
        assert_eq!(BenchmarkExit::classify(0), BenchmarkExit::Passed);
        assert_eq!(
            BenchmarkExit::classify(100),
            BenchmarkExit::CompilationFailed
        );
        assert_eq!(BenchmarkExit::classify(101), BenchmarkExit::BenchmarkFailed);
        for code in [1, 2, 42, -1, 137] {
            assert_eq!(BenchmarkExit::classify(code), BenchmarkExit::Uncalibrated);
        }
    }

    #[test]
    fn a_dataset_and_an_omission_cannot_coexist_or_both_be_absent() {
        assert!(
            observation(
                HarnessDetection::Unrecognized,
                None,
                Some(DatasetOmission::HarnessUnrecognized),
                0
            )
            .consistent()
        );
        assert!(!observation(HarnessDetection::Unrecognized, None, None, 0).consistent());
    }
}
