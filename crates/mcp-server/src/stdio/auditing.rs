// These types are used only to derive schemas; domain values own wire serialization.
pub(super) mod provider;
#[allow(dead_code)]
pub(super) mod schemas;
use super::clock::WallClock;
use super::workers::worker_error;
use super::{
    contract::{Contract, ToolOutput},
    project::Registry,
    workers::{Joined, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{
    ExecutionError, InspectionError, ProjectAuditError, ProjectError,
};
use rust_engineering_domain::{
    AuditDataError, AuditIssue, AuditObservation, AuditState, Evidence, IntegrityStatus,
    OperationalErrorCode, ProjectAudit, ProjectRef, RuntimeIdentity, SourceKind, ToolStatus,
};
use rust_engineering_execution::RustProjectInspector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) const NAME: &str = "rust.dependencies.audit";
const DEADLINE: Duration = Duration::from_secs(120);
const MAX_RESULT: usize = 512 * 1024;
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
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    source_fingerprint: String,
    #[schemars(with = "super::inspection::schemas::RuntimeIdentity")]
    runtime: RuntimeIdentity,
    #[schemars(with = "schemas::AuditObservation")]
    observation: AuditObservation,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    AuditSnapshotUnavailable,
    AuditSnapshotStale,
    AuditSnapshotUnknownAge,
    AuditSnapshotInvalid,
    AuditIntegrityFailed,
    AuditLockfileMissing,
    AuditLockfileInvalid,
    AuditBudgetExceeded,
    AuditIncomplete,
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
    findings_omitted: u64,
    paths_omitted: u64,
    unsupported_packages_omitted: u64,
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
            "Captured project structure is invalid or unsupported",
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
            (Code::CommandTimeout, "Audit exceeded its deadline", false)
        }
        OperationalErrorCode::SandboxDenied => (
            Code::SandboxDenied,
            "Host runtime policy, failed calibration or current capacity denied audit",
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
            "Project metadata exceeds the response budget",
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
                data: None,
            }
        },
        message,
    )
}
fn output(
    result: Result<ProjectAudit, ProjectAuditError>,
    duration_ms: u64,
) -> Result<Output, ErrorData> {
    let mut evidence = Evidence::Local;
    let mut truncation = Truncation::default();
    let (outcome, summary) = match result {
        Ok(project) => {
            evidence = Evidence::Snapshot(project.evidence);
            let mut data = Box::new(Data {
                project_ref: project.project_ref.to_string(),
                project_identity_fingerprint: project.project_identity_fingerprint.to_string(),
                semantics: Semantics::LatestKnown,
                source_fingerprint: project.source_fingerprint.to_string(),
                runtime: project.runtime,
                observation: project.observation,
            });
            truncation.findings_omitted = data.observation.findings_omitted;
            truncation.paths_omitted = data
                .observation
                .findings
                .iter()
                .chain(&data.observation.informational)
                .fold(0_u64, |total, finding| {
                    total.saturating_add(finding.paths_omitted)
                });
            classify(&mut data);
            let observation = &data.observation;
            match observation.issue {
                _ if observation.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.provenance().source_kind() != SourceKind::RustsecSnapshot
                        || snapshot.provenance().network_used()
                }) =>
                {
                    (
                        Outcome::Blocked {
                            error_code: Code::AuditSnapshotInvalid,
                            error_message: "Audit snapshot provenance is inconsistent",
                            data: None,
                        },
                        "Audit blocked; snapshot provenance is inconsistent",
                    )
                }
                _ if observation.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.provenance().integrity() != IntegrityStatus::Verified
                }) =>
                {
                    (
                        Outcome::Blocked {
                            error_code: Code::AuditIntegrityFailed,
                            error_message: "RustSec snapshot integrity is not verified",
                            data: Some(data),
                        },
                        "Audit blocked; unverified snapshot evidence retained",
                    )
                }
                Some(
                    AuditIssue::SnapshotUnavailable
                    | AuditIssue::SnapshotStale
                    | AuditIssue::SnapshotUnknownAge,
                ) => {
                    let code = match observation.issue {
                        Some(AuditIssue::SnapshotStale) => Code::AuditSnapshotStale,
                        Some(AuditIssue::SnapshotUnknownAge) => Code::AuditSnapshotUnknownAge,
                        _ => Code::AuditSnapshotUnavailable,
                    };
                    (
                        Outcome::Unavailable {
                            error_code: code,
                            error_message: "Fresh verified RustSec snapshot is unavailable",
                            data: Some(data),
                        },
                        "Audit unavailable; captured project and advisory evidence retained",
                    )
                }
                _ if observation.state == AuditState::Passed && observation.validation_complete => {
                    (
                        Outcome::Passed {
                            error_code: (),
                            error_message: (),
                            data,
                        },
                        "Captured lockfile has no known vulnerabilities in the fresh verified RustSec snapshot",
                    )
                }
                _ if !observation.findings.is_empty() => (
                    Outcome::Failed {
                        error_code: (),
                        error_message: (),
                        data,
                    },
                    "Known vulnerabilities found in the captured lockfile",
                ),
                _ => (
                    Outcome::Blocked {
                        error_code: Code::AuditIncomplete,
                        error_message: "Audit coverage is incomplete",
                        data: Some(data),
                    },
                    "Audit incomplete; partial evidence retained",
                ),
            }
        }
        Err(ProjectAuditError::Data(error)) => data_error(error)?,
        Err(ProjectAuditError::Inspection(error)) => inspection_error(error, &mut truncation)?,
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
fn inspection_error(
    error: InspectionError,
    truncation: &mut Truncation,
) -> Result<(Outcome, &'static str), ErrorData> {
    Ok(match error {
        InspectionError::Project(ProjectError::Rejected(code)) => operational(code),
        InspectionError::Project(ProjectError::Cancelled)
        | InspectionError::Execution(ExecutionError::Cancelled) => (
            Outcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Audit cancelled after worker completion",
        ),
        InspectionError::Execution(ExecutionError::Unavailable) => {
            operational(OperationalErrorCode::ToolNotInstalled)
        }
        InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        ) => operational(OperationalErrorCode::SandboxDenied),
        InspectionError::InvalidMetadata => operational(OperationalErrorCode::InvalidProject),
        InspectionError::OutputLimit => {
            truncation.stdout_truncated = true;
            operational(OperationalErrorCode::OutputLimitExceeded)
        }
        InspectionError::Execution(ExecutionError::CleanupUncertain) => {
            return Err(ErrorData::internal_error(
                "Gateway cleanup could not be verified; further execution is quarantined",
                None,
            ));
        }
        InspectionError::Internal
        | InspectionError::Project(ProjectError::Internal)
        | InspectionError::Execution(ExecutionError::Infrastructure) => {
            return Err(ErrorData::internal_error("Dependency audit failed", None));
        }
    })
}
pub(super) struct AuditTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
    provider: Arc<provider::AuditProvider>,
}
impl AuditTool {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
        config: Option<provider::HostAuditConfig>,
    ) -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Audit the captured Cargo.lock with a host-configured, integrity-verified offline RustSec snapshot. Returns resolved package/version findings, patched requirements, bounded workspace paths, source coverage and latest_known project/RustSec evidence. Uses approved offline metadata; performs no advisory downloads or installs. Requires a live project_ref and completed discovery. Paths are one shortest representative per workspace root; the captured lock is not proof of active feature resolution. Integrity is relative to the host-expected checksum, not publisher authentication. Incomplete or stale evidence never passes.",(*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema))
            .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(false));
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
            provider: Arc::new(provider::AuditProvider(config)),
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
            Err(ProjectAuditError::Inspection(InspectionError::Execution(
                ExecutionError::Denied,
            )))
        } else {
            let registry = Arc::clone(&self.registry);
            let inspector = Arc::clone(&self.inspector);
            let provider = Arc::clone(&self.provider);
            match self
                .workers
                .run_joined(context.ct, started + DEADLINE, move |control| {
                    registry
                        .lock()
                        .map_err(|_| ProjectAuditError::Inspection(InspectionError::Internal))?
                        .audit(
                            &input.project_ref,
                            inspector.as_ref(),
                            provider.as_ref(),
                            &WallClock,
                            control,
                        )
                })
                .await
            {
                Ok(joined) => joined_result(joined),
                Err(error) => Err(ProjectAuditError::Inspection(worker_error(error))),
            }
        };
        let duration = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let mut value = output(result, duration)?;
        if bootstrap {
            let message = "Audit requires completed discovery; retry with a new request ID";
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
fn joined_result<T>(joined: Joined<T, ProjectAuditError>) -> Result<T, ProjectAuditError> {
    match (joined.result, joined.interrupted) {
        (
            Err(
                ProjectAuditError::Inspection(
                    InspectionError::Project(ProjectError::Cancelled)
                    | InspectionError::Execution(ExecutionError::Cancelled),
                )
                | ProjectAuditError::Data(AuditDataError::Cancelled | AuditDataError::Timeout),
            ),
            Some(signal),
        ) => Err(ProjectAuditError::Inspection(worker_error(signal))),
        (Err(error), _) => Err(error),
        (Ok(_), Some(signal)) => Err(ProjectAuditError::Inspection(worker_error(signal))),
        (Ok(value), None) => Ok(value),
    }
}

// The MCP boundary checks pass preconditions independently of the audit port.
fn classify(data: &mut Data) {
    data.observation.normalize();
}

fn data_error(error: AuditDataError) -> Result<(Outcome, &'static str), ErrorData> {
    let (code, message) = match error {
        AuditDataError::Unavailable => {
            return Ok((
                Outcome::Unavailable {
                    error_code: Code::AuditSnapshotUnavailable,
                    error_message: "RustSec snapshot unavailable",
                    data: None,
                },
                "RustSec snapshot unavailable",
            ));
        }
        AuditDataError::InvalidSnapshot => (
            Code::AuditSnapshotInvalid,
            "RustSec snapshot is invalid or inaccessible",
        ),
        AuditDataError::Integrity => (
            Code::AuditIntegrityFailed,
            "RustSec snapshot does not match the host-expected checksum",
        ),
        AuditDataError::MissingLockfile => (
            Code::AuditLockfileMissing,
            "Captured project has no Cargo.lock",
        ),
        AuditDataError::InvalidLockfile => (
            Code::AuditLockfileInvalid,
            "Captured Cargo.lock is invalid or inconsistent",
        ),
        AuditDataError::Budget => (Code::AuditBudgetExceeded, "Audit safety budget exceeded"),
        AuditDataError::Cancelled => {
            return Ok((
                Outcome::Cancelled {
                    error_code: (),
                    error_message: (),
                    data: (),
                },
                "Audit cancelled after worker completion",
            ));
        }
        AuditDataError::Timeout => return Ok(operational(OperationalErrorCode::CommandTimeout)),
        AuditDataError::SandboxDenied => {
            return Ok(operational(OperationalErrorCode::SandboxDenied));
        }
        AuditDataError::UnsupportedPlatform => {
            return Ok(operational(OperationalErrorCode::UnsupportedPlatform));
        }
        AuditDataError::Internal => {
            return Err(ErrorData::internal_error("Dependency audit failed", None));
        }
    };
    Ok((
        Outcome::Blocked {
            error_code: code,
            error_message: message,
            data: None,
        },
        message,
    ))
}

fn encode_bounded(
    contract: &Contract<Input, Output>,
    mut value: Output,
) -> Result<CallToolResult, ErrorData> {
    // The text fallback can escape JSON again. Bound the complete SDK envelope,
    // with a conservative pre-encoding margin; report every discarded fact.
    while serde_json::to_vec(&value)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > MAX_RESULT / 4
    {
        let data = match &mut value.outcome {
            Outcome::Passed { data, .. } | Outcome::Failed { data, .. } => Some(data),
            Outcome::Blocked { data, .. } | Outcome::Unavailable { data, .. } => data.as_mut(),
            _ => None,
        };
        let Some(data) = data else { break };
        let observation = &mut data.observation;
        if let Some(finding) = observation
            .findings
            .iter_mut()
            .chain(&mut observation.informational)
            .rev()
            .find(|finding| !finding.paths.is_empty())
        {
            finding.paths.pop();
            finding.paths_omitted = finding.paths_omitted.saturating_add(1);
            value.truncation.paths_omitted = value.truncation.paths_omitted.saturating_add(1);
        } else if observation.informational.pop().is_some() || observation.findings.pop().is_some()
        {
            observation.findings_omitted = observation.findings_omitted.saturating_add(1);
            value.truncation.findings_omitted = value.truncation.findings_omitted.saturating_add(1);
        } else if observation.unsupported_packages.pop().is_some() {
            value.truncation.unsupported_packages_omitted = value
                .truncation
                .unsupported_packages_omitted
                .saturating_add(1);
        } else {
            break;
        }
        observation.validation_complete = false;
        if observation.state == AuditState::Passed {
            observation.state = AuditState::Incomplete;
        }
        if observation.issue.is_none() {
            observation.issue = Some(AuditIssue::OutputBudget);
        }
        if let Outcome::Passed { data, .. } = value.outcome {
            value.outcome = Outcome::Blocked {
                error_code: Code::AuditBudgetExceeded,
                error_message: "Audit response omitted facts",
                data: Some(data),
            };
            value.summary = "Audit incomplete; response omissions are explicit";
        }
    }
    let duration = value.duration_ms;
    let encoded = contract.encode(value)?;
    if serde_json::to_vec(&encoded)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > MAX_RESULT
    {
        return contract.encode(output(
            Err(ProjectAuditError::Data(AuditDataError::Budget)),
            duration,
        )?);
    }
    Ok(encoded)
}
#[cfg(test)]
mod tests;
