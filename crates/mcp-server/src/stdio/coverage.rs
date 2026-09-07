//! ADR-062 `rust.coverage` schema and bounded result projection.
//!
//! The instrumented runner keeps the report bundle opaque: HTML is retained as
//! a validated archive Resource, never rendered by this transport adapter.

use super::clock::WallClock;
use super::resources::hex;
use super::{
    contract::{Contract, ToolOutput},
    nextest::{ExecutionModeDto, ExecutionSelection, select_execution_mode},
    project::Registry,
    quality_artifacts::DurableCoveragePublisher,
    resources::{self, Store},
    workers::{Joined, WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::coverage::{
    CoverageArtifactKind, CoverageArtifactReference, CoverageTaskResult,
};
use rust_engineering_application::job::JobPermit;
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::coverage::CoverageMetric;
use rust_engineering_domain::{
    ArtifactCompleteness, ExecutionTermination, GuestArtifactName, OperationalErrorCode,
    ProjectRef, ToolStatus,
    coverage::{CoverageOptions, CoverageSelection},
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

pub(super) const NAME: &str = "rust.coverage";

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
    pub workspace: bool,
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
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 3600))]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub execution_mode: ExecutionModeDto,
}
fn default_timeout() -> u64 {
    300
}
impl Input {
    fn options(&self) -> Result<CoverageOptions, ErrorData> {
        CoverageOptions::try_from(CoverageSelection {
            package: self.package.clone(),
            workspace: self.workspace,
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            target: self.target.clone(),
            timeout_seconds: self.timeout_seconds,
        })
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome {
    Passed {
        error_code: (),
        error_message: (),
        data: Data,
    },
    Blocked {
        error_code: Code,
        error_message: &'static str,
        data: Option<Data>,
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
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Metric {
    count: u64,
    covered: u64,
    percent_millionths: u32,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Metrics {
    lines: Option<Metric>,
    regions: Option<Metric>,
    functions: Option<Metric>,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Package {
    name: String,
    metrics: Metrics,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    Json,
    Lcov,
    ArchiveBundle,
    ToolLog,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Artifact {
    kind: ArtifactKind,
    uri: String,
    sha256: String,
    size_bytes: u64,
    completeness: Completeness,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Completeness {
    Complete,
    Partial,
    Invalid,
    Unavailable,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Termination {
    Exited,
    TimedOut,
    Cancelled,
    OutputLimit,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    project_ref: String,
    aggregate: Metrics,
    packages: Vec<Package>,
    doctests_run: bool,
    cfg_coverage_enabled: bool,
    target: String,
    termination: Termination,
    exit_code: Option<i32>,
    files_page_rows: u16,
    files_omitted: bool,
    artifacts: Vec<Artifact>,
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
            Outcome::Blocked { .. } => ToolStatus::Blocked,
            Outcome::Unavailable { .. } => ToolStatus::Unavailable,
            Outcome::Cancelled { .. } => ToolStatus::Cancelled,
        }
    }
}

pub(super) struct CoverageTool {
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
    durable: Option<DurableCoveragePublisher>,
}
impl CoverageTool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Run cargo-llvm-cov once with a closed selection, then derive full JSON, LCOV and opaque HTML ArchiveBundle reports from the same profdata. No doctests, branch/MC/DC flags, custom ignore patterns, downloads or host execution are accepted. Auto and synchronous execution are qualified only for timeouts at most 60 seconds; longer work requires MCP Tasks.",(*contract.input_schema).clone()).with_raw_output_schema(Arc::clone(&contract.output_schema)).with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false));
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
        durable: Option<DurableCoveragePublisher>,
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
                    "Tasks are not enabled for coverage",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => return self.tasks_required(),
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("Coverage runtime is not configured", None))?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Coverage requires completed discovery; retry with a new request ID",
                None,
                0,
            );
        }
        let started = Instant::now();
        let permit = runtime
            .workers
            .admit_job()
            .map_err(|_| ErrorData::internal_error("Coverage worker unavailable", None))?;
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let store = Arc::clone(&runtime.store);
        let project_ref = input.project_ref.clone();
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
                    let (observation, artifacts) = if let Some(mut publisher) = durable {
                        registry.coverage_durable(
                            &project_ref,
                            &options,
                            inspector.as_ref(),
                            &mut *store,
                            &mut publisher,
                            &WallClock,
                            control,
                        )?
                    } else {
                        registry.coverage(
                            &project_ref,
                            &options,
                            inspector.as_ref(),
                            &mut *store,
                            &WallClock,
                            control,
                        )?
                    };
                    CoverageTaskResult::new(
                        observation,
                        artifacts,
                        started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
                    )
                    .map_err(|_| InspectionError::Internal)
                },
            )
            .await;
        permit.release_after_cleanup();
        let duration = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let result = match joined {
            Ok(value) => joined_result(value),
            Err(WorkerError::Cancelled) => Err(InspectionError::Project(ProjectError::Cancelled)),
            Err(WorkerError::TimedOut) => Err(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout,
            ))),
            Err(_) => Err(InspectionError::Internal),
        };
        match result {
            Ok(result) => self.encode_result(&input.project_ref, result),
            Err(error) => self.encode_error(error, duration),
        }
    }
    fn tasks_required(&self) -> Result<CallToolResult, ErrorData> {
        self.contract.encode(Output {
            outcome: Outcome::Blocked {
                error_code: Code::TasksRequired,
                error_message: "This coverage selection requires MCP Tasks",
                data: None,
            },
            summary: "Coverage requires Tasks for a work budget above 60 seconds",
            duration_ms: 0,
        })
    }
    fn blocked(
        &self,
        code: Code,
        message: &'static str,
        data: Option<Data>,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        self.contract.encode(Output {
            outcome: Outcome::Blocked {
                error_code: code,
                error_message: message,
                data,
            },
            summary: message,
            duration_ms,
        })
    }
    fn encode_error(
        &self,
        error: InspectionError,
        duration: u64,
    ) -> Result<CallToolResult, ErrorData> {
        match error {
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ToolNotInstalled,
            ))
            | InspectionError::Execution(ExecutionError::Unavailable) => {
                self.contract.encode(Output {
                    outcome: Outcome::Unavailable {
                        error_code: Code::ToolNotInstalled,
                        error_message: "Approved cargo-llvm-cov runtime is unavailable",
                        data: (),
                    },
                    summary: "Coverage is unavailable",
                    duration_ms: duration,
                })
            }
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled) => {
                self.contract.encode(Output {
                    outcome: Outcome::Cancelled {
                        error_code: (),
                        error_message: (),
                        data: (),
                    },
                    summary: "Coverage cancelled after joined cleanup",
                    duration_ms: duration,
                })
            }
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout,
            )) => self.blocked(
                Code::CommandTimeout,
                "Coverage exceeded its deadline",
                None,
                duration,
            ),
            InspectionError::OutputLimit => self.blocked(
                Code::OutputLimitExceeded,
                "Coverage evidence exceeded its fixed output budget",
                None,
                duration,
            ),
            InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            )) => self.blocked(
                Code::ProjectNotFound,
                "Project reference is missing or expired",
                None,
                duration,
            ),
            _ => self.blocked(
                Code::InvalidProject,
                "Coverage evidence is incomplete",
                None,
                duration,
            ),
        }
    }
    pub(super) fn encode_result(
        &self,
        project_ref: &ProjectRef,
        result: CoverageTaskResult,
    ) -> Result<CallToolResult, ErrorData> {
        let (observation, published, duration_ms) = result.into_parts();
        let (files, files_omitted) = observation.bounded_files();
        let data = Data {
            project_ref: project_ref.to_string(),
            aggregate: metrics(observation.aggregate_metrics()),
            packages: observation
                .package_metrics()
                .iter()
                .map(|p| Package {
                    name: p.name.clone(),
                    metrics: metrics(&p.metrics),
                })
                .collect(),
            doctests_run: observation.doctests_run,
            cfg_coverage_enabled: observation.cfg_coverage_enabled,
            target: observation.target.to_owned(),
            termination: termination(observation.termination),
            exit_code: observation.exit_code,
            files_page_rows: u16::try_from(files.len()).unwrap_or(u16::MAX),
            files_omitted,
            artifacts: artifacts(project_ref, published)?,
        };
        if observation.parse_complete
            && observation.termination == ExecutionTermination::Exited
            && observation.exit_code == Some(0)
        {
            self.contract.encode(Output {
                outcome: Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data,
                },
                summary: "Coverage completed from one instrumented run",
                duration_ms,
            })
        } else {
            self.blocked(
                Code::InvalidProject,
                "Coverage evidence is incomplete",
                Some(data),
                duration_ms,
            )
        }
    }
}

