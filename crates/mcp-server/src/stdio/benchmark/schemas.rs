//! Wire types for `rust.benchmark.run`.
//!
//! Every enum this tool publishes is declared here and nowhere else. A domain
//! enum is deliberately not re-exported into the wire contract: a later variant
//! added for the durable store would otherwise widen an already frozen tool
//! schema without anyone editing this file (ADR-076 §2).
//!
//! These types are also the *only* projection of a benchmark observation.
//! Neither the raw samples nor the harness logs reach them: the samples travel
//! in the `benchmark_dataset` artifact (ADR-076 §3) and each repetition's
//! `stdout`/`stderr` in their own artifacts (ADR-080 §1), and what appears here
//! is a bounded description of both.
use schemars::JsonSchema;
use serde::Serialize;

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

/// The harness the adapter found in the resolved dependency graph. It is never
/// inferred from a target name, and only `criterion` at the approved version
/// can carry a measurement.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(tag = "harness", rename_all = "snake_case", deny_unknown_fields)]
pub enum Harness {
    Criterion {
        #[schemars(length(min = 1, max = 128))]
        version: String,
    },
    CriterionUnapproved {
        #[schemars(length(min = 1, max = 128))]
        version: String,
    },
    Unrecognized,
}

/// Classification of the `cargo bench` exit code. `uncalibrated` is published
/// as such: no Docker receipt has pinned a meaning for that code yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Exit {
    Passed,
    BenchmarkFailed,
    CompilationFailed,
    Uncalibrated,
    Incomplete,
}

/// Why a run published no dataset, or an incomplete one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SamplingMode {
    Linear,
    Flat,
    Auto,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementCompleteness {
    Complete,
    Truncated,
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SampleUnit {
    Nanoseconds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkHarnessName {
    Criterion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Virtualization {
    Unknown,
    Container,
    VirtualMachine,
    Bare,
}

#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Feature(
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]{1,64}$"))] pub String,
);

/// The closed build/target selection the run executed under. Compared verbatim
/// between two datasets, so it is published verbatim here.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[schemars(length(min = 1, max = 64))]
    pub package: Option<String>,
    #[schemars(length(min = 1, max = 64))]
    pub bench_target: Option<String>,
    #[schemars(with = "Vec<Feature>", length(max = 16))]
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    #[schemars(length(min = 1, max = 64))]
    pub profile: String,
}

/// Resource ceilings the measuring runtime was placed under. `null` is UNKNOWN
/// and never means "unlimited".
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceQuotas {
    pub cpu_quota_millicores: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub pids: Option<u32>,
}

/// What was observable about the measuring host. An absent field is UNKNOWN and
/// is never replaced by a plausible default.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HardwareProfile {
    #[schemars(length(max = 512))]
    pub cpu_model: Option<String>,
    pub cpu_cores: Option<u16>,
    #[schemars(length(max = 512))]
    pub os_kernel: Option<String>,
    #[schemars(length(min = 1, max = 512))]
    pub arch: String,
    pub virtualization: Virtualization,
    #[schemars(length(max = 512))]
    pub cpu_governor: Option<String>,
    pub quotas: ResourceQuotas,
}

/// Everything a later comparison needs before it may quote these numbers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    #[schemars(length(min = 1, max = 512))]
    pub dataset_format: String,
    pub dataset_format_version: u8,
    pub unit: SampleUnit,
    #[schemars(length(min = 1, max = 512))]
    pub source_fingerprint: String,
    pub harness: BenchmarkHarnessName,
    #[schemars(length(min = 1, max = 128))]
    pub harness_version: String,
    #[schemars(length(min = 1, max = 128))]
    pub rust_version: String,
    #[schemars(length(min = 1, max = 128))]
    pub cargo_version: String,
    #[schemars(length(max = 128))]
    pub declared_toolchain: Option<String>,
    #[schemars(length(min = 1, max = 512))]
    pub image_digest: String,
    #[schemars(length(min = 1, max = 128))]
    pub platform: String,
    #[schemars(length(min = 1, max = 512))]
    pub configuration_fingerprint: String,
    #[schemars(length(min = 1, max = 512))]
    pub execution_fingerprint: String,
    pub selection: Selection,
    pub hardware: HardwareProfile,
    pub run_index: u8,
    pub run_count: u8,
    pub captured_at_unix: u64,
}

