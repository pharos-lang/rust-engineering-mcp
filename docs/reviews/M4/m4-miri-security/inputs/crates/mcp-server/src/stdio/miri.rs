//! D21: isolated interpreter evidence with the shared Tasks and Resource lifecycle.
#[allow(dead_code)]
mod schemas;
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
use rust_engineering_application::miri::{MiriObservation, MiriPorts, PublishedMiri};
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::miri::MiriOptions;
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

pub(super) const NAME: &str = "rust.miri";
const ADVERTISEMENT_READY: bool = false;
pub(super) fn advertised() -> bool {
    #[cfg(feature = "test-hooks")]
    if std::env::var_os("RUST_MCP_TEST_MIRI_READY").as_deref() == Some(std::ffi::OsStr::new("1")) {
        return true;
    }
    ADVERTISEMENT_READY
}
#[derive(Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 1800))]
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
    ObservedFailure,
    ClassificationIntegrityUnsupported,
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
        error_code: Code,
        error_message: &'static str,
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
    observation: MiriObservation,
    #[schemars(length(min = 1, max = 1))]
    artifacts: Vec<Artifact>,
}
pub(super) struct MiriTool {
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
    pub(super) publisher: Option<DurableSecurityPublisher>,
}
impl MiriTool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Run lib and integration tests in the approved offline Miri interpreter. Reports observed UB, unsupported operations, ordinary test failures and compilation failures separately. Requires fixed nightly/sysroot, authenticated vendor and MCP Tasks. Project proc macros, build scripts and custom harnesses are rejected to preserve diagnostic origin. Clean tests do not prove universal memory safety.",(*contract.input_schema).clone()).with_raw_output_schema(Arc::clone(&contract.output_schema)).with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false));
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
        let options = MiriOptions::new(input.timeout_seconds)
            .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))?;
        match select_execution_mode(input.execution_mode.into(), false, false)? {
            ExecutionSelection::Task => {
                return Err(ErrorData::internal_error(
                    "Tasks are not enabled for Miri",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => {
                return self.blocked(Code::TasksRequired, "Miri requires MCP Tasks", None, 0);
            }
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ErrorData::internal_error("Interpreter runtime is not configured", None)
        })?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Discovery must complete before interpreting",
                None,
                0,
            );
        }
        let Some(vendor) = runtime.vendor.clone() else {
            return self.unavailable(
                Code::MissingOfflineData,
                "Host-authenticated offline vendor is required",
                0,
            );
        };
        let Some(mut publisher) = runtime.publisher.clone() else {
            return self.unavailable(
                Code::ArtifactUnavailable,
                "Durable interpreter evidence is unavailable",
                0,
            );
        };
        let started = Instant::now();
        let permit = runtime
            .workers
            .admit_job()
            .map_err(|_| ErrorData::internal_error("Interpreter worker unavailable", None))?;
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let reference = input.project_ref.clone();
        let joined = runtime
            .workers
            .run_joined_with(
                Arc::clone(&permit),
                context.ct,
                started + Duration::from_secs(options.timeout_seconds()),
                move |control| {
                    let vendor = rust_engineering_project::capture_with_expected(
                        &vendor.directory,
                        &vendor.fingerprint,
                        control,
                    )?;
                    registry
                        .lock()
                        .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?
                        .miri_durable(
                            &reference,
                            &vendor,
                            &options,
                            MiriPorts {
                                executor: inspector.as_ref(),
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
                data: (),
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
                    summary: "Miri cancelled after joined cleanup",
                    duration_ms,
                });
            }
            SecurityError::Inspection(InspectionError::Execution(ExecutionError::Unavailable)) => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved interpreter runtime is unavailable",
                    duration_ms,
                );
            }
            SecurityError::ClassificationIntegrityUnsupported => (
                Code::ClassificationIntegrityUnsupported,
                "Miri diagnostic integrity requires packages without build scripts, proc macros or custom harnesses",
            ),
            SecurityError::Timeout => (Code::CommandTimeout, "Miri exceeded its deadline"),
            SecurityError::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            SecurityError::OutputLimit
            | SecurityError::Inspection(InspectionError::OutputLimit) => (
                Code::OutputLimitExceeded,
                "Interpreter evidence exceeded its fixed budget",
            ),
            SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            ))) => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            SecurityError::Inspection(InspectionError::Execution(_)) => (
                Code::SandboxDenied,
                "Approved interpreter execution could not be established",
            ),
            _ => (
                Code::InvalidProject,
                "Captured interpreter inputs or evidence could not be validated",
            ),
        };
        self.blocked(code, message, None, duration_ms)
    }
    fn encode_result(
        &self,
        reference: &ProjectRef,
        result: PublishedMiri,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let descriptor = result.artifact;
        descriptor.validate().map_err(|_| {
            ErrorData::internal_error("Invalid interpreter artifact descriptor", None)
        })?;
        let mut data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "observed_interpreter_evidence_not_a_proof_of_memory_safety",
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
            let outcome = if data.observation.report.clean
                && descriptor.completeness == ArtifactCompleteness::Complete
            {
                Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: data.clone(),
                }
            } else if data.observation.report.counts.failed > 0
                || data.observation.report.counts.compile_failures > 0
            {
                Outcome::Failed {
                    error_code: Code::ObservedFailure,
                    error_message: "Interpreter or compilation failure observed; inspect categories and coverage",
                    data: data.clone(),
                }
            } else {
                Outcome::Blocked {
                    error_code: Code::EvidenceIncomplete,
                    error_message: "Interpreter evidence is partial",
                    data: Some(data.clone()),
                }
            };
            let result = self.contract.encode(Output {
                outcome,
                summary: "Observed interpreter evidence; no proof of universal memory safety",
                duration_ms,
            })?;
            if serde_json::to_vec(&result)
                .map_err(|_| ErrorData::internal_error("Interpreter serialization failed", None))?
                .len()
                <= 512 * 1024
            {
                return Ok(result);
            }
            if data.observation.report.findings.pop().is_none() {
                return self.blocked(
                    Code::OutputLimitExceeded,
                    "Interpreter response exceeds its fixed budget",
                    None,
                    duration_ms,
                );
            }
            data.observation.report.findings_omitted += 1;
            data.observation.report.complete = false;
            data.observation.report.clean = false;
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
    fn closed_interpreter_input_rejects_expansion_paths_and_free_flags()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = MiriTool::new()?;
        let valid = serde_json::json!({"project_ref":"prj_00000000000000000000000000000001"});
        assert!(tool.contract.decode(valid.as_object().cloned()).is_ok());
        for (key, value) in [
            ("path", serde_json::json!("/tmp/secret")),
            ("expand_macros", serde_json::json!(true)),
            ("flags", serde_json::json!(["--expand"])),
            ("timeout_seconds", serde_json::json!(1801)),
            ("timeout_seconds", serde_json::json!(0)),
        ] {
            let mut value_map = valid.as_object().ok_or("object")?.clone();
            value_map.insert(key.into(), value);
            assert!(tool.contract.decode(Some(value_map)).is_err());
        }
        Ok(())
    }
}
