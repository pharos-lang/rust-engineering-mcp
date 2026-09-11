//! ADR-081 requalification instrument: a seeded simulation of the comparison
//! method against the criteria frozen in
//! `docs/adr/ADR-081-benchmark-statistical-requalification.md` §1, at the drift
//! points frozen in §2.
//!
//! This module is `#[cfg(test)]` and changes NOTHING about the shipped method.
//! It is a child of [`super`] on purpose: a child module can see its parent's
//! private items, so the simulation drives the product's OWN
//! [`super::cluster_draw`], [`super::bootstrap_ratio`], [`super::quantile_sorted`],
//! [`super::standard_deviation`], [`super::SideSamples`] and — the part that
//! matters most — the product's own [`super::decide`]. Nothing here paraphrases
//! the decision rule; the candidate estimators change only the INTERVAL and the
//! STANDARD ERROR handed to that rule, which is exactly the lever ADR-081 §5
//! authorizes.
//!
//! The one loop that is written out again rather than called is the resampling
//! loop of [`super::bootstrap_ratio`], because that function returns only the
//! finished interval and the alternative estimators need the resampled ratios
//! themselves. `the_local_bootstrap_loop_reproduces_the_products_bootstrap_ratio`
//! holds the two byte-for-byte identical, so the duplication is a verified
//! equivalence rather than a transcription anyone has to trust.
//!
//! Nothing here writes a file: `scripts/check-architecture.py` forbids
//! filesystem access anywhere under `crates/domain/src`, and the domain crate
//! has no business acquiring any. The harness prints one JSON document to
//! stdout between two markers and `scripts/simulate-m5-comparison-method.py`
//! reads it from there.
//!
//! Entry points, all driven by environment variables so the default `cargo test`
//! run stays a fast smoke test of the harness itself:
//!
//! ```text
//! M5_SIM_MODE=simulate M5_SIM_REPLICATES=10000 \
//!   cargo test --release -p rust-engineering-domain --lib \
//!   benchmark_compare::simulation::m5_02_requalification -- --exact --nocapture
//! ```

#![allow(clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.

use super::*;
use crate::benchmark::{
    APPROVED_CRITERION_VERSION, BenchmarkHarness, BenchmarkIdentity, BenchmarkProvenance,
    BenchmarkSelection, HardwareProfile, MeasurementCompleteness, RawSample, ResourceQuotas,
    SampleUnit, SamplingMode, Virtualization,
};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// Root seed of the SIMULATION's data generator. It has nothing to do with
/// [`BOOTSTRAP_SEED`], which stays exactly where it is: this one seeds the
/// synthetic executions the method is judged on, that one seeds the method's
/// own resampling draw and is not touched by anything here.
const SIMULATION_ROOT_SEED: u64 = 0x4D35_5F52_4551_5541;

/// Marker lines around the JSON document, so the reader does not have to guess
/// which part of the test harness's output is the payload.
const JSON_BEGIN: &str = "M5_SIMULATION_JSON_BEGIN";
const JSON_END: &str = "M5_SIMULATION_JSON_END";

/// Within-execution relative dispersion of the drift model, defaulted from the
/// real captures in `fixtures/benchmark-datasets/`: the eighteen
/// `sample.json` files of the six admitted-image executions have a
/// coefficient of variation of the per-iteration times between 0.018 and 0.070,
/// with a median of 0.033. 0.035 is that median rounded up.
const DEFAULT_WITHIN_EXECUTION_CV: f64 = 0.035;

/// Samples per execution. The frozen protocol asks criterion for thirty
/// (`--sample-size 30`, ADR-073 §2) and the real captures contain exactly
/// thirty.
const SAMPLES_PER_EXECUTION: usize = 30;

/// Baseline median in nanoseconds. Only the ratio matters; this is the order of
/// magnitude the real captures sit at (2 776–4 561 ns).
const BASELINE_MEDIAN_NS: f64 = 3_000.0;

/// Replicates per point when nothing overrides it. ADR-081 §1 requires at least
/// ten thousand.
const DEFAULT_REPLICATES: usize = 10_000;

/// Replicates handed to one worker at a time. Small enough that the costliest
/// cells (`k = 10`) still spread over every thread, large enough that the
/// atomic counter is not the bottleneck.
const WORK_CHUNK: usize = 50;

// ---------------------------------------------------------------------------
// Distributions the instrument needs and the method does not
// ---------------------------------------------------------------------------

/// Standard normal deviates by Box–Muller over the product's own SplitMix64, so
/// the generated data is a pure function of the seed.
struct Normals {
    rng: SplitMix64,
    spare: Option<f64>,
}

impl Normals {
    fn new(seed: u64) -> Self {
        Self {
            rng: SplitMix64::new(seed),
            spare: None,
        }
    }

    fn next(&mut self) -> f64 {
        if let Some(value) = self.spare.take() {
            return value;
        }
        // `next_unit` is uniform on [0, 1); one minus it is uniform on (0, 1],
        // so the logarithm below is always defined without a rejection loop.
        let first = 1.0 - self.rng.next_unit();
        let second = self.rng.next_unit();
        let radius = (-2.0 * first.ln()).sqrt();
        let angle = std::f64::consts::TAU * second;
        self.spare = Some(radius * angle.sin());
        radius * angle.cos()
    }
}

/// `erfc` to a fractional error below 1.2e-7 (Numerical Recipes' Chebyshev
/// form). Used only for the BCa endpoints, whose own resolution is the
/// bootstrap's order statistics.
fn erfc(x: f64) -> f64 {
    let z = x.abs();
    let t = 1.0 / (1.0 + 0.5 * z);
    let poly = -1.265_512_23
        + t * (1.000_023_68
            + t * (0.374_091_96
                + t * (0.096_784_18
                    + t * (-0.186_288_06
                        + t * (0.278_868_07
                            + t * (-1.135_203_98
                                + t * (1.488_515_87 + t * (-0.822_152_23 + t * 0.170_872_77))))))));
    let value = t * (-z * z + poly).exp();
    if x >= 0.0 { value } else { 2.0 - value }
}

/// Standard normal CDF.
fn standard_normal_cdf(x: f64) -> f64 {
    0.5 * erfc(-x / std::f64::consts::SQRT_2)
}

/// Lanczos log-gamma, g = 7, nine coefficients.
fn ln_gamma(x: f64) -> f64 {
    const COEFFICIENTS: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];
    if x < 0.5 {
        // Reflection, so the caller never has to know the branch.
        return std::f64::consts::PI.ln()
            - (std::f64::consts::PI * x).sin().abs().ln()
            - ln_gamma(1.0 - x);
    }
    let x = x - 1.0;
    let mut series = COEFFICIENTS[0];
    for (index, coefficient) in COEFFICIENTS.iter().enumerate().skip(1) {
        series += coefficient / (x + index as f64);
    }
    let t = x + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + series.ln()
}

/// Continued fraction for the incomplete beta function (Lentz).
fn beta_continued_fraction(a: f64, b: f64, x: f64) -> f64 {
    const TINY: f64 = 1e-300;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..300 {
        let m = f64::from(m);
        let m2 = 2.0 * m;
        let numerator = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + numerator * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + numerator / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let numerator = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + numerator * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + numerator / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-14 {
            break;
        }
    }
    h
}

/// Regularized incomplete beta `I_x(a, b)`.
fn regularized_incomplete_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let front =
        (ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln()).exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * beta_continued_fraction(a, b, x) / a
    } else {
        1.0 - front * beta_continued_fraction(b, a, 1.0 - x) / b
    }
}

/// CDF of Student's t with `df` degrees of freedom (real, not only integral:
/// the random-effects candidate uses a Welch–Satterthwaite df).
fn student_t_cdf(t: f64, df: f64) -> f64 {
    let x = df / (df + t * t);
    let tail = 0.5 * regularized_incomplete_beta(df / 2.0, 0.5, x);
    if t > 0.0 { 1.0 - tail } else { tail }
}

/// Upper quantile of Student's t by bisection on its CDF. Returns the two-sided
/// critical value's positive half, i.e. `t_{df, p}` for `p > 0.5`.
fn student_t_quantile(p: f64, df: f64) -> f64 {
    if !(0.5..1.0).contains(&p) || !df.is_finite() || df <= 0.0 {
        return Z_TWO_SIDED_95;
    }
    let (mut low, mut high) = (0.0_f64, 1.0_f64);
    // Grow the bracket rather than assume one: at df = 1 the 0.975 quantile is
    // already 12.7 and at df below one it is larger still.
    for _ in 0..60 {
        if student_t_cdf(high, df) >= p {
            break;
        }
        low = high;
        high *= 2.0;
    }
    for _ in 0..200 {
        let middle = 0.5 * (low + high);
        if student_t_cdf(middle, df) < p {
            low = middle;
        } else {
            high = middle;
        }
    }
    0.5 * (low + high)
}

