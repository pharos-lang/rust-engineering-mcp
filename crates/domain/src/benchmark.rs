//! Frozen, versioned benchmark dataset: raw samples plus the provenance a
//! reader needs before two datasets may be compared at all.
//!
//! The roadmap (docs/roadmap/m5-performance.md, "Método congelado antes de
//! medir") requires the dataset format to be independent of the server's own
//! SemVer and requires an unknown reader to fail closed: a payload whose
//! [`BenchmarkDataset::format`] is not exactly [`BENCHMARK_DATASET_FORMAT`], or
//! whose `format_version` is not `1`, is rejected by [`BenchmarkDataset::validate`]
//! and is never coerced, migrated or reinterpreted here.
//!
//! Nothing in this module measures anything. It only models what a measurement
//! run produced, and every field that was not observed stays `None`, meaning
//! UNKNOWN. An absent value is never replaced by a plausible default: the
//! comparison layer treats unknown hardware as a blocker, not as a match.
//!
//! Serialized values are described, never explained: this module records what a
//! harness reported and under which environment, and makes no claim about why
//! any number has the value it has.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::{error::Error, fmt};

/// Wire identity of the dataset format. Deliberately a full string and not a
/// bare integer: a foreign producer that reuses `format_version: 1` for its own
/// unrelated schema must still be rejected.
pub const BENCHMARK_DATASET_FORMAT: &str = "rust-engineering-mcp.benchmark-dataset.v1";

/// The only accepted `format_version`. A dataset carrying any other value is
/// rejected; migration either preserves the raw samples under a new reader or
/// requires a rerun, and never rewrites measurements into "equivalent" ones.
pub const BENCHMARK_DATASET_FORMAT_VERSION: u8 = 1;

/// Verified once during explicit M5 provisioning; never inferred from an
/// installed-file heuristic. A mismatch makes the harness unavailable, checked
/// before any benchmark command runs.
pub const APPROVED_CRITERION_VERSION: &str = "0.8.2";

/// Byte ceiling on every free text field carried by a dataset.
pub const BENCHMARK_MAX_TEXT: usize = 512;

/// Ceiling on the raw samples kept for one benchmark. A run that produced more
/// is `Truncated`, never silently thinned.
pub const BENCHMARK_MAX_SAMPLES: usize = 100_000;

/// Ceiling on independent repetitions of one candidate within a single
/// provenance record.
pub const BENCHMARK_MAX_RUNS: u8 = 16;

/// Every reason a benchmark value can be refused. Closed: a new rejection
/// reason is a deliberate contract change, never an opaque string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchmarkError {
    /// A required identity field was empty.
    EmptyIdentity,
    /// A text field exceeded [`BENCHMARK_MAX_TEXT`] bytes.
    IdentityTooLong,
    /// A text field carried a control character (NUL included).
    InvalidText,
    /// A measurement that is not `Missing` carried no samples.
    NoSamples,
    /// A measurement carried more than [`BENCHMARK_MAX_SAMPLES`] samples.
    TooManySamples,
    /// A raw sample was not a positive, finite, bounded duration over at least
    /// one iteration.
    InvalidSample,
    /// Two measurements in one dataset shared a benchmark key.
    DuplicateBenchmark,
    /// The `format` string or `format_version` was not the v1 contract.
    UnknownFormat,
    /// `run_index`/`run_count` were outside the 1-based closed range.
    InvalidRunIndex,
    /// A dataset carried no measurement at all.
    EmptyDataset,
    /// A required provenance field was empty.
    InvalidProvenance,
}

impl fmt::Display for BenchmarkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyIdentity => "benchmark identity field is empty",
            Self::IdentityTooLong => "benchmark text field exceeds its bounded length",
            Self::InvalidText => "benchmark text field carries a control character",
            Self::NoSamples => "benchmark measurement carries no sample",
            Self::TooManySamples => "benchmark measurement exceeds the sample ceiling",
            Self::InvalidSample => "raw sample is not a bounded positive duration",
            Self::DuplicateBenchmark => "dataset repeats a benchmark key",
            Self::UnknownFormat => "dataset format is not the v1 contract",
            Self::InvalidRunIndex => "run index is outside its 1-based run count",
            Self::EmptyDataset => "dataset carries no measurement",
            Self::InvalidProvenance => "provenance field is empty",
        })
    }
}
impl Error for BenchmarkError {}

