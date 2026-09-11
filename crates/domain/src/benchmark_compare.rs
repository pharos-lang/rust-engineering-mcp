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
//!
//! The unit that is resampled is the EXECUTION, not the sample. Two datasets
//! may only be compared when their `execution_fingerprint`s differ, so the
//! quantity a verdict is about is how much the statistic moves BETWEEN
//! executions. Resampling the samples inside one execution estimates something
//! else — how much the statistic would move if the same execution were read
//! again — and that number is far smaller, which is what turns host drift into
//! a confident direction. See the "Corrección" note in
//! docs/adr/ADR-073-benchmark-method-and-dataset.md §4.

use crate::benchmark::{
    BENCHMARK_DATASET_FORMAT, BENCHMARK_DATASET_FORMAT_VERSION, BenchmarkDataset, BenchmarkError,
    BenchmarkHarness, BenchmarkIdentity, BenchmarkMeasurement, BenchmarkSelection, HardwareProfile,
    MeasurementCompleteness, SampleUnit, SamplingMode, Virtualization,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::{error::Error, fmt};

/// Wire identity of the comparison method. A report that carries a different
/// value was produced by a different method and its numbers are not comparable
/// with these.
///
/// v2 resamples executions and then samples within them (a cluster bootstrap).
/// v1 resampled samples inside one execution, so its intervals are narrower
/// than v2's for the same data and the two cannot be read side by side. The
/// statistic, seed, resample count, confidence level, multiplicity correction,
/// threshold and outlier policy are unchanged.
pub const COMPARISON_METHOD: &str = "rust-engineering-mcp.benchmark-comparison.v2";

/// Bootstrap resamples per benchmark. Ten thousand keeps the Monte-Carlo error
/// of a percentile interval well below the reporting precision **at the
/// UNADJUSTED 95% level**, and stays inside the compare budget for a family of
/// benchmarks.
///
/// It says nothing about the level the method actually computes. Bonferroni
/// divides alpha by the family size, so the tail probability a report of `n`
/// benchmarks asks this distribution for is `0.025/n`, and the number of
/// resampled ratios beyond an endpoint is `BOOTSTRAP_RESAMPLES * 0.025/n` —
/// ten at `n = 25`, five at `n = 50`, one at `n = 250`, where the endpoint IS
/// the smallest ratio drawn. [`MAX_RESOLVABLE_FAMILY_SIZE`] is where that
/// arithmetic stops, and the method refuses beyond it instead of quoting an
/// extreme order statistic as an interval endpoint.
pub const BOOTSTRAP_RESAMPLES: u32 = 10_000;

/// Smallest number of resampled ratios that must fall beyond an interval
/// endpoint for that endpoint to be an interpolation INSIDE the bootstrap
/// distribution rather than one of its extreme order statistics.
///
/// Ten is the count at which `quantile_sorted` interpolates between the tenth
/// and eleventh draws. Below it the endpoint is decided by two or three
/// individual draws whose Monte-Carlo fluctuation is of the same order as the
/// endpoint's distance from the extreme, and the error is not symmetric: the
/// distribution is bounded on that side, so too few tail draws pull the
/// endpoint INWARD and the interval reads narrower — that is, more confident —
/// than the level it claims.
pub const MIN_TAIL_RESAMPLES: u32 = 10;

/// The largest family of benchmarks for which this method claims a percentile
/// interval, derived from [`BOOTSTRAP_RESAMPLES`], [`CONFIDENCE_LEVEL`] and
/// [`MIN_TAIL_RESAMPLES`]: the largest `n` with
/// `BOOTSTRAP_RESAMPLES * ((1 - CONFIDENCE_LEVEL) / n) / 2 >= MIN_TAIL_RESAMPLES`.
///
/// A report of a larger family still describes both measurements — medians,
/// sample counts, outliers and the observed ratio are all reported — but claims
/// no interval and no direction, with [`InconclusiveReason::FamilyBeyondResolution`]
/// saying so. `resolvable_family_size_matches_the_resample_budget` derives this
/// number from those three constants so it cannot drift away from them.
pub const MAX_RESOLVABLE_FAMILY_SIZE: u32 = 25;

/// Fixed root seed. The resampling draw is pseudo-random but not random: fixing
/// the seed makes every interval in this module exactly reproducible from the
/// two datasets alone, which is what makes a reported verdict auditable.
pub const BOOTSTRAP_SEED: u64 = 0x0005_EEDB_0075_7241u64;

/// Nominal two-sided confidence level before any multiplicity correction.
///
/// It is the level this method TARGETS, and it is not the coverage the
/// published interval delivers. Two approximations sit between the two, both
/// consequences of resampling the handful of executions a run actually has:
///
/// - the nonparametric cluster bootstrap's variance over `k` clusters has
///   expectation `((k - 1) / k) · σ²_between`, so the standard error it reports
///   is short of the one it estimates by a factor of `sqrt(k / (k - 1))` —
///   1.22× at the `k = 3` [`MIN_EXECUTIONS_FOR_DIRECTION`] requires;
/// - the endpoints are percentiles of that same bootstrap distribution, taken
///   with no `t_{k-1}` widening for a scale estimated from `k` clusters.
///
/// Both err in the SAME direction: the interval is NARROWER than 0.95 warrants
/// — more confident, never less — so every direction this method does claim is
/// claimed at a true coverage below the level published beside it. Measured
/// under one Gaussian random-effects null, an independent review put that
/// coverage at 0.84–0.89 with three executions per side. The MAGNITUDE of the
/// gap depends on that drift model and would be a different pair of numbers
/// under another one; the MECHANISM does not depend on it and does not vanish
/// at any `k` the published `run_count` range can reach.
///
/// The constant stays 0.95 and is published as 0.95 deliberately. It is what
/// the frozen method asks the distribution for, and substituting an
/// "effective" number measured under one drift model would publish that
/// model's assumptions as if they were the method's. The disclosure is the
/// correction; see ADR-073 §4, "Corrección (2026-09-09) — la cobertura que
/// entrega el intervalo".
pub const CONFIDENCE_LEVEL: f64 = 0.95;

/// Executions each side must pool before any direction is admissible.
///
/// The outer stage of [`cluster_draw`] takes `k` executions with replacement
/// from the `k` executions that side ran. For a statistic behaving like a mean
/// over clusters, the variance of that draw has expectation
/// `((k - 1) / k) · σ²_between`, so the standard error is understated by
/// `sqrt(k / (k - 1))` and the percentile endpoints inherit the understatement
/// with no `t_{k-1}` correction applied to them. The factor is a function of
/// `k` alone — `the_cluster_shortfall_is_a_function_of_the_execution_count`
/// computes it rather than restating it — and it is worst at the smallest `k`:
/// 1.41× at `k = 2` against 1.22× at `k = 3`.
///
/// `k = 2` is the worst row this contract can reach, because `run_count` is a
/// published input over `1..=3`. It is also the row an independent review
/// measured a false direction in: under a Gaussian random-effects null with 5%
/// drift, 27 of 1000 comparisons of IDENTICAL source emitted a direction at
/// `k = 2`, with delivered coverage 0.66–0.75 there against 0.84–0.89 at
/// `k = 3`. Requiring three executions removes that row entirely rather than
/// shrinking it: below three the direction is refused as
/// [`InconclusiveReason::InsufficientExecutions`] before any interval is read.
///
/// Three is not a new demand. It is the number of independent executions the
/// frozen protocol already runs by default (`BENCHMARK_DEFAULT_RUN_COUNT`, the
/// default of that same published input), so the gate asks that the protocol
/// was followed, not that anything extra be captured. What remains at `k = 3`
/// is the 1.22× understatement, which is disclosed at [`CONFIDENCE_LEVEL`]
/// instead of being silently absorbed.
pub const MIN_EXECUTIONS_FOR_DIRECTION: usize = 3;

/// Whether this method has been qualified to turn two measurements into a
/// DIRECTION. It has not.
///
/// ADR-081 says directional verdicts and `no_material_change` stay disabled
/// until the statistical requalification passes, and that the hardware gate and
/// the statistical gate are independent: both must open. Until this constant
/// exists that sentence lives only in Markdown. In practice the only thing that
/// has been withholding directions is that this container cannot read
/// `cpu_governor`, which sets [`InconclusiveReason::UnobservableHardware`] — an
/// accident of the runtime, not a decision. On a host where the governor IS
/// observable, `decide` would start emitting `Regression`, `Improvement` and
/// `NoMaterialChange` computed by an estimator whose own simulation puts its
/// coverage at 0.84 against a published 0.95. This constant is the gate that
/// sentence describes, written down where it can actually run.
///
/// To set it to `true`, ALL of the following must have happened, and the change
/// is a method-version change ([`COMPARISON_METHOD`]) because the intervals of
/// two estimators are not comparable:
///
/// 1. an estimator meets **every** criterion of ADR-081 §1 — coverage, false
///    positives, power, incorrect `no_material_change` and budget — at **every**
///    drift point of §2, recorded in `docs/validation/M5/02-method-simulation.json`;
/// 2. the real controls of ADR-081 §3 reproduce on captures from the admitted
///    image: positives with a known effect, and negatives for noise,
///    incompatibility and insufficient data;
/// 3. the independent statistical review of ADR-081 §6 has accepted it, having
///    checked that the criteria were fixed before the numbers and not after.
///
/// A criterion that turns out unreachable does NOT license setting this to
/// `true`: ADR-081 says an unreachable criterion is declared unreachable and the
/// directional verdicts stay disabled.
pub const METHOD_QUALIFIED_FOR_DIRECTION: bool = false;

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
    /// The digest over the frozen run configuration differs, so the two runs
    /// were not measured under the same configuration.
    Configuration,
    Architecture,
    CpuModel,
    CpuCores,
    OsKernel,
    /// The two runs observed different frequency governors. It is the ambient
    /// parameter most able to manufacture a difference that is not in the code.
    CpuGovernor,
    Virtualization,
    Quotas,
    Selection,
    SamplingMode,
    /// A descriptor the method requires is UNKNOWN on EXACTLY ONE side.
    ///
    /// This is an asymmetry, not a shared blindness: one capture observed the
    /// field and the other did not, so the two were not even observed alike and
    /// nothing here can establish that they agree. Unknown stays unknown and
    /// blocks the comparison. The case where NEITHER side could observe the
    /// field is a different fact and carries a different name; see
    /// [`InconclusiveReason::UnobservableHardware`].
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
    /// One side pooled fewer than [`MIN_EXECUTIONS_FOR_DIRECTION`] executions.
    ///
    /// At one execution there is no estimate of between-execution drift at all:
    /// everything that moved between the two runs — frequency, thermal state,
    /// page cache, co-tenants, address layout — is folded into the same
    /// difference a direction would attribute to the source, and no interval
    /// computed here can exclude it however wide the observed gap is. At two
    /// there is an estimate, and it is the one this method understates most:
    /// the cluster bootstrap's standard error is short by `sqrt(k / (k - 1))`,
    /// which is 1.41× at `k = 2`. Both are the same refusal because both are
    /// the same fact about the samples — the side did not pool the executions
    /// a direction needs — and the reported count says which one it was.
    InsufficientExecutions,
    /// The two sample sets carry no dispersion to measure.
    ///
    /// When every sample is identical the bootstrap standard error is zero, so
    /// the minimum detectable ratio is zero and the precision gate can never
    /// fire: a harness that emits a constant would otherwise receive the most
    /// confident verdict this method can produce, with a zero-width interval.
    /// Zero observed dispersion is an ABSENCE of information about dispersion,
    /// not infinite precision, and this method refuses to read it as the latter.
    DegenerateDispersion,
    /// The family is larger than the resample budget can resolve.
    ///
    /// Bonferroni divides alpha by the family size, so the interval endpoints
    /// of a large family are extreme order statistics of a fixed-size bootstrap
    /// distribution — at a family of 250 the lower endpoint is the smallest of
    /// the ten thousand draws. This method does not quote such an endpoint as
    /// an interval: beyond [`MAX_RESOLVABLE_FAMILY_SIZE`] no bootstrap is run,
    /// no interval is claimed and no direction is admissible. The two
    /// measurements are still described.
    FamilyBeyondResolution,
    /// The comparison method itself is not qualified to claim a direction.
    ///
    /// Every other reason here is a fact about the CALLER's two sample sets.
    /// This one is a fact about the PRODUCT: the method that would read the
    /// interval has not passed the statistical requalification ADR-081 §1
    /// requires, so no interval it produces is allowed to become a direction,
    /// however clean the samples are. See [`METHOD_QUALIFIED_FOR_DIRECTION`].
    ///
    /// It is reported LAST among the gates on purpose. Everything wrong with
    /// the caller's own data is named first, because that is what the caller can
    /// act on; this reason appears exactly when the data would otherwise have
    /// supported a verdict, and it says that what is missing is on this side.
    /// The two measurements are still described in full, and the interval is
    /// still computed and published — what is withheld is the direction.
    MethodUnqualified,
    /// A hardware descriptor the method requires was UNOBSERVABLE on BOTH
    /// sides.
    ///
    /// Neither capture could read the field — inside this container nobody can
    /// read the frequency governor — so the two runs may have been taken under
    /// different values of it and nothing in either dataset can exclude that.
    /// That is not the same fact as a field that is known and different, nor as
    /// one observed on a single side: those two are refusals of the comparison
    /// itself ([`IncompatibilityReason::CpuGovernor`],
    /// [`IncompatibilityReason::UnknownHardware`]). A blindness identical on
    /// both sides leaves the two datasets structurally comparable and their
    /// measurements worth reporting; what it forbids is reading a direction out
    /// of them, because the ambient parameter that could have produced the
    /// difference was never observed.
    UnobservableHardware,
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
    /// The largest family this resample budget can resolve an interval for,
    /// published so a reader can see the limit next to the family it applies
    /// to instead of inferring it.
    max_resolvable_family_size: u32,
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
            max_resolvable_family_size: MAX_RESOLVABLE_FAMILY_SIZE,
            outlier_policy: OutlierPolicy::ReportedNotRemoved,
        }
    }

    /// Whether this family is small enough for the fixed resample budget to
    /// place an interval endpoint inside the bootstrap distribution.
    pub fn resolves_family(&self) -> bool {
        self.family_size <= self.max_resolvable_family_size
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
    pub fn max_resolvable_family_size(&self) -> u32 {
        self.max_resolvable_family_size
    }
    pub fn outlier_policy(&self) -> OutlierPolicy {
        self.outlier_policy
    }
}