// ---------------------------------------------------------------------------
// The drift model
// ---------------------------------------------------------------------------

/// The assumption this whole simulation rests on, written down so a reader can
/// see it rather than infer it.
///
/// For one side with `k` executions and a population median `m`:
///
/// ```text
/// execution j center:  c_j = m * (1 + tau * z_j),        z_j ~ N(0, 1)
/// sample i of exec j:  x_ji = c_j * (1 + within * w_ji), w_ji ~ N(0, 1)
/// ```
///
/// `tau` is the between-execution relative standard deviation ADR-081 §2 fixes
/// the range of; `within` is the within-execution one, defaulted from the real
/// captures. Both noises are symmetric with mean zero, so the population median
/// of a side is exactly its `m` and the TRUE effect of a pair is exactly
/// `m_candidate / m_baseline - 1`. That is what "coverage of the true effect"
/// means below, with no estimation step between the truth and the criterion.
///
/// This is the same family of models the independent review used ("a Gaussian
/// random-effects null"); it is an assumption, not a measurement, and the
/// receipt says which conclusions depend on it.
#[derive(Clone, Copy, Debug)]
struct DriftModel {
    between_execution_sd: f64,
    within_execution_sd: f64,
    samples_per_execution: usize,
    baseline_median_ns: f64,
}

impl DriftModel {
    /// One side's executions, grouped the way [`SideSamples`] wants them.
    ///
    /// The `max(floor)` keeps a time positive. At the widest drift point of
    /// ADR-081 §2 (`tau = 0.10`) it would take a ten-sigma deviate to reach the
    /// floor, so it never fires in practice; it is there so the generator
    /// cannot emit a negative duration under any seed.
    fn side(&self, normals: &mut Normals, median_ns: f64, executions: usize) -> SideSamples {
        const RELATIVE_FLOOR: f64 = 0.01;
        let mut grouped = Vec::with_capacity(executions);
        for _ in 0..executions {
            let center = median_ns
                * self
                    .between_execution_sd
                    .mul_add(normals.next(), 1.0)
                    .max(RELATIVE_FLOOR);
            let mut values = Vec::with_capacity(self.samples_per_execution);
            for _ in 0..self.samples_per_execution {
                values.push(
                    center
                        * self
                            .within_execution_sd
                            .mul_add(normals.next(), 1.0)
                            .max(RELATIVE_FLOOR),
                );
            }
            grouped.push(values);
        }
        let values = grouped.iter().flatten().copied().collect();
        SideSamples {
            executions: grouped,
            values,
        }
    }
}

// ---------------------------------------------------------------------------
// Candidate estimators
// ---------------------------------------------------------------------------

/// The interval estimators being judged. Every one of them feeds the product's
/// own [`decide`]; none of them changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Estimator {
    /// The shipped v2 estimator: percentile endpoints of the cluster bootstrap,
    /// standard error read off the same distribution.
    Percentile,
    /// The same endpoints, widened about the observed effect by the cluster
    /// bootstrap's known shortfall `sqrt(k / (k - 1))`. Corrects the first of
    /// the two approximations ADR-073 §4 names and leaves the second.
    PercentileClusterScaled,
    /// The cluster-scaled endpoints widened again by `t_{k-1} / z`, so the
    /// endpoints carry the correction for a scale estimated from `k` clusters.
    /// Corrects both approximations.
    PercentileStudentScaled,
    /// Bias-corrected and accelerated endpoints of the same cluster bootstrap,
    /// with the acceleration from a leave-one-EXECUTION-out jackknife. Corrects
    /// bias and skew; it does not pretend to correct a scale shortfall.
    BiasCorrectedAccelerated,
    /// An explicit random-effects model over the per-execution medians: the
    /// effect is the difference of the two sides' mean log execution median,
    /// its standard error is the two-sample between-execution one, and the
    /// endpoints use `t` at Welch–Satterthwaite degrees of freedom. No
    /// bootstrap is involved at all.
    RandomEffects,
}

impl Estimator {
    fn wire(self) -> &'static str {
        match self {
            Self::Percentile => "percentile",
            Self::PercentileClusterScaled => "percentile_cluster_scaled",
            Self::PercentileStudentScaled => "percentile_student_scaled",
            Self::BiasCorrectedAccelerated => "bca",
            Self::RandomEffects => "random_effects",
        }
    }

    /// Short name used to build a candidate id. Separate from [`Self::wire`] so
    /// the ids already published in `docs/validation/M5-02-method-simulation.json`
    /// keep their exact spelling while the estimator keeps its descriptive one.
    fn family(self) -> &'static str {
        match self {
            Self::Percentile => "percentile",
            Self::PercentileClusterScaled => "cluster_scaled",
            Self::PercentileStudentScaled => "student_scaled",
            Self::BiasCorrectedAccelerated => "bca",
            Self::RandomEffects => "random_effects",
        }
    }
}

#[derive(Clone, Debug)]
struct Candidate {
    id: String,
    executions: usize,
    estimator: Estimator,
}

/// The candidate set. Every authorized lever of ADR-081 §5 appears, crossed with
/// the execution counts: the shipped estimator as the baseline, each interval
/// correction on its own so the levers can be told apart, and the execution
/// counts the caller asks for so "more executions" is measured rather than
/// assumed.
///
/// The id is derived rather than tabulated, so a probe at any `k` — including
/// `BENCHMARK_MAX_RUNS`, the most executions the dataset format can represent —
/// gets a name without editing this function.
fn candidates(execution_counts: &[usize]) -> Vec<Candidate> {
    let mut all = Vec::new();
    for &executions in execution_counts {
        for estimator in [
            Estimator::Percentile,
            Estimator::PercentileClusterScaled,
            Estimator::PercentileStudentScaled,
            Estimator::BiasCorrectedAccelerated,
            Estimator::RandomEffects,
        ] {
            all.push(Candidate {
                id: format!("{}_k{executions}", estimator.family()),
                executions,
                estimator,
            });
        }
    }
    all
}

/// Everything one replicate's cluster bootstrap produces, computed once and
/// shared by every candidate that resamples at that execution count.
struct SharedDraw {
    /// Resampled ratios, sorted. The unsorted standard error is taken first,
    /// exactly as [`bootstrap_ratio`] does.
    sorted_ratios: Vec<f64>,
    standard_error: f64,
    degenerate: bool,
    observed_effect: f64,
    baseline_median_ns: f64,
    /// Leave-one-execution-out effect ratios, both sides pooled. BCa only.
    jackknife: Vec<f64>,
}

/// The resampling loop of [`bootstrap_ratio`], written out because that
/// function returns the finished interval and the alternative estimators need
/// the distribution. Held byte-for-byte identical to it by
/// `the_local_bootstrap_loop_reproduces_the_products_bootstrap_ratio`.
fn bootstrap_ratios(
    key: &str,
    baseline: &SideSamples,
    candidate: &SideSamples,
    resamples: u32,
) -> Vec<f64> {
    let mut rng = SplitMix64::for_benchmark(key);
    let mut ratios = Vec::with_capacity(resamples as usize);
    let mut baseline_draw = Vec::with_capacity(baseline.values.len());
    let mut candidate_draw = Vec::with_capacity(candidate.values.len());
    for _ in 0..resamples {
        let baseline_median = cluster_draw(&mut rng, baseline, &mut baseline_draw);
        let candidate_median = cluster_draw(&mut rng, candidate, &mut candidate_draw);
        ratios.push(if baseline_median > 0.0 {
            candidate_median / baseline_median - 1.0
        } else {
            0.0
        });
    }
    ratios
}

/// Median of one side's samples with execution `skip` left out.
fn median_without_execution(side: &SideSamples, skip: usize) -> f64 {
    let mut values: Vec<f64> = side
        .executions
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != skip)
        .flat_map(|(_, execution)| execution.iter().copied())
        .collect();
    values.sort_unstable_by(f64::total_cmp);
    median_sorted(&values)
}

