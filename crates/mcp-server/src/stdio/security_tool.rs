//! Shared MCP boundary mechanics for the M4 security tools.

use super::{
    HostCargoVendorConfig,
    clock::WallClock,
    contract::{Contract, ToolOutput},
    deny::HostSecurityConfig,
    nextest::{ExecutionModeDto, ExecutionSelection, select_execution_mode},
    workers::{Control, Joined, WorkerError, Workers},
};
use rmcp::model::{CallToolResult, ErrorData};
use rust_engineering_application::job::JobPermit;
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::{
    AuditDataError, CargoVendorSnapshot, Clock, OperationalErrorCode, ProjectRef,
    QualityArtifactDescriptor, security::SecurityPolicy,
};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use std::time::Instant;
use tokio_util::sync::CancellationToken;

pub(super) const MAX_RESULT_BYTES: usize = 512 * 1024;
pub(super) const ARTIFACT_PAGE_BYTES: u64 = 320 * 1024;

pub(super) fn advertised(_test_variable: &str) -> bool {
    #[cfg(feature = "test-hooks")]
    if std::env::var_os(_test_variable).as_deref() == Some(std::ffi::OsStr::new("1")) {
        return true;
    }
    true
}

pub(super) enum SynchronousSelection {
    Run,
    TasksRequired,
}

pub(super) fn synchronous_selection(
    mode: ExecutionModeDto,
    synchronous_allowed: bool,
    task_error: &'static str,
) -> Result<SynchronousSelection, ErrorData> {
    match select_execution_mode(mode.into(), false, synchronous_allowed)? {
        ExecutionSelection::Task => Err(ErrorData::internal_error(task_error, None)),
        ExecutionSelection::TasksRequired => Ok(SynchronousSelection::TasksRequired),
        ExecutionSelection::Synchronous => Ok(SynchronousSelection::Run),
    }
}

pub(super) fn capture_vendor(
    config: &HostCargoVendorConfig,
    control: &Control,
) -> Result<CargoVendorSnapshot, SecurityError> {
    Ok(rust_engineering_project::capture_with_expected(
        &config.directory,
        &config.fingerprint,
        control,
    )?)
}

pub(super) fn load_policy(
    config: &HostSecurityConfig,
    control: &Control,
) -> Result<SecurityPolicy, SecurityError> {
    let bytes = rust_engineering_project::read_host_snapshot(&config.path, control)?;
    rust_engineering_execution::parse_security_policy(
        &bytes,
        &config.fingerprint,
        WallClock.now().0,
    )
    .map_err(|_| SecurityError::InvalidPolicy)
}

pub(super) async fn run_joined_security<T, F>(
    workers: &Workers,
    request: CancellationToken,
    timeout_seconds: u64,
    worker_error: &'static str,
    work: F,
) -> Result<(Result<T, SecurityError>, u64), ErrorData>
where
    T: Send + 'static,
    F: FnOnce(&Control) -> Result<T, SecurityError> + Send + 'static,
{
    let started = Instant::now();
    let permit = workers
        .admit_job()
        .map_err(|_| ErrorData::internal_error(worker_error, None))?;
    let joined = workers
        .run_joined_with(
            std::sync::Arc::clone(&permit),
            request,
            started + std::time::Duration::from_secs(timeout_seconds),
            work,
        )
        .await;
    permit.release_after_cleanup();
    let duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    let result = match joined {
        Ok(value) => joined_result(value),
        Err(WorkerError::Cancelled) => Err(ProjectError::Cancelled.into()),
        Err(WorkerError::TimedOut) => Err(SecurityError::Timeout),
        Err(_) => Err(SecurityError::Inspection(InspectionError::Internal)),
    };
    Ok((result, duration_ms))
}

// Cleanup failures retain precedence over the request/deadline signal observed
// after work. Audit cancellation is included for deny and harmless elsewhere.
pub(super) fn joined_result<T>(joined: Joined<T, SecurityError>) -> Result<T, SecurityError> {
    let cancelled = matches!(
        joined.result,
        Err(SecurityError::Inspection(InspectionError::Project(
            ProjectError::Cancelled
        ))) | Err(SecurityError::Inspection(InspectionError::Execution(
            ExecutionError::Cancelled
        ))) | Err(SecurityError::Audit(AuditDataError::Cancelled))
    );
    if cancelled && joined.interrupted == Some(WorkerError::TimedOut) {
        return Err(SecurityError::Timeout);
    }
    match (joined.result, joined.interrupted) {
        (Err(error), _) => Err(error),
        (Ok(value), None) => Ok(value),
        (Ok(_), Some(WorkerError::TimedOut)) => Err(SecurityError::Timeout),
        (Ok(_), Some(WorkerError::Cancelled)) => Err(ProjectError::Cancelled.into()),
        _ => Err(SecurityError::Inspection(InspectionError::Internal)),
    }
}

