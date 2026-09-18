//! `rust.analyzer.symbols`: the first M6 analyzer tool (ADR-083, ADR-084).
mod actions;
#[allow(dead_code)]
pub(super) mod schemas;
use super::workers::{Joined, Workers, worker_error};
use super::{
    contract::{Contract, ToolOutput},
    project::Registry,
};
pub(super) use actions::{ACTIONS_NAME, ActionsTool, definition as actions_definition};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{
    ExecutionError, InspectionError, ProjectError,
    analyzer::{
        AnalyzerReport, AnalyzerRequestError, DiagnosticsRequest, ReferencesRequest,
        SymbolsRequest, SymbolsScope,
    },
};
use rust_engineering_domain as domain;
use rust_engineering_domain::{
    AnalyzerFile, OperationalErrorCode, ProjectIdentityFingerprint, ProjectRef, SymbolQuery,
    ToolStatus,
};
use rust_engineering_execution::RustProjectInspector;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    num::NonZeroU32,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(super) const NAME: &str = "rust.analyzer.symbols";
// 180 s is the analyzer's own total-call ceiling (ADR-084 §8); the outer MCP
// worker deadline adds margin for registry resolve/capture around it so a
// legitimate `TIMEOUT_TOTAL` is reported by the analyzer itself, not raced by
// the outer join.
const DEADLINE: Duration = Duration::from_secs(200);
const MAX_RESULT: usize = 512 * 1024;
const MAX_TIMEOUT_SECONDS: u32 = 180;
const DEFAULT_TIMEOUT_SECONDS: u32 = 60;

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
enum Scope {
    Document {
        #[schemars(with = "String", regex(pattern = r"^[A-Za-z0-9_./-]{1,100}\.rs$"))]
        file: AnalyzerFile,
    },
    Workspace {
        #[schemars(with = "String", length(min = 1, max = 128))]
        query: SymbolQuery,
    },
}

fn default_timeout_seconds() -> u32 {
    DEFAULT_TIMEOUT_SECONDS
}