/// Bounded, control-character-free text. Emptiness is checked separately so
/// each caller can report the field class it owns.
fn bounded_text(value: &str) -> Result<(), BenchmarkError> {
    if value.len() > BENCHMARK_MAX_TEXT {
        return Err(BenchmarkError::IdentityTooLong);
    }
    // `char::is_control` covers NUL and every other C0/C1 control character.
    if value.chars().any(char::is_control) {
        return Err(BenchmarkError::InvalidText);
    }
    Ok(())
}

fn required_text(value: &str, on_empty: BenchmarkError) -> Result<(), BenchmarkError> {
    if value.is_empty() {
        return Err(on_empty);
    }
    bounded_text(value)
}

fn optional_text(value: Option<&String>) -> Result<(), BenchmarkError> {
    match value {
        Some(text) => bounded_text(text),
        None => Ok(()),
    }
}

/// The harness that produced the samples. Closed: `cargo bench` accepts any
/// harness, but an unrecognized one may report execution and logs only. It can
/// never fabricate comparable measurements, so it has no variant here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkHarness {
    Criterion,
}

/// The unit every sample is expressed in. Closed and singular by design: unit
/// coercion between datasets is not a supported operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SampleUnit {
    Nanoseconds,
}

/// How the harness scaled iterations across samples, as reported by the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SamplingMode {
    Linear,
    Flat,
    Auto,
    /// The run did not report a mode. Unknown stays unknown.
    Unknown,
}

/// Whether the sample set is the whole set the run produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementCompleteness {
    /// Every sample the run produced is present.
    Complete,
    /// Samples were dropped against a declared ceiling; the set is a prefix of
    /// what ran, not a summary of it.
    Truncated,
    /// The benchmark was selected but produced no usable sample.
    Missing,
}

/// Observed virtualization of the measuring host.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Virtualization {
    /// Not observable in this environment. Unknown stays unknown.
    Unknown,
    Container,
    VirtualMachine,
    Bare,
}

/// The identity of one benchmark as the harness names it. `full_id` is the
/// stable key: it is what a later run must reproduce for two datasets to
/// describe the same benchmark.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BenchmarkIdentity {
    group_id: String,
    function_id: Option<String>,
    value_str: Option<String>,
    full_id: String,
    directory_name: String,
}

impl BenchmarkIdentity {
    /// `group_id`, `full_id` and `directory_name` are required; the two
    /// optional parts are absent when the harness did not report them, and an
    /// absent part is never reconstructed from the others.
    pub fn new(
        group_id: String,
        function_id: Option<String>,
        value_str: Option<String>,
        full_id: String,
        directory_name: String,
    ) -> Result<Self, BenchmarkError> {
        let identity = Self {
            group_id,
            function_id,
            value_str,
            full_id,
            directory_name,
        };
        identity.validate()?;
        Ok(identity)
    }

    pub fn validate(&self) -> Result<(), BenchmarkError> {
        required_text(&self.group_id, BenchmarkError::EmptyIdentity)?;
        required_text(&self.full_id, BenchmarkError::EmptyIdentity)?;
        required_text(&self.directory_name, BenchmarkError::EmptyIdentity)?;
        optional_text(self.function_id.as_ref())?;
        optional_text(self.value_str.as_ref())
    }

    /// The stable key used to match a benchmark across datasets.
    pub fn key(&self) -> &str {
        &self.full_id
    }
    pub fn group_id(&self) -> &str {
        &self.group_id
    }
    pub fn function_id(&self) -> Option<&str> {
        self.function_id.as_deref()
    }
    pub fn value_str(&self) -> Option<&str> {
        self.value_str.as_deref()
    }
    pub fn full_id(&self) -> &str {
        &self.full_id
    }
    pub fn directory_name(&self) -> &str {
        &self.directory_name
    }
}