pub(super) enum CommonFailure {
    Cancelled,
    ToolNotInstalled,
    Timeout,
    MissingOfflineData,
    OutputLimit,
    ProjectNotFound,
    SandboxDenied,
    Specific(SecurityError),
}

pub(super) fn classify_error(error: SecurityError) -> CommonFailure {
    match error {
        SecurityError::Inspection(
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled),
        )
        | SecurityError::Audit(AuditDataError::Cancelled) => CommonFailure::Cancelled,
        SecurityError::Inspection(InspectionError::Execution(ExecutionError::Unavailable)) => {
            CommonFailure::ToolNotInstalled
        }
        SecurityError::Timeout
        | SecurityError::Audit(AuditDataError::Timeout)
        | SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
            OperationalErrorCode::CommandTimeout,
        ))) => CommonFailure::Timeout,
        SecurityError::MissingOfflineData => CommonFailure::MissingOfflineData,
        SecurityError::OutputLimit
        | SecurityError::Audit(AuditDataError::Budget)
        | SecurityError::Inspection(InspectionError::OutputLimit) => CommonFailure::OutputLimit,
        SecurityError::Inspection(InspectionError::Project(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound,
        ))) => CommonFailure::ProjectNotFound,
        SecurityError::Inspection(InspectionError::Execution(_)) => CommonFailure::SandboxDenied,
        other => CommonFailure::Specific(other),
    }
}

pub(super) fn encode_bounded<I, O, D>(
    contract: &Contract<I, O>,
    mut data: Box<D>,
    duration_ms: u64,
    serialization_error: &'static str,
    mut output: impl FnMut(&Box<D>, u64) -> O,
    mut trim: impl FnMut(&mut D) -> bool,
    exhausted: impl FnOnce(u64) -> O,
) -> Result<CallToolResult, ErrorData>
where
    I: DeserializeOwned + JsonSchema,
    O: ToolOutput,
{
    loop {
        let result = contract.encode(output(&data, duration_ms))?;
        if serde_json::to_vec(&result)
            .map_err(|_| ErrorData::internal_error(serialization_error, None))?
            .len()
            <= MAX_RESULT_BYTES
        {
            return Ok(result);
        }
        if !trim(data.as_mut()) {
            return contract.encode(exhausted(duration_ms));
        }
    }
}

pub(super) struct ArtifactFields {
    pub(super) uri: String,
    pub(super) sha256: String,
    pub(super) size_bytes: u64,
    pub(super) completeness: rust_engineering_domain::ArtifactCompleteness,
}

pub(super) fn artifact_fields(
    reference: &ProjectRef,
    descriptor: &QualityArtifactDescriptor,
    invalid_error: &'static str,
) -> Result<ArtifactFields, ErrorData> {
    descriptor
        .validate()
        .map_err(|_| ErrorData::internal_error(invalid_error, None))?;
    Ok(ArtifactFields {
        uri: format!(
            "rust-quality-artifact://{reference}/{}?offset=0&length={}",
            descriptor.artifact_id,
            descriptor.size_bytes.min(ARTIFACT_PAGE_BYTES)
        ),
        sha256: super::resources::hex(&descriptor.sha256),
        size_bytes: descriptor.size_bytes,
        completeness: descriptor.completeness,
    })
}

macro_rules! define_security_input {
    ($default:expr, $maximum:literal) => {
        #[derive(Clone, serde::Deserialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct Input {
            #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
            project_ref: rust_engineering_domain::ProjectRef,
            #[serde(default = "default_timeout")]
            #[schemars(range(min = 1, max = $maximum))]
            timeout_seconds: u64,
            #[serde(default)]
            execution_mode: super::nextest::ExecutionModeDto,
        }
        fn default_timeout() -> u64 {
            $default
        }
    };
}
pub(super) use define_security_input;

macro_rules! define_security_output {
    ($($pattern:pat => $status:expr),+ $(,)?) => {
        #[derive(Clone, serde::Serialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct Output {
            #[serde(flatten)]
            outcome: Outcome,
            summary: &'static str,
            duration_ms: u64,
        }
        impl super::contract::ToolOutput for Output {
            fn status(&self) -> rust_engineering_domain::ToolStatus {
                match self.outcome {
                    $($pattern => $status),+
                }
            }
        }
    };
}
pub(super) use define_security_output;