// `#[serde(deny_unknown_fields)]` stays declared here purely so
// `#[derive(JsonSchema)]` reads it and still closes the generated schema
// (`unevaluatedProperties: false`): `Deserialize` is hand-written below
// instead of derived, because deriving it with this attribute present would
// make serde reject the flattened `Scope` enum's own fields as "unknown"
// before flatten ever runs — a documented serde interaction between
// `flatten` and `deny_unknown_fields`, not a bug in this contract. The JSON
// Schema layer closes the object the same way either way.
#[derive(JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    #[serde(default)]
    #[schemars(with = "Option<String>", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    expected_project_fingerprint: Option<ProjectIdentityFingerprint>,
    #[serde(flatten)]
    scope: Scope,
    #[serde(default = "default_timeout_seconds")]
    #[schemars(range(min = 1, max = 180))]
    timeout_seconds: u32,
}
impl<'de> Deserialize<'de> for Input {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Shadow {
            project_ref: ProjectRef,
            #[serde(default)]
            expected_project_fingerprint: Option<ProjectIdentityFingerprint>,
            #[serde(flatten)]
            scope: Scope,
            #[serde(default = "default_timeout_seconds")]
            timeout_seconds: u32,
        }
        let shadow = Shadow::deserialize(deserializer)?;
        Ok(Self {
            project_ref: shadow.project_ref,
            expected_project_fingerprint: shadow.expected_project_fingerprint,
            scope: shadow.scope,
            timeout_seconds: shadow.timeout_seconds,
        })
    }
}
impl Input {
    fn request(self) -> SymbolsRequest {
        SymbolsRequest {
            expected_project_fingerprint: self.expected_project_fingerprint,
            scope: match self.scope {
                Scope::Document { file } => SymbolsScope::Document { file },
                Scope::Workspace { query } => SymbolsScope::Workspace { query },
            },
            timeout_seconds: self.timeout_seconds.min(MAX_TIMEOUT_SECONDS),
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Data {
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
    /// `None` exactly when the session never answered — `data` is still
    /// published (identity, readiness, completeness, session) but there is no
    /// result to report.
    symbols: Option<schemas::Symbols>,
    omitted: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Code {
    Conflict,
    FileNotInSnapshot,
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

// Statuses: `passed | blocked | unavailable | cancelled`. `failed` never
// occurs for this read-only tool (D2): even a project-triggered analyzer
// error (`ContentModified`) is `unavailable`, matching `rust.project.inspect`
// rather than `rust.check`.
#[derive(Serialize, JsonSchema)]
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
        data: Option<Box<Data>>,
    },
    Cancelled {
        error_code: (),
        error_message: (),
        data: (),
    },
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

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Output {
    #[serde(flatten)]
    outcome: Outcome,
    summary: &'static str,
    duration_ms: u64,
}

fn wire_position(value: domain::Position) -> schemas::Position {
    schemas::Position {
        line: value.line,
        column: value.column,
    }
}
fn wire_range(value: domain::TextRange) -> schemas::Range {
    schemas::Range {
        start: wire_position(value.start()),
        end: wire_position(value.end()),
    }
}
fn wire_document_symbol(value: &domain::DocumentSymbol) -> schemas::DocumentSymbol {
    schemas::DocumentSymbol {
        depth: value.depth(),
        name: value.name().as_str().to_owned(),
        kind: value.kind().into(),
        detail: value.detail().map(str::to_owned),
        detail_truncated: value.detail_truncated(),
        deprecated: value.deprecated(),
        range: wire_range(value.range()),
        selection_range: wire_range(value.selection_range()),
    }
}
fn wire_workspace_symbol(value: &domain::WorkspaceSymbol) -> schemas::WorkspaceSymbol {
    schemas::WorkspaceSymbol {
        name: value.name.as_str().to_owned(),
        kind: value.kind.into(),
        container: value.container.clone(),
        file: value.file.as_str().to_owned(),
        range: wire_range(value.range),
    }
}
fn wire_analyzer(execution: &domain::AnalyzerExecution) -> schemas::Analyzer {
    schemas::Analyzer {
        version: execution.identity.version.as_str().to_owned(),
        binary_sha256: execution.identity.binary_sha256.to_string(),
        image_id: execution.identity.image_id.as_str().to_owned(),
        config_digest: execution.identity.config_digest.to_string(),
        position_encoding: execution.position_encoding.map(Into::into),
    }
}
fn wire_readiness(value: domain::AnalyzerReadiness) -> schemas::Readiness {
    match value {
        domain::AnalyzerReadiness::Quiescent { elapsed_ms, health } => schemas::Readiness {
            state: schemas::ReadinessState::Quiescent,
            // `health: error` never reaches `Quiescent` (the domain readiness
            // loop returns `NotReady` first); an unrecognised spelling is
            // recorded as a warning rather than fabricated as `ok`.
            health: Some(match health {
                domain::ServerHealth::Ok => schemas::Health::Ok,
                domain::ServerHealth::Warning | domain::ServerHealth::Error => {
                    schemas::Health::Warning
                }
            }),
            elapsed_ms,
        },
        domain::AnalyzerReadiness::NotReady { elapsed_ms } => schemas::Readiness {
            state: schemas::ReadinessState::NotReady,
            health: None,
            elapsed_ms,
        },
    }
}
fn wire_completeness(value: &domain::Completeness) -> schemas::Completeness {
    schemas::Completeness {
        state: match value.state() {
            domain::CompletenessState::Complete => schemas::CompletenessState::Complete,
            domain::CompletenessState::Incomplete => schemas::CompletenessState::Incomplete,
        },
        omissions: value
            .omissions()
            .iter()
            .map(|omission| schemas::Omission {
                kind: omission.kind.into(),
                count: omission.count,
            })
            .collect(),
        reasons: value
            .reasons()
            .iter()
            .map(|reason| (*reason).into())
            .collect(),
    }
}
fn wire_session(value: &domain::SessionSummary) -> schemas::Session {
    schemas::Session {
        messages_in: value.messages_in,
        messages_out: value.messages_out,
        bytes_in: value.bytes_in,
        bytes_out: value.bytes_out,
        duration_ms: value.duration_ms,
        stderr_bytes: value.stderr_bytes,
        server_requests: value.server_requests_refused,
    }
}
/// The budgets actually enforced for this call (V05 P2), not the ADR-084 §8
/// ceilings: `total_timeout_seconds` is the caller's own `timeout_seconds`
/// (already clamped to `1..=180` by [`Input::request`]), and
/// `initialize`/`query` are the ADR ceilings only when the caller's total
/// leaves room for them — `LspSession::until` in `analyzer_gateway.rs` takes
/// the sooner of a phase's own budget and the session's overall deadline
/// (`started + total_timeout_seconds`), so a short total already cuts every
/// phase short in exactly this way. `max_visible`, `frame_bytes` and
/// `messages` are not caller-adjustable and stay the fixed ADR values.
fn wire_limits(total_timeout_seconds: u32) -> schemas::Limits {
    schemas::Limits {
        max_visible: domain::MAX_VISIBLE_RESULTS as u32,
        initialize_timeout_seconds: (domain::INITIALIZE_TIMEOUT_SECONDS as u32)
            .min(total_timeout_seconds),
        query_timeout_seconds: (domain::QUERY_TIMEOUT_SECONDS as u32).min(total_timeout_seconds),
        total_timeout_seconds,
        frame_bytes: domain::MAX_FRAME_BYTES as u32,
        messages: domain::MAX_MESSAGES_PER_JOB as u32,
    }
}

/// Every reason an analyzer session produced no answer, mapped onto the
/// closed wire vocabulary. `unavailable` for anything the guest/session/limit
/// classification names; `blocked` for what a caller could have avoided by
/// asking something this capture can answer.
///
/// Preconditions: never called with [`domain::AnalyzerFailure::Cancelled`] —
/// the caller maps that to `status: cancelled` first.
fn failure_code(failure: domain::AnalyzerFailure) -> Result<(Code, &'static str, bool), ErrorData> {
    use domain::AnalyzerFailure as F;
    Ok(match failure {
        F::FileNotInSnapshot => (
            Code::FileNotInSnapshot,
            "Queried file is absent from the capture",
            false,
        ),
        F::FileNotUtf8 => (
            Code::FileNotUtf8,
            "Queried file's captured bytes are not valid UTF-8",
            false,
        ),
        F::UnsupportedProjectConfig => (
            Code::UnsupportedProjectConfig,
            "Capture carries a rust-analyzer.toml workspace override",
            false,
        ),
        F::CapabilityMismatch => (
            Code::AnalyzerCapabilityMismatch,
            "Analyzer negotiated an unsupported position encoding",
            true,
        ),
        F::NotReady => (
            Code::AnalyzerNotReady,
            "Analyzer did not reach quiescent readiness in time",
            true,
        ),
        F::Crashed | F::ProtocolViolation | F::ServerError => (
            Code::AnalyzerCrashed,
            "Analyzer session ended without a valid answer",
            true,
        ),
        F::ProtocolLimit => (
            Code::MessageLimit,
            "Analyzer session exceeded its message or byte budget",
            true,
        ),
        // A peer-declared frame above the bound and any other refused header
        // are both frame-protocol faults from the caller's perspective.
        F::FrameTooLarge | F::MalformedHeader => (
            Code::FrameLimit,
            "Analyzer session exceeded the LSP frame budget",
            true,
        ),
        F::TimeoutInitialize => (
            Code::TimeoutInitialize,
            "Analyzer did not initialize in time",
            true,
        ),
        F::TimeoutQuery => (
            Code::TimeoutQuery,
            "Analyzer did not answer the query in time",
            true,
        ),
        F::TimeoutTotal => (
            Code::TimeoutTotal,
            "Analyzer call exceeded its total budget",
            true,
        ),
        // Unreachable for `rust.analyzer.symbols`: neither of its queries
        // carries a caller position, so the gateway's own precondition check
        // never produces this failure for this tool (see `wire_positions` in
        // `analyzer_gateway.rs`).
        F::PositionOutOfRange => {
            return Err(ErrorData::internal_error(
                "rust.analyzer.symbols never sends a position or range",
                None,
            ));
        }
        F::Cancelled => {
            return Err(ErrorData::internal_error(
                "cancellation must be classified before failure_code is called",
                None,
            ));
        }
    })
}

fn common_data(
    report: &AnalyzerReport,
    symbols: Option<schemas::Symbols>,
    omitted: u32,
    total_timeout_seconds: u32,
) -> Data {
    Data {
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
        symbols,
        omitted,
    }
}

fn execution_outcome(
    report: AnalyzerReport,
    total_timeout_seconds: u32,
) -> Result<(Outcome, &'static str), ErrorData> {
    match &report.execution.outcome {
        domain::AnalyzerOutcome::Answered(result) => {
            let omitted = report
                .execution
                .completeness
                .omissions()
                .iter()
                .filter(|omission| omission.kind == domain::OmissionKind::LimitVisible)
                .map(|omission| omission.count)
                .sum();
            let symbols = match result {
                domain::AnalyzerResult::DocumentSymbols(items) => {
                    schemas::Symbols::Document(items.iter().map(wire_document_symbol).collect())
                }
                domain::AnalyzerResult::WorkspaceSymbols(items) => {
                    schemas::Symbols::Workspace(items.iter().map(wire_workspace_symbol).collect())
                }
                // `rust.analyzer.symbols` only ever sends a `DocumentSymbols`
                // or `WorkspaceSymbols` query, and `AnalyzerResult::answers`
                // guarantees the answer's shape matches the question asked.
                domain::AnalyzerResult::References(_)
                | domain::AnalyzerResult::Diagnostics(_)
                | domain::AnalyzerResult::CodeActions(_) => {
                    return Err(ErrorData::internal_error(
                        "rust.analyzer.symbols received an answer to a different query",
                        None,
                    ));
                }
            };
            let data = common_data(&report, Some(symbols), omitted, total_timeout_seconds);
            Ok((
                Outcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: Box::new(data),
                },
                "rust-analyzer answered the symbols query",
            ))
        }
        domain::AnalyzerOutcome::Failed(domain::AnalyzerFailure::Cancelled) => Ok((
            Outcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Analyzer session cancelled",
        )),
        domain::AnalyzerOutcome::Failed(failure) => {
            let (code, message, unavailable) = failure_code(*failure)?;
            let data = Some(Box::new(common_data(
                &report,
                None,
                0,
                total_timeout_seconds,
            )));
            Ok((
                if unavailable {
                    Outcome::Unavailable {
                        error_code: code,
                        error_message: message,
                        data,
                    }
                } else {
                    Outcome::Blocked {
                        error_code: code,
                        error_message: message,
                        data,
                    }
                },
                message,
            ))
        }
    }
}

fn operational(code: OperationalErrorCode) -> (Outcome, &'static str) {
    let (code, message, unavailable) = match code {
        OperationalErrorCode::ProjectNotFound => (
            Code::ProjectNotFound,
            "Project reference is missing or expired",
            false,
        ),
        OperationalErrorCode::InvalidProject => (
            Code::InvalidProject,
            "Captured project is invalid or unsupported",
            false,
        ),
        OperationalErrorCode::ToolNotInstalled => (
            Code::SandboxDenied,
            "Approved analyzer runtime is unavailable",
            true,
        ),
        OperationalErrorCode::LockfileUpdateRequired => (
            Code::SandboxDenied,
            "Host runtime policy denied analyzer execution",
            true,
        ),
        // The outer MCP worker deadline, not an analyzer-session timeout: the
        // closest closed reason is the same one a session that ran out of its
        // own total budget would report.
        OperationalErrorCode::CommandTimeout => (
            Code::TimeoutTotal,
            "Analyzer call exceeded its total budget",
            true,
        ),
        OperationalErrorCode::SandboxDenied => (
            Code::SandboxDenied,
            "Host runtime policy, failed calibration or current capacity denied analysis",
            true,
        ),
        OperationalErrorCode::NetworkDenied => (
            Code::SandboxDenied,
            "Host runtime policy denied analyzer execution",
            true,
        ),
        OperationalErrorCode::UnsupportedPlatform => (
            Code::UnsupportedPlatform,
            "Secure analyzer session is unavailable on this platform",
            true,
        ),
        OperationalErrorCode::OutputLimitExceeded => (
            Code::OutputLimitExceeded,
            "Project metadata exceeds the response budget",
            false,
        ),
    };
    (
        if unavailable {
            Outcome::Unavailable {
                error_code: code,
                error_message: message,
                data: None,
            }
        } else {
            Outcome::Blocked {
                error_code: code,
                error_message: message,
                data: None,
            }
        },
        message,
    )
}

fn output(
    result: Result<AnalyzerReport, AnalyzerRequestError>,
    duration_ms: u64,
    total_timeout_seconds: u32,
) -> Result<Output, ErrorData> {
    let (outcome, summary) = match result {
        Ok(report) => execution_outcome(report, total_timeout_seconds)?,
        Err(AnalyzerRequestError::Conflict) => {
            let message = "expected_project_fingerprint does not match the live project identity";
            (
                Outcome::Blocked {
                    error_code: Code::Conflict,
                    error_message: message,
                    data: None,
                },
                message,
            )
        }
        Err(AnalyzerRequestError::FileNotInSnapshot) => {
            let message = "Queried file is absent from the capture";
            (
                Outcome::Blocked {
                    error_code: Code::FileNotInSnapshot,
                    error_message: message,
                    data: None,
                },
                message,
            )
        }
        // Unreachable for `rust.analyzer.symbols`: its request never carries a
        // position, so `ProjectRegistry::analyzer_symbols` never returns this.
        Err(AnalyzerRequestError::PositionOutOfRange) => {
            return Err(ErrorData::internal_error(
                "rust.analyzer.symbols never sends a position or range",
                None,
            ));
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::Project(
            ProjectError::Rejected(code),
        ))) => operational(code),
        Err(AnalyzerRequestError::Inspection(
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled),
        )) => (
            Outcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Analyzer symbols cancelled after worker completion",
        ),
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::Unavailable,
        ))) => operational(OperationalErrorCode::ToolNotInstalled),
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        ))) => operational(OperationalErrorCode::SandboxDenied),
        Err(AnalyzerRequestError::Inspection(InspectionError::OutputLimit)) => {
            operational(OperationalErrorCode::OutputLimitExceeded)
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::InvalidMetadata)) => {
            operational(OperationalErrorCode::InvalidProject)
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
        )) => return Err(ErrorData::internal_error("Analyzer symbols failed", None)),
    };
    Ok(Output {
        outcome,
        summary,
        duration_ms,
    })
}