/// One raw timing: the total time the harness spent running `iterations`
/// iterations. Raw samples are kept as reported; per-iteration time is derived,
/// never stored, so a later reader can recompute any statistic it needs.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct RawSample {
    iterations: u64,
    total_ns: f64,
}

impl RawSample {
    /// A single sample longer than ~11.5 days is not a benchmark measurement;
    /// the ceiling also keeps every derived sum inside f64's exact range.
    pub const MAX_TOTAL_NS: f64 = 1e15;

    pub fn new(iterations: u64, total_ns: f64) -> Result<Self, BenchmarkError> {
        let sample = Self {
            iterations,
            total_ns,
        };
        sample.validate()?;
        Ok(sample)
    }

    pub fn validate(&self) -> Result<(), BenchmarkError> {
        if self.iterations < 1
            || !self.total_ns.is_finite()
            || self.total_ns <= 0.0
            || self.total_ns > Self::MAX_TOTAL_NS
        {
            return Err(BenchmarkError::InvalidSample);
        }
        Ok(())
    }

    pub fn iterations(&self) -> u64 {
        self.iterations
    }
    pub fn total_ns(&self) -> f64 {
        self.total_ns
    }

    /// Derived per-iteration time. Meaningful only on a validated sample; a
    /// deserialized value must pass [`RawSample::validate`] first.
    pub fn per_iteration_ns(&self) -> f64 {
        self.total_ns / self.iterations as f64
    }
}

/// Every sample one benchmark produced in one run, with the request the run was
/// given. The requested sample size is kept next to the delivered samples so a
/// reader can see a shortfall instead of inferring one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BenchmarkMeasurement {
    identity: BenchmarkIdentity,
    sampling_mode: SamplingMode,
    samples: Vec<RawSample>,
    warm_up_ms: u64,
    measurement_ms: u64,
    sample_size_requested: u32,
    completeness: MeasurementCompleteness,
}

impl BenchmarkMeasurement {
    /// An empty sample set is only representable as `Missing`: a benchmark that
    /// reported nothing is never recorded as a complete measurement of nothing.
    pub fn new(
        identity: BenchmarkIdentity,
        sampling_mode: SamplingMode,
        samples: Vec<RawSample>,
        warm_up_ms: u64,
        measurement_ms: u64,
        sample_size_requested: u32,
        completeness: MeasurementCompleteness,
    ) -> Result<Self, BenchmarkError> {
        let measurement = Self {
            identity,
            sampling_mode,
            samples,
            warm_up_ms,
            measurement_ms,
            sample_size_requested,
            completeness,
        };
        measurement.validate()?;
        Ok(measurement)
    }

    pub fn validate(&self) -> Result<(), BenchmarkError> {
        self.identity.validate()?;
        if self.samples.len() > BENCHMARK_MAX_SAMPLES {
            return Err(BenchmarkError::TooManySamples);
        }
        if self.samples.is_empty() && self.completeness != MeasurementCompleteness::Missing {
            return Err(BenchmarkError::NoSamples);
        }
        for sample in &self.samples {
            sample.validate()?;
        }
        Ok(())
    }

    pub fn identity(&self) -> &BenchmarkIdentity {
        &self.identity
    }
    pub fn key(&self) -> &str {
        self.identity.key()
    }
    pub fn sampling_mode(&self) -> SamplingMode {
        self.sampling_mode
    }
    pub fn samples(&self) -> &[RawSample] {
        &self.samples
    }
    pub fn warm_up_ms(&self) -> u64 {
        self.warm_up_ms
    }
    pub fn measurement_ms(&self) -> u64 {
        self.measurement_ms
    }
    pub fn sample_size_requested(&self) -> u32 {
        self.sample_size_requested
    }
    pub fn completeness(&self) -> MeasurementCompleteness {
        self.completeness
    }
}

/// Resource ceilings the measuring runtime was placed under. `None` is UNKNOWN:
/// it never means "unlimited".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ResourceQuotas {
    pub cpu_quota_millicores: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub pids: Option<u32>,
}

