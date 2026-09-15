//! `rust.analyzer.actions` (M6-04, ADR-083 §2, §4): the code actions
//! rust-analyzer offers over one captured range, each with its applicability
//! and, when applicable, its `action_digest` and edits summary. Read-only: the
//! edits are resolved and structurally validated, never applied —
//! `rust.analyzer.action.apply` owns every effect.
use super::{
    DEADLINE, MAX_RESULT, MAX_TIMEOUT_SECONDS, ReferencesCode, WireRange, analyzer_joined_result,
    default_timeout_seconds, is_peer_text_hazard, references_failure_code, schemas, wire_analyzer,
    wire_completeness, wire_limits, wire_readiness, wire_session,
};
use crate::stdio::{
    contract::{Contract, ToolOutput},
    project::Registry,
    workers::{Workers, worker_error},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{
    ExecutionError, InspectionError, ProjectError,
    analyzer::{
        ActionSummary, ActionsReport, ActionsRequest, AnalyzerReport, AnalyzerRequestError,
    },
};
use rust_engineering_domain as domain;
use rust_engineering_domain::{
    AnalyzerFile, OperationalErrorCode, ProjectIdentityFingerprint, ProjectRef, ToolStatus,
};
use rust_engineering_execution::RustProjectInspector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

pub(in crate::stdio) const ACTIONS_NAME: &str = "rust.analyzer.actions";

/// A peer title is display text only: bounded to this many Unicode scalars
/// and flagged `title_truncated` rather than silently cut. The digest always
/// covers the complete title the peer sent.
const MAX_TITLE_SCALARS: usize = 256;

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::stdio) struct ActionsInput {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    /// Mandatory for this tool (ADR-083 §2); a mismatch is `blocked/CONFLICT`.
    #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    expected_project_fingerprint: ProjectIdentityFingerprint,
    #[schemars(with = "String", regex(pattern = r"^[A-Za-z0-9_./-]{1,100}\.rs$"))]
    file: AnalyzerFile,
    range: WireRange,
    /// Kinds to ask for; empty or absent asks for every kind.
    #[serde(default)]
    #[schemars(length(max = 7))]
    only: Vec<schemas::CodeActionKind>,
    #[serde(default = "default_timeout_seconds")]
    #[schemars(range(min = 1, max = 180))]
    timeout_seconds: u32,
}
impl ActionsInput {
    /// `None` for a reversed range: `start ≤ end` is a Rust-level invariant
    /// the schema cannot express. Duplicate `only` kinds collapse.
    fn request(self) -> Option<ActionsRequest> {
        let range = self.range.into_domain()?;
        let mut only: Vec<domain::CodeActionKind> = Vec::with_capacity(self.only.len());
        for kind in self.only.into_iter().map(domain::CodeActionKind::from) {
            if !only.contains(&kind) {
                only.push(kind);
            }
        }
        Some(ActionsRequest {
            expected_project_fingerprint: self.expected_project_fingerprint,
            file: self.file,
            range,
            only,
            timeout_seconds: self.timeout_seconds.min(MAX_TIMEOUT_SECONDS),
        })
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ActionsData {
    #[schemars(regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    project_identity_fingerprint: String,
    snapshot: schemas::Snapshot,
    analyzer: schemas::Analyzer,
    toolchain: schemas::Toolchain,
    readiness: schemas::Readiness,
    completeness: schemas::Completeness,
    limits: schemas::Limits,
    session: schemas::Session,
    termination: schemas::Termination,
    exit_code: Option<i32>,
    oom_killed: Option<bool>,
    /// `None` exactly when the session never answered. In the order
    /// rust-analyzer returned them, at most 32.
    #[schemars(length(max = 32))]
    actions: Option<Vec<schemas::CodeAction>>,
    /// Every omission: actions beyond the 32 visible, and any trimmed to fit
    /// the result budget.
    omitted: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ActionsCode {
    Conflict,
    FileNotInSnapshot,
    PositionOutOfRange,
    AnalyzerNotReady,
    AnalyzerCrashed,
    AnalyzerCapabilityMismatch,
    FrameLimit,
    MessageLimit,
    ResultLimit,
    TimeoutInitialize,
    TimeoutQuery,
    TimeoutTotal,
    UnsupportedProjectConfig,
    FileNotUtf8,
    SandboxDenied,
    UnsupportedPlatform,
    ProjectNotFound,
    InvalidProject,
    OutputLimitExceeded,
}

/// The two tools query a caller location and share one closed vocabulary.
impl From<ReferencesCode> for ActionsCode {
    fn from(value: ReferencesCode) -> Self {
        match value {
            ReferencesCode::Conflict => Self::Conflict,
            ReferencesCode::FileNotInSnapshot => Self::FileNotInSnapshot,
            ReferencesCode::PositionOutOfRange => Self::PositionOutOfRange,
            ReferencesCode::AnalyzerNotReady => Self::AnalyzerNotReady,
            ReferencesCode::AnalyzerCrashed => Self::AnalyzerCrashed,
            ReferencesCode::AnalyzerCapabilityMismatch => Self::AnalyzerCapabilityMismatch,
            ReferencesCode::FrameLimit => Self::FrameLimit,
            ReferencesCode::MessageLimit => Self::MessageLimit,
            ReferencesCode::ResultLimit => Self::ResultLimit,
            ReferencesCode::TimeoutInitialize => Self::TimeoutInitialize,
            ReferencesCode::TimeoutQuery => Self::TimeoutQuery,
            ReferencesCode::TimeoutTotal => Self::TimeoutTotal,
            ReferencesCode::UnsupportedProjectConfig => Self::UnsupportedProjectConfig,
            ReferencesCode::FileNotUtf8 => Self::FileNotUtf8,
            ReferencesCode::SandboxDenied => Self::SandboxDenied,
            ReferencesCode::UnsupportedPlatform => Self::UnsupportedPlatform,
            ReferencesCode::ProjectNotFound => Self::ProjectNotFound,
            ReferencesCode::InvalidProject => Self::InvalidProject,
            ReferencesCode::OutputLimitExceeded => Self::OutputLimitExceeded,
        }
    }
}

// Statuses: `passed | blocked | unavailable | cancelled`, never `failed`, as
// for every analyzer read tool.
#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ActionsOutcome {
    Passed {
        error_code: (),
        error_message: (),
        data: Box<ActionsData>,
    },
    Blocked {
        error_code: ActionsCode,
        error_message: &'static str,
        data: Option<Box<ActionsData>>,
    },
    Unavailable {
        error_code: ActionsCode,
        error_message: &'static str,
        data: Option<Box<ActionsData>>,
    },
    Cancelled {
        error_code: (),
        error_message: (),
        data: (),
    },
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::stdio) struct ActionsOutput {
    #[serde(flatten)]
    outcome: ActionsOutcome,
    summary: &'static str,
    duration_ms: u64,
}
impl ToolOutput for ActionsOutput {
    fn status(&self) -> ToolStatus {
        match self.outcome {
            ActionsOutcome::Passed { .. } => ToolStatus::Passed,
            ActionsOutcome::Blocked { .. } => ToolStatus::Blocked,
            ActionsOutcome::Unavailable { .. } => ToolStatus::Unavailable,
            ActionsOutcome::Cancelled { .. } => ToolStatus::Cancelled,
        }
    }
}

fn refusal(
    code: ActionsCode,
    message: &'static str,
    unavailable: bool,
    data: Option<Box<ActionsData>>,
) -> ActionsOutcome {
    if unavailable {
        ActionsOutcome::Unavailable {
            error_code: code,
            error_message: message,
            data,
        }
    } else {
        ActionsOutcome::Blocked {
            error_code: code,
            error_message: message,
            data,
        }
    }
}

/// Every peer-text hazard (see `is_peer_text_hazard`) is replaced and the
/// title bounded: a title is peer text, like a symbol name (D25 §1.6). Both
/// an applicable and a rejected action's title go through this.
fn bounded_title(text: &str) -> (String, bool) {
    let mut title = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index == MAX_TITLE_SCALARS {
            return (title, true);
        }
        title.push(if is_peer_text_hazard(ch) {
            '\u{fffd}'
        } else {
            ch
        });
    }
    (title, false)
}

fn wire_action(summary: &ActionSummary) -> schemas::CodeAction {
    match summary {
        ActionSummary::Applicable {
            action_digest,
            title,
            kind,
            is_preferred,
            edits_summary,
        } => {
            let (title, title_truncated) = bounded_title(title.as_str());
            schemas::CodeAction::Applicable {
                action_digest: action_digest.to_string(),
                title,
                title_truncated,
                kind: kind.map(Into::into),
                is_preferred: *is_preferred,
                edits_summary: schemas::EditsSummary {
                    files: edits_summary.files,
                    edits: edits_summary.edits,
                    bytes_delta: edits_summary.bytes_delta,
                },
            }
        }
        ActionSummary::Rejected {
            reason,
            title,
            kind,
        } => {
            let (title, title_truncated) = title
                .as_ref()
                .map(|title| bounded_title(title.as_str()))
                .map_or((None, false), |(title, truncated)| (Some(title), truncated));
            schemas::CodeAction::Rejected {
                reason: (*reason).into(),
                title,
                title_truncated,
                kind: kind.map(Into::into),
            }
        }
    }
}

fn actions_common_data(
    report: &AnalyzerReport,
    actions: Option<Vec<schemas::CodeAction>>,
    omitted: u32,
    total_timeout_seconds: u32,
) -> ActionsData {
    ActionsData {
        project_ref: report.project_ref.to_string(),
        project_identity_fingerprint: report.project_identity_fingerprint.to_string(),
        snapshot: schemas::Snapshot {
            source_fingerprint: report.snapshot.source_fingerprint.to_string(),
            files: report.snapshot.files,
            semantics: schemas::Semantics::LatestKnown,
            atomic: report.snapshot.atomic,
        },
        analyzer: wire_analyzer(&report.execution),
        toolchain: schemas::Toolchain {
            rust_version: "1.98.1",
            sysroot: schemas::Sysroot::Present,
        },
        readiness: wire_readiness(report.execution.readiness),
        completeness: wire_completeness(&report.execution.completeness),
        limits: wire_limits(total_timeout_seconds),
        session: wire_session(&report.execution.session),
        termination: report.execution.termination.into(),
        exit_code: report.execution.session.exit_code,
        oom_killed: report.execution.oom_killed,
        actions,
        omitted,
    }
}

fn actions_execution_outcome(
    answer: ActionsReport,
    total_timeout_seconds: u32,
) -> Result<(ActionsOutcome, &'static str), ErrorData> {
    let ActionsReport { report, actions } = answer;
    match &report.execution.outcome {
        domain::AnalyzerOutcome::Answered(result) => {
            if !matches!(result, domain::AnalyzerResult::CodeActions(_)) {
                return Err(ErrorData::internal_error(
                    "rust.analyzer.actions received an answer to a different query",
                    None,
                ));
            }
            let omitted = report
                .execution
                .completeness
                .omissions()
                .iter()
                .map(|omission| omission.count)
                .sum();
            let actions = actions.iter().map(wire_action).collect();
            let data = actions_common_data(&report, Some(actions), omitted, total_timeout_seconds);
            Ok((
                ActionsOutcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: Box::new(data),
                },
                "rust-analyzer answered the code actions query; nothing was applied",
            ))
        }
        domain::AnalyzerOutcome::Failed(domain::AnalyzerFailure::Cancelled) => Ok((
            ActionsOutcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Analyzer session cancelled",
        )),
        domain::AnalyzerOutcome::Failed(failure) => {
            let (code, message, unavailable) = references_failure_code(*failure)?;
            let data = Some(Box::new(actions_common_data(
                &report,
                None,
                0,
                total_timeout_seconds,
            )));
            Ok((refusal(code.into(), message, unavailable, data), message))
        }
    }
}