/// The same "interrupted signal wins" priority as
/// `super::workers::joined_result`, generic over this tool's own request
/// error instead of the shared [`InspectionError`].
fn analyzer_joined_result<T>(
    joined: Joined<T, AnalyzerRequestError>,
) -> Result<T, AnalyzerRequestError> {
    match (joined.result, joined.interrupted) {
        (
            Err(AnalyzerRequestError::Inspection(
                InspectionError::Project(ProjectError::Cancelled)
                | InspectionError::Execution(ExecutionError::Cancelled),
            )),
            Some(signal),
        ) => Err(worker_error(signal).into()),
        (Err(error), _) => Err(error),
        (Ok(_), Some(signal)) => Err(worker_error(signal).into()),
        (Ok(value), None) => Ok(value),
    }
}

pub(super) struct AnalyzerTool {
    pub(super) definition: Tool,
    contract: Contract<Input, Output>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
}
pub(super) fn symbols_definition() -> Result<(Contract<Input, Output>, Tool), ErrorData> {
    let contract = Contract::<Input, Output>::new()?;
    let definition = Tool::new(
        NAME,
        format!(
            "{}{}",
            super::stability::PREVIEW_PREFIX,
            "Read symbols from a captured Rust project using the host-approved \
             rust-analyzer 1.98.1 (aarch64-unknown-linux-gnu) inside the M6 guest \
             image. Snapshot semantics are latest_known and non-atomic: the capture \
             is not a live filesystem view. Build scripts, proc macros and \
             check-on-save stay disabled; only textDocument/documentSymbol or \
             workspace/symbol run, never cargo check. Results are bounded to 512 \
             visible entries per call; an over-budget answer is reported \
             incomplete, never silently truncated. Positions are Unicode-scalar, \
             1-based Position values, never byte offsets or UTF-16 units. \
             Requires the host --rust runtime configured with the approved M6 \
             image; without it the tool is unavailable. Hover, go-to-definition \
             and rename are not offered by this or any other tool."
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

impl AnalyzerTool {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, ErrorData> {
        let (contract, definition) = symbols_definition()?;
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
        })
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let project_ref = input.project_ref.clone();
        let symbols_request = input.request();
        let total_timeout_seconds = symbols_request.timeout_seconds;
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
                        .analyzer_symbols(
                            &project_ref,
                            symbols_request,
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
            bootstrap_refusal(duration)
        } else {
            output(result, duration, total_timeout_seconds)?
        };
        encode_bounded(&self.contract, value)
    }
}

/// The house bootstrap refusal (identical to `rust.check` and
/// `rust.project.inspect`): before discovery completes, every guest tool
/// below `--rust` is `blocked/SANDBOX_DENIED` with this message, never
/// `unavailable` — a caller retries with a new request ID instead of treating
/// the runtime as permanently absent. Distinct from the `unavailable`
/// `SANDBOX_DENIED` [`operational`] publishes once discovery has completed
/// but the runtime is not configured, not the admitted M6 image, or denied by
/// host policy.
fn bootstrap_refusal(duration_ms: u64) -> Output {
    let message = "Analyzer symbols requires completed discovery; retry with a new request ID";
    Output {
        outcome: Outcome::Blocked {
            error_code: Code::SandboxDenied,
            error_message: message,
            data: None,
        },
        summary: message,
        duration_ms,
    }
}

fn encode_bounded(
    contract: &Contract<Input, Output>,
    value: Output,
) -> Result<CallToolResult, ErrorData> {
    encode_bounded_within(contract, value, MAX_RESULT)
}

/// [`encode_bounded`] with the output budget as a parameter: the product path
/// always uses [`MAX_RESULT`], and a test-only smaller budget exercises the
/// fallback below without needing 512 KiB of fixture data.
fn encode_bounded_within(
    contract: &Contract<Input, Output>,
    mut value: Output,
    max_result: usize,
) -> Result<CallToolResult, ErrorData> {
    // Trim visible symbols before encoding, exactly like `check.rs` trims
    // diagnostics: the declared `RESULT_LIMIT` reason survives, the raw JSON
    // never does.
    //
    // The `/ 4` margin (V05 P3): `contract.encode` below wraps this value's
    // JSON *twice* — once as `structuredContent` and once escaped inside
    // `content[0].text`, the SDK's "identical JSON text" mirror — so the
    // encoded `CallToolResult` this loop must fit under `max_result` is
    // already roughly 2x this value's own serialized size before counting
    // string-escaping or the envelope around both copies. Trimming while this
    // value alone is still under a quarter of `max_result` leaves that
    // roughly-2x duplication a further 2x of headroom, which the final
    // post-`encode` check below verifies rather than assumes.
    while serde_json::to_vec(&value)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > max_result / 4
    {
        let Outcome::Passed { data, .. } = &mut value.outcome else {
            break;
        };
        let Some(symbols) = &mut data.symbols else {
            break;
        };
        let trimmed = match symbols {
            schemas::Symbols::Document(items) => items.pop().is_some(),
            schemas::Symbols::Workspace(items) => items.pop().is_some(),
        };
        if !trimmed {
            break;
        }
        data.omitted += 1;
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
        // `unavailable`, not `blocked` (V05 P1): every symbol was already
        // trimmed by the loop above and the encoded result is still over
        // budget, so this is the adapter failing to retain an answer within
        // its own output contract, not something the caller could have asked
        // differently to avoid. The declared, still-`passed` trim above
        // (`completeness.reasons: result_limit`) is the normal path; this is
        // only the fallback when even an empty symbol list would not fit.
        let message = "Analyzer result could not be retained within the output budget";
        return contract.encode(Output {
            outcome: Outcome::Unavailable {
                error_code: Code::ResultLimit,
                error_message: message,
                data: None,
            },
            summary: message,
            duration_ms: duration,
        });
    }
    Ok(encoded)
}

// ---------------------------------------------------------------------
// `rust.analyzer.references` (M6-02, ADR-083 §2, ADR-084 §2 phase 6 amended)
// ---------------------------------------------------------------------

pub(super) const REFERENCES_NAME: &str = "rust.analyzer.references";

fn default_include_declaration() -> bool {
    true
}

/// A 1-based line and Unicode-scalar column, matching
/// `rust_engineering_domain::Position` field for field: JSON Schema `minimum:
/// 1` rejects `0` here, so [`domain::Position`]'s own `NonZeroU32` invariant
/// is never violated by a decoded value.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct WirePosition {
    line: NonZeroU32,
    column: NonZeroU32,
}
impl From<WirePosition> for domain::Position {
    fn from(value: WirePosition) -> Self {
        Self {
            line: value.line,
            column: value.column,
        }
    }
}

