//! M3-01 synchronous `rust.test.nextest` contract and runtime integration.

use super::{
    contract::{Contract, ToolOutput},
    project::Registry,
    quality_artifacts::DurableNextestPublisher,
    resources::{self, Store},
    workers::{Joined, WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::job::JobPermit;
use rust_engineering_application::nextest::{
    NEXTEST_DEFAULT_TIMEOUT_SECONDS, NEXTEST_MAX_TEST_ROWS, NextestArtifactKind,
    NextestArtifactReference, NextestCompleteness, NextestOptions, NextestSelection,
    NextestTaskResult, NextestTestStatus,
};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::{
    ArtifactCompleteness, Clock, GuestArtifactName, OperationalErrorCode, ProjectRef, ToolStatus,
    UnixSeconds, job::ExecutionMode,
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

pub(super) const NAME: &str = "rust.test.nextest";
const MAX_RESULT: usize = 512 * 1024;
const TASKS_REQUIRED: &str = "Tasks capability is required";

#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    pub project_ref: ProjectRef,
    #[serde(default)]
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
    )]
    pub package: Option<String>,
    #[serde(default)]
    #[schemars(with = "Vec<super::check::schemas::Feature>", length(max = 32))]
    pub features: Vec<String>,
    #[serde(default)]
    pub all_features: bool,
    #[serde(default)]
    pub no_default_features: bool,
    #[serde(default)]
    #[schemars(regex(pattern = "^aarch64-unknown-linux-gnu$"))]
    pub target: Option<String>,
    #[serde(default)]
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_:]*$")
    )]
    pub test_filter: Option<String>,
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 3600))]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub execution_mode: ExecutionModeDto,
    #[serde(default)]
    #[schemars(range(max = 2))]
    pub retries: u8,
}

fn default_timeout() -> u64 {
    NEXTEST_DEFAULT_TIMEOUT_SECONDS
}

impl Input {
    pub(super) fn options(&self) -> Result<NextestOptions, ErrorData> {
        NextestOptions::try_from(NextestSelection {
            package: self.package.clone(),
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            target: self.target.clone(),
            test_filter: self.test_filter.clone(),
            timeout_seconds: self.timeout_seconds,
            retries: self.retries,
        })
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }

