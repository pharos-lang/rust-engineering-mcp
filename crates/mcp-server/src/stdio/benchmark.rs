//! M5-01: `rust.benchmark.run`, one measured benchmark execution.
//!
//! The response never carries a raw sample and never a byte of harness log.
//! ADR-076 §3 puts the samples in the `benchmark_dataset` artifact and the
//! harness's own output tree in the `criterion_archive` artifact, and ADR-080
//! puts each repetition's `stdout` and `stderr` in their own `harness_stdout` /
//! `harness_stderr` artifacts. What travels here is a bounded description of
//! them — counts, cut flags and the repetition each belongs to — plus the
//! complete provenance a later comparison needs.
#[allow(dead_code)]
mod schemas;
use super::{
    HostCargoVendorConfig, HostVendorCaptureConfig,
    clock::WallClock,
    nextest::ExecutionModeDto,
    project::Registry,
    quality_artifacts::performance::BenchmarkMemberKind,
    security_tool::{
        CommonFailure, artifact_fields, capture_vendor, classify_error,
        define_fallible_security_outcome, define_security_response_methods, define_security_tool,
        encode_bounded, run_joined_security,
    },
    workers::Workers,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::InspectionError;
use rust_engineering_application::benchmark::{
    BenchmarkPorts, BenchmarkPublisher, BenchmarkRunOptions, ProjectBenchmarkPort,
    PublishedBenchmark,
};
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::vendor_capture::BenchmarkVendor;
use rust_engineering_domain::benchmark::{
    BenchmarkDataset, BenchmarkMeasurement, BenchmarkSelection, MeasurementCompleteness,
    SamplingMode, Virtualization,
};
use rust_engineering_domain::benchmark_run::{
    BENCHMARK_MAX_LOG_BYTES, BenchmarkExit, BenchmarkObservation, DatasetOmission, HarnessDetection,
};
use rust_engineering_domain::{
    ArtifactCompleteness, ExecutionTermination, ProjectRef, QualityArtifactDescriptor,
    RuntimeIdentity,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const NAME: &str = "rust.benchmark.run";
/// Rows kept in one response before the trimming strategy starts dropping the
/// lowest-ranked ones. The dataset artifact still carries every measurement.
const MAX_RESPONSE_BENCHMARKS: usize = 128;

pub(super) fn advertised() -> bool {
    super::security_tool::advertised("RUST_MCP_TEST_BENCHMARK_READY")
}

/// The execution-mode decision the three M5 measuring tools share.
///
/// It deliberately does not reuse `nextest::select_execution_mode`. That helper
/// answers `Task` as soon as the peer negotiated MCP Tasks, and a task answer
/// is only materializable for a tool that owns a `JobKind`. M5 owns none: the
/// job kinds are frozen at the nine M3/M4 values, so a performance call that
/// asked for a task is refused as a declared result instead of being turned
/// into a protocol error the peer cannot act on.
pub(super) enum ModeSelection {
    Run,
    TasksRequired,
}

pub(super) fn mode_selection(mode: ExecutionModeDto) -> ModeSelection {
    match mode {
        ExecutionModeDto::Task => ModeSelection::TasksRequired,
        // A registry-owned background job re-enters a validated tool path under
        // its own permit; M5 never produces one, and both remaining modes run
        // inside the timeout the closed input already bounded.
        ExecutionModeDto::Auto | ExecutionModeDto::Synchronous => ModeSelection::Run,
    }
}

#[derive(Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    #[serde(default)]
    #[schemars(
        length(min = 1, max = 64),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}$")
    )]
    package: Option<String>,
    #[serde(default)]
    #[schemars(
        length(min = 1, max = 64),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]{0,63}$")
    )]
    bench_target: Option<String>,
    #[serde(default)]
    #[schemars(with = "Vec<schemas::Feature>", length(max = 16))]
    features: Vec<String>,
    #[serde(default)]
    all_features: bool,
    #[serde(default)]
    no_default_features: bool,
    #[serde(default = "default_run_count")]
    #[schemars(range(min = 1, max = 3))]
    run_count: u8,
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 900))]
    timeout_seconds: u64,
    #[serde(default)]
    execution_mode: ExecutionModeDto,
}
fn default_run_count() -> u8 {
    rust_engineering_application::benchmark::BENCHMARK_DEFAULT_RUN_COUNT
}
fn default_timeout() -> u64 {
    rust_engineering_application::benchmark::BENCHMARK_DEFAULT_TIMEOUT_SECONDS
}
impl Input {
    fn options(&self) -> Result<BenchmarkRunOptions, ErrorData> {
        BenchmarkRunOptions::new(
            self.package.clone(),
            self.bench_target.clone(),
            self.features.clone(),
            self.all_features,
            self.no_default_features,
            self.run_count,
            self.timeout_seconds,
        )
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }
}

