//! D20: isolated syntax scan with the shared Tasks and Resource lifecycle.
#[allow(dead_code)]
mod schemas;
use super::{
    HostCargoVendorConfig,
    clock::WallClock,
    project::Registry,
    quality_artifacts::DurableSecurityPublisher,
    security_tool::{
        CommonFailure, SynchronousSelection, artifact_fields, capture_vendor, classify_error,
        define_security_artifact, define_security_data, define_security_input,
        define_security_outcome, define_security_response_methods, define_security_tool,
        encode_bounded, run_joined_security, synchronous_selection,
    },
    workers::Workers,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::InspectionError;
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::unsafe_scan::{PublishedUnsafe, UnsafeObservation, UnsafePorts};
use rust_engineering_domain::unsafe_scan::UnsafeScanOptions;
use rust_engineering_domain::{ArtifactCompleteness, ProjectRef};
use rust_engineering_execution::RustProjectInspector;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const NAME: &str = "rust.unsafe.scan";
pub(super) fn advertised() -> bool {
    super::security_tool::advertised("RUST_MCP_TEST_SCANNER_READY")
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
    SyntaxIncomplete,
}
define_security_outcome!(());
define_security_artifact!("super::deny::schemas::ArtifactCompleteness");
define_security_data!(UnsafeObservation, "schemas::Observation");
pub(super) struct Runtime {
    pub(super) registry: Arc<Mutex<Registry>>,
    pub(super) workers: Workers,
    pub(super) inspector: Arc<RustProjectInspector>,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) vendor: Option<HostCargoVendorConfig>,
    pub(super) publisher: Option<DurableSecurityPublisher>,
}
define_security_tool!(
    UnsafeTool,
    "Scan captured Rust syntax in an isolated per-file parser. Reports unsafe and extern keyword spans by verified workspace/dependency origin. Does not expand macros, evaluate cfg or scan generated sources; zero findings never proves memory safety or absence of UB. Requires approved scanner runtime and offline vendor. Auto or synchronous supports timeout_seconds at most 60; longer calls require negotiated MCP Tasks. The work budget excludes joined cleanup."
);
impl UnsafeTool {
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
        let options = UnsafeScanOptions::new(input.timeout_seconds)
            .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))?;
        match synchronous_selection(
            input.execution_mode,
            input.timeout_seconds <= 60,
            "Tasks are not enabled for unsafe scan",
        )? {
            SynchronousSelection::TasksRequired => {
                return self.blocked(
                    Code::TasksRequired,
                    "Unsafe scan requires MCP Tasks",
                    None,
                    0,
                );
            }
            SynchronousSelection::Run => {}
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
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let reference = input.project_ref.clone();
        let (result, duration) = run_joined_security(
            &runtime.workers,
            request_token,
            options.timeout_seconds(),
            "Scanner worker unavailable",
            move |control| {
                let vendor = capture_vendor(&vendor, control)?;
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
        .await?;
        match result {
            Ok(result) => self.encode_result(&input.project_ref, result, duration),
            Err(error) => self.error(error, duration),
        }
    }
    define_security_response_methods!(self, ());

    fn error(&self, error: SecurityError, duration_ms: u64) -> Result<CallToolResult, ErrorData> {
        let (code, message) = match classify_error(error) {
            CommonFailure::Cancelled => {
                return self.cancelled("Syntax scan cancelled after joined cleanup", duration_ms);
            }
            CommonFailure::ToolNotInstalled => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved scanner runtime is unavailable",
                    duration_ms,
                );
            }
            CommonFailure::Timeout => (Code::CommandTimeout, "Syntax scan exceeded its deadline"),
            CommonFailure::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            CommonFailure::OutputLimit => (
                Code::OutputLimitExceeded,
                "Scanner evidence exceeded its fixed budget",
            ),
            CommonFailure::ProjectNotFound => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            CommonFailure::SandboxDenied => (
                Code::SandboxDenied,
                "Approved scanner execution could not be established",
            ),
            CommonFailure::Specific(_) => (
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
        let data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "syntactic_evidence_only_not_memory_safety",
            observation: result.observation,
            artifacts: vec![
                artifact_fields(
                    reference,
                    &descriptor,
                    "Invalid scanner artifact descriptor",
                )?
                .into(),
            ],
        });
        encode_bounded(
            &self.contract,
            data,
            duration_ms,
            "Scanner serialization failed",
            |data, duration_ms| Output {
                outcome: if data.observation.report.syntax_complete
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
                },
                summary: "Syntactic unsafe and extern evidence; no inference of memory safety",
                duration_ms,
            },
            |data| {
                if data.observation.report.findings.pop().is_none() {
                    return false;
                }
                data.observation.report.findings_omitted += 1;
                data.observation.report.syntax_complete = false;
                true
            },
            |duration_ms| Output {
                outcome: Outcome::Blocked {
                    error_code: Code::OutputLimitExceeded,
                    error_message: "Scanner response exceeds its fixed budget",
                    data: None,
                },
                summary: "Scanner response exceeds its fixed budget",
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
            let tool = UnsafeTool::new()?;
            let base = serde_json::from_str::<serde_json::Value>(
                r#"{"project_ref":"prj_00000000000000000000000000000001"}"#,
            )?;
            let arguments = base.as_object().cloned().ok_or("arguments")?;
            let required = tool
                .call_with_token(
                    CallToolRequestParams::new("rust.unsafe.scan")
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
                    CallToolRequestParams::new("rust.unsafe.scan").with_arguments(synchronous),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .err()
                .ok_or("runtime must be required")?;
            assert_eq!(error.message, "Scanner runtime is not configured");
            Ok::<_, Box<dyn std::error::Error>>(())
        })
    }
    #[test]
    fn operational_errors_have_closed_status_and_codes() -> Result<(), Box<dyn std::error::Error>> {
        assert_common_error_contract!(UnsafeTool::new()?, error);
        Ok(())
    }
    fn observation(
        report: rust_engineering_domain::unsafe_scan::UnsafeScanReport,
    ) -> Result<UnsafeObservation, Box<dyn std::error::Error>> {
        use super::super::security_tool::test_fixtures as fixture;
        Ok(UnsafeObservation {
            report,
            source_fingerprint: fixture::source_fingerprint('4')?,
            vendor_fingerprint: fixture::source_fingerprint('5')?,
            vendor_archive_fingerprint: fixture::source_fingerprint('6')?,
            metadata_fingerprint: fixture::source_fingerprint('7')?,
            manifest_fingerprint: fixture::source_fingerprint('8')?,
            runtime: fixture::runtime()?,
            execution_fingerprint: fixture::execution_fingerprint('3')?,
        })
    }

    #[test]
    fn result_encoding_distinguishes_complete_and_partial_syntax()
    -> Result<(), Box<dyn std::error::Error>> {
        use super::super::security_tool::test_fixtures as fixture;
        use rust_engineering_domain::unsafe_scan::{UnsafeCoverage, UnsafeScanReport};
        let tool = UnsafeTool::new()?;
        let reference = fixture::project_ref()?;
        let complete = UnsafeScanReport {
            coverage: UnsafeCoverage {
                files_total: 1,
                files_selected: 1,
                files_parsed: 1,
                workspace_files: 1,
                ..Default::default()
            },
            findings: Vec::new(),
            findings_total: 0,
            findings_omitted: 0,
            syntax_complete: true,
            cfg_evaluated: false,
            macros_expanded: false,
            generated_sources_scanned: false,
        };
        let partial = UnsafeScanReport {
            coverage: UnsafeCoverage {
                files_total: 1,
                files_selected: 1,
                files_timed_out: 1,
                workspace_files: 1,
                ..Default::default()
            },
            findings: Vec::new(),
            findings_total: 0,
            findings_omitted: 0,
            syntax_complete: false,
            cfg_evaluated: false,
            macros_expanded: false,
            generated_sources_scanned: false,
        };
        for (report, completeness, expected) in [
            (complete, ArtifactCompleteness::Complete, "passed"),
            (partial, ArtifactCompleteness::Partial, "blocked"),
        ] {
            let encoded = tool.encode_result(
                &reference,
                PublishedUnsafe {
                    observation: observation(report)?,
                    artifact: fixture::artifact(completeness)?,
                },
                7,
            )?;
            assert_eq!(
                encoded.structured_content.ok_or("content")?["status"],
                expected
            );
        }
        Ok(())
    }

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