/// Exactly the provenance the compatibility check reads, projected for one
/// side of a comparison.
///
/// It exists so a reported reason can be read against the two values that
/// produced it: a caller told `cpu_model` sees WHICH two CPUs, from this call,
/// without going back to the artifact it no longer has. The projection is
/// deliberately not the whole provenance record — `source_fingerprint`,
/// `declared_toolchain`, `run_index`, `run_count` and `captured_at_unix` are
/// not consulted by the compatibility check, and publishing a field the check
/// ignores would invite a reader to conclude something the method did not.
/// `provenance_projection_carries_every_consulted_field` holds the two sets
/// together.
///
/// How many executions each side pooled is published too, but on
/// [`BenchmarkComparison`] rather than here, and for both halves of that rule:
/// the compatibility check does not consult it, and it is a property of one
/// benchmark's samples rather than of the dataset — a measurement missing from
/// one repetition leaves that benchmark with fewer executions than its
/// neighbours in the very same pair of datasets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ComparedProvenance {
    pub format: String,
    pub format_version: u8,
    pub unit: SampleUnit,
    pub harness: BenchmarkHarness,
    pub harness_version: String,
    pub rust_version: String,
    pub cargo_version: String,
    pub image_digest: String,
    pub platform: String,
    pub configuration_fingerprint: String,
    pub execution_fingerprint: String,
    pub selection: BenchmarkSelection,
    pub hardware: HardwareProfile,
}

impl ComparedProvenance {
    /// Projects one dataset. Copies only; nothing here is derived, defaulted or
    /// normalized, so an UNKNOWN field stays absent exactly as the dataset
    /// recorded it.
    pub fn of(dataset: &BenchmarkDataset) -> Self {
        let provenance = dataset.provenance();
        Self {
            format: dataset.format().to_owned(),
            format_version: dataset.format_version(),
            unit: dataset.unit(),
            harness: provenance.harness,
            harness_version: provenance.harness_version.clone(),
            rust_version: provenance.rust_version.clone(),
            cargo_version: provenance.cargo_version.clone(),
            image_digest: provenance.image_digest.clone(),
            platform: provenance.platform.clone(),
            configuration_fingerprint: provenance.configuration_fingerprint.clone(),
            execution_fingerprint: provenance.execution_fingerprint.clone(),
            selection: provenance.selection.clone(),
            hardware: provenance.hardware.clone(),
        }
    }
}

/// One benchmark key whose two measurements disagree on something the method
/// requires to be identical.
///
/// The dataset-level reasons are readable from the two [`ComparedProvenance`]
/// records; these two are not, because they are properties of one benchmark
/// inside the dataset. Both observed values travel with the key so
/// `benchmark_identity` and `sampling_mode` are not bare tags either.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct MeasurementDisagreement {
    pub key: String,
    pub baseline_identity: BenchmarkIdentity,
    pub candidate_identity: BenchmarkIdentity,
    pub baseline_sampling_mode: SamplingMode,
    pub candidate_sampling_mode: SamplingMode,
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
    /// Distinct executions the samples above were pooled from, per side.
    ///
    /// Published beside the sample counts because it is a property of the same
    /// two sample sets and, unlike them, it decides whether a direction may be
    /// claimed at all: below [`MIN_EXECUTIONS_FOR_DIRECTION`] no interval is
    /// read. Two reports that agree on every other field can differ only here,
    /// and then one of them was allowed a verdict the other was refused, so a
    /// reader that cannot see this number cannot tell the two apart.
    pub baseline_executions: usize,
    pub candidate_executions: usize,
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
    /// What the compatibility check read on each side. Published for a
    /// compatible pair too: the two contexts are what a verdict is about, and a
    /// reader that cannot see them cannot see what the verdict describes.
    pub baseline_provenance: ComparedProvenance,
    pub candidate_provenance: ComparedProvenance,
}

/// Everything the compatibility check found, when it found something.
///
/// It carries the two provenance records as well as the reasons, so a refusal
/// shows the values it refused on. That is the whole point of reporting a
/// reason: `["cpu_model"]` alone names a field, not a difference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incompatibility {
    /// Sorted and deduplicated.
    pub reasons: Vec<IncompatibilityReason>,
    pub baseline_provenance: ComparedProvenance,
    pub candidate_provenance: ComparedProvenance,
    /// The keys behind a `BenchmarkIdentity` or `SamplingMode` reason, with
    /// both observed values. Empty for every other reason.
    pub disagreements: Vec<MeasurementDisagreement>,
}

/// Why no report could be produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareError {
    /// The two datasets do not describe comparable executions.
    Incompatible(Box<Incompatibility>),
    /// The datasets are compatible but share no benchmark key.
    NoCommonBenchmark,
    /// One of the datasets is not structurally valid.
    InvalidDataset(BenchmarkError),
}

impl CompareError {
    fn incompatible(
        mut reasons: Vec<IncompatibilityReason>,
        baseline: &BenchmarkDataset,
        candidate: &BenchmarkDataset,
        disagreements: Vec<MeasurementDisagreement>,
    ) -> Self {
        reasons.sort_unstable();
        reasons.dedup();
        Self::Incompatible(Box::new(Incompatibility {
            reasons,
            baseline_provenance: ComparedProvenance::of(baseline),
            candidate_provenance: ComparedProvenance::of(candidate),
            disagreements,
        }))
    }

    /// The reasons of an incompatibility, or an empty slice for any other
    /// error. Callers that only assert on the reasons do not have to reach
    /// through the box.
    pub fn reasons(&self) -> &[IncompatibilityReason] {
        match self {
            Self::Incompatible(details) => &details.reasons,
            _ => &[],
        }
    }
}

impl fmt::Display for CompareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incompatible(details) => {
                write!(
                    f,
                    "datasets are incompatible ({} reasons)",
                    details.reasons.len()
                )
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

/// One side's per-iteration values, kept grouped by the execution that produced
/// them. The grouping is the point: it is what the bootstrap resamples.
struct SideSamples {
    /// One vector per distinct `run_index`, ordered by that index so a draw
    /// depends on the data and never on the order samples arrived in.
    executions: Vec<Vec<f64>>,
    /// Every value of every execution, pooled. The reported median, the
    /// outlier count and the sample count are computed over this.
    values: Vec<f64>,
}

impl SideSamples {
    /// Groups one measurement's samples by the execution each came from.
    fn of(measurement: &BenchmarkMeasurement) -> Self {
        let mut grouped: BTreeMap<u8, Vec<f64>> = BTreeMap::new();
        for sample in measurement.samples() {
            grouped
                .entry(sample.run_index())
                .or_default()
                .push(sample.per_iteration_ns());
        }
        let executions: Vec<Vec<f64>> = grouped.into_values().collect();
        let values = executions.iter().flatten().copied().collect();
        Self { executions, values }
    }

