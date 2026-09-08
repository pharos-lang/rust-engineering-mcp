//! M5-02: `rust.benchmark.compare`, a comparison of two authorized datasets.
//!
//! This tool runs no process, creates no container and touches no project
//! source (ADR-076 §4), so it has no `execution_mode`: it is bounded arithmetic
//! over bytes the owning project already published. Both identifiers are opaque
//! and were emitted by the store; an artifact owned by another project simply
//! does not exist for this call.
#[allow(dead_code)]
mod schemas;
use super::{
    project::Registry,
    security_tool::{
        define_fallible_security_outcome, define_security_response_methods, define_security_tool,
        encode_bounded,
    },
    workers::{WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::QualityArtifactStore;
use rust_engineering_application::benchmark_compare::{
    BenchmarkCompareError, COMPARE_MAX_DATASET_BYTES, CompareOutcome, CompareRequest,
    DatasetDecoder,
};
use rust_engineering_domain::benchmark::{BenchmarkDataset, BenchmarkError};
use rust_engineering_domain::benchmark_compare::{
    BenchmarkComparison, ComparisonMethod, ComparisonReport, ComparisonStatistic,
    ComparisonVerdict, IncompatibilityReason, InconclusiveReason, MultiplicityCorrection,
    OutlierPolicy,
};
use rust_engineering_domain::{ProjectRef, QualityArtifactId};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

pub(super) const NAME: &str = "rust.benchmark.compare";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;
const MAX_RESPONSE_COMPARISONS: usize = 512;
const MAX_RESPONSE_KEYS: usize = 256;

pub(super) fn advertised() -> bool {
    super::security_tool::advertised("RUST_MCP_TEST_BENCHMARK_COMPARE_READY")
}

#[derive(Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    /// Opaque, store-issued and not composable by the peer.
    #[schemars(with = "String", regex(pattern = "^qart_[0-9a-f]{32}$"))]
    baseline_artifact_id: QualityArtifactId,
    #[schemars(with = "String", regex(pattern = "^qart_[0-9a-f]{32}$"))]
    candidate_artifact_id: QualityArtifactId,
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 30))]
    timeout_seconds: u64,
}
fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}

#[derive(Clone, Copy, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    SandboxDenied,
    ArtifactUnavailable,
    ArtifactNotFound,
    ArtifactUnreadable,
    ArtifactTooLarge,
    NotADataset,
    InvalidDataset,
    NoCommonBenchmark,
    IncompatibleDatasets,
    CommandTimeout,
    OutputLimitExceeded,
    EvidenceIncomplete,
}
define_fallible_security_outcome!(Code, &'static str, ());

#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    semantics: &'static str,
    #[schemars(regex(pattern = "^qart_[0-9a-f]{32}$"))]
    baseline_artifact_id: String,
    #[schemars(regex(pattern = "^qart_[0-9a-f]{32}$"))]
    candidate_artifact_id: String,
    report: schemas::Report,
}

/// The runtime this tool reads through.
///
/// `store` is optional because the durable quality artifact store is attached
/// only when the host qualified a state root. Without it the tool answers a
/// declared `unavailable`, never a protocol error.
pub(super) struct Runtime {
    pub(super) registry: Arc<Mutex<Registry>>,
    pub(super) workers: Workers,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) store: Option<Arc<Mutex<dyn QualityArtifactStore>>>,
}