/// What was observable about the measuring host. Every optional field is
/// UNKNOWN when absent and is never defaulted to a plausible value; the
/// comparison layer decides which unknowns block a comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct HardwareProfile {
    pub cpu_model: Option<String>,
    pub cpu_cores: Option<u16>,
    pub os_kernel: Option<String>,
    /// Always observable from the running build; required.
    pub arch: String,
    pub virtualization: Virtualization,
    pub cpu_governor: Option<String>,
    pub quotas: ResourceQuotas,
}

impl HardwareProfile {
    pub fn validate(&self) -> Result<(), BenchmarkError> {
        required_text(&self.arch, BenchmarkError::InvalidProvenance)?;
        optional_text(self.cpu_model.as_ref())?;
        optional_text(self.os_kernel.as_ref())?;
        optional_text(self.cpu_governor.as_ref())
    }
}

/// The closed build/target selection the run was executed under. It is compared
/// verbatim between datasets, so a producer normalizes `features` before
/// recording them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BenchmarkSelection {
    pub package: Option<String>,
    pub bench_target: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub profile: String,
}

impl BenchmarkSelection {
    pub fn validate(&self) -> Result<(), BenchmarkError> {
        required_text(&self.profile, BenchmarkError::InvalidProvenance)?;
        optional_text(self.package.as_ref())?;
        optional_text(self.bench_target.as_ref())?;
        for feature in &self.features {
            required_text(feature, BenchmarkError::InvalidProvenance)?;
        }
        Ok(())
    }
}

/// Everything a reader needs to decide whether two datasets are comparable.
///
/// `source_fingerprint` is expected to differ between a baseline and a
/// candidate: that difference is the subject of the comparison, not an
/// obstacle to it. Every other field describes the method and the environment
/// and is expected to match.
///
/// Fields are public because the record has no cross-field invariant beyond the
/// run counters; [`BenchmarkProvenance::validate`] is the single validator and
/// is always reached through [`BenchmarkDataset::validate`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BenchmarkProvenance {
    /// Digest of the measured source. Baseline and candidate differ here.
    pub source_fingerprint: String,
    pub harness: BenchmarkHarness,
    pub harness_version: String,
    pub rust_version: String,
    pub cargo_version: String,
    /// The toolchain the project declared, when it declared one.
    pub declared_toolchain: Option<String>,
    pub image_digest: String,
    pub platform: String,
    /// Digest over the frozen run configuration.
    pub configuration_fingerprint: String,
    /// Identifies this exact execution. Two datasets sharing it are the same
    /// artifact, not two observations.
    pub execution_fingerprint: String,
    pub selection: BenchmarkSelection,
    pub hardware: HardwareProfile,
    /// 1-based position of this run within `run_count` independent repetitions.
    pub run_index: u8,
    pub run_count: u8,
    pub captured_at_unix: u64,
}

impl BenchmarkProvenance {
    pub fn validate(&self) -> Result<(), BenchmarkError> {
        for field in [
            &self.source_fingerprint,
            &self.harness_version,
            &self.rust_version,
            &self.cargo_version,
            &self.image_digest,
            &self.platform,
            &self.configuration_fingerprint,
            &self.execution_fingerprint,
        ] {
            required_text(field, BenchmarkError::InvalidProvenance)?;
        }
        optional_text(self.declared_toolchain.as_ref())?;
        if self.run_count < 1
            || self.run_count > BENCHMARK_MAX_RUNS
            || self.run_index < 1
            || self.run_index > self.run_count
        {
            return Err(BenchmarkError::InvalidRunIndex);
        }
        self.selection.validate()?;
        self.hardware.validate()
    }
}

/// One frozen dataset: raw samples for one or more benchmarks, produced by one
/// execution, plus that execution's provenance.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct BenchmarkDataset {
    format_version: u8,
    format: String,
    unit: SampleUnit,
    measurements: Vec<BenchmarkMeasurement>,
    provenance: BenchmarkProvenance,
}

