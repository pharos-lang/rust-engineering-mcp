//! The frozen comparison method for two benchmark datasets.
//!
//! Everything here is a pure function of the two datasets: no I/O, no clock, no
//! entropy from the operating system. The same two datasets always produce the
//! same report, on any host, in any order.
//!
//! The method is frozen before anything is measured (docs/roadmap/m5-performance.md,
//! "Método congelado antes de medir"): statistic, resample count, seed,
//! confidence level, material threshold, multiplicity correction and outlier
//! policy are constants of this module and are reported alongside every result,
//! so a reader can see the method that produced a verdict instead of trusting it.
//!
//! A verdict describes the two measurements and nothing else. It states what the
//! samples show under this method on this host; it never attributes the
//! difference to any change, and it never generalizes to another host, another
//! project or another workload.

use crate::benchmark::{
    BENCHMARK_DATASET_FORMAT, BENCHMARK_DATASET_FORMAT_VERSION, BenchmarkDataset, BenchmarkError,
    BenchmarkMeasurement, MeasurementCompleteness, RawSample,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::{error::Error, fmt};

/// Wire identity of the comparison method. A report that carries a different
/// value was produced by a different method and its numbers are not comparable
/// with these.
pub const COMPARISON_METHOD: &str = "rust-engineering-mcp.benchmark-comparison.v1";

/// Bootstrap resamples per benchmark. Ten thousand keeps the Monte-Carlo error
/// of a 95% percentile interval well below the reporting precision while
/// staying inside the compare budget for a family of benchmarks.
pub const BOOTSTRAP_RESAMPLES: u32 = 10_000;

/// Fixed root seed. The resampling draw is pseudo-random but not random: fixing
/// the seed makes every interval in this module exactly reproducible from the
/// two datasets alone, which is what makes a reported verdict auditable.
pub const BOOTSTRAP_SEED: u64 = 0x0005_EEDB_0075_7241u64;

/// Nominal two-sided confidence level before any multiplicity correction.
pub const CONFIDENCE_LEVEL: f64 = 0.95;

/// Smallest ratio the product is willing to call a material difference. Frozen
/// at 5% before measuring, per the roadmap; it is a policy threshold, not an
/// estimate of what any particular run can resolve.
pub const MATERIAL_THRESHOLD_RATIO: f64 = 0.05;

/// Below this many samples on either side, no interval is claimed at all. Ten
/// is the floor for a percentile bootstrap of a median to mean anything; the
/// fixture protocol asks for thirty.
pub const MIN_SAMPLES_FOR_INFERENCE: usize = 10;

/// Power used when reporting the minimum detectable ratio. Conventional 80%.
pub const DETECTION_POWER: f64 = 0.80;

/// Standard normal quantile at 0.975, the two-sided 95% critical value.
pub const Z_TWO_SIDED_95: f64 = 1.959_963_984_540_054;

/// Standard normal quantile at 0.80, the one-sided critical value for 80% power.
pub const Z_POWER_80: f64 = 0.841_621_233_572_914;

/// The single statistic this method compares. Closed: a report never silently
/// switches to a mean, a minimum or a throughput.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonStatistic {
    MedianPerIterationNanoseconds,
}

/// What the two measurements show under this method.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonVerdict {
    /// The candidate measurement is materially slower than the baseline one.
    Regression,
    /// The candidate measurement is materially faster than the baseline one.
    Improvement,
    /// The interval excludes a material difference in either direction.
    NoMaterialChange,
    /// No verdict is claimed; see the reported reasons.
    Inconclusive,
}

/// Why two datasets may not be compared at all. Checked before any statistic is
/// computed. Sorted and deduplicated in the reported list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncompatibilityReason {
    /// A dataset format or format version that is not the v1 contract, or two
    /// different ones.
    FormatVersion,
    Unit,
    Harness,
    HarnessVersion,
    /// The same benchmark key carries a different identity on the two sides.
    BenchmarkIdentity,
    RustVersion,
    CargoVersion,
    /// The runtime image digest differs.
    RuntimeImage,
    Platform,
    Architecture,
    CpuModel,
    Quotas,
    Selection,
    SamplingMode,
    /// A hardware descriptor the method requires is UNKNOWN on at least one
    /// side. Unknown stays unknown and blocks the comparison.
    UnknownHardware,
    /// Both sides are the same execution, not two observations of it.
    SameArtifact,
}

/// Why a verdict was withheld for one benchmark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InconclusiveReason {
    InsufficientSamples,
    /// The run cannot discriminate the material threshold: the minimum
    /// detectable ratio is larger than the threshold itself.
    PrecisionBelowThreshold,
    IntervalSpansThreshold,
    ZeroOrNegativeBaseline,
    MissingMeasurement,
    TruncatedMeasurement,
}

/// Multiplicity correction applied to the family of benchmarks in one report.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MultiplicityCorrection {
    /// A family of one benchmark needs no correction.
    None,
    /// Applied to every family larger than one.
    Bonferroni,
}

/// What the method does with outliers. Closed and singular: outliers are
/// counted with Tukey fences and reported, and are never removed from the
/// sample set the statistic is computed over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutlierPolicy {
    ReportedNotRemoved,
}

/// The frozen method, carried in every report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ComparisonMethod {
    method: String,
    statistic: ComparisonStatistic,
    bootstrap_resamples: u32,
    seed: u64,
    confidence_level: f64,
    material_threshold_ratio: f64,
    multiplicity: MultiplicityCorrection,
    family_size: u32,
    adjusted_confidence_level: f64,
    outlier_policy: OutlierPolicy,
}

impl ComparisonMethod {
    /// The only producer. `family_size` is the number of benchmarks compared in
    /// one report; a family larger than one is Bonferroni corrected so the
    /// report's overall error rate stays at `1 - CONFIDENCE_LEVEL`.
    pub fn frozen(family_size: u32) -> Self {
        let family_size = family_size.max(1);
        let multiplicity = if family_size > 1 {
            MultiplicityCorrection::Bonferroni
        } else {
            MultiplicityCorrection::None
        };
        Self {
            method: COMPARISON_METHOD.to_owned(),
            statistic: ComparisonStatistic::MedianPerIterationNanoseconds,
            bootstrap_resamples: BOOTSTRAP_RESAMPLES,
            seed: BOOTSTRAP_SEED,
            confidence_level: CONFIDENCE_LEVEL,
            material_threshold_ratio: MATERIAL_THRESHOLD_RATIO,
            multiplicity,
            family_size,
            adjusted_confidence_level: 1.0 - (1.0 - CONFIDENCE_LEVEL) / f64::from(family_size),
            outlier_policy: OutlierPolicy::ReportedNotRemoved,
        }
    }

    /// Two-sided alpha after the multiplicity correction.
    pub fn adjusted_alpha(&self) -> f64 {
        1.0 - self.adjusted_confidence_level
    }

    pub fn method(&self) -> &str {
        &self.method
    }
    pub fn statistic(&self) -> ComparisonStatistic {
        self.statistic
    }
    pub fn bootstrap_resamples(&self) -> u32 {
        self.bootstrap_resamples
    }
    pub fn seed(&self) -> u64 {
        self.seed
    }
    pub fn confidence_level(&self) -> f64 {
        self.confidence_level
    }
    pub fn material_threshold_ratio(&self) -> f64 {
        self.material_threshold_ratio
    }
    pub fn multiplicity(&self) -> MultiplicityCorrection {
        self.multiplicity
    }
    pub fn family_size(&self) -> u32 {
        self.family_size
    }
    pub fn adjusted_confidence_level(&self) -> f64 {
        self.adjusted_confidence_level
    }
    pub fn outlier_policy(&self) -> OutlierPolicy {
        self.outlier_policy
    }
}