fn shared_draw(
    key: &str,
    baseline: &SideSamples,
    candidate: &SideSamples,
    resamples: u32,
) -> SharedDraw {
    let mut ratios = bootstrap_ratios(key, baseline, candidate, resamples);
    let standard_error = standard_deviation(&ratios);
    ratios.sort_unstable_by(f64::total_cmp);
    let degenerate = match (ratios.first(), ratios.last()) {
        (Some(low), Some(high)) => low.total_cmp(high) == std::cmp::Ordering::Equal,
        _ => true,
    };
    let baseline_median = median_sorted(&sorted_copy(&baseline.values));
    let candidate_median = median_sorted(&sorted_copy(&candidate.values));
    let observed_effect = if baseline_median > 0.0 {
        candidate_median / baseline_median - 1.0
    } else {
        0.0
    };
    let mut jackknife = Vec::with_capacity(baseline.executions.len() + candidate.executions.len());
    for index in 0..baseline.executions.len() {
        let reduced = median_without_execution(baseline, index);
        jackknife.push(if reduced > 0.0 {
            candidate_median / reduced - 1.0
        } else {
            0.0
        });
    }
    for index in 0..candidate.executions.len() {
        let reduced = median_without_execution(candidate, index);
        jackknife.push(if baseline_median > 0.0 {
            reduced / baseline_median - 1.0
        } else {
            0.0
        });
    }
    SharedDraw {
        sorted_ratios: ratios,
        standard_error,
        degenerate,
        observed_effect,
        baseline_median_ns: baseline_median,
        jackknife,
    }
}

/// One candidate's interval and standard error. The verdict is NOT decided
/// here: it is decided by the product's [`decide`], from these two numbers.
struct Estimate {
    interval: (f64, f64),
    standard_error: f64,
    degenerate: bool,
}

fn scale_about(interval: (f64, f64), center: f64, factor: f64) -> (f64, f64) {
    (
        center + (interval.0 - center) * factor,
        center + (interval.1 - center) * factor,
    )
}

/// `sqrt(k / (k - 1))`, the factor by which a cluster bootstrap over `k`
/// clusters understates the standard error. Computed, never restated.
fn cluster_shortfall(executions: usize) -> f64 {
    if executions < 2 {
        return 1.0;
    }
    let k = executions as f64;
    (k / (k - 1.0)).sqrt()
}

fn bca_endpoints(draw: &SharedDraw, alpha: f64) -> (f64, f64) {
    let count = draw.sorted_ratios.len();
    if count == 0 {
        return (0.0, 0.0);
    }
    let below = draw
        .sorted_ratios
        .iter()
        .filter(|value| **value < draw.observed_effect)
        .count();
    let equal = draw
        .sorted_ratios
        .iter()
        .filter(|value| value.total_cmp(&draw.observed_effect) == std::cmp::Ordering::Equal)
        .count();
    let proportion = ((below as f64) + 0.5 * (equal as f64)) / count as f64;
    // Keep the bias correction inside the open unit interval: the inverse
    // normal is undefined at the ends and an observed effect outside every
    // resample is a degenerate draw, not an infinite bias.
    let floor = 0.5 / count as f64;
    let clamped = proportion.clamp(floor, 1.0 - floor);
    let bias = inverse_standard_normal_cdf(clamped).unwrap_or(0.0);

    let mean = if draw.jackknife.is_empty() {
        0.0
    } else {
        draw.jackknife.iter().sum::<f64>() / draw.jackknife.len() as f64
    };
    let (mut cubes, mut squares) = (0.0_f64, 0.0_f64);
    for value in &draw.jackknife {
        let deviation = mean - value;
        squares += deviation * deviation;
        cubes += deviation * deviation * deviation;
    }
    let acceleration = if squares > 0.0 {
        cubes / (6.0 * squares.powf(1.5))
    } else {
        0.0
    };

    let adjust = |probability: f64| -> f64 {
        let z = inverse_standard_normal_cdf(probability).unwrap_or(0.0);
        let denominator = 1.0 - acceleration * (bias + z);
        if denominator.abs() < 1e-12 {
            return probability;
        }
        standard_normal_cdf(bias + (bias + z) / denominator)
    };
    (
        quantile_sorted(&draw.sorted_ratios, adjust(alpha / 2.0)),
        quantile_sorted(&draw.sorted_ratios, adjust(1.0 - alpha / 2.0)),
    )
}

/// The explicit random-effects interval over the per-execution medians.
///
/// One observation per execution, on the log scale so the two sides combine
/// into a ratio; the standard error is the ordinary two-sample one over those
/// observations, and the endpoints use `t` at Welch–Satterthwaite degrees of
/// freedom. It carries no cluster shortfall because it never resamples the
/// clusters: it uses their unbiased sample variance directly.
fn random_effects(baseline: &SideSamples, candidate: &SideSamples, alpha: f64) -> Estimate {
    let logs = |side: &SideSamples| -> Vec<f64> {
        side.executions
            .iter()
            .map(|execution| {
                let median = median_sorted(&sorted_copy(execution));
                if median > 0.0 { median.ln() } else { 0.0 }
            })
            .collect()
    };
    let baseline_logs = logs(baseline);
    let candidate_logs = logs(candidate);
    let (kb, kc) = (baseline_logs.len(), candidate_logs.len());
    if kb < 2 || kc < 2 {
        return Estimate {
            interval: (0.0, 0.0),
            standard_error: 0.0,
            degenerate: true,
        };
    }
    let mean = |values: &[f64]| values.iter().sum::<f64>() / values.len() as f64;
    let baseline_mean = mean(&baseline_logs);
    let candidate_mean = mean(&candidate_logs);
    let baseline_sd = standard_deviation(&baseline_logs);
    let candidate_sd = standard_deviation(&candidate_logs);
    let baseline_term = baseline_sd * baseline_sd / kb as f64;
    let candidate_term = candidate_sd * candidate_sd / kc as f64;
    let variance = baseline_term + candidate_term;
    let effect_log = candidate_mean - baseline_mean;
    if !variance.is_finite() || variance <= 0.0 {
        return Estimate {
            interval: (effect_log.exp() - 1.0, effect_log.exp() - 1.0),
            standard_error: 0.0,
            degenerate: true,
        };
    }
    let standard_error_log = variance.sqrt();
    let denominator = baseline_term * baseline_term / (kb as f64 - 1.0)
        + candidate_term * candidate_term / (kc as f64 - 1.0);
    let degrees = if denominator > 0.0 {
        variance * variance / denominator
    } else {
        (kb + kc - 2) as f64
    };
    let critical = student_t_quantile(1.0 - alpha / 2.0, degrees);
    let low = (effect_log - critical * standard_error_log).exp() - 1.0;
    let high = (effect_log + critical * standard_error_log).exp() - 1.0;
    Estimate {
        interval: (low, high),
        // Delta method back onto the ratio scale, so the frozen MDR formula
        // `(z + z_0.80) * SE` keeps meaning the same thing it means for the
        // shipped estimator.
        standard_error: standard_error_log * effect_log.exp(),
        degenerate: false,
    }
}

fn estimate(
    candidate: &Candidate,
    draw: &SharedDraw,
    baseline: &SideSamples,
    candidate_side: &SideSamples,
    alpha: f64,
) -> Estimate {
    let percentile = (
        quantile_sorted(&draw.sorted_ratios, alpha / 2.0),
        quantile_sorted(&draw.sorted_ratios, 1.0 - alpha / 2.0),
    );
    let shortfall = cluster_shortfall(candidate.executions);
    match candidate.estimator {
        Estimator::Percentile => Estimate {
            interval: percentile,
            standard_error: draw.standard_error,
            degenerate: draw.degenerate,
        },
        Estimator::PercentileClusterScaled => Estimate {
            interval: scale_about(percentile, draw.observed_effect, shortfall),
            standard_error: draw.standard_error * shortfall,
            degenerate: draw.degenerate,
        },
        Estimator::PercentileStudentScaled => {
            let degrees = candidate.executions as f64 - 1.0;
            let widening = student_t_quantile(1.0 - alpha / 2.0, degrees)
                / inverse_standard_normal_cdf(1.0 - alpha / 2.0).unwrap_or(Z_TWO_SIDED_95);
            Estimate {
                interval: scale_about(percentile, draw.observed_effect, shortfall * widening),
                // The `t` factor is a quantile, not a scale: it widens the
                // endpoints and leaves the standard error the cluster-scaled
                // one, so the frozen MDR keeps its definition.
                standard_error: draw.standard_error * shortfall,
                degenerate: draw.degenerate,
            }
        }
        Estimator::BiasCorrectedAccelerated => Estimate {
            interval: bca_endpoints(draw, alpha),
            standard_error: draw.standard_error,
            degenerate: draw.degenerate,
        },
        Estimator::RandomEffects => random_effects(baseline, candidate_side, alpha),
    }
}