impl BenchmarkDataset {
    /// Producer path. The format identity is stamped by this crate and is never
    /// taken from a caller, so a dataset this server emits cannot claim a
    /// format it does not implement.
    pub fn new(
        unit: SampleUnit,
        measurements: Vec<BenchmarkMeasurement>,
        provenance: BenchmarkProvenance,
    ) -> Result<Self, BenchmarkError> {
        let dataset = Self {
            format_version: BENCHMARK_DATASET_FORMAT_VERSION,
            format: BENCHMARK_DATASET_FORMAT.to_owned(),
            unit,
            measurements,
            provenance,
        };
        dataset.validate()?;
        Ok(dataset)
    }

    /// Total structural validation, callable after deserialization.
    ///
    /// An unknown `format`/`format_version` is rejected outright: a reader that
    /// does not implement a payload's format fails closed rather than reading
    /// the fields it happens to recognize.
    pub fn validate(&self) -> Result<(), BenchmarkError> {
        if self.format != BENCHMARK_DATASET_FORMAT
            || self.format_version != BENCHMARK_DATASET_FORMAT_VERSION
        {
            return Err(BenchmarkError::UnknownFormat);
        }
        if self.measurements.is_empty() {
            return Err(BenchmarkError::EmptyDataset);
        }
        let mut seen = BTreeSet::new();
        for measurement in &self.measurements {
            measurement.validate()?;
            if !seen.insert(measurement.key()) {
                return Err(BenchmarkError::DuplicateBenchmark);
            }
        }
        self.provenance.validate()
    }

    pub fn measurement(&self, key: &str) -> Option<&BenchmarkMeasurement> {
        self.measurements
            .iter()
            .find(|measurement| measurement.key() == key)
    }

    pub fn format_version(&self) -> u8 {
        self.format_version
    }
    pub fn format(&self) -> &str {
        &self.format
    }
    pub fn unit(&self) -> SampleUnit {
        self.unit
    }
    pub fn measurements(&self) -> &[BenchmarkMeasurement] {
        &self.measurements
    }
    pub fn provenance(&self) -> &BenchmarkProvenance {
        &self.provenance
    }
}

/// Classification of `cargo bench` exit codes.
///
/// Every mapping below is a documented HYPOTHESIS taken from the Cargo and
/// Criterion documentation. It is NOT confirmed against the pinned guest
/// binary, so [`BenchmarkExit::CALIBRATED`] is `false` and no exit code alone
/// may promote a run to a usable measurement: a calibration receipt has to pin
/// these before a dataset produced under them is trusted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkExit {
    /// Hypothesis: 0, every selected benchmark ran to completion.
    Passed,
    /// Hypothesis: 101, a benchmark itself failed while running.
    BenchmarkFailed,
    /// Hypothesis: 100, the bench target did not build.
    CompilationFailed,
    /// Any other code; no calibration has pinned a meaning for it.
    Uncalibrated,
    /// Not produced by an exit code. Reserved for application policy when the
    /// run terminated but the sample set is not whole.
    Incomplete,
}

impl BenchmarkExit {
    /// `false` until a Docker calibration receipt records the observed codes.
    pub const CALIBRATED: bool = false;