/// One benchmark compared across the two datasets.
///
/// When `verdict` is `Inconclusive` with `MissingMeasurement`, the numeric
/// fields carry zeros: there was no sample to compute them from, and a zero
/// here is an absence, not an observation of no difference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BenchmarkComparison {
    pub key: String,
    pub verdict: ComparisonVerdict,
    /// `candidate_median / baseline_median - 1.0`. Positive means the candidate
    /// measurement is the slower of the two.
    pub effect_ratio: f64,
    /// Percentile bootstrap interval for `effect_ratio` at the method's
    /// adjusted confidence level.
    pub confidence_interval: (f64, f64),
    pub baseline_median_ns: f64,
    pub candidate_median_ns: f64,
    pub baseline_samples: usize,
    pub candidate_samples: usize,
    /// Counted with Tukey fences and kept in the sample set.
    pub baseline_outliers: usize,
    pub candidate_outliers: usize,
    /// The smallest true ratio this sample size and dispersion could detect at
    /// the adjusted confidence level with 80% power. When it exceeds the
    /// material threshold, the run cannot discriminate the threshold and no
    /// verdict is claimed.
    pub minimum_detectable_ratio: f64,
    pub inconclusive_reasons: Vec<InconclusiveReason>,
}

/// The result of comparing two compatible datasets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ComparisonReport {
    pub method: ComparisonMethod,
    pub comparisons: Vec<BenchmarkComparison>,
    pub compared: usize,
    /// Benchmark keys present only in the baseline dataset, sorted.
    pub baseline_only: Vec<String>,
    /// Benchmark keys present only in the candidate dataset, sorted.
    pub candidate_only: Vec<String>,
}

/// Why no report could be produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareError {
    /// The two datasets do not describe comparable executions.
    Incompatible(Vec<IncompatibilityReason>),
    /// The datasets are compatible but share no benchmark key.
    NoCommonBenchmark,
    /// One of the datasets is not structurally valid.
    InvalidDataset(BenchmarkError),
}

impl CompareError {
    fn incompatible(mut reasons: Vec<IncompatibilityReason>) -> Self {
        reasons.sort_unstable();
        reasons.dedup();
        Self::Incompatible(reasons)
    }
}

impl fmt::Display for CompareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incompatible(reasons) => {
                write!(f, "datasets are incompatible ({} reasons)", reasons.len())
            }
            Self::NoCommonBenchmark => f.write_str("datasets share no benchmark key"),
            Self::InvalidDataset(error) => write!(f, "invalid dataset: {error}"),
        }
    }
}
impl Error for CompareError {}

// ---------------------------------------------------------------------------
// Deterministic pseudo-random source
// ---------------------------------------------------------------------------

