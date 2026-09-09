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
/// computed, and reported in full, sorted and deduplicated. Every tag here can
/// be read against the two `provenance` records the same response carries.
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
    Configuration,
    Architecture,
    CpuModel,
    CpuCores,
    OsKernel,
    CpuGovernor,
    Virtualization,
    Quotas,
    Selection,
    SamplingMode,
    /// A descriptor the method requires was observed on exactly one side.
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
    /// One side pooled fewer than the three independent executions the frozen
    /// protocol runs by default. Below three the cluster bootstrap's standard
    /// error is understated by `sqrt(k / (k - 1))` -- 1.41x at two executions,
    /// and at one there is no estimate of between-execution drift at all -- so
    /// no direction is claimed. Both sides' counts are published per comparison.
    InsufficientExecutions,
    DegenerateDispersion,
    /// The family is larger than `max_resolvable_family_size`, so the
    /// multiplicity-adjusted interval endpoints would be extreme order
    /// statistics of the bootstrap distribution. No interval is claimed.
    FamilyBeyondResolution,
    /// A descriptor the method requires was observed on neither side, so a
    /// difference in it cannot be excluded and no direction is claimed.
    UnobservableHardware,
    /// The comparison method has not passed the statistical requalification
    /// ADR-081 requires, so no direction and no `no_material_change` is claimed
    /// from any interval it produces, however clean the samples are. Every other
    /// reason here describes the caller's data; this one describes this product.
    /// The two measurements, the interval and the minimum detectable ratio are
    /// still reported.
    MethodUnqualified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SampleUnit {
    Nanoseconds,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Harness {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SamplingMode {
    Linear,
    Flat,
    Auto,
    Unknown,
}

#[derive(Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Feature(
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]{1,64}$"))] pub String,
);

/// Resource ceilings the measuring runtime was placed under. `null` is UNKNOWN
/// and never means "unlimited".
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceQuotas {
    pub cpu_quota_millicores: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub pids: Option<u32>,
}

/// What was observable about each measuring host. An absent field is UNKNOWN
/// and is never replaced by a plausible default; absent on ONE side is
/// `unknown_hardware` and absent on BOTH is `unobservable_hardware`.
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

/// The closed build/target selection each run executed under, compared
/// verbatim, so it is published verbatim.
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

/// Exactly the provenance the compatibility check reads, for one side.
///
/// It is a projection and not the whole record: `source_fingerprint`,
/// `declared_toolchain`, `run_index`, `run_count` and `captured_at_unix` are
/// not consulted by the check, and this tool does not publish a field the check
/// ignores. Nothing here is a path, a source or a name from the project tree.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    #[schemars(length(min = 1, max = 512))]
    pub format: String,
    pub format_version: u8,
    pub unit: SampleUnit,
    pub harness: Harness,
    #[schemars(length(min = 1, max = 128))]
    pub harness_version: String,
    #[schemars(length(min = 1, max = 128))]
    pub rust_version: String,
    #[schemars(length(min = 1, max = 128))]
    pub cargo_version: String,
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
}

/// One benchmark's identity as the harness named it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    #[schemars(length(min = 1, max = 512))]
    pub group_id: String,
    #[schemars(length(max = 512))]
    pub function_id: Option<String>,
    #[schemars(length(max = 512))]
    pub value_str: Option<String>,
    #[schemars(length(min = 1, max = 512))]
    pub full_id: String,
    #[schemars(length(min = 1, max = 512))]
    pub directory_name: String,
    pub sampling_mode: SamplingMode,
}

/// One benchmark key whose two measurements disagree on identity or sampling
/// mode. Those two reasons are properties of a single benchmark and cannot be
/// read off the two provenance records, so both observed values travel here.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Disagreement {
    #[schemars(length(min = 1, max = 512))]
    pub key: String,
    pub baseline: Identity,
    pub candidate: Identity,
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
    /// The NOMINAL two-sided level the method targets, not the coverage the
    /// interval delivers. The cluster bootstrap's standard error over `k`
    /// executions is understated by `sqrt(k / (k - 1))` -- 1.22x at the three
    /// executions a direction requires -- and the percentile endpoints carry no
    /// `t_{k-1}` widening for it. Both push the same way: the interval is
    /// narrower, that is more confident, than this level warrants, never wider.
    /// Measured under one Gaussian random-effects null, delivered coverage was
    /// 0.84-0.89 with three executions per side; the magnitude depends on the
    /// drift model, the mechanism does not (ADR-073 section 4).
    pub confidence_level: f64,
    pub material_threshold_ratio: f64,
    pub multiplicity: Multiplicity,
    pub family_size: u32,
    pub adjusted_confidence_level: f64,
    /// The largest family this resample budget resolves an interval for. A
    /// `family_size` above it withholds every verdict with
    /// `family_beyond_resolution`, because the multiplicity-adjusted endpoints
    /// would be extreme order statistics of the bootstrap distribution rather
    /// than points inside it.
    pub max_resolvable_family_size: u32,
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
    /// Distinct executions each side's samples were pooled from. Below the
    /// three the protocol runs by default no direction is claimed, so two
    /// otherwise identical reports differing only here were not entitled to
    /// the same verdicts.
    pub baseline_executions: u32,
    pub candidate_executions: u32,
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
    #[schemars(length(max = 24))]
    pub incompatibility_reasons: Vec<IncompatibilityReason>,
    /// What the compatibility check read on each side, published whether or not
    /// it refused. Every dataset-level reason above names a field of these two
    /// records, so a reader sees the two values behind the tag.
    pub baseline_provenance: Provenance,
    pub candidate_provenance: Provenance,
    /// The keys behind a `benchmark_identity` or `sampling_mode` reason, with
    /// both observed identities. Empty for every other reason.
    #[schemars(length(max = 64))]
    pub disagreements: Vec<Disagreement>,
    pub disagreements_omitted: u32,
    /// False whenever this response describes less than the comparison produced.
    pub complete: bool,
}