#[derive(Clone, Copy, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    TasksRequired,
    SandboxDenied,
    MissingOfflineData,
    ArtifactUnavailable,
    ToolNotInstalled,
    InvalidProject,
    ProjectNotFound,
    CommandTimeout,
    OutputLimitExceeded,
    EvidenceIncomplete,
    ObservedFailure,
    HarnessUnrecognized,
    HarnessUnapproved,
}
define_fallible_security_outcome!(Code, &'static str, ());

#[derive(Clone, Copy, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    /// The pooled samples of every repetition, in the versioned dataset format.
    BenchmarkDataset,
    /// One repetition's criterion output tree, exactly as the harness exported
    /// it: the last repetition that exported one.
    CriterionArchive,
    /// One repetition's `stdout`, as the harness wrote it (ADR-080 §1).
    HarnessStdout,
    /// One repetition's `stderr`. For an observed compilation failure this is
    /// where the compiler's own text is.
    HarnessStderr,
}
#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Artifact {
    kind: ArtifactKind,
    /// The 1-based repetition this evidence came from, numbered exactly as the
    /// dataset's samples are. `null` only for `benchmark_dataset`, which pools
    /// every repetition and carries the index on each sample instead.
    #[schemars(range(min = 1, max = 3))]
    run_index: Option<u8>,
    #[schemars(length(min = 1, max = 512))]
    uri: String,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    sha256: String,
    /// Bytes published. For a log whose `completeness` is `truncated` this is
    /// how many bytes survived the cut, never how many the harness wrote.
    size_bytes: u64,
    completeness: schemas::ArtifactCompleteness,
}
#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    semantics: &'static str,
    observation: schemas::Observation,
    /// At most eight: the dataset, the criterion tree, and one `stdout` and one
    /// `stderr` for each of the three repetitions `run_count` admits
    /// (ADR-080 §1).
    #[schemars(length(max = 8))]
    artifacts: Vec<Artifact>,
}

/// The runtime this tool measures through.
///
/// `executor` and `publisher` are optional because the M5 execution vertical
/// lands separately: the `ProjectBenchmarkPort` implementation belongs on
/// `RustProjectInspector` (execution-adapter) and the dataset/archive publisher
/// on the durable quality store. Until both are attached the tool answers a
/// declared `unavailable`, never a protocol error.
pub(super) struct Runtime {
    pub(super) registry: Arc<Mutex<Registry>>,
    pub(super) workers: Workers,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) vendor: Option<HostCargoVendorConfig>,
    /// ADR-078's capture, when the host provisioned one. It takes precedence
    /// over the directory source: a host that declared both meant the capture,
    /// which is the only one of the two that can carry a criterion closure.
    pub(super) capture: Option<HostVendorCaptureConfig>,
    pub(super) executor: Option<Arc<dyn ProjectBenchmarkPort>>,
    pub(super) publisher: Option<Arc<Mutex<dyn BenchmarkPublisher>>>,
}

/// `BenchmarkPorts` takes sized ports; these two forward a shared handle into
/// that shape without asking the application to know about `dyn`.
struct DynExecutor<'a>(&'a dyn ProjectBenchmarkPort);
impl ProjectBenchmarkPort for DynExecutor<'_> {
    fn benchmark(
        &self,
        source: &rust_engineering_domain::SourceBundle,
        vendor: rust_engineering_application::vendor_capture::BenchmarkVendor<'_>,
        options: &BenchmarkRunOptions,
        control: &dyn rust_engineering_application::InspectionControl,
    ) -> Result<BenchmarkObservation, SecurityError> {
        self.0.benchmark(source, vendor, options, control)
    }
}
struct DynPublisher<'a>(&'a mut dyn BenchmarkPublisher);
impl BenchmarkPublisher for DynPublisher<'_> {
    fn publish_benchmark(
        &mut self,
        capture: &rust_engineering_application::security::SecurityCapture,
        observation: &BenchmarkObservation,
        revalidate: &mut dyn FnMut() -> Result<
            rust_engineering_application::QualityOwnerFacts,
            InspectionError,
        >,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
        self.0.publish_benchmark(capture, observation, revalidate)
    }
}

