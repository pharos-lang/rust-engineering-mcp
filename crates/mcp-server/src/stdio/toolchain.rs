// These types are used only to derive schemas; domain values own wire serialization.
#[allow(dead_code)]
pub(super) mod schemas;
use super::{
    contract::{Contract, ToolOutput},
    project::Registry,
    workers::{Joined, WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::{
    Clock, Evidence, OperationalErrorCode, ProjectRef, ToolStatus, ToolchainInspection,
    ToolchainObservation, UnixSeconds,
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

pub(super) const NAME: &str = "rust.toolchain.inspect";
const DEADLINE: Duration = Duration::from_secs(120);
const MAX_RESULT: usize = 64 * 1024;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Semantics {
    LatestKnown,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    project_identity_fingerprint: String,
    semantics: Semantics,
    #[schemars(with = "schemas::ToolchainObservation")]
    observation: ToolchainObservation,
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
    Blocked {
        error_code: Code,
        error_message: &'static str,
        data: (),
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
    diagnostics: [(); 0],
    truncation: Truncation,
    #[schemars(with = "super::inspection::schemas::Evidence")]
    evidence: Evidence,
}
impl ToolOutput for Output {
    fn status(&self) -> ToolStatus {
        match self.outcome {
            Outcome::Passed { .. } => ToolStatus::Passed,
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
            "Captured project or runtime observations are invalid or unsupported",
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
        OperationalErrorCode::CommandTimeout => (
            Code::CommandTimeout,
            "Inspection exceeded its deadline",
            false,
        ),
        OperationalErrorCode::SandboxDenied => (
            Code::SandboxDenied,
            "Host runtime policy, failed calibration or current capacity denied inspection",
            false,
        ),
        OperationalErrorCode::NetworkDenied => {
            (Code::NetworkDenied, "Network access is denied", false)
        }
        OperationalErrorCode::UnsupportedPlatform => (
            Code::UnsupportedPlatform,
            "Secure inspection is unavailable on this platform",
            true,
        ),
        OperationalErrorCode::OutputLimitExceeded => (
            Code::OutputLimitExceeded,
            "Toolchain observations exceed the response budget",
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
                data: (),
            }
        },
        message,
    )
}
fn output(
    result: Result<ToolchainInspection, InspectionError>,
    duration_ms: u64,
) -> Result<Output, ErrorData> {
    let mut evidence = Evidence::Local;
    let mut truncation = Truncation::default();
    let (outcome, summary) = match result {
        Ok(project) => {
            evidence = Evidence::Snapshot(project.evidence);
            (
                Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: Box::new(Data {
                        project_ref: project.project_ref.to_string(),
                        project_identity_fingerprint: project
                            .project_identity_fingerprint
                            .to_string(),
                        semantics: Semantics::LatestKnown,
                        observation: project.observation,
                    }),
                },
                "Installed toolchain observed in the approved offline Linux runtime",
            )
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
            "Toolchain inspection cancelled after worker completion",
        ),
        Err(InspectionError::Execution(ExecutionError::Unavailable)) => {
            operational(OperationalErrorCode::ToolNotInstalled)
        }
        Err(InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        )) => operational(OperationalErrorCode::SandboxDenied),
        Err(InspectionError::InvalidMetadata) => {
            return Err(ErrorData::internal_error(
                "Runtime inventory validation failed",
                None,
            ));
        }
        Err(InspectionError::OutputLimit) => {
            truncation.stdout_truncated = true;
            operational(OperationalErrorCode::OutputLimitExceeded)
        }
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
        ) => {
            return Err(ErrorData::internal_error(
                "Toolchain inspection failed",
                None,
            ));
        }
    };
    Ok(Output {
        outcome,
        summary,
        duration_ms,
        diagnostics: [],
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
        // Actual failures, especially cleanup uncertainty, survive cancellation
        // and deadlines. Only cancellation itself is reclassified by the signal.
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
pub(super) struct ToolchainTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
}
impl ToolchainTool {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Observe rustc/Cargo versions, stable channel, guest host triple and installed targets/components in the host-approved offline Linux ARM64 runtime. Includes runtime execution identities and captured-project snapshot evidence. Requires a live project_ref, completed discovery and explicit host runtime policy; installs nothing.",(*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema))
            .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(false));
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
        })
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let started = Instant::now();
        let bootstrap = !self.ready.load(Ordering::Acquire);
        let result = if bootstrap {
            Err(InspectionError::Execution(ExecutionError::Denied))
        } else {
            let registry = Arc::clone(&self.registry);
            let inspector = Arc::clone(&self.inspector);
            match self
                .workers
                .run_joined(context.ct, started + DEADLINE, move |control| {
                    registry
                        .lock()
                        .map_err(|_| InspectionError::Internal)?
                        .inspect_toolchain(
                            &input.project_ref,
                            inspector.as_ref(),
                            &WallClock,
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
            let message = "Inspection requires completed discovery; retry with a new request ID";
            value.summary = message;
            value.outcome = Outcome::Blocked {
                error_code: Code::SandboxDenied,
                error_message: message,
                data: (),
            };
        }
        encode_bounded(&self.contract, value)
    }
}
fn encode_bounded(
    contract: &Contract<Input, Output>,
    value: Output,
) -> Result<CallToolResult, ErrorData> {
    let duration = value.duration_ms;
    let result = contract.encode(value)?;
    if serde_json::to_vec(&result)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > MAX_RESULT
    {
        return contract.encode(output(Err(InspectionError::OutputLimit), duration)?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
