//! D19: extended quality composition with the shared Tasks and Resource lifecycle.
#[allow(dead_code)]
mod schemas;
use super::auditing::provider::{AuditProvider, HostAuditConfig};
use super::deny::HostSecurityConfig;
use super::{
    HostCargoVendorConfig,
    clock::WallClock,
    contract::{Contract, ToolOutput},
    nextest::{ExecutionModeDto, ExecutionSelection, select_execution_mode},
    project::Registry,
    quality_artifacts::DurableSecurityPublisher,
    workers::{Joined, WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::job::JobPermit;
use rust_engineering_application::quality_v2::QualityV2Options;
use rust_engineering_application::quality_v2::{
    PublishedQualityV2, QualityV2Inputs, QualityV2Ports,
};
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::Clock;
use rust_engineering_domain::mutation_test::{MutationTestCommandOptions, MutationTestSelection};
use rust_engineering_domain::quality_v2::QualityV2Observation;
use rust_engineering_domain::quality_v2::QualityV2Profile;
use rust_engineering_domain::{ArtifactCompleteness, OperationalErrorCode, ProjectRef, ToolStatus};
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

pub(super) const NAME: &str = "rust.quality.gate.v2";
const ADVERTISEMENT_READY: bool = true;
pub(super) fn advertised() -> bool {
    #[cfg(feature = "test-hooks")]
    if std::env::var_os("RUST_MCP_TEST_GATE_V2_READY").as_deref() == Some(std::ffi::OsStr::new("1"))
    {
        return true;
    }
    ADVERTISEMENT_READY
}
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    #[schemars(with = "schemas::QualityV2Profile")]
    profile: QualityV2Profile,
    #[serde(default)]
    #[schemars(with = "Option<String>")]
    baseline_project_ref: Option<ProjectRef>,
    #[serde(default)]
    #[schemars(with = "Option<schemas::MutationSelection>")]
    mutation: Option<MutationTestSelection>,
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 3600))]
    timeout_seconds: u64,
    #[serde(default)]
    execution_mode: ExecutionModeDto,
}
fn default_timeout() -> u64 {
    300
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    TasksRequired,
    SandboxDenied,
    MissingOfflineData,
    ArtifactUnavailable,
    ToolNotInstalled,
    InvalidProject,
    ProjectNotFound,
    CommandTimeout,
    OutputLimitExceeded,
    EvidenceIncomplete,
}
#[derive(Clone, Serialize, JsonSchema)]
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
#[derive(Clone, Serialize, JsonSchema)]
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
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Artifact {
    uri: String,
    sha256: String,
    size_bytes: u64,
    #[schemars(with = "super::deny::schemas::ArtifactCompleteness")]
    completeness: ArtifactCompleteness,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    project_ref: String,
    semantics: &'static str,
    #[schemars(with = "schemas::Observation")]
    observation: QualityV2Observation,
    #[schemars(length(min = 1, max = 1))]
    artifacts: Vec<Artifact>,
}
pub(super) struct QualityV2Tool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    runtime: Option<Runtime>,
}
pub(super) struct Runtime {
    pub(super) registry: Arc<Mutex<Registry>>,
    pub(super) workers: Workers,
    pub(super) inspector: Arc<RustProjectInspector>,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) vendor: Option<HostCargoVendorConfig>,
    pub(super) policy: Option<HostSecurityConfig>,
    pub(super) audit: Option<HostAuditConfig>,
    pub(super) publisher: Option<DurableSecurityPublisher>,
}
impl QualityV2Tool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Run strict (M1 standard defaults plus dependency policy and workspace coverage) or release (strict plus SemVer against an explicit baseline ProjectRef). One candidate capture and one shared RustSec audit. Each required stage retains its verdict and normalized evidence; partial, unavailable or skipped evidence never passes. Mutation is explicit and budgeted. Uses fixed offline runtime and host policy/vendor; executes project code in the sandbox. Auto or synchronous supports only strict without mutation and timeout_seconds at most 60; release, mutation and longer calls require negotiated MCP Tasks. The work budget excludes joined cleanup.",(*contract.input_schema).clone()).with_raw_output_schema(Arc::clone(&contract.output_schema)).with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false));
        Ok(Self {
            definition,
            contract,
            runtime: None,
        })
    }
    pub(super) fn with_runtime(mut self, runtime: Runtime) -> Self {
        self.runtime = Some(runtime);
        self
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let options = QualityV2Options {
            profile: input.profile,
            baseline: input.baseline_project_ref.clone(),
            timeout_seconds: input.timeout_seconds,
            mutation: input
                .mutation
                .clone()
                .map(MutationTestCommandOptions::try_from)
                .transpose()
                .map_err(|_| ErrorData::invalid_params("Invalid mutation selection", None))?,
        };
        options.validate().map_err(|_|ErrorData::invalid_params("Release requires baseline, strict rejects baseline, and optional mutation needs its derived budget plus 300 seconds",None))?;
        match select_execution_mode(
            input.execution_mode.into(),
            false,
            input.timeout_seconds <= 60
                && input.profile == QualityV2Profile::Strict
                && input.mutation.is_none(),
        )? {
            ExecutionSelection::Task => {
                return Err(ErrorData::internal_error(
                    "Tasks are not enabled for extended quality gate",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => {
                return self.blocked(
                    Code::TasksRequired,
                    "Extended quality gate requires MCP Tasks",
                    None,
                    0,
                );
            }
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ErrorData::internal_error("Extended quality runtime is not configured", None)
        })?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Discovery must complete before inspection",
                None,
                0,
            );
        }
        let vendor_config = runtime.vendor.clone();
        let policy_config = runtime.policy.clone();
        let auditor = AuditProvider(runtime.audit.clone());
        let Some(mut publisher) = runtime.publisher.clone() else {
            return self.unavailable(
                Code::ArtifactUnavailable,
                "Durable extended quality evidence is unavailable",
                0,
            );
        };
        let started = Instant::now();
        let permit = runtime
            .workers
            .admit_job()
            .map_err(|_| ErrorData::internal_error("Extended quality worker unavailable", None))?;
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let reference = input.project_ref.clone();
        let joined = runtime
            .workers
            .run_joined_with(
                Arc::clone(&permit),
                context.ct,
                started + Duration::from_secs(options.timeout_seconds),
                move |control| {
                    let vendor = vendor_config
                        .as_ref()
                        .map(|config| {
                            rust_engineering_project::capture_with_expected(
                                &config.directory,
                                &config.fingerprint,
                                control,
                            )
                        })
                        .transpose()?;
                    let policy = policy_config
                        .as_ref()
                        .map(|config| -> Result<_, SecurityError> {
                            let bytes = rust_engineering_project::read_host_snapshot(
                                &config.path,
                                control,
                            )?;
                            rust_engineering_execution::parse_security_policy(
                                &bytes,
                                &config.fingerprint,
                                WallClock.now().0,
                            )
                            .map_err(|_| SecurityError::InvalidPolicy)
                        })
                        .transpose()?;
                    registry
                        .lock()
                        .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?
                        .quality_gate_v2(
                            &reference,
                            QualityV2Inputs {
                                vendor: vendor.as_ref(),
                                policy: policy.as_ref(),
                                options: &options,
                            },
                            QualityV2Ports {
                                executor: inspector.as_ref(),
                                auditor: &auditor,
                                publisher: &mut publisher,
                            },
                            &WallClock,
                            control,
                        )
                },
            )
            .await;
        permit.release_after_cleanup();
        let duration = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let result = match joined {
            Ok(joined) => joined_result(joined),
            Err(WorkerError::Cancelled) => Err(ProjectError::Cancelled.into()),
            Err(WorkerError::TimedOut) => Err(SecurityError::Timeout),
            Err(_) => Err(SecurityError::Inspection(InspectionError::Internal)),
        };
        match result {
            Ok(result) => self.encode_result(&input.project_ref, result, duration),
            Err(error) => self.error(error, duration),
        }
    }
    fn blocked(
        &self,
        code: Code,
        message: &'static str,
        data: Option<Box<Data>>,
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
    fn unavailable(
        &self,
        code: Code,
        message: &'static str,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        self.contract.encode(Output {
            outcome: Outcome::Unavailable {
                error_code: code,
                error_message: message,
                data: None,
            },
            summary: message,
            duration_ms,
        })
    }
    fn error(&self, error: SecurityError, duration_ms: u64) -> Result<CallToolResult, ErrorData> {
        let (code, message) = match error {
            SecurityError::Inspection(
                InspectionError::Project(ProjectError::Cancelled)
                | InspectionError::Execution(ExecutionError::Cancelled),
            ) => {
                return self.contract.encode(Output {
                    outcome: Outcome::Cancelled {
                        error_code: (),
                        error_message: (),
                        data: (),
                    },
                    summary: "Extended quality gate cancelled after joined cleanup",
                    duration_ms,
                });
            }
            SecurityError::Inspection(InspectionError::Execution(ExecutionError::Unavailable)) => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved extended quality runtime is unavailable",
                    duration_ms,
                );
            }
            SecurityError::Timeout => (
                Code::CommandTimeout,
                "Extended quality gate exceeded its deadline",
            ),
            SecurityError::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            SecurityError::OutputLimit
            | SecurityError::Inspection(InspectionError::OutputLimit) => (
                Code::OutputLimitExceeded,
                "Extended quality evidence exceeded its fixed budget",
            ),
            SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            ))) => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            SecurityError::Inspection(InspectionError::Execution(_)) => (
                Code::SandboxDenied,
                "Approved extended quality execution could not be established",
            ),
            _ => (
                Code::InvalidProject,
                "Captured extended quality inputs or evidence could not be validated",
            ),
        };
        self.blocked(code, message, None, duration_ms)
    }
    fn encode_result(
        &self,
        reference: &ProjectRef,
        result: PublishedQualityV2,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let descriptor = result.artifact;
        descriptor.validate().map_err(|_| {
            ErrorData::internal_error("Invalid extended quality artifact descriptor", None)
        })?;
        let mut data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "complete_required_quality_stages_over_one_capture",
            observation: result.observation,
            artifacts: vec![Artifact {
                uri: format!(
                    "rust-quality-artifact://{reference}/{}?offset=0&length={}",
                    descriptor.artifact_id,
                    descriptor.size_bytes.min(320 * 1024)
                ),
                sha256: super::resources::hex(&descriptor.sha256),
                size_bytes: descriptor.size_bytes,
                completeness: descriptor.completeness,
            }],
        });
        loop {
            let outcome = if data.observation.report.status == ToolStatus::Passed
                && data.observation.report.complete
                && descriptor.completeness == ArtifactCompleteness::Complete
            {
                Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: data.clone(),
                }
            } else if data.observation.report.status == ToolStatus::Failed {
                Outcome::Failed {
                    error_code: (),
                    error_message: (),
                    data: data.clone(),
                }
            } else if data.observation.report.status == ToolStatus::Unavailable {
                Outcome::Unavailable {
                    error_code: Code::ToolNotInstalled,
                    error_message: "Required quality stage unavailable",
                    data: Some(data.clone()),
                }
            } else {
                Outcome::Blocked {
                    error_code: Code::EvidenceIncomplete,
                    error_message: "Extended quality evidence is partial; inspect independent sources",
                    data: Some(data.clone()),
                }
            };
            let result = self.contract.encode(Output {
                outcome,
                summary: "Recorded extended quality facts with independent source coverage",
                duration_ms,
            })?;
            if serde_json::to_vec(&result)
                .map_err(|_| {
                    ErrorData::internal_error("Extended quality serialization failed", None)
                })?
                .len()
                <= 512 * 1024
            {
                return Ok(result);
            }
            if !data.observation.report.trim_one() {
                return self.blocked(
                    Code::OutputLimitExceeded,
                    "Extended quality response exceeds its fixed budget",
                    None,
                    duration_ms,
                );
            }
        }
    }
}
fn joined_result<T>(joined: Joined<T, SecurityError>) -> Result<T, SecurityError> {
    match (joined.result, joined.interrupted) {
        (
            Err(SecurityError::Inspection(
                InspectionError::Project(ProjectError::Cancelled)
                | InspectionError::Execution(ExecutionError::Cancelled),
            )),
            Some(WorkerError::TimedOut),
        ) => Err(SecurityError::Timeout),
        (Err(error), _) => Err(error),
        (Ok(result), None) => Ok(result),
        (Ok(_), Some(WorkerError::TimedOut)) => Err(SecurityError::Timeout),
        (Ok(_), Some(WorkerError::Cancelled)) => Err(ProjectError::Cancelled.into()),
        _ => Err(SecurityError::Inspection(InspectionError::Internal)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_quality_v2_input_rejects_expansion_paths_and_free_flags()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = QualityV2Tool::new()?;
        let valid = serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","profile":"strict"});
        assert!(tool.contract.decode(valid.as_object().cloned()).is_ok());
        for (key, value) in [
            ("path", serde_json::json!("/tmp/secret")),
            ("expand_macros", serde_json::json!(true)),
            ("flags", serde_json::json!(["--expand"])),
            ("timeout_seconds", serde_json::json!(3601)),
            ("timeout_seconds", serde_json::json!(0)),
        ] {
            let mut value_map = valid.as_object().ok_or("object")?.clone();
            value_map.insert(key.into(), value);
            assert!(tool.contract.decode(Some(value_map)).is_err());
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "quality_v2/trim_tests.rs"]
mod trim_tests;