fn termination(value: ExecutionTermination) -> Termination {
    match value {
        ExecutionTermination::Exited => Termination::Exited,
        ExecutionTermination::TimedOut => Termination::TimedOut,
        ExecutionTermination::Cancelled => Termination::Cancelled,
        ExecutionTermination::OutputLimit => Termination::OutputLimit,
    }
}
fn metrics(value: &rust_engineering_domain::coverage::CoverageMetrics) -> Metrics {
    fn one(value: Option<CoverageMetric>) -> Option<Metric> {
        value.map(|v| Metric {
            count: v.count,
            covered: v.covered,
            percent_millionths: v.percent_millionths,
        })
    }
    Metrics {
        lines: one(value.lines),
        regions: one(value.regions),
        functions: one(value.functions),
    }
}
fn artifacts(
    project: &ProjectRef,
    values: Vec<CoverageArtifactReference>,
) -> Result<Vec<Artifact>, ErrorData> {
    values
        .into_iter()
        .map(|value| match value {
            CoverageArtifactReference::Ephemeral { kind, metadata } => Ok(Artifact {
                kind: kind_dto(kind),
                uri: format!("rust-artifact://{project}/{}", metadata.id),
                sha256: hex(&metadata.sha256),
                size_bytes: u64::from(metadata.size_bytes),
                completeness: if metadata.truncated {
                    Completeness::Partial
                } else {
                    Completeness::Complete
                },
            }),
            CoverageArtifactReference::Durable(descriptor) => {
                descriptor.validate().map_err(|_| {
                    ErrorData::internal_error("Coverage artifact validation failed", None)
                })?;
                Ok(Artifact {
                    kind: match descriptor.source.guest_name {
                        GuestArtifactName::CoverageJson => ArtifactKind::Json,
                        GuestArtifactName::Lcov => ArtifactKind::Lcov,
                        GuestArtifactName::ReportArchive => ArtifactKind::ArchiveBundle,
                        _ => ArtifactKind::ToolLog,
                    },
                    uri: format!(
                        "rust-quality-artifact://{project}/{}?offset=0&length={}",
                        descriptor.artifact_id,
                        descriptor.size_bytes.min(320 * 1024)
                    ),
                    sha256: hex(&descriptor.sha256),
                    size_bytes: descriptor.size_bytes,
                    completeness: match descriptor.completeness {
                        ArtifactCompleteness::Complete => Completeness::Complete,
                        ArtifactCompleteness::Invalid => Completeness::Invalid,
                        ArtifactCompleteness::Unavailable => Completeness::Unavailable,
                        _ => Completeness::Partial,
                    },
                })
            }
        })
        .collect()
}
fn kind_dto(value: CoverageArtifactKind) -> ArtifactKind {
    match value {
        CoverageArtifactKind::Json => ArtifactKind::Json,
        CoverageArtifactKind::Lcov => ArtifactKind::Lcov,
        CoverageArtifactKind::ArchiveBundle => ArtifactKind::ArchiveBundle,
        CoverageArtifactKind::StdoutLog | CoverageArtifactKind::StderrLog => ArtifactKind::ToolLog,
    }
}
fn joined_result<T>(joined: Joined<T, InspectionError>) -> Result<T, InspectionError> {
    match (joined.result, joined.interrupted) {
        (Err(error), _) => Err(error),
        (Ok(_), Some(WorkerError::Cancelled)) => {
            Err(InspectionError::Project(ProjectError::Cancelled))
        }
        (Ok(_), Some(WorkerError::TimedOut)) => Err(InspectionError::Project(
            ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
        )),
        (Ok(_), Some(_)) => Err(InspectionError::Internal),
        (Ok(value), None) => Ok(value),
    }
}

#[cfg(test)]
mod tests;
