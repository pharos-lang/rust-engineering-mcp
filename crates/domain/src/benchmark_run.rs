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

/// ADR-080 §3: the ceiling one repetition's retained stream is published under.
///
/// It is the product's own ceiling and deliberately smaller than the 512 KiB
/// the supervisor captures a stream at, so cutting a log is a reachable state
/// this contract has to describe rather than a branch nothing can enter. A log
/// that reached it is published as a declared prefix — `truncated` on the
/// repetition and `truncated` completeness on the member — never as a whole
/// stream. Two streams times three repetitions bound one run's log evidence at
/// 1.5 MiB, well inside the store's 64 MiB job ceiling.
pub const BENCHMARK_MAX_LOG_BYTES: usize = 256 * 1024;

/// One repetition's harness logs, retained as evidence (ADR-080 §1, §2).
///
/// ADR-073 §2 runs each repetition as its own execution, so each has its own
/// `stdout` and `stderr`. They are **never concatenated**: a merged stream
/// would attribute one repetition's compiler error to the group, and nothing on
/// the wire could take it back apart. Each set therefore carries the `run_index`
/// of the execution that wrote it — the same 1-based numbering the dataset's
/// samples and [`CriterionArchive`] carry.
///
/// Both streams are bounded by [`BENCHMARK_MAX_LOG_BYTES`] and each declares
/// its own cut. An empty stream is an absence, not a cut: a repetition that
/// wrote nothing to `stderr` publishes no `stderr` member and cannot claim to
/// have been truncated.
#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkRunLog {
    /// 1-based position of the repetition inside the requested run set.
    pub run_index: u8,
    pub stdout: Vec<u8>,
    /// The harness wrote more `stdout` than [`BENCHMARK_MAX_LOG_BYTES`], and
    /// `stdout` is the prefix that was kept.
    pub stdout_truncated: bool,
    /// `stdout` carried bytes that are not valid UTF-8 and they were replaced.
    ///
    /// The bytes come from a benchmark the project wrote, so they are
    /// arbitrary. They are published declaring `Utf8LogV1`, and an artifact
    /// whose bytes do not satisfy its declared format is a lie about the
    /// evidence — so the adapter guarantees validity and this flag says when it
    /// had to intervene. Replacing rather than refusing is deliberate: these
    /// logs exist to diagnose a failed run, and a run that failed while
    /// emitting one stray byte is exactly when the rest of the text matters.
    pub stdout_replaced: bool,
    pub stderr: Vec<u8>,
    pub stderr_truncated: bool,
    /// `stderr` carried bytes that are not valid UTF-8 and they were replaced.
    pub stderr_replaced: bool,
}

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
/// - ADR-076 §3 names one `criterion_archive` beside one `benchmark_dataset`.
///
/// So the published artifact is **one repetition's tree**, and `run_index` says
/// which one rather than leaving the reader to guess. The dataset published
/// beside it is the one that pools every repetition; each of its samples
/// already carries its own `run_index`, so nothing about the other repetitions
/// is lost by retaining one tree.
///
/// Which repetition it is: the last one that **exported** a tree, which need
/// not be the last one that ran. [`BenchmarkObservation::exit_run_index`] names
/// the latter, and ADR-080 §4 requires both to be on the wire precisely because
/// they can differ.
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
    /// 1-based repetition whose `exit`, `exit_code` and `termination` the three
    /// fields above report: the LAST repetition that ran.
    ///
    /// It is on the observation because it is not always the repetition whose
    /// tree [`Self::archive`] retained (ADR-080 §4). A run whose third
    /// repetition failed without exporting keeps the second repetition's tree
    /// and reports the third repetition's exit, and without both indices a
    /// reader cannot tell that apart from a run where they coincide.
    pub exit_run_index: u8,
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
    /// One entry per repetition that ran, in run order (ADR-080 §2). Never a
    /// merge: the entry's own `run_index` says which execution wrote it.
    pub logs: Vec<BenchmarkRunLog>,
}
impl BenchmarkObservation {
    /// A dataset and an omission are mutually exclusive, and a dataset can only
    /// come from the approved harness. Callers check this before publishing.
    pub fn consistent(&self) -> bool {
        self.dataset_consistent() && self.archive_consistent() && self.logs_consistent()
    }