    pub fn classify(code: i32) -> Self {
        match code {
            0 => Self::Passed,
            101 => Self::BenchmarkFailed,
            100 => Self::CompilationFailed,
            _ => Self::Uncalibrated,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;

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

    fn samples(count: usize) -> Vec<RawSample> {
        (0..count)
            .map(|index| RawSample::new(1, 1_000.0 + index as f64).unwrap())
            .collect()
    }

    fn measurement(name: &str) -> BenchmarkMeasurement {
        BenchmarkMeasurement::new(
            identity(name),
            SamplingMode::Flat,
            samples(12),
            3_000,
            5_000,
            12,
            MeasurementCompleteness::Complete,
        )
        .unwrap()
    }

    fn hardware() -> HardwareProfile {
        HardwareProfile {
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
        }
    }

    fn selection() -> BenchmarkSelection {
        BenchmarkSelection {
            package: Some("member".to_owned()),
            bench_target: Some("throughput".to_owned()),
            features: vec!["std".to_owned()],
            all_features: false,
            no_default_features: false,
            profile: "bench".to_owned(),
        }
    }

    fn provenance(source: &str, execution: &str) -> BenchmarkProvenance {
        BenchmarkProvenance {
            source_fingerprint: source.to_owned(),
            harness: BenchmarkHarness::Criterion,
            harness_version: APPROVED_CRITERION_VERSION.to_owned(),
            rust_version: "1.98.1".to_owned(),
            cargo_version: "1.98.1".to_owned(),
            declared_toolchain: Some("1.98.1".to_owned()),
            image_digest: format!("sha256:{}", "c".repeat(64)),
            platform: "aarch64-unknown-linux-gnu".to_owned(),
            configuration_fingerprint: format!("sha256:{}", "d".repeat(64)),
            execution_fingerprint: execution.to_owned(),
            selection: selection(),
            hardware: hardware(),
            run_index: 1,
            run_count: 3,
            captured_at_unix: 1_757_000_000,
        }
    }

    fn dataset() -> BenchmarkDataset {
        BenchmarkDataset::new(
            SampleUnit::Nanoseconds,
            vec![measurement("bench/one")],
            provenance("sha256:aa", "sha256:run-1"),
        )
        .unwrap()
    }

    #[test]
    fn empty_identity_field_is_rejected() {
        for parts in [
            (String::new(), "full".to_owned(), "dir".to_owned()),
            ("group".to_owned(), String::new(), "dir".to_owned()),
            ("group".to_owned(), "full".to_owned(), String::new()),
        ] {
            assert_eq!(
                BenchmarkIdentity::new(parts.0, None, None, parts.1, parts.2).unwrap_err(),
                BenchmarkError::EmptyIdentity
            );
        }
    }

    #[test]
    fn identity_text_beyond_the_ceiling_is_rejected() {
        let long = "a".repeat(BENCHMARK_MAX_TEXT + 1);
        assert_eq!(
            BenchmarkIdentity::new(long.clone(), None, None, "full".into(), "dir".into())
                .unwrap_err(),
            BenchmarkError::IdentityTooLong
        );
        assert_eq!(
            BenchmarkIdentity::new(
                "group".into(),
                Some(long),
                None,
                "full".into(),
                "dir".into()
            )
            .unwrap_err(),
            BenchmarkError::IdentityTooLong
        );
        // Exactly at the ceiling is accepted.
        assert!(
            BenchmarkIdentity::new(
                "a".repeat(BENCHMARK_MAX_TEXT),
                None,
                None,
                "full".into(),
                "dir".into()
            )
            .is_ok()
        );
    }

    #[test]
    fn control_characters_in_identity_text_are_rejected() {
        for text in ["group\0id", "group\nid", "group\u{7f}id", "group\u{9b}id"] {
            assert_eq!(
                BenchmarkIdentity::new(text.into(), None, None, "full".into(), "dir".into())
                    .unwrap_err(),
                BenchmarkError::InvalidText,
                "{text:?}"
            );
        }
    }

    #[test]
    fn identity_key_is_the_full_id() {
        let identity = identity("group/bench/2");
        assert_eq!(identity.key(), "group/bench/2");
        assert_eq!(identity.key(), identity.full_id());
        assert_eq!(identity.group_id(), "group");
        assert_eq!(identity.function_id(), Some("function"));
        assert_eq!(identity.value_str(), None);
        assert_eq!(identity.directory_name(), "group_bench_2");
    }

    #[test]
    fn raw_sample_rejects_every_non_positive_bounded_duration() {
        for (iterations, total_ns) in [
            (0, 1_000.0),
            (1, 0.0),
            (1, -1.0),
            (1, f64::NAN),
            (1, f64::INFINITY),
            (1, RawSample::MAX_TOTAL_NS * 10.0),
        ] {
            assert_eq!(
                RawSample::new(iterations, total_ns).unwrap_err(),
                BenchmarkError::InvalidSample,
                "{iterations} {total_ns}"
            );
        }
        let sample = RawSample::new(4, 1_000.0).unwrap();
        assert_eq!(sample.iterations(), 4);
        assert_eq!(sample.total_ns(), 1_000.0);
        assert_eq!(sample.per_iteration_ns(), 250.0);
    }

    #[test]
    fn measurement_without_samples_is_only_representable_as_missing() {
        for completeness in [
            MeasurementCompleteness::Complete,
            MeasurementCompleteness::Truncated,
        ] {
            assert_eq!(
                BenchmarkMeasurement::new(
                    identity("bench"),
                    SamplingMode::Auto,
                    Vec::new(),
                    0,
                    0,
                    0,
                    completeness,
                )
                .unwrap_err(),
                BenchmarkError::NoSamples
            );
        }
        assert!(
            BenchmarkMeasurement::new(
                identity("bench"),
                SamplingMode::Auto,
                Vec::new(),
                0,
                0,
                30,
                MeasurementCompleteness::Missing,
            )
            .is_ok()
        );
    }

    #[test]
    fn measurement_beyond_the_sample_ceiling_is_rejected() {
        let sample = RawSample::new(1, 10.0).unwrap();
        assert_eq!(
            BenchmarkMeasurement::new(
                identity("bench"),
                SamplingMode::Linear,
                vec![sample; BENCHMARK_MAX_SAMPLES + 1],
                0,
                0,
                0,
                MeasurementCompleteness::Complete,
            )
            .unwrap_err(),
            BenchmarkError::TooManySamples
        );
    }

    #[test]
    fn dataset_rejects_a_duplicate_benchmark_key() {
        assert_eq!(
            BenchmarkDataset::new(
                SampleUnit::Nanoseconds,
                vec![measurement("bench/one"), measurement("bench/one")],
                provenance("sha256:aa", "sha256:run-1"),
            )
            .unwrap_err(),
            BenchmarkError::DuplicateBenchmark
        );
    }

    #[test]
    fn dataset_rejects_an_empty_measurement_list() {
        assert_eq!(
            BenchmarkDataset::new(
                SampleUnit::Nanoseconds,
                Vec::new(),
                provenance("sha256:aa", "sha256:run-1"),
            )
            .unwrap_err(),
            BenchmarkError::EmptyDataset
        );
    }

    #[test]
    fn provenance_run_counters_are_one_based_and_bounded() {
        for (index, count) in [(0, 3), (4, 3), (1, 0), (1, BENCHMARK_MAX_RUNS + 1)] {
            let mut record = provenance("sha256:aa", "sha256:run-1");
            record.run_index = index;
            record.run_count = count;
            assert_eq!(
                record.validate().unwrap_err(),
                BenchmarkError::InvalidRunIndex,
                "{index}/{count}"
            );
        }
    }

    #[test]
    fn provenance_rejects_every_empty_required_field() {
        let mutations: Vec<fn(&mut BenchmarkProvenance)> = vec![
            |record| record.source_fingerprint.clear(),
            |record| record.harness_version.clear(),
            |record| record.rust_version.clear(),
            |record| record.cargo_version.clear(),
            |record| record.image_digest.clear(),
            |record| record.platform.clear(),
            |record| record.configuration_fingerprint.clear(),
            |record| record.execution_fingerprint.clear(),
            |record| record.selection.profile.clear(),
            |record| record.hardware.arch.clear(),
        ];
        for mutate in mutations {
            let mut record = provenance("sha256:aa", "sha256:run-1");
            mutate(&mut record);
            assert_eq!(
                record.validate().unwrap_err(),
                BenchmarkError::InvalidProvenance
            );
        }
    }

    #[test]
    fn absent_hardware_fields_stay_none_after_a_round_trip() -> Result<(), serde_json::Error> {
        let mut record = provenance("sha256:aa", "sha256:run-1");
        record.hardware.cpu_model = None;
        record.hardware.cpu_cores = None;
        record.hardware.os_kernel = None;
        record.hardware.cpu_governor = None;
        record.hardware.virtualization = Virtualization::Unknown;
        record.hardware.quotas = ResourceQuotas::default();
        let decoded: BenchmarkProvenance = serde_json::from_str(&serde_json::to_string(&record)?)?;
        assert_eq!(decoded, record);
        assert_eq!(decoded.hardware.cpu_model, None);
        assert_eq!(decoded.hardware.quotas.memory_bytes, None);
        assert!(decoded.validate().is_ok());
        Ok(())
    }

    #[test]
    fn dataset_round_trips_through_serde() -> Result<(), serde_json::Error> {
        let dataset = dataset();
        let encoded = serde_json::to_string(&dataset)?;
        let decoded: BenchmarkDataset = serde_json::from_str(&encoded)?;
        assert_eq!(decoded, dataset);
        assert!(decoded.validate().is_ok());
        assert_eq!(decoded.format(), BENCHMARK_DATASET_FORMAT);
        assert_eq!(decoded.format_version(), 1);
        assert_eq!(decoded.unit(), SampleUnit::Nanoseconds);
        assert_eq!(decoded.measurements().len(), 1);
        assert!(decoded.measurement("bench/one").is_some());
        assert!(decoded.measurement("bench/absent").is_none());
        Ok(())
    }

    #[test]
    fn deserialization_denies_an_unknown_field() -> Result<(), serde_json::Error> {
        let encoded = serde_json::to_string(&dataset())?;
        let injected = encoded.replacen('{', r#"{"vendor_extension":1,"#, 1);
        assert!(serde_json::from_str::<BenchmarkDataset>(&injected).is_err());
        Ok(())
    }

    #[test]
    fn an_unknown_format_or_version_fails_closed() -> Result<(), serde_json::Error> {
        let encoded = serde_json::to_string(&dataset())?;

        let future_format = encoded.replace(
            BENCHMARK_DATASET_FORMAT,
            "rust-engineering-mcp.benchmark-dataset.v2",
        );
        let decoded: BenchmarkDataset = serde_json::from_str(&future_format)?;
        assert_eq!(
            decoded.validate().unwrap_err(),
            BenchmarkError::UnknownFormat
        );

        let future_version = encoded.replacen(r#""format_version":1"#, r#""format_version":2"#, 1);
        let decoded: BenchmarkDataset = serde_json::from_str(&future_version)?;
        assert_eq!(
            decoded.validate().unwrap_err(),
            BenchmarkError::UnknownFormat
        );
        Ok(())
    }

    #[test]
    fn enum_tokens_are_snake_case_and_closed() -> Result<(), serde_json::Error> {
        assert_eq!(
            serde_json::to_string(&SampleUnit::Nanoseconds)?,
            "\"nanoseconds\""
        );
        assert_eq!(
            serde_json::to_string(&BenchmarkHarness::Criterion)?,
            "\"criterion\""
        );
        assert_eq!(
            serde_json::to_string(&Virtualization::VirtualMachine)?,
            "\"virtual_machine\""
        );
        assert_eq!(
            serde_json::to_string(&MeasurementCompleteness::Truncated)?,
            "\"truncated\""
        );
        for token in ["\"microseconds\"", "\"seconds\"", "\"Nanoseconds\""] {
            assert!(
                serde_json::from_str::<SampleUnit>(token).is_err(),
                "{token}"
            );
        }
        assert!(serde_json::from_str::<BenchmarkHarness>("\"iai\"").is_err());
        Ok(())
    }

    #[test]
    fn exit_classification_is_an_uncalibrated_hypothesis() {
        assert_eq!(BenchmarkExit::classify(0), BenchmarkExit::Passed);
        assert_eq!(BenchmarkExit::classify(101), BenchmarkExit::BenchmarkFailed);
        assert_eq!(
            BenchmarkExit::classify(100),
            BenchmarkExit::CompilationFailed
        );
        for other in [1, 2, 99, 102, -1, i32::MAX, i32::MIN] {
            assert_eq!(
                BenchmarkExit::classify(other),
                BenchmarkExit::Uncalibrated,
                "{other}"
            );
        }
        const { assert!(!BenchmarkExit::CALIBRATED) };
    }

    #[test]
    fn approved_harness_version_is_pinned() {
        assert_eq!(APPROVED_CRITERION_VERSION, "0.8.2");
        assert_eq!(BENCHMARK_DATASET_FORMAT_VERSION, 1);
        assert!(BENCHMARK_DATASET_FORMAT.ends_with(".v1"));
    }
}