/// A `{start, end}` pair of [`WirePosition`]s, shared by
/// `rust.analyzer.actions` and `rust.analyzer.action.apply`. `start ≤ end` is
/// a Rust-level invariant ([`domain::TextRange::new`]) the schema cannot
/// express, so a reversed range is refused as invalid arguments after
/// decoding.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct WireRange {
    start: WirePosition,
    end: WirePosition,
}
impl WireRange {
    pub(super) fn into_domain(self) -> Option<domain::TextRange> {
        domain::TextRange::new(self.start.into(), self.end.into()).ok()
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReferencesInput {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    #[serde(default)]
    #[schemars(with = "Option<String>", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    expected_project_fingerprint: Option<ProjectIdentityFingerprint>,
    #[schemars(with = "String", regex(pattern = r"^[A-Za-z0-9_./-]{1,100}\.rs$"))]
    file: AnalyzerFile,
    position: WirePosition,
    #[serde(default = "default_include_declaration")]
    include_declaration: bool,
    #[serde(default = "default_timeout_seconds")]
    #[schemars(range(min = 1, max = 180))]
    timeout_seconds: u32,
}
impl ReferencesInput {
    fn request(self) -> ReferencesRequest {
        ReferencesRequest {
            expected_project_fingerprint: self.expected_project_fingerprint,
            file: self.file,
            position: self.position.into(),
            include_declaration: self.include_declaration,
            timeout_seconds: self.timeout_seconds.min(MAX_TIMEOUT_SECONDS),
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReferencesData {
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
    /// `None` exactly when the session never answered.
    #[schemars(length(max = 512))]
    references: Option<Vec<schemas::Reference>>,
    omitted: u32,
    /// Declaration locations dropped from `references` because the caller
    /// asked `include_declaration: false`; `0` whenever the caller asked
    /// `true` (the default), since none are then dropped.
    omitted_declarations: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ReferencesCode {
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

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ReferencesOutcome {
    Passed {
        error_code: (),
        error_message: (),
        data: Box<ReferencesData>,
    },
    Blocked {
        error_code: ReferencesCode,
        error_message: &'static str,
        data: Option<Box<ReferencesData>>,
    },
    Unavailable {
        error_code: ReferencesCode,
        error_message: &'static str,
        data: Option<Box<ReferencesData>>,
    },
    Cancelled {
        error_code: (),
        error_message: (),
        data: (),
    },
}
impl ToolOutput for ReferencesOutput {
    fn status(&self) -> ToolStatus {
        match self.outcome {
            ReferencesOutcome::Passed { .. } => ToolStatus::Passed,
            ReferencesOutcome::Blocked { .. } => ToolStatus::Blocked,
            ReferencesOutcome::Unavailable { .. } => ToolStatus::Unavailable,
            ReferencesOutcome::Cancelled { .. } => ToolStatus::Cancelled,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReferencesOutput {
    #[serde(flatten)]
    outcome: ReferencesOutcome,
    summary: &'static str,
    duration_ms: u64,
}

fn wire_reference(value: &domain::Reference) -> schemas::Reference {
    schemas::Reference {
        file: value.file.as_str().to_owned(),
        range: wire_range(value.range),
        is_declaration: value.is_declaration,
    }
}

/// Every `AnalyzerFailure` this tool can observe, mapped onto the closed wire
/// vocabulary. Unlike `rust.analyzer.symbols`, `PositionOutOfRange` is
/// reachable here (the query itself carries a caller position) and is
/// `blocked`: the same category as `FileNotInSnapshot`, a question this
/// capture could always have answered differently.
fn references_failure_code(
    failure: domain::AnalyzerFailure,
) -> Result<(ReferencesCode, &'static str, bool), ErrorData> {
    use domain::AnalyzerFailure as F;
    Ok(match failure {
        F::FileNotInSnapshot => (
            ReferencesCode::FileNotInSnapshot,
            "Queried file is absent from the capture",
            false,
        ),
        F::FileNotUtf8 => (
            ReferencesCode::FileNotUtf8,
            "Queried file's captured bytes are not valid UTF-8",
            false,
        ),
        F::UnsupportedProjectConfig => (
            ReferencesCode::UnsupportedProjectConfig,
            "Capture carries a rust-analyzer.toml workspace override",
            false,
        ),
        F::PositionOutOfRange => (
            ReferencesCode::PositionOutOfRange,
            "Queried position is outside the captured file",
            false,
        ),
        F::CapabilityMismatch => (
            ReferencesCode::AnalyzerCapabilityMismatch,
            "Analyzer negotiated an unsupported position encoding",
            true,
        ),
        F::NotReady => (
            ReferencesCode::AnalyzerNotReady,
            "Analyzer did not reach quiescent readiness in time",
            true,
        ),
        F::Crashed | F::ProtocolViolation | F::ServerError => (
            ReferencesCode::AnalyzerCrashed,
            "Analyzer session ended without a valid answer",
            true,
        ),
        F::ProtocolLimit => (
            ReferencesCode::MessageLimit,
            "Analyzer session exceeded its message or byte budget",
            true,
        ),
        F::FrameTooLarge | F::MalformedHeader => (
            ReferencesCode::FrameLimit,
            "Analyzer session exceeded the LSP frame budget",
            true,
        ),
        F::TimeoutInitialize => (
            ReferencesCode::TimeoutInitialize,
            "Analyzer did not initialize in time",
            true,
        ),
        F::TimeoutQuery => (
            ReferencesCode::TimeoutQuery,
            "Analyzer did not answer the query in time",
            true,
        ),
        F::TimeoutTotal => (
            ReferencesCode::TimeoutTotal,
            "Analyzer call exceeded its total budget",
            true,
        ),
        F::Cancelled => {
            return Err(ErrorData::internal_error(
                "cancellation must be classified before failure_code is called",
                None,
            ));
        }
    })
}

fn references_common_data(
    report: &AnalyzerReport,
    references: Option<Vec<schemas::Reference>>,
    omitted: u32,
    omitted_declarations: u32,
    total_timeout_seconds: u32,
) -> ReferencesData {
    ReferencesData {
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
        references,
        omitted,
        omitted_declarations,
    }
}

fn references_execution_outcome(
    report: AnalyzerReport,
    total_timeout_seconds: u32,
    include_declaration: bool,
) -> Result<(ReferencesOutcome, &'static str), ErrorData> {
    match &report.execution.outcome {
        domain::AnalyzerOutcome::Answered(result) => {
            // V06 P3: every omission counts here, not only `LimitVisible` —
            // for references, external/sysroot/dependency locations and
            // oversized entries are as common as the visible cap, and the
            // per-kind breakdown stays available in `completeness.omissions`.
            let omitted = report
                .execution
                .completeness
                .omissions()
                .iter()
                .map(|omission| omission.count)
                .sum();
            let domain::AnalyzerResult::References(items) = result else {
                return Err(ErrorData::internal_error(
                    "rust.analyzer.references received an answer to a different query",
                    None,
                ));
            };
            let (references, omitted_declarations) = if include_declaration {
                (items.iter().map(wire_reference).collect(), 0)
            } else {
                let mut visible = Vec::with_capacity(items.len());
                let mut removed = 0u32;
                for item in items {
                    if item.is_declaration {
                        removed = removed.saturating_add(1);
                    } else {
                        visible.push(wire_reference(item));
                    }
                }
                (visible, removed)
            };
            let data = references_common_data(
                &report,
                Some(references),
                omitted,
                omitted_declarations,
                total_timeout_seconds,
            );
            Ok((
                ReferencesOutcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: Box::new(data),
                },
                "rust-analyzer answered the references query",
            ))
        }
        domain::AnalyzerOutcome::Failed(domain::AnalyzerFailure::Cancelled) => Ok((
            ReferencesOutcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Analyzer session cancelled",
        )),
        domain::AnalyzerOutcome::Failed(failure) => {
            let (code, message, unavailable) = references_failure_code(*failure)?;
            let data = Some(Box::new(references_common_data(
                &report,
                None,
                0,
                0,
                total_timeout_seconds,
            )));
            Ok((
                if unavailable {
                    ReferencesOutcome::Unavailable {
                        error_code: code,
                        error_message: message,
                        data,
                    }
                } else {
                    ReferencesOutcome::Blocked {
                        error_code: code,
                        error_message: message,
                        data,
                    }
                },
                message,
            ))
        }
    }
}

fn references_operational(code: OperationalErrorCode) -> (ReferencesOutcome, &'static str) {
    let (code, message, unavailable) = match code {
        OperationalErrorCode::ProjectNotFound => (
            ReferencesCode::ProjectNotFound,
            "Project reference is missing or expired",
            false,
        ),
        OperationalErrorCode::InvalidProject => (
            ReferencesCode::InvalidProject,
            "Captured project is invalid or unsupported",
            false,
        ),
        OperationalErrorCode::ToolNotInstalled => (
            ReferencesCode::SandboxDenied,
            "Approved analyzer runtime is unavailable",
            true,
        ),
        OperationalErrorCode::LockfileUpdateRequired => (
            ReferencesCode::SandboxDenied,
            "Host runtime policy denied analyzer execution",
            true,
        ),
        OperationalErrorCode::CommandTimeout => (
            ReferencesCode::TimeoutTotal,
            "Analyzer call exceeded its total budget",
            true,
        ),
        OperationalErrorCode::SandboxDenied => (
            ReferencesCode::SandboxDenied,
            "Host runtime policy, failed calibration or current capacity denied analysis",
            true,
        ),
        OperationalErrorCode::NetworkDenied => (
            ReferencesCode::SandboxDenied,
            "Host runtime policy denied analyzer execution",
            true,
        ),
        OperationalErrorCode::UnsupportedPlatform => (
            ReferencesCode::UnsupportedPlatform,
            "Secure analyzer session is unavailable on this platform",
            true,
        ),
        OperationalErrorCode::OutputLimitExceeded => (
            ReferencesCode::OutputLimitExceeded,
            "Project metadata exceeds the response budget",
            false,
        ),
    };
    (
        if unavailable {
            ReferencesOutcome::Unavailable {
                error_code: code,
                error_message: message,
                data: None,
            }
        } else {
            ReferencesOutcome::Blocked {
                error_code: code,
                error_message: message,
                data: None,
            }
        },
        message,
    )
}

fn references_output(
    result: Result<AnalyzerReport, AnalyzerRequestError>,
    duration_ms: u64,
    total_timeout_seconds: u32,
    include_declaration: bool,
) -> Result<ReferencesOutput, ErrorData> {
    let (outcome, summary) = match result {
        Ok(report) => {
            references_execution_outcome(report, total_timeout_seconds, include_declaration)?
        }
        Err(AnalyzerRequestError::Conflict) => {
            let message = "expected_project_fingerprint does not match the live project identity";
            (
                ReferencesOutcome::Blocked {
                    error_code: ReferencesCode::Conflict,
                    error_message: message,
                    data: None,
                },
                message,
            )
        }
        Err(AnalyzerRequestError::FileNotInSnapshot) => {
            let message = "Queried file is absent from the capture";
            (
                ReferencesOutcome::Blocked {
                    error_code: ReferencesCode::FileNotInSnapshot,
                    error_message: message,
                    data: None,
                },
                message,
            )
        }
        Err(AnalyzerRequestError::PositionOutOfRange) => {
            let message = "Queried position is outside the captured file";
            (
                ReferencesOutcome::Blocked {
                    error_code: ReferencesCode::PositionOutOfRange,
                    error_message: message,
                    data: None,
                },
                message,
            )
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::Project(
            ProjectError::Rejected(code),
        ))) => references_operational(code),
        Err(AnalyzerRequestError::Inspection(
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled),
        )) => (
            ReferencesOutcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Analyzer references cancelled after worker completion",
        ),
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::Unavailable,
        ))) => references_operational(OperationalErrorCode::ToolNotInstalled),
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        ))) => references_operational(OperationalErrorCode::SandboxDenied),
        Err(AnalyzerRequestError::Inspection(InspectionError::OutputLimit)) => {
            references_operational(OperationalErrorCode::OutputLimitExceeded)
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::InvalidMetadata)) => {
            references_operational(OperationalErrorCode::InvalidProject)
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
        )) => {
            return Err(ErrorData::internal_error(
                "Analyzer references failed",
                None,
            ));
        }
    };
    Ok(ReferencesOutput {
        outcome,
        summary,
        duration_ms,
    })
}