macro_rules! define_security_outcome {
    ($unavailable_data:ty) => {
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
                data: $unavailable_data,
            },
            Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
        }
        super::security_tool::define_security_output!(
            Outcome::Passed { .. } => rust_engineering_domain::ToolStatus::Passed,
            Outcome::Blocked { .. } => rust_engineering_domain::ToolStatus::Blocked,
            Outcome::Unavailable { .. } => rust_engineering_domain::ToolStatus::Unavailable,
            Outcome::Cancelled { .. } => rust_engineering_domain::ToolStatus::Cancelled,
        );
    };
}
pub(super) use define_security_outcome;

macro_rules! define_fallible_security_outcome {
    ($failed_code:ty, $failed_message:ty, $unavailable_data:ty) => {
        #[derive(Clone, serde::Serialize, schemars::JsonSchema)]
        #[serde(tag = "status", rename_all = "snake_case")]
        enum Outcome {
            Passed {
                error_code: (),
                error_message: (),
                data: Box<Data>,
            },
            Failed {
                error_code: $failed_code,
                error_message: $failed_message,
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
                data: $unavailable_data,
            },
            Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
        }
        super::security_tool::define_security_output!(
            Outcome::Passed { .. } => rust_engineering_domain::ToolStatus::Passed,
            Outcome::Failed { .. } => rust_engineering_domain::ToolStatus::Failed,
            Outcome::Blocked { .. } => rust_engineering_domain::ToolStatus::Blocked,
            Outcome::Unavailable { .. } => rust_engineering_domain::ToolStatus::Unavailable,
            Outcome::Cancelled { .. } => rust_engineering_domain::ToolStatus::Cancelled,
        );
    };
}
pub(super) use define_fallible_security_outcome;

macro_rules! define_security_response_methods {
    ($receiver:ident, $unavailable_data:expr) => {
        fn blocked(
            &$receiver,
            code: Code,
            message: &'static str,
            data: Option<Box<Data>>,
            duration_ms: u64,
        ) -> Result<rmcp::model::CallToolResult, rmcp::model::ErrorData> {
            $receiver.contract.encode(Output {
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
            &$receiver,
            code: Code,
            message: &'static str,
            duration_ms: u64,
        ) -> Result<rmcp::model::CallToolResult, rmcp::model::ErrorData> {
            $receiver.contract.encode(Output {
                outcome: Outcome::Unavailable {
                    error_code: code,
                    error_message: message,
                    data: $unavailable_data,
                },
                summary: message,
                duration_ms,
            })
        }

        fn cancelled(
            &$receiver,
            summary: &'static str,
            duration_ms: u64,
        ) -> Result<rmcp::model::CallToolResult, rmcp::model::ErrorData> {
            $receiver.contract.encode(Output {
                outcome: Outcome::Cancelled {
                    error_code: (),
                    error_message: (),
                    data: (),
                },
                summary,
                duration_ms,
            })
        }
    };
}
pub(super) use define_security_response_methods;

macro_rules! define_security_artifact {
    ($completeness_schema:literal) => {
        #[derive(Clone, serde::Serialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct Artifact {
            uri: String,
            sha256: String,
            size_bytes: u64,
            #[schemars(with = $completeness_schema)]
            completeness: rust_engineering_domain::ArtifactCompleteness,
        }
        impl From<super::security_tool::ArtifactFields> for Artifact {
            fn from(value: super::security_tool::ArtifactFields) -> Self {
                Self {
                    uri: value.uri,
                    sha256: value.sha256,
                    size_bytes: value.size_bytes,
                    completeness: value.completeness,
                }
            }
        }
    };
}
pub(super) use define_security_artifact;

macro_rules! define_security_data {
    ($observation:ty, $observation_schema:literal) => {
        #[derive(Clone, serde::Serialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        struct Data {
            project_ref: String,
            semantics: &'static str,
            #[schemars(with = $observation_schema)]
            observation: $observation,
            #[schemars(length(min = 1, max = 1))]
            artifacts: Vec<Artifact>,
        }
    };
}
pub(super) use define_security_data;