define_security_tool!(
    BenchmarkTool,
    "Run the project's own criterion benchmarks once in the approved offline runtime and publish the raw samples as a private versioned dataset artifact, plus the harness output tree of one repetition and each repetition's own stdout and stderr as separate private artifacts. Every published artifact but the pooled dataset names the repetition it came from. The response carries no raw sample and no log text: it reports the detected harness, the exit and which repetition it belongs to, per-benchmark median, minimum, maximum, median absolute deviation, counted outliers and completeness, per-repetition retained log sizes and whether either stream was cut at the server ceiling, and the full provenance a later comparison needs. Requires an authenticated host cargo vendor tree. Warm-up, measurement time and sample size are fixed by the server and travel in the provenance; no harness flag, path or free argument is accepted. An unrecognized or unapproved harness and a failed compilation are observed results that publish no dataset and still publish their logs. Measurements describe this host and this execution and attribute no cause."
);

impl BenchmarkTool {
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
        let options = input.options()?;
        if matches!(
            mode_selection(input.execution_mode),
            ModeSelection::TasksRequired
        ) {
            return self.blocked(
                Code::TasksRequired,
                "Benchmark runs are not admitted as MCP Tasks",
                None,
                0,
            );
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ErrorData::internal_error("Benchmark runtime is not configured", None)
        })?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Discovery must complete before measuring",
                None,
                0,
            );
        }
        let capture = runtime.capture.clone();
        let vendor = runtime.vendor.clone();
        if capture.is_none() && vendor.is_none() {
            return self.unavailable(
                Code::MissingOfflineData,
                "Host-authenticated offline vendor is required",
                0,
            );
        }
        let Some(publisher) = runtime.publisher.clone() else {
            return self.unavailable(
                Code::ArtifactUnavailable,
                "Durable benchmark evidence is unavailable",
                0,
            );
        };
        let Some(executor) = runtime.executor.clone() else {
            return self.unavailable(
                Code::ToolNotInstalled,
                "Approved benchmark runtime is unavailable",
                0,
            );
        };
        let registry = Arc::clone(&runtime.registry);
        let reference = input.project_ref.clone();
        let (result, duration) = run_joined_security(
            &runtime.workers,
            request_token,
            options.timeout_seconds(),
            "Benchmark worker unavailable",
            move |control| {
                // Either arm is host-authenticated before the runtime sees a
                // byte: the directory source by re-capture against the approved
                // fingerprint, the capture by re-deriving its tree digest from
                // the artifact and refusing it when it is not the declared one.
                let opened;
                let snapshot;
                let resolved = match &capture {
                    Some(config) => {
                        opened = rust_engineering_project::vendor_capture::open_verified_capture(
                            &config.artifact,
                            &config.tree_digest,
                            control,
                        )?;
                        BenchmarkVendor::Capture(&opened)
                    }
                    None => {
                        let config = vendor
                            .as_ref()
                            .ok_or(SecurityError::Inspection(InspectionError::Internal))?;
                        snapshot = capture_vendor(config, control)?;
                        BenchmarkVendor::Snapshot(&snapshot)
                    }
                };
                let executor = DynExecutor(executor.as_ref());
                let mut published = publisher
                    .lock()
                    .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?;
                let mut published = DynPublisher(&mut *published);
                registry
                    .lock()
                    .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?
                    .benchmark_durable(
                        &reference,
                        resolved,
                        &options,
                        BenchmarkPorts {
                            executor: &executor,
                            publisher: &mut published,
                        },
                        &WallClock,
                        control,
                    )
            },
        )
        .await?;
        match result {
            Ok(result) => self.encode_result(&input.project_ref, result, duration),
            Err(error) => self.error(error, duration),
        }
    }
    define_security_response_methods!(self, ());

    fn error(&self, error: SecurityError, duration_ms: u64) -> Result<CallToolResult, ErrorData> {
        let (code, message) = match classify_error(error) {
            CommonFailure::Cancelled => {
                return self.cancelled("Benchmark cancelled after joined cleanup", duration_ms);
            }
            CommonFailure::ToolNotInstalled => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved benchmark runtime is unavailable",
                    duration_ms,
                );
            }
            CommonFailure::Timeout => (Code::CommandTimeout, "Benchmark exceeded its deadline"),
            CommonFailure::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            CommonFailure::OutputLimit => (
                Code::OutputLimitExceeded,
                "Benchmark evidence exceeded its fixed budget",
            ),
            CommonFailure::ProjectNotFound => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            CommonFailure::SandboxDenied => (
                Code::SandboxDenied,
                "Approved benchmark execution could not be established",
            ),
            CommonFailure::Specific(_) => (
                Code::InvalidProject,
                "Captured benchmark inputs or evidence could not be validated",
            ),
        };
        self.blocked(code, message, None, duration_ms)
    }

    fn encode_result(
        &self,
        reference: &ProjectRef,
        result: PublishedBenchmark,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let artifacts = artifacts(reference, &result)?;
        let artifacts_complete = !artifacts.is_empty()
            && result
                .artifacts
                .iter()
                .all(|descriptor| descriptor.completeness == ArtifactCompleteness::Complete);
        let data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "observed_measurement_of_one_execution_on_one_host_without_attribution",
            observation: observation(&result.observation, artifacts_complete),
            artifacts,
        });
        encode_bounded(
            &self.contract,
            data,
            duration_ms,
            "Benchmark serialization failed",
            |data, duration_ms| Output {
                outcome: outcome(data),
                summary: "Observed benchmark measurement; no causal or portable claim",
                duration_ms,
            },
            |data| {
                // The lowest-ranked row leaves first; the dataset artifact still
                // carries every measurement the run produced.
                if data.observation.benchmarks.pop().is_none() {
                    return false;
                }
                data.observation.benchmarks_omitted =
                    data.observation.benchmarks_omitted.saturating_add(1);
                data.observation.complete = false;
                true
            },
            |duration_ms| Output {
                outcome: Outcome::Blocked {
                    error_code: Code::OutputLimitExceeded,
                    error_message: "Benchmark response exceeds its fixed budget",
                    data: None,
                },
                summary: "Benchmark response exceeds its fixed budget",
                duration_ms,
            },
        )
    }
}