/// The application takes a sized store; this forwards a shared handle into that
/// shape without asking the application to know about `dyn`.
struct DynStore<'a>(&'a mut dyn QualityArtifactStore);
impl QualityArtifactStore for DynStore<'_> {
    fn owner_binding(
        &self,
        facts: &rust_engineering_application::QualityOwnerFacts,
    ) -> Result<[u8; 32], rust_engineering_domain::QualityArtifactError> {
        self.0.owner_binding(facts)
    }
    fn reserve(
        &mut self,
        reservation: &rust_engineering_application::QualityReservation,
    ) -> Result<(), rust_engineering_domain::QualityArtifactError> {
        self.0.reserve(reservation)
    }
    fn release(
        &mut self,
        reservation: &rust_engineering_application::QualityReservation,
    ) -> Result<(), rust_engineering_domain::QualityArtifactError> {
        self.0.release(reservation)
    }
    fn ingest_member(
        &mut self,
        reservation: &rust_engineering_application::QualityReservation,
        member_index: u16,
        member_cap_bytes: u64,
        input: &mut dyn rust_engineering_application::QualityArtifactInput,
    ) -> Result<rust_engineering_application::QualityIngest, rust_engineering_domain::QualityArtifactError>
    {
        self.0
            .ingest_member(reservation, member_index, member_cap_bytes, input)
    }
    fn publish_descriptor(
        &mut self,
        reservation: &rust_engineering_application::QualityReservation,
        descriptor: &rust_engineering_domain::QualityArtifactDescriptor,
    ) -> Result<(), rust_engineering_domain::QualityArtifactError> {
        self.0.publish_descriptor(reservation, descriptor)
    }
    fn read_chunk(
        &mut self,
        owner_binding: [u8; 32],
        artifact_id: &QualityArtifactId,
        offset: u64,
        length: u32,
    ) -> Result<
        rust_engineering_application::QualityArtifactChunk,
        rust_engineering_domain::QualityArtifactError,
    > {
        self.0
            .read_chunk(owner_binding, artifact_id, offset, length)
    }
    fn read_index_page(
        &mut self,
        owner_binding: [u8; 32],
        job_id: &rust_engineering_domain::QualityJobId,
        cursor: Option<&[u8]>,
    ) -> Result<
        rust_engineering_application::QualityArtifactIndexPage,
        rust_engineering_domain::QualityArtifactError,
    > {
        self.0.read_index_page(owner_binding, job_id, cursor)
    }
    fn reconcile_recover(
        &mut self,
    ) -> Result<rust_engineering_domain::RecoveryReport, rust_engineering_domain::QualityArtifactError>
    {
        self.0.reconcile_recover()
    }
    fn prune_expired(
        &mut self,
    ) -> Result<rust_engineering_domain::PruneReport, rust_engineering_domain::QualityArtifactError>
    {
        self.0.prune_expired()
    }
}

/// The dataset wire format is JSON, and the application crate may depend only
/// on the domain (`scripts/check-architecture.py`), so the decoder lives on
/// this side of the port. It fails closed: an oversize payload, a payload that
/// is not this format, and a payload that claims a format version this reader
/// does not implement are all refused before a single field is believed.
struct JsonDatasetDecoder;
impl DatasetDecoder for JsonDatasetDecoder {
    fn decode(&self, bytes: &[u8]) -> Result<BenchmarkDataset, BenchmarkCompareError> {
        if bytes.len() as u64 > COMPARE_MAX_DATASET_BYTES {
            return Err(BenchmarkCompareError::ArtifactTooLarge);
        }
        let dataset: BenchmarkDataset = serde_json::from_slice(bytes)
            .map_err(|_| BenchmarkCompareError::InvalidDataset(BenchmarkError::UnknownFormat))?;
        dataset
            .validate()
            .map_err(BenchmarkCompareError::InvalidDataset)?;
        Ok(dataset)
    }
}

/// What ended one comparison without a report.
enum Failure {
    Body(BenchmarkCompareError),
    Timeout,
    Cancelled,
    Worker,
}

define_security_tool!(
    ComparisonTool,
    "Compare two benchmark datasets this project already published, by their opaque store identifiers. Runs no process and reads no project source, so it has no execution mode. Publishes the complete frozen method (median of per-iteration time, percentile bootstrap, fixed seed, confidence level, multiplicity correction, material threshold, outliers counted and never removed) and, per shared benchmark, the verdict, effect ratio, interval, both medians, both sample counts, both outlier counts, the minimum detectable ratio and the reasons any verdict was withheld. Datasets that do not describe comparable executions are an observed result carrying the complete sorted reason list, not an infrastructure failure. The result describes these two executions on this host: it attributes no cause, generalizes to no other hardware and recommends nothing."
);