// ---------------------------------------------------------------------------
// Tally
// ---------------------------------------------------------------------------

/// The float accumulators of one tally, kept apart from the integer counts
/// because their reduction order is observable in the last bits of the reported
/// means.
#[derive(Clone, Copy, Default)]
struct Sums {
    width: f64,
    mdr: f64,
    effect: f64,
}

impl Sums {
    fn add(&mut self, other: &Self) {
        self.width += other.width;
        self.mdr += other.mdr;
        self.effect += other.effect;
    }
}

#[derive(Clone, Default)]
struct Tally {
    replicates: u64,
    covered: u64,
    regression: u64,
    improvement: u64,
    no_material_change: u64,
    inconclusive: u64,
    /// Verdict equal to the direction the true effect actually has. Zero by
    /// definition when the true effect is zero.
    directional_correct: u64,
    /// Any direction at all, which under a true null is a false positive.
    directional_any: u64,
    /// The direction the true effect does NOT have.
    directional_wrong: u64,
    /// The interval lay entirely beyond the material threshold on the true
    /// effect's side, whether or not the precision gate let a verdict out.
    /// Separates the interval's contribution from the gate's.
    interval_beyond_threshold: u64,
    /// The interval excluded zero on the true effect's side: the ordinary
    /// significance reading, which is laxer than this method's.
    interval_excludes_zero: u64,
    degenerate: u64,
    sums: Sums,
    reasons: BTreeMap<&'static str, u64>,
}

impl Tally {
    fn merge(&mut self, other: &Self) {
        self.replicates += other.replicates;
        self.covered += other.covered;
        self.regression += other.regression;
        self.improvement += other.improvement;
        self.no_material_change += other.no_material_change;
        self.inconclusive += other.inconclusive;
        self.directional_correct += other.directional_correct;
        self.directional_any += other.directional_any;
        self.directional_wrong += other.directional_wrong;
        self.interval_beyond_threshold += other.interval_beyond_threshold;
        self.interval_excludes_zero += other.interval_excludes_zero;
        self.degenerate += other.degenerate;
        // The float sums are deliberately NOT merged here. Floating-point
        // addition is not associative, so merging them in whatever order the
        // threads happened to finish would make the reported means depend on
        // scheduling. They are reduced in chunk order by `simulate` instead.
        for (reason, count) in &other.reasons {
            *self.reasons.entry(reason).or_default() += count;
        }
    }
}

fn reason_wire(reason: InconclusiveReason) -> &'static str {
    match reason {
        InconclusiveReason::InsufficientSamples => "insufficient_samples",
        InconclusiveReason::PrecisionBelowThreshold => "precision_below_threshold",
        InconclusiveReason::IntervalSpansThreshold => "interval_spans_threshold",
        InconclusiveReason::ZeroOrNegativeBaseline => "zero_or_negative_baseline",
        InconclusiveReason::MissingMeasurement => "missing_measurement",
        InconclusiveReason::TruncatedMeasurement => "truncated_measurement",
        InconclusiveReason::InsufficientExecutions => "insufficient_executions",
        InconclusiveReason::DegenerateDispersion => "degenerate_dispersion",
        InconclusiveReason::FamilyBeyondResolution => "family_beyond_resolution",
        InconclusiveReason::UnobservableHardware => "unobservable_hardware",
        InconclusiveReason::MethodUnqualified => "method_unqualified",
    }
}

// ---------------------------------------------------------------------------
// The simulation itself
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct Cell {
    executions: usize,
    drift: f64,
    true_effect: f64,
}

#[derive(Clone, Copy)]
struct Configuration {
    seed: u64,
    replicates: usize,
    resamples: u32,
    within_execution_sd: f64,
    /// Family of one, which is the narrowest interval this method ever claims
    /// (no Bonferroni) and therefore the hardest case for coverage and the
    /// easiest for a false direction. ADR-073's own demonstration uses it for
    /// the same reason.
    family_size: u32,
    /// Replicates handed to a worker at a time. Only the granularity of the
    /// work queue: the reduction below is chunk-ordered, so the results do not
    /// depend on it. Small values exist so a test can force many chunks over
    /// few replicates and actually exercise the ordering.
    work_chunk: usize,
}

impl Configuration {
    fn alpha(&self) -> f64 {
        ComparisonMethod::frozen(self.family_size).adjusted_alpha()
    }
}

/// The seed of one replicate. A pure function of the cell and the replicate
/// index, so the result does not depend on how many threads ran it.
fn replicate_seed(configuration: &Configuration, cell: &Cell, replicate: usize) -> u64 {
    let mut mixer = SplitMix64::new(configuration.seed);
    let mut mix = |value: u64| {
        mixer.state ^= value;
        mixer.next_u64()
    };
    let drift_key = (cell.drift * 1e9).round() as i64 as u64;
    let effect_key = (cell.true_effect * 1e9).round() as i64 as u64;
    mix(cell.executions as u64);
    mix(drift_key);
    mix(effect_key);
    mix(replicate as u64)
}

fn run_replicate(
    configuration: &Configuration,
    cell: &Cell,
    replicate: usize,
    applicable: &[(usize, Candidate)],
    tallies: &mut [Tally],
) {
    let model = DriftModel {
        between_execution_sd: cell.drift,
        within_execution_sd: configuration.within_execution_sd,
        samples_per_execution: SAMPLES_PER_EXECUTION,
        baseline_median_ns: BASELINE_MEDIAN_NS,
    };
    let mut normals = Normals::new(replicate_seed(configuration, cell, replicate));
    let baseline = model.side(&mut normals, model.baseline_median_ns, cell.executions);
    let candidate_side = model.side(
        &mut normals,
        model.baseline_median_ns * (1.0 + cell.true_effect),
        cell.executions,
    );
    // A benchmark key that varies with the replicate, so the simulation
    // averages over the method's OWN seed derivation instead of conditioning on
    // one bootstrap index stream. The derivation itself is untouched.
    let key = format!("m5/simulation/{replicate:06}");
    let alpha = configuration.alpha();
    let draw = shared_draw(&key, &baseline, &candidate_side, configuration.resamples);
    let samples = cell.executions * SAMPLES_PER_EXECUTION;

    for (slot, candidate) in applicable {
        let estimated = estimate(candidate, &draw, &baseline, &candidate_side, alpha);
        let minimum_detectable_ratio =
            (inverse_standard_normal_cdf(1.0 - alpha / 2.0).unwrap_or(Z_TWO_SIDED_95) + Z_POWER_80)
                * estimated.standard_error;
        let (verdict, reasons) = decide(&VerdictInput {
            baseline_completeness: MeasurementCompleteness::Complete,
            candidate_completeness: MeasurementCompleteness::Complete,
            baseline_samples: samples,
            candidate_samples: samples,
            baseline_executions: cell.executions,
            candidate_executions: cell.executions,
            baseline_median_ns: draw.baseline_median_ns,
            degenerate_dispersion: estimated.degenerate
                || fewer_than_two_distinct_values(&baseline.values, &candidate_side.values),
            family_beyond_resolution: false,
            unobservable_hardware: false,
            // The harness asks what the method WOULD decide if it were
            // qualified: that is the question the requalification exists to
            // answer, and scoring it through the shut gate would only ever
            // report `method_unqualified`. Production passes
            // METHOD_QUALIFIED_FOR_DIRECTION, and a test in the parent module
            // holds it to that.
            method_qualified: true,
            interval: estimated.interval,
            minimum_detectable_ratio,
        });

        let tally = &mut tallies[*slot];
        tally.replicates += 1;
        let (low, high) = estimated.interval;
        if low <= cell.true_effect && cell.true_effect <= high {
            tally.covered += 1;
        }
        tally.sums.width += high - low;
        tally.sums.mdr += minimum_detectable_ratio;
        tally.sums.effect += draw.observed_effect;
        if estimated.degenerate {
            tally.degenerate += 1;
        }
        match verdict {
            ComparisonVerdict::Regression => tally.regression += 1,
            ComparisonVerdict::Improvement => tally.improvement += 1,
            ComparisonVerdict::NoMaterialChange => tally.no_material_change += 1,
            ComparisonVerdict::Inconclusive => tally.inconclusive += 1,
        }
        let directional = matches!(
            verdict,
            ComparisonVerdict::Regression | ComparisonVerdict::Improvement
        );
        if directional {
            tally.directional_any += 1;
            let matches_truth = (cell.true_effect > 0.0
                && verdict == ComparisonVerdict::Regression)
                || (cell.true_effect < 0.0 && verdict == ComparisonVerdict::Improvement);
            if matches_truth {
                tally.directional_correct += 1;
            } else {
                tally.directional_wrong += 1;
            }
        }
        if cell.true_effect > 0.0 && low > MATERIAL_THRESHOLD_RATIO
            || cell.true_effect < 0.0 && high < -MATERIAL_THRESHOLD_RATIO
        {
            tally.interval_beyond_threshold += 1;
        }
        if cell.true_effect > 0.0 && low > 0.0 || cell.true_effect < 0.0 && high < 0.0 {
            tally.interval_excludes_zero += 1;
        }
        for reason in reasons {
            *tally.reasons.entry(reason_wire(reason)).or_default() += 1;
        }
    }
}

