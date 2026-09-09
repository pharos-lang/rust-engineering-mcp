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

/// Why a run produced no dataset, or no harness output tree, or a partial one.
///
/// The same vocabulary answers both questions because both evidences come from
/// the same export: an unrecognised harness, a failed execution, an absent
/// export and an oversize one are the reasons either of them is missing.
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

/// ADR-076 §7 caps the retained samples at 32 MiB. The criterion output tree is
/// those samples in the framing the harness wrote them in, so it carries the
/// same ceiling — and it carries it as a refusal, never as a trim. A USTAR
/// stream cut short is not an archive: publishing its first bytes under
/// `criterion_archive`/`UstarV1` would describe a file the harness never wrote.
/// An oversize tree is therefore a declared [`DatasetOmission::OutputTooLarge`]
/// and no member is committed for it.
pub const BENCHMARK_MAX_ARCHIVE_BYTES: usize = 32 * 1024 * 1024;

/// The harness output tree of **exactly one repetition**, retained verbatim.
///
/// ADR-073 §2 runs each repetition into its own `CRITERION_HOME`, so a run of
/// three repetitions exports three independent trees whose directory names all
/// collide. This type is deliberately not a merge of them, and the published
/// `criterion_archive` artifact is deliberately not a concatenation:
///
/// - merging the trees would produce a directory layout criterion never wrote,
///   and the `benchmark.json`/`sample.json` under a shared name would have to
///   come from one repetition while claiming to describe the group;
/// - concatenating the USTAR streams would put duplicate paths in one archive,
///   which is precisely the framing this server refuses when it decodes one;
/// - ADR-076 §3 names one `criterion_archive` beside one `benchmark_dataset`,
///   and the tool response admits at most those two members.
///
/// So the published artifact is **one repetition's tree**, and `run_index` says
/// which one rather than leaving the reader to guess. The dataset published
/// beside it is the one that pools every repetition; each of its samples
/// already carries its own `run_index`, so nothing about the other repetitions
/// is lost by retaining one tree.
#[derive(Clone, Debug, Serialize)]
pub struct CriterionArchive {
    /// 1-based position of the repetition inside the requested run set, the
    /// same numbering the dataset's samples carry.
    pub run_index: u8,
    /// The USTAR stream exactly as the guest exported it, byte for byte.
    pub bytes: Vec<u8>,
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
    /// The harness output tree ADR-076 §3 publishes as `criterion_archive`,
    /// present only when one repetition's tree was retained whole and within
    /// [`BENCHMARK_MAX_ARCHIVE_BYTES`].
    pub archive: Option<CriterionArchive>,
    /// Why no tree is carried. Mutually exclusive with `archive`, exactly as
    /// `omission` is with `dataset`: a tree that could not be retained is a
    /// declared result, never a silent absence.
    pub archive_omission: Option<DatasetOmission>,
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
        self.dataset_consistent() && self.archive_consistent()
    }

    fn dataset_consistent(&self) -> bool {
        match (&self.dataset, self.omission) {
            (Some(_), None) => self.harness.measurable() && self.runs_completed >= 1,
            (None, Some(_)) => true,
            _ => false,
        }
    }

    /// The retained tree obeys the same exclusion, plus the bound it is
    /// published under.
    ///
    /// A tree is evidence of *this* run, so it names a repetition inside the
    /// requested set; it is bytes, so an empty one is an absence and not an
    /// archive; and it is capped by [`BENCHMARK_MAX_ARCHIVE_BYTES`], because
    /// the alternative to refusing an oversize tree is committing a member that
    /// misdescribes what it contains.
    ///
    /// Whether a retained tree may be *published* is a separate question this
    /// type does not answer: an export the server could not turn into a
    /// measurement is still a coherent observation, and the application decides
    /// that `criterion_archive` only ever travels beside `benchmark_dataset`.
    fn archive_consistent(&self) -> bool {
        match (&self.archive, self.archive_omission) {
            (Some(archive), None) => {
                archive.run_index >= 1
                    && archive.run_index <= self.runs_requested
                    && !archive.bytes.is_empty()
                    && archive.bytes.len() <= BENCHMARK_MAX_ARCHIVE_BYTES
            }
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
            // The archive pair is exercised on its own below; a fixture that
            // carries no tree still has to say so.
            archive: None,
            archive_omission: Some(DatasetOmission::OutputMissing),
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

    /// The archive obeys the same exclusion the dataset does: a run either
    /// retained one repetition's tree or said why it did not.
    #[test]
    fn a_retained_tree_and_its_omission_cannot_coexist_or_both_be_absent() {
        let mut observed = observation(
            HarnessDetection::Unrecognized,
            None,
            Some(DatasetOmission::HarnessUnrecognized),
            0,
        );
        assert!(
            observed.consistent(),
            "an omitted tree is a declared result"
        );

        observed.archive = Some(CriterionArchive {
            run_index: 1,
            bytes: b"tree".to_vec(),
        });
        assert!(
            !observed.consistent(),
            "a tree and its omission cannot both be claimed"
        );

        observed.archive_omission = None;
        assert!(observed.consistent(), "a retained tree stands on its own");

        observed.archive = None;
        assert!(
            !observed.consistent(),
            "no tree and no reason is an undeclared absence"
        );
    }

    /// A tree is evidence of one repetition of *this* run. Position zero and a
    /// position past the requested set both describe some other execution.
    #[test]
    fn a_retained_tree_names_a_repetition_of_this_run() {
        let mut observed = observation(
            HarnessDetection::Criterion {
                version: "0.8.2".into(),
            },
            None,
            Some(DatasetOmission::OutputMissing),
            3,
        );
        observed.archive_omission = None;
        assert_eq!(observed.runs_requested, 3);
        for run_index in 1..=observed.runs_requested {
            observed.archive = Some(CriterionArchive {
                run_index,
                bytes: b"tree".to_vec(),
            });
            assert!(observed.consistent(), "rejected repetition {run_index}");
        }
        for run_index in [0, observed.runs_requested + 1, u8::MAX] {
            observed.archive = Some(CriterionArchive {
                run_index,
                bytes: b"tree".to_vec(),
            });
            assert!(!observed.consistent(), "accepted repetition {run_index}");
        }
    }

    /// ADR-076 §7 caps the retained tree at 32 MiB. A tar cut at the ceiling is
    /// not a tar, so the bound refuses the archive instead of trimming it: the
    /// adapter declares `OutputTooLarge` and no member is published.
    #[test]
    fn a_tree_over_the_published_ceiling_is_never_consistent_evidence() {
        let mut observed = observation(
            HarnessDetection::Criterion {
                version: "0.8.2".into(),
            },
            None,
            Some(DatasetOmission::OutputMissing),
            3,
        );
        observed.archive_omission = None;
        observed.archive = Some(CriterionArchive {
            run_index: 1,
            bytes: vec![0_u8; BENCHMARK_MAX_ARCHIVE_BYTES],
        });
        assert!(observed.consistent(), "exactly at the ceiling is retained");
        observed.archive = Some(CriterionArchive {
            run_index: 1,
            bytes: vec![0_u8; BENCHMARK_MAX_ARCHIVE_BYTES + 1],
        });
        assert!(
            !observed.consistent(),
            "one byte past the ceiling is refused, never trimmed"
        );
        // Zero bytes is an absence wearing an archive's name.
        observed.archive = Some(CriterionArchive {
            run_index: 1,
            bytes: Vec::new(),
        });
        assert!(!observed.consistent());
    }
}