pub(super) struct ReferencesTool {
    pub(super) definition: Tool,
    contract: Contract<ReferencesInput, ReferencesOutput>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
}
pub(super) fn references_definition()
-> Result<(Contract<ReferencesInput, ReferencesOutput>, Tool), ErrorData> {
    let contract = Contract::<ReferencesInput, ReferencesOutput>::new()?;
    let definition = Tool::new(
        REFERENCES_NAME,
        format!(
            "{}{}",
            super::stability::PREVIEW_PREFIX,
            "Find references to the symbol at a captured Rust file's position, using \
             the host-approved rust-analyzer 1.98.1 (aarch64-unknown-linux-gnu) inside \
             the M6 guest image. Snapshot semantics are latest_known and non-atomic. \
             Build scripts, proc macros and check-on-save stay disabled; only \
             textDocument/references runs, never cargo check. The same session sends \
             the request twice, once including the declaration and once excluding it, \
             so is_declaration reflects rust-analyzer's own answer rather than a guess; \
             when include_declaration is false, declaration locations are removed from \
             the visible list and counted in omitted_declarations instead. Results are \
             bounded to 512 visible entries per call; an over-budget answer is reported \
             incomplete, never silently truncated. Positions are Unicode-scalar, \
             1-based Position values, never byte offsets or UTF-16 units. Requires the \
             host --rust runtime configured with the approved M6 image; without it the \
             tool is unavailable. Hover, go-to-definition and rename are not offered by \
             this or any other tool."
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

impl ReferencesTool {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, ErrorData> {
        let (contract, definition) = references_definition()?;
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
        })
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let project_ref = input.project_ref.clone();
        let include_declaration = input.include_declaration;
        let references_request = input.request();
        let total_timeout_seconds = references_request.timeout_seconds;
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
                        .analyzer_references(
                            &project_ref,
                            references_request,
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
            references_bootstrap_refusal(duration)
        } else {
            references_output(result, duration, total_timeout_seconds, include_declaration)?
        };
        encode_references_bounded(&self.contract, value)
    }
}

fn references_bootstrap_refusal(duration_ms: u64) -> ReferencesOutput {
    let message = "Analyzer references requires completed discovery; retry with a new request ID";
    ReferencesOutput {
        outcome: ReferencesOutcome::Blocked {
            error_code: ReferencesCode::SandboxDenied,
            error_message: message,
            data: None,
        },
        summary: message,
        duration_ms,
    }
}

fn encode_references_bounded(
    contract: &Contract<ReferencesInput, ReferencesOutput>,
    value: ReferencesOutput,
) -> Result<CallToolResult, ErrorData> {
    encode_references_bounded_within(contract, value, MAX_RESULT)
}

fn encode_references_bounded_within(
    contract: &Contract<ReferencesInput, ReferencesOutput>,
    mut value: ReferencesOutput,
    max_result: usize,
) -> Result<CallToolResult, ErrorData> {
    while serde_json::to_vec(&value)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > max_result / 4
    {
        let ReferencesOutcome::Passed { data, .. } = &mut value.outcome else {
            break;
        };
        let Some(references) = &mut data.references else {
            break;
        };
        if references.pop().is_none() {
            break;
        }
        data.omitted += 1;
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
        return contract.encode(ReferencesOutput {
            outcome: ReferencesOutcome::Unavailable {
                error_code: ReferencesCode::ResultLimit,
                error_message: message,
                data: None,
            },
            summary: message,
            duration_ms: duration,
        });
    }
    Ok(encoded)
}

// ---------------------------------------------------------------------
// `rust.analyzer.diagnostics` (M6-03, ADR-083 §2)
// ---------------------------------------------------------------------

pub(super) const DIAGNOSTICS_NAME: &str = "rust.analyzer.diagnostics";

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DiagnosticsInput {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    #[serde(default)]
    #[schemars(with = "Option<String>", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    expected_project_fingerprint: Option<ProjectIdentityFingerprint>,
    #[schemars(with = "String", regex(pattern = r"^[A-Za-z0-9_./-]{1,100}\.rs$"))]
    file: AnalyzerFile,
    #[serde(default = "default_timeout_seconds")]
    #[schemars(range(min = 1, max = 180))]
    timeout_seconds: u32,
}
impl DiagnosticsInput {
    fn request(self) -> DiagnosticsRequest {
        DiagnosticsRequest {
            expected_project_fingerprint: self.expected_project_fingerprint,
            file: self.file,
            timeout_seconds: self.timeout_seconds.min(MAX_TIMEOUT_SECONDS),
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DiagnosticsData {
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
    /// `None` exactly when the session never answered.
    #[schemars(length(max = 512))]
    diagnostics: Option<Vec<schemas::AnalyzerDiagnostic>>,
    omitted: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum DiagnosticsCode {
    Conflict,
    FileNotInSnapshot,
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

#[derive(Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
enum DiagnosticsOutcome {
    Passed {
        error_code: (),
        error_message: (),
        data: Box<DiagnosticsData>,
    },
    Blocked {
        error_code: DiagnosticsCode,
        error_message: &'static str,
        data: Option<Box<DiagnosticsData>>,
    },
    Unavailable {
        error_code: DiagnosticsCode,
        error_message: &'static str,
        data: Option<Box<DiagnosticsData>>,
    },
    Cancelled {
        error_code: (),
        error_message: (),
        data: (),
    },
}
impl ToolOutput for DiagnosticsOutput {
    fn status(&self) -> ToolStatus {
        match self.outcome {
            DiagnosticsOutcome::Passed { .. } => ToolStatus::Passed,
            DiagnosticsOutcome::Blocked { .. } => ToolStatus::Blocked,
            DiagnosticsOutcome::Unavailable { .. } => ToolStatus::Unavailable,
            DiagnosticsOutcome::Cancelled { .. } => ToolStatus::Cancelled,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DiagnosticsOutput {
    #[serde(flatten)]
    outcome: DiagnosticsOutcome,
    summary: &'static str,
    duration_ms: u64,
}

/// Whether `ch` is neutralised on peer-controlled wire text: every
/// `char::is_control` (Cc) character, plus the bidi overrides and isolates
/// (U+202A–U+202E, U+2066–U+2069), the zero-width characters (U+200B–U+200D,
/// U+FEFF) and the line/paragraph separators (U+2028–U+2029). None of the
/// latter is `Cc`, so `char::is_control` alone lets them reach the wire.
pub(super) fn is_peer_text_hazard(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}'
                | '\u{200B}'..='\u{200D}'
                | '\u{FEFF}'
                | '\u{2028}'..='\u{2029}'
        )
}

/// Every control character other than `\n`/`\t`, plus every other peer-text
/// hazard (see [`is_peer_text_hazard`]), is replaced, never carried onto the
/// wire (D25 §1.6): unlike a symbol or reference name, a diagnostic `message`
/// is free-form project text and this is its only sanitization. Bounded to
/// [`MAX_DIAGNOSTIC_MESSAGE_SCALARS`] scalars, flagging `message_truncated`
/// rather than silently cutting — defensive here since
/// `domain::AnalyzerDiagnostic` already refuses a longer message at
/// construction, but never assumed.
const MAX_DIAGNOSTIC_MESSAGE_SCALARS: usize = 4_096;

fn bounded_message(text: &str) -> (String, bool) {
    let sanitized: String = text
        .chars()
        .map(|ch| {
            if ch != '\n' && ch != '\t' && is_peer_text_hazard(ch) {
                '\u{fffd}'
            } else {
                ch
            }
        })
        .collect();
    if sanitized.chars().count() > MAX_DIAGNOSTIC_MESSAGE_SCALARS {
        (
            sanitized
                .chars()
                .take(MAX_DIAGNOSTIC_MESSAGE_SCALARS)
                .collect(),
            true,
        )
    } else {
        (sanitized, false)
    }
}

fn wire_related_information(value: &domain::RelatedInformation) -> schemas::RelatedInformation {
    let (message, message_truncated) = bounded_message(value.message.as_str());
    schemas::RelatedInformation {
        file: value.file.as_str().to_owned(),
        range: wire_range(value.range),
        message,
        message_truncated,
    }
}

/// `code` gets the same control-character sanitization as `message` (V06
/// P2): it too is peer-controlled free text (a numeric LSP diagnostic code is
/// coerced to a decimal string upstream, but a `string` code is whatever the
/// server sent), and only `message` was sanitized before this fix.
fn wire_diagnostic(value: &domain::AnalyzerDiagnostic) -> schemas::AnalyzerDiagnostic {
    let (message, locally_truncated) = bounded_message(value.message().as_str());
    // `value.message_truncated()` is the signal that matters now: the codec
    // fits `message` to the domain bound *before* constructing the
    // diagnostic (V06 P1), so this recomputation almost never finds anything
    // left to cut on its own — it stays only as defense in depth.
    let message_truncated = value.message_truncated() || locally_truncated;
    let code = value.code().map(|code| bounded_message(code).0);
    schemas::AnalyzerDiagnostic {
        file: value.file().as_str().to_owned(),
        range: wire_range(value.range()),
        severity: value.severity().into(),
        code,
        source: "rust-analyzer",
        message,
        message_truncated,
        related: value
            .related()
            .iter()
            .map(wire_related_information)
            .collect(),
    }
}

/// Unlike `rust.analyzer.references`, no query this tool sends carries a
/// caller position, so `PositionOutOfRange` stays unreachable exactly as for
/// `rust.analyzer.symbols`.
fn diagnostics_failure_code(
    failure: domain::AnalyzerFailure,
) -> Result<(DiagnosticsCode, &'static str, bool), ErrorData> {
    use domain::AnalyzerFailure as F;
    Ok(match failure {
        F::FileNotInSnapshot => (
            DiagnosticsCode::FileNotInSnapshot,
            "Queried file is absent from the capture",
            false,
        ),
        F::FileNotUtf8 => (
            DiagnosticsCode::FileNotUtf8,
            "Queried file's captured bytes are not valid UTF-8",
            false,
        ),
        F::UnsupportedProjectConfig => (
            DiagnosticsCode::UnsupportedProjectConfig,
            "Capture carries a rust-analyzer.toml workspace override",
            false,
        ),
        F::CapabilityMismatch => (
            DiagnosticsCode::AnalyzerCapabilityMismatch,
            "Analyzer negotiated an unsupported position encoding",
            true,
        ),
        F::NotReady => (
            DiagnosticsCode::AnalyzerNotReady,
            "Analyzer did not reach quiescent readiness in time",
            true,
        ),
        F::Crashed | F::ProtocolViolation | F::ServerError => (
            DiagnosticsCode::AnalyzerCrashed,
            "Analyzer session ended without a valid answer",
            true,
        ),
        F::ProtocolLimit => (
            DiagnosticsCode::MessageLimit,
            "Analyzer session exceeded its message or byte budget",
            true,
        ),
        F::FrameTooLarge | F::MalformedHeader => (
            DiagnosticsCode::FrameLimit,
            "Analyzer session exceeded the LSP frame budget",
            true,
        ),
        F::TimeoutInitialize => (
            DiagnosticsCode::TimeoutInitialize,
            "Analyzer did not initialize in time",
            true,
        ),
        F::TimeoutQuery => (
            DiagnosticsCode::TimeoutQuery,
            "Analyzer did not answer the query in time",
            true,
        ),
        F::TimeoutTotal => (
            DiagnosticsCode::TimeoutTotal,
            "Analyzer call exceeded its total budget",
            true,
        ),
        F::PositionOutOfRange => {
            return Err(ErrorData::internal_error(
                "rust.analyzer.diagnostics never sends a position or range",
                None,
            ));
        }
        F::Cancelled => {
            return Err(ErrorData::internal_error(
                "cancellation must be classified before failure_code is called",
                None,
            ));
        }
    })
}

fn diagnostics_common_data(
    report: &AnalyzerReport,
    diagnostics: Option<Vec<schemas::AnalyzerDiagnostic>>,
    omitted: u32,
    total_timeout_seconds: u32,
) -> DiagnosticsData {
    DiagnosticsData {
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
        diagnostics,
        omitted,
    }
}

fn diagnostics_execution_outcome(
    report: AnalyzerReport,
    total_timeout_seconds: u32,
) -> Result<(DiagnosticsOutcome, &'static str), ErrorData> {
    match &report.execution.outcome {
        domain::AnalyzerOutcome::Answered(result) => {
            // V06 P3: every omission counts here, not only `LimitVisible` —
            // an unresolvable position or an oversized entry is as much a
            // hole in the answer, and the per-kind breakdown stays available
            // in `completeness.omissions`.
            let omitted = report
                .execution
                .completeness
                .omissions()
                .iter()
                .map(|omission| omission.count)
                .sum();
            let domain::AnalyzerResult::Diagnostics(items) = result else {
                return Err(ErrorData::internal_error(
                    "rust.analyzer.diagnostics received an answer to a different query",
                    None,
                ));
            };
            let diagnostics = items.iter().map(wire_diagnostic).collect();
            let data =
                diagnostics_common_data(&report, Some(diagnostics), omitted, total_timeout_seconds);
            Ok((
                DiagnosticsOutcome::Passed {
                    error_code: (),
                    error_message: (),
                    data: Box::new(data),
                },
                "rust-analyzer answered the diagnostics query",
            ))
        }
        domain::AnalyzerOutcome::Failed(domain::AnalyzerFailure::Cancelled) => Ok((
            DiagnosticsOutcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Analyzer session cancelled",
        )),
        domain::AnalyzerOutcome::Failed(failure) => {
            let (code, message, unavailable) = diagnostics_failure_code(*failure)?;
            let data = Some(Box::new(diagnostics_common_data(
                &report,
                None,
                0,
                total_timeout_seconds,
            )));
            Ok((
                if unavailable {
                    DiagnosticsOutcome::Unavailable {
                        error_code: code,
                        error_message: message,
                        data,
                    }
                } else {
                    DiagnosticsOutcome::Blocked {
                        error_code: code,
                        error_message: message,
                        data,
                    }
                },
                message,
            ))
        }
    }
}

fn diagnostics_operational(code: OperationalErrorCode) -> (DiagnosticsOutcome, &'static str) {
    let (code, message, unavailable) = match code {
        OperationalErrorCode::ProjectNotFound => (
            DiagnosticsCode::ProjectNotFound,
            "Project reference is missing or expired",
            false,
        ),
        OperationalErrorCode::InvalidProject => (
            DiagnosticsCode::InvalidProject,
            "Captured project is invalid or unsupported",
            false,
        ),
        OperationalErrorCode::ToolNotInstalled => (
            DiagnosticsCode::SandboxDenied,
            "Approved analyzer runtime is unavailable",
            true,
        ),
        OperationalErrorCode::LockfileUpdateRequired => (
            DiagnosticsCode::SandboxDenied,
            "Host runtime policy denied analyzer execution",
            true,
        ),
        OperationalErrorCode::CommandTimeout => (
            DiagnosticsCode::TimeoutTotal,
            "Analyzer call exceeded its total budget",
            true,
        ),
        OperationalErrorCode::SandboxDenied => (
            DiagnosticsCode::SandboxDenied,
            "Host runtime policy, failed calibration or current capacity denied analysis",
            true,
        ),
        OperationalErrorCode::NetworkDenied => (
            DiagnosticsCode::SandboxDenied,
            "Host runtime policy denied analyzer execution",
            true,
        ),
        OperationalErrorCode::UnsupportedPlatform => (
            DiagnosticsCode::UnsupportedPlatform,
            "Secure analyzer session is unavailable on this platform",
            true,
        ),
        OperationalErrorCode::OutputLimitExceeded => (
            DiagnosticsCode::OutputLimitExceeded,
            "Project metadata exceeds the response budget",
            false,
        ),
    };
    (
        if unavailable {
            DiagnosticsOutcome::Unavailable {
                error_code: code,
                error_message: message,
                data: None,
            }
        } else {
            DiagnosticsOutcome::Blocked {
                error_code: code,
                error_message: message,
                data: None,
            }
        },
        message,
    )
}

fn diagnostics_output(
    result: Result<AnalyzerReport, AnalyzerRequestError>,
    duration_ms: u64,
    total_timeout_seconds: u32,
) -> Result<DiagnosticsOutput, ErrorData> {
    let (outcome, summary) = match result {
        Ok(report) => diagnostics_execution_outcome(report, total_timeout_seconds)?,
        Err(AnalyzerRequestError::Conflict) => {
            let message = "expected_project_fingerprint does not match the live project identity";
            (
                DiagnosticsOutcome::Blocked {
                    error_code: DiagnosticsCode::Conflict,
                    error_message: message,
                    data: None,
                },
                message,
            )
        }
        Err(AnalyzerRequestError::FileNotInSnapshot) => {
            let message = "Queried file is absent from the capture";
            (
                DiagnosticsOutcome::Blocked {
                    error_code: DiagnosticsCode::FileNotInSnapshot,
                    error_message: message,
                    data: None,
                },
                message,
            )
        }
        Err(AnalyzerRequestError::PositionOutOfRange) => {
            return Err(ErrorData::internal_error(
                "rust.analyzer.diagnostics never queries a position",
                None,
            ));
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::Project(
            ProjectError::Rejected(code),
        ))) => diagnostics_operational(code),
        Err(AnalyzerRequestError::Inspection(
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled),
        )) => (
            DiagnosticsOutcome::Cancelled {
                error_code: (),
                error_message: (),
                data: (),
            },
            "Analyzer diagnostics cancelled after worker completion",
        ),
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::Unavailable,
        ))) => diagnostics_operational(OperationalErrorCode::ToolNotInstalled),
        Err(AnalyzerRequestError::Inspection(InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        ))) => diagnostics_operational(OperationalErrorCode::SandboxDenied),
        Err(AnalyzerRequestError::Inspection(InspectionError::OutputLimit)) => {
            diagnostics_operational(OperationalErrorCode::OutputLimitExceeded)
        }
        Err(AnalyzerRequestError::Inspection(InspectionError::InvalidMetadata)) => {
            diagnostics_operational(OperationalErrorCode::InvalidProject)
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
        )) => {
            return Err(ErrorData::internal_error(
                "Analyzer diagnostics failed",
                None,
            ));
        }
    };
    Ok(DiagnosticsOutput {
        outcome,
        summary,
        duration_ms,
    })
}