fn outcome(data: &Data) -> Outcome {
    let observation = &data.observation;
    if observation.complete && observation.exit == schemas::Exit::Passed {
        return Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(data.clone()),
        };
    }
    let failure = match observation.harness {
        schemas::Harness::Unrecognized => Some((
            Code::HarnessUnrecognized,
            "No recognized benchmark harness was resolved; no dataset was produced",
        )),
        schemas::Harness::CriterionUnapproved { .. } => Some((
            Code::HarnessUnapproved,
            "The resolved criterion version is not the approved one; no dataset was produced",
        )),
        schemas::Harness::Criterion { .. } => match observation.exit {
            schemas::Exit::BenchmarkFailed | schemas::Exit::CompilationFailed => Some((
                Code::ObservedFailure,
                "The benchmark execution reported a failure; inspect exit and artifacts",
            )),
            _ => None,
        },
    };
    match failure {
        Some((error_code, error_message)) => Outcome::Failed {
            error_code,
            error_message,
            data: Box::new(data.clone()),
        },
        None => Outcome::Blocked {
            error_code: Code::EvidenceIncomplete,
            error_message: "Benchmark evidence is partial",
            data: Some(Box::new(data.clone())),
        },
    }
}

/// The published members, tied back to the repetition each came from.
///
/// A `ToolLog` descriptor cannot say by itself whether it holds a `stdout` or a
/// `stderr`, nor which repetition wrote it — the store's kind vocabulary has one
/// value for both. So the association is not guessed from the descriptor: the
/// same plan the publisher committed is re-derived from the observation and
/// walked beside the descriptors it returned. A publisher that returned some
/// other set fails closed here rather than producing artifacts whose
/// `run_index` is a fabrication.
fn artifacts(
    reference: &ProjectRef,
    result: &PublishedBenchmark,
) -> Result<Vec<Artifact>, ErrorData> {
    let plan = super::quality_artifacts::performance::benchmark_member_plan(&result.observation);
    if plan.len() != result.artifacts.len() {
        return Err(ErrorData::internal_error(
            "Benchmark artifacts do not match the published members",
            None,
        ));
    }
    plan.into_iter()
        .zip(result.artifacts.iter())
        .map(|(planned, descriptor)| artifact(reference, descriptor, planned))
        .collect()
}

