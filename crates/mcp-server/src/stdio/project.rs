use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rmcp::model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations};
use rmcp::service::{RequestContext, RoleServer};
use rust_engineering_application::{OpenedProject, ProjectError, ProjectRegistry};
use rust_engineering_domain::OperationalErrorCode;
use rust_engineering_project::{MonotonicClock, OsReferences, SecureProjects};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::contract::{Contract, ToolOutput};
use super::workers::{WorkerError, Workers};

pub const NAME: &str = "rust.project.open";
const DEADLINE: Duration = Duration::from_secs(10);
pub(super) type Registry = ProjectRegistry<SecureProjects, OsReferences, MonotonicClock>;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OpenInput {
    #[schemars(length(min = 1, max = 4096))]
    path: String,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OpenData {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    #[schemars(length(min = 1, max = 4096))]
    workspace_root: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    fingerprint: String,
    validation: ValidationKind,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ValidationKind {
    Structural,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum BlockedCode {
    ProjectNotFound,
    InvalidProject,
    LockfileUpdateRequired,
    CommandTimeout,
    SandboxDenied,
    NetworkDenied,
    OutputLimitExceeded,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum UnavailableCode {
    ToolNotInstalled,
    UnsupportedPlatform,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum Outcome {
    Passed {
        error_code: (),
        error_message: (),
        data: OpenData,
    },
    Blocked {
        error_code: BlockedCode,
        error_message: String,
        data: (),
    },
    Unavailable {
        error_code: UnavailableCode,
        error_message: String,
        data: (),
    },
    Cancelled {
        error_code: (),
        error_message: (),
        data: (),
    },
}

#[derive(Default, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Truncation {
    stdout_truncated: bool,
    stderr_truncated: bool,
    diagnostics_omitted: u64,
}

#[derive(Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum LocalEvidence {
    Local,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OpenOutput {
    #[serde(flatten)]
    outcome: Outcome,
    summary: String,
    duration_ms: u64,
    diagnostics: [(); 0],
    truncation: Truncation,
    evidence: LocalEvidence,
}

impl ToolOutput for OpenOutput {
    fn status(&self) -> rust_engineering_domain::ToolStatus {
        use rust_engineering_domain::ToolStatus;
        match self.outcome {
            Outcome::Passed { .. } => ToolStatus::Passed,
            Outcome::Blocked { .. } => ToolStatus::Blocked,
            Outcome::Unavailable { .. } => ToolStatus::Unavailable,
            Outcome::Cancelled { .. } => ToolStatus::Cancelled,
        }
    }
}

fn failure(code: OperationalErrorCode) -> (Outcome, &'static str) {
    let message = match code {
        OperationalErrorCode::ProjectNotFound => "Project or registered reference was not found",
        OperationalErrorCode::InvalidProject => "Project structure is invalid or unsupported",
        OperationalErrorCode::ToolNotInstalled => "Required tooling is unavailable",
        OperationalErrorCode::LockfileUpdateRequired => "Lockfile update is required",
        OperationalErrorCode::CommandTimeout => "Project validation exceeded its deadline",
        OperationalErrorCode::SandboxDenied => {
            "Project access or server resource policy denied the operation"
        }
        OperationalErrorCode::NetworkDenied => "Network access is denied",
        OperationalErrorCode::UnsupportedPlatform => {
            "Secure project access is unavailable on this platform or filesystem"
        }
        OperationalErrorCode::OutputLimitExceeded => "Project metadata exceeds server limits",
    };
    let error_message = message.to_owned();
    let blocked = match code {
        OperationalErrorCode::ToolNotInstalled => {
            return (
                Outcome::Unavailable {
                    error_code: UnavailableCode::ToolNotInstalled,
                    error_message,
                    data: (),
                },
                message,
            );
        }
        OperationalErrorCode::UnsupportedPlatform => {
            return (
                Outcome::Unavailable {
                    error_code: UnavailableCode::UnsupportedPlatform,
                    error_message,
                    data: (),
                },
                message,
            );
        }
        OperationalErrorCode::ProjectNotFound => BlockedCode::ProjectNotFound,
        OperationalErrorCode::InvalidProject => BlockedCode::InvalidProject,
        OperationalErrorCode::LockfileUpdateRequired => BlockedCode::LockfileUpdateRequired,
        OperationalErrorCode::CommandTimeout => BlockedCode::CommandTimeout,
        OperationalErrorCode::SandboxDenied => BlockedCode::SandboxDenied,
        OperationalErrorCode::NetworkDenied => BlockedCode::NetworkDenied,
        OperationalErrorCode::OutputLimitExceeded => BlockedCode::OutputLimitExceeded,
    };
    (
        Outcome::Blocked {
            error_code: blocked,
            error_message,
            data: (),
        },
        message,
    )
}

fn output(
    result: Result<OpenedProject, ProjectError>,
    duration_ms: u64,
) -> Result<OpenOutput, ErrorData> {
    let (outcome, summary) = match result {
        Ok(project) => (
            Outcome::Passed {
                error_code: (),
                error_message: (),
                data: OpenData {
                    project_ref: project.project_ref.to_string(),
                    workspace_root: project.identity.workspace_root,
                    fingerprint: project.identity.fingerprint.to_string(),
                    validation: ValidationKind::Structural,
                },
            },
            "Project structurally validated and registered; Cargo was not executed",
        ),
        Err(ProjectError::Rejected(code)) => failure(code),
        Err(ProjectError::Cancelled) => (
            Outcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Project registration cancelled",
        ),
        Err(ProjectError::Internal) => {
            return Err(ErrorData::internal_error(
                "Project registration failed",
                None,
            ));
        }
    };
    Ok(OpenOutput {
        outcome,
        summary: summary.to_owned(),
        duration_ms,
        diagnostics: [],
        truncation: Truncation::default(),
        evidence: LocalEvidence::Local,
    })
}

pub(super) struct ProjectTool {
    pub(super) definition: Tool,
    contract: Contract<OpenInput, OpenOutput>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
}

impl ProjectTool {
    pub(super) fn registry(&self) -> Arc<Mutex<Registry>> {
        Arc::clone(&self.registry)
    }

    pub(super) fn new(
        backend: SecureProjects,
        ttl_seconds: u64,
        workers: Workers,
    ) -> Result<Self, ProjectError> {
        let contract =
            Contract::<OpenInput, OpenOutput>::new().map_err(|_| ProjectError::Internal)?;
        let definition = Tool::new(NAME,
            "Register an explicitly selected Rust package/workspace root authorized by the host. Reads bounded manifests and validates structural membership/path dependencies without running Cargo or project code. Returns a process-local opaque reference; no compilation or dependency resolution is certified.",
            (*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema))
            .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false));
        Ok(Self {
            definition,
            contract,
            registry: Arc::new(Mutex::new(ProjectRegistry::new(
                backend,
                OsReferences,
                MonotonicClock::default(),
                ttl_seconds,
                64,
            )?)),
            workers,
        })
    }

    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        if request.name != NAME {
            return Err(ErrorData::new(
                rmcp::model::ErrorCode::METHOD_NOT_FOUND,
                "Unknown tool",
                None,
            ));
        }
        let input = self.contract.decode(request.arguments)?;
        let started = Instant::now();
        let registry = Arc::clone(&self.registry);
        let result = match self
            .workers
            .run(context.ct, started + DEADLINE, move |control| {
                registry
                    .lock()
                    .map_err(|_| ProjectError::Internal)?
                    .open(&input.path, control)
            })
            .await
        {
            Ok(result) => result,
            Err(WorkerError::Busy) => {
                Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied))
            }
            Err(WorkerError::TimedOut) => {
                Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
            }
            Err(WorkerError::Cancelled) => Err(ProjectError::Cancelled),
            Err(WorkerError::Internal) => Err(ProjectError::Internal),
        };
        let result = output(
            result,
            started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        )?;
        self.contract.encode(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_application::ProjectIdentity;

    #[test]
    fn every_operational_code_and_cancellation_use_error_content()
    -> Result<(), Box<dyn std::error::Error>> {
        let contract = Contract::<OpenInput, OpenOutput>::new()?;
        let codes = [
            OperationalErrorCode::ProjectNotFound,
            OperationalErrorCode::InvalidProject,
            OperationalErrorCode::ToolNotInstalled,
            OperationalErrorCode::LockfileUpdateRequired,
            OperationalErrorCode::CommandTimeout,
            OperationalErrorCode::SandboxDenied,
            OperationalErrorCode::NetworkDenied,
            OperationalErrorCode::UnsupportedPlatform,
            OperationalErrorCode::OutputLimitExceeded,
        ];
        for error in codes
            .into_iter()
            .map(ProjectError::Rejected)
            .chain([ProjectError::Cancelled])
        {
            let result = serde_json::to_value(contract.encode(output(Err(error), 3)?)?)?;
            assert_eq!(result["isError"], true);
            let content = &result["structuredContent"];
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    result["content"][0]["text"]
                        .as_str()
                        .ok_or("missing text")?
                )?,
                *content
            );
            for field in [
                "status",
                "error_code",
                "error_message",
                "summary",
                "data",
                "evidence",
                "diagnostics",
                "truncation",
                "duration_ms",
            ] {
                let mut missing = content.clone();
                missing.as_object_mut().ok_or("not object")?.remove(field);
                let validator =
                    jsonschema::validator_for(&serde_json::to_value(&contract.output_schema)?)?;
                assert!(!validator.is_valid(&missing), "accepted missing {field}");
            }
        }
        assert!(output(Err(ProjectError::Internal), 0).is_err());
        Ok(())
    }

    #[test]
    fn all_outcomes_serialize_to_their_closed_runtime_schema() -> Result<(), String> {
        let schema = serde_json::to_value(schemars::schema_for!(OpenOutput))
            .map_err(|error| error.to_string())?;
        let validator = jsonschema::validator_for(&schema).map_err(|error| error.to_string())?;
        let project = OpenedProject {
            project_ref: format!("prj_{}", "a".repeat(32))
                .parse()
                .map_err(|error| format!("{error:?}"))?,
            identity: ProjectIdentity {
                workspace_root: "/root".to_owned(),
                fingerprint: format!("sha256:{}", "b".repeat(64))
                    .parse()
                    .map_err(|error| format!("{error:?}"))?,
            },
        };
        let cases = [
            Ok(project),
            Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject)),
            Err(ProjectError::Rejected(
                OperationalErrorCode::UnsupportedPlatform,
            )),
            Err(ProjectError::Cancelled),
        ];
        for result in cases {
            let dto = output(result, 1).map_err(|error| error.to_string())?;
            let value = serde_json::to_value(dto).map_err(|error| error.to_string())?;
            validator
                .validate(&value)
                .map_err(|error| error.to_string())?;
            let mut extra = value.clone();
            extra["unrecognized"] = serde_json::json!(true);
            assert!(!validator.is_valid(&extra));
            let mut inconsistent = value;
            inconsistent["error_code"] = serde_json::json!("UNKNOWN_CODE");
            assert!(!validator.is_valid(&inconsistent));
        }
        Ok(())
    }
}