    /// How many independent executions this side pools. One is not a small
    /// number of executions; it is no estimate of between-execution drift at
    /// all. Two is an estimate this method understates most
    /// ([`MIN_EXECUTIONS_FOR_DIRECTION`]).
    fn execution_count(&self) -> usize {
        self.executions.len()
    }
}

struct BootstrapOutcome {
    interval: (f64, f64),
    standard_error: f64,
    /// The ten thousand resampled ratios were all the same number, so the
    /// bootstrap distribution has no width: the standard error is zero.
    ///
    /// It is read off the distribution rather than off `standard_error > 0.0`
    /// because summing ten thousand identical values leaves a last-unit
    /// rounding residue, and a standard error of 1e-17 is zero dispersion
    /// reported as spectacular precision — exactly the reading this flag
    /// exists to prevent.
    degenerate: bool,
}

/// One two-stage draw: `k` executions taken with replacement from the `k` this
/// side ran, then, inside each execution drawn, as many samples with
/// replacement as that execution reported. Returns the median of the pooled
/// draw.
///
/// The two stages are what put between-execution variance into the interval. A
/// side that ran one execution can only ever draw that one, which is why a
/// direction is refused earlier rather than read off a draw that cannot vary
/// the way the underlying quantity does. The same outer stage is why a small
/// `k` understates the standard error by `sqrt(k / (k - 1))`, and why the
/// refusal covers every `k` below [`MIN_EXECUTIONS_FOR_DIRECTION`] rather than
/// only `k = 1`.
fn cluster_draw(rng: &mut SplitMix64, side: &SideSamples, draw: &mut Vec<f64>) -> f64 {
    draw.clear();
    let count = side.executions.len();
    for _ in 0..count {
        // `index` is always below the bound it is given, so `get` cannot be
        // `None` here; it is used anyway because a statistic never panics.
        let Some(execution) = side.executions.get(rng.index(count)) else {
            continue;
        };
        for _ in 0..execution.len() {
            if let Some(value) = execution.get(rng.index(execution.len())) {
                draw.push(*value);
            }
        }
    }
    draw.sort_unstable_by(f64::total_cmp);
    median_sorted(draw)
}

/// Paired percentile CLUSTER bootstrap of the ratio of medians. Each side is
/// resampled independently and in two stages — executions, then samples within
/// each execution drawn; both medians are recomputed on every resample and the
/// recorded value is `candidate/baseline - 1`.
///
/// Resampling only the samples, as an earlier form of this method did, holds
/// the executions fixed and so reports the precision of ONE execution's median.
/// The verdict is about a difference between executions, so that interval
/// answered a question nobody asked and answered it far too tightly.
fn bootstrap_ratio(
    key: &str,
    baseline: &SideSamples,
    candidate: &SideSamples,
    alpha: f64,
) -> BootstrapOutcome {
    let mut rng = SplitMix64::for_benchmark(key);
    let mut ratios = Vec::with_capacity(BOOTSTRAP_RESAMPLES as usize);
    let mut baseline_draw = Vec::with_capacity(baseline.values.len());
    let mut candidate_draw = Vec::with_capacity(candidate.values.len());
    for _ in 0..BOOTSTRAP_RESAMPLES {
        let baseline_median = cluster_draw(&mut rng, baseline, &mut baseline_draw);
        let candidate_median = cluster_draw(&mut rng, candidate, &mut candidate_draw);
        ratios.push(if baseline_median > 0.0 {
            candidate_median / baseline_median - 1.0
        } else {
            0.0
        });
    }
    let standard_error = standard_deviation(&ratios);
    ratios.sort_unstable_by(f64::total_cmp);
    let degenerate = match (ratios.first(), ratios.last()) {
        (Some(low), Some(high)) => low.total_cmp(high) == std::cmp::Ordering::Equal,
        // No resample at all is not a dispersion this method observed either.
        _ => true,
    };
    BootstrapOutcome {
        interval: (
            quantile_sorted(&ratios, alpha / 2.0),
            quantile_sorted(&ratios, 1.0 - alpha / 2.0),
        ),
        standard_error,
        degenerate,
    }
}

/// Whether the two sides carry fewer than two distinct per-iteration values
/// between them. A single value repeated is a sample set with no dispersion to
/// measure, whatever its size.
fn fewer_than_two_distinct_values(baseline: &[f64], candidate: &[f64]) -> bool {
    let mut first: Option<f64> = None;
    for value in baseline.iter().chain(candidate) {
        match first {
            None => first = Some(*value),
            Some(seen) if seen.total_cmp(value) != std::cmp::Ordering::Equal => return false,
            Some(_) => (),
        }
    }
    true
}

// ---------------------------------------------------------------------------
// Compatibility
// ---------------------------------------------------------------------------

/// How two observations of one descriptor relate. The last two are separated
/// because they are different facts: one side blind is an asymmetry between the
/// two captures, both sides blind is a property of the environment they share.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Agreement {
    Equal,
    Different,
    OneSideUnknown,
    NeitherObserved,
}

fn agreement<T: PartialEq>(left: Option<&T>, right: Option<&T>) -> Agreement {
    match (left, right) {
        (Some(one), Some(other)) if one == other => Agreement::Equal,
        (Some(_), Some(_)) => Agreement::Different,
        (None, None) => Agreement::NeitherObserved,
        _ => Agreement::OneSideUnknown,
    }
}

/// Records one descriptor's verdict.
///
/// A difference blocks under its own name. An asymmetric unknown blocks as
/// `UnknownHardware`: one capture saw the field and the other did not, so the
/// two were not observed alike. A field NEITHER capture could read blocks no
/// comparison — the blindness is identical on both sides and is a property of
/// the runtime image, which is itself compared and must be equal — but it does
/// set `unobservable`, and every verdict in the report is withheld for it.
fn descriptor(
    observed: Agreement,
    differs: IncompatibilityReason,
    reasons: &mut Vec<IncompatibilityReason>,
    unobservable: &mut bool,
) {
    match observed {
        Agreement::Equal => {}
        Agreement::Different => reasons.push(differs),
        Agreement::OneSideUnknown => reasons.push(IncompatibilityReason::UnknownHardware),
        Agreement::NeitherObserved => *unobservable = true,
    }
}

/// Dataset-level compatibility. `source_fingerprint` is deliberately absent:
/// the two sides are expected to be different code, and that difference is the
/// subject of the comparison. `declared_toolchain` is absent for a related
/// reason: it is what the project's own files asked for, so it belongs to the
/// source under test, and the toolchain that actually measured is
/// `rust_version`/`cargo_version`, both of which do block.
///
/// Every other field of the provenance is consulted. `configuration_fingerprint`
/// is the digest of the frozen run configuration and blocks when it differs;
/// `cpu_cores`, `os_kernel`, `cpu_governor` and `virtualization` are hardware
/// descriptors and take the three-way treatment [`descriptor`] describes.
///
/// Sets `unobservable` when at least one required descriptor was readable on
/// neither side.
fn dataset_compatibility(
    baseline: &BenchmarkDataset,
    candidate: &BenchmarkDataset,
    reasons: &mut Vec<IncompatibilityReason>,
    unobservable: &mut bool,
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
    if left.configuration_fingerprint != right.configuration_fingerprint {
        reasons.push(IncompatibilityReason::Configuration);
    }
    if left.selection != right.selection {
        reasons.push(IncompatibilityReason::Selection);
    }
    if left.hardware.arch != right.hardware.arch {
        reasons.push(IncompatibilityReason::Architecture);
    }
    descriptor(
        agreement(
            left.hardware.cpu_model.as_ref(),
            right.hardware.cpu_model.as_ref(),
        ),
        IncompatibilityReason::CpuModel,
        reasons,
        unobservable,
    );
    descriptor(
        agreement(
            left.hardware.cpu_cores.as_ref(),
            right.hardware.cpu_cores.as_ref(),
        ),
        IncompatibilityReason::CpuCores,
        reasons,
        unobservable,
    );
    descriptor(
        agreement(
            left.hardware.os_kernel.as_ref(),
            right.hardware.os_kernel.as_ref(),
        ),
        IncompatibilityReason::OsKernel,
        reasons,
        unobservable,
    );
    descriptor(
        agreement(
            left.hardware.cpu_governor.as_ref(),
            right.hardware.cpu_governor.as_ref(),
        ),
        IncompatibilityReason::CpuGovernor,
        reasons,
        unobservable,
    );
    // `Virtualization::Unknown` is this field's absent value: the enum has no
    // `Option` around it, and an environment the runtime could not classify is
    // exactly as unobserved as an absent CPU model.
    descriptor(
        agreement(
            observed_virtualization(left.hardware.virtualization).as_ref(),
            observed_virtualization(right.hardware.virtualization).as_ref(),
        ),
        IncompatibilityReason::Virtualization,
        reasons,
        unobservable,
    );
    if left.hardware.quotas != right.hardware.quotas {
        reasons.push(IncompatibilityReason::Quotas);
    }
    if left.execution_fingerprint == right.execution_fingerprint {
        reasons.push(IncompatibilityReason::SameArtifact);
    }
}

fn observed_virtualization(value: Virtualization) -> Option<Virtualization> {
    match value {
        Virtualization::Unknown => None,
        observed => Some(observed),
    }
}

