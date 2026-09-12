//! The M6 analyzer lifecycle: one transient rust-analyzer per query, inside the
//! single execution gateway (ADR-084 §2).
//!
//! Seven phases, all under the existing `WorkBudget` and the gateway's
//! single-flight lock: capture (the caller's, ADR-031), volume plus ingest
//! (existing phases), the new `Phase::Analyzer` container, `initialize` →
//! `initialized` → the readiness oracle, `textDocument/didOpen` of the exact
//! captured bytes, **one** request, then `shutdown`/`exit` with the container's
//! kill, removal and absence check joined before the lock is released.
//!
//! Three properties of this module are load-bearing and are stated once, here:
//!
//! * **Admission.** Only the ADR-085 digest may run an analyzer session, and any
//!   other image is refused before a container exists. The version and binary
//!   digest published with every result are properties of that digest (see
//!   [`ANALYZER_VERSION`]).
//! * **One request.** The query type decides the single request. There is no
//!   second round trip, no `codeAction/resolve` and no `didChange`; a different
//!   question is a new call with a new capture.
//! * **No peer text escapes.** The server's stderr is reduced to a length and a
//!   digest by the session, and every answer travels through the closed DTOs of
//!   [`crate::lsp_codec`] into validated domain values. No `serde_json::Value`
//!   reaches a caller.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use rust_engineering_domain as domain;
use rust_engineering_domain::{AnalyzerQuery, SourceBundle};

use super::rust_gateway::{Phase, PhaseRequest, RustGateway, Volume, WorkBudget, labels};
use super::*;
use crate::lsp_codec;
use crate::lsp_session::{Event, LspSession, SessionBudget, SessionError, SessionOutcome};

/// The exact M6 runtime built by ADR-082 and admitted by ADR-085: the M5 image
/// plus `rust-analyzer` 1.98.1 and `rust-src` 1.98.1. No other image may run an
/// analyzer session, because [`ANALYZER_VERSION`] and
/// [`ANALYZER_BINARY_SHA256`] are properties of this digest alone.
pub const APPROVED_M6_IMAGE: &str =
    "sha256:f39a5b33ee7d54243664162eb635f8ec223d512042beb7cd18ecf071046b310c";

/// The real `rust-analyzer --version` line of [`APPROVED_M6_IMAGE`].
///
/// Pinned rather than probed per call, deliberately: the version is a property
/// of an image admitted **by digest**, so a binary with a different version line
/// is a different image and is already refused. Probing it would add a container
/// to every query to learn something the digest already fixes.
///
/// What makes this a measurement rather than a claim is the native calibration,
/// which reads `/usr/share/doc/rust-runtime/m6/rust-analyzer-version.txt` and
/// `installed.json` out of the live guest through the closed
/// `Phase::AnalyzerDocument` phase and fails unless this constant and
/// [`ANALYZER_BINARY_SHA256`] match both the guest and
/// `docs/validation/M6/provisioning.json`.
pub(super) const ANALYZER_VERSION: &str = "rust-analyzer 1.98.1 (48a229c 2026-09-01)";

/// The sha256 of `/opt/analyzer/bin/rust-analyzer` inside
/// [`APPROVED_M6_IMAGE`]. Same standing as [`ANALYZER_VERSION`].
pub(super) const ANALYZER_BINARY_SHA256: &str =
    "sha256:a0c3f11a153e5d5f12a6de6ceabd2def60c2793293e901a4a045946050456e2f";

/// The guest workspace root every URI is relative to.
const SOURCE_URI: &str = "file:///source";

/// The shutdown grace of ADR-084 §2 phase 7.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// The sha256 of the fixed `initializationOptions` actually sent (ADR-084 §3).
///
/// Published with every result so a runtime rollback invalidates an action plan
/// by comparison instead of being applied against a different configuration.
pub fn analyzer_config_digest() -> Result<domain::SourceFingerprint, ExecutionError> {
    lsp_codec::config_digest().map_err(|_| ExecutionError::Infrastructure)
}

/// The per-phase budgets of ADR-084 §8.
///
/// The product path always uses [`Self::standard`]. A *smaller* initialize
/// budget is representable because the calibration needs one to exercise the
/// never-ready path; the ADR values are ceilings, never floors, and no
/// constructor can exceed them.
#[derive(Clone, Copy, Debug)]
pub(super) struct AnalyzerBudgets {
    limits: ExecutionLimits,
    initialize: Duration,
    query: Duration,
}

impl AnalyzerBudgets {
    pub(super) fn standard(limits: ExecutionLimits) -> Self {
        Self {
            limits,
            initialize: Duration::from_secs(domain::INITIALIZE_TIMEOUT_SECONDS),
            query: Duration::from_secs(domain::QUERY_TIMEOUT_SECONDS),
        }
    }

    /// The same budgets with a tighter initialize phase. `None` when the value
    /// is zero or above the ADR-084 §8 ceiling.
    #[cfg(test)]
    pub(super) fn with_initialize(limits: ExecutionLimits, initialize: Duration) -> Option<Self> {
        let ceiling = Duration::from_secs(domain::INITIALIZE_TIMEOUT_SECONDS);
        if initialize.is_zero() || initialize > ceiling {
            return None;
        }
        Some(Self {
            initialize,
            ..Self::standard(limits)
        })
    }
}

/// One captured file, ready to be opened at `version: 1`.
struct Document {
    file: domain::AnalyzerFile,
    text: String,
    index: domain::LineIndex,
}

/// What the conversation achieved, before cleanup decides the termination.
struct Conversation {
    /// `None` when the call never reached a session.
    session: Option<SessionOutcome>,
    encoding: Option<domain::PositionEncoding>,
    readiness: domain::AnalyzerReadiness,
    /// `None` when the budget or the token ended the call during setup; the
    /// caller names that failure from the terminal signal instead.
    outcome: Option<domain::AnalyzerOutcome>,
    reasons: Vec<domain::IncompleteReason>,
    omissions: Vec<domain::Omission>,
    /// The container's own verdict, read before cleanup removed it. `None` when
    /// it could not be observed — never `false` by default.
    oom_killed: Option<bool>,
    /// The phase the conversation had reached, so a terminal session fault is
    /// published as the timeout it actually was.
    stage: Stage,
}

impl Conversation {
    /// A call that stopped before any session existed.
    fn unstarted() -> Self {
        Self {
            session: None,
            encoding: None,
            readiness: domain::AnalyzerReadiness::NotReady { elapsed_ms: 0 },
            outcome: None,
            reasons: Vec::new(),
            omissions: Vec::new(),
            oom_killed: None,
            stage: Stage::Initialize,
        }
    }
}

