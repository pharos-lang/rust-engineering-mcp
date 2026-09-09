//! Wire types for `rust.benchmark.compare`.
//!
//! Every enum this tool publishes is declared here and nowhere else, so a
//! variant added elsewhere cannot widen this frozen schema (ADR-076 §2).
//!
//! Nothing here is causal. The vocabulary describes what two measurements show
//! under one frozen method; it names no cause, recommends nothing and does not
//! generalize past the two executions it read (ADR-073 §6).
use schemars::JsonSchema;
use serde::Serialize;

/// The single statistic the frozen method compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Statistic {
    MedianPerIterationNanoseconds,
}

/// What the two measurements show under this method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The candidate measurement is materially slower than the baseline one.
    Regression,
    /// The candidate measurement is materially faster than the baseline one.
    Improvement,
    /// The interval excludes a material difference in either direction.
    NoMaterialChange,
    /// No verdict is claimed; the reported reasons say why.
    Inconclusive,
}

/// Multiplicity correction applied to the family of benchmarks in one report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Multiplicity {
    None,
    Bonferroni,
}

/// Outliers are counted with Tukey fences and reported; they are never removed
/// from the sample set the statistic is computed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OutlierPolicy {
    ReportedNotRemoved,
}

/// Why two datasets may not be compared at all. Checked before any statistic is
/// computed, and reported in full, sorted and deduplicated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IncompatibilityReason {
    FormatVersion,
    Unit,
    Harness,
    HarnessVersion,
    BenchmarkIdentity,
    RustVersion,
    CargoVersion,
    RuntimeImage,
    Platform,
    Architecture,
    CpuModel,
    Quotas,
    Selection,
    SamplingMode,
    UnknownHardware,
    SameArtifact,
}

/// Why a verdict was withheld for one benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InconclusiveReason {
    InsufficientSamples,
    PrecisionBelowThreshold,
    IntervalSpansThreshold,
    ZeroOrNegativeBaseline,
    MissingMeasurement,
    TruncatedMeasurement,
    SingleExecutionPerSide,
    DegenerateDispersion,
}

/// The frozen method, published with every report so a reader can reproduce it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Method {
    #[schemars(length(min = 1, max = 512))]
    pub method: String,
    pub statistic: Statistic,
    pub bootstrap_resamples: u32,
    pub seed: u64,
    pub confidence_level: f64,
    pub material_threshold_ratio: f64,
    pub multiplicity: Multiplicity,
    pub family_size: u32,
    pub adjusted_confidence_level: f64,
    pub outlier_policy: OutlierPolicy,
}

/// Percentile bootstrap interval for the effect ratio at the adjusted level.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Interval {
    pub low: f64,
    pub high: f64,
}

/// One benchmark compared across the two datasets. When the verdict is
/// `inconclusive` for a missing measurement the numeric fields carry zeros:
/// a zero here is an absence, not an observation of no difference.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    #[schemars(length(min = 1, max = 512))]
    pub key: String,
    pub verdict: Verdict,
    /// `candidate_median_ns / baseline_median_ns - 1`. A positive value means
    /// the candidate measurement is the slower of the two.
    pub effect_ratio: f64,
    pub confidence_interval: Interval,
    pub baseline_median_ns: f64,
    pub candidate_median_ns: f64,
    pub baseline_samples: u32,
    pub candidate_samples: u32,
    pub baseline_outliers: u32,
    pub candidate_outliers: u32,
    /// The smallest true ratio this sample size and dispersion could detect at
    /// the adjusted level with 80% power. It is never equated to the threshold.
    pub minimum_detectable_ratio: f64,
    #[schemars(length(max = 6))]
    pub inconclusive_reasons: Vec<InconclusiveReason>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Report {
    /// Absent only when the pair was refused before any statistic was computed.
    pub method: Option<Method>,
    #[schemars(length(max = 512))]
    pub comparisons: Vec<Comparison>,
    pub compared: u32,
    pub comparisons_omitted: u32,
    /// Benchmark keys present only in the baseline dataset, sorted.
    #[schemars(length(max = 256))]
    pub baseline_only: Vec<String>,
    pub baseline_only_omitted: u32,
    /// Benchmark keys present only in the candidate dataset, sorted.
    #[schemars(length(max = 256))]
    pub candidate_only: Vec<String>,
    pub candidate_only_omitted: u32,
    /// The complete sorted reason list when the two datasets are incompatible.
    #[schemars(length(max = 16))]
    pub incompatibility_reasons: Vec<IncompatibilityReason>,
    /// False whenever this response describes less than the comparison produced.
    pub complete: bool,
}
