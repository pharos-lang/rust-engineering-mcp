//! D19: extended quality composition with the shared Tasks and Resource lifecycle.
#[allow(dead_code)]
mod schemas;
use super::auditing::provider::{AuditProvider, HostAuditConfig};
use super::deny::HostSecurityConfig;
use super::{
    HostCargoVendorConfig,
    clock::WallClock,
    nextest::ExecutionModeDto,
    project::Registry,
    quality_artifacts::DurableSecurityPublisher,
    security_tool::{
        CommonFailure, SynchronousSelection, artifact_fields, capture_vendor, classify_error,
        define_fallible_security_outcome, define_security_artifact, define_security_data,
        define_security_response_methods, define_security_tool, encode_bounded, load_policy,
        run_joined_security, synchronous_selection,
    },
    workers::Workers,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::InspectionError;
use rust_engineering_application::quality_v2::QualityV2Options;
use rust_engineering_application::quality_v2::{
    PublishedQualityV2, QualityV2Inputs, QualityV2Ports,
};
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::mutation_test::{MutationTestCommandOptions, MutationTestSelection};
use rust_engineering_domain::quality_v2::QualityV2Observation;
use rust_engineering_domain::quality_v2::QualityV2Profile;
use rust_engineering_domain::{ArtifactCompleteness, ProjectRef, ToolStatus};
use rust_engineering_execution::RustProjectInspector;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const NAME: &str = "rust.quality.gate.v2";
pub(super) fn advertised() -> bool {
    super::security_tool::advertised("RUST_MCP_TEST_GATE_V2_READY")
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
#[derive(Clone, serde::Serialize, JsonSchema)]
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
define_fallible_security_outcome!((), (), Option<Box<Data>>);
define_security_artifact!("super::deny::schemas::ArtifactCompleteness");
define_security_data!(QualityV2Observation, "schemas::Observation");
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
define_security_tool!(
    QualityV2Tool,
    "Run strict (M1 standard defaults plus dependency policy and workspace coverage) or release (strict plus SemVer against an explicit baseline ProjectRef). One candidate capture and one shared RustSec audit. Each required stage retains its verdict and normalized evidence; partial, unavailable or skipped evidence never passes. Mutation is explicit and budgeted. Uses fixed offline runtime and host policy/vendor; executes project code in the sandbox. Auto or synchronous supports only strict without mutation and timeout_seconds at most 60; release, mutation and longer calls require negotiated MCP Tasks. The work budget excludes joined cleanup."
);
impl QualityV2Tool {
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
        match synchronous_selection(
            input.execution_mode,
            input.timeout_seconds <= 60
                && input.profile == QualityV2Profile::Strict
                && input.mutation.is_none(),
            "Tasks are not enabled for extended quality gate",
        )? {
            SynchronousSelection::TasksRequired => {
                return self.blocked(
                    Code::TasksRequired,
                    "Extended quality gate requires MCP Tasks",
                    None,
                    0,
                );
            }
            SynchronousSelection::Run => {}
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
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let reference = input.project_ref.clone();
        let (result, duration) = run_joined_security(
            &runtime.workers,
            request_token,
            options.timeout_seconds,
            "Extended quality worker unavailable",
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
        .await?;
        match result {
            Ok(result) => self.encode_result(&input.project_ref, result, duration),
            Err(error) => self.error(error, duration),
        }
    }
    define_security_response_methods!(self, None);

    fn error(&self, error: SecurityError, duration_ms: u64) -> Result<CallToolResult, ErrorData> {
        let (code, message) = match classify_error(error) {
            CommonFailure::Cancelled => {
                return self.cancelled(
                    "Extended quality gate cancelled after joined cleanup",
                    duration_ms,
                );
            }
            CommonFailure::ToolNotInstalled => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved extended quality runtime is unavailable",
                    duration_ms,
                );
            }
            CommonFailure::Timeout => (
                Code::CommandTimeout,
                "Extended quality gate exceeded its deadline",
            ),
            CommonFailure::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            CommonFailure::OutputLimit => (
                Code::OutputLimitExceeded,
                "Extended quality evidence exceeded its fixed budget",
            ),
            CommonFailure::ProjectNotFound => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            CommonFailure::SandboxDenied => (
                Code::SandboxDenied,
                "Approved extended quality execution could not be established",
            ),
            CommonFailure::Specific(_) => (
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
        let data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "complete_required_quality_stages_over_one_capture",
            observation: result.observation,
            artifacts: vec![
                artifact_fields(
                    reference,
                    &descriptor,
                    "Invalid extended quality artifact descriptor",
                )?
                .into(),
            ],
        });
        encode_bounded(
            &self.contract,
            data,
            duration_ms,
            "Extended quality serialization failed",
            |data, duration_ms| Output {
                outcome: if data.observation.report.status == ToolStatus::Passed
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
                },
                summary: "Recorded extended quality facts with independent source coverage",
                duration_ms,
            },
            |data| data.observation.report.trim_one(),
            |duration_ms| Output {
                outcome: Outcome::Blocked {
                    error_code: Code::OutputLimitExceeded,
                    error_message: "Extended quality response exceeds its fixed budget",
                    data: None,
                },
                summary: "Extended quality response exceeds its fixed budget",
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
            let tool = QualityV2Tool::new()?;
            let base = serde_json::from_str::<serde_json::Value>(
                r#"{"project_ref":"prj_00000000000000000000000000000001","profile":"strict"}"#,
            )?;
            let arguments = base.as_object().cloned().ok_or("arguments")?;
            let required = tool
                .call_with_token(
                    CallToolRequestParams::new("rust.quality.gate.v2")
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
                    CallToolRequestParams::new("rust.quality.gate.v2").with_arguments(synchronous),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .err()
                .ok_or("runtime must be required")?;
            assert_eq!(error.message, "Extended quality runtime is not configured");
            Ok::<_, Box<dyn std::error::Error>>(())
        })
    }
    #[test]
    fn operational_errors_have_closed_status_and_codes() -> Result<(), Box<dyn std::error::Error>> {
        assert_common_error_contract!(QualityV2Tool::new()?, error);
        Ok(())
    }
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
