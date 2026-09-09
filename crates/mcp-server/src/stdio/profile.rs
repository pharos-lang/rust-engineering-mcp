//! M5-03: `rust.profile.flamegraph`, one sampled CPU profile of one binary.
//!
//! The host's profiling grant is checked here, before anything is dispatched
//! (ADR-074 §2). The peer, the project, the URI and the tool annotations cannot
//! produce it; only the host configuration can, and without it this tool is
//! `blocked` with `PROFILING_NOT_AUTHORIZED` and no container is created.
//!
//! Zero samples is a valid, declared observation (ADR-074 §5), never an error.
#[allow(dead_code)]
mod schemas;
use super::{
    HostCargoVendorConfig, HostProfilingConfig, ProfilingGrant,
    benchmark::{ModeSelection, mode_selection},
    clock::WallClock,
    nextest::ExecutionModeDto,
    project::Registry,
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
use rust_engineering_application::profile::{
    PROFILING_NOT_AUTHORIZED, ProfileObservation, ProfileOptions, ProfilePorts, ProfilePublisher,
    ProfilingAuthorization, ProjectProfilePort, PublishedProfile,
};
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::profile::{
    PROFILE_DEFAULT_DURATION_SECONDS, PROFILE_DEFAULT_FREQUENCY_HZ, ProfileBuildOutcome,
    ProfileCompleteness, ProfileStatus,
};
use rust_engineering_domain::{
    ArtifactCompleteness, ExecutionTermination, ProjectRef, QualityArtifactDescriptor,
    QualityArtifactKind, RuntimeIdentity,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const NAME: &str = "rust.profile.flamegraph";
const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
/// The ranking is bounded, and the bound belongs to the product (ADR-076 §5).
/// It is a ceiling, not a target: the sampler's own ranking is far smaller,
/// and the 512 KiB response budget binds before this does.
const MAX_RESPONSE_FRAMES: usize = 1024;

pub(super) fn advertised() -> bool {
    super::security_tool::advertised("RUST_MCP_TEST_PROFILE_READY")
}

#[derive(Clone, serde::Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    /// A target name, never a path, and the child receives no peer argument.
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]{1,64}$"))]
    binary_target: String,
    #[serde(default = "default_frequency")]
    #[schemars(range(min = 1, max = 999))]
    frequency_hz: u32,
    #[serde(default = "default_duration")]
    #[schemars(range(min = 1, max = 60))]
    duration_seconds: u64,
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 300))]
    timeout_seconds: u64,
    #[serde(default)]
    execution_mode: ExecutionModeDto,
}
fn default_frequency() -> u32 {
    PROFILE_DEFAULT_FREQUENCY_HZ
}
fn default_duration() -> u64 {
    PROFILE_DEFAULT_DURATION_SECONDS
}
fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECONDS
}
impl Input {
    fn options(&self) -> Result<ProfileOptions, ErrorData> {
        ProfileOptions::new(
            self.binary_target.clone(),
            self.frequency_hz,
            self.duration_seconds,
        )
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }
}

#[derive(Clone, Copy, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    TasksRequired,
    ProfilingNotAuthorized,
    SandboxDenied,
    MissingOfflineData,
    ArtifactUnavailable,
    ToolNotInstalled,
    InvalidProject,
    ProjectNotFound,
    CommandTimeout,
    OutputLimitExceeded,
    EvidenceIncomplete,
    ProfilerUnavailable,
    ObservedFailure,
}
define_fallible_security_outcome!(Code, &'static str, ());

#[derive(Clone, Copy, serde::Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    FlamegraphSvg,
    CollapsedStacks,
}
#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct Artifact {
    kind: ArtifactKind,
    #[schemars(length(min = 1, max = 512))]
    uri: String,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    sha256: String,
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
    #[schemars(length(max = 2))]
    artifacts: Vec<Artifact>,
}

/// The runtime this tool samples through.
///
/// `profiling` is the host grant of ADR-074 §2. `executor` and `publisher` are
/// optional for the same reason as the benchmark tool's: the M5 execution
/// vertical and the durable SVG/stacks publisher land separately, and until
/// then the tool answers a declared `unavailable`, never a protocol error.
pub(super) struct Runtime {
    pub(super) registry: Arc<Mutex<Registry>>,
    pub(super) workers: Workers,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) vendor: Option<HostCargoVendorConfig>,
    pub(super) profiling: Option<HostProfilingConfig>,
    pub(super) executor: Option<Arc<dyn ProjectProfilePort>>,
    pub(super) publisher: Option<Arc<Mutex<dyn ProfilePublisher>>>,
}