fn artifact(
    reference: &ProjectRef,
    descriptor: &QualityArtifactDescriptor,
    planned: BenchmarkMemberKind,
) -> Result<Artifact, ErrorData> {
    if descriptor.kind != planned.artifact_kind() {
        return Err(ErrorData::internal_error(
            "Benchmark artifact kind is invalid",
            None,
        ));
    }
    let kind = match planned {
        BenchmarkMemberKind::Dataset => ArtifactKind::BenchmarkDataset,
        BenchmarkMemberKind::CriterionArchive { .. } => ArtifactKind::CriterionArchive,
        BenchmarkMemberKind::HarnessStdout { .. } => ArtifactKind::HarnessStdout,
        BenchmarkMemberKind::HarnessStderr { .. } => ArtifactKind::HarnessStderr,
    };
    let fields = artifact_fields(
        reference,
        descriptor,
        "Invalid benchmark artifact descriptor",
    )?;
    Ok(Artifact {
        kind,
        run_index: planned.run_index(),
        uri: fields.uri,
        sha256: fields.sha256,
        size_bytes: fields.size_bytes,
        completeness: completeness(fields.completeness),
    })
}

fn completeness(value: ArtifactCompleteness) -> schemas::ArtifactCompleteness {
    match value {
        ArtifactCompleteness::Complete => schemas::ArtifactCompleteness::Complete,
        ArtifactCompleteness::Truncated => schemas::ArtifactCompleteness::Truncated,
        ArtifactCompleteness::Partial => schemas::ArtifactCompleteness::Partial,
        ArtifactCompleteness::Invalid => schemas::ArtifactCompleteness::Invalid,
        ArtifactCompleteness::Unavailable => schemas::ArtifactCompleteness::Unavailable,
    }
}

fn observation(value: &BenchmarkObservation, artifacts_complete: bool) -> schemas::Observation {
    let mut benchmarks: Vec<schemas::BenchmarkSummary> = value
        .dataset
        .iter()
        .flat_map(|dataset| dataset.measurements())
        .map(summary)
        .collect();
    // Ranked so the trimming strategy drops the smallest measurement first.
    benchmarks.sort_by(|left, right| {
        right
            .median_per_iteration_ns
            .total_cmp(&left.median_per_iteration_ns)
            .then_with(|| left.key.cmp(&right.key))
    });
    let benchmarks_omitted =
        u32::try_from(benchmarks.len().saturating_sub(MAX_RESPONSE_BENCHMARKS)).unwrap_or(u32::MAX);
    benchmarks.truncate(MAX_RESPONSE_BENCHMARKS);
    let measurements_complete = value.dataset.as_ref().is_some_and(|dataset| {
        dataset
            .measurements()
            .iter()
            .all(|measurement| measurement.completeness() == MeasurementCompleteness::Complete)
    });
    schemas::Observation {
        selection: selection(&value.selection),
        harness: match &value.harness {
            HarnessDetection::Criterion { version } => schemas::Harness::Criterion {
                version: version.clone(),
            },
            HarnessDetection::CriterionUnapproved { version } => {
                schemas::Harness::CriterionUnapproved {
                    version: version.clone(),
                }
            }
            HarnessDetection::Unrecognized => schemas::Harness::Unrecognized,
        },
        exit: match value.exit {
            BenchmarkExit::Passed => schemas::Exit::Passed,
            BenchmarkExit::BenchmarkFailed => schemas::Exit::BenchmarkFailed,
            BenchmarkExit::CompilationFailed => schemas::Exit::CompilationFailed,
            BenchmarkExit::Uncalibrated => schemas::Exit::Uncalibrated,
            BenchmarkExit::Incomplete => schemas::Exit::Incomplete,
        },
        exit_code: value.exit_code,
        termination: termination(value.termination),
        exit_run_index: value.exit_run_index,
        runs_requested: value.runs_requested,
        runs_completed: value.runs_completed,
        dataset_published: value.dataset.is_some(),
        dataset_omission: value.omission.map(|omission| match omission {
            DatasetOmission::HarnessUnrecognized => schemas::DatasetOmission::HarnessUnrecognized,
            DatasetOmission::HarnessUnapproved => schemas::DatasetOmission::HarnessUnapproved,
            DatasetOmission::ExecutionFailed => schemas::DatasetOmission::ExecutionFailed,
            DatasetOmission::OutputMissing => schemas::DatasetOmission::OutputMissing,
            DatasetOmission::OutputUnparsable => schemas::DatasetOmission::OutputUnparsable,
            DatasetOmission::OutputTooLarge => schemas::DatasetOmission::OutputTooLarge,
            DatasetOmission::Cancelled => schemas::DatasetOmission::Cancelled,
        }),
        complete: value.dataset.is_some()
            && value.omission.is_none()
            && value.runs_completed == value.runs_requested
            && measurements_complete
            && benchmarks_omitted == 0
            && artifacts_complete,
        benchmarks,
        benchmarks_omitted,
        provenance: value.dataset.as_ref().map(provenance),
        runtime: runtime_identity(&value.runtime),
        execution_fingerprint: value.execution_fingerprint.to_string(),
        vendor_fingerprint: value.vendor_fingerprint.to_string(),
        // Byte counts and cut flags, never the bytes: the logs travel as
        // artifacts, and the server's stdout stays the protocol transport.
        logs: value
            .logs
            .iter()
            .map(|log| schemas::HarnessLog {
                run_index: log.run_index,
                stdout_bytes: log.stdout.len() as u64,
                stdout_truncated: log.stdout_truncated,
                stdout_replaced: log.stdout_replaced,
                stderr_bytes: log.stderr.len() as u64,
                stderr_truncated: log.stderr_truncated,
                stderr_replaced: log.stderr_replaced,
                retained_ceiling_bytes: BENCHMARK_MAX_LOG_BYTES as u64,
            })
            .collect(),
        stdout_truncated: value.any_stdout_truncated(),
        stderr_truncated: value.any_stderr_truncated(),
    }
}