impl ComparisonTool {
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call_with_token(request, context.ct).await
    }

    async fn call_with_token(
        &self,
        request: CallToolRequestParams,
        request_token: tokio_util::sync::CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ErrorData::internal_error("Comparison runtime is not configured", None)
        })?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Discovery must complete before comparing",
                None,
                0,
            );
        }
        let Some(store) = runtime.store.clone() else {
            return self.unavailable(
                Code::ArtifactUnavailable,
                "Durable benchmark evidence is unavailable",
                0,
            );
        };
        let registry = Arc::clone(&runtime.registry);
        let reference = input.project_ref.clone();
        let comparison = CompareRequest {
            baseline: input.baseline_artifact_id.clone(),
            candidate: input.candidate_artifact_id.clone(),
        };
        let started = Instant::now();
        let joined = runtime
            .workers
            .run_joined(
                request_token,
                started + Duration::from_secs(input.timeout_seconds),
                move |control| {
                    let mut store = store
                        .lock()
                        .map_err(|_| BenchmarkCompareError::Internal)?;
                    let mut store = DynStore(&mut *store);
                    registry
                        .lock()
                        .map_err(|_| BenchmarkCompareError::Internal)?
                        .benchmark_compare(
                            &reference,
                            &comparison,
                            &mut store,
                            &JsonDatasetDecoder,
                            control,
                        )
                },
            )
            .await;
        let duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        match resolve(joined) {
            Ok(outcome) => self.encode_result(&input, outcome, duration_ms),
            Err(failure) => self.error(failure, duration_ms),
        }
    }
    define_security_response_methods!(self, ());

    fn error(&self, failure: Failure, duration_ms: u64) -> Result<CallToolResult, ErrorData> {
        let (code, message) = match failure {
            Failure::Cancelled => {
                return self.cancelled("Comparison cancelled after joined cleanup", duration_ms);
            }
            Failure::Timeout => (Code::CommandTimeout, "Comparison exceeded its deadline"),
            Failure::Worker => (
                Code::SandboxDenied,
                "Comparison capacity is unavailable on this host",
            ),
            Failure::Body(error) => match error {
                BenchmarkCompareError::Cancelled => {
                    return self
                        .cancelled("Comparison cancelled after joined cleanup", duration_ms);
                }
                BenchmarkCompareError::ArtifactNotFound => (
                    Code::ArtifactNotFound,
                    "An identifier does not name an artifact this project owns",
                ),
                BenchmarkCompareError::ArtifactUnreadable => (
                    Code::ArtifactUnreadable,
                    "Stored dataset bytes could not be read whole",
                ),
                BenchmarkCompareError::ArtifactTooLarge => (
                    Code::ArtifactTooLarge,
                    "A dataset exceeds the comparison read ceiling",
                ),
                BenchmarkCompareError::NotADataset => (
                    Code::NotADataset,
                    "An identifier names an artifact that is not a benchmark dataset",
                ),
                BenchmarkCompareError::InvalidDataset(_) => (
                    Code::InvalidDataset,
                    "A stored dataset is not the v1 contract this reader implements",
                ),
                BenchmarkCompareError::NoCommonBenchmark => (
                    Code::NoCommonBenchmark,
                    "The two datasets share no benchmark key",
                ),
                BenchmarkCompareError::Internal => {
                    return Err(ErrorData::internal_error("Comparison failed", None));
                }
            },
        };
        self.blocked(code, message, None, duration_ms)
    }

    fn encode_result(
        &self,
        input: &Input,
        outcome: CompareOutcome,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let data = Box::new(Data {
            project_ref: input.project_ref.to_string(),
            semantics: "observed_difference_between_two_measurements_without_attribution",
            baseline_artifact_id: input.baseline_artifact_id.to_string(),
            candidate_artifact_id: input.candidate_artifact_id.to_string(),
            report: report(outcome),
        });
        encode_bounded(
            &self.contract,
            data,
            duration_ms,
            "Comparison serialization failed",
            |data, duration_ms| Output {
                outcome: comparison_outcome(data),
                summary: "Observed difference between two measurements under the frozen method",
                duration_ms,
            },
            |data| {
                // Names carry no measurement, so they leave first; then the
                // lowest-ranked comparison rows.
                let report = &mut data.report;
                if report.candidate_only.pop().is_some() {
                    report.candidate_only_omitted = report.candidate_only_omitted.saturating_add(1);
                } else if report.baseline_only.pop().is_some() {
                    report.baseline_only_omitted = report.baseline_only_omitted.saturating_add(1);
                } else if report.comparisons.pop().is_some() {
                    report.comparisons_omitted = report.comparisons_omitted.saturating_add(1);
                } else {
                    return false;
                }
                report.complete = false;
                true
            },
            |duration_ms| Output {
                outcome: Outcome::Blocked {
                    error_code: Code::OutputLimitExceeded,
                    error_message: "Comparison response exceeds its fixed budget",
                    data: None,
                },
                summary: "Comparison response exceeds its fixed budget",
                duration_ms,
            },
        )
    }
}