fn actions_operational(code: OperationalErrorCode) -> (ActionsOutcome, &'static str) {
    let (code, message, unavailable) = match code {
        OperationalErrorCode::ProjectNotFound => (
            ActionsCode::ProjectNotFound,
            "Project reference is missing or expired",
            false,
        ),
        OperationalErrorCode::InvalidProject => (
            ActionsCode::InvalidProject,
            "Captured project is invalid or unsupported",
            false,
        ),
        OperationalErrorCode::ToolNotInstalled => (
            ActionsCode::SandboxDenied,
            "Approved analyzer runtime is unavailable",
            true,
        ),
        OperationalErrorCode::LockfileUpdateRequired | OperationalErrorCode::NetworkDenied => (
            ActionsCode::SandboxDenied,
            "Host runtime policy denied analyzer execution",
            true,
        ),
        OperationalErrorCode::CommandTimeout => (
            ActionsCode::TimeoutTotal,
            "Analyzer call exceeded its total budget",
            true,
        ),
        OperationalErrorCode::SandboxDenied => (
            ActionsCode::SandboxDenied,
            "Host runtime policy, failed calibration or current capacity denied analysis",
            true,
        ),
        OperationalErrorCode::UnsupportedPlatform => (
            ActionsCode::UnsupportedPlatform,
            "Secure analyzer session is unavailable on this platform",
            true,
        ),
        OperationalErrorCode::OutputLimitExceeded => (
            ActionsCode::OutputLimitExceeded,
            "Project metadata exceeds the response budget",
            false,
        ),
    };
    (refusal(code, message, unavailable, None), message)
}

