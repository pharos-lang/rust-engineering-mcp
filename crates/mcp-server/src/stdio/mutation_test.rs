//! M3-05 `rust.mutation.test` contract and runtime integration.
//!
//! The tool runs `cargo mutants` over a private writable copy of the captured
//! source. Its verdict comes only from `mutants.out`; the mandatory baseline is
//! reported as a first-class outcome; and a surviving (missed) mutant is a
//! failure, never a warning. Timeout, unviable and incomplete evidence never
//! credit a clean result.

use super::{
    contract::{Contract, ToolOutput},
    nextest::{ExecutionSelection, select_execution_mode},
    project::Registry,
    resources::{self, Store},
    workers::{Joined, WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::job::JobPermit;
use rust_engineering_application::mutation_test::{
    MutationArtifactKind, MutationArtifactReference, MutationCompleteness, MutationTestObservation,
    MutationTestTaskResult, synchronous_qualified, total_budget_seconds,
};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::mutation_test::{
    MUTATION_DEFAULT_MAX_MUTANTS, MUTATION_DEFAULT_MUTANT_TIMEOUT_SECONDS, MutationBaseline,
    MutationGuestIdentity, MutationOutcomeClass, MutationTestCommandOptions, MutationTestSelection,
};
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

pub(super) const NAME: &str = "rust.mutation.test";
const MAX_RESULT: usize = 512 * 1024;

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
    #[serde(default = "default_max_mutants")]
    #[schemars(range(min = 1, max = 100))]
    pub max_mutants: u32,
    #[serde(default = "default_mutant_timeout")]
    #[schemars(range(min = 1, max = 60))]
    pub mutant_timeout_seconds: u64,
    #[serde(default)]
    pub execution_mode: ExecutionModeDto,
}

fn default_max_mutants() -> u32 {
    MUTATION_DEFAULT_MAX_MUTANTS
}

fn default_mutant_timeout() -> u64 {
    MUTATION_DEFAULT_MUTANT_TIMEOUT_SECONDS
}

impl Input {
    pub(super) fn options(&self) -> Result<MutationTestCommandOptions, ErrorData> {
        MutationTestCommandOptions::try_from(MutationTestSelection {
            package: self.package.clone(),
            features: self.features.clone(),
            all_features: self.all_features,
            no_default_features: self.no_default_features,
            target: self.target.clone(),
            max_mutants: self.max_mutants,
            mutant_timeout_seconds: self.mutant_timeout_seconds,
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

#[derive(Clone, Copy, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Completeness {
    Complete,
    Partial,
    Invalid,
    Unavailable,
}

impl From<MutationCompleteness> for Completeness {
    fn from(value: MutationCompleteness) -> Self {
        match value {
            MutationCompleteness::Complete => Self::Complete,
            MutationCompleteness::Partial => Self::Partial,
            MutationCompleteness::Invalid => Self::Invalid,
            MutationCompleteness::Unavailable => Self::Unavailable,
        }
    }
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Baseline {
    Passed,
    Failed,
    Missing,
}

impl From<MutationBaseline> for Baseline {
    fn from(value: MutationBaseline) -> Self {
        match value {
            MutationBaseline::Passed => Self::Passed,
            MutationBaseline::Failed => Self::Failed,
            MutationBaseline::Missing => Self::Missing,
        }
    }
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum GuestIdentity {
    Guest,
    Redacted,
    Unavailable,
}

impl From<MutationGuestIdentity> for GuestIdentity {
    fn from(value: MutationGuestIdentity) -> Self {
        match value {
            MutationGuestIdentity::Guest => Self::Guest,
            MutationGuestIdentity::Redacted => Self::Redacted,
            MutationGuestIdentity::Unavailable => Self::Unavailable,
        }
    }
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum MutantOutcome {
    Caught,
    Missed,
    Timeout,
    Unviable,
    Success,
    Failure,
}

impl From<MutationOutcomeClass> for MutantOutcome {
    fn from(value: MutationOutcomeClass) -> Self {
        match value {
            MutationOutcomeClass::Caught => Self::Caught,
            MutationOutcomeClass::Missed => Self::Missed,
            MutationOutcomeClass::Timeout => Self::Timeout,
            MutationOutcomeClass::Unviable => Self::Unviable,
            MutationOutcomeClass::Success => Self::Success,
            MutationOutcomeClass::Failure => Self::Failure,
        }
    }
}

/// Every count carries the denominator it is measured against.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Counts {
    /// Mutants generated for this selection (the listing-pass denominator).
    generated: u32,
    /// Mutant scenarios with a recorded outcome, excluding the baseline.
    tested: u32,
    /// Mutants that built and ran: the denominator for `caught` and `missed`.
    viable: u32,
    caught: u32,
    missed: u32,
    timeout: u32,
    unviable: u32,
    /// Mutant scenarios whose recorded class is not a mutation verdict.
    other: u32,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Mutant {
    #[schemars(length(min = 1, max = 256))]
    name: String,
    outcome: MutantOutcome,
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ArtifactKind {
    OutcomesJson,
    ArchiveBundle,
    ToolLog,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Artifact {
    kind: ArtifactKind,
    #[schemars(length(min = 1, max = 512))]
    uri: String,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    sha256: String,
    size_bytes: u64,
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
    #[schemars(length(min = 1, max = 32))]
    declared_toolchain: Option<String>,
    #[schemars(length(max = 64))]
    mutants_version: String,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Omissions {
    mutants_omitted: u64,
    outcomes_truncated: bool,
    report_bundle_unavailable: bool,
    report_bundle_entries: u16,
    stdout_truncated: bool,
    stderr_truncated: bool,
    artifacts_unavailable: bool,
    /// The generated mutant set exceeded `max_mutants`; nothing was built.
    mutant_limit_exceeded: bool,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    baseline: Baseline,
    validation_complete: bool,
    completeness: Completeness,
    counts: Counts,
    #[schemars(length(max = 128))]
    mutants: Vec<Mutant>,
    /// The guest recorded its own identity in `mutants.out/lock.json`. The file
    /// itself is never published; only this verdict is.
    guest_identity: GuestIdentity,
    #[schemars(with = "super::check::schemas::ExecutionTermination")]
    termination: rust_engineering_domain::ExecutionTermination,
    exit_code: Option<i32>,
    runtime: RuntimeEvidence,
    #[schemars(length(max = 128))]
    artifacts: Vec<Artifact>,
    omissions: Omissions,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[allow(dead_code)] // One closed vocabulary; not every code is reachable in one cut.
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
    MutantLimitExceeded,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
#[allow(dead_code)] // Constructed through `encode_task_result`'s closed mapping.
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

pub(super) struct MutationTestTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    runtime: Option<MutationRuntime>,
}

struct MutationRuntime {
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
    store: Arc<Mutex<Store>>,
}

impl MutationTestTool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition = Tool::new(
            NAME,
            "Run cargo-mutants over captured Rust source in the approved offline gateway. The baseline test run is mandatory and a failing baseline is reported as a failed outcome with its own evidence, never as a clean mutation report. Mutants are applied only inside a private writable copy in the sandbox: the captured source is mounted read-only, no mutated source is ever written back or exported, and only the machine-readable mutants.out report leaves the guest. Verdicts come only from outcomes.json plus its caught/missed/timeout/unviable lists; tool text is never an oracle. A surviving (missed) mutant fails; timeout, unviable and incomplete evidence never count as clean, and every count carries its denominator. At most 100 mutants per job with a per-mutant timeout of at most 60 seconds; a larger generated set is refused before anything is built. No mutation selection fits the 60-second synchronous budget, so auto requires negotiated MCP Tasks and task mode is accepted only when the peer declares io.modelcontextprotocol/tasks. Installs nothing and never shards.",
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
    ) -> Self {
        self.runtime = Some(MutationRuntime {
            registry,
            workers,
            inspector,
            ready,
            store: resources.store(),
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
        // Identical negotiation to M3-01, with the mutation budget: one build
        // plus one bounded run per capped mutant never fits 60 seconds, so this
        // currently always yields the structured remediation below.
        match select_execution_mode(input.mode(), false, synchronous_qualified(&options))? {
            ExecutionSelection::Task => {
                return Err(ErrorData::internal_error(
                    "Tasks are not enabled for mutation testing",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => return self.tasks_required(),
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("Mutation runtime is not configured", None))?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.encode_operational(
                OperationalErrorCode::SandboxDenied,
                "Mutation testing requires completed discovery; retry with a new request ID",
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
        let budget = total_budget_seconds(&options);
        // ADR-060 excludes joined cleanup from the work budget; the gateway
        // enforces the work cap while the outer join permits its cleanup.
        let joined = runtime
            .workers
            .run_joined_with(
                Arc::clone(&permit),
                context.ct,
                started + Duration::from_secs(budget + 240),
                move |control| {
                    let mut registry = registry.lock().map_err(|_| InspectionError::Internal)?;
                    let mut stage0 = store.lock().map_err(|_| InspectionError::Internal)?;
                    let (observation, artifacts) = registry.mutation_test(
                        &project_ref,
                        &options,
                        inspector.as_ref(),
                        &mut *stage0,
                        &WallClock,
                        control,
                    )?;
                    MutationTestTaskResult::new(
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
            Ok(result) => self.encode_task_result(&input.project_ref, result),
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
                error_message: "Mutation testing requires MCP Tasks",
                data: None,
            },
            summary: "Mutation testing exceeds the synchronous work budget",
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
                    summary: "Mutation testing cancelled after joined cleanup",
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
                Err(ErrorData::internal_error("Mutation testing failed", None))
            }
        }
    }

    pub(super) fn encode_task_result(
        &self,
        project_ref: &ProjectRef,
        result: MutationTestTaskResult,
    ) -> Result<CallToolResult, ErrorData> {
        let (observation, artifacts, expected_artifacts, duration_ms) = result.into_parts();
        observation
            .validate()
            .map_err(|_| ErrorData::internal_error("Mutation result validation failed", None))?;
        let published = published_artifacts(project_ref, artifacts)?;
        let artifacts_unavailable = published.len() < usize::from(expected_artifacts)
            || published
                .iter()
                .any(|artifact| artifact.completeness == Completeness::Unavailable);
        let timed_out =
            observation.termination == rust_engineering_domain::ExecutionTermination::TimedOut;
        let unavailable = observation.completeness == MutationCompleteness::Unavailable
            && observation.exit_code.is_none()
            && !timed_out;
        let cap_exceeded = observation.cap_exceeded;
        let clean = observation.clean() && !artifacts_unavailable;
        let failed = observation.conclusive_failure();
        let data = Box::new(data(
            project_ref,
            observation,
            published,
            artifacts_unavailable,
        ));
        let output = if timed_out {
            Output {
                outcome: Outcome::Blocked {
                    error_code: Code::CommandTimeout,
                    error_message: "Mutation testing exceeded its deadline",
                    data: Some(data),
                },
                summary: "Mutation testing timed out after joined cleanup",
                duration_ms,
            }
        } else if cap_exceeded {
            Output {
                outcome: Outcome::Blocked {
                    error_code: Code::MutantLimitExceeded,
                    error_message: "The generated mutant set exceeds max_mutants; narrow the selection",
                    data: Some(data),
                },
                summary: "Mutation testing refused an oversized mutant set",
                duration_ms,
            }
        } else if unavailable {
            Output {
                outcome: Outcome::Unavailable {
                    error_code: Code::ToolNotInstalled,
                    error_message: "Approved cargo-mutants runtime is unavailable",
                    data: (),
                },
                summary: "Mutation testing is unavailable",
                duration_ms,
            }
        } else if failed {
            // A surviving mutant or a failing baseline is conclusive on the
            // parsed report alone: absent secondary evidence must not erase it.
            Output {
                outcome: Outcome::Failed {
                    error_code: (),
                    error_message: (),
                    data,
                },
                summary: "Mutation testing found surviving mutants or a failing baseline",
                duration_ms,
            }
        } else if clean {
            Output {
                outcome: Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data,
                },
                summary: "Every viable mutant was caught",
                duration_ms,
            }
        } else {
            Output {
                outcome: Outcome::Blocked {
                    error_code: Code::InvalidProject,
                    error_message: "Mutation evidence is incomplete",
                    data: Some(data),
                },
                summary: "Mutation evidence is incomplete",
                duration_ms,
            }
        };
        let result = self.contract.encode(output)?;
        if serde_json::to_vec(&result)
            .map_err(|_| ErrorData::internal_error("Mutation response encoding failed", None))?
            .len()
            > MAX_RESULT
        {
            return Err(ErrorData::internal_error(
                "Mutation response exceeds its fixed budget",
                None,
            ));
        }
        Ok(result)
    }
}

fn data(
    project_ref: &ProjectRef,
    observation: MutationTestObservation,
    artifacts: Vec<Artifact>,
    artifacts_unavailable: bool,
) -> Data {
    Data {
        project_ref: project_ref.to_string(),
        baseline: observation.baseline.into(),
        validation_complete: observation.validation_complete,
        completeness: observation.completeness.into(),
        counts: Counts {
            generated: observation.counts.generated,
            tested: observation.counts.tested,
            viable: observation.counts.viable(),
            caught: observation.counts.caught,
            missed: observation.counts.missed,
            timeout: observation.counts.timeout,
            unviable: observation.counts.unviable,
            other: observation.counts.other,
        },
        mutants: observation
            .mutants
            .iter()
            .map(|row| Mutant {
                name: row.name().to_owned(),
                outcome: row.class().into(),
            })
            .collect(),
        guest_identity: observation.guest_identity.into(),
        termination: observation.termination,
        exit_code: observation.exit_code,
        runtime: RuntimeEvidence {
            platform: observation.runtime.platform,
            image_id: observation.runtime.image_id,
            configuration_fingerprint: observation.runtime.configuration_fingerprint.to_string(),
            execution_fingerprint: observation.execution_fingerprint.to_string(),
            rust_version: observation.runtime.rust_version,
            cargo_version: observation.runtime.cargo_version,
            declared_toolchain: observation.runtime.declared_toolchain,
            mutants_version: observation.mutants_version,
        },
        artifacts,
        omissions: Omissions {
            mutants_omitted: observation.mutants_omitted,
            outcomes_truncated: observation.artifacts.outcomes_truncated,
            report_bundle_unavailable: observation.artifacts.bundle_unavailable,
            report_bundle_entries: observation.artifacts.bundle_entries,
            stdout_truncated: observation.artifacts.stdout_truncated,
            stderr_truncated: observation.artifacts.stderr_truncated,
            artifacts_unavailable,
            mutant_limit_exceeded: observation.cap_exceeded,
        },
    }
}

fn operational_message(code: OperationalErrorCode) -> &'static str {
    match code {
        OperationalErrorCode::ProjectNotFound => "Project reference is missing or expired",
        OperationalErrorCode::InvalidProject => "Captured project is invalid or unsupported",
        OperationalErrorCode::ToolNotInstalled => "Approved cargo-mutants runtime is unavailable",
        OperationalErrorCode::LockfileUpdateRequired => "Lockfile update is required",
        OperationalErrorCode::CommandTimeout => "Mutation testing exceeded its deadline",
        OperationalErrorCode::SandboxDenied => {
            "Host runtime policy, failed calibration or capacity denied mutation testing"
        }
        OperationalErrorCode::NetworkDenied => "Network access is denied",
        OperationalErrorCode::UnsupportedPlatform => {
            "Secure mutation execution is unavailable on this platform"
        }
        OperationalErrorCode::OutputLimitExceeded => {
            "Mutation evidence exceeded its fixed output budget"
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
    artifacts: Vec<MutationArtifactReference>,
) -> Result<Vec<Artifact>, ErrorData> {
    if artifacts.len() > 128 {
        return Err(ErrorData::internal_error(
            "Mutation artifact limit exceeded",
            None,
        ));
    }
    artifacts
        .into_iter()
        .map(|artifact| match artifact {
            MutationArtifactReference::Ephemeral { kind, metadata } => {
                if &metadata.owner != project_ref {
                    return Err(ErrorData::internal_error(
                        "Mutation artifact authorization failed",
                        None,
                    ));
                }
                Ok(Artifact {
                    kind: match kind {
                        MutationArtifactKind::OutcomesJson => ArtifactKind::OutcomesJson,
                        MutationArtifactKind::ArchiveBundle => ArtifactKind::ArchiveBundle,
                        MutationArtifactKind::StdoutLog | MutationArtifactKind::StderrLog => {
                            ArtifactKind::ToolLog
                        }
                    },
                    uri: format!("rust-artifact://{project_ref}/{}", metadata.id),
                    sha256: hex(&metadata.sha256),
                    size_bytes: u64::from(metadata.size_bytes),
                    completeness: if metadata.truncated {
                        Completeness::Partial
                    } else {
                        Completeness::Complete
                    },
                })
            }
            MutationArtifactReference::Durable(descriptor) => {
                descriptor.validate().map_err(|_| {
                    ErrorData::internal_error("Mutation artifact validation failed", None)
                })?;
                let length = descriptor.size_bytes.min(320 * 1024);
                Ok(Artifact {
                    kind: match descriptor.source.guest_name {
                        GuestArtifactName::ReportArchive => ArtifactKind::ArchiveBundle,
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
mod tests;
