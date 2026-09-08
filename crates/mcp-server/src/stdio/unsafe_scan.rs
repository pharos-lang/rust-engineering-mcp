//! D20: isolated syntax scan with the shared Tasks and Resource lifecycle.
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
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::unsafe_scan::{PublishedUnsafe, UnsafeObservation, UnsafePorts};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::unsafe_scan::UnsafeScanOptions;
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

pub(super) const NAME: &str = "rust.unsafe.scan";
const ADVERTISEMENT_READY: bool = true;
pub(super) fn advertised() -> bool {
    #[cfg(feature = "test-hooks")]
    if std::env::var_os("RUST_MCP_TEST_SCANNER_READY").as_deref() == Some(std::ffi::OsStr::new("1"))
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
    #[serde(default = "default_timeout")]
    #[schemars(range(min = 1, max = 120))]
    timeout_seconds: u64,
    #[serde(default)]
    execution_mode: ExecutionModeDto,
}
fn default_timeout() -> u64 {
    120
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
    SyntaxIncomplete,
}
#[derive(Clone, Serialize, JsonSchema)]
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
    observation: UnsafeObservation,
    #[schemars(length(min = 1, max = 1))]
    artifacts: Vec<Artifact>,
}
pub(super) struct UnsafeTool {
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
impl UnsafeTool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Scan captured Rust syntax in an isolated per-file parser. Reports unsafe and extern keyword spans by verified workspace/dependency origin. Does not expand macros, evaluate cfg or scan generated sources; zero findings never proves memory safety or absence of UB. Requires approved scanner runtime and offline vendor. Auto or synchronous supports timeout_seconds at most 60; longer calls require negotiated MCP Tasks. The work budget excludes joined cleanup.",(*contract.input_schema).clone()).with_raw_output_schema(Arc::clone(&contract.output_schema)).with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false));
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
        let options = UnsafeScanOptions::new(input.timeout_seconds)
            .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))?;
        match select_execution_mode(
            input.execution_mode.into(),
            false,
            input.timeout_seconds <= 60,
        )? {
            ExecutionSelection::Task => {
                return Err(ErrorData::internal_error(
                    "Tasks are not enabled for unsafe scan",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => {
                return self.blocked(
                    Code::TasksRequired,
                    "Unsafe scan requires MCP Tasks",
                    None,
                    0,
                );
            }
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("Scanner runtime is not configured", None))?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Discovery must complete before scanning",
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
                "Durable scanner evidence is unavailable",
                0,
            );
        };
        let started = Instant::now();
        let permit = runtime
            .workers
            .admit_job()
            .map_err(|_| ErrorData::internal_error("Scanner worker unavailable", None))?;
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
                        .unsafe_scan_durable(
                            &reference,
                            &vendor,
                            &options,
                            UnsafePorts {
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
                    summary: "Syntax scan cancelled after joined cleanup",
                    duration_ms,
                });
            }
            SecurityError::Inspection(InspectionError::Execution(ExecutionError::Unavailable)) => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved scanner runtime is unavailable",
                    duration_ms,
                );
            }
            SecurityError::Timeout => (Code::CommandTimeout, "Syntax scan exceeded its deadline"),
            SecurityError::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            SecurityError::OutputLimit
            | SecurityError::Inspection(InspectionError::OutputLimit) => (
                Code::OutputLimitExceeded,
                "Scanner evidence exceeded its fixed budget",
            ),
            SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            ))) => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            SecurityError::Inspection(InspectionError::Execution(_)) => (
                Code::SandboxDenied,
                "Approved scanner execution could not be established",
            ),
            _ => (
                Code::InvalidProject,
                "Captured scanner inputs or evidence could not be validated",
            ),
        };
        self.blocked(code, message, None, duration_ms)
    }
    fn encode_result(
        &self,
        reference: &ProjectRef,
        result: PublishedUnsafe,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let descriptor = result.artifact;
        descriptor
            .validate()
            .map_err(|_| ErrorData::internal_error("Invalid scanner artifact descriptor", None))?;
        let mut data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "syntactic_evidence_only_not_memory_safety",
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
            let outcome = if data.observation.report.syntax_complete
                && descriptor.completeness == ArtifactCompleteness::Complete
            {
                Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: data.clone(),
                }
            } else {
                Outcome::Blocked {
                    error_code: Code::SyntaxIncomplete,
                    error_message: "Syntax evidence is partial; no complete scan claimed",
                    data: Some(data.clone()),
                }
            };
            let result = self.contract.encode(Output {
                outcome,
                summary: "Syntactic unsafe and extern evidence; no inference of memory safety",
                duration_ms,
            })?;
            if serde_json::to_vec(&result)
                .map_err(|_| ErrorData::internal_error("Scanner serialization failed", None))?
                .len()
                <= 512 * 1024
            {
                return Ok(result);
            }
            if data.observation.report.findings.pop().is_none() {
                return self.blocked(
                    Code::OutputLimitExceeded,
                    "Scanner response exceeds its fixed budget",
                    None,
                    duration_ms,
                );
            }
            data.observation.report.findings_omitted += 1;
            data.observation.report.syntax_complete = false;
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
    fn closed_scanner_input_rejects_expansion_paths_and_free_flags()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = UnsafeTool::new()?;
        let valid = serde_json::json!({"project_ref":"prj_00000000000000000000000000000001"});
        assert!(tool.contract.decode(valid.as_object().cloned()).is_ok());
        for (key, value) in [
            ("path", serde_json::json!("/tmp/secret")),
            ("expand_macros", serde_json::json!(true)),
            ("flags", serde_json::json!(["--expand"])),
            ("timeout_seconds", serde_json::json!(121)),
            ("timeout_seconds", serde_json::json!(0)),
        ] {
            let mut value_map = valid.as_object().ok_or("object")?.clone();
            value_map.insert(key.into(), value);
            assert!(tool.contract.decode(Some(value_map)).is_err());
        }
        Ok(())
    }
}