/// A bounded description of one benchmark's raw samples. Every statistic here
/// is descriptive of the sample set the dataset carries; none of it is a
/// comparison, a verdict or a claim about what produced any value.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BenchmarkSummary {
    #[schemars(length(min = 1, max = 512))]
    pub key: String,
    #[schemars(length(min = 1, max = 512))]
    pub group_id: String,
    #[schemars(length(max = 512))]
    pub function_id: Option<String>,
    #[schemars(length(max = 512))]
    pub value_str: Option<String>,
    pub samples: u32,
    pub sample_size_requested: u32,
    pub warm_up_ms: u64,
    pub measurement_ms: u64,
    pub median_per_iteration_ns: f64,
    pub minimum_per_iteration_ns: f64,
    pub maximum_per_iteration_ns: f64,
    /// Median of the absolute deviations from the median. Reported, like the
    /// outlier count, so a reader can see dispersion without the raw samples.
    pub median_absolute_deviation_ns: f64,
    /// Counted with Tukey fences and kept in the sample set; nothing is removed.
    pub outliers_counted: u32,
    pub sampling_mode: SamplingMode,
    pub completeness: MeasurementCompleteness,
}

/// What one repetition wrote, and what was published of it.
///
/// The logs themselves never travel in the response (ADR-080 §3): they are
/// published as `harness_stdout` and `harness_stderr` artifacts, each naming
/// the same `run_index` this row does. What travels here is how many bytes were
/// retained and whether the stream was cut at the server's ceiling, so a reader
/// knows before fetching whether the artifact is the whole stream.
///
/// `0` retained bytes means the repetition wrote nothing to that stream, and no
/// artifact was published for it.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HarnessLog {
    /// 1-based repetition inside the requested run set, the same numbering the
    /// dataset's samples and the criterion archive carry.
    #[schemars(range(min = 1, max = 3))]
    pub run_index: u8,
    pub stdout_bytes: u64,
    /// The repetition wrote more than `retained_ceiling_bytes` and
    /// `stdout_bytes` is the prefix that was kept. A cut log is never published
    /// as a whole one.
    pub stdout_truncated: bool,
    /// The repetition wrote bytes that are not valid UTF-8 and they were
    /// replaced.
    ///
    /// These logs come from a benchmark the PROJECT wrote, so they can contain
    /// any byte, and they are published declaring a UTF-8 payload format. The
    /// server guarantees that declaration rather than assuming it, and this
    /// field is how a reader learns it had to intervene. Replacing rather than
    /// refusing is deliberate: the logs exist to diagnose a failed run, and one
    /// stray byte is exactly when the surrounding text matters.
    pub stdout_replaced: bool,
    pub stderr_bytes: u64,
    pub stderr_truncated: bool,
    /// The repetition wrote bytes that are not valid UTF-8 on `stderr` and they
    /// were replaced. See `stdout_replaced`.
    pub stderr_replaced: bool,
    /// The server's own per-stream, per-repetition ceiling, in bytes.
    ///
    /// Published so a reader can tell a stream that happened to be short from
    /// one the server cut, without knowing the build's constants (ADR-080 §3).
    pub retained_ceiling_bytes: u64,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub selection: Selection,
    pub harness: Harness,
    pub exit: Exit,
    pub exit_code: Option<i32>,
    pub termination: ExecutionTermination,
    /// The 1-based repetition whose `exit`, `exit_code` and `termination` the
    /// three fields above describe: the last repetition that ran.
    ///
    /// It is not necessarily the repetition whose tree `criterion_archive`
    /// retained. The retained tree is the last repetition that *exported* one,
    /// which an earlier repetition can be, so both indices are published and a
    /// reader compares them instead of assuming they agree.
    #[schemars(range(min = 1, max = 3))]
    pub exit_run_index: u8,
    pub runs_requested: u8,
    pub runs_completed: u8,
    pub dataset_published: bool,
    pub dataset_omission: Option<DatasetOmission>,
    #[schemars(length(max = 128))]
    pub benchmarks: Vec<BenchmarkSummary>,
    pub benchmarks_omitted: u32,
    pub provenance: Option<Provenance>,
    pub runtime: RuntimeIdentity,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub execution_fingerprint: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub vendor_fingerprint: String,
    /// Harness logs are evidence, not a measurement, so they never travel in
    /// this response and never reach the server's stdout, which is the protocol
    /// transport. Each repetition's `stdout` and `stderr` are published as
    /// their own private `harness_stdout` / `harness_stderr` artifacts, listed
    /// in `artifacts` with the `run_index` they belong to; this row says how
    /// much of each was kept.
    #[schemars(length(max = 3))]
    pub logs: Vec<HarnessLog>,
    /// True when ANY repetition's stream was cut at the ceiling. `logs` says
    /// which; these two summarize it for a reader that only needs to know
    /// whether some log is a prefix.
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// False whenever this response describes less than the run produced.
    pub complete: bool,
}