/// `OperationControl` failures reach this layer as `Cancelled` or, for a
/// deadline, as `ArtifactNotFound`: the application maps every project
/// rejection to "no artifact exists for this call". Those two are the only
/// shapes a control signal can produce, so when the worker also observed an
/// interrupt the interrupt is what gets named.
fn resolve(
    joined: Result<super::workers::Joined<CompareOutcome, BenchmarkCompareError>, WorkerError>,
) -> Result<CompareOutcome, Failure> {
    let joined = match joined {
        Ok(joined) => joined,
        Err(WorkerError::TimedOut) => return Err(Failure::Timeout),
        Err(WorkerError::Cancelled) => return Err(Failure::Cancelled),
        Err(_) => return Err(Failure::Worker),
    };
    let signal_shaped = matches!(
        joined.result,
        Err(BenchmarkCompareError::Cancelled | BenchmarkCompareError::ArtifactNotFound)
    );
    match (joined.result, joined.interrupted) {
        (_, Some(WorkerError::TimedOut)) if signal_shaped => Err(Failure::Timeout),
        (_, Some(WorkerError::Cancelled)) if signal_shaped => Err(Failure::Cancelled),
        (Err(error), _) => Err(Failure::Body(error)),
        (Ok(value), None) => Ok(value),
        (Ok(_), Some(WorkerError::TimedOut)) => Err(Failure::Timeout),
        (Ok(_), Some(WorkerError::Cancelled)) => Err(Failure::Cancelled),
        (Ok(_), Some(_)) => Err(Failure::Worker),
    }
}

fn comparison_outcome(data: &Data) -> Outcome {
    if !data.report.incompatibility_reasons.is_empty() {
        return Outcome::Failed {
            error_code: Code::IncompatibleDatasets,
            error_message: "The two datasets do not describe comparable executions",
            data: Box::new(data.clone()),
        };
    }
    if data.report.complete {
        return Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(data.clone()),
        };
    }
    Outcome::Blocked {
        error_code: Code::EvidenceIncomplete,
        error_message: "Comparison evidence is partial",
        data: Some(Box::new(data.clone())),
    }
}

fn report(outcome: CompareOutcome) -> schemas::Report {
    match outcome {
        CompareOutcome::Incompatible(mut reasons) => {
            reasons.sort_unstable();
            reasons.dedup();
            schemas::Report {
                method: None,
                comparisons: Vec::new(),
                compared: 0,
                comparisons_omitted: 0,
                baseline_only: Vec::new(),
                baseline_only_omitted: 0,
                candidate_only: Vec::new(),
                candidate_only_omitted: 0,
                incompatibility_reasons: reasons
                    .into_iter()
                    .map(incompatibility_reason)
                    .collect(),
                // The pair was refused before any statistic ran, and every
                // reason for that refusal is reported.
                complete: true,
            }
        }
        CompareOutcome::Report(report) => published_report(*report),
    }
}