/// `ProfilePorts` takes sized ports; these two forward a shared handle into
/// that shape without asking the application to know about `dyn`.
struct DynExecutor<'a>(&'a dyn ProjectProfilePort);
impl ProjectProfilePort for DynExecutor<'_> {
    fn profile(
        &self,
        source: &rust_engineering_domain::SourceBundle,
        vendor: &rust_engineering_domain::CargoVendorSnapshot,
        options: &ProfileOptions,
        control: &dyn rust_engineering_application::InspectionControl,
    ) -> Result<ProfileObservation, SecurityError> {
        self.0.profile(source, vendor, options, control)
    }
}
struct DynPublisher<'a>(&'a mut dyn ProfilePublisher);
impl ProfilePublisher for DynPublisher<'_> {
    fn publish_profile(
        &mut self,
        capture: &rust_engineering_application::security::SecurityCapture,
        observation: &ProfileObservation,
        revalidate: &mut dyn FnMut() -> Result<
            rust_engineering_application::QualityOwnerFacts,
            InspectionError,
        >,
    ) -> Result<Vec<QualityArtifactDescriptor>, InspectionError> {
        self.0.publish_profile(capture, observation, revalidate)
    }
}

define_security_tool!(
    ProfileTool,
    "Sample one project binary on CPU inside the approved profiling sandbox and publish a sanitized flame graph and the collapsed stacks as private artifacts. Requires an explicit host profiling grant; without it the call is blocked before any container is created. The binary is named by cargo target, never by path, and the child receives no argument from the caller. The response reports the sampler identity and parameters, the requested and the observed duration, samples collected and lost, stacks written, frames total and unresolved, stacks truncated, modules seen, the child's exit or signal, and a bounded ranking of frames by self and total samples. Symbol names are sanitized to a closed alphabet and no filesystem path is emitted. Collecting zero samples is a valid, declared observation, not a failure."
);

impl ProfileTool {
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
                "Profiling runs are not admitted as MCP Tasks",
                None,
                0,
            );
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ErrorData::internal_error("Profiling runtime is not configured", None)
        })?;
        // ADR-074 §2: the grant is decided before discovery, before the vendor
        // tree, before the worker and before any container.
        let authorization = match runtime.profiling {
            Some(HostProfilingConfig {
                grant: ProfilingGrant::UserSpaceSampling,
            }) => ProfilingAuthorization::Granted,
            None => {
                return self.blocked(
                    Code::ProfilingNotAuthorized,
                    "The host has not granted this server the profiling capability",
                    None,
                    0,
                );
            }
        };
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Discovery must complete before profiling",
                None,
                0,
            );
        }
        let Some(vendor) = runtime.vendor.clone() else {
            return self.unavailable(
                Code::MissingOfflineData,
                "Host-authenticated offline vendor is required",
                0,
            );
        };
        let Some(publisher) = runtime.publisher.clone() else {
            return self.unavailable(
                Code::ArtifactUnavailable,
                "Durable profiling evidence is unavailable",
                0,
            );
        };
        let Some(executor) = runtime.executor.clone() else {
            return self.unavailable(
                Code::ToolNotInstalled,
                "Approved profiling runtime is unavailable",
                0,
            );
        };
        let registry = Arc::clone(&runtime.registry);
        let reference = input.project_ref.clone();
        let (result, duration) = run_joined_security(
            &runtime.workers,
            request_token,
            input.timeout_seconds,
            "Profiling worker unavailable",
            move |control| {
                let vendor = capture_vendor(&vendor, control)?;
                let executor = DynExecutor(executor.as_ref());
                let mut published = publisher
                    .lock()
                    .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?;
                let mut published = DynPublisher(&mut *published);
                registry
                    .lock()
                    .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?
                    .profile_durable(
                        &reference,
                        &vendor,
                        &options,
                        authorization,
                        ProfilePorts {
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
        // The application refuses an ungranted capability with the host's own
        // containment rejection; it is named here rather than folded into the
        // generic project rejection it shares a shape with.
        if error == PROFILING_NOT_AUTHORIZED {
            return self.blocked(
                Code::ProfilingNotAuthorized,
                "The host has not granted this server the profiling capability",
                None,
                duration_ms,
            );
        }
        let (code, message) = match classify_error(error) {
            CommonFailure::Cancelled => {
                return self.cancelled("Profiling cancelled after joined cleanup", duration_ms);
            }
            CommonFailure::ToolNotInstalled => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved profiling runtime is unavailable",
                    duration_ms,
                );
            }
            CommonFailure::Timeout => (Code::CommandTimeout, "Profiling exceeded its deadline"),
            CommonFailure::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            CommonFailure::OutputLimit => (
                Code::OutputLimitExceeded,
                "Profiling evidence exceeded its fixed budget",
            ),
            CommonFailure::ProjectNotFound => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            CommonFailure::SandboxDenied => (
                Code::SandboxDenied,
                "Approved profiling execution could not be established",
            ),
            CommonFailure::Specific(_) => (
                Code::InvalidProject,
                "Captured profiling inputs or evidence could not be validated",
            ),
        };
        self.blocked(code, message, None, duration_ms)
    }

    fn encode_result(
        &self,
        reference: &ProjectRef,
        result: PublishedProfile,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let artifacts = result
            .artifacts
            .iter()
            .map(|descriptor| artifact(reference, descriptor))
            .collect::<Result<Vec<_>, _>>()?;
        let artifacts_complete = result
            .artifacts
            .iter()
            .all(|descriptor| descriptor.completeness == ArtifactCompleteness::Complete);
        let data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "observed_cpu_samples_of_one_execution_without_attribution",
            observation: observation(
                &result.observation,
                artifacts_complete,
                result.artifacts.is_empty(),
            ),
            artifacts,
        });
        encode_bounded(
            &self.contract,
            data,
            duration_ms,
            "Profiling serialization failed",
            |data, duration_ms| Output {
                outcome: outcome(data),
                summary: "Observed CPU samples of one execution on this host",
                duration_ms,
            },
            |data| {
                // The lowest-ranked frame leaves first; the collapsed stacks
                // artifact still carries every stack the sampler wrote.
                if data.observation.top_frames.pop().is_none() {
                    return false;
                }
                data.observation.top_frames_omitted =
                    data.observation.top_frames_omitted.saturating_add(1);
                data.observation.complete = false;
                true
            },
            |duration_ms| Output {
                outcome: Outcome::Blocked {
                    error_code: Code::OutputLimitExceeded,
                    error_message: "Profiling response exceeds its fixed budget",
                    data: None,
                },
                summary: "Profiling response exceeds its fixed budget",
                duration_ms,
            },
        )
    }
}