    pub(super) fn mode(&self) -> ExecutionMode {
        self.execution_mode.into()
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ExecutionModeDto {
    #[default]
    Auto,
    Task,
    Synchronous,
}

impl From<ExecutionModeDto> for ExecutionMode {
    fn from(value: ExecutionModeDto) -> Self {
        match value {
            ExecutionModeDto::Auto => Self::Auto,
            ExecutionModeDto::Task => Self::Task,
            ExecutionModeDto::Synchronous => Self::Synchronous,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExecutionSelection {
    Task,
    Synchronous,
    TasksRequired,
}

/// Pure negotiation decision run before worker admission or result reservation.
pub(super) fn select_execution_mode(
    requested: ExecutionMode,
    client_tasks: bool,
    synchronous_qualified: bool,
) -> Result<ExecutionSelection, ErrorData> {
    // The outer product handler records the negotiated capability in a
    // task-local scope. A registry-owned background job re-enters the same
    // validated tool path under its already-held permit and is therefore the
    // one internal case that must execute synchronously regardless of the
    // client-facing selection.
    if super::workers::executing_admitted_job() {
        return Ok(ExecutionSelection::Synchronous);
    }
    let client_tasks = super::workers::negotiated_tasks(client_tasks);
    match requested {
        ExecutionMode::Task if !client_tasks => {
            Err(ErrorData::invalid_params(TASKS_REQUIRED, None))
        }
        ExecutionMode::Task => Ok(ExecutionSelection::Task),
        ExecutionMode::Synchronous if synchronous_qualified => Ok(ExecutionSelection::Synchronous),
        ExecutionMode::Synchronous => Err(ErrorData::invalid_params(
            "Operation exceeds the synchronous budget",
            None,
        )),
        ExecutionMode::Auto if client_tasks => Ok(ExecutionSelection::Task),
        ExecutionMode::Auto if synchronous_qualified => Ok(ExecutionSelection::Synchronous),
        ExecutionMode::Auto => Ok(ExecutionSelection::TasksRequired),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Completeness {
    Complete,
    Truncated,
    Partial,
    Invalid,
    Unavailable,
}

impl From<NextestCompleteness> for Completeness {
    fn from(value: NextestCompleteness) -> Self {
        match value {
            NextestCompleteness::Complete => Self::Complete,
            NextestCompleteness::Partial => Self::Partial,
            NextestCompleteness::Invalid => Self::Invalid,
            NextestCompleteness::Unavailable => Self::Unavailable,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TestStatus {
    Passed,
    Failed,
    Ignored,
    Flaky,
    Leaked,
    TimedOut,
}

impl From<NextestTestStatus> for TestStatus {
    fn from(value: NextestTestStatus) -> Self {
        match value {
            NextestTestStatus::Passed => Self::Passed,
            NextestTestStatus::Failed => Self::Failed,
            NextestTestStatus::Ignored => Self::Ignored,
            NextestTestStatus::Flaky => Self::Flaky,
            NextestTestStatus::Leaked => Self::Leaked,
            NextestTestStatus::TimedOut => Self::TimedOut,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Counts {
    selected: u64,
    passed: u64,
    failed: u64,
    ignored: u64,
    retried: u64,
    flaky: u64,
    leaked: u64,
    timed_out: u64,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TestRow {
    #[schemars(length(min = 1, max = 256))]
    test_id: String,
    status: TestStatus,
    #[schemars(range(min = 1))]
    attempts: u16,
    duration_ms: u64,
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum ArtifactKind {
    JunitXml,
    StdoutLog,
    StderrLog,
    ToolLog,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PublishedArtifact {
    pub kind: ArtifactKind,
    #[schemars(length(min = 1, max = 512))]
    pub uri: String,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    pub sha256: String,
    pub size_bytes: u64,
    pub completeness: Completeness,
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
    #[schemars(length(min = 1, max = 32))]
    declared_toolchain: Option<String>,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Omissions {
    tests_omitted: u64,
    junit_truncated: bool,
    stdout_truncated: bool,
    stderr_truncated: bool,
    artifacts_unavailable: bool,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    #[schemars(length(min = 1, max = 128))]
    profile: &'static str,
    validation_complete: bool,
    completeness: Completeness,
    counts: Counts,
    #[schemars(length(max = 128))]
    tests: Vec<TestRow>,
    doctests_run: bool,
    #[schemars(with = "super::check::schemas::ExecutionTermination")]
    termination: rust_engineering_domain::ExecutionTermination,
    exit_code: Option<i32>,
    runtime: RuntimeEvidence,
    #[schemars(length(max = 128))]
    artifacts: Vec<PublishedArtifact>,
    omissions: Omissions,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(dead_code)]
enum Code {
    ProjectNotFound,
    InvalidProject,
    CommandTimeout,
    OutputLimitExceeded,
    ToolNotInstalled,
    SandboxDenied,
    NetworkDenied,
    UnsupportedPlatform,
    TasksRequired,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
#[allow(dead_code)]
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

#[allow(dead_code)]
pub(super) struct NextestTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    runtime: Option<NextestRuntime>,
}

struct NextestRuntime {
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
    store: Arc<Mutex<Store>>,
    durable: Option<DurableNextestPublisher>,
}

impl NextestTool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition = Tool::new(
            NAME,
            "Run cargo-nextest over captured Rust source in the approved offline gateway. Executes hostile project code, uses the fixed default rust-mcp profile, accepts only closed selections, and returns bounded machine-readable counts plus private artifact Resources. Auto or synchronous is qualified only when timeout_seconds is at most 60 and the fixed default profile is used; longer auto calls require negotiated MCP Tasks, and task mode is accepted only when the peer declares io.modelcontextprotocol/tasks. The synchronous work budget excludes joined cleanup, defaults to the requested timeout and never exceeds 60 seconds here. Installs nothing and never runs doctests.",
            (*contract.input_schema).clone(),
        )
        .with_raw_output_schema(Arc::clone(&contract.output_schema))
        .with_annotations(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(false)
                .open_world(false),
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
        durable: Option<DurableNextestPublisher>,
    ) -> Self {
        self.runtime = Some(NextestRuntime {
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
        let qualified_short = options.timeout_seconds() <= 60;
        match select_execution_mode(input.mode(), false, qualified_short)? {
            ExecutionSelection::Task => {
                return Err(ErrorData::internal_error(
                    "Tasks are not enabled for nextest",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => return self.tasks_required(),
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("Nextest runtime is not configured", None))?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.encode_operational(
                OperationalErrorCode::SandboxDenied,
                "Nextest requires completed discovery; retry with a new request ID",
                0,
            );
        }
        let started = Instant::now();
        let permit = match runtime.workers.admit_job() {
            Ok(permit) => permit,
            Err(error) => return self.encode_inspection_error(worker_error(error), 0),
        };
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let store = Arc::clone(&runtime.store);
        let project_ref = input.project_ref.clone();
        let durable = runtime.durable.clone();
        // ADR-060 excludes cleanup from the <=60 s synchronous work budget.
        // The outer join therefore permits the gateway's separately bounded
        // cleanup while the execution gateway enforces the requested work cap.
        let joined = runtime
            .workers
            .run_joined_with(
                Arc::clone(&permit),
                context.ct,
                started + Duration::from_secs(options.timeout_seconds() + 240),
                move |control| {
                    let mut registry = registry.lock().map_err(|_| InspectionError::Internal)?;
                    let mut stage0 = store.lock().map_err(|_| InspectionError::Internal)?;
                    let (observation, artifacts) = if let Some(mut durable) = durable {
                        registry.nextest_durable(
                            &project_ref,
                            &options,
                            inspector.as_ref(),
                            &mut *stage0,
                            &mut durable,
                            &WallClock,
                            control,
                        )?
                    } else {
                        registry.nextest(
                            &project_ref,
                            &options,
                            inspector.as_ref(),
                            &mut *stage0,
                            &WallClock,
                            control,
                        )?
                    };
                    NextestTaskResult::new(
                        observation,
                        artifacts,
                        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                    )
                    .map_err(|_| InspectionError::Internal)
                },
            )
            .await;
        permit.release_after_cleanup();
        let result = match joined {
            Ok(joined) => joined_result(joined),
            Err(error) => Err(worker_error(error)),
        };
        match result {
            Ok(result) => self.encode_task_result(&input.project_ref, result, false),
            Err(error) => self.encode_inspection_error(
                error,
                started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            ),
        }
    }

    fn tasks_required(&self) -> Result<CallToolResult, ErrorData> {
        self.contract.encode(Output {
            outcome: Outcome::Blocked {
                error_code: Code::TasksRequired,
                error_message: "This nextest selection requires MCP Tasks",
                data: None,
            },
            summary: "Nextest requires Tasks for a work budget above 60 seconds",
            duration_ms: 0,
        })
    }

    fn encode_operational(
        &self,
        code: OperationalErrorCode,
        override_message: &'static str,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let (error_code, unavailable) = match code {
            OperationalErrorCode::ProjectNotFound => (Code::ProjectNotFound, false),
            OperationalErrorCode::InvalidProject => (Code::InvalidProject, false),
            OperationalErrorCode::ToolNotInstalled => (Code::ToolNotInstalled, true),
            OperationalErrorCode::LockfileUpdateRequired => (Code::InvalidProject, false),
            OperationalErrorCode::CommandTimeout => (Code::CommandTimeout, false),
            OperationalErrorCode::SandboxDenied => (Code::SandboxDenied, false),
            OperationalErrorCode::NetworkDenied => (Code::NetworkDenied, false),
            OperationalErrorCode::UnsupportedPlatform => (Code::UnsupportedPlatform, true),
            OperationalErrorCode::OutputLimitExceeded => (Code::OutputLimitExceeded, false),
        };
        self.contract.encode(Output {
            outcome: if unavailable {
                Outcome::Unavailable {
                    error_code,
                    error_message: override_message,
                    data: (),
                }
            } else {
                Outcome::Blocked {
                    error_code,
                    error_message: override_message,
                    data: None,
                }
            },
            summary: override_message,
            duration_ms,
        })
    }

    fn encode_inspection_error(
        &self,
        error: InspectionError,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        match error {
            InspectionError::Project(ProjectError::Rejected(code)) => {
                self.encode_operational(code, operational_message(code), duration_ms)
            }
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled) => {
                self.contract.encode(Output {
                    outcome: Outcome::Cancelled {
                        error_code: (),
                        error_message: (),
                        data: (),
                    },
                    summary: "Nextest cancelled after joined cleanup",
                    duration_ms,
                })
            }
            InspectionError::Execution(ExecutionError::Unavailable) => self.encode_operational(
                OperationalErrorCode::ToolNotInstalled,
                operational_message(OperationalErrorCode::ToolNotInstalled),
                duration_ms,
            ),
            InspectionError::Execution(
                ExecutionError::Denied
                | ExecutionError::Busy
                | ExecutionError::InvalidConfiguration,
            ) => self.encode_operational(
                OperationalErrorCode::SandboxDenied,
                operational_message(OperationalErrorCode::SandboxDenied),
                duration_ms,
            ),
            InspectionError::OutputLimit => self.encode_operational(
                OperationalErrorCode::OutputLimitExceeded,
                operational_message(OperationalErrorCode::OutputLimitExceeded),
                duration_ms,
            ),
            InspectionError::InvalidMetadata => self.encode_operational(
                OperationalErrorCode::InvalidProject,
                operational_message(OperationalErrorCode::InvalidProject),
                duration_ms,
            ),
            InspectionError::Execution(ExecutionError::CleanupUncertain) => {
                Err(ErrorData::internal_error(
                    "Gateway cleanup could not be verified; further execution is quarantined",
                    None,
                ))
            }
            InspectionError::Internal
            | InspectionError::Project(ProjectError::Internal)
            | InspectionError::Execution(ExecutionError::Infrastructure) => {
                Err(ErrorData::internal_error("Nextest failed", None))
            }
        }
    }

    pub(super) fn encode_task_result(
        &self,
        project_ref: &ProjectRef,
        result: NextestTaskResult,
        is_error: bool,
    ) -> Result<CallToolResult, ErrorData> {
        let (observation, artifacts, expected_artifacts, duration_ms) = result.into_parts();
        observation
            .validate()
            .map_err(|_| ErrorData::internal_error("Nextest result validation failed", None))?;
        let structurally_complete = observation.validation_complete
            && observation.completeness == NextestCompleteness::Complete
            // cargo-nextest emits a complete zero-test JUnit report and exit
            // code 4 for `no-tests = "fail"`. That is a conclusive failed
            // validation, not incomplete evidence.
            && (observation.counts.selected > 0 || observation.exit_code == Some(4))
            && observation.termination == rust_engineering_domain::ExecutionTermination::Exited;
        let unavailable = observation.completeness == NextestCompleteness::Unavailable;
        let timed_out =
            observation.termination == rust_engineering_domain::ExecutionTermination::TimedOut;
        let published_artifacts = published_artifacts(project_ref, artifacts)?;
        let artifacts_unavailable = published_artifacts.len() < usize::from(expected_artifacts)
            || published_artifacts
                .iter()
                .any(|artifact| artifact.completeness == Completeness::Unavailable);
        let complete = structurally_complete && !artifacts_unavailable;
        let passed = complete
            && observation.exit_code == Some(0)
            && observation
                .counts
                .passed
                .checked_add(observation.counts.flaky)
                .is_some_and(|passed| passed > 0)
            && observation.counts.failed == 0
            && observation.counts.ignored == 0
            && observation.counts.leaked == 0
            && observation.counts.timed_out == 0;
        let data = Box::new(Data {
            project_ref: project_ref.to_string(),
            profile: observation.options.profile(),
            validation_complete: observation.validation_complete,
            completeness: observation.completeness.into(),
            counts: Counts {
                selected: observation.counts.selected,
                passed: observation.counts.passed,
                failed: observation.counts.failed,
                ignored: observation.counts.ignored,
                retried: observation.counts.retried,
                flaky: observation.counts.flaky,
                leaked: observation.counts.leaked,
                timed_out: observation.counts.timed_out,
            },
            tests: observation
                .tests
                .into_iter()
                .take(NEXTEST_MAX_TEST_ROWS)
                .map(|row| TestRow {
                    test_id: row.test_id,
                    status: row.status.into(),
                    attempts: row.attempts,
                    duration_ms: row.duration_ms,
                })
                .collect(),
            doctests_run: observation.doctests_run,
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
                declared_toolchain: observation.runtime.declared_toolchain,
            },
            artifacts: published_artifacts,
            omissions: Omissions {
                tests_omitted: observation.tests_omitted,
                junit_truncated: observation.artifacts.junit_truncated,
                stdout_truncated: observation.artifacts.stdout_truncated,
                stderr_truncated: observation.artifacts.stderr_truncated,
                artifacts_unavailable,
            },
        });
        let output = if timed_out {
            Output {
                outcome: Outcome::Blocked {
                    error_code: Code::CommandTimeout,
                    error_message: "Nextest exceeded its deadline",
                    data: Some(data),
                },
                summary: "Nextest timed out after joined cleanup",
                duration_ms,
            }
        } else if unavailable {
            Output {
                outcome: Outcome::Unavailable {
                    error_code: Code::ToolNotInstalled,
                    error_message: "Approved cargo-nextest runtime is unavailable",
                    data: (),
                },
                summary: "Nextest is unavailable",
                duration_ms,
            }
        } else if is_error || !complete {
            Output {
                outcome: Outcome::Blocked {
                    // Distinct from a stream/response-cap violation: bounded
                    // evidence existed, but was not complete enough to decide.
                    error_code: Code::InvalidProject,
                    error_message: "Nextest evidence is incomplete",
                    data: Some(data),
                },
                summary: "Nextest evidence is incomplete",
                duration_ms,
            }
        } else if passed {
            Output {
                outcome: Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data,
                },
                summary: "Selected nextest tests passed",
                duration_ms,
            }
        } else {
            Output {
                outcome: Outcome::Failed {
                    error_code: (),
                    error_message: (),
                    data,
                },
                summary: "Selected nextest tests failed",
                duration_ms,
            }
        };
        let result = self.contract.encode(output)?;
        if serde_json::to_vec(&result)
            .map_err(|_| ErrorData::internal_error("Nextest response encoding failed", None))?
            .len()
            > MAX_RESULT
        {
            return Err(ErrorData::internal_error(
                "Nextest response exceeds its fixed budget",
                None,
            ));
        }
        Ok(result)
    }
}

fn operational_message(code: OperationalErrorCode) -> &'static str {
    match code {
        OperationalErrorCode::ProjectNotFound => "Project reference is missing or expired",
        OperationalErrorCode::InvalidProject => "Captured project is invalid or unsupported",
        OperationalErrorCode::ToolNotInstalled => "Approved cargo-nextest runtime is unavailable",
        OperationalErrorCode::LockfileUpdateRequired => "Lockfile update is required",
        OperationalErrorCode::CommandTimeout => "Nextest exceeded its deadline",
        OperationalErrorCode::SandboxDenied => {
            "Host runtime policy, failed calibration or capacity denied nextest"
        }
        OperationalErrorCode::NetworkDenied => "Network access is denied",
        OperationalErrorCode::UnsupportedPlatform => {
            "Secure nextest execution is unavailable on this platform"
        }
        OperationalErrorCode::OutputLimitExceeded => {
            "Nextest evidence exceeded its fixed output budget"
        }
    }
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

fn published_artifacts(
    project_ref: &ProjectRef,
    artifacts: Vec<NextestArtifactReference>,
) -> Result<Vec<PublishedArtifact>, ErrorData> {
    if artifacts.len() > 128 {
        return Err(ErrorData::internal_error(
            "Nextest artifact limit exceeded",
            None,
        ));
    }
    artifacts
        .into_iter()
        .map(|artifact| match artifact {
            NextestArtifactReference::Ephemeral { kind, metadata } => {
                if &metadata.owner != project_ref {
                    return Err(ErrorData::internal_error(
                        "Nextest artifact authorization failed",
                        None,
                    ));
                }
                Ok(PublishedArtifact {
                    kind: match kind {
                        NextestArtifactKind::JunitXml => ArtifactKind::JunitXml,
                        NextestArtifactKind::StdoutLog => ArtifactKind::StdoutLog,
                        NextestArtifactKind::StderrLog => ArtifactKind::StderrLog,
                    },
                    uri: format!("rust-artifact://{project_ref}/{}", metadata.id),
                    sha256: hex(&metadata.sha256),
                    size_bytes: u64::from(metadata.size_bytes),
                    completeness: if metadata.truncated {
                        Completeness::Truncated
                    } else {
                        Completeness::Complete
                    },
                })
            }
            NextestArtifactReference::EphemeralUnavailable { kind, metadata } => {
                if &metadata.owner != project_ref {
                    return Err(ErrorData::internal_error(
                        "Nextest artifact authorization failed",
                        None,
                    ));
                }
                Ok(PublishedArtifact {
                    kind: match kind {
                        NextestArtifactKind::JunitXml => ArtifactKind::JunitXml,
                        NextestArtifactKind::StdoutLog => ArtifactKind::StdoutLog,
                        NextestArtifactKind::StderrLog => ArtifactKind::StderrLog,
                    },
                    uri: format!("rust-artifact://{project_ref}/{}", metadata.id),
                    sha256: hex(&metadata.sha256),
                    size_bytes: u64::from(metadata.size_bytes),
                    completeness: Completeness::Unavailable,
                })
            }
            NextestArtifactReference::Durable(descriptor) => {
                descriptor.validate().map_err(|_| {
                    ErrorData::internal_error("Nextest artifact validation failed", None)
                })?;
                let length = descriptor.size_bytes.min(320 * 1024);
                Ok(PublishedArtifact {
                    kind: match descriptor.source.guest_name {
                        GuestArtifactName::JunitXml => ArtifactKind::JunitXml,
                        _ => ArtifactKind::ToolLog,
                    },
                    uri: format!(
                        "rust-quality-artifact://{project_ref}/{}?offset=0&length={length}",
                        descriptor.artifact_id
                    ),
                    sha256: hex(&descriptor.sha256),
                    size_bytes: descriptor.size_bytes,
                    completeness: match descriptor.completeness {
                        ArtifactCompleteness::Complete => Completeness::Complete,
                        ArtifactCompleteness::Truncated | ArtifactCompleteness::Partial => {
                            Completeness::Partial
                        }
                        ArtifactCompleteness::Invalid => Completeness::Invalid,
                        ArtifactCompleteness::Unavailable => Completeness::Unavailable,
                    },
                })
            }
        })
        .collect()
}

fn hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(64);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_application::nextest::{
        ArtifactStreams, NextestCounts, NextestObservation,
    };
    use rust_engineering_domain::{
        ArtifactMetadata, ExecutionFingerprint, ExecutionTermination, RuntimeIdentity,
    };
    use sha2::{Digest, Sha256};

    fn observation(counts: NextestCounts) -> Result<NextestTaskResult, Box<dyn std::error::Error>> {
        let execution_fingerprint =
            format!("sha256:{}", "3".repeat(64)).parse::<ExecutionFingerprint>()?;
        Ok(NextestTaskResult::new(
            NextestObservation {
                options: NextestOptions::try_from(NextestSelection::default())?,
                validation_complete: true,
                completeness: NextestCompleteness::Complete,
                counts,
                tests: Vec::new(),
                tests_omitted: 0,
                doctests_run: false,
                termination: ExecutionTermination::Exited,
                exit_code: Some(0),
                runtime: RuntimeIdentity {
                    platform: "linux-aarch64".to_owned(),
                    image_id: format!("sha256:{}", "1".repeat(64)),
                    configuration_fingerprint: format!("sha256:{}", "2".repeat(64)).parse()?,
                    execution_fingerprint: execution_fingerprint.clone(),
                    rust_version: "rustc 1.98.1".to_owned(),
                    cargo_version: "cargo 1.98.1".to_owned(),
                    declared_toolchain: None,
                },
                execution_fingerprint,
                artifacts: ArtifactStreams::default(),
            },
            Vec::new(),
            1,
        )?)
    }

    #[test]
    fn schema_is_closed_and_stable() -> Result<(), Box<dyn std::error::Error>> {
        let tool = NextestTool::new()?;
        let definition = serde_json::to_value(&tool.definition)?;
        assert_eq!(definition["name"], NAME);
        assert_eq!(definition["inputSchema"]["additionalProperties"], false);
        assert_eq!(definition["outputSchema"]["unevaluatedProperties"], false);
        assert_eq!(
            definition["inputSchema"]["$defs"]["ExecutionModeDto"]["enum"],
            serde_json::json!(["auto", "task", "synchronous"])
        );
        assert_eq!(
            definition["inputSchema"]["properties"]["timeout_seconds"]["default"],
            NEXTEST_DEFAULT_TIMEOUT_SECONDS
        );
        let bytes = serde_json::to_vec(&definition)?;
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        let hash = hex(&digest);
        assert_eq!(
            hash,
            "766e2a95420548830c7689824f48a1251b32308d89f08feed5da15b81863be5f"
        );
        Ok(())
    }

    #[test]
    fn semantic_validation_rejects_contradictory_or_open_arguments()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = NextestTool::new()?;
        for value in [
            serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","all_features":true,"features":["std"]}),
            serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","test_filter":"--ignored"}),
            serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","timeout_seconds":3601}),
            serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","execution_mode":"background"}),
            serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","args":["--workspace"]}),
        ] {
            let arguments = serde_json::from_value(value)?;
            if let Ok(input) = tool.contract.decode(Some(arguments)) {
                assert!(input.options().is_err());
            }
        }
        let arguments = serde_json::from_value(serde_json::json!({
            "project_ref":"prj_00000000000000000000000000000001",
            "no_default_features":true,
            "execution_mode":"task"
        }))?;
        let input = tool.contract.decode(Some(arguments))?;
        assert_eq!(
            input.project_ref.to_string(),
            "prj_00000000000000000000000000000001"
        );
        assert!(input.options()?.no_default_features());
        assert_eq!(input.mode(), ExecutionMode::Task);
        Ok(())
    }

    #[test]
    fn execution_mode_selection_is_closed_and_precedes_admission() {
        assert_eq!(
            select_execution_mode(ExecutionMode::Task, true, false),
            Ok(ExecutionSelection::Task)
        );
        let error = select_execution_mode(ExecutionMode::Task, false, true)
            .err()
            .unwrap_or_else(|| ErrorData::internal_error("expected rejection", None));
        assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert_eq!(error.message, TASKS_REQUIRED);
        assert!(error.data.is_none());
        assert_eq!(
            select_execution_mode(ExecutionMode::Auto, false, true),
            Ok(ExecutionSelection::Synchronous)
        );
        assert_eq!(
            select_execution_mode(ExecutionMode::Auto, false, false),
            Ok(ExecutionSelection::TasksRequired)
        );
        assert_eq!(
            select_execution_mode(ExecutionMode::Auto, true, true),
            Ok(ExecutionSelection::Task)
        );
        assert!(select_execution_mode(ExecutionMode::Synchronous, false, false).is_err());
    }

    #[test]
    fn skipped_or_unpublished_evidence_never_passes_with_positive_control()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = NextestTool::new()?;
        let project: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
        let skipped = tool.encode_task_result(
            &project,
            observation(NextestCounts {
                selected: 1,
                ignored: 1,
                ..Default::default()
            })?,
            false,
        )?;
        let skipped = serde_json::to_value(skipped)?;
        assert_eq!(skipped["isError"], false);
        assert_eq!(skipped["structuredContent"]["status"], "failed");

        let unavailable_artifact = observation(NextestCounts {
            selected: 1,
            passed: 1,
            ..Default::default()
        })?;
        let (mut unpublished, _, _, _) = unavailable_artifact.into_parts();
        unpublished.artifacts.junit_xml = b"bounded junit".to_vec();
        let unavailable_artifact = NextestTaskResult::new(unpublished, Vec::new(), 1)?;
        let unavailable_artifact = serde_json::to_value(tool.encode_task_result(
            &project,
            unavailable_artifact,
            false,
        )?)?;
        assert_eq!(unavailable_artifact["isError"], true);
        assert_eq!(
            unavailable_artifact["structuredContent"]["data"]["omissions"]["artifacts_unavailable"],
            true
        );

        let passed = serde_json::to_value(tool.encode_task_result(
            &project,
            observation(NextestCounts {
                selected: 1,
                passed: 1,
                ..Default::default()
            })?,
            false,
        )?)?;
        assert_eq!(passed["isError"], false);
        assert_eq!(passed["structuredContent"]["status"], "passed");
        Ok(())
    }

    fn artifact(owner: &str) -> Result<ArtifactMetadata, Box<dyn std::error::Error>> {
        Ok(ArtifactMetadata {
            owner: owner.parse()?,
            id: "art_00000000000000000000000000000009".parse()?,
            sha256: [0x3c; 32],
            size_bytes: 64,
            truncated: false,
            created_seconds: 0,
            expires_seconds: 3600,
        })
    }

    #[test]
    fn every_operational_code_has_one_declared_outcome_and_message()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = NextestTool::new()?;
        for (code, status, error_code) in [
            (
                OperationalErrorCode::ProjectNotFound,
                "blocked",
                "PROJECT_NOT_FOUND",
            ),
            (
                OperationalErrorCode::InvalidProject,
                "blocked",
                "INVALID_PROJECT",
            ),
            (
                OperationalErrorCode::LockfileUpdateRequired,
                "blocked",
                "INVALID_PROJECT",
            ),
            (
                OperationalErrorCode::CommandTimeout,
                "blocked",
                "COMMAND_TIMEOUT",
            ),
            (
                OperationalErrorCode::SandboxDenied,
                "blocked",
                "SANDBOX_DENIED",
            ),
            (
                OperationalErrorCode::NetworkDenied,
                "blocked",
                "NETWORK_DENIED",
            ),
            (
                OperationalErrorCode::OutputLimitExceeded,
                "blocked",
                "OUTPUT_LIMIT_EXCEEDED",
            ),
            (
                OperationalErrorCode::ToolNotInstalled,
                "unavailable",
                "TOOL_NOT_INSTALLED",
            ),
            (
                OperationalErrorCode::UnsupportedPlatform,
                "unavailable",
                "UNSUPPORTED_PLATFORM",
            ),
        ] {
            let message = operational_message(code);
            let encoded = serde_json::to_value(tool.encode_operational(code, message, 5)?)?;
            let value = &encoded["structuredContent"];
            assert_eq!(value["status"], status, "{error_code}");
            assert_eq!(value["error_code"], error_code);
            assert_eq!(value["error_message"], message);
            assert_eq!(value["summary"], message);
            assert_eq!(value["duration_ms"], 5);
            assert_eq!(encoded["isError"], true);
            // An unavailable runtime carries no partial assessment at all.
            assert_eq!(value["data"], serde_json::Value::Null);
        }
        Ok(())
    }

    #[test]
    fn every_inspection_error_maps_to_one_declared_outcome()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = NextestTool::new()?;
        for (error, status, code) in [
            (
                InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::ProjectNotFound,
                )),
                "blocked",
                serde_json::json!("PROJECT_NOT_FOUND"),
            ),
            (
                InspectionError::Project(ProjectError::Cancelled),
                "cancelled",
                serde_json::Value::Null,
            ),
            (
                InspectionError::Execution(ExecutionError::Cancelled),
                "cancelled",
                serde_json::Value::Null,
            ),
            (
                InspectionError::Execution(ExecutionError::Unavailable),
                "unavailable",
                serde_json::json!("TOOL_NOT_INSTALLED"),
            ),
            (
                InspectionError::Execution(ExecutionError::Denied),
                "blocked",
                serde_json::json!("SANDBOX_DENIED"),
            ),
            (
                InspectionError::Execution(ExecutionError::Busy),
                "blocked",
                serde_json::json!("SANDBOX_DENIED"),
            ),
            (
                InspectionError::Execution(ExecutionError::InvalidConfiguration),
                "blocked",
                serde_json::json!("SANDBOX_DENIED"),
            ),
            (
                InspectionError::OutputLimit,
                "blocked",
                serde_json::json!("OUTPUT_LIMIT_EXCEEDED"),
            ),
            (
                InspectionError::InvalidMetadata,
                "blocked",
                serde_json::json!("INVALID_PROJECT"),
            ),
        ] {
            let encoded = serde_json::to_value(tool.encode_inspection_error(error, 6)?)?;
            assert_eq!(encoded["structuredContent"]["status"], status);
            assert_eq!(encoded["structuredContent"]["error_code"], code);
        }
        // Unverified cleanup quarantines the runtime and infrastructure faults
        // are never reported as an assessment a peer could act on.
        for error in [
            InspectionError::Execution(ExecutionError::CleanupUncertain),
            InspectionError::Execution(ExecutionError::Infrastructure),
            InspectionError::Project(ProjectError::Internal),
            InspectionError::Internal,
        ] {
            let failure = tool
                .encode_inspection_error(error, 6)
                .err()
                .ok_or("tool result")?;
            assert_eq!(failure.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        }
        Ok(())
    }

    #[test]
    fn worker_signals_and_joined_cleanup_map_to_inspection_errors() {
        assert!(matches!(
            worker_error(WorkerError::Busy),
            InspectionError::Execution(ExecutionError::Busy)
        ));
        assert!(matches!(
            worker_error(WorkerError::Cancelled),
            InspectionError::Project(ProjectError::Cancelled)
        ));
        assert!(matches!(
            worker_error(WorkerError::TimedOut),
            InspectionError::Project(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
        ));
        assert!(matches!(
            worker_error(WorkerError::Internal),
            InspectionError::Internal
        ));
        assert!(matches!(
            joined_result(Joined {
                result: Ok(3_u8),
                interrupted: None
            }),
            Ok(3)
        ));
        assert!(matches!(
            joined_result(Joined {
                result: Ok(3_u8),
                interrupted: Some(WorkerError::Cancelled)
            }),
            Err(InspectionError::Project(ProjectError::Cancelled))
        ));
        // A body that observed its own cancellation still reports the signal
        // that caused it rather than a bare cancellation.
        assert!(matches!(
            joined_result::<u8>(Joined {
                result: Err(InspectionError::Execution(ExecutionError::Cancelled)),
                interrupted: Some(WorkerError::TimedOut)
            }),
            Err(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout
            )))
        ));
        assert!(matches!(
            joined_result::<u8>(Joined {
                result: Err(InspectionError::OutputLimit),
                interrupted: None
            }),
            Err(InspectionError::OutputLimit)
        ));
    }

    #[test]
    fn published_artifacts_are_owner_bound_and_kind_declared()
    -> Result<(), Box<dyn std::error::Error>> {
        let project: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
        let published = published_artifacts(
            &project,
            vec![
                NextestArtifactReference::Ephemeral {
                    kind: NextestArtifactKind::JunitXml,
                    metadata: artifact("prj_00000000000000000000000000000001")?,
                },
                NextestArtifactReference::EphemeralUnavailable {
                    kind: NextestArtifactKind::StderrLog,
                    metadata: artifact("prj_00000000000000000000000000000001")?,
                },
            ],
        )?;
        let value = serde_json::to_value(&published)?;
        assert_eq!(value[0]["kind"], "junit_xml");
        assert_eq!(
            value[0]["uri"],
            format!("rust-artifact://{project}/art_00000000000000000000000000000009")
        );
        assert_eq!(value[0]["sha256"], "3c".repeat(32));
        assert_eq!(value[0]["size_bytes"], 64);
        assert_eq!(value[0]["completeness"], "complete");
        assert_eq!(value[1]["kind"], "stderr_log");
        assert_eq!(value[1]["completeness"], "unavailable");
        // An artifact captured for another project is never republished here.
        for reference in [
            NextestArtifactReference::Ephemeral {
                kind: NextestArtifactKind::StdoutLog,
                metadata: artifact("prj_00000000000000000000000000000002")?,
            },
            NextestArtifactReference::EphemeralUnavailable {
                kind: NextestArtifactKind::StdoutLog,
                metadata: artifact("prj_00000000000000000000000000000002")?,
            },
        ] {
            assert!(published_artifacts(&project, vec![reference]).is_err());
        }
        let overflow: Vec<_> = (0..129)
            .map(|_| NextestArtifactReference::Ephemeral {
                kind: NextestArtifactKind::JunitXml,
                metadata: ArtifactMetadata {
                    owner: project.clone(),
                    id: "art_00000000000000000000000000000009"
                        .parse()
                        .unwrap_or_else(|_| unreachable!()),
                    sha256: [0; 32],
                    size_bytes: 0,
                    truncated: false,
                    created_seconds: 0,
                    expires_seconds: 1,
                },
            })
            .collect();
        assert!(published_artifacts(&project, overflow).is_err());
        assert_eq!(hex(&[0xde; 32]), "de".repeat(32));
        Ok(())
    }

    #[test]
    fn the_wall_clock_advances_with_the_host() {
        assert!(WallClock.now().0 > 1_700_000_000);
    }
}