fn published_report(report: ComparisonReport) -> schemas::Report {
    let compared = u32::try_from(report.compared).unwrap_or(u32::MAX);
    let mut comparisons: Vec<schemas::Comparison> =
        report.comparisons.into_iter().map(comparison).collect();
    // Ranked so trimming drops the least informative row: a withheld verdict
    // before a material one, and a smaller observed difference before a larger.
    comparisons.sort_by(|left, right| {
        rank(left.verdict)
            .cmp(&rank(right.verdict))
            .then_with(|| {
                right
                    .effect_ratio
                    .abs()
                    .total_cmp(&left.effect_ratio.abs())
            })
            .then_with(|| left.key.cmp(&right.key))
    });
    let comparisons_omitted =
        u32::try_from(comparisons.len().saturating_sub(MAX_RESPONSE_COMPARISONS)).unwrap_or(u32::MAX);
    comparisons.truncate(MAX_RESPONSE_COMPARISONS);
    let baseline_only_omitted =
        u32::try_from(report.baseline_only.len().saturating_sub(MAX_RESPONSE_KEYS))
            .unwrap_or(u32::MAX);
    let candidate_only_omitted =
        u32::try_from(report.candidate_only.len().saturating_sub(MAX_RESPONSE_KEYS))
            .unwrap_or(u32::MAX);
    let mut baseline_only = report.baseline_only;
    baseline_only.truncate(MAX_RESPONSE_KEYS);
    let mut candidate_only = report.candidate_only;
    candidate_only.truncate(MAX_RESPONSE_KEYS);
    schemas::Report {
        method: Some(method(&report.method)),
        compared,
        complete: comparisons_omitted == 0
            && baseline_only_omitted == 0
            && candidate_only_omitted == 0,
        comparisons,
        comparisons_omitted,
        baseline_only,
        baseline_only_omitted,
        candidate_only,
        candidate_only_omitted,
        incompatibility_reasons: Vec::new(),
    }
}

fn rank(verdict: schemas::Verdict) -> u8 {
    match verdict {
        schemas::Verdict::Regression | schemas::Verdict::Improvement => 0,
        schemas::Verdict::NoMaterialChange => 1,
        schemas::Verdict::Inconclusive => 2,
    }
}

fn method(value: &ComparisonMethod) -> schemas::Method {
    schemas::Method {
        method: value.method().to_owned(),
        statistic: match value.statistic() {
            ComparisonStatistic::MedianPerIterationNanoseconds => {
                schemas::Statistic::MedianPerIterationNanoseconds
            }
        },
        bootstrap_resamples: value.bootstrap_resamples(),
        seed: value.seed(),
        confidence_level: finite(value.confidence_level()),
        material_threshold_ratio: finite(value.material_threshold_ratio()),
        multiplicity: match value.multiplicity() {
            MultiplicityCorrection::None => schemas::Multiplicity::None,
            MultiplicityCorrection::Bonferroni => schemas::Multiplicity::Bonferroni,
        },
        family_size: value.family_size(),
        adjusted_confidence_level: finite(value.adjusted_confidence_level()),
        outlier_policy: match value.outlier_policy() {
            OutlierPolicy::ReportedNotRemoved => schemas::OutlierPolicy::ReportedNotRemoved,
        },
    }
}

