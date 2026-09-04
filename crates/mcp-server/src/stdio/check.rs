//! One admitted check owns capture, execution, normalization and log publication.
#[allow(dead_code)]
pub(super) mod schemas;
use super::{
    contract::{Contract, ToolOutput},
    project::Registry,
    resources::{self, ArtifactClock, Store},
    workers::{Joined, WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::{
    CheckOptions, CheckOutcome, CheckSelection, Clock, Diagnostic, Evidence, ExecutionTermination,
    OperationalErrorCode, ProjectCheck, ProjectRef, RuntimeIdentity, ToolStatus, UnixSeconds,
};
use rust_engineering_execution::RustProjectInspector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub(super) const NAME: &str = "rust.check";
const DEADLINE: Duration = Duration::from_secs(120);
const MAX_RESULT: usize = 512 * 1024;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    #[serde(default)]
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
    )]
    package: Option<String>,
    #[serde(default)]
    workspace: bool,
    #[serde(default)]
    #[schemars(with = "Vec<schemas::Feature>", length(max = 32))]
    features: Vec<String>,
    #[serde(default)]
    all_features: bool,
    #[serde(default)]
    no_default_features: bool,
    #[serde(default)]
    all_targets: bool,
    #[serde(default)]
    #[schemars(regex(pattern = "^aarch64-unknown-linux-gnu$"))]
    target: Option<String>,
}
impl Input {
    fn options(&self) -> Result<CheckOptions, ErrorData> {
        CheckSelection {
            package: self.package.clone(),
            workspace: self.workspace,
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            all_targets: self.all_targets,
            target: self.target.clone(),
        }
        .try_into()
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Semantics {
    LatestKnown,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Log {
    #[schemars(regex(pattern = "^rust-artifact://prj_[0-9a-f]{32}/art_[0-9a-f]{32}$"))]
    uri: String,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    sha256: String,
    #[schemars(range(max = 262144))]
    size_bytes: u32,
    truncated: bool,
    retention_remaining_seconds: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    project_identity_fingerprint: String,
    semantics: Semantics,
    #[schemars(with = "schemas::CheckOptions")]
    options: CheckOptions,
    validation_complete: bool,
    #[schemars(with = "schemas::ExecutionTermination")]
    termination: ExecutionTermination,
    exit_code: Option<i32>,
    #[schemars(with = "super::inspection::schemas::RuntimeIdentity")]
    runtime: RuntimeIdentity,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    source_fingerprint: String,
    log: Option<Log>,
    log_unavailable_reason: Option<LogUnavailableReason>,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum LogUnavailableReason {
    RetentionCapacity,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    ProjectNotFound,
    InvalidProject,
    ToolNotInstalled,
    LockfileUpdateRequired,
    CommandTimeout,
    SandboxDenied,
    NetworkDenied,
    UnsupportedPlatform,
    OutputLimitExceeded,
}
#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome {
    Passed {
        error_code: (),
        error_message: (),
        data: Box<Data>,
    },
    Failed {
        error_code: (),
        error_message: (),
        data: Box<Data>,
    },
    Blocked {
        error_code: Code,
        error_message: &'static str,
        data: Option<Box<Data>>,
    },
    Unavailable {
        error_code: Code,
        error_message: &'static str,
        data: (),
    },
    Cancelled {
        error_code: (),
        error_message: (),
        data: (),
    },
}
#[derive(Default, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Truncation {
    stdout_truncated: bool,
    stderr_truncated: bool,
    diagnostics_omitted: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Output {
    #[serde(flatten)]
    outcome: Outcome,
    summary: &'static str,
    duration_ms: u64,
    /// Normalized project-writable Cargo output; compiler origin is not authenticated.
    #[schemars(with = "Vec<schemas::Diagnostic>", length(max = 128))]
    diagnostics: Vec<Diagnostic>,
    truncation: Truncation,
    #[schemars(with = "super::inspection::schemas::Evidence")]
    evidence: Evidence,
}
impl ToolOutput for Output {
    fn status(&self) -> ToolStatus {
        match self.outcome {
            Outcome::Passed { .. } => ToolStatus::Passed,
            Outcome::Failed { .. } => ToolStatus::Failed,
            Outcome::Blocked { .. } => ToolStatus::Blocked,
            Outcome::Unavailable { .. } => ToolStatus::Unavailable,
            Outcome::Cancelled { .. } => ToolStatus::Cancelled,
        }
    }
}
fn operational(code: OperationalErrorCode) -> (Outcome, &'static str) {
    let (code, message, unavailable) = match code {
        OperationalErrorCode::ProjectNotFound => (
            Code::ProjectNotFound,
            "Project reference is missing or expired",
            false,
        ),
        OperationalErrorCode::InvalidProject => (
            Code::InvalidProject,
            "Captured project is invalid or unsupported",
            false,
        ),
        OperationalErrorCode::ToolNotInstalled => (
            Code::ToolNotInstalled,
            "Approved local runtime is unavailable",
            true,
        ),
        OperationalErrorCode::LockfileUpdateRequired => (
            Code::LockfileUpdateRequired,
            "Lockfile update is required",
            false,
        ),
        OperationalErrorCode::CommandTimeout => {
            (Code::CommandTimeout, "Check exceeded its deadline", false)
        }
        OperationalErrorCode::SandboxDenied => (
            Code::SandboxDenied,
            "Host runtime policy, failed calibration or current capacity denied check",
            false,
        ),
        OperationalErrorCode::NetworkDenied => {
            (Code::NetworkDenied, "Network access is denied", false)
        }
        OperationalErrorCode::UnsupportedPlatform => (
            Code::UnsupportedPlatform,
            "Secure check is unavailable on this platform",
            true,
        ),
        OperationalErrorCode::OutputLimitExceeded => (
            Code::OutputLimitExceeded,
            "Check evidence could not be retained within the output budget",
            false,
        ),
    };
    (
        if unavailable {
            Outcome::Unavailable {
                error_code: code,
                error_message: message,
                data: (),
            }
        } else {
            Outcome::Blocked {
                error_code: code,
                error_message: message,
                data: None,
            }
        },
        message,
    )
}
fn output(
    result: Result<ProjectCheck, InspectionError>,
    duration_ms: u64,
) -> Result<Output, ErrorData> {
    let mut evidence = Evidence::Local;
    let mut diagnostics = Vec::new();
    let mut truncation = Truncation::default();
    let (outcome, summary) = match result {
        Ok(project) => {
            evidence = Evidence::Snapshot(project.evidence);
            let observation = project.observation;
            diagnostics = observation.diagnostics;
            truncation = Truncation {
                stdout_truncated: observation.stdout_truncated,
                stderr_truncated: observation.stderr_truncated,
                diagnostics_omitted: observation.diagnostics_omitted,
            };
            let log = match (project.log, project.retention_remaining_seconds) {
                (Some(metadata), Some(retention)) if retention > 0 => {
                    let mut hash = String::with_capacity(64);
                    for byte in metadata.sha256 {
                        use std::fmt::Write;
                        write!(&mut hash, "{byte:02x}").map_err(|_| {
                            ErrorData::internal_error("Artifact encoding failed", None)
                        })?;
                    }
                    Some(Log {
                        uri: resources::uri(&metadata.owner, &metadata.id),
                        sha256: hash,
                        size_bytes: metadata.size_bytes,
                        truncated: metadata.truncated,
                        retention_remaining_seconds: retention,
                    })
                }
                (None, None) => None,
                _ => {
                    return Err(ErrorData::internal_error(
                        "Artifact retention metadata failed",
                        None,
                    ));
                }
            };
            let has_log = log.is_some();
            let data = Box::new(Data {
                project_ref: project.project_ref.to_string(),
                project_identity_fingerprint: project.project_identity_fingerprint.to_string(),
                semantics: Semantics::LatestKnown,
                options: project.options,
                validation_complete: observation.validation_complete,
                termination: observation.termination,
                exit_code: observation.exit_code,
                runtime: observation.runtime,
                source_fingerprint: observation.source_fingerprint.to_string(),
                log,
                log_unavailable_reason: (!has_log)
                    .then_some(LogUnavailableReason::RetentionCapacity),
            });
            if observation.termination == ExecutionTermination::TimedOut {
                (
                    Outcome::Blocked {
                        error_code: Code::CommandTimeout,
                        error_message: "Check exceeded its deadline",
                        data: Some(data),
                    },
                    "Check timed out; partial evidence is retained",
                )
            } else {
                match observation.outcome {
                    CheckOutcome::LockfileUpdateRequired => (
                        Outcome::Blocked {
                            error_code: Code::LockfileUpdateRequired,
                            error_message: "Lockfile update is required",
                            data: Some(data),
                        },
                        "Frozen Cargo requires a lockfile update; retained source was not changed",
                    ),
                    CheckOutcome::Passed => (
                        Outcome::Passed {
                            error_code: (),
                            error_message: (),
                            data,
                        },
                        if has_log {
                            "Captured project passed Cargo check in the approved offline runtime"
                        } else {
                            "Cargo check passed; log retention capacity exhausted"
                        },
                    ),
                    CheckOutcome::Failed => (
                        Outcome::Failed {
                            error_code: (),
                            error_message: (),
                            data,
                        },
                        if has_log {
                            "Cargo check reported compilation failure"
                        } else {
                            "Compilation failure reported; log retention capacity exhausted"
                        },
                    ),
                    CheckOutcome::Incomplete => (
                        Outcome::Failed {
                            error_code: (),
                            error_message: (),
                            data,
                        },
                        if has_log {
                            "Validation incomplete; inspect retained diagnostics and log"
                        } else {
                            "Validation incomplete; log retention capacity exhausted"
                        },
                    ),
                }
            }
        }
        Err(InspectionError::Project(ProjectError::Rejected(code))) => operational(code),
        Err(
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled),
        ) => (
            Outcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Check cancelled after worker completion",
        ),
        Err(InspectionError::Execution(ExecutionError::Unavailable)) => {
            operational(OperationalErrorCode::ToolNotInstalled)
        }
        Err(InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        )) => operational(OperationalErrorCode::SandboxDenied),
        Err(InspectionError::OutputLimit) => operational(OperationalErrorCode::OutputLimitExceeded),
        Err(InspectionError::InvalidMetadata) => operational(OperationalErrorCode::InvalidProject),
        Err(InspectionError::Execution(ExecutionError::CleanupUncertain)) => {
            return Err(ErrorData::internal_error(
                "Gateway cleanup could not be verified; further execution is quarantined",
                None,
            ));
        }
        Err(
            InspectionError::Internal
            | InspectionError::Project(ProjectError::Internal)
            | InspectionError::Execution(ExecutionError::Infrastructure),
        ) => return Err(ErrorData::internal_error("Check failed", None)),
    };
    Ok(Output {
        outcome,
        summary,
        duration_ms,
        diagnostics,
        truncation,
        evidence,
    })
}
fn worker_error(error: WorkerError) -> InspectionError {
    match error {
        WorkerError::Busy => InspectionError::Execution(ExecutionError::Busy),
        WorkerError::Cancelled => InspectionError::Project(ProjectError::Cancelled),
        WorkerError::TimedOut => {
            InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
        }
        WorkerError::Internal => InspectionError::Internal,
    }
}
fn joined_result<T>(joined: Joined<T, InspectionError>) -> Result<T, InspectionError> {
    match (joined.result, joined.interrupted) {
        (
            Err(
                InspectionError::Project(ProjectError::Cancelled)
                | InspectionError::Execution(ExecutionError::Cancelled),
            ),
            Some(signal),
        ) => Err(worker_error(signal)),
        (Err(error), _) => Err(error),
        (Ok(_), Some(signal)) => Err(worker_error(signal)),
        (Ok(value), None) => Ok(value),
    }
}
struct WallClock;
impl Clock for WallClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|v| v.as_secs())
                .unwrap_or(0),
        )
    }
}
pub(super) struct CheckTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
    store: Arc<Mutex<Store>>,
    clock: ArtifactClock,
}
impl CheckTool {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
        resources: &resources::Resources,
    ) -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Check captured Rust source using host-approved offline Cargo. Can execute build scripts and proc macros inside the calibrated sandbox. Accepts only bounded closed Cargo selections. Returns normalized diagnostics and an ephemeral owner-authorized log Resource. Project code may write the diagnostic stream; normalization does not authenticate its origin. Requires a live project_ref and completed discovery; installs nothing.",(*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema)).with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false));
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
            store: resources.store(),
            clock: resources.clock(),
        })
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let options = input.options()?;
        let started = Instant::now();
        let bootstrap = !self.ready.load(Ordering::Acquire);
        let result = if bootstrap {
            Err(InspectionError::Execution(ExecutionError::Denied))
        } else {
            let registry = Arc::clone(&self.registry);
            let inspector = Arc::clone(&self.inspector);
            let store = Arc::clone(&self.store);
            let clock = self.clock.clone();
            match self
                .workers
                .run_joined(context.ct, started + DEADLINE, move |control| {
                    registry
                        .lock()
                        .map_err(|_| InspectionError::Internal)?
                        .check(
                            &input.project_ref,
                            &options,
                            inspector.as_ref(),
                            &mut *store.lock().map_err(|_| InspectionError::Internal)?,
                            (&WallClock, &clock),
                            control,
                        )
                })
                .await
            {
                Ok(joined) => joined_result(joined),
                Err(error) => Err(worker_error(error)),
            }
        };
        let duration = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let mut value = output(result, duration)?;
        if bootstrap {
            let message = "Check requires completed discovery; retry with a new request ID";
            value.summary = message;
            value.outcome = Outcome::Blocked {
                error_code: Code::SandboxDenied,
                error_message: message,
                data: None,
            };
        }
        encode_bounded(&self.contract, value)
    }
}
fn encode_bounded(
    contract: &Contract<Input, Output>,
    mut value: Output,
) -> Result<CallToolResult, ErrorData> {
    // Normalize size before encoding both structured content and its text fallback.
    // Dropping diagnostics preserves the authorized log link and visible truncation.
    while serde_json::to_vec(&value)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > MAX_RESULT / 4
        && !value.diagnostics.is_empty()
    {
        value.diagnostics.pop();
        value.truncation.diagnostics_omitted += 1;
        match &mut value.outcome {
            Outcome::Passed { data, .. } | Outcome::Failed { data, .. } => {
                data.validation_complete = false
            }
            Outcome::Blocked {
                data: Some(data), ..
            } => data.validation_complete = false,
            _ => (),
        }
        if let Outcome::Passed { data, .. } = value.outcome {
            value.outcome = Outcome::Failed {
                error_code: (),
                error_message: (),
                data,
            };
            value.summary = "Validation incomplete; inspect retained diagnostics and log";
        }
    }
    let duration = value.duration_ms;
    let encoded = contract.encode(value)?;
    if serde_json::to_vec(&encoded)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > MAX_RESULT
    {
        return contract.encode(output(Err(InspectionError::OutputLimit), duration)?);
    }
    Ok(encoded)
}
#[cfg(test)]
mod tests;
