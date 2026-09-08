//! D22: independently sourced supply chain facts with the shared Tasks and Resource lifecycle.
#[allow(dead_code)]
pub(super) mod schemas;
use super::auditing::provider::{AuditProvider, HostAuditConfig};
use super::catalog::provider::CatalogProvider;
use super::deny::HostSecurityConfig;
use super::{
    HostCargoVendorConfig,
    clock::WallClock,
    project::Registry,
    quality_artifacts::DurableSecurityPublisher,
    security_tool::{
        CommonFailure, SynchronousSelection, artifact_fields, capture_vendor, classify_error,
        define_security_artifact, define_security_data, define_security_input,
        define_security_output, define_security_tool, encode_bounded, load_policy,
        run_joined_security, synchronous_selection,
    },
    workers::Workers,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::InspectionError;
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::supply_chain::{PublishedSupply, SupplyInputs, SupplyPorts};
use rust_engineering_domain::security::DenyOptions;
use rust_engineering_domain::supply_chain::SupplyObservation;
use rust_engineering_domain::{ArtifactCompleteness, ProjectRef, ToolStatus};
use rust_engineering_execution::RustProjectInspector;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const NAME: &str = "rust.supply_chain.inspect";
pub(super) fn advertised() -> bool {
    super::security_tool::advertised("RUST_MCP_TEST_SUPPLY_READY")
}
define_security_input!(120, 120);
#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
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
#[derive(Clone, serde::Serialize, schemars::JsonSchema)]
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
define_security_output!(
    Outcome::Passed { .. } => ToolStatus::Passed,
    Outcome::Blocked { .. } => ToolStatus::Blocked,
    Outcome::Unavailable { .. } => ToolStatus::Unavailable,
    Outcome::Cancelled { .. } => ToolStatus::Cancelled,
);
define_security_artifact!("super::deny::schemas::ArtifactCompleteness");
define_security_data!(SupplyObservation, "schemas::Observation");
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
define_security_tool!(
    SupplyTool,
    "Inspect captured dependency facts, checksums, duplicates, features, one RustSec audit, dependency policy and exact-version yanked facts from one authenticated catalog generation. Missing sources and freshness remain explicit. Source locators are withheld; this is no security score or legal approval. Requires durable evidence; performs no acquisition. Auto or synchronous supports timeout_seconds at most 60; longer calls require negotiated MCP Tasks. The work budget excludes joined cleanup."
);
impl SupplyTool {
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        self.call_with_token(request, context.ct).await
    }

    async fn call_with_token(
        &self,
        request: CallToolRequestParams,
        request_token: tokio_util::sync::CancellationToken,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let options = DenyOptions::try_from(rust_engineering_domain::security::DenySelection {
            timeout_seconds: input.timeout_seconds,
        })
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))?;
        match synchronous_selection(
            input.execution_mode,
            input.timeout_seconds <= 60,
            "Tasks are not enabled for supply chain inspection",
        )? {
            SynchronousSelection::TasksRequired => {
                return self.blocked(
                    Code::TasksRequired,
                    "Supply chain inspection requires MCP Tasks",
                    None,
                    0,
                );
            }
            SynchronousSelection::Run => {}
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
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let reference = input.project_ref.clone();
        let (result, duration) = run_joined_security(
            &runtime.workers,
            request_token,
            options.timeout_seconds(),
            "Supply chain worker unavailable",
            move |control| {
                let vendor = vendor_config
                    .as_ref()
                    .map(|config| capture_vendor(config, control))
                    .transpose()?;
                let policy = policy_config
                    .as_ref()
                    .map(|config| load_policy(config, control))
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
        .await?;
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
        let (code, message) = match classify_error(error) {
            CommonFailure::Cancelled => {
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
            CommonFailure::ToolNotInstalled => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved supply chain runtime is unavailable",
                    duration_ms,
                );
            }
            CommonFailure::Timeout => (
                Code::CommandTimeout,
                "Supply chain inspection exceeded its deadline",
            ),
            CommonFailure::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            CommonFailure::OutputLimit => (
                Code::OutputLimitExceeded,
                "Supply chain evidence exceeded its fixed budget",
            ),
            CommonFailure::ProjectNotFound => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            CommonFailure::SandboxDenied => (
                Code::SandboxDenied,
                "Approved supply chain execution could not be established",
            ),
            CommonFailure::Specific(_) => (
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
        let data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "recorded_facts_not_a_security_score_or_legal_approval",
            observation: result.observation,
            artifacts: vec![
                artifact_fields(
                    reference,
                    &descriptor,
                    "Invalid supply chain artifact descriptor",
                )?
                .into(),
            ],
        });
        encode_bounded(
            &self.contract,
            data,
            duration_ms,
            "Supply chain serialization failed",
            |data, duration_ms| Output {
                outcome: if data.observation.report.complete
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
                },
                summary: "Recorded supply chain facts with independent source coverage",
                duration_ms,
            },
            |data| {
                if !data.observation.report.trim_one() {
                    return false;
                }
                data.observation.report.complete = false;
                true
            },
            |duration_ms| Output {
                outcome: Outcome::Blocked {
                    error_code: Code::OutputLimitExceeded,
                    error_message: "Supply chain response exceeds its fixed budget",
                    data: None,
                },
                summary: "Supply chain response exceeds its fixed budget",
                duration_ms,
            },
        )
    }
}
#[cfg(test)]
mod tests {
    use super::super::security_tool::assert_common_error_contract;
    use super::*;
    #[test]
    fn call_boundary_covers_task_gate_and_missing_runtime() -> Result<(), Box<dyn std::error::Error>>
    {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()?;
        runtime.block_on(async {
            let tool = SupplyTool::new()?;
            let base = serde_json::from_str::<serde_json::Value>(
                r#"{"project_ref":"prj_00000000000000000000000000000001"}"#,
            )?;
            let arguments = base.as_object().cloned().ok_or("arguments")?;
            let required = tool
                .call_with_token(
                    CallToolRequestParams::new("rust.supply_chain.inspect")
                        .with_arguments(arguments.clone()),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await?;
            assert_eq!(
                required.structured_content.ok_or("content")?["error_code"],
                "TASKS_REQUIRED"
            );
            let mut synchronous = arguments;
            synchronous.insert("execution_mode".into(), serde_json::json!("synchronous"));
            synchronous.insert("timeout_seconds".into(), serde_json::json!(60));
            let error = tool
                .call_with_token(
                    CallToolRequestParams::new("rust.supply_chain.inspect")
                        .with_arguments(synchronous),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .err()
                .ok_or("runtime must be required")?;
            assert_eq!(error.message, "Supply chain runtime is not configured");
            Ok::<_, Box<dyn std::error::Error>>(())
        })
    }
    #[test]
    fn operational_errors_have_closed_status_and_codes() -> Result<(), Box<dyn std::error::Error>> {
        assert_common_error_contract!(SupplyTool::new()?, error);
        Ok(())
    }
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