fn comparison(value: BenchmarkComparison) -> schemas::Comparison {
    schemas::Comparison {
        key: value.key,
        verdict: match value.verdict {
            ComparisonVerdict::Regression => schemas::Verdict::Regression,
            ComparisonVerdict::Improvement => schemas::Verdict::Improvement,
            ComparisonVerdict::NoMaterialChange => schemas::Verdict::NoMaterialChange,
            ComparisonVerdict::Inconclusive => schemas::Verdict::Inconclusive,
        },
        effect_ratio: finite(value.effect_ratio),
        confidence_interval: schemas::Interval {
            low: finite(value.confidence_interval.0),
            high: finite(value.confidence_interval.1),
        },
        baseline_median_ns: finite(value.baseline_median_ns),
        candidate_median_ns: finite(value.candidate_median_ns),
        baseline_samples: u32::try_from(value.baseline_samples).unwrap_or(u32::MAX),
        candidate_samples: u32::try_from(value.candidate_samples).unwrap_or(u32::MAX),
        baseline_outliers: u32::try_from(value.baseline_outliers).unwrap_or(u32::MAX),
        candidate_outliers: u32::try_from(value.candidate_outliers).unwrap_or(u32::MAX),
        minimum_detectable_ratio: finite(value.minimum_detectable_ratio),
        inconclusive_reasons: value
            .inconclusive_reasons
            .into_iter()
            .map(|reason| match reason {
                InconclusiveReason::InsufficientSamples => {
                    schemas::InconclusiveReason::InsufficientSamples
                }
                InconclusiveReason::PrecisionBelowThreshold => {
                    schemas::InconclusiveReason::PrecisionBelowThreshold
                }
                InconclusiveReason::IntervalSpansThreshold => {
                    schemas::InconclusiveReason::IntervalSpansThreshold
                }
                InconclusiveReason::ZeroOrNegativeBaseline => {
                    schemas::InconclusiveReason::ZeroOrNegativeBaseline
                }
                InconclusiveReason::MissingMeasurement => {
                    schemas::InconclusiveReason::MissingMeasurement
                }
                InconclusiveReason::TruncatedMeasurement => {
                    schemas::InconclusiveReason::TruncatedMeasurement
                }
            })
            .collect(),
    }
}

fn incompatibility_reason(value: IncompatibilityReason) -> schemas::IncompatibilityReason {
    match value {
        IncompatibilityReason::FormatVersion => schemas::IncompatibilityReason::FormatVersion,
        IncompatibilityReason::Unit => schemas::IncompatibilityReason::Unit,
        IncompatibilityReason::Harness => schemas::IncompatibilityReason::Harness,
        IncompatibilityReason::HarnessVersion => schemas::IncompatibilityReason::HarnessVersion,
        IncompatibilityReason::BenchmarkIdentity => {
            schemas::IncompatibilityReason::BenchmarkIdentity
        }
        IncompatibilityReason::RustVersion => schemas::IncompatibilityReason::RustVersion,
        IncompatibilityReason::CargoVersion => schemas::IncompatibilityReason::CargoVersion,
        IncompatibilityReason::RuntimeImage => schemas::IncompatibilityReason::RuntimeImage,
        IncompatibilityReason::Platform => schemas::IncompatibilityReason::Platform,
        IncompatibilityReason::Architecture => schemas::IncompatibilityReason::Architecture,
        IncompatibilityReason::CpuModel => schemas::IncompatibilityReason::CpuModel,
        IncompatibilityReason::Quotas => schemas::IncompatibilityReason::Quotas,
        IncompatibilityReason::Selection => schemas::IncompatibilityReason::Selection,
        IncompatibilityReason::SamplingMode => schemas::IncompatibilityReason::SamplingMode,
        IncompatibilityReason::UnknownHardware => schemas::IncompatibilityReason::UnknownHardware,
        IncompatibilityReason::SameArtifact => schemas::IncompatibilityReason::SameArtifact,
    }
}

/// JSON has no representation for a non-finite number, and the contract must
/// never fail to encode a result that was computed. A non-finite statistic is
/// an absence and is published as zero, exactly as the domain publishes an
/// absent statistic.
fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod trim_tests;
