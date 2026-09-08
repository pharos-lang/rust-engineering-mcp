//! D19: one captured audit plus real licenses/bans/sources, owner-bound evidence.
#[allow(dead_code)]
pub(super) mod schemas;
use super::{
    HostCargoVendorConfig,
    auditing::provider::{AuditProvider, HostAuditConfig},
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
use rust_engineering_application::security::{PublishedSecurity, SecurityError, SecurityPorts};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::security::*;
use rust_engineering_domain::{
    AuditDataError, AuditObservation, Clock, ExecutionTermination, OperationalErrorCode,
    ProjectRef, RuntimeIdentity, SourceFingerprint, ToolStatus,
};
use rust_engineering_execution::RustProjectInspector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) const NAME: &str = "rust.deny";
// The advertised candidate is qualified by the source-bound M4 gate and client receipts.
const ADVERTISEMENT_READY: bool = true;
pub(super) fn advertised() -> bool {
    #[cfg(feature = "test-hooks")]
    if std::env::var_os("RUST_MCP_TEST_SECURITY_READY").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        return true;
    }
    ADVERTISEMENT_READY
}

#[derive(Clone)]
pub struct HostSecurityConfig {
    pub path: PathBuf,
    pub fingerprint: SourceFingerprint,
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
    DENY_DEFAULT_TIMEOUT_SECONDS
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    TasksRequired,
    SandboxDenied,
    ProjectNotFound,
    InvalidProject,
    SecurityPolicyInvalid,
    SecurityIncomplete,
    MissingOfflineData,
    CommandTimeout,
    OutputLimitExceeded,
    ToolNotInstalled,
    AuditSnapshotInvalid,
    ArtifactUnavailable,
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
    #[schemars(with = "schemas::ArtifactCompleteness")]
    completeness: rust_engineering_domain::ArtifactCompleteness,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Coverage {
    workspace: bool,
    normal_dependencies: bool,
    build_dependencies: bool,
    dev_dependencies: bool,
    default_features: bool,
    all_features: bool,
    target_filter: Option<String>,
    packages: u32,
    license_evidence: &'static str,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Engines {
    cargo_deny_version: &'static str,
    #[schemars(with = "schemas::Counts")]
    licenses: SecurityCounts,
    #[schemars(with = "schemas::Counts")]
    bans: SecurityCounts,
    #[schemars(with = "schemas::Counts")]
    sources: SecurityCounts,
    parse_complete: bool,
    exit_code: Option<i32>,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AuditSummary {
    #[schemars(with = "super::auditing::schemas::AuditState")]
    state: rust_engineering_domain::AuditState,
    #[schemars(with = "Option<super::auditing::schemas::AuditIssue>")]
    issue: Option<rust_engineering_domain::AuditIssue>,
    validation_complete: bool,
    lock_fingerprint: Option<String>,
    snapshot_fingerprint: Option<String>,
    #[schemars(with = "Option<super::auditing::schemas::RustSecEvidence>")]
    snapshot: Option<rust_engineering_domain::SnapshotEvidence>,
    snapshot_record_count: Option<u32>,
    snapshot_sequence: Option<u64>,
    packages_total: u32,
    crates_io_scanned: u32,
    workspace_packages_excluded: u32,
    vulnerabilities_returned: u64,
    informational_returned: u64,
    findings_omitted: u64,
}
impl From<AuditObservation> for AuditSummary {
    fn from(value: AuditObservation) -> Self {
        Self {
            state: value.state,
            issue: value.issue,
            validation_complete: value.validation_complete,
            lock_fingerprint: value.lock_fingerprint.map(|v| v.to_string()),
            snapshot_fingerprint: value.snapshot_fingerprint.map(|v| v.to_string()),
            snapshot: value.snapshot,
            snapshot_record_count: value.snapshot_record_count,
            snapshot_sequence: value.snapshot_sequence,
            packages_total: value.packages_total,
            crates_io_scanned: value.crates_io_scanned,
            workspace_packages_excluded: value.workspace_packages_excluded,
            vulnerabilities_returned: value.findings.len() as u64,
            informational_returned: value.informational.len() as u64,
            findings_omitted: value.findings_omitted,
        }
    }
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    project_ref: String,
    semantics: &'static str,
    #[schemars(with = "schemas::Completeness")]
    completeness: SecurityCompleteness,
    #[schemars(with = "schemas::PolicyState")]
    policy_state: SecurityPolicyState,
    #[schemars(with = "Vec<schemas::Finding>", length(max = 128))]
    findings: Vec<SecurityFinding>,
    findings_omitted: u64,
    audit: AuditSummary,
    engines: Engines,
    coverage: Coverage,
    source_fingerprint: String,
    vendor_fingerprint: String,
    vendor_archive_fingerprint: String,
    policy_fingerprint: String,
    deny_config_fingerprint: String,
    cargo_config_fingerprint: String,
    metadata_original_fingerprint: String,
    metadata_derived_fingerprint: String,
    lock_fingerprint: String,
    #[schemars(with = "super::inspection::schemas::RuntimeIdentity")]
    runtime: RuntimeIdentity,
    execution_fingerprint: String,
    assessed_at_utc_seconds: u64,
    #[schemars(length(min = 1, max = 1))]
    artifacts: Vec<Artifact>,
}

pub(super) struct DenyTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    runtime: Option<Runtime>,
}
pub(super) struct Runtime {
    pub(super) registry: Arc<Mutex<Registry>>,
    pub(super) workers: Workers,
    pub(super) inspector: Arc<RustProjectInspector>,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) policy: Option<HostSecurityConfig>,
    pub(super) vendor: Option<HostCargoVendorConfig>,
    pub(super) audit: Option<HostAuditConfig>,
    pub(super) publisher: Option<DurableSecurityPublisher>,
}
impl DenyTool {
    pub(super) fn new() -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition=Tool::new(NAME,"Evaluate a captured workspace with the existing RustSec audit and pinned cargo-deny licenses, bans and sources. Requires host-authenticated policy, offline vendor, approved runtime and durable evidence. Declared license strings are not license-text evidence. Findings retain exact suppressions; incomplete or stale input never passes. Auto or synchronous is supported with timeout_seconds at most 60; longer calls require negotiated MCP Tasks. The work budget excludes joined cleanup.",(*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema))
            .with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false));
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
        let options = DenyOptions::try_from(DenySelection {
            timeout_seconds: input.timeout_seconds,
        })
        .map_err(|_| ErrorData::invalid_params("Invalid tool arguments", None))?;
        // The operation budget is bounded independently from mandatory joined cleanup.
        match select_execution_mode(
            input.execution_mode.into(),
            false,
            input.timeout_seconds <= 60,
        )? {
            ExecutionSelection::Task => {
                return Err(ErrorData::internal_error(
                    "Tasks are not enabled for deny",
                    None,
                ));
            }
            ExecutionSelection::TasksRequired => {
                return self.blocked(Code::TasksRequired, "Deny requires MCP Tasks", None, 0);
            }
            ExecutionSelection::Synchronous => {}
        }
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| ErrorData::internal_error("Security runtime is not configured", None))?;
        if !runtime.ready.load(Ordering::Acquire) {
            return self.blocked(
                Code::SandboxDenied,
                "Discovery must complete before security work",
                None,
                0,
            );
        }
        let Some(policy_config) = runtime.policy.clone() else {
            return self.unavailable(
                Code::SecurityPolicyInvalid,
                "Host security policy is not configured",
                0,
            );
        };
        let Some(vendor_config) = runtime.vendor.clone() else {
            return self.unavailable(
                Code::MissingOfflineData,
                "Authenticated offline vendor is not configured",
                0,
            );
        };
        let Some(mut publisher) = runtime.publisher.clone() else {
            return self.unavailable(
                Code::ArtifactUnavailable,
                "Durable security evidence is unavailable",
                0,
            );
        };
        let started = Instant::now();
        let permit = runtime
            .workers
            .admit_job()
            .map_err(|_| ErrorData::internal_error("Security worker unavailable", None))?;
        let registry = Arc::clone(&runtime.registry);
        let inspector = Arc::clone(&runtime.inspector);
        let auditor = AuditProvider(runtime.audit.clone());
        let reference = input.project_ref.clone();
        let joined = runtime
            .workers
            .run_joined_with(
                Arc::clone(&permit),
                context.ct,
                started + Duration::from_secs(options.timeout_seconds()),
                move |control| {
                    let bytes =
                        rust_engineering_project::read_host_snapshot(&policy_config.path, control)?;
                    let policy = rust_engineering_execution::parse_security_policy(
                        &bytes,
                        &policy_config.fingerprint,
                        WallClock.now().0,
                    )
                    .map_err(|_| SecurityError::InvalidPolicy)?;
                    let vendor = rust_engineering_project::capture_with_expected(
                        &vendor_config.directory,
                        &vendor_config.fingerprint,
                        control,
                    )?;
                    registry
                        .lock()
                        .map_err(|_| SecurityError::Inspection(InspectionError::Internal))?
                        .deny_durable(
                            &reference,
                            &vendor,
                            &policy,
                            &options,
                            SecurityPorts {
                                executor: inspector.as_ref(),
                                auditor: &auditor,
                            },
                            &mut publisher,
                            &WallClock,
                            control,
                        )
                },
            )
            .await;
        permit.release_after_cleanup();
        let duration = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let result = match joined {
            Ok(value) => joined_result(value),
            Err(WorkerError::Cancelled) => Err(ProjectError::Cancelled.into()),
            Err(WorkerError::TimedOut) => Err(SecurityError::Timeout),
            Err(_) => Err(SecurityError::Inspection(InspectionError::Internal)),
        };
        match result {
            Ok(result) => self.encode_result(&input.project_ref, result, duration),
            Err(error) => self.encode_error(error, duration),
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
    fn encode_error(
        &self,
        error: SecurityError,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        use SecurityError as E;
        let (code, message) = match error {
            E::Inspection(InspectionError::Project(ProjectError::Cancelled))
            | E::Inspection(InspectionError::Execution(ExecutionError::Cancelled))
            | E::Audit(AuditDataError::Cancelled) => {
                return self.contract.encode(Output {
                    outcome: Outcome::Cancelled {
                        error_code: (),
                        error_message: (),
                        data: (),
                    },
                    summary: "Security analysis cancelled after joined cleanup",
                    duration_ms,
                });
            }
            E::Inspection(InspectionError::Execution(ExecutionError::Unavailable)) => {
                return self.unavailable(
                    Code::ToolNotInstalled,
                    "Approved security runtime is unavailable",
                    duration_ms,
                );
            }
            E::InvalidPolicy => (
                Code::SecurityPolicyInvalid,
                "Security policy is invalid, expired or overridden by project exceptions",
            ),
            E::MissingOfflineData => (
                Code::MissingOfflineData,
                "Offline dependency source is missing or invalid",
            ),
            E::Timeout
            | E::Audit(AuditDataError::Timeout)
            | E::Inspection(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::CommandTimeout,
            ))) => (
                Code::CommandTimeout,
                "Security analysis exceeded its deadline",
            ),
            E::OutputLimit
            | E::Audit(AuditDataError::Budget)
            | E::Inspection(InspectionError::OutputLimit) => (
                Code::OutputLimitExceeded,
                "Security evidence exceeded its fixed budget",
            ),
            E::Inspection(InspectionError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            ))) => (
                Code::ProjectNotFound,
                "Project authority is missing or expired",
            ),
            E::Audit(_) => (
                Code::AuditSnapshotInvalid,
                "RustSec snapshot could not be validated",
            ),
            E::Inspection(InspectionError::Execution(_)) => (
                Code::SandboxDenied,
                "Approved security execution could not be established",
            ),
            _ => (
                Code::InvalidProject,
                "Security evidence could not be completed",
            ),
        };
        self.blocked(code, message, None, duration_ms)
    }
    fn encode_result(
        &self,
        reference: &ProjectRef,
        result: PublishedSecurity,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        let observation = result.observation;
        let deny = observation.deny;
        let descriptor = result.artifact;
        descriptor
            .validate()
            .map_err(|_| ErrorData::internal_error("Invalid security artifact", None))?;
        let artifact_complete =
            descriptor.completeness == rust_engineering_domain::ArtifactCompleteness::Complete;
        let data = Box::new(Data {
            project_ref: reference.to_string(),
            semantics: "latest_known",
            completeness: observation.completeness,
            policy_state: observation.policy_state,
            findings: observation.findings,
            findings_omitted: observation.findings_omitted,
            audit: observation.audit.into(),
            engines: Engines {
                cargo_deny_version: "0.19.7",
                licenses: deny.licenses,
                bans: deny.bans,
                sources: deny.sources,
                parse_complete: deny.parse_complete,
                exit_code: deny.exit_code,
            },
            coverage: Coverage {
                workspace: true,
                normal_dependencies: true,
                build_dependencies: true,
                dev_dependencies: true,
                default_features: true,
                all_features: false,
                target_filter: None,
                packages: deny.packages.len().try_into().unwrap_or(u32::MAX),
                license_evidence: "verified_offline_source_text",
            },
            source_fingerprint: deny.source_fingerprint.to_string(),
            vendor_fingerprint: deny.vendor_fingerprint.to_string(),
            vendor_archive_fingerprint: deny.vendor_archive_fingerprint.to_string(),
            policy_fingerprint: deny.policy_fingerprint.to_string(),
            deny_config_fingerprint: deny.deny_config_fingerprint.to_string(),
            cargo_config_fingerprint: deny.cargo_config_fingerprint.to_string(),
            metadata_original_fingerprint: deny.metadata_original_fingerprint.to_string(),
            metadata_derived_fingerprint: deny.metadata_derived_fingerprint.to_string(),
            lock_fingerprint: deny.lock_fingerprint.to_string(),
            runtime: deny.runtime,
            execution_fingerprint: deny.execution_fingerprint.to_string(),
            assessed_at_utc_seconds: observation.assessed_at.0,
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
        let execution_complete =
            deny.termination == ExecutionTermination::Exited && artifact_complete;
        self.encode_bounded(data, execution_complete, duration_ms)
    }
    fn encode_bounded(
        &self,
        mut data: Box<Data>,
        execution_complete: bool,
        duration_ms: u64,
    ) -> Result<CallToolResult, ErrorData> {
        loop {
            let outcome = if data.completeness != SecurityCompleteness::Complete
                || !execution_complete
                || data.policy_state == SecurityPolicyState::Undetermined
            {
                Outcome::Blocked {
                    error_code: Code::SecurityIncomplete,
                    error_message: "Security evidence is partial; no passing verdict",
                    data: Some(data.clone()),
                }
            } else if data.policy_state == SecurityPolicyState::Violated {
                Outcome::Failed {
                    error_code: (),
                    error_message: (),
                    data: data.clone(),
                }
            } else {
                Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: data.clone(),
                }
            };
            let result=self.contract.encode(Output{outcome,summary:"Security policy evaluated against captured sources and latest-known advisory data",duration_ms})?;
            if serde_json::to_vec(&result)
                .map_err(|_| ErrorData::internal_error("Security serialization failed", None))?
                .len()
                <= 512 * 1024
            {
                return Ok(result);
            }
            if data.findings.pop().is_none() {
                return self.blocked(
                    Code::OutputLimitExceeded,
                    "Security response exceeds its fixed budget",
                    None,
                    duration_ms,
                );
            }
            data.findings_omitted = data.findings_omitted.saturating_add(1);
            data.completeness = SecurityCompleteness::Partial;
            if data.policy_state != SecurityPolicyState::Violated {
                data.policy_state = SecurityPolicyState::Undetermined;
            }
        }
    }
}