fn actions_output(
    result: Result<ActionsReport, AnalyzerRequestError>,
    duration_ms: u64,
    total_timeout_seconds: u32,
) -> Result<ActionsOutput, ErrorData> {
    let (outcome, summary) = match result {
        Ok(answer) => actions_execution_outcome(answer, total_timeout_seconds)?,
        Err(AnalyzerRequestError::Conflict) => {
            let message = "expected_project_fingerprint does not match the live project identity";
            (
                refusal(ActionsCode::Conflict, message, false, None),
                message,
            )
        }
        Err(AnalyzerRequestError::FileNotInSnapshot) => {
            let message = "Queried file is absent from the capture";
            (
                refusal(ActionsCode::FileNotInSnapshot, message, false, None),
                message,
            )
        }
        Err(AnalyzerRequestError::PositionOutOfRange) => {
            let message = "Queried range is outside the captured file";
            (
                refusal(ActionsCode::PositionOutOfRange, message, false, None),
                message,
            )
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::Project(
            ProjectError::Rejected(code),
        ))) => actions_operational(code),
        Err(AnalyzerRequestError::Inspection(
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled),
        )) => (
            ActionsOutcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Analyzer actions cancelled after worker completion",
        ),
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::Unavailable,
        ))) => actions_operational(OperationalErrorCode::ToolNotInstalled),
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        ))) => actions_operational(OperationalErrorCode::SandboxDenied),
        Err(AnalyzerRequestError::Inspection(InspectionError::OutputLimit)) => {
            actions_operational(OperationalErrorCode::OutputLimitExceeded)
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::InvalidMetadata)) => {
            actions_operational(OperationalErrorCode::InvalidProject)
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::CleanupUncertain,
        ))) => {
            return Err(ErrorData::internal_error(
                "Gateway cleanup could not be verified; further execution is quarantined",
                None,
            ));
        }
        Err(AnalyzerRequestError::Inspection(
            InspectionError::Internal
            | InspectionError::Project(ProjectError::Internal)
            | InspectionError::Execution(ExecutionError::Infrastructure),
        )) => return Err(ErrorData::internal_error("Analyzer actions failed", None)),
    };
    Ok(ActionsOutput {
        outcome,
        summary,
        duration_ms,
    })
}