    /// Any repetition's `stdout` was cut at the published ceiling.
    ///
    /// Derived, never stored: the per-repetition flags in [`Self::logs`] are
    /// the record, and a second stored copy could disagree with them.
    pub fn any_stdout_truncated(&self) -> bool {
        self.logs.iter().any(|log| log.stdout_truncated)
    }

    /// Any repetition's `stderr` was cut at the published ceiling.
    pub fn any_stderr_truncated(&self) -> bool {
        self.logs.iter().any(|log| log.stderr_truncated)
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
                    // ADR-080 §4: the tree comes from a repetition that ran, so
                    // it can precede the repetition whose exit is reported but
                    // can never follow it.
                    && archive.run_index <= self.exit_run_index
                    && !archive.bytes.is_empty()
                    && archive.bytes.len() <= BENCHMARK_MAX_ARCHIVE_BYTES
            }
            (None, Some(_)) => true,
            _ => false,
        }
    }

    /// The logs describe repetitions of *this* run, one entry each, in order.
    ///
    /// Every repetition that ran leaves an entry even when it wrote nothing, so
    /// the last entry names the repetition whose exit this observation reports.
    /// Indices strictly increase, which is what makes "not concatenated" a
    /// checkable property rather than a comment: two entries claiming one
    /// repetition, or one entry standing for several, both fail here.
    ///
    /// Each stream is bounded by [`BENCHMARK_MAX_LOG_BYTES`], and a stream that
    /// kept nothing cannot claim to have been cut — that would be a log
    /// declaring a truncation of no bytes at all.
    fn logs_consistent(&self) -> bool {
        if self.logs.is_empty()
            || self.logs.len() > usize::from(self.runs_requested)
            || self.exit_run_index < 1
            || self.exit_run_index > self.runs_requested
        {
            return false;
        }
        let mut previous = 0_u8;
        for log in &self.logs {
            if log.run_index <= previous || log.run_index > self.runs_requested {
                return false;
            }
            previous = log.run_index;
            let bounded = |bytes: &Vec<u8>, truncated: bool| {
                bytes.len() <= BENCHMARK_MAX_LOG_BYTES && !(bytes.is_empty() && truncated)
            };
            if !bounded(&log.stdout, log.stdout_truncated)
                || !bounded(&log.stderr, log.stderr_truncated)
            {
                return false;
            }
        }
        previous == self.exit_run_index
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
            // Three repetitions ran, so the exit reported is the third one's.
            exit_run_index: 3,
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
            logs: (1..=3).map(log).collect(),
        }
    }

    /// One repetition's logs, distinguishable from every other repetition's so
    /// a merge would be visible.
    fn log(run_index: u8) -> BenchmarkRunLog {
        BenchmarkRunLog {
            run_index,
            stdout: format!("stdout of repetition {run_index}").into_bytes(),
            stdout_truncated: false,
            stdout_replaced: false,
            stderr: format!("stderr of repetition {run_index}").into_bytes(),
            stderr_truncated: false,
            stderr_replaced: false,
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

    /// ADR-080 §2. Every repetition that ran leaves its own entry, and the
    /// entries are ordered and unique. A vector that merged three repetitions
    /// into one entry, or repeated an index, is not an observation of this run.
    #[test]
    fn each_repetitions_logs_stand_alone_and_name_their_own_repetition() {
        let mut observed = observation(
            HarnessDetection::Criterion {
                version: "0.8.2".into(),
            },
            None,
            Some(DatasetOmission::OutputMissing),
            3,
        );
        assert!(observed.consistent(), "three ordered repetitions");
        assert_eq!(observed.logs.len(), 3);
        for (position, entry) in observed.logs.iter().enumerate() {
            let expected = u8::try_from(position + 1).expect("three repetitions");
            assert_eq!(entry.run_index, expected);
        }
        // The three repetitions' bytes are distinct, so a concatenation would
        // be a different value and not a longer one of the same shape.
        assert_ne!(observed.logs[0].stdout, observed.logs[2].stdout);

        // One entry standing for the group: the exit names repetition three and
        // no entry does.
        observed.logs = vec![log(1)];
        assert!(!observed.consistent(), "a merged log lost two repetitions");

        // Two entries claiming one repetition.
        observed.logs = vec![log(1), log(1), log(3)];
        assert!(!observed.consistent(), "duplicate repetition accepted");

        // Out of run order.
        observed.logs = vec![log(2), log(1), log(3)];
        assert!(!observed.consistent(), "unordered repetitions accepted");

        // A repetition outside the requested set.
        observed.logs = vec![log(1), log(2), log(4)];
        assert!(!observed.consistent(), "repetition four does not exist");

        // No logs at all: a run that produced an exit produced a repetition.
        observed.logs = Vec::new();
        assert!(!observed.consistent(), "an exit without a repetition");
    }

    /// ADR-080 §3. A cut log declares the cut; it is never published as whole,
    /// and the bytes it kept never exceed the published ceiling.
    #[test]
    fn a_cut_log_declares_the_cut_and_stays_inside_the_ceiling() {
        let mut observed = observation(
            HarnessDetection::Criterion {
                version: "0.8.2".into(),
            },
            None,
            Some(DatasetOmission::OutputMissing),
            3,
        );
        observed.logs = vec![
            log(1),
            log(2),
            BenchmarkRunLog {
                run_index: 3,
                stdout: vec![b'o'; BENCHMARK_MAX_LOG_BYTES],
                stdout_truncated: true,
                stdout_replaced: false,
                stderr: vec![b'e'; BENCHMARK_MAX_LOG_BYTES],
                stderr_truncated: true,
                stderr_replaced: false,
            },
        ];
        assert!(
            observed.consistent(),
            "exactly at the ceiling, declared cut"
        );
        assert!(observed.any_stdout_truncated());
        assert!(observed.any_stderr_truncated());

        observed.logs[2].stdout = vec![b'o'; BENCHMARK_MAX_LOG_BYTES + 1];
        assert!(
            !observed.consistent(),
            "one byte past the ceiling is never retained"
        );

        // A stream that kept nothing cannot have been cut: that would be a
        // truncation flag with no surviving evidence behind it.
        observed.logs[2].stdout = Vec::new();
        assert!(!observed.consistent(), "an empty stream claimed a cut");
        observed.logs[2].stdout_truncated = false;
        assert!(observed.consistent(), "an empty stream is an absence");
        assert!(!observed.any_stdout_truncated());
        assert!(observed.any_stderr_truncated());
    }

    /// ADR-080 §4, the reachable mismatch. Repetition three failed without
    /// exporting while one and two succeeded: the retained tree is repetition
    /// two's and the reported exit is repetition three's. That observation is
    /// legal — and both indices are on it, so a reader can see the difference.
    /// A tree from a repetition that never ran is not.
    #[test]
    fn the_retained_tree_may_precede_the_repetition_whose_exit_is_reported() {
        let mut observed = observation(
            HarnessDetection::Criterion {
                version: "0.8.2".into(),
            },
            None,
            Some(DatasetOmission::OutputMissing),
            2,
        );
        observed.archive_omission = None;
        observed.exit_run_index = 3;
        observed.archive = Some(CriterionArchive {
            run_index: 2,
            bytes: b"repetition two's tree".to_vec(),
        });
        assert!(
            observed.consistent(),
            "the mismatch is a real observation, not a contradiction"
        );
        assert_ne!(
            observed
                .archive
                .as_ref()
                .map(|archive| archive.run_index)
                .expect("tree"),
            observed.exit_run_index,
            "the two indices are separately readable"
        );
        assert_eq!(
            observed.logs.last().map(|log| log.run_index),
            Some(observed.exit_run_index),
            "the exit belongs to the last repetition that ran"
        );

        observed.archive = Some(CriterionArchive {
            run_index: 3,
            bytes: b"repetition three's tree".to_vec(),
        });
        assert!(
            observed.consistent(),
            "the last repetition may also be the exporting one"
        );

        // The exit says three repetitions ran; a tree from a fourth is not
        // evidence of this run.
        observed.exit_run_index = 2;
        observed.logs = vec![log(1), log(2)];
        assert!(
            !observed.consistent(),
            "a tree from a repetition after the last one that ran"
        );
    }
}