pub fn execute(
    gateway: &RustGateway,
    source: &SourceBundle,
    query: &AnalyzerQuery,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<domain::AnalyzerExecution, ExecutionError> {
    execute_bounded(
        gateway,
        source,
        query,
        AnalyzerBudgets::standard(limits),
        cancel,
    )
}

/// The whole lifecycle, with explicit budgets.
///
/// Preconditions: `gateway` runs [`APPROVED_M6_IMAGE`]; anything else is
/// `Unavailable` before a container exists.
///
/// Postconditions: on `Ok` the analyzer container and the source volume have
/// been removed and verified absent before the single-flight lock was released;
/// on an uncertain cleanup the gateway is quarantined and the error says so. A
/// session that produced no answer returns `Ok` with a named
/// [`domain::AnalyzerFailure`] rather than an `Err`: the evidence of *how* it
/// failed is the point of the call.
pub(super) fn execute_bounded(
    gateway: &RustGateway,
    source: &SourceBundle,
    query: &AnalyzerQuery,
    budgets: AnalyzerBudgets,
    cancel: &dyn ExecutionCancellation,
) -> Result<domain::AnalyzerExecution, ExecutionError> {
    // Admission first: before the lock, before a volume, before a container.
    if gateway.image_id() != APPROVED_M6_IMAGE {
        return Err(ExecutionError::Unavailable);
    }
    let started = Instant::now();
    let _busy = gateway.hold_busy()?;
    if gateway.is_quarantined() {
        return Err(ExecutionError::CleanupUncertain);
    }
    if gateway.calibrating.load(Ordering::Acquire) || !gateway.verified.load(Ordering::Acquire) {
        return Err(ExecutionError::Denied);
    }
    gateway.approved_runtime(cancel)?;
    let identity = runtime_identity(gateway)?;

    // Defence in depth for ADR-084 §6. The hardened capture already refuses a
    // `rust-analyzer.toml`, but this gateway is handed a `SourceBundle` and
    // cannot know who built it; a bundle that carries one would let the
    // workspace override the fixed `initializationOptions`, so no container is
    // created for it.
    if let Some(path) = analyzer_configuration(source) {
        let _ = path;
        return refusal(
            identity,
            domain::AnalyzerFailure::UnsupportedProjectConfig,
            started,
        );
    }

    // Input preconditions that need no guest: a query this capture cannot
    // answer is a named failure, not a container.
    let document = match query.file() {
        Some(file) => match document(source, file) {
            Ok(document) => Some(document),
            Err(failure) => return refusal(identity, failure, started),
        },
        None => None,
    };
    if let Err(failure) = wire_positions(query, document.as_ref()) {
        return refusal(identity, failure, started);
    }
    let (indices, not_utf8) = snapshot(source);

    let archive = super::source_archive::encode(source)?;
    let budget = WorkBudget {
        started,
        deadline: started + Duration::from_millis(budgets.limits.wall_ms()),
        limits: budgets.limits,
        cancel,
    };
    let nonce = state::nonce()?;
    let volume = format!("rust-mcp-source-{nonce}");
    let ingest = format!("rust-mcp-ingest-{nonce}");
    let analyzer = format!("rust-mcp-analyzer-{nonce}");
    if !gateway.absent("volume", &volume)? {
        return Err(ExecutionError::CleanupUncertain);
    }
    let work = (|| -> Result<Conversation, ExecutionError> {
        if budget.stop().is_some() {
            return Ok(Conversation::unstarted());
        }
        let mut args = vec!["volume".into(), "create".into(), "--driver=local".into()];
        for (key, value) in labels(&nonce) {
            args.push(format!("--label={key}={value}"));
        }
        args.push(volume.clone());
        if gateway.inner.control(&args)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect =
            gateway
                .inner
                .control(&["volume".into(), "inspect".into(), volume.clone()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let parsed = Volume::parse(&inspect.stdout, &volume, &nonce)?;
        let (ingested, _) = gateway.phase(
            PhaseRequest {
                name: &ingest,
                nonce: &nonce,
                volume: &parsed,
                phase: &Phase::Ingest,
            },
            &archive,
            &budget,
        )?;
        if ingested.stop != Stop::Exited {
            return Ok(Conversation::unstarted());
        }
        if ingested.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        // No writer of /source remains while the analyzer parses it.
        gateway.inner.remove(&ingest)?;
        converse(
            gateway,
            Session {
                name: &analyzer,
                nonce: &nonce,
                volume: &parsed,
            },
            Question {
                query,
                document: document.as_ref(),
                indices: &indices,
                not_utf8,
            },
            budgets,
            &budget,
        )
    })();
    let terminal = budget.stop();
    // G3: every container is joined and both objects verified absent before the
    // single-flight guard drops at the end of this function.
    gateway.cleanup_analyzer(&[&ingest, &analyzer], &volume, &nonce)?;
    assemble(identity, work?, terminal, started)
}

/// The container this call owns.
struct Session<'a> {
    name: &'a str,
    nonce: &'a str,
    volume: &'a Volume,
}

/// Everything the conversation needs to know about the question.
struct Question<'a> {
    query: &'a AnalyzerQuery,
    document: Option<&'a Document>,
    indices: &'a BTreeMap<domain::AnalyzerFile, domain::LineIndex>,
    /// Captured `.rs` files whose bytes are not UTF-8, and which therefore
    /// cannot host a translated position.
    not_utf8: u32,
}

/// The runtime identity of the admitted image, from the constants the native
/// calibration verifies against the guest.
fn runtime_identity(gateway: &RustGateway) -> Result<domain::AnalyzerRuntime, ExecutionError> {
    // Unreachable for the constants above, which are canonical by inspection;
    // a future edit that broke one would fail the call rather than publish a
    // half-built identity.
    fn malformed<E>(_: E) -> ExecutionError {
        ExecutionError::Infrastructure
    }
    Ok(domain::AnalyzerRuntime {
        version: ANALYZER_VERSION.to_owned().try_into().map_err(malformed)?,
        binary_sha256: ANALYZER_BINARY_SHA256.parse().map_err(malformed)?,
        image_id: gateway
            .image_id()
            .to_owned()
            .try_into()
            .map_err(malformed)?,
        config_digest: analyzer_config_digest()?,
    })
}

/// A refusal decided before any session existed.
///
/// `completeness` carries no `IncompleteReason`: the closed reason set of
/// M6-01 names readiness, limits, sysroot and timeouts, and none of those is
/// what happened — the named failure is the reason, and inventing a second
/// vocabulary for it here would let the two disagree.
fn refusal(
    identity: domain::AnalyzerRuntime,
    failure: domain::AnalyzerFailure,
    started: Instant,
) -> Result<domain::AnalyzerExecution, ExecutionError> {
    Ok(stamp(
        domain::AnalyzerExecution {
            identity,
            position_encoding: None,
            readiness: domain::AnalyzerReadiness::NotReady { elapsed_ms: 0 },
            outcome: domain::AnalyzerOutcome::Failed(failure),
            completeness: domain::Completeness::incomplete(Vec::new()),
            session: absent_session()?,
            termination: ExecutionTermination::Exited,
            oom_killed: None,
            call_duration_ms: 0,
        },
        started,
    ))
}

/// Stamps the *call's* duration. The session's own measurement is left alone:
/// it is the duration of the conversation, while this covers the capture
/// checks, the volume, the ingest and the cleanup around it as well.
fn stamp(mut execution: domain::AnalyzerExecution, started: Instant) -> domain::AnalyzerExecution {
    execution.call_duration_ms = elapsed_ms(started);
    execution
}

/// The summary of a session that never opened. The digest is the real digest of
/// the zero stderr bytes observed, not a placeholder.
fn absent_session() -> Result<domain::SessionSummary, ExecutionError> {
    Ok(domain::SessionSummary {
        stop: domain::SessionStop::NotStarted,
        exit_code: None,
        messages_in: 0,
        messages_out: 0,
        bytes_in: 0,
        bytes_out: 0,
        stderr_bytes: 0,
        stderr_sha256: fingerprint(&digest(&[]))?,
        stderr_truncated: false,
        server_requests_refused: 0,
        notifications_dropped: 0,
        late_responses: 0,
        status_transcript: Vec::new(),
        status_notifications: 0,
        fault: None,
        declared_frame_bytes: None,
        kill_error: None,
        reap_error: None,
        duration_ms: 0,
    })
}

fn fingerprint(text: &str) -> Result<domain::SourceFingerprint, ExecutionError> {
    text.parse().map_err(|_| ExecutionError::Infrastructure)
}

/// The first `rust-analyzer.toml` or `.rust-analyzer.toml` in the bundle, at
/// any depth and in any casing (ADR-084 §6).
///
/// Matched on the last path component of **every** entry the bundle carries,
/// files and directories alike, exactly as the capture does — the capture
/// refuses the name whatever kind of object wears it, and a directory of that
/// name reaching the guest would be the same override arriving by another
/// route. The same names are refused by both, so the two refusals cannot drift
/// into disagreeing about what a workspace configuration is.
fn analyzer_configuration(source: &SourceBundle) -> Option<&str> {
    source
        .files()
        .iter()
        .map(|file| file.path())
        .chain(source.directories().iter().map(String::as_str))
        .find(|path| {
            path.rsplit('/').next().is_some_and(|name| {
                name.eq_ignore_ascii_case("rust-analyzer.toml")
                    || name.eq_ignore_ascii_case(".rust-analyzer.toml")
            })
        })
}

/// Reads the queried file out of the capture.
fn document(
    source: &SourceBundle,
    file: &domain::AnalyzerFile,
) -> Result<Document, domain::AnalyzerFailure> {
    let bytes = source
        .files()
        .iter()
        .find(|candidate| candidate.path() == file.as_str())
        .map(|candidate| candidate.bytes())
        .ok_or(domain::AnalyzerFailure::FileNotInSnapshot)?;
    let text =
        String::from_utf8(bytes.to_vec()).map_err(|_| domain::AnalyzerFailure::FileNotUtf8)?;
    let index = domain::LineIndex::new(bytes).map_err(|_| domain::AnalyzerFailure::FileNotUtf8)?;
    Ok(Document {
        file: file.clone(),
        text,
        index,
    })
}

/// Every caller position must resolve against the captured bytes before a
/// container is created: a position that does not is the caller's fault, and
/// spending a guest session to discover it would be waste.
fn wire_positions(
    query: &AnalyzerQuery,
    document: Option<&Document>,
) -> Result<(), domain::AnalyzerFailure> {
    match (query, document) {
        (AnalyzerQuery::References { position, .. }, Some(document)) => {
            document
                .index
                .utf8_from_position(*position)
                .map_err(out_of_range)?;
        }
        (AnalyzerQuery::CodeActions { range, .. }, Some(document)) => {
            document
                .index
                .utf8_from_position(range.start())
                .map_err(out_of_range)?;
            document
                .index
                .utf8_from_position(range.end())
                .map_err(out_of_range)?;
        }
        // The remaining variants carry no caller position.
        _ => (),
    }
    Ok(())
}

/// Line indices for every captured `.rs` file, plus the count of `.rs` files
/// whose bytes are not UTF-8.
fn snapshot(source: &SourceBundle) -> (BTreeMap<domain::AnalyzerFile, domain::LineIndex>, u32) {
    let mut indices = BTreeMap::new();
    let mut not_utf8 = 0u32;
    for file in source.files() {
        let Ok(analyzer_file) = domain::AnalyzerFile::new(file.path().to_owned()) else {
            continue;
        };
        match domain::LineIndex::new(file.bytes()) {
            Ok(index) => {
                indices.insert(analyzer_file, index);
            }
            Err(_) => not_utf8 = not_utf8.saturating_add(1),
        }
    }
    (indices, not_utf8)
}

fn file_uri(file: &domain::AnalyzerFile) -> String {
    // `validate_source_path` admits only `[A-Za-z0-9._/-]`, so there is nothing
    // here to percent-encode.
    format!("{SOURCE_URI}/{}", file.as_str())
}

/// Creates, verifies and drives the analyzer container.
fn converse(
    gateway: &RustGateway,
    session: Session<'_>,
    question: Question<'_>,
    budgets: AnalyzerBudgets,
    budget: &WorkBudget<'_>,
) -> Result<Conversation, ExecutionError> {
    if budget.stop().is_some() {
        return Ok(Conversation::unstarted());
    }
    if !gateway.absent("container", session.name)? {
        return Err(ExecutionError::CleanupUncertain);
    }
    let created = gateway.inner.control(&gateway.arguments(
        session.name,
        session.nonce,
        session.volume,
        &Phase::Analyzer,
    )?)?;
    if created.code != Some(0) {
        return Err(ExecutionError::Infrastructure);
    }
    let inspect =
        gateway
            .inner
            .control(&["container".into(), "inspect".into(), session.name.into()])?;
    if inspect.code != Some(0) {
        return Err(ExecutionError::Infrastructure);
    }
    super::rust_applied::verify(
        &inspect.stdout,
        gateway.image_id(),
        &Phase::Analyzer,
        session.volume,
        session.nonce,
    )?;
    if budget.stop().is_some() {
        return Ok(Conversation::unstarted());
    }
    let mut command = DockerGateway::command(&gateway.inner.config, &gateway.inner.state)?;
    command.args([
        "container",
        "start",
        "--attach",
        "--interactive",
        session.name,
    ]);
    // The session's cancellation is the caller's token, never the combined
    // `WorkBudget`: the total deadline is the session's own `deadline`, so a
    // timeout and a cancellation stay distinguishable in the outcome.
    let mut live = LspSession::open(
        command,
        SessionBudget::standard(budget.deadline),
        budget.cancel,
    )?;
    let mut progress = Progress::default();
    let failure = protocol(&mut live, &question, budgets, &mut progress).err();
    let outcome = live.close(SHUTDOWN_GRACE);
    let oom_killed = guest_verdict(gateway, session.name);
    let outcome_value = match (failure, progress.result.take()) {
        (Some(failure), _) => domain::AnalyzerOutcome::Failed(failure),
        (None, Some(result)) => domain::AnalyzerOutcome::Answered(result),
        // Unreachable: `protocol` returns `Ok` only once a result is set.
        (None, None) => domain::AnalyzerOutcome::Failed(domain::AnalyzerFailure::ProtocolViolation),
    };
    Ok(Conversation {
        session: Some(outcome),
        encoding: progress.encoding,
        readiness: progress
            .readiness
            .unwrap_or(domain::AnalyzerReadiness::NotReady { elapsed_ms: 0 }),
        outcome: Some(outcome_value),
        reasons: progress.reasons,
        omissions: progress.omissions,
        oom_killed,
        stage: progress.stage,
    })
}

/// Whether the guest was OOM-killed, read before cleanup removes the container.
///
/// `None` whenever the container cannot be inspected or is still running: an
/// unobserved kill is reported as unobserved, never as `false`. The exit code
/// published in the session summary is the attached client's, which is the
/// container's own when the container really exited.
fn guest_verdict(gateway: &RustGateway, name: &str) -> Option<bool> {
    let inspected = gateway
        .inner
        .control(&["container".into(), "inspect".into(), name.into()])
        .ok()?;
    if inspected.code != Some(0) {
        return None;
    }
    let containers: Vec<crate::Container> = serde_json::from_slice(&inspected.stdout).ok()?;
    match containers.as_slice() {
        [container] if !container.state.running => Some(container.state.oom_killed),
        _ => None,
    }
}

/// Which phase of ADR-084 §2 the conversation is in, so a timeout can be named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Initialize,
    Query,
}