/// SplitMix64, written out here so the resampling draw depends on nothing
/// outside this file. It is a fixed arithmetic sequence, not entropy.
#[derive(Clone, Debug)]
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    const GAMMA: u64 = 0x9E37_79B9_7F4A_7C15;

    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Root seed mixed with the benchmark key, so each benchmark draws its own
    /// reproducible stream and the stream does not depend on the position of
    /// the benchmark in the report.
    fn for_benchmark(key: &str) -> Self {
        // FNV-1a over the key bytes, then folded into the fixed root seed.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in key.as_bytes() {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        Self::new(BOOTSTRAP_SEED ^ hash)
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(Self::GAMMA);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Index in `0..bound` by multiply-shift. The residual bias is below
    /// `bound / 2^64` and is irrelevant next to the resampling variance; the
    /// method is chosen because it is branch-free and therefore reproducible.
    fn index(&mut self, bound: usize) -> usize {
        if bound == 0 {
            return 0;
        }
        let product = u128::from(self.next_u64()) * bound as u128;
        (product >> 64) as usize
    }

    /// Uniform in `[0, 1)`, from the top 53 bits.
    #[cfg(test)]
    fn next_unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[-1, 1)`.
    #[cfg(test)]
    fn next_symmetric(&mut self) -> f64 {
        self.next_unit().mul_add(2.0, -1.0)
    }
}

// ---------------------------------------------------------------------------
// Small statistics, all deterministic
// ---------------------------------------------------------------------------

/// Inverse of the standard normal CDF (Acklam's rational approximation).
///
/// Returns `None` outside the open interval `(0, 1)`; the method never panics
/// on a degenerate probability. The approximation is accurate to roughly
/// 1.2e-9, far inside the four decimals the method reports.
pub fn inverse_standard_normal_cdf(p: f64) -> Option<f64> {
    if !p.is_finite() || p <= 0.0 || p >= 1.0 {
        return None;
    }
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_69e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239e0,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838e0,
        -2.549_732_539_343_734e0,
        4.374_664_141_464_968e0,
        2.938_163_982_698_783e0,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996e0,
        3.754_408_661_907_416e0,
    ];
    const P_LOW: f64 = 0.024_25;

    let tail = |q: f64| {
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    };
    let value = if p < P_LOW {
        tail((-2.0 * p.ln()).sqrt())
    } else if p > 1.0 - P_LOW {
        -tail((-2.0 * (1.0 - p).ln()).sqrt())
    } else {
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    };
    Some(value)
}

fn sorted_copy(values: &[f64]) -> Vec<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable_by(f64::total_cmp);
    sorted
}

/// Median of an already sorted slice. Zero for an empty slice, which every
/// caller treats as "no statistic", never as a measured zero.
fn median_sorted(sorted: &[f64]) -> f64 {
    let count = sorted.len();
    if count == 0 {
        return 0.0;
    }
    let middle = count / 2;
    if count.is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

/// Linearly interpolated quantile of an already sorted slice (the common
/// "type 7" definition), fixed here so intervals are reproducible.
fn quantile_sorted(sorted: &[f64], q: f64) -> f64 {
    let count = sorted.len();
    if count == 0 {
        return 0.0;
    }
    let position = (count - 1) as f64 * q.clamp(0.0, 1.0);
    let lower = position.floor() as usize;
    let upper = (lower + 1).min(count - 1);
    let fraction = position - lower as f64;
    sorted[lower] + fraction * (sorted[upper] - sorted[lower])
}

/// Count of values outside the Tukey fences `Q1 - 1.5*IQR` and `Q3 + 1.5*IQR`.
/// Counting only: the values stay in the sample set the statistic uses.
fn tukey_outliers(sorted: &[f64]) -> usize {
    if sorted.len() < 4 {
        return 0;
    }
    let first = quantile_sorted(sorted, 0.25);
    let third = quantile_sorted(sorted, 0.75);
    let spread = third - first;
    let low = first - 1.5 * spread;
    let high = third + 1.5 * spread;
    sorted
        .iter()
        .filter(|value| **value < low || **value > high)
        .count()
}

fn standard_deviation(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let variance = values
        .iter()
        .map(|value| {
            let deviation = value - mean;
            deviation * deviation
        })
        .sum::<f64>()
        / (count - 1.0);
    variance.sqrt()
}

struct BootstrapOutcome {
    interval: (f64, f64),
    standard_error: f64,
}

/// Paired percentile bootstrap of the ratio of medians. Each group is resampled
/// independently with replacement; both medians are recomputed on every
/// resample and the recorded value is `candidate/baseline - 1`.
fn bootstrap_ratio(key: &str, baseline: &[f64], candidate: &[f64], alpha: f64) -> BootstrapOutcome {
    let mut rng = SplitMix64::for_benchmark(key);
    let mut ratios = Vec::with_capacity(BOOTSTRAP_RESAMPLES as usize);
    let mut baseline_draw = vec![0.0f64; baseline.len()];
    let mut candidate_draw = vec![0.0f64; candidate.len()];
    for _ in 0..BOOTSTRAP_RESAMPLES {
        for slot in &mut baseline_draw {
            *slot = baseline[rng.index(baseline.len())];
        }
        baseline_draw.sort_unstable_by(f64::total_cmp);
        let baseline_median = median_sorted(&baseline_draw);

        for slot in &mut candidate_draw {
            *slot = candidate[rng.index(candidate.len())];
        }
        candidate_draw.sort_unstable_by(f64::total_cmp);
        let candidate_median = median_sorted(&candidate_draw);

        ratios.push(if baseline_median > 0.0 {
            candidate_median / baseline_median - 1.0
        } else {
            0.0
        });
    }
    let standard_error = standard_deviation(&ratios);
    ratios.sort_unstable_by(f64::total_cmp);
    BootstrapOutcome {
        interval: (
            quantile_sorted(&ratios, alpha / 2.0),
            quantile_sorted(&ratios, 1.0 - alpha / 2.0),
        ),
        standard_error,
    }
}

// ---------------------------------------------------------------------------
// Compatibility
// ---------------------------------------------------------------------------

/// Dataset-level compatibility. `source_fingerprint` is deliberately absent:
/// the two sides are expected to be different code, and that difference is the
/// subject of the comparison.
///
/// The blocking set is frozen at v1. `cpu_cores`, `os_kernel`, `cpu_governor`,
/// `virtualization`, `declared_toolchain` and `configuration_fingerprint` are
/// carried in provenance for the reader but are not part of it; widening the
/// set is a method version change, not a silent tightening.
fn dataset_compatibility(
    baseline: &BenchmarkDataset,
    candidate: &BenchmarkDataset,
    reasons: &mut Vec<IncompatibilityReason>,
) {
    if baseline.format() != candidate.format()
        || baseline.format() != BENCHMARK_DATASET_FORMAT
        || baseline.format_version() != candidate.format_version()
        || baseline.format_version() != BENCHMARK_DATASET_FORMAT_VERSION
    {
        reasons.push(IncompatibilityReason::FormatVersion);
    }
    if baseline.unit() != candidate.unit() {
        reasons.push(IncompatibilityReason::Unit);
    }
    let (left, right) = (baseline.provenance(), candidate.provenance());
    if left.harness != right.harness {
        reasons.push(IncompatibilityReason::Harness);
    }
    if left.harness_version != right.harness_version {
        reasons.push(IncompatibilityReason::HarnessVersion);
    }
    if left.rust_version != right.rust_version {
        reasons.push(IncompatibilityReason::RustVersion);
    }
    if left.cargo_version != right.cargo_version {
        reasons.push(IncompatibilityReason::CargoVersion);
    }
    if left.image_digest != right.image_digest {
        reasons.push(IncompatibilityReason::RuntimeImage);
    }
    if left.platform != right.platform {
        reasons.push(IncompatibilityReason::Platform);
    }
    if left.selection != right.selection {
        reasons.push(IncompatibilityReason::Selection);
    }
    if left.hardware.arch != right.hardware.arch {
        reasons.push(IncompatibilityReason::Architecture);
    }
    match (&left.hardware.cpu_model, &right.hardware.cpu_model) {
        (Some(one), Some(other)) if one == other => {}
        (Some(_), Some(_)) => reasons.push(IncompatibilityReason::CpuModel),
        _ => reasons.push(IncompatibilityReason::UnknownHardware),
    }
    if left.hardware.quotas != right.hardware.quotas {
        reasons.push(IncompatibilityReason::Quotas);
    }
    if left.execution_fingerprint == right.execution_fingerprint {
        reasons.push(IncompatibilityReason::SameArtifact);
    }
}

/// Per-benchmark compatibility for the keys the two datasets share.
fn measurement_compatibility(
    baseline: &BenchmarkMeasurement,
    candidate: &BenchmarkMeasurement,
    reasons: &mut Vec<IncompatibilityReason>,
) {
    if baseline.identity() != candidate.identity() {
        reasons.push(IncompatibilityReason::BenchmarkIdentity);
    }
    if baseline.sampling_mode() != candidate.sampling_mode() {
        reasons.push(IncompatibilityReason::SamplingMode);
    }
}

// ---------------------------------------------------------------------------
// Verdict
// ---------------------------------------------------------------------------

struct VerdictInput {
    baseline_completeness: MeasurementCompleteness,
    candidate_completeness: MeasurementCompleteness,
    baseline_samples: usize,
    candidate_samples: usize,
    baseline_median_ns: f64,
    interval: (f64, f64),
    minimum_detectable_ratio: f64,
}

/// The frozen decision rule, in order. It reads the interval and the reported
/// precision only; it describes the two measurements and attributes nothing.
fn decide(input: &VerdictInput) -> (ComparisonVerdict, Vec<InconclusiveReason>) {
    use MeasurementCompleteness as Completeness;

    if input.baseline_completeness == Completeness::Missing
        || input.candidate_completeness == Completeness::Missing
    {
        return (
            ComparisonVerdict::Inconclusive,
            vec![InconclusiveReason::MissingMeasurement],
        );
    }

    let mut reasons = Vec::new();
    // A truncated sample set may still be summarized, but the summary describes
    // a prefix of the run, so no verdict is claimed from it.
    let truncated = input.baseline_completeness == Completeness::Truncated
        || input.candidate_completeness == Completeness::Truncated;
    if truncated {
        reasons.push(InconclusiveReason::TruncatedMeasurement);
    }

    if input.baseline_samples < MIN_SAMPLES_FOR_INFERENCE
        || input.candidate_samples < MIN_SAMPLES_FOR_INFERENCE
    {
        reasons.push(InconclusiveReason::InsufficientSamples);
        return (ComparisonVerdict::Inconclusive, reasons);
    }
    if input.baseline_median_ns <= 0.0 {
        reasons.push(InconclusiveReason::ZeroOrNegativeBaseline);
        return (ComparisonVerdict::Inconclusive, reasons);
    }
    if input.minimum_detectable_ratio > MATERIAL_THRESHOLD_RATIO {
        reasons.push(InconclusiveReason::PrecisionBelowThreshold);
        return (ComparisonVerdict::Inconclusive, reasons);
    }

    let (low, high) = input.interval;
    let verdict = if low > MATERIAL_THRESHOLD_RATIO {
        ComparisonVerdict::Regression
    } else if high < -MATERIAL_THRESHOLD_RATIO {
        ComparisonVerdict::Improvement
    } else if low > -MATERIAL_THRESHOLD_RATIO && high < MATERIAL_THRESHOLD_RATIO {
        ComparisonVerdict::NoMaterialChange
    } else {
        reasons.push(InconclusiveReason::IntervalSpansThreshold);
        ComparisonVerdict::Inconclusive
    };
    if truncated {
        return (ComparisonVerdict::Inconclusive, reasons);
    }
    (verdict, reasons)
}

fn compare_one(
    key: &str,
    baseline: &BenchmarkMeasurement,
    candidate: &BenchmarkMeasurement,
    alpha: f64,
    z_two_sided: f64,
) -> BenchmarkComparison {
    let baseline_values: Vec<f64> = baseline
        .samples()
        .iter()
        .map(RawSample::per_iteration_ns)
        .collect();
    let candidate_values: Vec<f64> = candidate
        .samples()
        .iter()
        .map(RawSample::per_iteration_ns)
        .collect();
    let baseline_sorted = sorted_copy(&baseline_values);
    let candidate_sorted = sorted_copy(&candidate_values);
    let baseline_median = median_sorted(&baseline_sorted);
    let candidate_median = median_sorted(&candidate_sorted);

    let usable =
        !baseline_values.is_empty() && !candidate_values.is_empty() && baseline_median > 0.0;
    let effect_ratio = if usable {
        candidate_median / baseline_median - 1.0
    } else {
        0.0
    };
    let outcome = if usable {
        bootstrap_ratio(key, &baseline_values, &candidate_values, alpha)
    } else {
        BootstrapOutcome {
            interval: (0.0, 0.0),
            standard_error: 0.0,
        }
    };
    let minimum_detectable_ratio = if usable {
        (z_two_sided + Z_POWER_80) * outcome.standard_error
    } else {
        0.0
    };

    let (verdict, inconclusive_reasons) = decide(&VerdictInput {
        baseline_completeness: baseline.completeness(),
        candidate_completeness: candidate.completeness(),
        baseline_samples: baseline_values.len(),
        candidate_samples: candidate_values.len(),
        baseline_median_ns: baseline_median,
        interval: outcome.interval,
        minimum_detectable_ratio,
    });

    BenchmarkComparison {
        key: key.to_owned(),
        verdict,
        effect_ratio,
        confidence_interval: outcome.interval,
        baseline_median_ns: baseline_median,
        candidate_median_ns: candidate_median,
        baseline_samples: baseline_values.len(),
        candidate_samples: candidate_values.len(),
        baseline_outliers: tukey_outliers(&baseline_sorted),
        candidate_outliers: tukey_outliers(&candidate_sorted),
        minimum_detectable_ratio,
        inconclusive_reasons,
    }
}

/// Compare two datasets under the frozen method.
///
/// Compatibility is decided before any statistic is computed, so an
/// incompatible pair never produces a number a reader could quote. The result
/// describes what the two sample sets show under this method; it makes no claim
/// about what produced the difference and does not generalize beyond these two
/// executions.
pub fn compare(
    baseline: &BenchmarkDataset,
    candidate: &BenchmarkDataset,
) -> Result<ComparisonReport, CompareError> {
    let mut reasons = Vec::new();
    dataset_compatibility(baseline, candidate, &mut reasons);
    if !reasons.is_empty() {
        return Err(CompareError::incompatible(reasons));
    }
    baseline.validate().map_err(CompareError::InvalidDataset)?;
    candidate.validate().map_err(CompareError::InvalidDataset)?;

    let baseline_keys: BTreeSet<&str> = baseline
        .measurements()
        .iter()
        .map(BenchmarkMeasurement::key)
        .collect();
    let candidate_keys: BTreeSet<&str> = candidate
        .measurements()
        .iter()
        .map(BenchmarkMeasurement::key)
        .collect();
    let common: Vec<&str> = baseline_keys
        .intersection(&candidate_keys)
        .copied()
        .collect();
    if common.is_empty() {
        return Err(CompareError::NoCommonBenchmark);
    }

    let mut pairs = Vec::with_capacity(common.len());
    for key in &common {
        let (Some(left), Some(right)) = (baseline.measurement(key), candidate.measurement(key))
        else {
            continue;
        };
        measurement_compatibility(left, right, &mut reasons);
        pairs.push((*key, left, right));
    }
    if !reasons.is_empty() {
        return Err(CompareError::incompatible(reasons));
    }

    let method = ComparisonMethod::frozen(u32::try_from(pairs.len()).unwrap_or(u32::MAX));
    let alpha = method.adjusted_alpha();
    let z_two_sided = inverse_standard_normal_cdf(1.0 - alpha / 2.0).unwrap_or(Z_TWO_SIDED_95);

    let comparisons = pairs
        .into_iter()
        .map(|(key, left, right)| compare_one(key, left, right, alpha, z_two_sided))
        .collect::<Vec<_>>();

    Ok(ComparisonReport {
        method,
        compared: comparisons.len(),
        comparisons,
        baseline_only: baseline_keys
            .difference(&candidate_keys)
            .map(|key| (*key).to_owned())
            .collect(),
        candidate_only: candidate_keys
            .difference(&baseline_keys)
            .map(|key| (*key).to_owned())
            .collect(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;
    use crate::benchmark::{
        APPROVED_CRITERION_VERSION, BenchmarkHarness, BenchmarkIdentity, BenchmarkProvenance,
        BenchmarkSelection, HardwareProfile, ResourceQuotas, SampleUnit, SamplingMode,
        Virtualization,
    };

    fn identity(name: &str) -> BenchmarkIdentity {
        BenchmarkIdentity::new(
            "group".to_owned(),
            Some("function".to_owned()),
            None,
            name.to_owned(),
            name.replace('/', "_"),
        )
        .unwrap()
    }

    fn measurement_with(
        name: &str,
        values: &[f64],
        completeness: MeasurementCompleteness,
    ) -> BenchmarkMeasurement {
        let samples = values
            .iter()
            .map(|value| RawSample::new(1, *value).unwrap())
            .collect();
        BenchmarkMeasurement::new(
            identity(name),
            SamplingMode::Flat,
            samples,
            3_000,
            5_000,
            u32::try_from(values.len()).unwrap_or(u32::MAX),
            completeness,
        )
        .unwrap()
    }

    fn measurement(name: &str, values: &[f64]) -> BenchmarkMeasurement {
        measurement_with(name, values, MeasurementCompleteness::Complete)
    }

    fn provenance(execution: &str) -> BenchmarkProvenance {
        BenchmarkProvenance {
            source_fingerprint: format!("sha256:{}", "a".repeat(64)),
            harness: BenchmarkHarness::Criterion,
            harness_version: APPROVED_CRITERION_VERSION.to_owned(),
            rust_version: "1.98.1".to_owned(),
            cargo_version: "1.98.1".to_owned(),
            declared_toolchain: Some("1.98.1".to_owned()),
            image_digest: format!("sha256:{}", "c".repeat(64)),
            platform: "aarch64-unknown-linux-gnu".to_owned(),
            configuration_fingerprint: format!("sha256:{}", "d".repeat(64)),
            execution_fingerprint: execution.to_owned(),
            selection: BenchmarkSelection {
                package: Some("member".to_owned()),
                bench_target: Some("throughput".to_owned()),
                features: vec!["std".to_owned()],
                all_features: false,
                no_default_features: false,
                profile: "bench".to_owned(),
            },
            hardware: HardwareProfile {
                cpu_model: Some("Neoverse-N1".to_owned()),
                cpu_cores: Some(4),
                os_kernel: Some("Linux 6.6.0".to_owned()),
                arch: "aarch64".to_owned(),
                virtualization: Virtualization::Container,
                cpu_governor: Some("performance".to_owned()),
                quotas: ResourceQuotas {
                    cpu_quota_millicores: Some(2_000),
                    memory_bytes: Some(2 << 30),
                    pids: Some(256),
                },
            },
            run_index: 1,
            run_count: 3,
            captured_at_unix: 1_757_000_000,
        }
    }

    fn dataset(execution: &str, measurements: Vec<BenchmarkMeasurement>) -> BenchmarkDataset {
        BenchmarkDataset::new(SampleUnit::Nanoseconds, measurements, provenance(execution)).unwrap()
    }

    /// Deterministic jitter around `center`, drawn from this file's own PRNG so
    /// every oracle below is byte-for-byte reproducible.
    fn jitter(seed: u64, count: usize, center: f64, spread: f64) -> Vec<f64> {
        let mut rng = SplitMix64::new(seed);
        (0..count)
            .map(|_| center * rng.next_symmetric().mul_add(spread, 1.0))
            .collect()
    }

    fn only(report: &ComparisonReport) -> &BenchmarkComparison {
        report.comparisons.first().unwrap()
    }

    // -- statistics primitives ---------------------------------------------

    #[test]
    fn inverse_normal_matches_known_quantiles() {
        for (probability, expected) in [
            (0.975, 1.959_96),
            (0.995, 2.575_83),
            (0.9, 1.281_55),
            (0.005, -2.575_83),
            (0.5, 0.0),
        ] {
            let value = inverse_standard_normal_cdf(probability).unwrap();
            assert!(
                (value - expected).abs() < 5e-5,
                "z({probability}) = {value}, expected {expected}"
            );
        }
        for outside in [0.0, 1.0, -0.1, 1.1, f64::NAN, f64::INFINITY] {
            assert!(inverse_standard_normal_cdf(outside).is_none(), "{outside}");
        }
    }

    #[test]
    fn frozen_z_constants_agree_with_the_inverse_normal() {
        let two_sided = inverse_standard_normal_cdf(1.0 - (1.0 - CONFIDENCE_LEVEL) / 2.0).unwrap();
        assert!((two_sided - Z_TWO_SIDED_95).abs() < 1e-8, "{two_sided}");
        let power = inverse_standard_normal_cdf(DETECTION_POWER).unwrap();
        assert!((power - Z_POWER_80).abs() < 1e-8, "{power}");
    }

    #[test]
    fn prng_is_reproducible_and_key_dependent() {
        let mut first = SplitMix64::new(BOOTSTRAP_SEED);
        let mut second = SplitMix64::new(BOOTSTRAP_SEED);
        let left: Vec<u64> = (0..8).map(|_| first.next_u64()).collect();
        let right: Vec<u64> = (0..8).map(|_| second.next_u64()).collect();
        assert_eq!(left, right);

        let mut other = SplitMix64::new(BOOTSTRAP_SEED ^ 1);
        let different: Vec<u64> = (0..8).map(|_| other.next_u64()).collect();
        assert_ne!(left, different);

        let mut one = SplitMix64::for_benchmark("bench/one");
        let mut two = SplitMix64::for_benchmark("bench/two");
        let mut one_again = SplitMix64::for_benchmark("bench/one");
        let stream: Vec<u64> = (0..8).map(|_| one.next_u64()).collect();
        assert_eq!(
            stream,
            (0..8).map(|_| one_again.next_u64()).collect::<Vec<_>>()
        );
        assert_ne!(stream, (0..8).map(|_| two.next_u64()).collect::<Vec<_>>());

        let mut bounded = SplitMix64::for_benchmark("bench/one");
        for _ in 0..1_000 {
            assert!(bounded.index(7) < 7);
        }
        assert_eq!(bounded.index(0), 0);
    }

    #[test]
    fn median_and_quantiles_are_hand_checkable() {
        assert_eq!(median_sorted(&[]), 0.0);
        assert_eq!(median_sorted(&[3.0]), 3.0);
        assert_eq!(median_sorted(&[1.0, 2.0, 3.0, 4.0]), 2.5);
        assert_eq!(median_sorted(&[1.0, 2.0, 3.0]), 2.0);
        let sorted = sorted_copy(&[9.0, 1.0, 5.0, 3.0, 7.0]);
        assert_eq!(sorted, vec![1.0, 3.0, 5.0, 7.0, 9.0]);
        assert_eq!(quantile_sorted(&sorted, 0.0), 1.0);
        assert_eq!(quantile_sorted(&sorted, 0.5), 5.0);
        assert_eq!(quantile_sorted(&sorted, 1.0), 9.0);
        assert_eq!(quantile_sorted(&sorted, 0.25), 3.0);
        assert_eq!(standard_deviation(&[2.0]), 0.0);
        assert!(
            (standard_deviation(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]) - 2.13809).abs() < 1e-4
        );
    }

    #[test]
    fn tukey_fences_count_outliers_without_removing_them() {
        // n = 10, Q1 = 3.25, Q3 = 7.75, IQR = 4.5, fences = [-3.5, 14.5].
        let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 100.0];
        let sorted = sorted_copy(&values);
        assert_eq!(quantile_sorted(&sorted, 0.25), 3.25);
        assert_eq!(quantile_sorted(&sorted, 0.75), 7.75);
        assert_eq!(tukey_outliers(&sorted), 1);
        // The outlier is still part of the set the statistic is computed over.
        assert_eq!(sorted.len(), 10);
        assert_eq!(median_sorted(&sorted), 5.5);
        assert_eq!(tukey_outliers(&[1.0, 2.0, 3.0]), 0);
    }

    #[test]
    fn outliers_are_reported_and_kept_in_the_compared_sample_set() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 100.0];
        let report = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &values)]),
            &dataset("run-candidate", vec![measurement("bench/one", &values)]),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.baseline_outliers, 1);
        assert_eq!(comparison.candidate_outliers, 1);
        assert_eq!(comparison.baseline_samples, 10);
        assert_eq!(comparison.candidate_samples, 10);
        assert_eq!(comparison.baseline_median_ns, 5.5);
        assert_eq!(comparison.candidate_median_ns, 5.5);
    }

    // -- oracles ------------------------------------------------------------

    #[test]
    fn a_known_twenty_percent_ratio_reads_as_a_regression() {
        let baseline = jitter(1, 60, 1_000.0, 0.02);
        let candidate = jitter(2, 60, 1_200.0, 0.02);
        let report = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &baseline)]),
            &dataset("run-candidate", vec![measurement("bench/one", &candidate)]),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.verdict, ComparisonVerdict::Regression);
        assert!(
            (comparison.effect_ratio - 0.20).abs() < 0.03,
            "{}",
            comparison.effect_ratio
        );
        let (low, high) = comparison.confidence_interval;
        assert!(low > MATERIAL_THRESHOLD_RATIO, "{low}");
        assert!(low <= comparison.effect_ratio && comparison.effect_ratio <= high);
        assert!(
            comparison.minimum_detectable_ratio < MATERIAL_THRESHOLD_RATIO,
            "{}",
            comparison.minimum_detectable_ratio
        );
        assert!(comparison.inconclusive_reasons.is_empty());
    }

    #[test]
    fn the_symmetric_case_reads_as_an_improvement() {
        let baseline = jitter(1, 60, 1_000.0, 0.02);
        let candidate = jitter(2, 60, 800.0, 0.02);
        let report = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &baseline)]),
            &dataset("run-candidate", vec![measurement("bench/one", &candidate)]),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.verdict, ComparisonVerdict::Improvement);
        assert!(
            (comparison.effect_ratio + 0.20).abs() < 0.03,
            "{}",
            comparison.effect_ratio
        );
        assert!(comparison.confidence_interval.1 < -MATERIAL_THRESHOLD_RATIO);
    }

    #[test]
    fn two_independent_runs_of_the_same_distribution_show_no_material_change() {
        let baseline = jitter(11, 60, 1_000.0, 0.02);
        let candidate = jitter(22, 60, 1_000.0, 0.02);
        let report = compare(
            &dataset("run-control-a", vec![measurement("bench/one", &baseline)]),
            &dataset("run-control-b", vec![measurement("bench/one", &candidate)]),
        )
        .unwrap();
        let comparison = only(&report);
        assert_ne!(comparison.verdict, ComparisonVerdict::Regression);
        assert_ne!(comparison.verdict, ComparisonVerdict::Improvement);
        assert_eq!(comparison.verdict, ComparisonVerdict::NoMaterialChange);
        assert!(comparison.effect_ratio.abs() < MATERIAL_THRESHOLD_RATIO);
    }

    #[test]
    fn dispersion_larger_than_the_effect_is_inconclusive() {
        let baseline = jitter(31, 12, 1_000.0, 0.80);
        let candidate = jitter(41, 12, 1_010.0, 0.80);
        let report = compare(
            &dataset("run-noisy-a", vec![measurement("bench/one", &baseline)]),
            &dataset("run-noisy-b", vec![measurement("bench/one", &candidate)]),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::PrecisionBelowThreshold]
        );
        assert!(
            comparison.minimum_detectable_ratio > MATERIAL_THRESHOLD_RATIO,
            "{}",
            comparison.minimum_detectable_ratio
        );
    }

    #[test]
    fn nine_samples_are_below_the_inference_floor() {
        let baseline = jitter(51, 9, 1_000.0, 0.01);
        let candidate = jitter(61, 9, 1_500.0, 0.01);
        let report = compare(
            &dataset("run-small-a", vec![measurement("bench/one", &baseline)]),
            &dataset("run-small-b", vec![measurement("bench/one", &candidate)]),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.baseline_samples, MIN_SAMPLES_FOR_INFERENCE - 1);
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::InsufficientSamples]
        );
    }

    #[test]
    fn a_missing_measurement_yields_no_numbers() {
        let baseline = jitter(71, 30, 1_000.0, 0.01);
        let report = compare(
            &dataset("run-missing-a", vec![measurement("bench/one", &baseline)]),
            &dataset(
                "run-missing-b",
                vec![measurement_with(
                    "bench/one",
                    &[],
                    MeasurementCompleteness::Missing,
                )],
            ),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::MissingMeasurement]
        );
        assert_eq!(comparison.candidate_samples, 0);
        assert_eq!(comparison.effect_ratio, 0.0);
        assert_eq!(comparison.confidence_interval, (0.0, 0.0));
        assert_eq!(comparison.minimum_detectable_ratio, 0.0);
    }

    #[test]
    fn a_truncated_measurement_is_summarized_but_never_concluded() {
        let baseline = jitter(81, 60, 1_000.0, 0.02);
        let candidate = jitter(91, 60, 1_200.0, 0.02);
        let report = compare(
            &dataset("run-trunc-a", vec![measurement("bench/one", &baseline)]),
            &dataset(
                "run-trunc-b",
                vec![measurement_with(
                    "bench/one",
                    &candidate,
                    MeasurementCompleteness::Truncated,
                )],
            ),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::TruncatedMeasurement]
        );
        // The statistic was still computed and reported.
        assert!((comparison.effect_ratio - 0.20).abs() < 0.03);
        assert!(comparison.confidence_interval.0 > 0.0);
    }

    #[test]
    fn the_decision_rule_covers_its_defensive_branches() {
        let base = |median: f64, interval: (f64, f64), mdr: f64| VerdictInput {
            baseline_completeness: MeasurementCompleteness::Complete,
            candidate_completeness: MeasurementCompleteness::Complete,
            baseline_samples: 30,
            candidate_samples: 30,
            baseline_median_ns: median,
            interval,
            minimum_detectable_ratio: mdr,
        };
        assert_eq!(
            decide(&base(0.0, (0.0, 0.0), 0.0)),
            (
                ComparisonVerdict::Inconclusive,
                vec![InconclusiveReason::ZeroOrNegativeBaseline]
            )
        );
        assert_eq!(
            decide(&base(-1.0, (0.0, 0.0), 0.0)).1,
            vec![InconclusiveReason::ZeroOrNegativeBaseline]
        );
        assert_eq!(
            decide(&base(1_000.0, (-0.30, 0.30), 0.01)),
            (
                ComparisonVerdict::Inconclusive,
                vec![InconclusiveReason::IntervalSpansThreshold]
            )
        );
        assert_eq!(
            decide(&base(1_000.0, (0.06, 0.30), 0.01)).0,
            ComparisonVerdict::Regression
        );
        assert_eq!(
            decide(&base(1_000.0, (-0.30, -0.06), 0.01)).0,
            ComparisonVerdict::Improvement
        );
        assert_eq!(
            decide(&base(1_000.0, (-0.01, 0.01), 0.01)).0,
            ComparisonVerdict::NoMaterialChange
        );
        // Exactly on the threshold is not "strictly outside" it.
        assert_eq!(
            decide(&base(1_000.0, (MATERIAL_THRESHOLD_RATIO, 0.30), 0.01)).0,
            ComparisonVerdict::Inconclusive
        );
        // Precision is checked before the interval is read.
        assert_eq!(
            decide(&base(1_000.0, (0.06, 0.30), 0.9)).1,
            vec![InconclusiveReason::PrecisionBelowThreshold]
        );
    }

    // -- compatibility ------------------------------------------------------

    /// One field of a provenance record, mutated in place.
    type ProvenanceMutation = fn(&mut BenchmarkProvenance);

    fn incompatibility(mutate: ProvenanceMutation) -> Vec<IncompatibilityReason> {
        let values = jitter(101, 12, 1_000.0, 0.01);
        let baseline = dataset("run-baseline", vec![measurement("bench/one", &values)]);
        let mut record = provenance("run-candidate");
        mutate(&mut record);
        let candidate = BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            vec![measurement("bench/one", &values)],
            record,
        )
        .unwrap();
        match compare(&baseline, &candidate) {
            // A compatible pair reports no reason, which fails the caller's
            // assertion with the expected reason still in the message.
            Err(CompareError::Incompatible(reasons)) => reasons,
            _ => Vec::new(),
        }
    }

    #[test]
    fn one_mutated_provenance_field_yields_exactly_one_reason() {
        let cases: Vec<(ProvenanceMutation, IncompatibilityReason)> = vec![
            (
                |record| record.harness_version = "0.7.0".to_owned(),
                IncompatibilityReason::HarnessVersion,
            ),
            (
                |record| record.rust_version = "1.97.0".to_owned(),
                IncompatibilityReason::RustVersion,
            ),
            (
                |record| record.cargo_version = "1.97.0".to_owned(),
                IncompatibilityReason::CargoVersion,
            ),
            (
                |record| record.image_digest = format!("sha256:{}", "e".repeat(64)),
                IncompatibilityReason::RuntimeImage,
            ),
            (
                |record| record.platform = "x86_64-unknown-linux-gnu".to_owned(),
                IncompatibilityReason::Platform,
            ),
            (
                |record| record.selection.all_features = true,
                IncompatibilityReason::Selection,
            ),
            (
                |record| record.hardware.arch = "x86_64".to_owned(),
                IncompatibilityReason::Architecture,
            ),
            (
                |record| record.hardware.cpu_model = Some("Skylake".to_owned()),
                IncompatibilityReason::CpuModel,
            ),
            (
                |record| record.hardware.cpu_model = None,
                IncompatibilityReason::UnknownHardware,
            ),
            (
                |record| record.hardware.quotas.pids = Some(1),
                IncompatibilityReason::Quotas,
            ),
        ];
        for (mutate, expected) in cases {
            assert_eq!(incompatibility(mutate), vec![expected]);
        }
    }

    #[test]
    fn a_differing_source_fingerprint_is_never_a_reason() {
        let values = jitter(111, 12, 1_000.0, 0.01);
        let baseline = dataset("run-baseline", vec![measurement("bench/one", &values)]);
        let mut record = provenance("run-candidate");
        record.source_fingerprint = format!("sha256:{}", "f".repeat(64));
        let candidate = BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            vec![measurement("bench/one", &values)],
            record,
        )
        .unwrap();
        assert!(compare(&baseline, &candidate).is_ok());
    }

    #[test]
    fn the_same_execution_is_one_artifact_not_two_observations() {
        let values = jitter(121, 12, 1_000.0, 0.01);
        let one = dataset("run-identical", vec![measurement("bench/one", &values)]);
        assert_eq!(
            compare(&one, &one),
            Err(CompareError::Incompatible(vec![
                IncompatibilityReason::SameArtifact
            ]))
        );
    }

    #[test]
    fn several_mutations_report_every_reason_sorted_and_deduplicated() {
        let values = jitter(131, 12, 1_000.0, 0.01);
        let baseline = dataset("run-baseline", vec![measurement("bench/one", &values)]);
        let mut record = provenance("run-baseline");
        record.hardware.arch = "x86_64".to_owned();
        record.platform = "x86_64-unknown-linux-gnu".to_owned();
        record.rust_version = "1.97.0".to_owned();
        let candidate = BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            vec![measurement("bench/one", &values)],
            record,
        )
        .unwrap();
        assert_eq!(
            compare(&baseline, &candidate),
            Err(CompareError::Incompatible(vec![
                IncompatibilityReason::RustVersion,
                IncompatibilityReason::Platform,
                IncompatibilityReason::Architecture,
                IncompatibilityReason::SameArtifact,
            ]))
        );
    }

    #[test]
    fn a_differing_identity_or_sampling_mode_under_one_key_blocks_the_comparison() {
        let values = jitter(141, 12, 1_000.0, 0.01);
        let baseline = dataset("run-baseline", vec![measurement("bench/one", &values)]);

        let renamed = BenchmarkMeasurement::new(
            BenchmarkIdentity::new(
                "other-group".to_owned(),
                Some("function".to_owned()),
                None,
                "bench/one".to_owned(),
                "bench_one".to_owned(),
            )
            .unwrap(),
            SamplingMode::Flat,
            values
                .iter()
                .map(|v| RawSample::new(1, *v).unwrap())
                .collect(),
            3_000,
            5_000,
            12,
            MeasurementCompleteness::Complete,
        )
        .unwrap();
        assert_eq!(
            compare(&baseline, &dataset("run-candidate", vec![renamed])),
            Err(CompareError::Incompatible(vec![
                IncompatibilityReason::BenchmarkIdentity
            ]))
        );

        let remoded = BenchmarkMeasurement::new(
            identity("bench/one"),
            SamplingMode::Linear,
            values
                .iter()
                .map(|v| RawSample::new(1, *v).unwrap())
                .collect(),
            3_000,
            5_000,
            12,
            MeasurementCompleteness::Complete,
        )
        .unwrap();
        assert_eq!(
            compare(&baseline, &dataset("run-candidate", vec![remoded])),
            Err(CompareError::Incompatible(vec![
                IncompatibilityReason::SamplingMode
            ]))
        );
    }

    #[test]
    fn an_unknown_dataset_format_is_incompatible_before_any_statistic()
    -> Result<(), serde_json::Error> {
        let values = jitter(151, 12, 1_000.0, 0.01);
        let baseline = dataset("run-baseline", vec![measurement("bench/one", &values)]);
        let encoded = serde_json::to_string(&dataset(
            "run-candidate",
            vec![measurement("bench/one", &values)],
        ))?;
        let future: BenchmarkDataset = serde_json::from_str(&encoded.replacen(
            r#""format_version":1"#,
            r#""format_version":2"#,
            1,
        ))?;
        assert_eq!(
            compare(&baseline, &future),
            Err(CompareError::Incompatible(vec![
                IncompatibilityReason::FormatVersion
            ]))
        );
        Ok(())
    }

    #[test]
    fn a_structurally_invalid_dataset_is_refused_before_the_statistic()
    -> Result<(), serde_json::Error> {
        let values = jitter(161, 12, 1_000.0, 0.01);
        let baseline = dataset("run-baseline", vec![measurement("bench/one", &values)]);
        let encoded = serde_json::to_string(&dataset(
            "run-candidate",
            vec![measurement("bench/one", &values)],
        ))?;
        let broken: BenchmarkDataset =
            serde_json::from_str(&encoded.replacen(r#""run_index":1"#, r#""run_index":9"#, 1))?;
        assert_eq!(
            compare(&baseline, &broken),
            Err(CompareError::InvalidDataset(
                BenchmarkError::InvalidRunIndex
            ))
        );
        Ok(())
    }

    #[test]
    fn the_unit_and_harness_guards_are_unreachable_while_both_enums_are_singular() {
        // `SampleUnit` and `BenchmarkHarness` each admit exactly one value, so
        // `Unit` and `Harness` cannot be produced today. They are the
        // fail-closed guards that become reachable the moment a second unit or
        // a second approved harness is added, and the serde surface below is
        // what keeps a foreign token from entering through deserialization.
        assert_eq!(SampleUnit::Nanoseconds, SampleUnit::Nanoseconds);
        assert_eq!(BenchmarkHarness::Criterion, BenchmarkHarness::Criterion);
        assert!(serde_json::from_str::<SampleUnit>("\"microseconds\"").is_err());
        assert!(serde_json::from_str::<BenchmarkHarness>("\"iai\"").is_err());
        assert!(IncompatibilityReason::Unit < IncompatibilityReason::Harness);
    }

    // -- report shape -------------------------------------------------------

    #[test]
    fn keys_present_on_one_side_only_are_reported_sorted() {
        let values = jitter(171, 12, 1_000.0, 0.01);
        let report = compare(
            &dataset(
                "run-baseline",
                vec![
                    measurement("bench/shared", &values),
                    measurement("bench/zeta", &values),
                    measurement("bench/alpha", &values),
                ],
            ),
            &dataset(
                "run-candidate",
                vec![
                    measurement("bench/shared", &values),
                    measurement("bench/omega", &values),
                ],
            ),
        )
        .unwrap();
        assert_eq!(report.compared, 1);
        assert_eq!(report.comparisons.len(), 1);
        assert_eq!(only(&report).key, "bench/shared");
        assert_eq!(report.baseline_only, vec!["bench/alpha", "bench/zeta"]);
        assert_eq!(report.candidate_only, vec!["bench/omega"]);
    }

    #[test]
    fn datasets_without_a_shared_key_cannot_be_compared() {
        let values = jitter(181, 12, 1_000.0, 0.01);
        assert_eq!(
            compare(
                &dataset("run-baseline", vec![measurement("bench/one", &values)]),
                &dataset("run-candidate", vec![measurement("bench/two", &values)]),
            ),
            Err(CompareError::NoCommonBenchmark)
        );
    }

    #[test]
    fn bonferroni_applies_to_every_family_larger_than_one() {
        let values = jitter(191, 12, 1_000.0, 0.01);
        let single = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &values)]),
            &dataset("run-candidate", vec![measurement("bench/one", &values)]),
        )
        .unwrap();
        assert_eq!(single.method.family_size(), 1);
        assert_eq!(single.method.multiplicity(), MultiplicityCorrection::None);
        assert!((single.method.adjusted_confidence_level() - 0.95).abs() < 1e-12);

        let names = ["a", "b", "c", "d", "e"];
        let family = compare(
            &dataset(
                "run-baseline",
                names.iter().map(|n| measurement(n, &values)).collect(),
            ),
            &dataset(
                "run-candidate",
                names.iter().map(|n| measurement(n, &values)).collect(),
            ),
        )
        .unwrap();
        assert_eq!(family.compared, 5);
        assert_eq!(family.method.family_size(), 5);
        assert_eq!(
            family.method.multiplicity(),
            MultiplicityCorrection::Bonferroni
        );
        assert!((family.method.adjusted_confidence_level() - 0.99).abs() < 1e-12);
        assert!((family.method.adjusted_alpha() - 0.01).abs() < 1e-12);
        assert_eq!(ComparisonMethod::frozen(0).family_size(), 1);
    }

    #[test]
    fn a_benchmark_result_does_not_depend_on_its_position_in_the_report() {
        let one = jitter(201, 20, 1_000.0, 0.02);
        let two = jitter(211, 20, 2_000.0, 0.02);
        let baseline = dataset(
            "run-baseline",
            vec![
                measurement("bench/one", &one),
                measurement("bench/two", &two),
            ],
        );
        let forward = dataset(
            "run-candidate",
            vec![
                measurement("bench/one", &one),
                measurement("bench/two", &two),
            ],
        );
        let reversed = dataset(
            "run-candidate",
            vec![
                measurement("bench/two", &two),
                measurement("bench/one", &one),
            ],
        );
        assert_eq!(
            compare(&baseline, &forward).unwrap(),
            compare(&baseline, &reversed).unwrap()
        );
    }

    #[test]
    fn the_report_carries_the_frozen_method_and_round_trips() -> Result<(), serde_json::Error> {
        let values = jitter(221, 12, 1_000.0, 0.01);
        let report = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &values)]),
            &dataset("run-candidate", vec![measurement("bench/one", &values)]),
        )
        .unwrap();
        assert_eq!(report.method.method(), COMPARISON_METHOD);
        assert_eq!(
            report.method.statistic(),
            ComparisonStatistic::MedianPerIterationNanoseconds
        );
        assert_eq!(report.method.bootstrap_resamples(), BOOTSTRAP_RESAMPLES);
        assert_eq!(report.method.seed(), BOOTSTRAP_SEED);
        assert_eq!(report.method.confidence_level(), CONFIDENCE_LEVEL);
        assert_eq!(
            report.method.material_threshold_ratio(),
            MATERIAL_THRESHOLD_RATIO
        );
        assert_eq!(
            report.method.outlier_policy(),
            OutlierPolicy::ReportedNotRemoved
        );

        let encoded = serde_json::to_string(&report)?;
        let decoded: ComparisonReport = serde_json::from_str(&encoded)?;
        // Every discrete field survives exactly. The floats are compared with a
        // relative tolerance rather than bitwise: this workspace's serde_json
        // is built without the `float_roundtrip` feature, so its parser is
        // accurate to about one unit in the last place. A dataset or a report
        // is therefore never treated as bit-identical across a serialization
        // boundary, and no equality in this module depends on that.
        assert_eq!(decoded.compared, report.compared);
        assert_eq!(decoded.baseline_only, report.baseline_only);
        assert_eq!(decoded.candidate_only, report.candidate_only);
        assert_eq!(decoded.method.method(), report.method.method());
        assert_eq!(decoded.method.seed(), report.method.seed());
        assert_eq!(decoded.method.family_size(), report.method.family_size());
        assert_eq!(decoded.method.multiplicity(), report.method.multiplicity());
        assert_eq!(decoded.method.statistic(), report.method.statistic());
        assert_eq!(
            decoded.method.outlier_policy(),
            report.method.outlier_policy()
        );
        let (left, right) = (only(&decoded), only(&report));
        assert_eq!(left.key, right.key);
        assert_eq!(left.verdict, right.verdict);
        assert_eq!(left.inconclusive_reasons, right.inconclusive_reasons);
        assert_eq!(left.baseline_samples, right.baseline_samples);
        assert_eq!(left.candidate_samples, right.candidate_samples);
        assert_eq!(left.baseline_outliers, right.baseline_outliers);
        assert_eq!(left.candidate_outliers, right.candidate_outliers);
        for (one, other) in [
            (left.effect_ratio, right.effect_ratio),
            (left.baseline_median_ns, right.baseline_median_ns),
            (left.candidate_median_ns, right.candidate_median_ns),
            (
                left.minimum_detectable_ratio,
                right.minimum_detectable_ratio,
            ),
            (left.confidence_interval.0, right.confidence_interval.0),
            (left.confidence_interval.1, right.confidence_interval.1),
        ] {
            assert!(
                (one - other).abs() <= 1e-12 * other.abs().max(1.0),
                "{one} != {other}"
            );
        }
        assert!(
            serde_json::from_str::<ComparisonReport>(&encoded.replacen(
                '{',
                r#"{"vendor_extension":1,"#,
                1
            ))
            .is_err()
        );
        assert_eq!(
            serde_json::to_string(&ComparisonVerdict::NoMaterialChange)?,
            "\"no_material_change\""
        );
        assert_eq!(
            serde_json::to_string(&IncompatibilityReason::UnknownHardware)?,
            "\"unknown_hardware\""
        );
        assert_eq!(
            serde_json::to_string(&InconclusiveReason::PrecisionBelowThreshold)?,
            "\"precision_below_threshold\""
        );
        Ok(())
    }

    #[test]
    fn comparing_the_same_two_datasets_twice_is_byte_identical() {
        let baseline = jitter(231, 30, 1_000.0, 0.05);
        let candidate = jitter(241, 30, 1_050.0, 0.05);
        let left = dataset("run-a", vec![measurement("bench/one", &baseline)]);
        let right = dataset("run-b", vec![measurement("bench/one", &candidate)]);
        let first = compare(&left, &right).unwrap();
        let second = compare(&left, &right).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            only(&first).confidence_interval,
            only(&second).confidence_interval
        );
    }
}