fn outcome(data: &Data) -> Outcome {
    let observation = &data.observation;
    if observation.complete {
        return Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(data.clone()),
        };
    }
    if matches!(
        observation.build,
        schemas::BuildOutcome::CompilationFailed | schemas::BuildOutcome::TargetNotFound
    ) {
        return Outcome::Failed {
            error_code: Code::ObservedFailure,
            error_message: "The profiled target did not build or does not exist in this project",
            data: Box::new(data.clone()),
        };
    }
    let (error_code, error_message) =
        if observation.completeness == schemas::Completeness::Unavailable {
            (
                Code::ProfilerUnavailable,
                "The sampler could not open a performance event; the reported errno is the refusal",
            )
        } else {
            (
                Code::EvidenceIncomplete,
                "Profiling evidence is partial; lost samples or truncated stacks are declared",
            )
        };
    Outcome::Blocked {
        error_code,
        error_message,
        data: Some(Box::new(data.clone())),
    }
}

fn artifact(
    reference: &ProjectRef,
    descriptor: &QualityArtifactDescriptor,
) -> Result<Artifact, ErrorData> {
    let kind = match descriptor.kind {
        QualityArtifactKind::FlamegraphSvg => ArtifactKind::FlamegraphSvg,
        QualityArtifactKind::CollapsedStacks => ArtifactKind::CollapsedStacks,
        _ => {
            return Err(ErrorData::internal_error(
                "Profiling artifact kind is invalid",
                None,
            ));
        }
    };
    let fields = artifact_fields(
        reference,
        descriptor,
        "Invalid profiling artifact descriptor",
    )?;
    Ok(Artifact {
        kind,
        uri: fields.uri,
        sha256: fields.sha256,
        size_bytes: fields.size_bytes,
        completeness: match fields.completeness {
            ArtifactCompleteness::Complete => schemas::ArtifactCompleteness::Complete,
            ArtifactCompleteness::Truncated => schemas::ArtifactCompleteness::Truncated,
            ArtifactCompleteness::Partial => schemas::ArtifactCompleteness::Partial,
            ArtifactCompleteness::Invalid => schemas::ArtifactCompleteness::Invalid,
            ArtifactCompleteness::Unavailable => schemas::ArtifactCompleteness::Unavailable,
        },
    })
}

