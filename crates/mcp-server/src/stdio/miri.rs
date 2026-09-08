//! D21: isolated interpreter evidence with the shared Tasks and Resource lifecycle.
#[allow(dead_code)]
mod schemas;
use super::{
    HostCargoVendorConfig,
    clock::WallClock,
    project::Registry,
    quality_artifacts::DurableSecurityPublisher,
    security_tool::{
        CommonFailure, SynchronousSelection, artifact_fields, capture_vendor, classify_error,
        define_fallible_security_outcome, define_security_artifact, define_security_data,
        define_security_input, define_security_response_methods, define_security_tool,
        encode_bounded, run_joined_security, synchronous_selection,
    },
    workers::Workers,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::InspectionError;
use rust_engineering_application::miri::{MiriObservation, MiriPorts, PublishedMiri};
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::miri::MiriOptions;
use rust_engineering_domain::{ArtifactCompleteness, ProjectRef};
use rust_engineering_execution::RustProjectInspector;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const NAME: &str = "rust.miri";
pub(super) fn advertised() -> bool {
    super::security_tool::advertised("RUST_MCP_TEST_MIRI_READY")
}
define_security_input!(300, 1800);
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
    ObservedFailure,
    ClassificationIntegrityUnsupported,
}
define_fallible_security_outcome!(Code, &'static str, ());
define_security_artifact!("super::deny::schemas::ArtifactCompleteness");
define_security_data!(MiriObservation, "schemas::Observation");
pub(super) struct Runtime {
    pub(super) registry: Arc<Mutex<Registry>>,
    pub(super) workers: Workers,
    pub(super) inspector: Arc<RustProjectInspector>,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) vendor: Option<HostCargoVendorConfig>,
    pub(super) publisher: Option<DurableSecurityPublisher>,
}
define_security_tool!(
    MiriTool,
    "Run library, binary and integration tests in the approved offline Miri interpreter. Reports observed UB, unsupported operations, ordinary test failures and compilation failures separately. Requires fixed nightly/sysroot and authenticated vendor. Auto or synchronous supports timeout_seconds at most 60; longer calls require negotiated MCP Tasks. The work budget excludes joined cleanup. Project proc macros, build scripts and custom harnesses are rejected to preserve diagnostic origin. Clean tests do not prove universal memory safety."
);
impl MiriTool {
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
        let options = MiriOptions::new(input.timeout_seconds)
            .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))?;
        match synchronous_selection(
            input.execution_mode,
            input.timeout_seconds <= 60,
            "Tasks are not enabled for Miri",
        )? {
            SynchronousSelection::TasksRequired => {
                return self.blocked(Code::TasksRequired, "Miri requires MCP Tasks", None, 0);
            }
            SynchronousSelection::Run => {}
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
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let reference = input.project_ref.clone();
        let (result, duration) = run_joined_security(
            &runtime.workers,
            request_token,
            options.timeout_seconds(),
            "Interpreter worker unavailable",
            move |control| {
                let vendor = capture_vendor(&vendor, control)?;
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
                return self.cancelled("Miri cancelled after joined cleanup", duration_ms);
            }
            CommonFailure::ToolNotInstalled => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved interpreter runtime is unavailable",
                    duration_ms,
                );
            }
            CommonFailure::Specific(SecurityError::ClassificationIntegrityUnsupported) => (
                Code::ClassificationIntegrityUnsupported,
                "Miri diagnostic integrity requires packages without build scripts, proc macros or custom harnesses",
            ),
            CommonFailure::Timeout => (Code::CommandTimeout, "Miri exceeded its deadline"),
            CommonFailure::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            CommonFailure::OutputLimit => (
                Code::OutputLimitExceeded,
                "Interpreter evidence exceeded its fixed budget",
            ),
            CommonFailure::ProjectNotFound => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            CommonFailure::SandboxDenied => (
                Code::SandboxDenied,
                "Approved interpreter execution could not be established",
            ),
            CommonFailure::Specific(_) => (
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
        let data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "observed_interpreter_evidence_not_a_proof_of_memory_safety",
            observation: result.observation,
            artifacts: vec![
                artifact_fields(
                    reference,
                    &descriptor,
                    "Invalid interpreter artifact descriptor",
                )?
                .into(),
            ],
        });
        encode_bounded(
            &self.contract,
            data,
            duration_ms,
            "Interpreter serialization failed",
            |data, duration_ms| Output {
                outcome: if data.observation.report.clean
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
                },
                summary: "Observed interpreter evidence; no proof of universal memory safety",
                duration_ms,
            },
            |data| {
                if data.observation.report.findings.pop().is_none() {
                    return false;
                }
                data.observation.report.findings_omitted += 1;
                data.observation.report.complete = false;
                data.observation.report.clean = false;
                true
            },
            |duration_ms| Output {
                outcome: Outcome::Blocked {
                    error_code: Code::OutputLimitExceeded,
                    error_message: "Interpreter response exceeds its fixed budget",
                    data: None,
                },
                summary: "Interpreter response exceeds its fixed budget",
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
            let tool = MiriTool::new()?;
            let base = serde_json::from_str::<serde_json::Value>(
                r#"{"project_ref":"prj_00000000000000000000000000000001"}"#,
            )?;
            let arguments = base.as_object().cloned().ok_or("arguments")?;
            let required = tool
                .call_with_token(
                    CallToolRequestParams::new("rust.miri").with_arguments(arguments.clone()),
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
                    CallToolRequestParams::new("rust.miri").with_arguments(synchronous),
                    tokio_util::sync::CancellationToken::new(),
                )
                .await
                .err()
                .ok_or("runtime must be required")?;
            assert_eq!(error.message, "Interpreter runtime is not configured");
            Ok::<_, Box<dyn std::error::Error>>(())
        })
    }
    #[test]
    fn operational_errors_have_closed_status_and_codes() -> Result<(), Box<dyn std::error::Error>> {
        assert_common_error_contract!(
            MiriTool::new()?,
            error,
            SecurityError::ClassificationIntegrityUnsupported => ("blocked", "CLASSIFICATION_INTEGRITY_UNSUPPORTED"),
        );
        Ok(())
    }
    fn observation(
        report: rust_engineering_domain::miri::MiriReport,
    ) -> Result<MiriObservation, Box<dyn std::error::Error>> {
        use super::super::security_tool::test_fixtures as fixture;
        Ok(MiriObservation {
            report,
            source_fingerprint: fixture::source_fingerprint('4')?,
            vendor_fingerprint: fixture::source_fingerprint('5')?,
            metadata_fingerprint: fixture::source_fingerprint('6')?,
            config_fingerprint: fixture::source_fingerprint('7')?,
            junit_fingerprint: Some(fixture::source_fingerprint('8')?),
            runtime: fixture::runtime()?,
            execution_fingerprint: fixture::execution_fingerprint('3')?,
            nightly_commit: "5a2be9f5f075d31e3ca5526b5b029881ce441253".into(),
            sysroot_fingerprint: fixture::source_fingerprint('9')?,
        })
    }

    #[test]
    fn result_encoding_distinguishes_clean_failure_and_partial_evidence()
    -> Result<(), Box<dyn std::error::Error>> {
        use super::super::security_tool::test_fixtures as fixture;
        use rust_engineering_domain::miri::{MiriCategory, MiriCounts, MiriFinding, MiriReport};
        let tool = MiriTool::new()?;
        let reference = fixture::project_ref()?;
        let clean = MiriReport {
            counts: MiriCounts {
                tests: 1,
                passed: 1,
                ..Default::default()
            },
            findings: Vec::new(),
            findings_omitted: 0,
            complete: true,
            clean: true,
            junit_present: true,
            exit_code: Some(0),
        };
        let failed = MiriReport {
            counts: MiriCounts {
                tests: 1,
                failed: 1,
                test_failures: 1,
                ..Default::default()
            },
            findings: vec![MiriFinding {
                category: MiriCategory::TestFailure,
                test_name: Some("case".into()),
                test_binary: Some("suite".into()),
            }],
            findings_omitted: 0,
            complete: true,
            clean: false,
            junit_present: true,
            exit_code: Some(101),
        };
        let partial = MiriReport {
            counts: MiriCounts::default(),
            findings: Vec::new(),
            findings_omitted: 0,
            complete: false,
            clean: false,
            junit_present: false,
            exit_code: None,
        };
        for (report, completeness, expected) in [
            (clean, ArtifactCompleteness::Complete, "passed"),
            (failed, ArtifactCompleteness::Complete, "failed"),
            (partial, ArtifactCompleteness::Partial, "blocked"),
        ] {
            let encoded = tool.encode_result(
                &reference,
                PublishedMiri {
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