fn termination(value: ExecutionTermination) -> schemas::ExecutionTermination {
    match value {
        ExecutionTermination::Exited => schemas::ExecutionTermination::Exited,
        ExecutionTermination::TimedOut => schemas::ExecutionTermination::TimedOut,
        ExecutionTermination::Cancelled => schemas::ExecutionTermination::Cancelled,
        ExecutionTermination::OutputLimit => schemas::ExecutionTermination::OutputLimit,
    }
}

fn runtime_identity(value: &RuntimeIdentity) -> schemas::RuntimeIdentity {
    schemas::RuntimeIdentity {
        platform: value.platform.clone(),
        image_id: value.image_id.clone(),
        configuration_fingerprint: value.configuration_fingerprint.to_string(),
        execution_fingerprint: value.execution_fingerprint.to_string(),
        rust_version: value.rust_version.clone(),
        cargo_version: value.cargo_version.clone(),
        declared_toolchain: value.declared_toolchain.clone(),
    }
}

fn selection(value: &BenchmarkSelection) -> schemas::Selection {
    schemas::Selection {
        package: value.package.clone(),
        bench_target: value.bench_target.clone(),
        features: value.features.clone(),
        all_features: value.all_features,
        no_default_features: value.no_default_features,
        profile: value.profile.clone(),
    }
}