#[derive(Serialize)]
struct PointReport {
    candidate: String,
    estimator: String,
    executions_per_side: usize,
    bootstrap_resamples: u32,
    drift_sd: f64,
    true_effect: f64,
    replicates: u64,
    coverage: f64,
    verdict_regression: u64,
    verdict_improvement: u64,
    verdict_no_material_change: u64,
    verdict_inconclusive: u64,
    directional_rate: f64,
    directional_correct_rate: f64,
    directional_wrong_rate: f64,
    no_material_change_rate: f64,
    interval_beyond_threshold_rate: f64,
    interval_excludes_zero_rate: f64,
    degenerate_rate: f64,
    mean_interval_width: f64,
    mean_minimum_detectable_ratio: f64,
    mean_observed_effect: f64,
    inconclusive_reasons: BTreeMap<String, u64>,
}

#[derive(Serialize)]
struct CandidateReport {
    id: String,
    estimator: String,
    executions_per_side: usize,
    bootstrap_resamples: u32,
}

#[derive(Serialize)]
struct SimulationReport {
    schema: &'static str,
    mode: String,
    seed: u64,
    seed_hex: String,
    replicates_per_point: usize,
    threads: usize,
    wall_seconds: f64,
    drift_model: DriftModelReport,
    method_constants: MethodConstants,
    candidates: Vec<CandidateReport>,
    points: Vec<PointReport>,
}

#[derive(Serialize)]
struct DriftModelReport {
    family: &'static str,
    between_execution_sd_values: Vec<f64>,
    within_execution_sd: f64,
    within_execution_sd_source: &'static str,
    samples_per_execution: usize,
    baseline_median_ns: f64,
    true_effects: Vec<f64>,
    estimand: &'static str,
    positivity_floor: &'static str,
}

#[derive(Serialize)]
struct MethodConstants {
    comparison_method: &'static str,
    bootstrap_resamples: u32,
    bootstrap_seed_hex: String,
    confidence_level: f64,
    material_threshold_ratio: f64,
    min_executions_for_direction: usize,
    min_samples_for_inference: usize,
    /// The most independent executions the v2 dataset format can carry at all
    /// (`BenchmarkDataset::validate` rejects a larger `run_count`). Published
    /// here so a probe of the "more executions" lever can be read against the
    /// contract's own ceiling instead of an assumed one.
    benchmark_max_runs: u8,
    family_size: u32,
    adjusted_alpha: f64,
    detection_power: f64,
}

fn environment(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn numbers(name: &str, fallback: &[f64]) -> Vec<f64> {
    match environment(name) {
        Some(raw) => raw
            .split(',')
            .filter_map(|part| part.trim().parse::<f64>().ok())
            .collect(),
        None => fallback.to_vec(),
    }
}

fn counts(name: &str, fallback: &[usize]) -> Vec<usize> {
    match environment(name) {
        Some(raw) => raw
            .split(',')
            .filter_map(|part| part.trim().parse::<usize>().ok())
            .collect(),
        None => fallback.to_vec(),
    }
}

fn simulate(
    configuration: Configuration,
    execution_counts: &[usize],
    drifts: &[f64],
    effects: &[f64],
    threads: usize,
    mode: &str,
) -> SimulationReport {
    let started = Instant::now();
    let all = candidates(execution_counts);
    let mut cells = Vec::new();
    for &executions in execution_counts {
        for &drift in drifts {
            for &effect in effects {
                cells.push(Cell {
                    executions,
                    drift,
                    true_effect: effect,
                });
            }
        }
    }
    let slots = cells.len() * all.len();
    // Which candidates apply to which cell, resolved once.
    let applicable: Vec<Vec<(usize, Candidate)>> = cells
        .iter()
        .enumerate()
        .map(|(cell_index, cell)| {
            all.iter()
                .enumerate()
                .filter(|(_, candidate)| candidate.executions == cell.executions)
                .map(|(candidate_index, candidate)| {
                    (cell_index * all.len() + candidate_index, candidate.clone())
                })
                .collect()
        })
        .collect();

    let chunk_size = configuration.work_chunk.max(1);
    let chunks_per_cell = configuration.replicates.div_ceil(chunk_size);
    let total_chunks = cells.len() * chunks_per_cell;
    let next = AtomicUsize::new(0);
    let mut merged: Vec<Tally> = vec![Tally::default(); slots];
    // One entry per chunk actually run, carrying that chunk's float sums. The
    // integer counts merge in any order because integer addition IS
    // associative; these do not, so they are collected with the chunk index and
    // reduced in ascending chunk order below. That is what makes the reported
    // means a function of the seed alone and not of how the work was scheduled.
    let mut chunk_sums: Vec<(usize, Vec<Sums>)> = Vec::with_capacity(total_chunks);

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let next = &next;
            let cells = &cells;
            let applicable = &applicable;
            handles.push(scope.spawn(move || {
                let mut local: Vec<Tally> = vec![Tally::default(); slots];
                let mut local_sums: Vec<(usize, Vec<Sums>)> = Vec::new();
                loop {
                    let chunk = next.fetch_add(1, Ordering::Relaxed);
                    if chunk >= total_chunks {
                        break;
                    }
                    let cell_index = chunk / chunks_per_cell;
                    let within = chunk % chunks_per_cell;
                    let start = within * chunk_size;
                    let end = (start + chunk_size).min(configuration.replicates);
                    let Some(cell) = cells.get(cell_index) else {
                        continue;
                    };
                    let Some(candidates_here) = applicable.get(cell_index) else {
                        continue;
                    };
                    // Reset the float accumulators so this chunk's sums are its
                    // own; the integer counts keep accumulating into `local`.
                    for tally in &mut local {
                        tally.sums = Sums::default();
                    }
                    for replicate in start..end {
                        run_replicate(&configuration, cell, replicate, candidates_here, &mut local);
                    }
                    local_sums.push((chunk, local.iter().map(|tally| tally.sums).collect()));
                }
                (local, local_sums)
            }));
        }
        for handle in handles {
            if let Ok((local, local_sums)) = handle.join() {
                for (slot, tally) in local.iter().enumerate() {
                    if let Some(target) = merged.get_mut(slot) {
                        target.merge(tally);
                    }
                }
                chunk_sums.extend(local_sums);
            }
        }
    });

    chunk_sums.sort_unstable_by_key(|(chunk, _)| *chunk);
    for (_, sums) in &chunk_sums {
        for (slot, chunk_sum) in sums.iter().enumerate() {
            if let Some(target) = merged.get_mut(slot) {
                target.sums.add(chunk_sum);
            }
        }
    }

    let mut points = Vec::with_capacity(slots);
    for (cell_index, cell) in cells.iter().enumerate() {
        for (candidate_index, candidate) in all.iter().enumerate() {
            if candidate.executions != cell.executions {
                continue;
            }
            let Some(tally) = merged.get(cell_index * all.len() + candidate_index) else {
                continue;
            };
            if tally.replicates == 0 {
                continue;
            }
            let denominator = tally.replicates as f64;
            points.push(PointReport {
                candidate: candidate.id.to_owned(),
                estimator: candidate.estimator.wire().to_owned(),
                executions_per_side: candidate.executions,
                bootstrap_resamples: if candidate.estimator == Estimator::RandomEffects {
                    0
                } else {
                    configuration.resamples
                },
                drift_sd: cell.drift,
                true_effect: cell.true_effect,
                replicates: tally.replicates,
                coverage: tally.covered as f64 / denominator,
                verdict_regression: tally.regression,
                verdict_improvement: tally.improvement,
                verdict_no_material_change: tally.no_material_change,
                verdict_inconclusive: tally.inconclusive,
                directional_rate: tally.directional_any as f64 / denominator,
                directional_correct_rate: tally.directional_correct as f64 / denominator,
                directional_wrong_rate: tally.directional_wrong as f64 / denominator,
                no_material_change_rate: tally.no_material_change as f64 / denominator,
                interval_beyond_threshold_rate: tally.interval_beyond_threshold as f64
                    / denominator,
                interval_excludes_zero_rate: tally.interval_excludes_zero as f64 / denominator,
                degenerate_rate: tally.degenerate as f64 / denominator,
                mean_interval_width: tally.sums.width / denominator,
                mean_minimum_detectable_ratio: tally.sums.mdr / denominator,
                mean_observed_effect: tally.sums.effect / denominator,
                inconclusive_reasons: tally
                    .reasons
                    .iter()
                    .map(|(reason, count)| ((*reason).to_owned(), *count))
                    .collect(),
            });
        }
    }

    let method = ComparisonMethod::frozen(configuration.family_size);
    SimulationReport {
        schema: "rust-engineering-mcp.m5-02-method-simulation-run.v1",
        mode: mode.to_owned(),
        seed: configuration.seed,
        seed_hex: format!("{:#018x}", configuration.seed),
        replicates_per_point: configuration.replicates,
        threads,
        wall_seconds: started.elapsed().as_secs_f64(),
        drift_model: DriftModelReport {
            family: "gaussian random effects, multiplicative, symmetric and mean-zero at both levels",
            between_execution_sd_values: drifts.to_vec(),
            within_execution_sd: configuration.within_execution_sd,
            within_execution_sd_source: "median coefficient of variation of the per-iteration times in the eighteen sample.json files of fixtures/benchmark-datasets/criterion-{baseline,candidate}-{1,2,3}.tar (observed range 0.018-0.070, median 0.033)",
            samples_per_execution: SAMPLES_PER_EXECUTION,
            baseline_median_ns: BASELINE_MEDIAN_NS,
            true_effects: effects.to_vec(),
            estimand: "candidate population median / baseline population median - 1, exact by construction because both noise levels are symmetric and mean-zero",
            positivity_floor: "each multiplicative factor is floored at 0.01; at the widest drift point that needs a ten-sigma deviate and never fires",
        },
        method_constants: MethodConstants {
            comparison_method: COMPARISON_METHOD,
            bootstrap_resamples: BOOTSTRAP_RESAMPLES,
            bootstrap_seed_hex: format!("{BOOTSTRAP_SEED:#018x}"),
            confidence_level: CONFIDENCE_LEVEL,
            material_threshold_ratio: MATERIAL_THRESHOLD_RATIO,
            min_executions_for_direction: MIN_EXECUTIONS_FOR_DIRECTION,
            min_samples_for_inference: MIN_SAMPLES_FOR_INFERENCE,
            benchmark_max_runs: crate::benchmark::BENCHMARK_MAX_RUNS,
            family_size: configuration.family_size,
            adjusted_alpha: method.adjusted_alpha(),
            detection_power: DETECTION_POWER,
        },
        candidates: all
            .iter()
            .map(|candidate| CandidateReport {
                id: candidate.id.to_owned(),
                estimator: candidate.estimator.wire().to_owned(),
                executions_per_side: candidate.executions,
                bootstrap_resamples: if candidate.estimator == Estimator::RandomEffects {
                    0
                } else {
                    configuration.resamples
                },
            })
            .collect(),
        points,
    }
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct BudgetReport {
    schema: &'static str,
    seed_hex: String,
    candidate: String,
    executions_per_side: usize,
    family_size: usize,
    repetitions: usize,
    /// Seconds for ONE comparison of the whole family, averaged over the
    /// repetitions, exclusive of building the synthetic datasets.
    seconds_per_comparison: f64,
    seconds_total: f64,
    /// The public `compare` entry point on real `BenchmarkDataset`s, measured
    /// only for the shipped estimator because it is the only one `compare`
    /// implements.
    seconds_per_public_compare: Option<f64>,
}