fn observation(
    value: &ProfileObservation,
    artifacts_complete: bool,
    artifacts_absent: bool,
) -> schemas::Observation {
    let mut top_frames: Vec<schemas::FrameWeight> = value
        .top_frames
        .iter()
        .map(|frame| schemas::FrameWeight {
            frame: frame.frame.clone(),
            self_samples: frame.self_samples,
            total_samples: frame.total_samples,
        })
        .collect();
    // Ranked so the trimming strategy drops the least weighted frame first.
    top_frames.sort_by(|left, right| {
        right
            .self_samples
            .cmp(&left.self_samples)
            .then_with(|| right.total_samples.cmp(&left.total_samples))
            .then_with(|| left.frame.cmp(&right.frame))
    });
    let top_frames_omitted =
        u32::try_from(top_frames.len().saturating_sub(MAX_RESPONSE_FRAMES)).unwrap_or(u32::MAX);
    top_frames.truncate(MAX_RESPONSE_FRAMES);
    let completeness = match value.completeness {
        ProfileCompleteness::Complete => schemas::Completeness::Complete,
        ProfileCompleteness::LostSamples => schemas::Completeness::LostSamples,
        ProfileCompleteness::NoSamples => schemas::Completeness::NoSamples,
        ProfileCompleteness::Truncated => schemas::Completeness::Truncated,
        ProfileCompleteness::Unavailable => schemas::Completeness::Unavailable,
    };
    let build = match value.build {
        ProfileBuildOutcome::Built => schemas::BuildOutcome::Built,
        ProfileBuildOutcome::CompilationFailed => schemas::BuildOutcome::CompilationFailed,
        ProfileBuildOutcome::TargetNotFound => schemas::BuildOutcome::TargetNotFound,
    };
    schemas::Observation {
        backend: value.backend.to_owned(),
        binary_target: value.options.binary_target().to_owned(),
        frequency_hz: value.options.frequency_hz(),
        requested_duration_seconds: value.options.duration_seconds(),
        observed_duration_ms: value.counters.observed_duration_ms,
        build,
        build_exit_code: value.build_exit_code,
        status: match value.status {
            ProfileStatus::Complete => schemas::ProfilerStatus::Complete,
            ProfileStatus::SampleLimit => schemas::ProfilerStatus::SampleLimit,
            ProfileStatus::DurationLimit => schemas::ProfilerStatus::DurationLimit,
            ProfileStatus::ChildExited => schemas::ProfilerStatus::ChildExited,
            ProfileStatus::ProfilerUnavailable => schemas::ProfilerStatus::ProfilerUnavailable,
        },
        samples_collected: value.counters.samples_collected,
        samples_lost: value.counters.samples_lost,
        stacks_written: value.counters.stacks_written,
        frames_total: value.counters.frames_total,
        frames_unresolved: value.counters.frames_unresolved,
        stacks_truncated: value.counters.stacks_truncated,
        modules_seen: value.counters.modules_seen,
        max_depth_applied: value.counters.max_depth_applied,
        child_exit_code: value.child.exit_code,
        child_signal: value.child.signal,
        perf_errno: value.perf_errno,
        // Zero samples is a valid, declared observation: it is complete when
        // the sampler ran, published what it had and lost nothing.
        complete: build == schemas::BuildOutcome::Built
            && matches!(
                completeness,
                schemas::Completeness::Complete | schemas::Completeness::NoSamples
            )
            && !(completeness == schemas::Completeness::Complete && artifacts_absent)
            && artifacts_complete
            && top_frames_omitted == 0,
        completeness,
        top_frames,
        top_frames_omitted,
        termination: match value.termination {
            ExecutionTermination::Exited => schemas::ExecutionTermination::Exited,
            ExecutionTermination::TimedOut => schemas::ExecutionTermination::TimedOut,
            ExecutionTermination::Cancelled => schemas::ExecutionTermination::Cancelled,
            ExecutionTermination::OutputLimit => schemas::ExecutionTermination::OutputLimit,
        },
        runtime: runtime_identity(&value.runtime),
        execution_fingerprint: value.execution_fingerprint.to_string(),
        vendor_fingerprint: value.vendor_fingerprint.to_string(),
        stdout_truncated: value.stdout_truncated,
        stderr_truncated: value.stderr_truncated,
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

#[cfg(test)]
mod tests;
#[cfg(test)]
mod trim_tests;