pub(in crate::stdio) struct ActionsTool {
    pub(in crate::stdio) definition: Tool,
    contract: Contract<ActionsInput, ActionsOutput>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
}
pub(in crate::stdio) fn definition()
-> Result<(Contract<ActionsInput, ActionsOutput>, Tool), ErrorData> {
    let contract = Contract::<ActionsInput, ActionsOutput>::new()?;
    let definition = Tool::new(
        ACTIONS_NAME,
        format!(
            "{}{}",
            crate::stdio::stability::PREVIEW_PREFIX,
            "List the code actions the host-approved rust-analyzer 1.98.1 \
             (aarch64-unknown-linux-gnu) offers over a range of a captured Rust file, \
             inside the M6 guest image. Read-only: each action's WorkspaceEdit is \
             resolved and structurally validated but never applied; applying one is \
             rust.analyzer.action.apply. Snapshot semantics are latest_known and \
             non-atomic. Build scripts, proc macros and check-on-save stay disabled; \
             only textDocument/codeAction runs, never cargo check. An applicable action \
             carries an action_digest binding its title, kind and edits to this analyzer \
             version, binary, configuration and capture, plus an edits_summary (files, \
             edits, bytes_delta); an action may edit several captured files. A rejected \
             action carries its closed reason (command, snippet, resource_operation, \
             external_uri, version_mismatch, overlapping_ranges, edit_limit, bytes_limit, \
             not_utf8, file_not_in_snapshot, unresolved_edit) and, when the analyzer sent \
             them, its title and kind; a Command is never executed. At most 32 actions \
             and 128 edits per action. Titles are analyzer text, bounded to 256 \
             Unicode scalars with control characters replaced. expected_project_fingerprint \
             is required. Positions are Unicode-scalar, 1-based Position values. Requires \
             the host --rust runtime configured with the approved M6 image; without it \
             the tool is unavailable."
        ),
        (*contract.input_schema).clone(),
    )
    .with_raw_output_schema(Arc::clone(&contract.output_schema))
    .with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    );
    Ok((contract, definition))
}