fn budget(
    candidate: &Candidate,
    family_size: usize,
    repetitions: usize,
    seed: u64,
) -> BudgetReport {
    let configuration = Configuration {
        seed,
        replicates: 1,
        resamples: BOOTSTRAP_RESAMPLES,
        within_execution_sd: DEFAULT_WITHIN_EXECUTION_CV,
        family_size: u32::try_from(family_size).unwrap_or(1),
        work_chunk: WORK_CHUNK,
    };
    let cell = Cell {
        executions: candidate.executions,
        drift: 0.05,
        true_effect: 0.0,
    };
    let model = DriftModel {
        between_execution_sd: cell.drift,
        within_execution_sd: configuration.within_execution_sd,
        samples_per_execution: SAMPLES_PER_EXECUTION,
        baseline_median_ns: BASELINE_MEDIAN_NS,
    };
    let alpha = configuration.alpha();
    // Build every side first: the budget of a comparison is the comparison, not
    // the fabrication of its inputs.
    let mut sides = Vec::with_capacity(family_size);
    for benchmark in 0..family_size {
        let mut normals = Normals::new(seed ^ (benchmark as u64).wrapping_mul(0x9E37_79B9));
        let baseline = model.side(&mut normals, model.baseline_median_ns, cell.executions);
        let other = model.side(&mut normals, model.baseline_median_ns, cell.executions);
        sides.push((format!("m5/budget/{benchmark:03}"), baseline, other));
    }

    // The random-effects candidate never resamples, so charging it a bootstrap
    // would report a budget it does not spend. Every other candidate reads the
    // same cluster bootstrap the shipped estimator runs.
    let resamples = if candidate.estimator == Estimator::RandomEffects {
        0
    } else {
        configuration.resamples
    };

    let mut total = 0.0_f64;
    for _ in 0..repetitions {
        let started = Instant::now();
        for (key, baseline, other) in &sides {
            let draw = shared_draw(key, baseline, other, resamples);
            let estimated = estimate(candidate, &draw, baseline, other, alpha);
            let minimum_detectable_ratio = (inverse_standard_normal_cdf(1.0 - alpha / 2.0)
                .unwrap_or(Z_TWO_SIDED_95)
                + Z_POWER_80)
                * estimated.standard_error;
            let (verdict, _) = decide(&VerdictInput {
                baseline_completeness: MeasurementCompleteness::Complete,
                candidate_completeness: MeasurementCompleteness::Complete,
                baseline_samples: cell.executions * SAMPLES_PER_EXECUTION,
                candidate_samples: cell.executions * SAMPLES_PER_EXECUTION,
                baseline_executions: cell.executions,
                candidate_executions: cell.executions,
                baseline_median_ns: draw.baseline_median_ns,
                degenerate_dispersion: estimated.degenerate,
                family_beyond_resolution: false,
                unobservable_hardware: false,
                method_qualified: true,
                interval: estimated.interval,
                minimum_detectable_ratio,
            });
            // Consume the verdict so nothing above can be optimized away.
            if verdict == ComparisonVerdict::Regression && minimum_detectable_ratio < 0.0 {
                total += 1.0;
            }
        }
        total += started.elapsed().as_secs_f64();
    }

    let public = if candidate.estimator == Estimator::Percentile {
        let (baseline_dataset, candidate_dataset) =
            synthetic_datasets(&sides, cell.executions, family_size);
        let started = Instant::now();
        let report = compare(&baseline_dataset, &candidate_dataset);
        let elapsed = started.elapsed().as_secs_f64();
        if report.is_ok() { Some(elapsed) } else { None }
    } else {
        None
    };

    BudgetReport {
        schema: "rust-engineering-mcp.m5-02-method-budget.v1",
        seed_hex: format!("{seed:#018x}"),
        candidate: candidate.id.to_owned(),
        executions_per_side: candidate.executions,
        family_size,
        repetitions,
        seconds_per_comparison: total / repetitions as f64,
        seconds_total: total,
        seconds_per_public_compare: public,
    }
}

// ---------------------------------------------------------------------------
// Real datasets, for driving the public entry point
// ---------------------------------------------------------------------------

fn simulation_identity(name: &str) -> BenchmarkIdentity {
    BenchmarkIdentity::new(
        "m5".to_owned(),
        Some(name.to_owned()),
        None,
        format!("m5/{name}"),
        format!("m5_{name}"),
    )
    .unwrap()
}