#[derive(Default)]
struct Progress {
    stage: Stage,
    encoding: Option<domain::PositionEncoding>,
    readiness: Option<domain::AnalyzerReadiness>,
    reasons: Vec<domain::IncompleteReason>,
    omissions: Vec<domain::Omission>,
    result: Option<domain::AnalyzerResult>,
}

/// Maps a terminal session fault onto the closed failure vocabulary.
fn classify(error: SessionError, stage: Stage) -> domain::AnalyzerFailure {
    use domain::AnalyzerFailure as Failure;
    use lsp_codec::CodecError as Codec;
    match error {
        SessionError::Cancelled => Failure::Cancelled,
        SessionError::Timeout => match stage {
            Stage::Initialize => Failure::TimeoutInitialize,
            Stage::Query => Failure::TimeoutQuery,
        },
        // The peer left without a handshake, or its pipe died: from this side
        // both are the server going away mid-session.
        SessionError::Eof | SessionError::Io => Failure::Crashed,
        // The two halves of the old `FramingRejected`: a declared length above
        // the ADR-084 §8 bound, and any other header this codec refuses.
        SessionError::Codec(Codec::FrameLimit) => Failure::FrameTooLarge,
        SessionError::Codec(Codec::MalformedHeader) => Failure::MalformedHeader,
        SessionError::Codec(Codec::MessageLimit | Codec::ByteLimit)
        | SessionError::MessageLimit
        | SessionError::ByteLimit
        | SessionError::FrameLimit => Failure::ProtocolLimit,
        SessionError::Codec(_) | SessionError::Encode => Failure::ProtocolViolation,
    }
}

