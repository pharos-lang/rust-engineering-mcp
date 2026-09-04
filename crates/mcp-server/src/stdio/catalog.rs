//! Availability is an observation: each usable component carries verified evidence.
pub(super) mod provider;
#[allow(dead_code)]
mod schemas;
use super::{
    contract::{Contract, ToolOutput},
    workers::{Joined, WorkerError, Workers},
};
use provider::CatalogProvider;
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::ProjectError;
use rust_engineering_domain::{
    CatalogContextStatus, Clock, OperationalErrorCode, ToolStatus, UnixSeconds,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub(super) const NAME: &str = "rust.catalog.status";
const DEADLINE: Duration = Duration::from_secs(120);
const MAX_RESULT: usize = 128 * 1024;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Semantics {
    LatestKnown,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Lifecycle {
    SessionGenerationRestartToReload,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Enforcement {
    RuntimeApiDisabled,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Network {
    acquisition_allowed: bool,
    enforcement: Enforcement,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    semantics: Semantics,
    lifecycle: Lifecycle,
    network: Network,
    #[schemars(with = "schemas::CatalogContextStatus")]
    context: CatalogContextStatus,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    SandboxDenied,
    CommandTimeout,
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LocalEvidence {
    Local,
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
    evidence: LocalEvidence,
}
impl ToolOutput for Output {
    fn status(&self) -> ToolStatus {
        match self.outcome {
            Outcome::Passed { .. } => ToolStatus::Passed,
            Outcome::Blocked { .. } => ToolStatus::Blocked,
            Outcome::Cancelled { .. } => ToolStatus::Cancelled,
        }
    }
}
fn output(
    result: Result<CatalogContextStatus, ProjectError>,
    duration_ms: u64,
) -> Result<Output, ErrorData> {
    let (outcome, summary) = match result {
        Ok(context) => (
            Outcome::Passed {
                error_code: (),
                error_message: (),
                data: Box::new(Data {
                    semantics: Semantics::LatestKnown,
                    lifecycle: Lifecycle::SessionGenerationRestartToReload,
                    network: Network {
                        acquisition_allowed: false,
                        enforcement: Enforcement::RuntimeApiDisabled,
                    },
                    context,
                }),
            },
            "Verified local component availability observed",
        ),
        Err(ProjectError::Cancelled) => (
            Outcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Catalog status cancelled after worker completion",
        ),
        Err(ProjectError::Rejected(code)) => {
            let (code, message) = match code {
                OperationalErrorCode::CommandTimeout => {
                    (Code::CommandTimeout, "Catalog status exceeded its deadline")
                }
                OperationalErrorCode::OutputLimitExceeded => (
                    Code::OutputLimitExceeded,
                    "Catalog status exceeds the response budget",
                ),
                OperationalErrorCode::SandboxDenied => (
                    Code::SandboxDenied,
                    "Host policy, bootstrap or current capacity denied catalog status",
                ),
                _ => {
                    return Err(ErrorData::internal_error(
                        "Unexpected catalog status failure",
                        None,
                    ));
                }
            };
            (
                Outcome::Blocked {
                    error_code: code,
                    error_message: message,
                    data: (),
                },
                message,
            )
        }
        Err(ProjectError::Internal) => {
            return Err(ErrorData::internal_error(
                "Catalog status validation failed",
                None,
            ));
        }
    };
    Ok(Output {
        outcome,
        summary,
        duration_ms,
        diagnostics: [],
        truncation: Truncation::default(),
        evidence: LocalEvidence::Local,
    })
}
fn worker_error(error: WorkerError) -> ProjectError {
    match error {
        WorkerError::Busy => ProjectError::Rejected(OperationalErrorCode::SandboxDenied),
        WorkerError::Cancelled => ProjectError::Cancelled,
        WorkerError::TimedOut => ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
        WorkerError::Internal => ProjectError::Internal,
    }
}
fn joined_result<T>(joined: Joined<T, ProjectError>) -> Result<T, ProjectError> {
    match (joined.result, joined.interrupted) {
        (Err(ProjectError::Cancelled), Some(signal)) | (Ok(_), Some(signal)) => {
            Err(worker_error(signal))
        }
        (Err(error), _) => Err(error),
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
pub(super) struct CatalogTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    workers: Workers,
    provider: Arc<CatalogProvider>,
    ready: Arc<AtomicBool>,
}
impl CatalogTool {
    pub(super) fn new(
        workers: Workers,
        ready: Arc<AtomicBool>,
        provider: Arc<CatalogProvider>,
    ) -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Inspect verified local catalog, model, semantic index and configured RustSec availability, identities and latest_known freshness. Reads the host-configured session generation only; no project paths, downloads or synchronization. Restart to load changed catalog/model/index; RustSec follows the audit source on each call.",(*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema))
            .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(false));
        Ok(Self {
            definition,
            contract,
            workers,
            provider,
            ready,
        })
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.contract.decode(request.arguments)?;
        let started = Instant::now();
        let bootstrap = !self.ready.load(Ordering::Acquire);
        let result = if bootstrap {
            Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied))
        } else {
            let provider = Arc::clone(&self.provider);
            match self
                .workers
                .run_joined(context.ct, started + DEADLINE, move |control| {
                    rust_engineering_application::catalog_context(
                        provider.as_ref(),
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
            let message =
                "Catalog status requires completed discovery; retry with a new request ID";
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
        return contract.encode(output(
            Err(ProjectError::Rejected(
                OperationalErrorCode::OutputLimitExceeded,
            )),
            duration,
        )?);
    }
    Ok(result)
}
#[cfg(test)]
mod tests;