fn simulation_provenance(execution: &str, run_count: u8) -> BenchmarkProvenance {
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
            bench_target: Some("perf".to_owned()),
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
        run_count,
        captured_at_unix: 1_757_000_000,
    }
}

/// Turns generated sides into two real [`BenchmarkDataset`]s, so the public
/// [`compare`] can be driven on exactly the data the harness judged.
fn synthetic_datasets(
    sides: &[(String, SideSamples, SideSamples)],
    executions: usize,
    family_size: usize,
) -> (BenchmarkDataset, BenchmarkDataset) {
    let run_count = u8::try_from(executions).unwrap_or(u8::MAX);
    let measurement = |name: &str, side: &SideSamples| {
        let mut samples = Vec::new();
        for (index, execution) in side.executions.iter().enumerate() {
            let run_index = u8::try_from(index + 1).unwrap_or(1);
            for value in execution {
                samples.push(RawSample::new(1, *value, run_index).unwrap());
            }
        }
        let count = u32::try_from(samples.len()).unwrap_or(u32::MAX);
        BenchmarkMeasurement::new(
            simulation_identity(name),
            SamplingMode::Flat,
            samples,
            3_000,
            5_000,
            count,
            MeasurementCompleteness::Complete,
        )
        .unwrap()
    };
    let mut baseline = Vec::with_capacity(family_size);
    let mut candidate = Vec::with_capacity(family_size);
    for (index, (_, left, right)) in sides.iter().enumerate() {
        let name = format!("bench{index:03}");
        baseline.push(measurement(&name, left));
        candidate.push(measurement(&name, right));
    }
    (
        BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            baseline,
            simulation_provenance("execution-baseline", run_count),
        )
        .unwrap(),
        BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            candidate,
            simulation_provenance("execution-candidate", run_count),
        )
        .unwrap(),
    )
}

// ---------------------------------------------------------------------------
// Tests, and the harness entry point
// ---------------------------------------------------------------------------

#[test]
fn student_t_quantiles_match_the_published_table() {
    for (probability, degrees, expected) in [
        (0.975_f64, 1.0_f64, 12.706_2_f64),
        (0.975, 2.0, 4.302_65),
        (0.975, 4.0, 2.776_45),
        (0.975, 9.0, 2.262_16),
        (0.975, 30.0, 2.042_27),
        (0.995, 2.0, 9.924_84),
        (0.9, 5.0, 1.475_88),
    ] {
        let value = student_t_quantile(probability, degrees);
        assert!(
            (value - expected).abs() < 1e-4,
            "t({probability}, {degrees}) = {value}, expected {expected}"
        );
    }
}

#[test]
fn standard_normal_cdf_matches_known_values() {
    for (x, expected) in [
        (0.0_f64, 0.5_f64),
        (1.0, 0.841_344_75),
        (-1.0, 0.158_655_25),
        (1.959_963_98, 0.975),
        (-2.575_829_3, 0.005),
    ] {
        let value = standard_normal_cdf(x);
        assert!(
            (value - expected).abs() < 2e-7,
            "phi({x}) = {value}, expected {expected}"
        );
    }
}

#[test]
fn the_cluster_shortfall_is_the_documented_factor() {
    assert!((cluster_shortfall(2) - std::f64::consts::SQRT_2).abs() < 1e-12);
    assert!((cluster_shortfall(3) - 1.224_744_871_391_589).abs() < 1e-12);
}

/// The one duplication in this file, held to the product byte for byte.
///
/// The local loop exists because [`bootstrap_ratio`] returns the finished
/// interval and the alternative estimators need the distribution behind it. If
/// the two ever drift apart, every number this harness reports about the
/// shipped estimator would describe something else, so the equality is asserted
/// rather than assumed — and asserted exactly, not to a tolerance.
#[test]
fn the_local_bootstrap_loop_reproduces_the_products_bootstrap_ratio() {
    let model = DriftModel {
        between_execution_sd: 0.05,
        within_execution_sd: DEFAULT_WITHIN_EXECUTION_CV,
        samples_per_execution: SAMPLES_PER_EXECUTION,
        baseline_median_ns: BASELINE_MEDIAN_NS,
    };
    for (index, alpha) in [(0_u64, 0.05_f64), (1, 0.002)] {
        let mut normals = Normals::new(SIMULATION_ROOT_SEED ^ index);
        let baseline = model.side(&mut normals, model.baseline_median_ns, 3);
        let candidate = model.side(&mut normals, model.baseline_median_ns * 1.05, 3);
        let key = format!("m5/equivalence/{index}");
        let product = bootstrap_ratio(&key, &baseline, &candidate, alpha);
        let local = shared_draw(&key, &baseline, &candidate, BOOTSTRAP_RESAMPLES);
        let reconstructed = (
            quantile_sorted(&local.sorted_ratios, alpha / 2.0),
            quantile_sorted(&local.sorted_ratios, 1.0 - alpha / 2.0),
        );
        assert_eq!(
            product.interval.0.total_cmp(&reconstructed.0),
            std::cmp::Ordering::Equal,
            "low endpoint {} vs {}",
            product.interval.0,
            reconstructed.0
        );
        assert_eq!(
            product.interval.1.total_cmp(&reconstructed.1),
            std::cmp::Ordering::Equal,
            "high endpoint {} vs {}",
            product.interval.1,
            reconstructed.1
        );
        assert_eq!(
            product.standard_error.total_cmp(&local.standard_error),
            std::cmp::Ordering::Equal,
            "standard error {} vs {}",
            product.standard_error,
            local.standard_error
        );
        assert_eq!(product.degenerate, local.degenerate);
    }
}

/// The harness's shipped-estimator path against the PUBLIC entry point.
///
/// `compare` is what the product runs: compatibility, `compare_one`,
/// `bootstrap_ratio`, `decide`. If the harness's `percentile` candidate agrees
/// with it on the interval, the minimum detectable ratio and the verdict, then
/// what the simulation measures is what the product does.
#[test]
fn the_harness_agrees_with_the_public_compare_on_the_shipped_estimator() {
    let model = DriftModel {
        between_execution_sd: 0.05,
        within_execution_sd: DEFAULT_WITHIN_EXECUTION_CV,
        samples_per_execution: SAMPLES_PER_EXECUTION,
        baseline_median_ns: BASELINE_MEDIAN_NS,
    };
    let candidate = Candidate {
        id: "percentile_k3".to_owned(),
        executions: 3,
        estimator: Estimator::Percentile,
    };
    for (index, effect) in [(0_u64, 0.0_f64), (1, 0.25)] {
        let mut normals = Normals::new(SIMULATION_ROOT_SEED ^ (index + 17));
        let baseline = model.side(&mut normals, model.baseline_median_ns, 3);
        let other = model.side(&mut normals, model.baseline_median_ns * (1.0 + effect), 3);
        let sides = vec![("m5/bench000".to_owned(), baseline, other)];
        let (baseline_dataset, candidate_dataset) = synthetic_datasets(&sides, 3, 1);
        let report = compare(&baseline_dataset, &candidate_dataset).unwrap();
        let published = report.comparisons.first().unwrap();

        // The public path derives the key from the benchmark identity; the
        // harness must use that same key or it draws a different stream.
        let key = published.key.clone();
        let (_, left, right) = sides.first().unwrap();
        let alpha = report.method.adjusted_alpha();
        let draw = shared_draw(&key, left, right, BOOTSTRAP_RESAMPLES);
        let estimated = estimate(&candidate, &draw, left, right, alpha);
        let minimum_detectable_ratio =
            (inverse_standard_normal_cdf(1.0 - alpha / 2.0).unwrap_or(Z_TWO_SIDED_95) + Z_POWER_80)
                * estimated.standard_error;
        let (verdict, _) = decide(&VerdictInput {
            baseline_completeness: MeasurementCompleteness::Complete,
            candidate_completeness: MeasurementCompleteness::Complete,
            baseline_samples: 90,
            candidate_samples: 90,
            baseline_executions: 3,
            candidate_executions: 3,
            baseline_median_ns: draw.baseline_median_ns,
            degenerate_dispersion: estimated.degenerate
                || fewer_than_two_distinct_values(&left.values, &right.values),
            family_beyond_resolution: false,
            unobservable_hardware: false,
            // The harness asks what the method WOULD decide if it were
            // qualified: that is the question the requalification exists to
            // answer, and scoring it through the shut gate would only ever
            // report `method_unqualified`. Production passes
            // METHOD_QUALIFIED_FOR_DIRECTION, and a test in the parent module
            // holds it to that.
            method_qualified: true,
            interval: estimated.interval,
            minimum_detectable_ratio,
        });

        assert_eq!(
            published
                .confidence_interval
                .0
                .total_cmp(&estimated.interval.0),
            std::cmp::Ordering::Equal,
            "{:?} vs {:?}",
            published.confidence_interval,
            estimated.interval
        );
        assert_eq!(
            published
                .confidence_interval
                .1
                .total_cmp(&estimated.interval.1),
            std::cmp::Ordering::Equal
        );
        assert_eq!(
            published
                .minimum_detectable_ratio
                .total_cmp(&minimum_detectable_ratio),
            std::cmp::Ordering::Equal
        );
        assert_eq!(published.verdict, verdict);
        assert_eq!(
            published.effect_ratio.total_cmp(&draw.observed_effect),
            std::cmp::Ordering::Equal
        );
    }
}

