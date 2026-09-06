//! Synchronous M3-04 `rust.semver.check` contract and runtime integration.

use super::{
    contract::{Contract, ToolOutput},
    nextest::{ExecutionModeDto, ExecutionSelection, select_execution_mode},
    project::Registry,
    quality_artifacts::DurableSemverPublisher,
    resources::{self, Store},
    workers::{Joined, WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::job::JobPermit;
use rust_engineering_application::semver_check::{
    SEMVER_DEFAULT_TIMEOUT_SECONDS, SemverArtifactReference, SemverOptions, SemverOutcome,
    SemverProjectResult,
};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::{
    ArtifactCompleteness, Clock, GuestArtifactName, OperationalErrorCode, ProjectRef, ToolStatus,
    UnixSeconds,
    semver_check::{
        SemverCommandOptions, SemverFindingLevel, SemverProjectSelection, SemverRequiredUpdate,
    },
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

pub(super) const NAME: &str = "rust.semver.check";
const MAX_RESPONSE_FINDINGS: usize = 16;

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    baseline_project_ref: ProjectRef,
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    candidate_project_ref: ProjectRef,
    #[serde(default)]
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
    )]
    package: Option<String>,
    #[serde(default)]
    #[schemars(with = "Vec<super::check::schemas::Feature>", length(max = 32))]
    features: Vec<String>,
    #[serde(default)]
    all_features: bool,
    #[serde(default)]
    no_default_features: bool,
    #[serde(default)]
    #[schemars(regex(pattern = "^aarch64-unknown-linux-gnu$"))]
    target: Option<String>,
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 3600))]
    timeout_seconds: u64,
    #[serde(default)]
    execution_mode: ExecutionModeDto,
}
fn default_timeout() -> u64 {
    SEMVER_DEFAULT_TIMEOUT_SECONDS
}
impl Input {
    fn options(&self) -> Result<SemverOptions, ErrorData> {
        let selection = SemverCommandOptions::try_from(SemverProjectSelection {
            package: self.package.clone(),
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            target: self.target.clone(),
        })
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))?;
        // The public DTO contains exactly one selection. Cloning it into both
        // application sides makes divergence unrepresentable at this boundary;
        // SemverOptions still rejects divergence for non-MCP callers.
        SemverOptions::new(selection.clone(), selection, self.timeout_seconds)
            .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Completeness {
    Complete,
    Partial,
    Incomplete,
    Truncated,
}
#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum FindingLevel {
    Deny,
    Warn,
}
#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RequiredUpdate {
    Major,
    Minor,
    Patch,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SideEvidence {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    project_identity_fingerprint: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    captured_source_sha256: String,
    captured_at_unix_seconds: Option<u64>,
    assessed_at_unix_seconds: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Finding {
    #[schemars(length(min = 1, max = 512))]
    item: String,
    #[schemars(length(min = 1, max = 512))]
    lint: String,
    level: FindingLevel,
    required_update: Option<RequiredUpdate>,
    #[schemars(length(max = 512))]
    span: Option<String>,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Counts {
    deny: u32,
    warn: u32,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Selection {
    #[schemars(length(min = 1, max = 128))]
    package: Option<String>,
    #[schemars(with = "Vec<super::check::schemas::Feature>", length(max = 32))]
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    #[schemars(regex(pattern = "^aarch64-unknown-linux-gnu$"))]
    target: Option<String>,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Artifact {
    #[schemars(length(min = 1, max = 512))]
    uri: String,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    sha256: String,
    size_bytes: u32,
    completeness: Completeness,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RuntimeEvidence {
    #[schemars(length(min = 1, max = 128))]
    platform: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    image_id: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    configuration_fingerprint: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    execution_fingerprint: String,
    #[schemars(length(min = 1, max = 128))]
    rust_version: String,
    #[schemars(length(min = 1, max = 128))]
    cargo_version: String,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    baseline: SideEvidence,
    candidate: SideEvidence,
    baseline_selection: Selection,
    candidate_selection: Selection,
    counts: Counts,
    #[schemars(length(max = 16))]
    findings: Vec<Finding>,
    completeness: Completeness,
    #[schemars(with = "super::check::schemas::ExecutionTermination")]
    termination: rust_engineering_domain::ExecutionTermination,
    exit_code: Option<i32>,
    runtime: RuntimeEvidence,
    raw_output: Option<Artifact>,
    findings_omitted: u64,
    raw_output_omitted: bool,
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    ProjectNotFound,
    InvalidProject,
    CommandTimeout,
    OutputLimitExceeded,
    ToolNotInstalled,
    SandboxDenied,
    TasksRequired,
    IncompleteEvidence,
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
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Output {
    #[serde(flatten)]
    outcome: Outcome,
    summary: &'static str,
    duration_ms: u64,
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

pub(super) struct SemverTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    runtime: Option<Runtime>,
}
struct Runtime {
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
    store: Arc<Mutex<Store>>,
    durable: Option<DurableSemverPublisher>,
}
impl SemverTool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition = Tool::new(
            NAME,
            "Compare a captured local baseline with a captured candidate using cargo-semver-checks in the approved offline gateway. Both sides use one identical closed package/feature/target selection; baseline is captured first and mounted read-only at /baseline, candidate at /source. Results retain bounded non-colored raw output as a private Resource; a qualified state root uses durable Stage 1 and absence or an unsupported/busy attach falls back to Stage 0. Because the pinned tool has no machine-readable findings mode, itemized findings are best-effort and partial; unrecognized output is incomplete, never a clean pass. Auto or synchronous is qualified only when timeout_seconds is at most 60; longer auto calls require negotiated MCP Tasks, and task mode is accepted only when the peer declares io.modelcontextprotocol/tasks.",
            (*contract.input_schema).clone(),
        ).with_raw_output_schema(Arc::clone(&contract.output_schema)).with_annotations(
            ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false)
        );
        Ok(Self {
            definition,
            contract,
            runtime: None,
        })
    }
    pub(super) fn with_runtime(
        mut self,
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
        resources: &resources::Resources,
        durable: Option<DurableSemverPublisher>,
    ) -> Self {
        self.runtime = Some(Runtime {
            registry,
            workers,
            inspector,
            ready,
            store: resources.store(),
            durable,
        });
        self
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let options = input.options()?;
        match select_execution_mode(
            input.execution_mode.into(),
            false,
            options.timeout_seconds() <= 60,
        )? {
            ExecutionSelection::Task => {
                return Err(ErrorData::internal_error(
                    "Tasks are not enabled for semver",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => return self.tasks_required(),
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("Semver runtime is not configured", None))?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.operational(
                Code::SandboxDenied,
                "Semver requires completed discovery",
                0,
            );
        }
        let started = Instant::now();
        let permit = match runtime.workers.admit_job() {
            Ok(permit) => permit,
            Err(error) => return self.encode_error(worker_error(error), 0),
        };
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let store = Arc::clone(&runtime.store);
        let baseline_ref = input.baseline_project_ref.clone();
        let candidate_ref = input.candidate_project_ref.clone();
        let durable = runtime.durable.clone();
        let joined = runtime
            .workers
            .run_joined_with(
                Arc::clone(&permit),
                context.ct,
                started + Duration::from_secs(options.timeout_seconds() + 240),
                move |control| {
                    let mut registry = registry.lock().map_err(|_| InspectionError::Internal)?;
                    let mut store = store.lock().map_err(|_| InspectionError::Internal)?;
                    if let Some(mut publisher) = durable {
                        registry.semver_check_durable(
                            &baseline_ref,
                            &candidate_ref,
                            &options,
                            inspector.as_ref(),
                            inspector.as_ref(),
                            &mut *store,
                            &mut publisher,
                            &WallClock,
                            control,
                        )
                    } else {
                        registry.semver_check(
                            &baseline_ref,
                            &candidate_ref,
                            &options,
                            inspector.as_ref(),
                            inspector.as_ref(),
                            &mut *store,
                            &WallClock,
                            control,
                        )
                    }
                },
            )
            .await;
        permit.release_after_cleanup();
        let result = match joined {
            Ok(value) => joined_result(value),
            Err(error) => Err(worker_error(error)),
        };
        match result {
            Ok(result) => self.encode_result(
                result,
                started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            ),
            Err(error) => self.encode_error(
                error,
                started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            ),
        }
    }
    fn tasks_required(&self) -> Result<CallToolResult, ErrorData> {
        self.contract.encode(Output {
            outcome: Outcome::Blocked {
                error_code: Code::TasksRequired,
                error_message: "This semver selection requires MCP Tasks",
                data: None,
            },
            summary: "Semver requires Tasks for a work budget above 60 seconds",
            duration_ms: 0,
        })
    }
    fn operational(
        &self,
        code: Code,
        message: &'static str,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        self.contract.encode(Output {
            outcome: Outcome::Blocked {
                error_code: code,
                error_message: message,
                data: None,
            },
            summary: message,
            duration_ms,
        })
    }
    fn encode_error(
        &self,
        error: InspectionError,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        match error {
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled) => {
                self.contract.encode(Output {
                    outcome: Outcome::Cancelled {
                        error_code: (),
                        error_message: (),
                        data: (),
                    },
                    summary: "Semver cancelled after joined cleanup",
                    duration_ms,
                })
            }
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            )) => self.operational(
                Code::ProjectNotFound,
                "Project reference is unavailable",
                duration_ms,
            ),
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::InvalidProject,
            ))
            | InspectionError::InvalidMetadata => self.operational(
                Code::InvalidProject,
                "Captured project is invalid",
                duration_ms,
            ),
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout,
            )) => self.operational(
                Code::CommandTimeout,
                "Semver exceeded its deadline",
                duration_ms,
            ),
            InspectionError::OutputLimit => self.operational(
                Code::OutputLimitExceeded,
                "Semver evidence exceeded its fixed output budget",
                duration_ms,
            ),
            InspectionError::Execution(ExecutionError::Unavailable) => {
                self.contract.encode(Output {
                    outcome: Outcome::Unavailable {
                        error_code: Code::ToolNotInstalled,
                        error_message: "Approved cargo-semver-checks runtime is unavailable",
                        data: (),
                    },
                    summary: "Semver runtime unavailable",
                    duration_ms,
                })
            }
            InspectionError::Execution(ExecutionError::CleanupUncertain) => {
                Err(ErrorData::internal_error(
                    "Gateway cleanup could not be verified; further execution is quarantined",
                    None,
                ))
            }
            InspectionError::Project(ProjectError::Rejected(_))
            | InspectionError::Execution(
                ExecutionError::Denied
                | ExecutionError::Busy
                | ExecutionError::InvalidConfiguration,
            ) => self.operational(
                Code::SandboxDenied,
                "Host runtime policy or capacity denied semver",
                duration_ms,
            ),
            InspectionError::Internal
            | InspectionError::Project(ProjectError::Internal)
            | InspectionError::Execution(ExecutionError::Infrastructure) => {
                Err(ErrorData::internal_error("Semver failed", None))
            }
        }
    }
    fn encode_result(
        &self,
        result: SemverProjectResult,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        if result.outcome == SemverOutcome::Unavailable {
            return self.contract.encode(Output {
                outcome: Outcome::Unavailable {
                    error_code: Code::InvalidProject,
                    error_message: "Baseline and candidate must select a library target",
                    data: (),
                },
                summary: "Semver unavailable because a selected library target is missing",
                duration_ms,
            });
        }
        let observation = result
            .observation
            .ok_or_else(|| ErrorData::internal_error("Semver result validation failed", None))?;
        // The tool result is mirrored in structuredContent and text. Sixteen
        // maximally escaped 512-byte fields per row still keep the complete MCP
        // result below 512 KiB; the authoritative totals and omission count
        // remain intact when the parser observed more rows.
        let itemized = observation.findings.len().min(MAX_RESPONSE_FINDINGS);
        let response_omitted = observation.findings.len().saturating_sub(itemized) as u64;
        let data = Box::new(Data {
            baseline: side(
                result.baseline_project_ref,
                result.baseline_project_identity_fingerprint.to_string(),
                &result.baseline_evidence,
            ),
            candidate: side(
                result.candidate_project_ref.clone(),
                result.candidate_project_identity_fingerprint.to_string(),
                &result.candidate_evidence,
            ),
            baseline_selection: selection(observation.options.baseline_selection()),
            candidate_selection: selection(observation.options.selection()),
            counts: Counts {
                deny: observation.counts.deny,
                warn: observation.counts.warn,
            },
            findings: observation
                .findings
                .into_iter()
                .take(itemized)
                .map(|finding| Finding {
                    item: finding.item().into(),
                    lint: finding.lint().into(),
                    level: match finding.level() {
                        SemverFindingLevel::Deny => FindingLevel::Deny,
                        SemverFindingLevel::Warn => FindingLevel::Warn,
                    },
                    required_update: finding.required_update().map(|update| match update {
                        SemverRequiredUpdate::Major => RequiredUpdate::Major,
                        SemverRequiredUpdate::Minor => RequiredUpdate::Minor,
                        SemverRequiredUpdate::Patch => RequiredUpdate::Patch,
                    }),
                    span: finding.span().map(str::to_owned),
                })
                .collect(),
            completeness: match observation.completeness {
                rust_engineering_domain::semver_check::SemverFindingCompleteness::Partial => {
                    Completeness::Partial
                }
                rust_engineering_domain::semver_check::SemverFindingCompleteness::Incomplete => {
                    Completeness::Incomplete
                }
            },
            termination: observation.termination,
            exit_code: observation.exit_code,
            runtime: RuntimeEvidence {
                platform: observation.runtime.platform,
                image_id: observation.runtime.image_id,
                configuration_fingerprint: observation
                    .runtime
                    .configuration_fingerprint
                    .to_string(),
                execution_fingerprint: observation.execution_fingerprint.to_string(),
                rust_version: observation.runtime.rust_version,
                cargo_version: observation.runtime.cargo_version,
            },
            raw_output: result
                .raw_output
                .map(|metadata| artifact(&result.candidate_project_ref, metadata))
                .transpose()?,
            findings_omitted: observation
                .findings_omitted
                .saturating_add(response_omitted),
            raw_output_omitted: result.raw_output_omitted,
        });
        let output = match result.outcome {
            SemverOutcome::NoBreak => Output {
                outcome: Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data,
                },
                summary: "No deny-level semantic-version break was observed",
                duration_ms,
            },
            SemverOutcome::Breaking => Output {
                outcome: Outcome::Failed {
                    error_code: (),
                    error_message: (),
                    data,
                },
                summary: "Semantic-version breaking changes were observed",
                duration_ms,
            },
            SemverOutcome::Incomplete => Output {
                outcome: Outcome::Blocked {
                    error_code: Code::IncompleteEvidence,
                    error_message: "Semver evidence is incomplete",
                    data: Some(data),
                },
                summary: "Semver could not establish a complete coarse outcome",
                duration_ms,
            },
            SemverOutcome::Blocked => Output {
                outcome: Outcome::Blocked {
                    error_code: Code::IncompleteEvidence,
                    error_message: "Semver output contradicted the calibrated coarse outcome",
                    data: Some(data),
                },
                summary: "Semver evidence was contradictory",
                duration_ms,
            },
            SemverOutcome::Unavailable => {
                return Err(ErrorData::internal_error(
                    "Semver result validation failed",
                    None,
                ));
            }
        };
        self.contract.encode(output)
    }
}