impl ActionsTool {
    pub(in crate::stdio) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, ErrorData> {
        let (contract, definition) = definition()?;
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
        })
    }

    pub(in crate::stdio) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let project_ref = input.project_ref.clone();
        let actions_request = input
            .request()
            .ok_or_else(|| ErrorData::invalid_params("Invalid tool arguments", None))?;
        let total_timeout_seconds = actions_request.timeout_seconds;
        let started = Instant::now();
        let bootstrap = !self.ready.load(Ordering::Acquire);
        let result = if bootstrap {
            Err(AnalyzerRequestError::Inspection(
                InspectionError::Execution(ExecutionError::Denied),
            ))
        } else {
            let registry = Arc::clone(&self.registry);
            let inspector = Arc::clone(&self.inspector);
            match self
                .workers
                .run_joined(context.ct, started + DEADLINE, move |control| {
                    registry
                        .lock()
                        .map_err(|_| AnalyzerRequestError::Inspection(InspectionError::Internal))?
                        .analyzer_actions(
                            &project_ref,
                            actions_request,
                            inspector.as_ref(),
                            control,
                        )
                })
                .await
            {
                Ok(joined) => analyzer_joined_result(joined),
                Err(error) => Err(worker_error(error).into()),
            }
        };
        let duration = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let value = if bootstrap {
            actions_bootstrap_refusal(duration)
        } else {
            actions_output(result, duration, total_timeout_seconds)?
        };
        encode_actions_bounded(&self.contract, value)
    }
}

fn actions_bootstrap_refusal(duration_ms: u64) -> ActionsOutput {
    let message = "Analyzer actions requires completed discovery; retry with a new request ID";
    ActionsOutput {
        outcome: refusal(ActionsCode::SandboxDenied, message, false, None),
        summary: message,
        duration_ms,
    }
}

fn encode_actions_bounded(
    contract: &Contract<ActionsInput, ActionsOutput>,
    value: ActionsOutput,
) -> Result<CallToolResult, ErrorData> {
    encode_actions_bounded_within(contract, value, MAX_RESULT)
}

