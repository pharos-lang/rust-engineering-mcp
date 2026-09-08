//! D22: independently sourced supply chain facts with the shared Tasks and Resource lifecycle.
#[allow(dead_code)]
pub(super) mod schemas;
use super::auditing::provider::{AuditProvider, HostAuditConfig};
use super::catalog::provider::CatalogProvider;
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
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::supply_chain::{PublishedSupply, SupplyInputs, SupplyPorts};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::Clock;
use rust_engineering_domain::security::DenyOptions;
use rust_engineering_domain::supply_chain::SupplyObservation;
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

pub(super) const NAME: &str = "rust.supply_chain.inspect";
const ADVERTISEMENT_READY: bool = true;
pub(super) fn advertised() -> bool {
    #[cfg(feature = "test-hooks")]
    if std::env::var_os("RUST_MCP_TEST_SUPPLY_READY").as_deref() == Some(std::ffi::OsStr::new("1"))
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
    observation: SupplyObservation,
    #[schemars(length(min = 1, max = 1))]
    artifacts: Vec<Artifact>,
}
pub(super) struct SupplyTool {
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
    pub(super) catalog: Arc<CatalogProvider>,
    pub(super) publisher: Option<DurableSecurityPublisher>,
}
impl SupplyTool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Inspect captured dependency facts, checksums, duplicates, features, one RustSec audit, dependency policy and exact-version yanked facts from one authenticated catalog generation. Missing sources and freshness remain explicit. Source locators are withheld; this is no security score or legal approval. Requires durable evidence; performs no acquisition. Auto or synchronous supports timeout_seconds at most 60; longer calls require negotiated MCP Tasks. The work budget excludes joined cleanup.",(*contract.input_schema).clone()).with_raw_output_schema(Arc::clone(&contract.output_schema)).with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false));
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
        let options = DenyOptions::try_from(rust_engineering_domain::security::DenySelection {
            timeout_seconds: input.timeout_seconds,
        })
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))?;
        match select_execution_mode(
            input.execution_mode.into(),
            false,
            input.timeout_seconds <= 60,
        )? {
            ExecutionSelection::Task => {
                return Err(ErrorData::internal_error(
                    "Tasks are not enabled for supply chain inspection",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => {
                return self.blocked(
                    Code::TasksRequired,
                    "Supply chain inspection requires MCP Tasks",
                    None,
                    0,
                );
            }
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            ErrorData::internal_error("Supply chain runtime is not configured", None)
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
        let catalog = Arc::clone(&runtime.catalog);
        let Some(mut publisher) = runtime.publisher.clone() else {
            return self.unavailable(
                Code::ArtifactUnavailable,
                "Durable supply chain evidence is unavailable",
                0,
            );
        };
        let started = Instant::now();
        let permit = runtime
            .workers
            .admit_job()
            .map_err(|_| ErrorData::internal_error("Supply chain worker unavailable", None))?;
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
                        .supply_chain_durable(
                            &reference,
                            SupplyInputs {
                                vendor: vendor.as_ref(),
                                policy: policy.as_ref(),
                                options: &options,
                            },
                            SupplyPorts {
                                executor: inspector.as_ref(),
                                auditor: &auditor,
                                catalog: catalog.as_ref(),
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
                    summary: "Supply chain inspection cancelled after joined cleanup",
                    duration_ms,
                });
            }
            SecurityError::Inspection(InspectionError::Execution(ExecutionError::Unavailable)) => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved supply chain runtime is unavailable",
                    duration_ms,
                );
            }
            SecurityError::Timeout => (
                Code::CommandTimeout,
                "Supply chain inspection exceeded its deadline",
            ),
            SecurityError::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            SecurityError::OutputLimit
            | SecurityError::Inspection(InspectionError::OutputLimit) => (
                Code::OutputLimitExceeded,
                "Supply chain evidence exceeded its fixed budget",
            ),
            SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            ))) => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            SecurityError::Inspection(InspectionError::Execution(_)) => (
                Code::SandboxDenied,
                "Approved supply chain execution could not be established",
            ),
            _ => (
                Code::InvalidProject,
                "Captured supply chain inputs or evidence could not be validated",
            ),
        };
        self.blocked(code, message, None, duration_ms)
    }
    fn encode_result(
        &self,
        reference: &ProjectRef,
        result: PublishedSupply,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let descriptor = result.artifact;
        descriptor.validate().map_err(|_| {
            ErrorData::internal_error("Invalid supply chain artifact descriptor", None)
        })?;
        let mut data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "recorded_facts_not_a_security_score_or_legal_approval",
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
            let outcome = if data.observation.report.complete
                && descriptor.completeness == ArtifactCompleteness::Complete
            {
                Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: data.clone(),
                }
            } else {
                Outcome::Blocked {
                    error_code: Code::EvidenceIncomplete,
                    error_message: "Supply chain evidence is partial; inspect independent sources",
                    data: Some(data.clone()),
                }
            };
            let result = self.contract.encode(Output {
                outcome,
                summary: "Recorded supply chain facts with independent source coverage",
                duration_ms,
            })?;
            if serde_json::to_vec(&result)
                .map_err(|_| ErrorData::internal_error("Supply chain serialization failed", None))?
                .len()
                <= 512 * 1024
            {
                return Ok(result);
            }
            if !data.observation.report.trim_one() {
                return self.blocked(
                    Code::OutputLimitExceeded,
                    "Supply chain response exceeds its fixed budget",
                    None,
                    duration_ms,
                );
            }
            data.observation.report.complete = false;
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
    fn closed_supply_chain_input_rejects_expansion_paths_and_free_flags()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = SupplyTool::new()?;
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
#[cfg(test)]
#[path = "supply_chain/trim_tests.rs"]
mod trim_tests;