fn provenance(dataset: &BenchmarkDataset) -> schemas::Provenance {
    let value = dataset.provenance();
    schemas::Provenance {
        dataset_format: dataset.format().to_owned(),
        dataset_format_version: dataset.format_version(),
        unit: schemas::SampleUnit::Nanoseconds,
        source_fingerprint: value.source_fingerprint.clone(),
        harness: schemas::BenchmarkHarnessName::Criterion,
        harness_version: value.harness_version.clone(),
        rust_version: value.rust_version.clone(),
        cargo_version: value.cargo_version.clone(),
        declared_toolchain: value.declared_toolchain.clone(),
        image_digest: value.image_digest.clone(),
        platform: value.platform.clone(),
        configuration_fingerprint: value.configuration_fingerprint.clone(),
        execution_fingerprint: value.execution_fingerprint.clone(),
        selection: selection(&value.selection),
        hardware: schemas::HardwareProfile {
            cpu_model: value.hardware.cpu_model.clone(),
            cpu_cores: value.hardware.cpu_cores,
            os_kernel: value.hardware.os_kernel.clone(),
            arch: value.hardware.arch.clone(),
            virtualization: match value.hardware.virtualization {
                Virtualization::Unknown => schemas::Virtualization::Unknown,
                Virtualization::Container => schemas::Virtualization::Container,
                Virtualization::VirtualMachine => schemas::Virtualization::VirtualMachine,
                Virtualization::Bare => schemas::Virtualization::Bare,
            },
            cpu_governor: value.hardware.cpu_governor.clone(),
            quotas: schemas::ResourceQuotas {
                cpu_quota_millicores: value.hardware.quotas.cpu_quota_millicores,
                memory_bytes: value.hardware.quotas.memory_bytes,
                pids: value.hardware.quotas.pids,
            },
        },
        run_index: value.run_index,
        run_count: value.run_count,
        captured_at_unix: value.captured_at_unix,
    }
}

/// Descriptive statistics of one benchmark's per-iteration times.
///
/// They describe the sample set the dataset carries and nothing else: no
/// comparison, no verdict and no statement about what produced a value.
fn summary(measurement: &BenchmarkMeasurement) -> schemas::BenchmarkSummary {
    let mut values: Vec<f64> = measurement
        .samples()
        .iter()
        .map(|sample| sample.per_iteration_ns())
        .filter(|value| value.is_finite())
        .collect();
    values.sort_unstable_by(f64::total_cmp);
    let identity = measurement.identity();
    schemas::BenchmarkSummary {
        key: identity.key().to_owned(),
        group_id: identity.group_id().to_owned(),
        function_id: identity.function_id().map(str::to_owned),
        value_str: identity.value_str().map(str::to_owned),
        samples: u32::try_from(values.len()).unwrap_or(u32::MAX),
        sample_size_requested: measurement.sample_size_requested(),
        warm_up_ms: measurement.warm_up_ms(),
        measurement_ms: measurement.measurement_ms(),
        median_per_iteration_ns: median(&values),
        minimum_per_iteration_ns: values.first().copied().unwrap_or(0.0),
        maximum_per_iteration_ns: values.last().copied().unwrap_or(0.0),
        median_absolute_deviation_ns: median_absolute_deviation(&values),
        outliers_counted: u32::try_from(tukey_outliers(&values)).unwrap_or(u32::MAX),
        sampling_mode: match measurement.sampling_mode() {
            SamplingMode::Linear => schemas::SamplingMode::Linear,
            SamplingMode::Flat => schemas::SamplingMode::Flat,
            SamplingMode::Auto => schemas::SamplingMode::Auto,
            SamplingMode::Unknown => schemas::SamplingMode::Unknown,
        },
        completeness: match measurement.completeness() {
            MeasurementCompleteness::Complete => schemas::MeasurementCompleteness::Complete,
            MeasurementCompleteness::Truncated => schemas::MeasurementCompleteness::Truncated,
            MeasurementCompleteness::Missing => schemas::MeasurementCompleteness::Missing,
        },
    }
}

/// Median of an already sorted slice. Zero for an empty slice, which is an
/// absence and never a measured zero.
fn median(sorted: &[f64]) -> f64 {
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

fn median_absolute_deviation(sorted: &[f64]) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let centre = median(sorted);
    let mut deviations: Vec<f64> = sorted.iter().map(|value| (value - centre).abs()).collect();
    deviations.sort_unstable_by(f64::total_cmp);
    median(&deviations)
}

/// Linearly interpolated quantile of an already sorted slice (the common
/// "type 7" definition).
fn quantile(sorted: &[f64], q: f64) -> f64 {
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

/// Values outside `Q1 - 1.5*IQR` and `Q3 + 1.5*IQR`. Counted only: nothing is
/// removed from the sample set any statistic is computed over.
fn tukey_outliers(sorted: &[f64]) -> usize {
    if sorted.len() < 4 {
        return 0;
    }
    let first = quantile(sorted, 0.25);
    let third = quantile(sorted, 0.75);
    let spread = third - first;
    let low = first - 1.5 * spread;
    let high = third + 1.5 * spread;
    sorted
        .iter()
        .filter(|value| **value < low || **value > high)
        .count()
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod trim_tests;
