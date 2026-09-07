//! A single admitted quality run publishes all required stages and retained repair facts.
#[allow(dead_code)]
mod schemas;
use super::clock::WallClock;
use super::workers::worker_error;
use super::{
    auditing::provider::{AuditProvider, HostAuditConfig},
    contract::{Contract, ToolOutput},
    project::Registry,
    resources::{self, ArtifactClock, Store},
    workers::{Joined, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError, QualityPorts};
use rust_engineering_domain::{
    AuditObservation, AuditState, CheckObservation, Diagnostic, Evidence, ExecutionTermination,
    OperationalErrorCode, ProjectQualityGate, ProjectRef, QualityIssue, QualityObservation,
    QualityProfile, QualityStage, QualityStageReport, RuntimeIdentity, ToolStatus,
    quality_runtime_matches, quality_status,
};
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
pub(super) const NAME: &str = "rust.quality.gate";
const DEADLINE: Duration = Duration::from_secs(240);
const MAX_RESULT: usize = 512 * 1024;
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    #[schemars(with = "schemas::QualityProfile")]
    profile: QualityProfile,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Semantics {
    LatestKnown,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AppliedSelection {
    FormatAll,
    CheckCargoDefaults,
    ClippyStrictCargoDefaults,
    #[serde(rename = "test_cargo_defaults_30_seconds")]
    TestCargoDefaults30Seconds,
    AuditCapturedLockfile,
}
impl From<QualityStage> for AppliedSelection {
    fn from(stage: QualityStage) -> Self {
        match stage {
            QualityStage::Format => Self::FormatAll,
            QualityStage::Check => Self::CheckCargoDefaults,
            QualityStage::Clippy => Self::ClippyStrictCargoDefaults,
            QualityStage::Test => Self::TestCargoDefaults30Seconds,
            QualityStage::Audit => Self::AuditCapturedLockfile,
        }
    }
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Log {
    #[schemars(regex(pattern = "^rust-artifact://prj_[0-9a-f]{32}/art_[0-9a-f]{32}$"))]
    uri: String,
    #[schemars(regex(pattern = "^[0-9a-f]{64}$"))]
    sha256: String,
    #[schemars(range(max = 262144))]
    size_bytes: u32,
    truncated: bool,
    retention_remaining_seconds: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum LogUnavailableReason {
    RetentionCapacity,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Execution {
    #[schemars(with = "super::inspection::schemas::RuntimeIdentity")]
    runtime: RuntimeIdentity,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    source_fingerprint: String,
    #[schemars(with = "super::check::schemas::ExecutionTermination")]
    termination: ExecutionTermination,
    exit_code: Option<i32>,
    validation_complete: bool,
    /// Project-writable normalized diagnostics; compiler origin is not authenticated.
    #[schemars(with = "Vec<super::check::schemas::Diagnostic>", length(max = 128))]
    diagnostics: Vec<Diagnostic>,
    stdout_truncated: bool,
    stderr_truncated: bool,
    diagnostics_omitted: u64,
}
impl From<CheckObservation> for Execution {
    fn from(o: CheckObservation) -> Self {
        Self {
            runtime: o.runtime,
            source_fingerprint: o.source_fingerprint.to_string(),
            termination: o.termination,
            exit_code: o.exit_code,
            validation_complete: o.validation_complete,
            diagnostics: o.diagnostics,
            stdout_truncated: o.stdout_truncated,
            stderr_truncated: o.stderr_truncated,
            diagnostics_omitted: o.diagnostics_omitted,
        }
    }
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FormatDetails {
    affected_files: Vec<String>,
    affected_files_omitted: u64,
    diff: Option<String>,
    diff_omitted: bool,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TestDetails {
    build_succeeded: Option<bool>,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AuditDetails {
    #[schemars(with = "super::inspection::schemas::RuntimeIdentity")]
    runtime: RuntimeIdentity,
    #[schemars(with = "super::auditing::schemas::AuditObservation")]
    observation: AuditObservation,
    unsupported_packages_omitted: u64,
    paths_omitted: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Stage {
    #[schemars(with = "schemas::QualityStage")]
    stage: QualityStage,
    duration_ms: u64,
    #[schemars(with = "schemas::ToolStatus")]
    status: ToolStatus,
    #[schemars(with = "Option<schemas::QualityIssue>")]
    issue: Option<QualityIssue>,
    applied_selection: AppliedSelection,
    execution: Option<Execution>,
    format: Option<FormatDetails>,
    test: Option<TestDetails>,
    audit: Option<AuditDetails>,
    log: Option<Log>,
    log_unavailable_reason: Option<LogUnavailableReason>,
}
impl Stage {
    fn from_report(mut row: QualityStageReport, owner: &ProjectRef) -> Result<Self, ErrorData> {
        row.classify();
        let log = match (row.log, row.retention_remaining_seconds) {
            (Some(metadata), Some(retention)) if retention > 0 && metadata.owner == *owner => {
                let mut hash = String::with_capacity(64);
                for byte in metadata.sha256 {
                    use std::fmt::Write;
                    write!(&mut hash, "{byte:02x}").map_err(|_| internal())?;
                }
                Some(Log {
                    uri: resources::uri(&metadata.owner, &metadata.id),
                    sha256: hash,
                    size_bytes: metadata.size_bytes,
                    truncated: metadata.truncated,
                    retention_remaining_seconds: retention,
                })
            }
            (None, None) => None,
            _ => return Err(internal()),
        };
        let mut value = Self {
            stage: row.stage,
            duration_ms: row.duration_ms,
            status: row.status,
            issue: row.issue,
            applied_selection: row.stage.into(),
            execution: None,
            format: None,
            test: None,
            audit: None,
            log,
            log_unavailable_reason: None,
        };
        match (row.stage, row.observation) {
            (QualityStage::Format, Some(QualityObservation::Format(o))) => {
                value.execution = Some(o.execution.into());
                value.format = Some(FormatDetails {
                    affected_files: o.affected_files,
                    affected_files_omitted: o.affected_files_omitted,
                    diff: o.diff,
                    diff_omitted: o.diff_omitted,
                });
            }
            (QualityStage::Check, Some(QualityObservation::Check(o)))
            | (QualityStage::Clippy, Some(QualityObservation::Clippy(o))) => {
                value.execution = Some(o.into())
            }
            (QualityStage::Test, Some(QualityObservation::Test(o))) => {
                value.execution = Some(o.execution.into());
                value.test = Some(TestDetails {
                    build_succeeded: o.build_succeeded,
                });
            }
            (
                QualityStage::Audit,
                Some(QualityObservation::Audit {
                    runtime,
                    observation,
                }),
            ) => {
                value.audit = Some(AuditDetails {
                    runtime,
                    paths_omitted: observation
                        .findings
                        .iter()
                        .chain(&observation.informational)
                        .fold(0_u64, |sum, finding| {
                            sum.saturating_add(finding.paths_omitted)
                        }),
                    observation,
                    unsupported_packages_omitted: 0,
                });
            }
            (_, None) => (),
            _ => return Err(internal()),
        }
        if value.log.is_some() && value.execution.is_none() {
            return Err(internal());
        }
        if value.execution.is_some() && value.log.is_none() {
            value.log_unavailable_reason = Some(LogUnavailableReason::RetentionCapacity);
        }
        Ok(value)
    }
    fn incomplete(&mut self) {
        if let Some(execution) = &mut self.execution {
            execution.validation_complete = false;
            // Keep timeout, lockfile and cancellation facts; ordinary partial execution blocks.
            if !matches!(self.status, ToolStatus::Cancelled | ToolStatus::Unavailable) {
                self.status = ToolStatus::Blocked;
                if self.issue.is_none() {
                    self.issue = Some(QualityIssue::Incomplete);
                }
            }
        }
        if let Some(audit) = &mut self.audit {
            audit.observation.validation_complete = false;
            audit.observation.normalize();
            self.status = match audit.observation.state {
                AuditState::Passed | AuditState::Incomplete => ToolStatus::Blocked,
                AuditState::Failed => ToolStatus::Failed,
                AuditState::Unavailable => ToolStatus::Unavailable,
            };
            self.issue = (self.status == ToolStatus::Blocked).then_some(QualityIssue::Incomplete);
        }
    }
    /// Each removal is visible and preserves the stage, runtime and retained log.
    fn trim_one(&mut self) -> bool {
        let removed = if let Some(execution) = &mut self.execution
            && !execution.diagnostics.is_empty()
        {
            let removed = execution.diagnostics.len().div_ceil(2);
            execution
                .diagnostics
                .truncate(execution.diagnostics.len() - removed);
            execution.diagnostics_omitted =
                execution.diagnostics_omitted.saturating_add(removed as u64);
            true
        } else if let Some(format) = &mut self.format {
            if format.diff.take().is_some() {
                format.diff_omitted = true;
                true
            } else if !format.affected_files.is_empty() {
                let removed = format.affected_files.len().div_ceil(2);
                format
                    .affected_files
                    .truncate(format.affected_files.len() - removed);
                format.affected_files_omitted =
                    format.affected_files_omitted.saturating_add(removed as u64);
                true
            } else {
                false
            }
        } else if let Some(audit) = &mut self.audit {
            let observation = &mut audit.observation;
            let mut paths_removed = 0_u64;
            for finding in observation
                .findings
                .iter_mut()
                .chain(&mut observation.informational)
            {
                let count = finding.paths.len() as u64;
                finding.paths.clear();
                finding.paths_omitted = finding.paths_omitted.saturating_add(count);
                paths_removed = paths_removed.saturating_add(count);
            }
            audit.paths_omitted = audit.paths_omitted.saturating_add(paths_removed);
            if paths_removed > 0 {
                true
            } else if !observation.informational.is_empty() || !observation.findings.is_empty() {
                let findings = if observation.informational.is_empty() {
                    &mut observation.findings
                } else {
                    &mut observation.informational
                };
                let removed = findings.len().div_ceil(2);
                findings.truncate(findings.len() - removed);
                observation.findings_omitted =
                    observation.findings_omitted.saturating_add(removed as u64);
                true
            } else if !observation.unsupported_packages.is_empty() {
                let removed = observation.unsupported_packages.len().div_ceil(2);
                observation
                    .unsupported_packages
                    .truncate(observation.unsupported_packages.len() - removed);
                audit.unsupported_packages_omitted = audit
                    .unsupported_packages_omitted
                    .saturating_add(removed as u64);
                true
            } else {
                false
            }
        } else {
            false
        };
        if removed {
            self.incomplete();
        }
        removed
    }
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    project_identity_fingerprint: String,
    semantics: Semantics,
    #[schemars(with = "schemas::QualityProfile")]
    profile: QualityProfile,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    source_fingerprint: Option<String>,
    #[schemars(length(min = 3, max = 5))]
    stages: Vec<Stage>,
}
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    ProjectNotFound,
    InvalidProject,
    ToolNotInstalled,
    LockfileUpdateRequired,
    CommandTimeout,
    SandboxDenied,
    NetworkDenied,
    UnsupportedPlatform,
    OutputLimitExceeded,
    QualityGateBlocked,
    QualityGateUnavailable,
}
impl From<OperationalErrorCode> for Code {
    fn from(code: OperationalErrorCode) -> Self {
        match code {
            OperationalErrorCode::ProjectNotFound => Self::ProjectNotFound,
            OperationalErrorCode::InvalidProject => Self::InvalidProject,
            OperationalErrorCode::ToolNotInstalled => Self::ToolNotInstalled,
            OperationalErrorCode::LockfileUpdateRequired => Self::LockfileUpdateRequired,
            OperationalErrorCode::CommandTimeout => Self::CommandTimeout,
            OperationalErrorCode::SandboxDenied => Self::SandboxDenied,
            OperationalErrorCode::NetworkDenied => Self::NetworkDenied,
            OperationalErrorCode::UnsupportedPlatform => Self::UnsupportedPlatform,
            OperationalErrorCode::OutputLimitExceeded => Self::OutputLimitExceeded,
        }
    }
}
#[derive(Serialize, JsonSchema)]
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
        data: Option<Box<Data>>,
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
    affected_files_omitted: u64,
    diffs_omitted: u64,
    findings_omitted: u64,
    paths_omitted: u64,
    unsupported_packages_omitted: u64,
}
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Output {
    #[serde(flatten)]
    outcome: Outcome,
    summary: &'static str,
    duration_ms: u64,
    diagnostics: [(); 0],
    truncation: Truncation,
    #[schemars(with = "super::inspection::schemas::Evidence")]
    evidence: Evidence,
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
fn internal() -> ErrorData {
    ErrorData::internal_error("Quality gate evidence is inconsistent", None)
}
fn outcome(data: Box<Data>, status: ToolStatus) -> Outcome {
    match status {
        ToolStatus::Passed => Outcome::Passed {
            error_code: (),
            error_message: (),
            data,
        },
        ToolStatus::Failed => Outcome::Failed {
            error_code: (),
            error_message: (),
            data,
        },
        ToolStatus::Blocked => Outcome::Blocked {
            error_code: Code::QualityGateBlocked,
            error_message: "Required quality evidence is incomplete or blocked",
            data: Some(data),
        },
        ToolStatus::Unavailable => Outcome::Unavailable {
            error_code: Code::QualityGateUnavailable,
            error_message: "Required quality evidence is unavailable",
            data: Some(data),
        },
        ToolStatus::Cancelled => Outcome::Cancelled {
            error_code: (),
            error_message: (),
            data: (),
        },
    }
}
fn output(
    result: Result<ProjectQualityGate, InspectionError>,
    duration_ms: u64,
) -> Result<Output, ErrorData> {
    let mut evidence = Evidence::Local;
    let result = match result {
        Ok(mut project) => {
            if project.stages.len() != project.profile.stages().len()
                || !project
                    .stages
                    .iter()
                    .zip(project.profile.stages())
                    .all(|(row, stage)| row.stage == *stage)
            {
                return Err(internal());
            }
            let mut runtime = None;
            for row in &mut project.stages {
                if let Some(execution) = row
                    .observation
                    .as_mut()
                    .and_then(QualityObservation::execution_mut)
                    && execution.diagnostics.len() > 128
                {
                    let omitted = execution.diagnostics.len() - 128;
                    execution.diagnostics.truncate(128);
                    execution.diagnostics_omitted =
                        execution.diagnostics_omitted.saturating_add(omitted as u64);
                    execution.validation_complete = false;
                }
                row.classify();
                if let Some(observation) = &row.observation {
                    let expected = runtime.get_or_insert_with(|| observation.runtime().clone());
                    if !quality_runtime_matches(expected, observation.runtime()) {
                        return Err(internal());
                    }
                }
                if let Some(o) = &row.observation
                    && (project.source_fingerprint.is_none()
                        || o.execution().is_some_and(|e| {
                            Some(&e.source_fingerprint) != project.source_fingerprint.as_ref()
                        }))
                {
                    return Err(internal());
                }
            }
            let status = quality_status(project.profile, &project.stages);
            evidence = project.evidence;
            let stages = project
                .stages
                .into_iter()
                .map(|row| Stage::from_report(row, &project.project_ref))
                .collect::<Result<Vec<_>, _>>()?;
            outcome(
                Box::new(Data {
                    project_ref: project.project_ref.to_string(),
                    project_identity_fingerprint: project.project_identity_fingerprint.to_string(),
                    semantics: Semantics::LatestKnown,
                    profile: project.profile,
                    source_fingerprint: project.source_fingerprint.map(|f| f.to_string()),
                    stages,
                }),
                status,
            )
        }
        Err(
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled),
        ) => Outcome::Cancelled {
            error_code: (),
            error_message: (),
            data: (),
        },
        Err(error) => {
            let code = match error {
                InspectionError::Project(ProjectError::Rejected(code)) => code,
                InspectionError::Execution(ExecutionError::Unavailable) => {
                    OperationalErrorCode::ToolNotInstalled
                }
                InspectionError::Execution(
                    ExecutionError::Denied
                    | ExecutionError::Busy
                    | ExecutionError::InvalidConfiguration,
                ) => OperationalErrorCode::SandboxDenied,
                InspectionError::OutputLimit => OperationalErrorCode::OutputLimitExceeded,
                InspectionError::InvalidMetadata => OperationalErrorCode::InvalidProject,
                InspectionError::Execution(ExecutionError::CleanupUncertain) => {
                    return Err(ErrorData::internal_error(
                        "Gateway cleanup could not be verified; further execution is quarantined",
                        None,
                    ));
                }
                _ => return Err(internal()),
            };
            if code.status() == ToolStatus::Unavailable {
                Outcome::Unavailable {
                    error_code: code.into(),
                    error_message: "Approved quality runtime is unavailable",
                    data: None,
                }
            } else {
                Outcome::Blocked {
                    error_code: code.into(),
                    error_message: "Quality gate could not execute under the current project and runtime policy",
                    data: None,
                }
            }
        }
    };
    let mut value = Output {
        outcome: result,
        summary: "Quality gate completed; inspect every required stage and its applied selection",
        duration_ms,
        diagnostics: [],
        truncation: Truncation::default(),
        evidence,
    };
    refresh(&mut value);
    Ok(value)
}
fn data_mut(value: &mut Output) -> Option<&mut Data> {
    match &mut value.outcome {
        Outcome::Passed { data, .. }
        | Outcome::Failed { data, .. }
        | Outcome::Blocked {
            data: Some(data), ..
        }
        | Outcome::Unavailable {
            data: Some(data), ..
        } => Some(data),
        _ => None,
    }
}
fn refresh(value: &mut Output) {
    let mut truncation = Truncation::default();
    if let Some(data) = data_mut(value) {
        for row in &data.stages {
            if let Some(o) = &row.execution {
                truncation.stdout_truncated |= o.stdout_truncated;
                truncation.stderr_truncated |= o.stderr_truncated;
                truncation.diagnostics_omitted = truncation
                    .diagnostics_omitted
                    .saturating_add(o.diagnostics_omitted);
            }
            if let Some(o) = &row.format {
                truncation.affected_files_omitted = truncation
                    .affected_files_omitted
                    .saturating_add(o.affected_files_omitted);
                truncation.diffs_omitted += u64::from(o.diff_omitted);
            }
            if let Some(o) = &row.audit {
                truncation.findings_omitted = truncation
                    .findings_omitted
                    .saturating_add(o.observation.findings_omitted);
                truncation.unsupported_packages_omitted = truncation
                    .unsupported_packages_omitted
                    .saturating_add(o.unsupported_packages_omitted);
                truncation.paths_omitted = truncation.paths_omitted.saturating_add(o.paths_omitted);
            }
        }
    }
    value.truncation = truncation;
    value.summary = match value.status() {
        ToolStatus::Passed => "Every required quality stage passed with complete evidence",
        ToolStatus::Failed => "Quality failures found; inspect each stage's repair evidence",
        ToolStatus::Blocked => {
            "Quality gate blocked; inspect every stage's status and retained evidence"
        }
        ToolStatus::Unavailable => {
            "Required quality evidence is unavailable; other stage results are retained"
        }
        ToolStatus::Cancelled => "Quality gate cancelled after worker completion",
    };
}
fn reaggregate(value: &mut Output) {
    let previous = std::mem::replace(
        &mut value.outcome,
        Outcome::Cancelled {
            error_code: (),
            error_message: (),
            data: (),
        },
    );
    value.outcome = match previous {
        Outcome::Passed { data, .. }
        | Outcome::Failed { data, .. }
        | Outcome::Blocked {
            data: Some(data), ..
        }
        | Outcome::Unavailable {
            data: Some(data), ..
        } => {
            let status = [
                ToolStatus::Cancelled,
                ToolStatus::Blocked,
                ToolStatus::Unavailable,
                ToolStatus::Failed,
            ]
            .into_iter()
            .find(|status| data.stages.iter().any(|row| row.status == *status))
            .unwrap_or(ToolStatus::Passed);
            outcome(data, status)
        }
        other => other,
    };
    refresh(value);
}
fn encode_bounded(
    contract: &Contract<Input, Output>,
    mut value: Output,
) -> Result<CallToolResult, ErrorData> {
    // A quarter budget covers duplicated structured/text content and JSON escaping.
    while serde_json::to_vec(&value).map_err(|_| internal())?.len() > MAX_RESULT / 4 {
        let removed =
            data_mut(&mut value).is_some_and(|data| data.stages.iter_mut().any(Stage::trim_one));
        if !removed {
            return Err(ErrorData::internal_error(
                "Quality evidence exceeds the response budget",
                None,
            ));
        }
        reaggregate(&mut value);
    }
    let encoded = contract.encode(value)?;
    if serde_json::to_vec(&encoded).map_err(|_| internal())?.len() > MAX_RESULT {
        return Err(internal());
    }
    Ok(encoded)
}
fn joined_result<T>(joined: Joined<T, InspectionError>) -> Result<T, InspectionError> {
    match (joined.result, joined.interrupted) {
        (
            Err(
                InspectionError::Project(ProjectError::Cancelled)
                | InspectionError::Execution(ExecutionError::Cancelled),
            ),
            Some(signal),
        ) => Err(worker_error(signal)),
        (Err(error), _) => Err(error),
        // The application checks control immediately before its publication commit point.
        // Ok means the lease and grouped logs are committed; a later signal must not
        // reinterpret that successful publication as an uncommitted cancelled result.
        (Ok(value), _) => Ok(value),
    }
}
pub(super) struct QualityTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
    provider: Arc<AuditProvider>,
    store: Arc<Mutex<Store>>,
    clock: ArtifactClock,
}
fn definition(contract: &Contract<Input, Output>) -> Tool {
    Tool::new(NAME,"Run one captured-source quality gate: fast runs fmt --all, Cargo-default check and strict Clippy; standard adds Cargo-default tests (30 seconds) and offline RustSec audit. No all-target/all-feature coverage is implied. Can execute project build scripts, proc macros and test code inside the calibrated sandbox. Preserves each stage, normalized repair evidence and owner-authorized ephemeral log Resources. Only all complete required stages pass. Requires completed discovery and a live project_ref; downloads and installs nothing.",(*contract.input_schema).clone()).with_raw_output_schema(Arc::clone(&contract.output_schema)).with_annotations(ToolAnnotations::new().read_only(true).destructive(false).idempotent(false).open_world(false))
}
impl QualityTool {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
        resources: &resources::Resources,
        config: Option<HostAuditConfig>,
    ) -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition = definition(&contract);
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
            provider: Arc::new(AuditProvider(config)),
            store: resources.store(),
            clock: resources.clock(),
        })
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let started = Instant::now();
        let bootstrap = !self.ready.load(Ordering::Acquire);
        let result = if bootstrap {
            Err(InspectionError::Execution(ExecutionError::Denied))
        } else {
            let registry = Arc::clone(&self.registry);
            let inspector = Arc::clone(&self.inspector);
            let store = Arc::clone(&self.store);
            let clock = self.clock.clone();
            let provider = Arc::clone(&self.provider);
            match self
                .workers
                .run_joined(context.ct, started + DEADLINE, move |control| {
                    registry
                        .lock()
                        .map_err(|_| InspectionError::Internal)?
                        .quality_gate(
                            &input.project_ref,
                            input.profile,
                            QualityPorts {
                                executor: inspector.as_ref(),
                                auditor: provider.as_ref(),
                            },
                            &mut *store.lock().map_err(|_| InspectionError::Internal)?,
                            (&WallClock, &clock),
                            control,
                        )
                })
                .await
            {
                Ok(joined) => joined_result(joined),
                Err(error) => Err(worker_error(error)),
            }
        };
        let mut value = output(
            result,
            started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        )?;
        if bootstrap {
            let message = "Quality gate requires completed discovery; retry with a new request ID";
            value.summary = message;
            value.outcome = Outcome::Blocked {
                error_code: Code::SandboxDenied,
                error_message: message,
                data: None,
            };
        }
        encode_bounded(&self.contract, value)
    }
}
#[cfg(test)]
mod tests;