/// The conversation itself: initialize, readiness, `didOpen`, one request.
fn protocol(
    session: &mut LspSession<'_>,
    question: &Question<'_>,
    budgets: AnalyzerBudgets,
    progress: &mut Progress,
) -> Result<(), domain::AnalyzerFailure> {
    let initialize_started = Instant::now();
    let params = serde_json::to_value(lsp_codec::InitializeParams::new())
        .map_err(|_| domain::AnalyzerFailure::ProtocolViolation)?;
    let matched = session
        .request("initialize", Some(params), budgets.initialize)
        .map_err(|error| classify(error, Stage::Initialize))?;
    let value = matched.result.map_err(|error| server_error(error.code))?;
    let result: lsp_codec::InitializeResult =
        serde_json::from_value(value).map_err(|_| domain::AnalyzerFailure::ProtocolViolation)?;
    // A server that names no encoding has not agreed to `utf-8`, and LSP's
    // default is `utf-16`: either way this client refuses to guess (ADR-084 §5).
    let encoding = match result.capabilities.position_encoding.as_deref() {
        Some("utf-8") => domain::PositionEncoding::Utf8,
        Some("utf-16") => domain::PositionEncoding::Utf16,
        _ => return Err(domain::AnalyzerFailure::CapabilityMismatch),
    };
    progress.encoding = Some(encoding);
    if encoding != domain::PositionEncoding::Utf8 {
        return Err(domain::AnalyzerFailure::CapabilityMismatch);
    }
    // Every write is bounded by the phase it belongs to, not only by the
    // session deadline: a peer that stops reading its stdin must exhaust the
    // initialize budget and be named a `TimeoutInitialize`, never spend the
    // whole call and be published as a claim about its indexing.
    let until = session.until(remaining(
        budgets.initialize,
        initialize_started,
        Stage::Initialize,
    )?);
    session
        .send(
            &lsp_codec::OutgoingMessage::Notification {
                method: "initialized".to_owned(),
                params: Some(serde_json::Value::Object(serde_json::Map::new())),
            },
            until,
        )
        .map_err(|error| classify(error, Stage::Initialize))?;

    // Readiness: the notification oracle only, never silence (ADR-084 §5).
    let health = loop {
        let waited = elapsed_ms(initialize_started);
        let not_ready = |progress: &mut Progress| {
            progress.readiness = Some(domain::AnalyzerReadiness::NotReady { elapsed_ms: waited });
            domain::AnalyzerFailure::NotReady
        };
        let Some(remaining) = budgets
            .initialize
            .checked_sub(initialize_started.elapsed())
            .filter(|remaining| !remaining.is_zero())
        else {
            return Err(not_ready(progress));
        };
        match session.next_event(remaining) {
            Ok(Event::ServerStatus(record)) => {
                if record.health == Some(domain::ServerHealth::Error) {
                    return Err(not_ready(progress));
                }
                if record.quiescent {
                    // An unrecognised spelling is not assumed healthy: it is
                    // recorded as a warning, which reaches `completeness`.
                    break record.health.unwrap_or(domain::ServerHealth::Warning);
                }
            }
            // Nothing is outstanding, so a response here is the peer inventing
            // one; end of stdout before readiness is the server dying.
            Ok(Event::Response(_)) => return Err(domain::AnalyzerFailure::ProtocolViolation),
            Ok(Event::Eof) => return Err(domain::AnalyzerFailure::Crashed),
            Err(SessionError::Timeout) => return Err(not_ready(progress)),
            Err(error) => return Err(classify(error, Stage::Initialize)),
        }
    };
    progress.readiness = Some(domain::AnalyzerReadiness::Quiescent {
        elapsed_ms: elapsed_ms(initialize_started),
        health,
    });
    // Any warning degrades the answer, whatever it was about: the accompanying
    // `message` can carry project text and is never read (D25 §1.6), so this
    // side knows a warning happened and nothing more. Saying `complete` over it
    // would be a claim about a server that said otherwise.
    if health == domain::ServerHealth::Warning {
        progress
            .reasons
            .push(domain::IncompleteReason::AnalyzerWarning);
    }

    progress.stage = Stage::Query;
    let query_started = Instant::now();
    if let Some(document) = question.document {
        let open = lsp_codec::DidOpenTextDocumentParams::new(
            file_uri(&document.file),
            document.text.clone(),
        );
        let until = session.until(remaining(budgets.query, query_started, Stage::Query)?);
        session
            .send(
                &lsp_codec::OutgoingMessage::Notification {
                    method: "textDocument/didOpen".to_owned(),
                    params: Some(
                        serde_json::to_value(open)
                            .map_err(|_| domain::AnalyzerFailure::ProtocolViolation)?,
                    ),
                },
                until,
            )
            .map_err(|error| classify(error, Stage::Query))?;
    }

    let (method, params) = request_for(question)?;
    let matched = session
        .request(&method, Some(params), budgets.query)
        .map_err(|error| classify(error, Stage::Query))?;
    let value = matched.result.map_err(|error| server_error(error.code))?;
    let (result, omitted) = convert(question, value)?;
    record_omissions(progress, omitted, question.not_utf8);
    progress.result = Some(result);
    Ok(())
}