pub(super) struct DiagnosticsTool {
    pub(super) definition: Tool,
    contract: Contract<DiagnosticsInput, DiagnosticsOutput>,
    registry: Arc<Mutex<Registry>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
}
pub(super) fn diagnostics_definition()
-> Result<(Contract<DiagnosticsInput, DiagnosticsOutput>, Tool), ErrorData> {
    let contract = Contract::<DiagnosticsInput, DiagnosticsOutput>::new()?;
    let definition = Tool::new(
        DIAGNOSTICS_NAME,
        format!(
            "{}{}",
            super::stability::PREVIEW_PREFIX,
            "Read native rust-analyzer diagnostics for a captured Rust file, using the \
             host-approved rust-analyzer 1.98.1 (aarch64-unknown-linux-gnu) inside the \
             M6 guest image. Snapshot semantics are latest_known and non-atomic. Build \
             scripts, proc macros and check-on-save stay disabled; only the pull \
             textDocument/diagnostic request runs, never cargo check — these are \
             analyzer-native diagnostics, distinct from rust.check. Results are bounded \
             to 512 visible entries per call; an over-budget answer is reported \
             incomplete, never silently truncated. message is project-derived text, \
             bounded to 4,096 Unicode scalars with message_truncated flagging a cut and \
             control characters other than newline/tab replaced; it is never the \
             server's own status message or stderr. Requires the host --rust runtime \
             configured with the approved M6 image; without it the tool is unavailable."
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

impl DiagnosticsTool {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, ErrorData> {
        let (contract, definition) = diagnostics_definition()?;
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
        })
    }
    pub(super) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let project_ref = input.project_ref.clone();
        let diagnostics_request = input.request();
        let total_timeout_seconds = diagnostics_request.timeout_seconds;
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
                        .analyzer_diagnostics(
                            &project_ref,
                            diagnostics_request,
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
            diagnostics_bootstrap_refusal(duration)
        } else {
            diagnostics_output(result, duration, total_timeout_seconds)?
        };
        encode_diagnostics_bounded(&self.contract, value)
    }
}