fn selection(options: &SemverCommandOptions) -> Selection {
    Selection {
        package: options.package().map(str::to_owned),
        features: options.features().to_vec(),
        all_features: options.all_features(),
        no_default_features: options.no_default_features(),
        target: options.target().map(str::to_owned),
    }
}

fn side(
    project_ref: ProjectRef,
    identity: String,
    evidence: &rust_engineering_domain::SnapshotEvidence,
) -> SideEvidence {
    SideEvidence {
        project_ref: project_ref.to_string(),
        project_identity_fingerprint: identity,
        captured_source_sha256: evidence.provenance().source_id().to_string(),
        captured_at_unix_seconds: evidence.provenance().created_at().map(|value| value.0),
        assessed_at_unix_seconds: evidence.freshness().assessed_at().0,
    }
}
fn artifact(owner: &ProjectRef, reference: SemverArtifactReference) -> Result<Artifact, ErrorData> {
    match reference {
        SemverArtifactReference::Ephemeral(metadata) => {
            if &metadata.owner != owner {
                return Err(ErrorData::internal_error(
                    "Semver artifact authorization failed",
                    None,
                ));
            }
            Ok(Artifact {
                uri: resources::uri(owner, &metadata.id),
                sha256: hex(&metadata.sha256),
                size_bytes: metadata.size_bytes,
                completeness: if metadata.truncated {
                    Completeness::Truncated
                } else {
                    Completeness::Complete
                },
            })
        }
        SemverArtifactReference::Durable(descriptor) => {
            descriptor.validate().map_err(|_| {
                ErrorData::internal_error("Semver artifact validation failed", None)
            })?;
            if descriptor.source.guest_name != GuestArtifactName::ToolLog {
                return Err(ErrorData::internal_error(
                    "Semver artifact kind is invalid",
                    None,
                ));
            }
            Ok(Artifact {
                uri: format!(
                    "rust-quality-artifact://{owner}/{}?offset=0&length={}",
                    descriptor.artifact_id,
                    descriptor.size_bytes.min(256 * 1024)
                ),
                sha256: hex(&descriptor.sha256),
                size_bytes: descriptor.size_bytes.try_into().map_err(|_| {
                    ErrorData::internal_error("Semver artifact size is invalid", None)
                })?,
                completeness: match descriptor.completeness {
                    ArtifactCompleteness::Complete => Completeness::Complete,
                    ArtifactCompleteness::Partial => Completeness::Partial,
                    ArtifactCompleteness::Truncated => Completeness::Truncated,
                    ArtifactCompleteness::Invalid => Completeness::Incomplete,
                    ArtifactCompleteness::Unavailable => Completeness::Incomplete,
                },
            })
        }
    }
}
fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
                .map(|value| value.as_secs())
                .unwrap_or(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_is_closed_and_selection_is_single() -> Result<(), Box<dyn std::error::Error>> {
        let tool = SemverTool::new()?;
        let definition = serde_json::to_value(tool.definition)?;
        assert_eq!(definition["name"], NAME);
        assert_eq!(definition["inputSchema"]["additionalProperties"], false);
        assert!(
            definition["inputSchema"]["properties"]
                .get("baseline_features")
                .is_none()
        );
        assert!(
            definition["inputSchema"]["properties"]
                .get("candidate_features")
                .is_none()
        );
        assert_eq!(
            definition["outputSchema"]["$defs"]["Data"]["properties"]["findings"]["maxItems"],
            MAX_RESPONSE_FINDINGS
        );
        Ok(())
    }

    #[test]
    fn maximally_escaped_itemized_findings_keep_the_mirrored_result_below_512_kib()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = SemverTool::new()?;
        let side = |suffix: char| SideEvidence {
            project_ref: format!("prj_{}", suffix.to_string().repeat(32)),
            project_identity_fingerprint: format!("sha256:{}", suffix.to_string().repeat(64)),
            captured_source_sha256: format!("sha256:{}", suffix.to_string().repeat(64)),
            captured_at_unix_seconds: Some(1),
            assessed_at_unix_seconds: 1,
        };
        let selection = || Selection {
            package: Some("p".repeat(128)),
            features: vec!["f".repeat(128); 32],
            all_features: false,
            no_default_features: false,
            target: Some("aarch64-unknown-linux-gnu".into()),
        };
        let escaped = "\0".repeat(512);
        let result = tool.contract.encode(Output {
            outcome: Outcome::Failed {
                error_code: (),
                error_message: (),
                data: Box::new(Data {
                    baseline: side('1'),
                    candidate: side('2'),
                    baseline_selection: selection(),
                    candidate_selection: selection(),
                    counts: Counts {
                        deny: MAX_RESPONSE_FINDINGS as u32,
                        warn: 0,
                    },
                    findings: (0..MAX_RESPONSE_FINDINGS)
                        .map(|_| Finding {
                            item: escaped.clone(),
                            lint: escaped.clone(),
                            level: FindingLevel::Deny,
                            required_update: Some(RequiredUpdate::Major),
                            span: Some(escaped.clone()),
                        })
                        .collect(),
                    completeness: Completeness::Partial,
                    termination: rust_engineering_domain::ExecutionTermination::Exited,
                    exit_code: Some(100),
                    runtime: RuntimeEvidence {
                        platform: "linux/aarch64".into(),
                        image_id: format!("sha256:{}", "3".repeat(64)),
                        configuration_fingerprint: format!("sha256:{}", "4".repeat(64)),
                        execution_fingerprint: format!("sha256:{}", "5".repeat(64)),
                        rust_version: "1.98.1".into(),
                        cargo_version: "1.98.1".into(),
                    },
                    raw_output: None,
                    findings_omitted: 1,
                    raw_output_omitted: false,
                }),
            },
            summary: "Semantic-version breaking changes were observed",
            duration_ms: 1,
        })?;
        assert!(serde_json::to_vec(&result)?.len() <= 512 * 1024);
        Ok(())
    }
}
