// These types are used only to derive schemas; domain values own wire serialization.
#[allow(dead_code)]
pub(super) mod schemas;
use super::{
    contract::{Contract, ToolOutput},
    workers::{Joined, WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::{
    Clock, DiagnosticCode, DiagnosticExplanation, Evidence, ExecutionTermination,
    ExplainObservation, OperationalErrorCode, ToolStatus, UnixSeconds,
};
use rust_engineering_execution::RustProjectInspector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(super) const NAME: &str = "rust.diagnostics.explain";
const DEADLINE: Duration = Duration::from_secs(120);
const MAX_RESULT: usize = 512 * 1024;
const MAX_EXPLANATION: usize = 64 * 1024;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^E[0-9]{4}$"))]
    code: DiagnosticCode,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Semantics {
    LatestKnown,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    semantics: Semantics,
    #[schemars(with = "schemas::ExplainObservation")]
    observation: ExplainObservation,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    DiagnosticExplanationUnavailable,
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
        data: Option<Box<Data>>,
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
    #[schemars(with = "schemas::Evidence")]
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
            "Compiler explanations exceed the response budget",
            false,
        ),
    };
    (
        if unavailable {
            Outcome::Unavailable {
                error_code: code,
                error_message: message,
                data: None,
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
    result: Result<DiagnosticExplanation, InspectionError>,
    duration_ms: u64,
) -> Result<Output, ErrorData> {
    let mut evidence = Evidence::Local;
    let mut truncation = Truncation::default();
    let (outcome, summary) = match result {
        Ok(explanation) => {
            let observed = explanation.observation;
            evidence = Evidence::Snapshot(explanation.evidence);
            truncation.stdout_truncated = observed.stdout_truncated
                || observed
                    .explanation
                    .as_ref()
                    .is_some_and(|text| text.len() > MAX_EXPLANATION);
            truncation.stderr_truncated = observed.stderr_truncated;
            if observed.stdout_truncated
                || observed.stderr_truncated
                || observed.termination == ExecutionTermination::OutputLimit
                || observed
                    .explanation
                    .as_ref()
                    .is_some_and(|text| text.len() > MAX_EXPLANATION)
            {
                operational(OperationalErrorCode::OutputLimitExceeded)
            } else if observed.termination == ExecutionTermination::TimedOut {
                operational(OperationalErrorCode::CommandTimeout)
            } else if observed.termination == ExecutionTermination::Cancelled {
                (
                    Outcome::Cancelled {
                        error_code: (),
                        error_message: (),
                        data: (),
                    },
                    "Compiler explanation cancelled",
                )
            } else if !observed.complete || observed.exit_code.is_none() {
                return Err(ErrorData::internal_error(
                    "Compiler explanation observation is incomplete",
                    None,
                ));
            } else if observed.exit_code == Some(0)
                && observed
                    .explanation
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty())
            {
                (
                    Outcome::Passed {
                        error_code: (),
                        error_message: (),
                        data: Box::new(Data {
                            semantics: Semantics::LatestKnown,
                            observation: observed,
                        }),
                    },
                    "Compiler explanation captured from the approved offline runtime",
                )
            } else if observed.explanation.is_none() && observed.exit_code == Some(1) {
                (
                    Outcome::Unavailable {
                        error_code: Code::DiagnosticExplanationUnavailable,
                        error_message: "This installed compiler has no explanation for the requested code",
                        data: Some(Box::new(Data {
                            semantics: Semantics::LatestKnown,
                            observation: observed,
                        })),
                    },
                    "This installed compiler has no explanation for the requested code",
                )
            } else {
                return Err(ErrorData::internal_error(
                    "Compiler explanation observation is inconsistent",
                    None,
                ));
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
            "Compiler explanation cancelled after worker completion",
        ),
        Err(InspectionError::Execution(ExecutionError::Unavailable)) => {
            operational(OperationalErrorCode::ToolNotInstalled)
        }
        Err(InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        )) => operational(OperationalErrorCode::SandboxDenied),
        Err(InspectionError::InvalidMetadata) => {
            return Err(ErrorData::internal_error(
                "Compiler explanation validation failed",
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
                "Compiler explanation failed",
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
pub(super) struct ExplainTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
}
impl ExplainTool {
    pub(super) fn new(
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Explain one validated Rust compiler error code using the host-approved offline compiler. Returns bounded installed-compiler text with runtime identity and latest_known evidence. Requires completed discovery and explicit host runtime policy; no project reference or project code is used.",(*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema))
            .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(false));
        Ok(Self {
            definition,
            contract,
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
            let inspector = Arc::clone(&self.inspector);
            match self
                .workers
                .run_joined(context.ct, started + DEADLINE, move |control| {
                    rust_engineering_application::explain_diagnostic(
                        inspector.as_ref(),
                        &input.code,
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