macro_rules! define_security_tool {
    ($tool:ident, $description:literal) => {
        pub(super) struct $tool {
            pub(super) definition: rmcp::model::Tool,
            contract: super::contract::Contract<Input, Output>,
            runtime: Option<Runtime>,
        }
        impl $tool {
            pub(super) fn new() -> Result<Self, rmcp::model::ErrorData> {
                let contract = super::contract::Contract::<Input, Output>::new()?;
                let definition =
                    rmcp::model::Tool::new(NAME, $description, (*contract.input_schema).clone())
                        .with_raw_output_schema(std::sync::Arc::clone(&contract.output_schema))
                        .with_annotations(
                            rmcp::model::ToolAnnotations::new()
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
            pub(super) fn with_runtime(mut self, runtime: Runtime) -> Self {
                self.runtime = Some(runtime);
                self
            }
        }
    };
}
pub(super) use define_security_tool;

#[cfg(test)]
macro_rules! assert_common_error_contract {
    ($tool:expr, $method:ident $(, $specific:expr => ($specific_status:literal, $specific_code:literal))* $(,)?) => {{
        let tool = $tool;
        let cases = [
            (
                rust_engineering_application::security::SecurityError::from(
                    rust_engineering_application::ProjectError::Cancelled,
                ),
                "cancelled",
                serde_json::Value::Null,
            ),
            (
                rust_engineering_application::security::SecurityError::Inspection(
                    rust_engineering_application::InspectionError::Execution(
                        rust_engineering_application::ExecutionError::Unavailable,
                    ),
                ),
                "unavailable",
                serde_json::json!("TOOL_NOT_INSTALLED"),
            ),
            (
                rust_engineering_application::security::SecurityError::Timeout,
                "blocked",
                serde_json::json!("COMMAND_TIMEOUT"),
            ),
            (
                rust_engineering_application::security::SecurityError::MissingOfflineData,
                "blocked",
                serde_json::json!("MISSING_OFFLINE_DATA"),
            ),
            (
                rust_engineering_application::security::SecurityError::OutputLimit,
                "blocked",
                serde_json::json!("OUTPUT_LIMIT_EXCEEDED"),
            ),
            (
                rust_engineering_application::security::SecurityError::Inspection(
                    rust_engineering_application::InspectionError::Project(
                        rust_engineering_application::ProjectError::Rejected(
                            rust_engineering_domain::OperationalErrorCode::ProjectNotFound,
                        ),
                    ),
                ),
                "blocked",
                serde_json::json!("PROJECT_NOT_FOUND"),
            ),
            (
                rust_engineering_application::security::SecurityError::Inspection(
                    rust_engineering_application::InspectionError::Execution(
                        rust_engineering_application::ExecutionError::InvalidConfiguration,
                    ),
                ),
                "blocked",
                serde_json::json!("SANDBOX_DENIED"),
            ),
            (
                rust_engineering_application::security::SecurityError::InvalidMetadata,
                "blocked",
                serde_json::json!("INVALID_PROJECT"),
            ),
            $(($specific, $specific_status, serde_json::json!($specific_code)),)*
        ];
        for (error, status, code) in cases {
            let result = tool.$method(error, 7)?;
            let value = result.structured_content.ok_or("structured content")?;
            assert_eq!(value["status"], status);
            assert_eq!(value["error_code"], code);
            assert_eq!(value["duration_ms"], 7);
        }
    }};
}
#[cfg(test)]
pub(super) use assert_common_error_contract;

#[cfg(test)]
pub(super) mod test_fixtures {
    use rust_engineering_domain::{
        ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection,
        ArtifactSensitivity, ArtifactSource, ExecutionFingerprint, GuestArtifactName,
        PayloadFormatVersion, PluginIdentity, ProjectRef, QualityArtifactDescriptor,
        QualityArtifactDraft, QualityArtifactId, QualityArtifactKind, QualityJobId,
        QualityMimeType, RuntimeIdentity, SourceFingerprint, UtcInstant,
    };

    pub(in crate::stdio) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    pub(in crate::stdio) fn project_ref() -> Result<ProjectRef> {
        Ok("prj_00000000000000000000000000000001".parse()?)
    }

    pub(in crate::stdio) fn source_fingerprint(digit: char) -> Result<SourceFingerprint> {
        Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
    }

    pub(in crate::stdio) fn execution_fingerprint(digit: char) -> Result<ExecutionFingerprint> {
        Ok(format!("sha256:{}", digit.to_string().repeat(64)).parse()?)
    }

    pub(in crate::stdio) fn runtime() -> Result<RuntimeIdentity> {
        Ok(RuntimeIdentity {
            platform: "linux/aarch64".into(),
            image_id: format!("sha256:{}", "1".repeat(64)),
            configuration_fingerprint: execution_fingerprint('2')?,
            execution_fingerprint: execution_fingerprint('3')?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        })
    }

    pub(in crate::stdio) fn artifact(
        completeness: ArtifactCompleteness,
    ) -> Result<QualityArtifactDescriptor> {
        let created = UtcInstant::from_unix_seconds(1_788_000_000)?;
        Ok(QualityArtifactDraft {
            artifact_id: QualityArtifactId::from_random_bytes([1; 16]),
            member_index: 0,
            kind: QualityArtifactKind::ToolLog,
            mime_type: QualityMimeType::TextPlain,
            payload_format_version: PayloadFormatVersion::Utf8LogV1,
            completeness,
            sensitivity: ArtifactSensitivity::PotentiallySensitive,
            created_at_utc: created.clone(),
            expires_at_utc: created.checked_add_seconds(60)?,
            source: ArtifactSource {
                captured_source_sha256: [2; 32],
                guest_name: GuestArtifactName::ToolLog,
                selection: ArtifactSelection::Workspace,
            },
            runtime: ArtifactRuntime {
                image_digest: [3; 32],
                toolchain_identity: [4; 32],
                plugin: ArtifactPlugin {
                    identity: PluginIdentity::Builtin,
                    version: 1,
                    digest: [5; 32],
                },
                implementation_digest: [6; 32],
            },
        }
        .into_descriptor(
            QualityJobId::from_random_bytes([7; 16]),
            [8; 32],
            [9; 32],
            512,
        )?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
    }

    #[test]
    fn synchronous_selection_covers_direct_task_and_required_paths()
    -> Result<(), Box<dyn std::error::Error>> {
        assert!(matches!(
            synchronous_selection(ExecutionModeDto::Auto, true, "task")?,
            SynchronousSelection::Run
        ));
        assert!(matches!(
            synchronous_selection(ExecutionModeDto::Auto, false, "task")?,
            SynchronousSelection::TasksRequired
        ));
        assert!(synchronous_selection(ExecutionModeDto::Synchronous, false, "task").is_err());
        assert!(synchronous_selection(ExecutionModeDto::Task, true, "task").is_err());
        runtime()?.block_on(async {
            super::super::workers::with_negotiated_tasks(true, async {
                let error = synchronous_selection(ExecutionModeDto::Task, true, "task")
                    .err()
                    .ok_or("task selection must materialize through the outer handler")?;
                assert_eq!(error.message, "task");
                Ok::<_, Box<dyn std::error::Error>>(())
            })
            .await?;
            Ok::<_, Box<dyn std::error::Error>>(())
        })?;
        Ok(())
    }

    #[test]
    fn joined_security_execution_preserves_result_and_interrupt_cause()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let (result, _) =
                run_joined_security(&workers, CancellationToken::new(), 1, "worker", |_| {
                    Ok(7_u8)
                })
                .await?;
            assert_eq!(result, Ok(7));

            let (result, _) =
                run_joined_security(&workers, CancellationToken::new(), 1, "worker", |_| {
                    Err::<(), _>(SecurityError::InvalidMetadata)
                })
                .await?;
            assert_eq!(result, Err(SecurityError::InvalidMetadata));

            let cancelled = CancellationToken::new();
            cancelled.cancel();
            let (result, _) =
                run_joined_security(&workers, cancelled, 1, "worker", |_| Ok(())).await?;
            assert_eq!(result, Err(ProjectError::Cancelled.into()));

            let (result, _) =
                run_joined_security(&workers, CancellationToken::new(), 0, "worker", |_| Ok(()))
                    .await?;
            assert_eq!(result, Err(SecurityError::Timeout));
            Ok::<_, ErrorData>(())
        })?;
        Ok(())
    }

    #[test]
    fn joined_result_never_turns_cleanup_interruption_into_success() {
        assert_eq!(
            joined_result(Joined {
                result: Ok(1_u8),
                interrupted: Some(WorkerError::TimedOut),
            }),
            Err(SecurityError::Timeout),
        );
        assert_eq!(
            joined_result(Joined {
                result: Ok(1_u8),
                interrupted: Some(WorkerError::Cancelled),
            }),
            Err(ProjectError::Cancelled.into()),
        );
        assert_eq!(
            joined_result(Joined {
                result: Ok(1_u8),
                interrupted: Some(WorkerError::Internal),
            }),
            Err(SecurityError::Inspection(InspectionError::Internal)),
        );
    }
}
