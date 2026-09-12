//! `rust.analyzer.symbols`: the first M6 analyzer tool (ADR-083, ADR-084).
#[allow(dead_code)]
pub(super) mod schemas;
use super::workers::{Joined, Workers, worker_error};
use super::{
    contract::{Contract, ToolOutput},
    project::Registry,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{
    ExecutionError, InspectionError, ProjectError,
    analyzer::{AnalyzerReport, AnalyzerRequestError, SymbolsRequest, SymbolsScope},
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
struct Input {
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
struct Output {
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
fn analyzer_joined_result(
    joined: Joined<AnalyzerReport, AnalyzerRequestError>,
) -> Result<AnalyzerReport, AnalyzerRequestError> {
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
impl AnalyzerTool {
    pub(super) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
    ) -> Result<Self, ErrorData> {
        let contract = Contract::<Input, Output>::new()?;
        let definition = Tool::new(
            NAME,
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
             and rename are not offered by this or any other tool.",
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

#[cfg(test)]
mod tests;