fn diagnostics_bootstrap_refusal(duration_ms: u64) -> DiagnosticsOutput {
    let message = "Analyzer diagnostics requires completed discovery; retry with a new request ID";
    DiagnosticsOutput {
        outcome: DiagnosticsOutcome::Blocked {
            error_code: DiagnosticsCode::SandboxDenied,
            error_message: message,
            data: None,
        },
        summary: message,
        duration_ms,
    }
}

fn encode_diagnostics_bounded(
    contract: &Contract<DiagnosticsInput, DiagnosticsOutput>,
    value: DiagnosticsOutput,
) -> Result<CallToolResult, ErrorData> {
    encode_diagnostics_bounded_within(contract, value, MAX_RESULT)
}

fn encode_diagnostics_bounded_within(
    contract: &Contract<DiagnosticsInput, DiagnosticsOutput>,
    mut value: DiagnosticsOutput,
    max_result: usize,
) -> Result<CallToolResult, ErrorData> {
    while serde_json::to_vec(&value)
        .map_err(|_| ErrorData::internal_error("Response encoding failed", None))?
        .len()
        > max_result / 4
    {
        let DiagnosticsOutcome::Passed { data, .. } = &mut value.outcome else {
            break;
        };
        let Some(diagnostics) = &mut data.diagnostics else {
            break;
        };
        if diagnostics.pop().is_none() {
            break;
        }
        data.omitted += 1;
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
        return contract.encode(DiagnosticsOutput {
            outcome: DiagnosticsOutcome::Unavailable {
                error_code: DiagnosticsCode::ResultLimit,
                error_message: message,
                data: None,
            },
            summary: message,
            duration_ms: duration,
        });
    }
    Ok(encoded)
}

// The `expect`/`unwrap` allow used to blanket this whole module (V06 P3);
// it now lives on `tests::new_tools` alone, since that is the only part that
// needs it (fixed fixtures are malformed only by mistake, and should fail
// immediately) — the M6-01 symbols tests above it get no such leniency.
#[cfg(test)]
mod tests;