// Gateway cancellation uses a boolean, while the joined control retains whether
// its work deadline expired. Preserve cleanup failures before refining that cause.
fn joined_result<T>(joined: Joined<T, SecurityError>) -> Result<T, SecurityError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audit_summary_preserves_rustsec_provenance_in_its_schema()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::{
            FreshnessPolicy, IntegrityStatus, Provenance, SnapshotEvidence, SourceKind, UnixSeconds,
        };
        let mut audit = AuditObservation::unavailable();
        audit.snapshot = Some(SnapshotEvidence::assess(
            Provenance::new(
                SourceKind::RustsecSnapshot,
                "fixture-rustsec".parse()?,
                Some(UnixSeconds(1)),
                Some(UnixSeconds(1)),
                IntegrityStatus::Verified,
                false,
            )?,
            FreshnessPolicy::new("fixture".parse()?, 60, 120)?,
            &WallClock,
        ));
        let schema = serde_json::to_value(schemars::schema_for!(AuditSummary))?;
        let validator = jsonschema::validator_for(&schema)?;
        let mut value = serde_json::to_value(AuditSummary::from(audit))?;
        assert!(validator.is_valid(&value));
        value["snapshot"]["provenance"]["source_kind"] = serde_json::json!("project_snapshot");
        assert!(!validator.is_valid(&value));
        Ok(())
    }
    #[test]
    fn complete_mirrored_wire_budget_trims_findings_and_never_passes_omissions()
    -> Result<(), Box<dyn std::error::Error>> {
        let digest = format!("sha256:{}", "a".repeat(64));
        let fingerprint: SourceFingerprint = digest.parse()?;
        let finding = SecurityFinding {
            engine: SecurityEngine::Bans,
            rule: "banned".into(),
            package: Some(SecurityPackage {
                name: "a".repeat(64),
                version: format!("1.2.3+{}", "a".repeat(120)),
                source: SecuritySource::CratesIo,
                source_fingerprint: Some(fingerprint.clone()),
            }),
            severity: SecuritySeverity::Error,
            message: "\\\"".repeat(256),
            disposition: FindingDisposition::Suppressed(SecuritySuppression {
                id: "suppression".into(),
                engine: SecurityEngine::Bans,
                rule: "banned".into(),
                package: "a".repeat(64),
                package_source: SecuritySource::CratesIo,
                version_requirement: "=1.2.3".into(),
                reason: "\\\"".repeat(256),
                owner: "\\\"".repeat(64),
                expires_at: 2_000_000_000,
                rules_digest: fingerprint,
            }),
        };
        let data = Box::new(Data {
            project_ref: "prj_00000000000000000000000000000001".into(),
            semantics: "latest_known",
            completeness: SecurityCompleteness::Complete,
            policy_state: SecurityPolicyState::SatisfiedWithSuppressions,
            findings: vec![finding; 128],
            findings_omitted: 0,
            audit: AuditObservation::unavailable().into(),
            engines: Engines {
                cargo_deny_version: "0.19.7",
                licenses: SecurityCounts::default(),
                bans: SecurityCounts {
                    errors: 128,
                    ..Default::default()
                },
                sources: SecurityCounts::default(),
                parse_complete: true,
                exit_code: Some(2),
            },
            coverage: Coverage {
                workspace: true,
                normal_dependencies: true,
                build_dependencies: true,
                dev_dependencies: true,
                default_features: true,
                all_features: false,
                target_filter: None,
                packages: 1,
                license_evidence: "verified_offline_source_text",
            },
            source_fingerprint: digest.clone(),
            vendor_fingerprint: digest.clone(),
            vendor_archive_fingerprint: digest.clone(),
            policy_fingerprint: digest.clone(),
            deny_config_fingerprint: digest.clone(),
            cargo_config_fingerprint: digest.clone(),
            metadata_original_fingerprint: digest.clone(),
            metadata_derived_fingerprint: digest.clone(),
            lock_fingerprint: digest.clone(),
            runtime: RuntimeIdentity {
                platform: "linux/aarch64".into(),
                image_id: rust_engineering_execution::APPROVED_SECURITY_IMAGE.into(),
                configuration_fingerprint: digest.parse()?,
                execution_fingerprint: digest.parse()?,
                rust_version: "1.98.1".into(),
                cargo_version: "1.98.1".into(),
                declared_toolchain: None,
            },
            execution_fingerprint: digest.clone(),
            assessed_at_utc_seconds: 1,
            artifacts: vec![Artifact {
                uri: "rust-quality-artifact://fixture".into(),
                sha256: "a".repeat(64),
                size_bytes: 100,
                completeness: rust_engineering_domain::ArtifactCompleteness::Complete,
            }],
        });
        let tool = DenyTool::new()?;
        let result = tool.encode_bounded(data, true, 1)?;
        assert!(serde_json::to_vec(&result)?.len() <= 512 * 1024);
        let value = result.structured_content.ok_or("structured content")?;
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["data"]["completeness"], "partial");
        let emitted = value["data"]["findings"]
            .as_array()
            .ok_or("findings")?
            .len() as u64;
        let omitted = value["data"]["findings_omitted"]
            .as_u64()
            .ok_or("omissions")?;
        assert!(emitted > 0 && omitted > 0);
        assert_eq!(emitted + omitted, 128);
        assert_eq!(
            value["data"]["artifacts"][0]["uri"],
            "rust-quality-artifact://fixture"
        );
        Ok(())
    }
    #[test]
    fn work_timeout_is_distinct_from_cancel_and_cleanup_failure() {
        let cancelled = SecurityError::from(ProjectError::Cancelled);
        assert_eq!(
            joined_result::<()>(Joined {
                result: Err(cancelled),
                interrupted: Some(WorkerError::TimedOut)
            }),
            Err(SecurityError::Timeout)
        );
        assert_eq!(
            joined_result::<()>(Joined {
                result: Err(cancelled),
                interrupted: Some(WorkerError::Cancelled)
            }),
            Err(cancelled)
        );
        let cleanup = SecurityError::from(ExecutionError::CleanupUncertain);
        assert_eq!(
            joined_result::<()>(Joined {
                result: Err(cleanup),
                interrupted: Some(WorkerError::TimedOut)
            }),
            Err(cleanup)
        );
    }
    #[test]
    fn closed_contract_rejects_client_policy_paths_flags_and_budgets()
    -> Result<(), Box<dyn std::error::Error>> {
        let tool = DenyTool::new().map_err(|e| format!("{e:?}"))?;
        let valid = serde_json::json!({"project_ref":"prj_00000000000000000000000000000001"});
        assert!(tool.contract.decode(valid.as_object().cloned()).is_ok());
        for (key, value) in [
            ("policy", serde_json::json!("/tmp/evil")),
            ("flags", serde_json::json!(["--disable-fetch"])),
            ("timeout_seconds", serde_json::json!(0)),
            ("timeout_seconds", serde_json::json!(121)),
        ] {
            let mut args = valid.as_object().ok_or("object")?.clone();
            args.insert(key.into(), value);
            assert!(tool.contract.decode(Some(args)).is_err());
        }
        Ok(())
    }
}