/// The same declared trim as the other analyzer tools: actions are popped
/// from the end and counted in `omitted` with `result_limit`, never a
/// truncated JSON; `unavailable/RESULT_LIMIT` only if nothing is left to trim.
fn encode_actions_bounded_within(
    contract: &Contract<ActionsInput, ActionsOutput>,
    mut value: ActionsOutput,
    max_result: usize,
) -> Result<CallToolResult, ErrorData> {
    while serde_json::to_vec(&value)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > max_result / 4
    {
        let ActionsOutcome::Passed { data, .. } = &mut value.outcome else {
            break;
        };
        let Some(actions) = &mut data.actions else {
            break;
        };
        if actions.pop().is_none() {
            break;
        }
        data.omitted = data.omitted.saturating_add(1);
        data.completeness.state = schemas::CompletenessState::Incomplete;
        if !data
            .completeness
            .reasons
            .iter()
            .any(|reason| matches!(reason, schemas::Reason::ResultLimit))
        {
            data.completeness.reasons.push(schemas::Reason::ResultLimit);
        }
    }
    let duration = value.duration_ms;
    let encoded = contract.encode(value)?;
    if serde_json::to_vec(&encoded)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > max_result
    {
        let message = "Analyzer result could not be retained within the output budget";
        return contract.encode(ActionsOutput {
            outcome: refusal(ActionsCode::ResultLimit, message, true, None),
            summary: message,
            duration_ms: duration,
        });
    }
    Ok(encoded)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake.
mod tests {
    use super::super::tests::{TestResult, answered, failed, report, session};
    use super::*;
    use rust_engineering_domain::{
        ActionRejection, AnalyzerFailure, AnalyzerOutcome, AnalyzerResult, CodeActionKind,
        EditsSummary, NonEmptyText,
    };

    fn digest(value: u8) -> domain::SourceFingerprint {
        format!("sha256:{value:064x}").parse().unwrap()
    }

    fn code_actions_answered() -> domain::AnalyzerExecution {
        domain::AnalyzerExecution {
            outcome: AnalyzerOutcome::Answered(AnalyzerResult::CodeActions(Vec::new())),
            ..answered().unwrap()
        }
    }

    fn applicable(title: &str) -> ActionSummary {
        ActionSummary::Applicable {
            action_digest: digest(7),
            title: NonEmptyText::try_from(title.to_owned()).unwrap(),
            kind: Some(CodeActionKind::RefactorInline),
            is_preferred: true,
            edits_summary: EditsSummary {
                files: 2,
                edits: 3,
                bytes_delta: -4,
            },
        }
    }

    fn answer(actions: Vec<ActionSummary>) -> ActionsReport {
        ActionsReport {
            report: report(code_actions_answered()).unwrap(),
            actions,
        }
    }

    fn status_of(value: &ActionsOutput) -> &'static str {
        match value.outcome {
            ActionsOutcome::Passed { .. } => "passed",
            ActionsOutcome::Blocked { .. } => "blocked",
            ActionsOutcome::Unavailable { .. } => "unavailable",
            ActionsOutcome::Cancelled { .. } => "cancelled",
        }
    }

    fn code_of(value: &ActionsOutput) -> Option<ActionsCode> {
        match &value.outcome {
            ActionsOutcome::Blocked { error_code, .. }
            | ActionsOutcome::Unavailable { error_code, .. } => Some(*error_code),
            ActionsOutcome::Passed { .. } | ActionsOutcome::Cancelled { .. } => None,
        }
    }

    #[test]
    fn applicable_and_rejected_actions_publish_their_digest_summary_and_labels() -> TestResult {
        let actions = vec![
            applicable("Inline variable"),
            ActionSummary::Rejected {
                reason: ActionRejection::Command,
                title: Some(NonEmptyText::try_from("Run test".to_owned())?),
                kind: Some(CodeActionKind::QuickFix),
            },
            ActionSummary::Rejected {
                reason: ActionRejection::UnresolvedEdit,
                title: None,
                kind: None,
            },
        ];
        let value = actions_output(Ok(answer(actions)), 5, 60)?;
        assert_eq!(status_of(&value), "passed");
        let contract = Contract::<ActionsInput, ActionsOutput>::new()?;
        let encoded = serde_json::to_value(encode_actions_bounded(&contract, value)?)?;
        let data = &encoded["structuredContent"]["data"];
        assert_eq!(
            data["actions"],
            serde_json::json!([
                {
                    "applicability": "applicable",
                    "action_digest": digest(7).to_string(),
                    "title": "Inline variable",
                    "title_truncated": false,
                    "kind": "refactor_inline",
                    "is_preferred": true,
                    "edits_summary": {"files": 2, "edits": 3, "bytes_delta": -4}
                },
                {
                    "applicability": "rejected",
                    "reason": "command",
                    "title": "Run test",
                    "title_truncated": false,
                    "kind": "quickfix"
                },
                {
                    "applicability": "rejected",
                    "reason": "unresolved_edit",
                    "title": null,
                    "title_truncated": false,
                    "kind": null
                }
            ])
        );
        assert_eq!(data["omitted"], 0);
        Ok(())
    }

    /// Every `AnalyzerFailure` this tool can observe maps to its closed pair;
    /// `PositionOutOfRange` is reachable, as for references.
    #[test]
    fn every_analyzer_failure_maps_to_its_closed_actions_code() -> TestResult {
        let table: &[(AnalyzerFailure, &str, ActionsCode)] = &[
            (
                AnalyzerFailure::FileNotInSnapshot,
                "blocked",
                ActionsCode::FileNotInSnapshot,
            ),
            (
                AnalyzerFailure::FileNotUtf8,
                "blocked",
                ActionsCode::FileNotUtf8,
            ),
            (
                AnalyzerFailure::UnsupportedProjectConfig,
                "blocked",
                ActionsCode::UnsupportedProjectConfig,
            ),
            (
                AnalyzerFailure::PositionOutOfRange,
                "blocked",
                ActionsCode::PositionOutOfRange,
            ),
            (
                AnalyzerFailure::CapabilityMismatch,
                "unavailable",
                ActionsCode::AnalyzerCapabilityMismatch,
            ),
            (
                AnalyzerFailure::NotReady,
                "unavailable",
                ActionsCode::AnalyzerNotReady,
            ),
            (
                AnalyzerFailure::Crashed,
                "unavailable",
                ActionsCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ProtocolViolation,
                "unavailable",
                ActionsCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ServerError,
                "unavailable",
                ActionsCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ProtocolLimit,
                "unavailable",
                ActionsCode::MessageLimit,
            ),
            (
                AnalyzerFailure::FrameTooLarge,
                "unavailable",
                ActionsCode::FrameLimit,
            ),
            (
                AnalyzerFailure::MalformedHeader,
                "unavailable",
                ActionsCode::FrameLimit,
            ),
            (
                AnalyzerFailure::TimeoutInitialize,
                "unavailable",
                ActionsCode::TimeoutInitialize,
            ),
            (
                AnalyzerFailure::TimeoutQuery,
                "unavailable",
                ActionsCode::TimeoutQuery,
            ),
            (
                AnalyzerFailure::TimeoutTotal,
                "unavailable",
                ActionsCode::TimeoutTotal,
            ),
        ];
        for (failure, expected_status, expected_code) in table.iter().copied() {
            let answer = ActionsReport {
                report: report(failed(failure)?)?,
                actions: Vec::new(),
            };
            let value = actions_output(Ok(answer), 1, 60)?;
            assert_eq!(status_of(&value), expected_status, "{failure:?}");
            assert_eq!(code_of(&value), Some(expected_code), "{failure:?}");
        }
        let cancelled = ActionsReport {
            report: report(failed(AnalyzerFailure::Cancelled)?)?,
            actions: Vec::new(),
        };
        let value = actions_output(Ok(cancelled), 1, 60)?;
        assert_eq!(status_of(&value), "cancelled");
        assert_eq!(code_of(&value), None);
        Ok(())
    }

    #[test]
    fn request_errors_map_to_closed_codes_with_no_data() -> TestResult {
        use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
        for (error, status, code) in [
            (
                AnalyzerRequestError::Conflict,
                "blocked",
                ActionsCode::Conflict,
            ),
            (
                AnalyzerRequestError::FileNotInSnapshot,
                "blocked",
                ActionsCode::FileNotInSnapshot,
            ),
            (
                AnalyzerRequestError::PositionOutOfRange,
                "blocked",
                ActionsCode::PositionOutOfRange,
            ),
            (
                AnalyzerRequestError::Inspection(InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::ProjectNotFound,
                ))),
                "blocked",
                ActionsCode::ProjectNotFound,
            ),
            (
                AnalyzerRequestError::Inspection(InspectionError::Execution(
                    ExecutionError::Unavailable,
                )),
                "unavailable",
                ActionsCode::SandboxDenied,
            ),
            (
                AnalyzerRequestError::Inspection(InspectionError::Project(ProjectError::Rejected(
                    OperationalErrorCode::CommandTimeout,
                ))),
                "unavailable",
                ActionsCode::TimeoutTotal,
            ),
        ] {
            let value = actions_output(Err(error), 1, 60)?;
            assert_eq!(status_of(&value), status, "{error:?}");
            assert_eq!(code_of(&value), Some(code), "{error:?}");
            assert!(
                matches!(
                    &value.outcome,
                    ActionsOutcome::Blocked { data: None, .. }
                        | ActionsOutcome::Unavailable { data: None, .. }
                ),
                "no stale data ever published"
            );
        }
        for error in [
            AnalyzerRequestError::Inspection(InspectionError::Project(ProjectError::Cancelled)),
            AnalyzerRequestError::Inspection(InspectionError::Execution(ExecutionError::Cancelled)),
        ] {
            assert_eq!(status_of(&actions_output(Err(error), 1, 60)?), "cancelled");
        }
        assert!(
            actions_output(
                Err(AnalyzerRequestError::Inspection(InspectionError::Internal)),
                1,
                60
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn bounded_title_neutralises_every_peer_text_hazard_category() {
        for hazard in [
            '\u{0007}', // Cc control (bell)
            '\u{202E}', // bidi override (RLO)
            '\u{2066}', // bidi isolate (LRI)
            '\u{200B}', // zero-width space
            '\u{FEFF}', // zero-width no-break space / BOM
            '\u{2028}', // line separator
            '\u{2029}', // paragraph separator
        ] {
            let text = format!("before{hazard}after");
            let (sanitized, truncated) = bounded_title(&text);
            assert!(!truncated);
            assert_eq!(sanitized, "before\u{fffd}after", "{hazard:?}");
        }
    }

    #[test]
    fn a_rejected_actions_title_is_sanitized_exactly_like_an_applicable_ones() -> TestResult {
        let hostile = "before\u{202E}after";
        let rejected = ActionSummary::Rejected {
            reason: ActionRejection::Command,
            title: Some(NonEmptyText::try_from(hostile.to_owned())?),
            kind: Some(CodeActionKind::QuickFix),
        };
        let schemas::CodeAction::Rejected { title, .. } = wire_action(&rejected) else {
            return Err("expected a rejected action".into());
        };
        assert_eq!(title.as_deref(), Some("before\u{fffd}after"));
        Ok(())
    }

    #[test]
    fn no_stderr_kill_reap_text_or_raw_control_characters_reach_the_wire() -> TestResult {
        let mut execution = code_actions_answered();
        execution.session = session(
            Some("SECRET_KILL_ERROR_should_never_leak"),
            Some("SECRET_REAP_ERROR_should_never_leak"),
        )?;
        let hostile = format!("Inline\u{1b}[31m\nvariable{}", "x".repeat(400));
        let answer = ActionsReport {
            report: report(execution)?,
            actions: vec![applicable(&hostile)],
        };
        let value = actions_output(Ok(answer), 1, 60)?;
        let encoded = serde_json::to_string(&value)?;
        assert!(!encoded.contains("SECRET_KILL_ERROR_should_never_leak"));
        assert!(!encoded.contains("SECRET_REAP_ERROR_should_never_leak"));
        assert!(!encoded.contains('\u{1b}'));
        let ActionsOutcome::Passed { data, .. } = &value.outcome else {
            return Err("expected passed".into());
        };
        let Some(schemas::CodeAction::Applicable {
            title,
            title_truncated,
            ..
        }) = data.actions.as_ref().and_then(|actions| actions.first())
        else {
            return Err("expected an applicable action".into());
        };
        assert_eq!(title.chars().count(), MAX_TITLE_SCALARS);
        assert!(title.starts_with("Inline\u{fffd}[31m\u{fffd}variable"));
        assert!(title_truncated);
        Ok(())
    }

    #[test]
    fn oversized_actions_are_trimmed_and_declared_then_fall_back_to_result_limit() -> TestResult {
        let actions = (0..32).map(|_| applicable(&"t".repeat(256))).collect();
        let contract = Contract::<ActionsInput, ActionsOutput>::new()?;
        let value = actions_output(Ok(answer(actions)), 1, 60)?;
        let full = serde_json::to_vec(&value)?.len();
        let encoded = encode_actions_bounded_within(&contract, value, 4 * (full - 1))?;
        let structured = serde_json::to_value(&encoded)?["structuredContent"].clone();
        assert_eq!(structured["status"], "passed");
        assert_eq!(structured["data"]["omitted"], 1);
        assert_eq!(
            structured["data"]["actions"].as_array().map(Vec::len),
            Some(31)
        );
        assert_eq!(structured["data"]["completeness"]["state"], "incomplete");
        assert!(
            structured["data"]["completeness"]["reasons"]
                .as_array()
                .ok_or("reasons")?
                .iter()
                .any(|reason| reason == "result_limit")
        );

        let value = actions_output(Ok(answer(vec![applicable("Inline")])), 1, 60)?;
        let encoded = encode_actions_bounded_within(&contract, value, 16)?;
        let structured = serde_json::to_value(&encoded)?["structuredContent"].clone();
        assert_eq!(structured["status"], "unavailable");
        assert_eq!(structured["error_code"], "RESULT_LIMIT");
        assert!(structured["data"].is_null());
        Ok(())
    }

    #[test]
    fn bootstrap_refusal_is_blocked_sandbox_denied_with_no_data() {
        let value = actions_bootstrap_refusal(7);
        assert_eq!(status_of(&value), "blocked");
        assert_eq!(code_of(&value), Some(ActionsCode::SandboxDenied));
        assert!(matches!(
            value.outcome,
            ActionsOutcome::Blocked { data: None, .. }
        ));
        assert_eq!(value.duration_ms, 7);
    }

    #[test]
    fn a_reversed_range_is_refused_and_duplicate_kinds_collapse() -> TestResult {
        let input = |range: serde_json::Value| -> Result<ActionsInput, Box<dyn std::error::Error>> {
            Ok(serde_json::from_value(serde_json::json!({
                "project_ref": "prj_00000000000000000000000000000001",
                "expected_project_fingerprint": format!("sha256:{}", "a".repeat(64)),
                "file": "src/lib.rs",
                "range": range,
                "only": ["quickfix", "refactor_inline", "quickfix"],
                "timeout_seconds": 900
            }))?)
        };
        let reversed = input(serde_json::json!({
            "start": {"line": 2, "column": 1},
            "end": {"line": 1, "column": 9}
        }))?;
        assert!(reversed.request().is_none());
        let request = input(serde_json::json!({
            "start": {"line": 2, "column": 9},
            "end": {"line": 2, "column": 9}
        }))?
        .request()
        .ok_or("a collapsed range is valid")?;
        assert_eq!(
            request.only,
            vec![CodeActionKind::QuickFix, CodeActionKind::RefactorInline]
        );
        assert_eq!(request.timeout_seconds, MAX_TIMEOUT_SECONDS);
        Ok(())
    }
}