/// Several threads must produce the same tallies as one, to the LAST BIT, or
/// nothing the harness reports is reproducible from the seed alone.
///
/// `work_chunk: 1` is the point of this test. With the production chunk size a
/// small replicate count fits in a single chunk per cell, every cell is then
/// summed by one thread in one go, and the test passes without ever exercising
/// what it claims — which is exactly what happened until a re-run caught a
/// one-ULP disagreement in `mean_observed_effect`. One replicate per chunk
/// spreads a cell's chunks across threads, so the float sums agree only if the
/// reduction really is chunk-ordered.
#[test]
fn the_simulation_does_not_depend_on_the_thread_count() {
    let configuration = Configuration {
        work_chunk: 1,
        seed: SIMULATION_ROOT_SEED,
        replicates: 6,
        resamples: BOOTSTRAP_RESAMPLES,
        within_execution_sd: DEFAULT_WITHIN_EXECUTION_CV,
        family_size: 1,
    };
    let single = simulate(configuration, &[3], &[0.05], &[0.0], 1, "smoke");
    let parallel = simulate(configuration, &[3], &[0.05], &[0.0], 4, "smoke");
    assert_eq!(single.points.len(), parallel.points.len());
    for (left, right) in single.points.iter().zip(&parallel.points) {
        assert_eq!(left.candidate, right.candidate);
        assert_eq!(left.replicates, right.replicates);
        assert_eq!(
            left.coverage.total_cmp(&right.coverage),
            std::cmp::Ordering::Equal
        );
        assert_eq!(left.verdict_regression, right.verdict_regression);
        assert_eq!(left.verdict_inconclusive, right.verdict_inconclusive);
        for (name, one, many) in [
            (
                "mean_interval_width",
                left.mean_interval_width,
                right.mean_interval_width,
            ),
            (
                "mean_minimum_detectable_ratio",
                left.mean_minimum_detectable_ratio,
                right.mean_minimum_detectable_ratio,
            ),
            (
                "mean_observed_effect",
                left.mean_observed_effect,
                right.mean_observed_effect,
            ),
        ] {
            assert_eq!(
                one.total_cmp(&many),
                std::cmp::Ordering::Equal,
                "{name} on {}: one thread gave {one:?}, four gave {many:?}",
                left.candidate
            );
        }
    }
}

/// Every candidate must produce a finite interval and a decision at every drift
/// point, at a size the default gate can afford. The full run is the same code
/// with a larger replicate count.
#[test]
fn every_candidate_decides_at_every_drift_point() {
    let configuration = Configuration {
        seed: SIMULATION_ROOT_SEED,
        replicates: 1,
        resamples: BOOTSTRAP_RESAMPLES,
        within_execution_sd: DEFAULT_WITHIN_EXECUTION_CV,
        family_size: 1,
        work_chunk: WORK_CHUNK,
    };
    let report = simulate(
        configuration,
        &[3],
        &[0.0, 0.01, 0.02, 0.05, 0.10],
        &[0.0, 0.05],
        2,
        "smoke",
    );
    assert_eq!(report.points.len(), 5 * 2 * 5);
    for point in &report.points {
        assert!(point.coverage.is_finite(), "{}", point.candidate);
        assert!(point.mean_interval_width.is_finite(), "{}", point.candidate);
        assert!(
            point.mean_minimum_detectable_ratio >= 0.0,
            "{}",
            point.candidate
        );
        assert_eq!(
            point.verdict_regression
                + point.verdict_improvement
                + point.verdict_no_material_change
                + point.verdict_inconclusive,
            point.replicates
        );
    }
}

/// The requalification run. Prints one JSON document between two markers.
///
/// With no environment it is a smoke test of two replicates so the ordinary
/// gate keeps this file compiling and correct without paying for the full
/// simulation. `scripts/simulate-m5-comparison-method.py` sets the variables.
#[test]
fn m5_02_requalification() {
    let mode = environment("M5_SIM_MODE").unwrap_or_else(|| "smoke".to_owned());
    let seed = environment("M5_SIM_SEED")
        .and_then(|raw| {
            raw.strip_prefix("0x").map_or_else(
                || raw.parse::<u64>().ok(),
                |hex| u64::from_str_radix(hex, 16).ok(),
            )
        })
        .unwrap_or(SIMULATION_ROOT_SEED);
    let threads = counts("M5_SIM_THREADS", &[])
        .first()
        .copied()
        .or_else(|| std::thread::available_parallelism().ok().map(Into::into))
        .unwrap_or(1)
        .max(1);

    if mode == "budget" {
        let family_size = counts("M5_SIM_FAMILY", &[1]).first().copied().unwrap_or(1);
        let repetitions = counts("M5_SIM_REPETITIONS", &[1])
            .first()
            .copied()
            .unwrap_or(1);
        let wanted = environment("M5_SIM_CANDIDATE").unwrap_or_else(|| "percentile_k3".to_owned());
        let all = candidates(&[3, 5, 10]);
        let Some(candidate) = all.iter().find(|entry| entry.id == wanted) else {
            println!("{JSON_BEGIN}");
            println!("{{\"error\":\"unknown candidate\",\"requested\":\"{wanted}\"}}");
            println!("{JSON_END}");
            return;
        };
        let report = budget(candidate, family_size, repetitions, seed);
        println!("{JSON_BEGIN}");
        println!("{}", serde_json::to_string(&report).unwrap());
        println!("{JSON_END}");
        return;
    }

    // The smoke defaults keep the ordinary `cargo test` gate paying for a
    // plumbing check and not for the requalification: one replicate of one
    // execution count. The full grid is what the script asks for.
    let smoke = mode == "smoke";
    let replicates = counts("M5_SIM_REPLICATES", &[])
        .first()
        .copied()
        .unwrap_or(if smoke { 1 } else { DEFAULT_REPLICATES })
        .max(1);
    let execution_counts = counts("M5_SIM_EXECUTIONS", if smoke { &[3] } else { &[3, 5, 10] });
    let drifts = numbers(
        "M5_SIM_DRIFTS",
        if smoke {
            &[0.0, 0.05]
        } else {
            &[0.0, 0.01, 0.02, 0.05, 0.10]
        },
    );
    let effects = numbers(
        "M5_SIM_EFFECTS",
        if smoke {
            &[0.0, 0.05]
        } else {
            &[0.0, 0.05, 0.10, -0.10]
        },
    );
    let resamples = counts("M5_SIM_RESAMPLES", &[])
        .first()
        .copied()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(BOOTSTRAP_RESAMPLES);
    let within = numbers("M5_SIM_WITHIN_SD", &[DEFAULT_WITHIN_EXECUTION_CV])
        .first()
        .copied()
        .unwrap_or(DEFAULT_WITHIN_EXECUTION_CV);
    let family_size = counts("M5_SIM_FAMILY", &[1])
        .first()
        .copied()
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(1);

    let configuration = Configuration {
        seed,
        replicates,
        resamples,
        within_execution_sd: within,
        family_size,
        work_chunk: counts("M5_SIM_WORK_CHUNK", &[WORK_CHUNK])
            .first()
            .copied()
            .unwrap_or(WORK_CHUNK),
    };
    let report = simulate(
        configuration,
        &execution_counts,
        &drifts,
        &effects,
        threads,
        &mode,
    );
    println!("{JSON_BEGIN}");
    println!("{}", serde_json::to_string(&report).unwrap());
    println!("{JSON_END}");
}