/// Per-benchmark compatibility for the keys the two datasets share.
///
/// A disagreement is recorded with both observed values, not only its name:
/// these two reasons are about one benchmark inside the dataset and cannot be
/// read off the two provenance records the way every other reason can.
fn measurement_compatibility(
    key: &str,
    baseline: &BenchmarkMeasurement,
    candidate: &BenchmarkMeasurement,
    reasons: &mut Vec<IncompatibilityReason>,
    disagreements: &mut Vec<MeasurementDisagreement>,
) {
    let identity_differs = baseline.identity() != candidate.identity();
    let mode_differs = baseline.sampling_mode() != candidate.sampling_mode();
    if identity_differs {
        reasons.push(IncompatibilityReason::BenchmarkIdentity);
    }
    if mode_differs {
        reasons.push(IncompatibilityReason::SamplingMode);
    }
    if identity_differs || mode_differs {
        disagreements.push(MeasurementDisagreement {
            key: key.to_owned(),
            baseline_identity: baseline.identity().clone(),
            candidate_identity: candidate.identity().clone(),
            baseline_sampling_mode: baseline.sampling_mode(),
            candidate_sampling_mode: candidate.sampling_mode(),
        });
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
    /// Distinct executions behind each side's samples.
    baseline_executions: usize,
    candidate_executions: usize,
    baseline_median_ns: f64,
    /// The bootstrap standard error was zero, or the two sides carry a single
    /// per-iteration value between them.
    degenerate_dispersion: bool,
    /// The family is larger than the resample budget can resolve, so no
    /// bootstrap was run and there is no interval to read.
    family_beyond_resolution: bool,
    /// A descriptor the method requires was readable on neither side.
    unobservable_hardware: bool,
    /// Whether the METHOD may claim a direction at all. Production always passes
    /// [`METHOD_QUALIFIED_FOR_DIRECTION`]; it is a field rather than a direct
    /// read of the constant so that the requalification harness can score what
    /// the method WOULD decide if it were qualified, which is the whole question
    /// the harness exists to answer. `only_the_frozen_constant_qualifies_a_comparison`
    /// holds production to the constant.
    method_qualified: bool,
    interval: (f64, f64),
    minimum_detectable_ratio: f64,
}

/// The frozen decision rule, in order. It reads what the two sample sets are —
/// their completeness, their size, how many executions they came from, whether
/// they carry any dispersion at all — and then the interval and the reported
/// precision; it describes the two measurements and attributes nothing.
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
    // The next three guards run BEFORE the interval and before the precision
    // gate, because each describes something that makes those two meaningless:
    // too few executions per side give an interval that measures the wrong
    // thing or measures it with a standard error this method understates, a
    // family past the resolution limit has no interval at all because no
    // bootstrap was run for it, and zero dispersion gives a minimum detectable
    // ratio of zero that no threshold can ever exceed. The family gate is read
    // before the dispersion one because skipping the bootstrap leaves an empty
    // distribution, and an empty distribution is not a degenerate measurement.
    if input.baseline_executions < MIN_EXECUTIONS_FOR_DIRECTION
        || input.candidate_executions < MIN_EXECUTIONS_FOR_DIRECTION
    {
        reasons.push(InconclusiveReason::InsufficientExecutions);
        return (ComparisonVerdict::Inconclusive, reasons);
    }
    if input.family_beyond_resolution {
        reasons.push(InconclusiveReason::FamilyBeyondResolution);
        return (ComparisonVerdict::Inconclusive, reasons);
    }
    if input.degenerate_dispersion {
        reasons.push(InconclusiveReason::DegenerateDispersion);
        return (ComparisonVerdict::Inconclusive, reasons);
    }
    // The samples are sound and an interval exists; what is missing is the
    // ambient condition under which both were taken. A difference the method
    // could otherwise call is not attributable while a parameter that could
    // have produced it was observed on neither side.
    if input.unobservable_hardware {
        reasons.push(InconclusiveReason::UnobservableHardware);
        return (ComparisonVerdict::Inconclusive, reasons);
    }
    if input.minimum_detectable_ratio > MATERIAL_THRESHOLD_RATIO {
        reasons.push(InconclusiveReason::PrecisionBelowThreshold);
        return (ComparisonVerdict::Inconclusive, reasons);
    }
    // Last, and deliberately last. Everything above is a fact about the two
    // sample sets and is worth telling the caller on its own; reaching this line
    // means the samples WOULD have carried a verdict and the only thing missing
    // is on this side of the wire. ADR-081 keeps directions and
    // `no_material_change` disabled until the requalification passes, and this
    // is that sentence as code rather than as prose.
    if !input.method_qualified {
        reasons.push(InconclusiveReason::MethodUnqualified);
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

/// The report-level facts every benchmark in one comparison shares.
#[derive(Clone, Copy)]
struct FamilyContext {
    alpha: f64,
    z_two_sided: f64,
    /// The family is past what [`BOOTSTRAP_RESAMPLES`] can resolve, so no
    /// bootstrap is run for any benchmark in it.
    beyond_resolution: bool,
    /// A required descriptor was readable on neither side.
    unobservable_hardware: bool,
}

fn compare_one(
    key: &str,
    baseline: &BenchmarkMeasurement,
    candidate: &BenchmarkMeasurement,
    family: FamilyContext,
) -> BenchmarkComparison {
    let baseline_side = SideSamples::of(baseline);
    let candidate_side = SideSamples::of(candidate);
    let baseline_values = &baseline_side.values;
    let candidate_values = &candidate_side.values;
    let baseline_sorted = sorted_copy(baseline_values);
    let candidate_sorted = sorted_copy(candidate_values);
    let baseline_median = median_sorted(&baseline_sorted);
    let candidate_median = median_sorted(&candidate_sorted);

    // What the two sample sets support a ratio for. The observed ratio is
    // reported whenever it exists, including for a family this method will not
    // claim an interval for: the measurement is still a measurement.
    let measurable =
        !baseline_values.is_empty() && !candidate_values.is_empty() && baseline_median > 0.0;
    // An interval this method has already decided it cannot resolve is not
    // computed at all: ten thousand resamples of a family of five hundred would
    // cost the whole compare budget to produce an endpoint that is the minimum
    // of the draws, and publishing that endpoint is the defect.
    let usable = measurable && !family.beyond_resolution;
    let effect_ratio = if measurable {
        candidate_median / baseline_median - 1.0
    } else {
        0.0
    };
    let outcome = if usable {
        bootstrap_ratio(key, &baseline_side, &candidate_side, family.alpha)
    } else {
        BootstrapOutcome {
            interval: (0.0, 0.0),
            standard_error: 0.0,
            degenerate: true,
        }
    };
    let minimum_detectable_ratio = if usable {
        (family.z_two_sided + Z_POWER_80) * outcome.standard_error
    } else {
        0.0
    };

    let baseline_executions = baseline_side.execution_count();
    let candidate_executions = candidate_side.execution_count();
    let (verdict, inconclusive_reasons) = decide(&VerdictInput {
        baseline_completeness: baseline.completeness(),
        candidate_completeness: candidate.completeness(),
        baseline_samples: baseline_values.len(),
        candidate_samples: candidate_values.len(),
        baseline_executions,
        candidate_executions,
        baseline_median_ns: baseline_median,
        degenerate_dispersion: usable
            && (outcome.degenerate
                || fewer_than_two_distinct_values(baseline_values, candidate_values)),
        family_beyond_resolution: family.beyond_resolution,
        unobservable_hardware: family.unobservable_hardware,
        method_qualified: METHOD_QUALIFIED_FOR_DIRECTION,
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
        baseline_executions,
        candidate_executions,
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
    let mut unobservable_hardware = false;
    dataset_compatibility(
        baseline,
        candidate,
        &mut reasons,
        &mut unobservable_hardware,
    );
    if !reasons.is_empty() {
        return Err(CompareError::incompatible(
            reasons,
            baseline,
            candidate,
            Vec::new(),
        ));
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
    let mut disagreements = Vec::new();
    for key in &common {
        let (Some(left), Some(right)) = (baseline.measurement(key), candidate.measurement(key))
        else {
            continue;
        };
        measurement_compatibility(key, left, right, &mut reasons, &mut disagreements);
        pairs.push((*key, left, right));
    }
    if !reasons.is_empty() {
        return Err(CompareError::incompatible(
            reasons,
            baseline,
            candidate,
            disagreements,
        ));
    }

    let method = ComparisonMethod::frozen(u32::try_from(pairs.len()).unwrap_or(u32::MAX));
    let alpha = method.adjusted_alpha();
    let family = FamilyContext {
        alpha,
        z_two_sided: inverse_standard_normal_cdf(1.0 - alpha / 2.0).unwrap_or(Z_TWO_SIDED_95),
        beyond_resolution: !method.resolves_family(),
        unobservable_hardware,
    };

    let comparisons = pairs
        .into_iter()
        .map(|(key, left, right)| compare_one(key, left, right, family))
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
        baseline_provenance: ComparedProvenance::of(baseline),
        candidate_provenance: ComparedProvenance::of(candidate),
    })
}

/// The ADR-081 requalification instrument. Test-only, and a CHILD of this
/// module on purpose: it drives this module's own private `decide`,
/// `cluster_draw`, `bootstrap_ratio` and `SideSamples` rather than a paraphrase
/// of them. It adds candidate INTERVAL estimators for comparison; it changes
/// nothing the product computes.
#[cfg(test)]
mod simulation;

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;
    use crate::benchmark::{
        APPROVED_CRITERION_VERSION, BenchmarkHarness, BenchmarkIdentity, BenchmarkProvenance,
        BenchmarkSelection, HardwareProfile, RawSample, ResourceQuotas, SampleUnit, SamplingMode,
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

    /// The frozen protocol runs three independent executions per call, so a
    /// fixture measurement deals its values over three `run_index` values. A
    /// fixture that pretended one execution produced everything would be
    /// refused a direction by the method itself, which is the point of
    /// `single_execution` below.
    fn measurement_with(
        name: &str,
        values: &[f64],
        completeness: MeasurementCompleteness,
    ) -> BenchmarkMeasurement {
        let samples = values
            .iter()
            .enumerate()
            .map(|(index, value)| RawSample::new(1, *value, (index % 3) as u8 + 1).unwrap())
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

    /// Every sample from one and the same execution.
    fn single_execution(name: &str, values: &[f64]) -> BenchmarkMeasurement {
        let samples = values
            .iter()
            .map(|value| RawSample::new(1, *value, 1).unwrap())
            .collect();
        BenchmarkMeasurement::new(
            identity(name),
            SamplingMode::Flat,
            samples,
            3_000,
            5_000,
            u32::try_from(values.len()).unwrap_or(u32::MAX),
            MeasurementCompleteness::Complete,
        )
        .unwrap()
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

    /// The verdict the frozen rule reaches over a published comparison with the
    /// qualification gate held OPEN.
    ///
    /// [`METHOD_QUALIFIED_FOR_DIRECTION`] is `false`, so every direction is
    /// currently withheld and an oracle that only asserted `Inconclusive` would
    /// stop discriminating anything about the estimator. These oracles are about
    /// WHICH direction the samples support, a question that outlives the gate,
    /// so they ask it with the gate open. It calls [`decide`] itself rather than
    /// restating the rule, and every input it passes is read off the published
    /// comparison except the three flags the caller states.
    ///
    /// Pair it with an assertion that the shipped verdict is `Inconclusive` for
    /// [`InconclusiveReason::MethodUnqualified`], which is what says the gate —
    /// and nothing else — is holding this direction back.
    fn direction_if_qualified(comparison: &BenchmarkComparison) -> ComparisonVerdict {
        decide(&VerdictInput {
            baseline_completeness: MeasurementCompleteness::Complete,
            candidate_completeness: MeasurementCompleteness::Complete,
            baseline_samples: comparison.baseline_samples,
            candidate_samples: comparison.candidate_samples,
            baseline_executions: comparison.baseline_executions,
            candidate_executions: comparison.candidate_executions,
            baseline_median_ns: comparison.baseline_median_ns,
            degenerate_dispersion: false,
            family_beyond_resolution: false,
            unobservable_hardware: false,
            method_qualified: true,
            interval: comparison.confidence_interval,
            minimum_detectable_ratio: comparison.minimum_detectable_ratio,
        })
        .0
    }

    /// The shipped verdict of a comparison the gate — and only the gate — is
    /// holding back.
    fn held_only_by_the_qualification_gate(comparison: &BenchmarkComparison) {
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::MethodUnqualified],
            "something other than the qualification gate withheld this verdict"
        );
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
        // The ten values were dealt over the protocol's three executions, and
        // the report says so beside the sample counts.
        assert_eq!(comparison.baseline_executions, MIN_EXECUTIONS_FOR_DIRECTION);
        assert_eq!(
            comparison.candidate_executions,
            MIN_EXECUTIONS_FOR_DIRECTION
        );
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
        held_only_by_the_qualification_gate(comparison);
        assert_eq!(
            direction_if_qualified(comparison),
            ComparisonVerdict::Regression
        );
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
        held_only_by_the_qualification_gate(comparison);
        assert_eq!(
            direction_if_qualified(comparison),
            ComparisonVerdict::Improvement
        );
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
        held_only_by_the_qualification_gate(comparison);
        let would_be = direction_if_qualified(comparison);
        assert_ne!(would_be, ComparisonVerdict::Regression);
        assert_ne!(would_be, ComparisonVerdict::Improvement);
        assert_eq!(would_be, ComparisonVerdict::NoMaterialChange);
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
        // Both facts hold and both are reported: the summary describes a
        // prefix of the run, AND the method may not turn any interval into a
        // direction. The truncation is named first because it is the caller's.
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![
                InconclusiveReason::TruncatedMeasurement,
                InconclusiveReason::MethodUnqualified
            ]
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
            baseline_executions: MIN_EXECUTIONS_FOR_DIRECTION,
            candidate_executions: MIN_EXECUTIONS_FOR_DIRECTION,
            baseline_median_ns: median,
            degenerate_dispersion: false,
            family_beyond_resolution: false,
            unobservable_hardware: false,
            // These two tests are about the INTERVAL logic, so they hold the
            // qualification gate open; `no_input_produces_a_direction_while_the_
            // method_is_unqualified` is what covers it being shut.
            method_qualified: true,
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
        // Too few executions on either side refuses the direction the interval
        // would otherwise support, and is read before the precision gate. Two
        // is included: it is the row the cluster bootstrap understates most.
        for (baseline_executions, candidate_executions) in
            [(1, 3), (3, 1), (1, 1), (0, 3), (2, 3), (3, 2), (2, 2)]
        {
            let input = VerdictInput {
                baseline_executions,
                candidate_executions,
                ..base(1_000.0, (0.06, 0.30), 0.01)
            };
            assert_eq!(
                decide(&input),
                (
                    ComparisonVerdict::Inconclusive,
                    vec![InconclusiveReason::InsufficientExecutions]
                ),
                "{baseline_executions}/{candidate_executions}"
            );
        }
        // A zero-width interval with a zero minimum detectable ratio is the
        // shape a constant-emitting harness produces. It is refused for what it
        // is: no dispersion was observed, so nothing was learned about it.
        assert_eq!(
            decide(&VerdictInput {
                degenerate_dispersion: true,
                ..base(1_000.0, (0.20, 0.20), 0.0)
            }),
            (
                ComparisonVerdict::Inconclusive,
                vec![InconclusiveReason::DegenerateDispersion]
            )
        );
        // The two gates added for the G8 review, and the order they sit in.
        assert_eq!(
            decide(&VerdictInput {
                family_beyond_resolution: true,
                ..base(1_000.0, (0.06, 0.30), 0.01)
            })
            .1,
            vec![InconclusiveReason::FamilyBeyondResolution]
        );
        assert_eq!(
            decide(&VerdictInput {
                unobservable_hardware: true,
                ..base(1_000.0, (0.06, 0.30), 0.01)
            })
            .1,
            vec![InconclusiveReason::UnobservableHardware]
        );
        // A defect of the samples outranks a defect of the report: too few
        // executions per side is named even when the family is also past the
        // limit, so a caller is told the thing it has to fix first. The real
        // guest captures depend on this ordering to keep saying what they say.
        assert_eq!(
            decide(&VerdictInput {
                baseline_executions: 1,
                family_beyond_resolution: true,
                unobservable_hardware: true,
                ..base(1_000.0, (0.06, 0.30), 0.01)
            })
            .1,
            vec![InconclusiveReason::InsufficientExecutions]
        );
        // A family past the limit ran no bootstrap, so the empty distribution
        // it leaves behind must not be reported as a degenerate measurement.
        assert_eq!(
            decide(&VerdictInput {
                family_beyond_resolution: true,
                degenerate_dispersion: true,
                ..base(1_000.0, (0.0, 0.0), 0.0)
            })
            .1,
            vec![InconclusiveReason::FamilyBeyondResolution]
        );
        // An unobservable ambient parameter is read after the sample-set
        // defects and before the precision gate: the interval exists, and what
        // is missing is the condition it was measured under.
        assert_eq!(
            decide(&VerdictInput {
                unobservable_hardware: true,
                ..base(1_000.0, (0.06, 0.30), 0.9)
            })
            .1,
            vec![InconclusiveReason::UnobservableHardware]
        );
        assert_eq!(
            decide(&VerdictInput {
                unobservable_hardware: true,
                degenerate_dispersion: true,
                ..base(1_000.0, (0.20, 0.20), 0.0)
            })
            .1,
            vec![InconclusiveReason::DegenerateDispersion]
        );
    }

    #[test]
    fn one_execution_per_side_never_receives_a_direction() {
        // A 20% gap, sixty samples a side, tight dispersion: everything the
        // method needs except the executions to compare against.
        let baseline = jitter(1, 60, 1_000.0, 0.02);
        let candidate = jitter(2, 60, 1_200.0, 0.02);
        let report = compare(
            &dataset(
                "run-baseline",
                vec![single_execution("bench/one", &baseline)],
            ),
            &dataset(
                "run-candidate",
                vec![single_execution("bench/one", &candidate)],
            ),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::InsufficientExecutions]
        );
        // The measurement itself is still reported; only the direction is not.
        assert!((comparison.effect_ratio - 0.20).abs() < 0.03);
        assert_eq!(comparison.baseline_samples, 60);
        assert_eq!(comparison.candidate_samples, 60);
        assert_eq!(comparison.baseline_executions, 1);
        assert_eq!(comparison.candidate_executions, 1);
    }

    #[test]
    fn one_execution_on_a_single_side_is_enough_to_withhold_the_direction() {
        let baseline = jitter(3, 60, 1_000.0, 0.02);
        let candidate = jitter(4, 60, 1_200.0, 0.02);
        let report = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &baseline)]),
            &dataset(
                "run-candidate",
                vec![single_execution("bench/one", &candidate)],
            ),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::InsufficientExecutions]
        );
        // And the two counts show WHICH side failed the gate.
        assert_eq!(comparison.baseline_executions, 3);
        assert_eq!(comparison.candidate_executions, 1);
    }

    /// The G8 residue, stated as an oracle over `compare` and not only over
    /// `decide`: two executions a side is the worst row the cluster bootstrap
    /// has — its standard error is short by `sqrt(2/1) = 1.41` — and it is
    /// reachable from the published `run_count` range, so the direction is
    /// refused there too. The same data with the protocol's three executions
    /// is admitted, which is what keeps this a gate on the protocol and not a
    /// blanket refusal.
    #[test]
    fn two_executions_per_side_are_refused_and_three_are_admitted() {
        // One deterministic population per side, dealt over `k` executions, so
        // the ONLY difference between the two cases below is `k` itself.
        let deal = |name: &str, values: &[f64], executions: u8| {
            let samples = values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    RawSample::new(1, *value, (index as u8) % executions + 1).unwrap()
                })
                .collect();
            BenchmarkMeasurement::new(
                identity(name),
                SamplingMode::Flat,
                samples,
                3_000,
                5_000,
                u32::try_from(values.len()).unwrap_or(u32::MAX),
                MeasurementCompleteness::Complete,
            )
            .unwrap()
        };
        let baseline = jitter(401, 60, 1_000.0, 0.02);
        let candidate = jitter(411, 60, 1_200.0, 0.02);
        let compared = |executions: u8| {
            compare(
                &dataset(
                    "run-baseline",
                    vec![deal("bench/one", &baseline, executions)],
                ),
                &dataset(
                    "run-candidate",
                    vec![deal("bench/one", &candidate, executions)],
                ),
            )
            .unwrap()
        };

        let refused = compared(2);
        let comparison = only(&refused);
        assert_eq!(comparison.baseline_executions, 2);
        assert_eq!(comparison.candidate_executions, 2);
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::InsufficientExecutions],
            "interval {:?}, mdr {}",
            comparison.confidence_interval,
            comparison.minimum_detectable_ratio
        );
        // The measurement is still described, and it is a measurement the
        // method WOULD have called a regression: the refusal is about the
        // execution count, not about the data being uninformative.
        assert!((comparison.effect_ratio - 0.20).abs() < 0.03);
        assert!(comparison.confidence_interval.0 > MATERIAL_THRESHOLD_RATIO);

        let admitted = compared(u8::try_from(MIN_EXECUTIONS_FOR_DIRECTION).unwrap());
        let comparison = only(&admitted);
        assert_eq!(comparison.baseline_executions, MIN_EXECUTIONS_FOR_DIRECTION);
        assert_eq!(
            comparison.candidate_executions,
            MIN_EXECUTIONS_FOR_DIRECTION
        );
        // The execution gate no longer refuses it: what remains is the
        // qualification gate, alone, and the interval underneath it is the one
        // that supports a regression.
        held_only_by_the_qualification_gate(comparison);
        assert_eq!(
            direction_if_qualified(comparison),
            ComparisonVerdict::Regression
        );
    }

    /// H-01. While the method is unqualified, NO combination of inputs may
    /// produce a direction or a `no_material_change`.
    ///
    /// ADR-081 says both stay disabled until the requalification passes. Before
    /// [`METHOD_QUALIFIED_FOR_DIRECTION`] existed, the only thing withholding
    /// them was that this container cannot read `cpu_governor` — an accident of
    /// the runtime, not a decision — so a host where that field IS readable
    /// would have started emitting directions from an estimator whose measured
    /// coverage is 0.84 against a published 0.95. This sweep is that sentence
    /// made falsifiable: it walks the whole input space of [`decide`] and
    /// asserts the answer is always `Inconclusive`.
    #[test]
    fn no_input_produces_a_direction_while_the_method_is_unqualified() {
        use MeasurementCompleteness as Completeness;
        let completeness = [
            Completeness::Complete,
            Completeness::Truncated,
            Completeness::Missing,
        ];
        // Intervals chosen to land on every branch of the final rule: entirely
        // above the threshold, entirely below it, entirely inside it, and
        // straddling it.
        let intervals = [
            (0.06, 0.30),
            (-0.30, -0.06),
            (-0.01, 0.01),
            (-0.10, 0.10),
            (0.0, 0.0),
        ];
        let mut seen = 0_usize;
        for baseline_completeness in completeness {
            for candidate_completeness in completeness {
                for samples in [
                    0,
                    MIN_SAMPLES_FOR_INFERENCE - 1,
                    MIN_SAMPLES_FOR_INFERENCE,
                    90,
                ] {
                    for executions in [0, 1, 2, MIN_EXECUTIONS_FOR_DIRECTION, 10] {
                        for median in [-1.0, 0.0, 1_000.0] {
                            for degenerate in [false, true] {
                                for beyond in [false, true] {
                                    for unobservable in [false, true] {
                                        for interval in intervals {
                                            for mdr in [0.0, 0.01, 0.049, 0.05, 0.5] {
                                                let (verdict, reasons) = decide(&VerdictInput {
                                                    baseline_completeness,
                                                    candidate_completeness,
                                                    baseline_samples: samples,
                                                    candidate_samples: samples,
                                                    baseline_executions: executions,
                                                    candidate_executions: executions,
                                                    baseline_median_ns: median,
                                                    degenerate_dispersion: degenerate,
                                                    family_beyond_resolution: beyond,
                                                    unobservable_hardware: unobservable,
                                                    method_qualified:
                                                        METHOD_QUALIFIED_FOR_DIRECTION,
                                                    interval,
                                                    minimum_detectable_ratio: mdr,
                                                });
                                                seen += 1;
                                                assert_eq!(
                                                    verdict,
                                                    ComparisonVerdict::Inconclusive,
                                                    "samples {samples}, executions \
                                                     {executions}, median {median}, \
                                                     interval {interval:?}, mdr {mdr}, \
                                                     reasons {reasons:?}"
                                                );
                                                assert!(
                                                    !reasons.is_empty(),
                                                    "an inconclusive verdict always names a reason"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(seen > 10_000, "the sweep covered only {seen} inputs");
    }

    /// The sweep above would pass vacuously if something OTHER than the
    /// qualification gate were refusing these inputs, so this pins the gate as
    /// the thing that does it: the same inputs with the gate open produce all
    /// three withheld verdicts, and with it shut produce `MethodUnqualified`
    /// and nothing else.
    #[test]
    fn the_qualification_gate_is_what_withholds_each_of_the_three_verdicts() {
        let input = |interval: (f64, f64), method_qualified: bool| VerdictInput {
            baseline_completeness: MeasurementCompleteness::Complete,
            candidate_completeness: MeasurementCompleteness::Complete,
            baseline_samples: 90,
            candidate_samples: 90,
            baseline_executions: MIN_EXECUTIONS_FOR_DIRECTION,
            candidate_executions: MIN_EXECUTIONS_FOR_DIRECTION,
            baseline_median_ns: 1_000.0,
            degenerate_dispersion: false,
            family_beyond_resolution: false,
            unobservable_hardware: false,
            method_qualified,
            interval,
            minimum_detectable_ratio: 0.01,
        };
        for (interval, would_be) in [
            ((0.06, 0.30), ComparisonVerdict::Regression),
            ((-0.30, -0.06), ComparisonVerdict::Improvement),
            ((-0.01, 0.01), ComparisonVerdict::NoMaterialChange),
        ] {
            assert_eq!(
                decide(&input(interval, true)),
                (would_be, Vec::new()),
                "with the gate open, {interval:?} must read as {would_be:?}"
            );
            assert_eq!(
                decide(&input(interval, false)),
                (
                    ComparisonVerdict::Inconclusive,
                    vec![InconclusiveReason::MethodUnqualified]
                ),
                "with the gate shut, {interval:?} must be withheld by the gate alone"
            );
        }
    }

    /// Production never opens the gate on its own: the only value `compare`
    /// puts into that field is the frozen constant. A future edit that passed
    /// `true` from anywhere in the shipped path would make the sweep above
    /// meaningless, and this is what notices.
    #[test]
    // Asserting a constant is the entire point: this test exists to fail the
    // moment someone flips it without the receipt ADR-081 §1 requires.
    #[allow(clippy::assertions_on_constants)]
    fn only_the_frozen_constant_qualifies_a_comparison() {
        assert!(
            !METHOD_QUALIFIED_FOR_DIRECTION,
            "the requalification has not passed; see ADR-081 §1 and the receipt at \
             docs/validation/M5/02-method-simulation.json"
        );
        let baseline = jitter(1, 60, 1_000.0, 0.02);
        let candidate = jitter(2, 60, 1_200.0, 0.02);
        let report = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &baseline)]),
            &dataset("run-candidate", vec![measurement("bench/one", &candidate)]),
        )
        .unwrap();
        let comparison = only(&report);
        // A 20% ratio with tight dispersion is the friendliest input this method
        // has; if anything at all could still emit a direction today, it is this.
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert!(
            comparison
                .inconclusive_reasons
                .contains(&InconclusiveReason::MethodUnqualified)
        );
    }

    /// The gate is the constant, not a literal that happens to equal it: every
    /// count below it is refused and every count from it upward is not.
    #[test]
    fn the_direction_gate_reads_the_frozen_execution_constant() {
        let input = |baseline_executions: usize, candidate_executions: usize| VerdictInput {
            baseline_completeness: MeasurementCompleteness::Complete,
            candidate_completeness: MeasurementCompleteness::Complete,
            baseline_samples: 30,
            candidate_samples: 30,
            baseline_executions,
            candidate_executions,
            baseline_median_ns: 1_000.0,
            degenerate_dispersion: false,
            family_beyond_resolution: false,
            unobservable_hardware: false,
            method_qualified: true,
            interval: (0.06, 0.30),
            minimum_detectable_ratio: 0.01,
        };
        for count in 0..MIN_EXECUTIONS_FOR_DIRECTION {
            for (baseline, candidate) in [
                (count, MIN_EXECUTIONS_FOR_DIRECTION),
                (MIN_EXECUTIONS_FOR_DIRECTION, count),
            ] {
                assert_eq!(
                    decide(&input(baseline, candidate)),
                    (
                        ComparisonVerdict::Inconclusive,
                        vec![InconclusiveReason::InsufficientExecutions]
                    ),
                    "{baseline}/{candidate}"
                );
            }
        }
        for count in MIN_EXECUTIONS_FOR_DIRECTION..MIN_EXECUTIONS_FOR_DIRECTION + 3 {
            assert_eq!(
                decide(&input(count, count)).0,
                ComparisonVerdict::Regression,
                "{count} executions a side"
            );
        }
    }

    /// The mechanism [`MIN_EXECUTIONS_FOR_DIRECTION`] answers, computed here
    /// instead of asserted from the algebra.
    ///
    /// The outer stage of [`cluster_draw`] takes `k` clusters with replacement
    /// from `k`. Enumerate EVERY draw it can make — all `k^k`, equally likely —
    /// and take the exact variance of the cluster mean over that distribution.
    /// It lands at `((k - 1) / k)` of `s²/k`, which is what a standard error of
    /// a mean over `k` clusters is supposed to estimate. So the reported
    /// standard error is short by `sqrt(k / (k - 1))`, worst at the smallest
    /// `k` and never 1.0 at any `k` this contract can reach.
    #[test]
    fn the_cluster_shortfall_is_a_function_of_the_execution_count() {
        // Exact variance of the resampled mean over the whole draw space.
        let bootstrap_variance = |values: &[f64]| {
            let count = values.len();
            let draws = count.pow(u32::try_from(count).unwrap());
            let mut sum = 0.0;
            let mut sum_squares = 0.0;
            for draw in 0..draws {
                let mut position = draw;
                let mut total = 0.0;
                for _ in 0..count {
                    total += values[position % count];
                    position /= count;
                }
                let mean = total / count as f64;
                sum += mean;
                sum_squares += mean * mean;
            }
            let draws = draws as f64;
            let mean = sum / draws;
            sum_squares / draws - mean * mean
        };

        let mut shortfalls = Vec::new();
        for values in [vec![1.0, 3.0], vec![1.0, 3.0, 8.0]] {
            let k = values.len() as f64;
            let observed = bootstrap_variance(&values);
            // What the interval would need the resampling to reproduce.
            let deviation = standard_deviation(&values);
            let target = deviation * deviation / k;
            assert!(
                (observed / target - (k - 1.0) / k).abs() < 1e-9,
                "k={k}: bootstrap variance {observed} is not (k-1)/k of {target}"
            );
            shortfalls.push((k, (target / observed).sqrt()));
        }
        let (two, three) = (shortfalls[0].1, shortfalls[1].1);
        // The two magnitudes the constant's doc comment quotes, recomputed.
        assert!((two - std::f64::consts::SQRT_2).abs() < 1e-9, "{two}");
        assert!((1.224..1.225).contains(&three), "{three}");
        // Worst at the smallest k, and never absent at any k this contract can
        // reach: the admitted threshold bounds the shortfall, it does not
        // remove it. What remains is what CONFIDENCE_LEVEL discloses.
        assert!(two > three);
        assert!(three > 1.0, "{three}");
        assert_eq!(shortfalls[1].0 as usize, MIN_EXECUTIONS_FOR_DIRECTION);
        assert_eq!(CONFIDENCE_LEVEL, 0.95);
    }

    #[test]
    fn a_harness_that_emits_a_constant_receives_no_verdict() {
        // Two constants, one per side: the bootstrap moves nothing, so the
        // standard error is zero and the precision gate can never fire.
        let baseline = vec![1_000.0; 60];
        let candidate = vec![1_200.0; 60];
        let report = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &baseline)]),
            &dataset("run-candidate", vec![measurement("bench/one", &candidate)]),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::DegenerateDispersion]
        );
        // The reported minimum detectable ratio is a rounding residue of
        // summing ten thousand identical numbers, not a resolution this run
        // achieved. It is orders of magnitude below the threshold, which is
        // precisely why the degeneracy is decided from the width of the
        // bootstrap distribution and never from this number.
        assert!(
            comparison.minimum_detectable_ratio < 1e-9,
            "{}",
            comparison.minimum_detectable_ratio
        );
        // A zero-width interval: every resample returned the same ratio.
        let (low, high) = comparison.confidence_interval;
        assert_eq!(low, high);
        assert!((low - 0.20).abs() < 1e-12, "{low}");
        assert!((comparison.effect_ratio - 0.20).abs() < 1e-12);

        // The same constant on both sides is equally uninformative: a
        // zero-width interval around zero is not "no material change".
        let flat = compare(
            &dataset("run-baseline", vec![measurement("bench/one", &baseline)]),
            &dataset("run-candidate", vec![measurement("bench/one", &baseline)]),
        )
        .unwrap();
        assert_eq!(
            only(&flat).inconclusive_reasons,
            vec![InconclusiveReason::DegenerateDispersion]
        );
    }

    /// The defect this method was corrected for, stated as an oracle.
    ///
    /// Each side ran the protocol's three executions, whose medians span ~6% —
    /// ordinary host drift — while the two sides differ by ~7.5%. Resampling
    /// the samples inside the executions treats the 180 pooled samples as 180
    /// independent draws and reports a standard error small enough to call a
    /// regression. Resampling the executions reports the drift as well, and the
    /// drift alone is larger than the material threshold, so no direction is
    /// claimed. Three executions a side is what keeps this test about the
    /// variance model: the execution gate is satisfied here, so the refusal it
    /// asserts is the precision gate reading a real between-execution spread.
    #[test]
    fn drift_between_executions_is_not_read_as_a_difference_between_datasets() {
        let three_executions = |name: &str, runs: [&[f64]; 3]| {
            let samples = runs
                .iter()
                .enumerate()
                .flat_map(|(position, values)| {
                    let index = u8::try_from(position + 1).unwrap();
                    values
                        .iter()
                        .map(move |value| RawSample::new(1, *value, index).unwrap())
                })
                .collect::<Vec<_>>();
            BenchmarkMeasurement::new(
                identity(name),
                SamplingMode::Flat,
                samples,
                3_000,
                5_000,
                60,
                MeasurementCompleteness::Complete,
            )
            .unwrap()
        };
        let baseline = three_executions(
            "bench/one",
            [
                &jitter(301, 20, 1_000.0, 0.005),
                &jitter(306, 20, 1_030.0, 0.005),
                &jitter(311, 20, 1_060.0, 0.005),
            ],
        );
        let candidate = three_executions(
            "bench/one",
            [
                &jitter(321, 20, 1_075.0, 0.005),
                &jitter(326, 20, 1_107.0, 0.005),
                &jitter(331, 20, 1_140.0, 0.005),
            ],
        );
        let report = compare(
            &dataset("run-baseline", vec![baseline]),
            &dataset("run-candidate", vec![candidate]),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.baseline_executions, MIN_EXECUTIONS_FOR_DIRECTION);
        assert_eq!(
            comparison.candidate_executions,
            MIN_EXECUTIONS_FOR_DIRECTION
        );
        assert!(
            (comparison.effect_ratio - 0.075).abs() < 0.01,
            "{}",
            comparison.effect_ratio
        );
        assert_eq!(
            comparison.verdict,
            ComparisonVerdict::Inconclusive,
            "effect {:.4}, interval {:.4}..{:.4}, mdr {:.4}",
            comparison.effect_ratio,
            comparison.confidence_interval.0,
            comparison.confidence_interval.1,
            comparison.minimum_detectable_ratio
        );
        // Refused for the precision the drift leaves, not for the execution
        // count: the protocol's three executions are present.
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
            Err(CompareError::Incompatible(details)) => details.reasons,
            _ => Vec::new(),
        }
    }

    /// The reasons of a refusal, or an empty list for anything else. Used where
    /// the assertion is about the exact sorted reason list and not about the
    /// provenance that now travels beside it.
    fn refusal(result: Result<ComparisonReport, CompareError>) -> Vec<IncompatibilityReason> {
        match result {
            Err(error) => error.reasons().to_vec(),
            Ok(_) => Vec::new(),
        }
    }

    /// The provenance pair a refusal carries.
    fn refusal_details(result: Result<ComparisonReport, CompareError>) -> Incompatibility {
        match result {
            Err(CompareError::Incompatible(details)) => Some(*details),
            _ => None,
        }
        .unwrap()
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
            // Consulted since the G8 review: ADR-073 §3 says an unobservable or
            // differing hardware field blocks, and until now only `cpu_model`
            // did. A differing value under any of these is a different machine
            // or a different frozen configuration, not a different program.
            (
                |record| record.configuration_fingerprint = format!("sha256:{}", "9".repeat(64)),
                IncompatibilityReason::Configuration,
            ),
            (
                |record| record.hardware.cpu_cores = Some(8),
                IncompatibilityReason::CpuCores,
            ),
            (
                |record| record.hardware.cpu_cores = None,
                IncompatibilityReason::UnknownHardware,
            ),
            (
                |record| record.hardware.os_kernel = Some("Linux 5.15.0".to_owned()),
                IncompatibilityReason::OsKernel,
            ),
            (
                |record| record.hardware.os_kernel = None,
                IncompatibilityReason::UnknownHardware,
            ),
            (
                |record| record.hardware.cpu_governor = Some("powersave".to_owned()),
                IncompatibilityReason::CpuGovernor,
            ),
            (
                |record| record.hardware.cpu_governor = None,
                IncompatibilityReason::UnknownHardware,
            ),
            (
                |record| record.hardware.virtualization = Virtualization::Bare,
                IncompatibilityReason::Virtualization,
            ),
            (
                |record| record.hardware.virtualization = Virtualization::Unknown,
                IncompatibilityReason::UnknownHardware,
            ),
        ];
        for (mutate, expected) in cases {
            assert_eq!(incompatibility(mutate), vec![expected]);
        }
    }

    /// `declared_toolchain` stays out of the blocking set for the same reason
    /// `source_fingerprint` does: it describes what the project asked for, and
    /// the toolchain that actually measured is `rust_version`/`cargo_version`,
    /// which do block.
    #[test]
    fn a_differing_declared_toolchain_is_never_a_reason() {
        assert_eq!(
            incompatibility(|record| record.declared_toolchain = Some("nightly".to_owned())),
            Vec::new()
        );
        assert_eq!(
            incompatibility(|record| record.declared_toolchain = None),
            Vec::new()
        );
    }

    /// P2-2. The three ways a descriptor can fail to agree are three different
    /// facts and the caller is told which one it got.
    #[test]
    fn one_side_blind_and_both_sides_blind_are_different_answers() {
        let values = jitter(1_101, 60, 1_000.0, 0.02);
        let faster = jitter(1_111, 60, 800.0, 0.02);
        let side = |execution: &str, samples: &[f64], governor: Option<&str>| {
            let mut record = provenance(execution);
            record.hardware.cpu_governor = governor.map(str::to_owned);
            BenchmarkDataset::new(
                SampleUnit::Nanoseconds,
                vec![measurement("bench/one", samples)],
                record,
            )
            .unwrap()
        };

        // Known and different: its own reason, and the comparison is refused.
        assert_eq!(
            refusal(compare(
                &side("run-baseline", &values, Some("performance")),
                &side("run-candidate", &faster, Some("powersave")),
            )),
            vec![IncompatibilityReason::CpuGovernor]
        );

        // Observed on one side only: an asymmetry between the two captures, so
        // they were not observed alike and the comparison is refused too.
        assert_eq!(
            refusal(compare(
                &side("run-baseline", &values, Some("performance")),
                &side("run-candidate", &faster, None),
            )),
            vec![IncompatibilityReason::UnknownHardware]
        );

        // Readable on NEITHER side: the blindness is a property of the runtime
        // image, which is itself compared and equal. The two datasets stay
        // comparable and their measurements are still reported -- but the
        // direction the interval would otherwise support is withheld, and the
        // reason names the parameter nobody could see.
        let report = compare(
            &side("run-baseline", &values, None),
            &side("run-candidate", &faster, None),
        )
        .unwrap();
        let comparison = only(&report);
        assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
        assert_eq!(
            comparison.inconclusive_reasons,
            vec![InconclusiveReason::UnobservableHardware]
        );
        // The measurement itself survives; only the direction does not.
        assert!((comparison.effect_ratio + 0.20).abs() < 0.03);
        assert!(comparison.confidence_interval.1 < -MATERIAL_THRESHOLD_RATIO);
        // The same two datasets WITH the governor observed reach the interval,
        // so the refusal above is the unobservable field and nothing else. What
        // withholds the direction there is the qualification gate, which is a
        // different fact and carries a different name.
        let observed = compare(
            &side("run-baseline", &values, Some("performance")),
            &side("run-candidate", &faster, Some("performance")),
        )
        .unwrap();
        held_only_by_the_qualification_gate(only(&observed));
        assert_eq!(
            direction_if_qualified(only(&observed)),
            ComparisonVerdict::Improvement
        );
    }

    /// The other unobservable descriptors take the same path, and one blind
    /// field is enough.
    #[test]
    fn every_unobservable_descriptor_withholds_the_direction() {
        let values = jitter(1_121, 60, 1_000.0, 0.02);
        let faster = jitter(1_131, 60, 800.0, 0.02);
        let blind: [ProvenanceMutation; 4] = [
            |record| record.hardware.cpu_model = None,
            |record| record.hardware.cpu_cores = None,
            |record| record.hardware.os_kernel = None,
            |record| record.hardware.virtualization = Virtualization::Unknown,
        ];
        for mutate in blind {
            let side = |execution: &str, samples: &[f64]| {
                let mut record = provenance(execution);
                mutate(&mut record);
                BenchmarkDataset::new(
                    SampleUnit::Nanoseconds,
                    vec![measurement("bench/one", samples)],
                    record,
                )
                .unwrap()
            };
            let report = compare(
                &side("run-baseline", &values),
                &side("run-candidate", &faster),
            )
            .unwrap();
            assert_eq!(
                only(&report).inconclusive_reasons,
                vec![InconclusiveReason::UnobservableHardware]
            );
        }
    }

    /// Whether one mutation moves the published projection.
    fn projection_changed(mutate: ProvenanceMutation) -> bool {
        let values = jitter(1_201, 12, 1_000.0, 0.01);
        let build = |record: BenchmarkProvenance| {
            BenchmarkDataset::new(
                SampleUnit::Nanoseconds,
                vec![measurement("bench/one", &values)],
                record,
            )
            .unwrap()
        };
        let untouched = build(provenance("run-candidate"));
        let mut record = provenance("run-candidate");
        mutate(&mut record);
        ComparedProvenance::of(&untouched) != ComparedProvenance::of(&build(record))
    }

    /// P2-3. The published projection and the blocking set are the same set of
    /// fields, asserted as a biconditional over every provenance field: a field
    /// that can block a comparison is visible to the caller, and a field that
    /// cannot block is not published as if it mattered.
    ///
    /// `format`, `format_version` and `unit` sit on the dataset rather than the
    /// provenance record, so they are not reachable through a
    /// [`ProvenanceMutation`]; `an_unknown_dataset_format_is_incompatible_before_any_statistic`
    /// covers them and asserts both published versions.
    #[test]
    fn provenance_projection_carries_every_consulted_field() {
        let cases: Vec<(&str, ProvenanceMutation)> = vec![
            ("harness_version", |record| {
                record.harness_version = "0.7.0".to_owned();
            }),
            ("rust_version", |record| {
                record.rust_version = "1.97.0".to_owned();
            }),
            ("cargo_version", |record| {
                record.cargo_version = "1.97.0".to_owned();
            }),
            ("image_digest", |record| {
                record.image_digest = format!("sha256:{}", "e".repeat(64));
            }),
            ("platform", |record| {
                record.platform = "x86_64-unknown-linux-gnu".to_owned();
            }),
            ("configuration_fingerprint", |record| {
                record.configuration_fingerprint = format!("sha256:{}", "9".repeat(64));
            }),
            ("execution_fingerprint", |record| {
                // Equal fingerprints are one artifact, not two observations, so
                // for this field the blocking mutation is making it MATCH.
                record.execution_fingerprint = "run-baseline".to_owned();
            }),
            ("selection", |record| record.selection.all_features = true),
            ("hardware.arch", |record| {
                record.hardware.arch = "x86_64".to_owned();
            }),
            ("hardware.cpu_model", |record| {
                record.hardware.cpu_model = Some("Skylake".to_owned());
            }),
            ("hardware.cpu_cores", |record| {
                record.hardware.cpu_cores = Some(8);
            }),
            ("hardware.os_kernel", |record| {
                record.hardware.os_kernel = Some("Linux 5.15.0".to_owned());
            }),
            ("hardware.cpu_governor", |record| {
                record.hardware.cpu_governor = Some("powersave".to_owned());
            }),
            ("hardware.virtualization", |record| {
                record.hardware.virtualization = Virtualization::Bare;
            }),
            ("hardware.quotas", |record| {
                record.hardware.quotas.pids = Some(1);
            }),
            // Not consulted, and therefore not published.
            ("source_fingerprint", |record| {
                record.source_fingerprint = format!("sha256:{}", "f".repeat(64));
            }),
            ("declared_toolchain", |record| {
                record.declared_toolchain = Some("nightly".to_owned());
            }),
            ("run_index", |record| record.run_index = 2),
            ("captured_at_unix", |record| {
                record.captured_at_unix = 1_800_000_000;
            }),
        ];
        for (field, mutate) in cases {
            let blocks = !incompatibility(mutate).is_empty();
            assert_eq!(
                blocks,
                projection_changed(mutate),
                "{field}: blocks={blocks} but the published projection disagrees"
            );
        }
    }

    /// A refusal that names a field shows the two values it refused on.
    #[test]
    fn a_refusal_carries_both_sides_of_what_it_refused() {
        let values = jitter(1_211, 12, 1_000.0, 0.01);
        let baseline = dataset("run-baseline", vec![measurement("bench/one", &values)]);
        let mut record = provenance("run-candidate");
        record.hardware.cpu_model = Some("Skylake".to_owned());
        let candidate = BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            vec![measurement("bench/one", &values)],
            record,
        )
        .unwrap();
        let refused = refusal_details(compare(&baseline, &candidate));
        assert_eq!(refused.reasons, vec![IncompatibilityReason::CpuModel]);
        assert_eq!(
            refused.baseline_provenance.hardware.cpu_model.as_deref(),
            Some("Neoverse-N1")
        );
        assert_eq!(
            refused.candidate_provenance.hardware.cpu_model.as_deref(),
            Some("Skylake")
        );
        assert!(refused.disagreements.is_empty());
    }

    /// A compatible pair publishes the same two contexts: a verdict is about
    /// them, so a reader that cannot see them cannot see what it describes.
    #[test]
    fn a_successful_report_carries_both_provenances() {
        let values = jitter(1_221, 30, 1_000.0, 0.01);
        let baseline = dataset("run-baseline", vec![measurement("bench/one", &values)]);
        let candidate = dataset("run-candidate", vec![measurement("bench/one", &values)]);
        let report = compare(&baseline, &candidate).unwrap();
        assert_eq!(
            report.baseline_provenance,
            ComparedProvenance::of(&baseline)
        );
        assert_eq!(
            report.candidate_provenance,
            ComparedProvenance::of(&candidate)
        );
        assert_eq!(
            report.baseline_provenance.execution_fingerprint,
            "run-baseline"
        );
        assert_eq!(
            report.candidate_provenance.execution_fingerprint,
            "run-candidate"
        );
        assert_eq!(report.baseline_provenance.format, BENCHMARK_DATASET_FORMAT);
        // The projection is a copy, never a reconstruction: an absent field
        // stays absent rather than acquiring a plausible value.
        let mut blind = provenance("run-blind");
        blind.hardware.cpu_governor = None;
        let dataset = BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            vec![measurement("bench/one", &values)],
            blind,
        )
        .unwrap();
        assert_eq!(ComparedProvenance::of(&dataset).hardware.cpu_governor, None);
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
            refusal(compare(&one, &one)),
            vec![IncompatibilityReason::SameArtifact]
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
            refusal(compare(&baseline, &candidate)),
            vec![
                IncompatibilityReason::RustVersion,
                IncompatibilityReason::Platform,
                IncompatibilityReason::Architecture,
                IncompatibilityReason::SameArtifact,
            ]
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
                .enumerate()
                .map(|(index, v)| RawSample::new(1, *v, (index % 3) as u8 + 1).unwrap())
                .collect(),
            3_000,
            5_000,
            12,
            MeasurementCompleteness::Complete,
        )
        .unwrap();
        let renamed_refusal =
            refusal_details(compare(&baseline, &dataset("run-candidate", vec![renamed])));
        assert_eq!(
            renamed_refusal.reasons,
            vec![IncompatibilityReason::BenchmarkIdentity]
        );
        // The reason is not a bare tag: the key and BOTH observed identities
        // travel with it, because neither is readable from the provenance.
        assert_eq!(renamed_refusal.disagreements.len(), 1);
        let disagreement = &renamed_refusal.disagreements[0];
        assert_eq!(disagreement.key, "bench/one");
        assert_eq!(disagreement.baseline_identity.group_id(), "group");
        assert_eq!(disagreement.candidate_identity.group_id(), "other-group");
        assert_eq!(
            disagreement.baseline_sampling_mode,
            disagreement.candidate_sampling_mode
        );

        let remoded = BenchmarkMeasurement::new(
            identity("bench/one"),
            SamplingMode::Linear,
            values
                .iter()
                .enumerate()
                .map(|(index, v)| RawSample::new(1, *v, (index % 3) as u8 + 1).unwrap())
                .collect(),
            3_000,
            5_000,
            12,
            MeasurementCompleteness::Complete,
        )
        .unwrap();
        let remoded_refusal =
            refusal_details(compare(&baseline, &dataset("run-candidate", vec![remoded])));
        assert_eq!(
            remoded_refusal.reasons,
            vec![IncompatibilityReason::SamplingMode]
        );
        assert_eq!(remoded_refusal.disagreements.len(), 1);
        assert_eq!(
            remoded_refusal.disagreements[0].baseline_sampling_mode,
            SamplingMode::Flat
        );
        assert_eq!(
            remoded_refusal.disagreements[0].candidate_sampling_mode,
            SamplingMode::Linear
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
            r#""format_version":2"#,
            r#""format_version":3"#,
            1,
        ))?;
        let refused = refusal_details(compare(&baseline, &future));
        assert_eq!(refused.reasons, vec![IncompatibilityReason::FormatVersion]);
        // The refusal shows the two versions it refused on.
        assert_eq!(refused.baseline_provenance.format_version, 2);
        assert_eq!(refused.candidate_provenance.format_version, 3);
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

    /// P2-6. The published limit is not a hand-picked number: it is the largest
    /// family whose Bonferroni-adjusted tail still holds
    /// [`MIN_TAIL_RESAMPLES`] of the fixed [`BOOTSTRAP_RESAMPLES`] draws. If
    /// any of the three constants moves, this recomputes and the limit has to
    /// move with it.
    #[test]
    fn the_resolvable_family_size_is_derived_from_the_resample_budget() {
        let tail_draws = |family: u32| {
            let alpha = (1.0 - CONFIDENCE_LEVEL) / f64::from(family);
            f64::from(BOOTSTRAP_RESAMPLES) * alpha / 2.0
        };
        let derived = (1..10_000)
            .take_while(|family| tail_draws(*family) >= f64::from(MIN_TAIL_RESAMPLES))
            .last()
            .unwrap();
        assert_eq!(derived, MAX_RESOLVABLE_FAMILY_SIZE);
        // The boundary, stated in the units the reviewer used: at the limit the
        // endpoint still interpolates between the tenth and eleventh draws; one
        // benchmark further it does not.
        assert!(tail_draws(MAX_RESOLVABLE_FAMILY_SIZE) >= 10.0);
        assert!(tail_draws(MAX_RESOLVABLE_FAMILY_SIZE + 1) < 10.0);
        // And the method publishes it beside the family it applies to.
        let method = ComparisonMethod::frozen(MAX_RESOLVABLE_FAMILY_SIZE);
        assert_eq!(
            method.max_resolvable_family_size(),
            MAX_RESOLVABLE_FAMILY_SIZE
        );
        assert!(method.resolves_family());
        assert!(!ComparisonMethod::frozen(MAX_RESOLVABLE_FAMILY_SIZE + 1).resolves_family());
    }

    /// One benchmark past the limit and the whole report stops claiming
    /// intervals — including for a difference large enough that the same data
    /// in a resolvable family is called.
    #[test]
    fn a_family_past_the_resolution_limit_claims_no_interval() {
        let baseline_values = jitter(1_301, 60, 1_000.0, 0.005);
        let candidate_values = jitter(1_311, 60, 1_500.0, 0.005);
        let family = |count: u32| {
            let names: Vec<String> = (0..count)
                .map(|index| format!("bench/{index:04}"))
                .collect();
            let side = |execution: &str, values: &[f64]| {
                dataset(
                    execution,
                    names
                        .iter()
                        .map(|name| measurement(name, values))
                        .collect::<Vec<_>>(),
                )
            };
            compare(
                &side("run-baseline", &baseline_values),
                &side("run-candidate", &candidate_values),
            )
            .unwrap()
        };

        // At the limit the method still resolves an endpoint and reads it.
        let resolved = family(MAX_RESOLVABLE_FAMILY_SIZE);
        assert_eq!(resolved.method.family_size(), MAX_RESOLVABLE_FAMILY_SIZE);
        for comparison in &resolved.comparisons {
            assert!(
                !comparison
                    .inconclusive_reasons
                    .contains(&InconclusiveReason::FamilyBeyondResolution),
                "{} refused at the limit itself",
                comparison.key
            );
            assert!(comparison.confidence_interval.0 < comparison.confidence_interval.1);
            held_only_by_the_qualification_gate(comparison);
            assert_eq!(
                direction_if_qualified(comparison),
                ComparisonVerdict::Regression
            );
        }

        // One benchmark further, the adjusted tail holds fewer than ten of the
        // ten thousand draws, so no bootstrap is run and no endpoint is quoted.
        let refused = family(MAX_RESOLVABLE_FAMILY_SIZE + 1);
        assert_eq!(refused.method.family_size(), MAX_RESOLVABLE_FAMILY_SIZE + 1);
        assert!(!refused.method.resolves_family());
        for comparison in &refused.comparisons {
            assert_eq!(comparison.verdict, ComparisonVerdict::Inconclusive);
            assert_eq!(
                comparison.inconclusive_reasons,
                vec![InconclusiveReason::FamilyBeyondResolution]
            );
            assert_eq!(comparison.confidence_interval, (0.0, 0.0));
            assert_eq!(comparison.minimum_detectable_ratio, 0.0);
            // The measurement is still described: only the interval is absent.
            assert!((comparison.effect_ratio - 0.50).abs() < 0.03);
            assert_eq!(comparison.baseline_samples, 60);
            assert_eq!(comparison.candidate_samples, 60);
        }
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
        assert_eq!(left.baseline_executions, right.baseline_executions);
        assert_eq!(left.candidate_executions, right.candidate_executions);
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
        // The token states the fact the gate checks. It is not
        // `single_execution_per_side`: the threshold is three executions, so
        // the reason is emitted for two as well, and two are not "a single
        // execution".
        assert_eq!(
            serde_json::to_string(&InconclusiveReason::InsufficientExecutions)?,
            "\"insufficient_executions\""
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