/// Records what the answer left out.
///
/// Postcondition: every omission is pushed together with the reason that makes
/// the result `incomplete`. An omission on its own would let an answer that
/// silently skipped part of the capture be published as exhaustive — and, for
/// `LimitVisible`, would be refused outright by
/// [`domain::Completeness::with_omissions`].
fn record_omissions(progress: &mut Progress, omitted: u32, not_utf8: u32) {
    if omitted > 0 {
        progress.omissions.push(domain::Omission {
            kind: domain::OmissionKind::LimitVisible,
            count: omitted,
        });
        progress
            .reasons
            .push(domain::IncompleteReason::LimitVisible);
    }
    // A file with no line index is a hole in what the answer could have
    // covered: no position in it could have been translated, so nothing in it
    // could have been reported.
    if not_utf8 > 0 {
        progress.omissions.push(domain::Omission {
            kind: domain::OmissionKind::NotUtf8File,
            count: not_utf8,
        });
        progress.reasons.push(domain::IncompleteReason::NotUtf8File);
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

/// What is left of a phase budget, or the timeout that phase has already spent.
///
/// A zero remainder is the timeout, not a zero-length wait: a `send` given no
/// budget at all would report the peer as unresponsive on the first poll.
fn remaining(
    budget: Duration,
    started: Instant,
    stage: Stage,
) -> Result<Duration, domain::AnalyzerFailure> {
    budget
        .checked_sub(started.elapsed())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| classify(SessionError::Timeout, stage))
}

/// The two JSON-RPC error codes this lifecycle names; any other error response
/// is the server refusing a request whose support it advertised.
fn server_error(code: i64) -> domain::AnalyzerFailure {
    match code {
        // ADR-084 §5: no `didChange` exists here, so this should not happen and
        // is never retried.
        lsp_codec::ResponseError::CONTENT_MODIFIED => domain::AnalyzerFailure::Crashed,
        lsp_codec::ResponseError::REQUEST_CANCELLED => domain::AnalyzerFailure::Cancelled,
        _ => domain::AnalyzerFailure::ServerError,
    }
}

/// The single request the query type implies (ADR-084 §2 phase 6).
fn request_for(
    question: &Question<'_>,
) -> Result<(String, serde_json::Value), domain::AnalyzerFailure> {
    let opened = || {
        question
            .document
            .ok_or(domain::AnalyzerFailure::FileNotInSnapshot)
    };
    let identifier = |document: &Document| lsp_codec::TextDocumentIdentifier {
        uri: file_uri(&document.file),
    };
    match question.query {
        AnalyzerQuery::DocumentSymbols { .. } => Ok((
            "textDocument/documentSymbol".to_owned(),
            serde_json::to_value(lsp_codec::DocumentSymbolParams {
                text_document: identifier(opened()?),
            })
            .map_err(violation)?,
        )),
        AnalyzerQuery::WorkspaceSymbols { query } => Ok((
            "workspace/symbol".to_owned(),
            serde_json::to_value(lsp_codec::WorkspaceSymbolParams {
                query: query.as_str().to_owned(),
            })
            .map_err(violation)?,
        )),
        AnalyzerQuery::References {
            position,
            include_declaration,
            ..
        } => {
            let document = opened()?;
            let (line, character) = document
                .index
                .utf8_from_position(*position)
                .map_err(|_| domain::AnalyzerFailure::PositionOutOfRange)?;
            Ok((
                "textDocument/references".to_owned(),
                serde_json::to_value(lsp_codec::ReferenceParams {
                    text_document: identifier(document),
                    position: lsp_codec::LspPosition { line, character },
                    context: lsp_codec::ReferenceContext {
                        include_declaration: *include_declaration,
                    },
                })
                .map_err(violation)?,
            ))
        }
        AnalyzerQuery::Diagnostics { .. } => Ok((
            "textDocument/diagnostic".to_owned(),
            serde_json::to_value(lsp_codec::DocumentDiagnosticParams {
                text_document: identifier(opened()?),
            })
            .map_err(violation)?,
        )),
        AnalyzerQuery::CodeActions { range, only, .. } => {
            let document = opened()?;
            // Sorted and de-duplicated: a repeated kind carries no meaning, and
            // a fixed order keeps the framed request a pure function of the
            // validated query.
            let kinds: Vec<String> = only
                .iter()
                .map(|kind| kind.to_lsp().to_owned())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            Ok((
                "textDocument/codeAction".to_owned(),
                serde_json::to_value(lsp_codec::CodeActionParams {
                    text_document: identifier(document),
                    range: wire_range(document, *range)?,
                    context: lsp_codec::CodeActionContext {
                        diagnostics: Vec::new(),
                        only: (!kinds.is_empty()).then_some(kinds),
                    },
                })
                .map_err(violation)?,
            ))
        }
    }
}

fn wire_range(
    document: &Document,
    range: domain::TextRange,
) -> Result<lsp_codec::LspRange, domain::AnalyzerFailure> {
    let (start_line, start_character) = document
        .index
        .utf8_from_position(range.start())
        .map_err(out_of_range)?;
    let (end_line, end_character) = document
        .index
        .utf8_from_position(range.end())
        .map_err(out_of_range)?;
    Ok(lsp_codec::LspRange {
        start: lsp_codec::LspPosition {
            line: start_line,
            character: start_character,
        },
        end: lsp_codec::LspPosition {
            line: end_line,
            character: end_character,
        },
    })
}

/// Converts one answer into domain values, returning the count of entries the
/// peer sent that are not in the result.
fn convert(
    question: &Question<'_>,
    value: serde_json::Value,
) -> Result<(domain::AnalyzerResult, u32), domain::AnalyzerFailure> {
    // Only `utf-8` reaches this point: any other negotiated encoding already
    // failed the call with `CapabilityMismatch`.
    let encoding = domain::PositionEncoding::Utf8;
    let opened = || {
        question
            .document
            .ok_or(domain::AnalyzerFailure::FileNotInSnapshot)
    };
    match question.query {
        AnalyzerQuery::DocumentSymbols { .. } => {
            let document = opened()?;
            let symbols: Vec<lsp_codec::LspDocumentSymbol> =
                serde_json::from_value(value).map_err(violation)?;
            let (symbols, omitted) =
                lsp_codec::document_symbols_to_domain(symbols, &document.index, encoding)
                    .map_err(violation)?;
            Ok((
                domain::AnalyzerResult::DocumentSymbols(symbols),
                bounded_count(omitted),
            ))
        }
        AnalyzerQuery::WorkspaceSymbols { .. } => {
            let symbols: Vec<lsp_codec::SymbolInformation> =
                serde_json::from_value(value).map_err(violation)?;
            let (symbols, omitted) =
                lsp_codec::workspace_symbols_to_domain(symbols, question.indices, encoding)
                    .map_err(violation)?;
            Ok((
                domain::AnalyzerResult::WorkspaceSymbols(symbols),
                bounded_count(omitted),
            ))
        }
        AnalyzerQuery::References { .. } => {
            let locations: Vec<lsp_codec::Location> =
                serde_json::from_value(value).map_err(violation)?;
            // One request per session (ADR-084 §2 phase 6), and a bare
            // `textDocument/references` answer carries no flag saying which
            // location is the declaration, so none is marked as one: deducing it
            // from the queried position would be an inference, not an
            // observation.
            let (references, omitted) =
                lsp_codec::references_to_domain(locations, &[], question.indices, encoding)
                    .map_err(violation)?;
            Ok((
                domain::AnalyzerResult::References(references),
                bounded_count(omitted),
            ))
        }
        AnalyzerQuery::Diagnostics { .. } => {
            let document = opened()?;
            let report: lsp_codec::DocumentDiagnosticReport =
                serde_json::from_value(value).map_err(violation)?;
            let items = report.into_full().map_err(violation)?;
            let (diagnostics, omitted) =
                lsp_codec::diagnostics_to_domain(&document.file, items, &document.index, encoding)
                    .map_err(violation)?;
            Ok((
                domain::AnalyzerResult::Diagnostics(diagnostics),
                bounded_count(omitted),
            ))
        }
        AnalyzerQuery::CodeActions { .. } => {
            let elements: Vec<serde_json::Value> =
                serde_json::from_value(value).map_err(violation)?;
            let total = elements.len();
            let visible = total.min(domain::MAX_ACTIONS);
            let resolved = lsp_codec::code_actions_to_candidates(
                elements.into_iter().take(visible).collect(),
                question.indices,
                encoding,
            );
            let candidates = resolved.into_iter().map(candidate).collect();
            Ok((
                domain::AnalyzerResult::CodeActions(candidates),
                bounded_count(total - visible),
            ))
        }
    }
}

fn candidate(
    resolved: Result<lsp_codec::ResolvedAction, domain::ActionRejection>,
) -> domain::ActionCandidate {
    match resolved {
        Ok(action) => match domain::NonEmptyText::try_from(action.title) {
            Ok(title) => domain::ActionCandidate::Applicable(domain::AnalyzerAction {
                title,
                kind: action.kind,
                is_preferred: action.is_preferred,
                edits: action.edits,
            }),
            // A blank title leaves nothing a caller could show or digest, so
            // this element never became a resolvable action.
            Err(_) => domain::ActionCandidate::Rejected(domain::ActionRejection::UnresolvedEdit),
        },
        Err(rejection) => domain::ActionCandidate::Rejected(rejection),
    }
}

fn bounded_count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// The peer sent something this adapter will not interpret. Generic over the
/// rejecting error type so one name covers serde, codec and domain refusals
/// without any of them widening into a second meaning.
fn violation<E>(_: E) -> domain::AnalyzerFailure {
    domain::AnalyzerFailure::ProtocolViolation
}

/// A caller position that does not resolve against the captured bytes.
fn out_of_range<E>(_: E) -> domain::AnalyzerFailure {
    domain::AnalyzerFailure::PositionOutOfRange
}

/// Builds the published execution from the conversation and the terminal signal.
fn assemble(
    identity: domain::AnalyzerRuntime,
    conversation: Conversation,
    terminal: Option<Stop>,
    started: Instant,
) -> Result<domain::AnalyzerExecution, ExecutionError> {
    let termination = match terminal {
        Some(Stop::Cancelled) => ExecutionTermination::Cancelled,
        Some(Stop::TimedOut) => ExecutionTermination::TimedOut,
        Some(Stop::OutputLimit) => ExecutionTermination::OutputLimit,
        Some(Stop::Exited) | None => ExecutionTermination::Exited,
    };
    let outcome = conversation
        .outcome
        .unwrap_or(domain::AnalyzerOutcome::Failed(match terminal {
            Some(Stop::Cancelled) => domain::AnalyzerFailure::Cancelled,
            _ => domain::AnalyzerFailure::TimeoutTotal,
        }));
    let mut reasons = conversation.reasons;
    match &outcome {
        domain::AnalyzerOutcome::Failed(domain::AnalyzerFailure::NotReady) => {
            reasons.push(domain::IncompleteReason::AnalyzerNotReady);
        }
        domain::AnalyzerOutcome::Failed(
            domain::AnalyzerFailure::TimeoutInitialize
            | domain::AnalyzerFailure::TimeoutQuery
            | domain::AnalyzerFailure::TimeoutTotal,
        ) => reasons.push(domain::IncompleteReason::Timeout),
        _ => (),
    }
    let answered = matches!(outcome, domain::AnalyzerOutcome::Answered(_));
    let completeness = if answered && reasons.is_empty() {
        domain::Completeness::complete()
    } else {
        domain::Completeness::incomplete(reasons)
    };
    // A `Complete` state may not carry a visible-limit omission, and the branch
    // above never builds that pair, so this cannot reject the omissions.
    let completeness = completeness
        .with_omissions(conversation.omissions)
        .map_err(|_| ExecutionError::Infrastructure)?;
    let session = match conversation.session {
        Some(outcome) => summary(outcome, conversation.stage)?,
        None => absent_session()?,
    };
    Ok(stamp(
        domain::AnalyzerExecution {
            identity,
            position_encoding: conversation.encoding,
            readiness: conversation.readiness,
            outcome,
            completeness,
            session,
            termination,
            oom_killed: conversation.oom_killed,
            call_duration_ms: 0,
        },
        started,
    ))
}

/// The published summary of one session.
///
/// `stage` is the phase the conversation had reached, so a terminal fault is
/// named as the timeout it actually was rather than as a generic one.
fn summary(
    outcome: SessionOutcome,
    stage: Stage,
) -> Result<domain::SessionSummary, ExecutionError> {
    Ok(domain::SessionSummary {
        stop: outcome.stop,
        exit_code: outcome.exit_code,
        messages_in: u32::try_from(outcome.messages_in).unwrap_or(u32::MAX),
        messages_out: u32::try_from(outcome.messages_out).unwrap_or(u32::MAX),
        bytes_in: outcome.bytes_in,
        bytes_out: outcome.bytes_out,
        stderr_bytes: outcome.stderr_len,
        stderr_sha256: fingerprint(&outcome.stderr_sha256)?,
        stderr_truncated: outcome.stderr_truncated,
        server_requests_refused: bounded_count(outcome.server_requests.len()),
        notifications_dropped: u32::try_from(outcome.notifications_dropped).unwrap_or(u32::MAX),
        late_responses: u32::try_from(outcome.late_responses).unwrap_or(u32::MAX),
        status_notifications: bounded_count(outcome.status_transcript.len()),
        status_transcript: outcome
            .status_transcript
            .iter()
            .take(domain::MAX_PUBLISHED_STATUS)
            .map(|record| domain::ServerStatusObservation {
                quiescent: record.quiescent,
                health: record.health,
                elapsed_ms: record.elapsed_ms,
            })
            .collect(),
        fault: outcome.fatal.map(|error| classify(error, stage)),
        declared_frame_bytes: outcome.declared_frame_bytes,
        kill_error: bounded_text(outcome.kill_error),
        reap_error: bounded_text(outcome.reap_error),
        duration_ms: outcome.duration_ms,
    })
}

/// An io error kind, if the session recorded one. An empty string never
/// becomes a published value: `None` says "nothing to report" honestly.
fn bounded_text(value: Option<String>) -> Option<domain::NonEmptyText> {
    value.and_then(|text| domain::NonEmptyText::try_from(text).ok())
}

/// Runs one closed, argument-free analyzer phase against an empty source volume
/// and returns its stdout.
///
/// Calibration only (ADR-084 §3 and §1): the `Phase::AnalyzerDocument` identity
/// probe and the `Phase::AnalyzerConfigSchema` dump exist so a receipt can check
/// the pinned constants and the fixed configuration keys against the real
/// binary. No tool reaches either phase, and neither reads project bytes — the
/// volume is created and mounted read-only precisely so the container shape
/// stays the one `rust_applied` verifies, with nothing ingested into it.
#[cfg(test)]
pub(super) fn probe(
    gateway: &RustGateway,
    phase: &Phase,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<(Vec<u8>, Option<i32>), ExecutionError> {
    if gateway.image_id() != APPROVED_M6_IMAGE {
        return Err(ExecutionError::Unavailable);
    }
    let started = Instant::now();
    let _busy = gateway.hold_busy()?;
    if gateway.is_quarantined() {
        return Err(ExecutionError::CleanupUncertain);
    }
    gateway.approved_runtime(cancel)?;
    let budget = WorkBudget {
        started,
        deadline: started + Duration::from_millis(limits.wall_ms()),
        limits,
        cancel,
    };
    let nonce = state::nonce()?;
    let volume = format!("rust-mcp-source-{nonce}");
    let container = format!("rust-mcp-analyzer-probe-{nonce}");
    if !gateway.absent("volume", &volume)? {
        return Err(ExecutionError::CleanupUncertain);
    }
    let work = (|| -> Result<(Capture, Option<bool>), ExecutionError> {
        let mut args = vec!["volume".into(), "create".into(), "--driver=local".into()];
        for (key, value) in labels(&nonce) {
            args.push(format!("--label={key}={value}"));
        }
        args.push(volume.clone());
        if gateway.inner.control(&args)?.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let inspect =
            gateway
                .inner
                .control(&["volume".into(), "inspect".into(), volume.clone()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        let parsed = Volume::parse(&inspect.stdout, &volume, &nonce)?;
        gateway.phase(
            PhaseRequest {
                name: &container,
                nonce: &nonce,
                volume: &parsed,
                phase,
            },
            &[],
            &budget,
        )
    })();
    gateway.cleanup_analyzer(&[&container], &volume, &nonce)?;
    let (capture, _) = work?;
    if capture.stdout_truncated {
        return Err(ExecutionError::Infrastructure);
    }
    Ok((capture.stdout, capture.code))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle(files: &[(&str, &[u8])]) -> Result<SourceBundle, Box<dyn std::error::Error>> {
        let files = files
            .iter()
            .map(|(path, bytes)| domain::SourceFile::new((*path).to_owned(), bytes.to_vec()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{error:?}"))?;
        Ok(SourceBundle::new(files).map_err(|error| format!("{error:?}"))?)
    }

    #[test]
    fn the_admitted_digest_is_the_only_one_this_module_names() {
        assert_eq!(APPROVED_M6_IMAGE.len(), 71);
        assert!(APPROVED_M6_IMAGE.starts_with("sha256:"));
        assert_ne!(APPROVED_M6_IMAGE, crate::APPROVED_M5_IMAGE);
        assert_ne!(APPROVED_M6_IMAGE, crate::APPROVED_M4_IMAGE);
    }

    #[test]
    fn the_config_digest_is_a_canonical_fingerprint_of_the_fixed_options()
    -> Result<(), Box<dyn std::error::Error>> {
        let first = analyzer_config_digest().map_err(|error| format!("{error:?}"))?;
        let second = analyzer_config_digest().map_err(|error| format!("{error:?}"))?;
        assert_eq!(first, second);
        assert!(first.as_str().starts_with("sha256:"));
        Ok(())
    }

    #[test]
    fn a_file_outside_the_capture_is_named_rather_than_guessed()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = bundle(&[("src/lib.rs", b"pub fn f() {}\n")])?;
        let missing = domain::AnalyzerFile::new("src/other.rs".into())?;
        assert_eq!(
            document(&source, &missing).err(),
            Some(domain::AnalyzerFailure::FileNotInSnapshot)
        );
        let present = domain::AnalyzerFile::new("src/lib.rs".into())?;
        assert!(document(&source, &present).is_ok());
        Ok(())
    }

    #[test]
    fn a_queried_file_that_is_not_utf8_is_refused_before_any_container()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = bundle(&[("src/lib.rs", &[0xff, 0xfe, 0x00])])?;
        let file = domain::AnalyzerFile::new("src/lib.rs".into())?;
        assert_eq!(
            document(&source, &file).err(),
            Some(domain::AnalyzerFailure::FileNotUtf8)
        );
        Ok(())
    }

    fn identity() -> Result<domain::AnalyzerRuntime, Box<dyn std::error::Error>> {
        Ok(domain::AnalyzerRuntime {
            version: domain::NonEmptyText::try_from(ANALYZER_VERSION.to_owned())?,
            binary_sha256: ANALYZER_BINARY_SHA256.parse()?,
            image_id: domain::NonEmptyText::try_from(APPROVED_M6_IMAGE.to_owned())?,
            config_digest: analyzer_config_digest().map_err(|error| format!("{error:?}"))?,
        })
    }

    #[test]
    fn a_non_utf8_rust_file_is_counted_as_an_omission_not_a_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = bundle(&[
            ("src/lib.rs", b"pub fn f() {}\n"),
            ("src/bad.rs", &[0xff]),
            ("Cargo.toml", b"[package]\n"),
        ])?;
        let (indices, not_utf8) = snapshot(&source);
        assert_eq!(not_utf8, 1);
        assert_eq!(indices.len(), 1, "only the readable .rs file has an index");
        assert!(indices.contains_key(&domain::AnalyzerFile::new("src/lib.rs".into())?));
        Ok(())
    }

    /// The whole pairing, from the capture to the published completeness: an
    /// answered call over a bundle with an unreadable `.rs` file is
    /// `incomplete[not_utf8_file]` carrying the omission. Never `Complete` —
    /// which would present a partial answer as exhaustive — and never an
    /// `Infrastructure` error, which is what an unpaired omission used to risk.
    #[test]
    fn an_unreadable_file_makes_an_answered_call_incomplete()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = bundle(&[
            ("src/lib.rs", b"pub fn f() {}\n"),
            ("src/bad.rs", &[0xff, 0xfe]),
        ])?;
        let (_, not_utf8) = snapshot(&source);
        let mut progress = Progress::default();
        record_omissions(&mut progress, 0, not_utf8);
        let conversation = Conversation {
            session: None,
            encoding: Some(domain::PositionEncoding::Utf8),
            readiness: domain::AnalyzerReadiness::Quiescent {
                elapsed_ms: 12,
                health: domain::ServerHealth::Ok,
            },
            outcome: Some(domain::AnalyzerOutcome::Answered(
                domain::AnalyzerResult::DocumentSymbols(Vec::new()),
            )),
            reasons: progress.reasons,
            omissions: progress.omissions,
            oom_killed: Some(false),
            stage: Stage::Query,
        };
        let execution = assemble(identity()?, conversation, None, Instant::now())
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            execution.completeness.state(),
            domain::CompletenessState::Incomplete
        );
        assert_eq!(
            execution.completeness.reasons(),
            [domain::IncompleteReason::NotUtf8File]
        );
        assert_eq!(
            execution.completeness.omissions(),
            [domain::Omission {
                kind: domain::OmissionKind::NotUtf8File,
                count: 1,
            }]
        );
        assert!(
            execution.result().is_some(),
            "the answer is still published"
        );
        Ok(())
    }

    /// A warning at quiescent degrades the answer, whatever it was about.
    #[test]
    fn a_server_warning_makes_the_answer_incomplete() -> Result<(), Box<dyn std::error::Error>> {
        let conversation = Conversation {
            session: None,
            encoding: Some(domain::PositionEncoding::Utf8),
            readiness: domain::AnalyzerReadiness::Quiescent {
                elapsed_ms: 12,
                health: domain::ServerHealth::Warning,
            },
            outcome: Some(domain::AnalyzerOutcome::Answered(
                domain::AnalyzerResult::DocumentSymbols(Vec::new()),
            )),
            reasons: vec![domain::IncompleteReason::AnalyzerWarning],
            omissions: Vec::new(),
            oom_killed: Some(false),
            stage: Stage::Query,
        };
        let execution = assemble(identity()?, conversation, None, Instant::now())
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            execution.completeness.state(),
            domain::CompletenessState::Incomplete
        );
        assert_eq!(
            execution.completeness.reasons(),
            [domain::IncompleteReason::AnalyzerWarning]
        );
        Ok(())
    }

    #[test]
    fn a_workspace_configuration_directory_is_refused_like_the_file()
    -> Result<(), Box<dyn std::error::Error>> {
        let files = vec![
            domain::SourceFile::new("src/lib.rs".to_owned(), b"pub fn f() {}\n".to_vec())
                .map_err(|error| format!("{error:?}"))?,
        ];
        let carried = SourceBundle::with_directories(
            files.clone(),
            vec!["src".to_owned(), "nested/rust-analyzer.toml".to_owned()],
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            analyzer_configuration(&carried),
            Some("nested/rust-analyzer.toml"),
            "the capture refuses the name whatever kind of object wears it"
        );
        let clean = SourceBundle::with_directories(files, vec!["src".to_owned()])
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(analyzer_configuration(&clean), None);
        Ok(())
    }

    #[test]
    fn a_position_outside_the_captured_bytes_never_reaches_the_guest()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = bundle(&[("src/lib.rs", b"fn f() {}\n")])?;
        let file = domain::AnalyzerFile::new("src/lib.rs".into())?;
        let opened = document(&source, &file).map_err(|error| format!("{error:?}"))?;
        let query = AnalyzerQuery::References {
            file: file.clone(),
            position: domain::Position::new(99, 1)?,
            include_declaration: true,
        };
        assert_eq!(
            wire_positions(&query, Some(&opened)).err(),
            Some(domain::AnalyzerFailure::PositionOutOfRange)
        );
        let inside = AnalyzerQuery::References {
            file,
            position: domain::Position::new(1, 4)?,
            include_declaration: true,
        };
        assert!(wire_positions(&inside, Some(&opened)).is_ok());
        Ok(())
    }

    #[test]
    fn each_query_sends_exactly_one_named_request() -> Result<(), Box<dyn std::error::Error>> {
        let source = bundle(&[("src/lib.rs", b"pub fn add() {}\n")])?;
        let file = domain::AnalyzerFile::new("src/lib.rs".into())?;
        let opened = document(&source, &file).map_err(|error| format!("{error:?}"))?;
        let (indices, not_utf8) = snapshot(&source);
        let range =
            domain::TextRange::new(domain::Position::new(1, 1)?, domain::Position::new(1, 8)?)?;
        let cases = [
            (
                AnalyzerQuery::DocumentSymbols { file: file.clone() },
                "textDocument/documentSymbol",
            ),
            (
                AnalyzerQuery::WorkspaceSymbols {
                    query: domain::SymbolQuery::new("add".into())?,
                },
                "workspace/symbol",
            ),
            (
                AnalyzerQuery::References {
                    file: file.clone(),
                    position: domain::Position::new(1, 8)?,
                    include_declaration: true,
                },
                "textDocument/references",
            ),
            (
                AnalyzerQuery::Diagnostics { file: file.clone() },
                "textDocument/diagnostic",
            ),
            (
                AnalyzerQuery::CodeActions {
                    file,
                    range,
                    only: vec![
                        domain::CodeActionKind::QuickFix,
                        domain::CodeActionKind::QuickFix,
                        domain::CodeActionKind::RefactorExtract,
                    ],
                },
                "textDocument/codeAction",
            ),
        ];
        for (query, expected) in cases {
            let question = Question {
                query: &query,
                document: Some(&opened),
                indices: &indices,
                not_utf8,
            };
            let (method, params) = request_for(&question).map_err(|error| format!("{error:?}"))?;
            assert_eq!(method, expected);
            if let AnalyzerQuery::CodeActions { .. } = query {
                assert_eq!(
                    params["context"]["only"],
                    serde_json::json!(["quickfix", "refactor.extract"]),
                    "duplicates removed, order fixed"
                );
                assert_eq!(params["context"]["diagnostics"], serde_json::json!([]));
            }
            if expected == "workspace/symbol" {
                assert_eq!(params["query"], serde_json::json!("add"));
            } else {
                assert_eq!(
                    params["textDocument"]["uri"],
                    serde_json::json!("file:///source/src/lib.rs")
                );
            }
        }
        Ok(())
    }

    #[test]
    fn an_empty_only_filter_is_absent_rather_than_an_empty_array()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = bundle(&[("src/lib.rs", b"pub fn add() {}\n")])?;
        let file = domain::AnalyzerFile::new("src/lib.rs".into())?;
        let opened = document(&source, &file).map_err(|error| format!("{error:?}"))?;
        let (indices, not_utf8) = snapshot(&source);
        let query = AnalyzerQuery::CodeActions {
            file,
            range: domain::TextRange::new(
                domain::Position::new(1, 1)?,
                domain::Position::new(1, 1)?,
            )?,
            only: Vec::new(),
        };
        let question = Question {
            query: &query,
            document: Some(&opened),
            indices: &indices,
            not_utf8,
        };
        let (_, params) = request_for(&question).map_err(|error| format!("{error:?}"))?;
        assert!(
            params["context"].get("only").is_none(),
            "an empty `only` would mean no kind is acceptable"
        );
        Ok(())
    }

    #[test]
    fn wire_positions_are_zero_based_utf8_offsets() -> Result<(), Box<dyn std::error::Error>> {
        // An astral scalar before the cursor: the LSP character is a byte
        // offset inside the line under `utf-8`, not a scalar count.
        let source = bundle(&[("src/lib.rs", "// \u{1f600}x\nfn f() {}\n".as_bytes())])?;
        let file = domain::AnalyzerFile::new("src/lib.rs".into())?;
        let opened = document(&source, &file).map_err(|error| format!("{error:?}"))?;
        let (indices, not_utf8) = snapshot(&source);
        let query = AnalyzerQuery::References {
            file,
            // Line 1, fifth scalar: `/`, `/`, ` `, emoji, then `x`.
            position: domain::Position::new(1, 5)?,
            include_declaration: false,
        };
        let question = Question {
            query: &query,
            document: Some(&opened),
            indices: &indices,
            not_utf8,
        };
        let (_, params) = request_for(&question).map_err(|error| format!("{error:?}"))?;
        assert_eq!(params["position"]["line"], serde_json::json!(0));
        assert_eq!(
            params["position"]["character"],
            serde_json::json!(7),
            "three ASCII bytes plus the four bytes of the astral scalar"
        );
        Ok(())
    }

    #[test]
    fn a_session_fault_maps_to_exactly_one_named_failure() {
        use domain::AnalyzerFailure as Failure;
        use lsp_codec::CodecError as Codec;
        assert_eq!(
            classify(SessionError::Timeout, Stage::Initialize),
            Failure::TimeoutInitialize
        );
        assert_eq!(
            classify(SessionError::Timeout, Stage::Query),
            Failure::TimeoutQuery
        );
        assert_eq!(
            classify(SessionError::Cancelled, Stage::Query),
            Failure::Cancelled
        );
        assert_eq!(classify(SessionError::Eof, Stage::Query), Failure::Crashed);
        assert_eq!(classify(SessionError::Io, Stage::Query), Failure::Crashed);
        assert_eq!(
            classify(SessionError::Codec(Codec::FrameLimit), Stage::Query),
            Failure::FrameTooLarge,
            "an oversized Content-Length is refused while parsing the header"
        );
        assert_eq!(
            classify(SessionError::Codec(Codec::MalformedHeader), Stage::Query),
            Failure::MalformedHeader,
            "every other refused header is not evidence about the frame bound"
        );
        for limit in [
            SessionError::Codec(Codec::MessageLimit),
            SessionError::Codec(Codec::ByteLimit),
            SessionError::MessageLimit,
            SessionError::ByteLimit,
            SessionError::FrameLimit,
        ] {
            assert_eq!(classify(limit, Stage::Query), Failure::ProtocolLimit);
        }
        for violation in [
            SessionError::Codec(Codec::MalformedMessage),
            SessionError::Codec(Codec::BatchRejected),
            SessionError::Codec(Codec::UnknownResponseId),
            SessionError::Encode,
        ] {
            assert_eq!(
                classify(violation, Stage::Query),
                Failure::ProtocolViolation
            );
        }
    }

    #[test]
    fn content_modified_is_a_crash_and_never_a_retry() {
        use domain::AnalyzerFailure as Failure;
        assert_eq!(
            server_error(lsp_codec::ResponseError::CONTENT_MODIFIED),
            Failure::Crashed
        );
        assert_eq!(
            server_error(lsp_codec::ResponseError::REQUEST_CANCELLED),
            Failure::Cancelled
        );
        assert_eq!(server_error(-32603), Failure::ServerError);
    }

    #[test]
    fn a_visible_limit_always_makes_the_result_incomplete() -> Result<(), Box<dyn std::error::Error>>
    {
        let identity = domain::AnalyzerRuntime {
            version: domain::NonEmptyText::try_from(ANALYZER_VERSION.to_owned())?,
            binary_sha256: ANALYZER_BINARY_SHA256.parse()?,
            image_id: domain::NonEmptyText::try_from(APPROVED_M6_IMAGE.to_owned())?,
            config_digest: analyzer_config_digest().map_err(|error| format!("{error:?}"))?,
        };
        let conversation = Conversation {
            session: None,
            encoding: Some(domain::PositionEncoding::Utf8),
            readiness: domain::AnalyzerReadiness::Quiescent {
                elapsed_ms: 5,
                health: domain::ServerHealth::Ok,
            },
            outcome: Some(domain::AnalyzerOutcome::Answered(
                domain::AnalyzerResult::References(Vec::new()),
            )),
            reasons: vec![domain::IncompleteReason::LimitVisible],
            omissions: vec![domain::Omission {
                kind: domain::OmissionKind::LimitVisible,
                count: 7,
            }],
            oom_killed: Some(false),
            stage: Stage::Query,
        };
        let execution = assemble(identity, conversation, None, Instant::now())
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            execution.completeness.state(),
            domain::CompletenessState::Incomplete
        );
        assert_eq!(execution.completeness.omissions().len(), 1);
        assert_eq!(execution.session.stop, domain::SessionStop::NotStarted);
        Ok(())
    }

    #[test]
    fn a_call_cut_short_before_any_session_is_a_total_timeout()
    -> Result<(), Box<dyn std::error::Error>> {
        let identity = domain::AnalyzerRuntime {
            version: domain::NonEmptyText::try_from(ANALYZER_VERSION.to_owned())?,
            binary_sha256: ANALYZER_BINARY_SHA256.parse()?,
            image_id: domain::NonEmptyText::try_from(APPROVED_M6_IMAGE.to_owned())?,
            config_digest: analyzer_config_digest().map_err(|error| format!("{error:?}"))?,
        };
        let timed_out = assemble(
            identity.clone(),
            Conversation::unstarted(),
            Some(Stop::TimedOut),
            Instant::now(),
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            timed_out.failure(),
            Some(domain::AnalyzerFailure::TimeoutTotal)
        );
        assert_eq!(timed_out.termination, ExecutionTermination::TimedOut);
        let cancelled = assemble(
            identity,
            Conversation::unstarted(),
            Some(Stop::Cancelled),
            Instant::now(),
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            cancelled.failure(),
            Some(domain::AnalyzerFailure::Cancelled)
        );
        assert_eq!(cancelled.termination, ExecutionTermination::Cancelled);
        Ok(())
    }

    #[test]
    fn the_initialize_budget_cannot_be_raised_above_the_adr_ceiling() {
        let limits = ExecutionLimits::new_job(180_000, 512 * 1024);
        let Some(limits) = limits else {
            return;
        };
        assert!(
            AnalyzerBudgets::with_initialize(limits, Duration::from_secs(61)).is_none(),
            "60 s is the ceiling of ADR-084 §8"
        );
        assert!(AnalyzerBudgets::with_initialize(limits, Duration::ZERO).is_none());
        assert!(AnalyzerBudgets::with_initialize(limits, Duration::from_secs(1)).is_some());
        assert_eq!(
            AnalyzerBudgets::standard(limits).initialize,
            Duration::from_secs(domain::INITIALIZE_TIMEOUT_SECONDS)
        );
        assert_eq!(
            AnalyzerBudgets::standard(limits).query,
            Duration::from_secs(domain::QUERY_TIMEOUT_SECONDS)
        );
    }
}
