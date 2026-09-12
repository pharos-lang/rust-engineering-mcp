use super::*;
use rust_engineering_application::analyzer::AnalyzerSnapshot;
use rust_engineering_domain::{
    AnalyzerFailure, AnalyzerOutcome, AnalyzerReadiness, AnalyzerResult, AnalyzerRuntime,
    Completeness, ExecutionTermination, InspectionSemantics, NonEmptyText, PositionEncoding,
    ServerHealth, SessionStop, SessionSummary,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn fingerprint(value: u8) -> Result<domain::SourceFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{value:064x}").parse()?)
}

fn identity() -> Result<AnalyzerRuntime, Box<dyn std::error::Error>> {
    Ok(AnalyzerRuntime {
        version: NonEmptyText::try_from("rust-analyzer 1.98.1 (48a229c 2026-09-01)".to_owned())?,
        binary_sha256: fingerprint(1)?,
        image_id: NonEmptyText::try_from("sha256:m6".to_owned())?,
        config_digest: fingerprint(2)?,
    })
}

fn session(
    kill_error: Option<&str>,
    reap_error: Option<&str>,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    Ok(SessionSummary {
        stop: SessionStop::Exited,
        exit_code: Some(0),
        messages_in: 5,
        messages_out: 6,
        bytes_in: 3_804,
        bytes_out: 1_934,
        stderr_bytes: 0,
        stderr_sha256: fingerprint(3)?,
        stderr_truncated: false,
        server_requests_refused: 0,
        notifications_dropped: 0,
        late_responses: 0,
        status_transcript: Vec::new(),
        status_notifications: 1,
        fault: None,
        declared_frame_bytes: None,
        kill_error: kill_error
            .map(|value| NonEmptyText::try_from(value.to_owned()))
            .transpose()?,
        reap_error: reap_error
            .map(|value| NonEmptyText::try_from(value.to_owned()))
            .transpose()?,
        duration_ms: 400,
    })
}

fn report(
    execution: domain::AnalyzerExecution,
) -> Result<AnalyzerReport, Box<dyn std::error::Error>> {
    Ok(AnalyzerReport {
        project_ref: "prj_00000000000000000000000000000001".parse()?,
        project_identity_fingerprint: format!("sha256:{:064x}", 9u8).parse()?,
        snapshot: AnalyzerSnapshot {
            source_fingerprint: fingerprint(4)?,
            files: 2,
            semantics: InspectionSemantics::LatestKnown,
            atomic: false,
        },
        execution,
    })
}

fn answered() -> Result<domain::AnalyzerExecution, Box<dyn std::error::Error>> {
    Ok(domain::AnalyzerExecution {
        identity: identity()?,
        position_encoding: Some(PositionEncoding::Utf8),
        readiness: AnalyzerReadiness::Quiescent {
            elapsed_ms: 362,
            health: ServerHealth::Ok,
        },
        outcome: AnalyzerOutcome::Answered(AnalyzerResult::DocumentSymbols(Vec::new())),
        completeness: Completeness::complete(),
        session: session(None, None)?,
        termination: ExecutionTermination::Exited,
        oom_killed: Some(false),
        call_duration_ms: 1_039,
    })
}

fn failed(
    failure: AnalyzerFailure,
) -> Result<domain::AnalyzerExecution, Box<dyn std::error::Error>> {
    Ok(domain::AnalyzerExecution {
        identity: identity()?,
        position_encoding: Some(PositionEncoding::Utf8),
        readiness: AnalyzerReadiness::NotReady { elapsed_ms: 12 },
        outcome: AnalyzerOutcome::Failed(failure),
        completeness: Completeness::incomplete(Vec::new()),
        session: session(None, None)?,
        termination: ExecutionTermination::Exited,
        oom_killed: Some(false),
        call_duration_ms: 40,
    })
}

fn status_of(value: &Output) -> &'static str {
    match value.outcome {
        Outcome::Passed { .. } => "passed",
        Outcome::Blocked { .. } => "blocked",
        Outcome::Unavailable { .. } => "unavailable",
        Outcome::Cancelled { .. } => "cancelled",
    }
}

fn code_of(value: &Output) -> Option<&Code> {
    match &value.outcome {
        Outcome::Blocked { error_code, .. } | Outcome::Unavailable { error_code, .. } => {
            Some(error_code)
        }
        Outcome::Passed { .. } | Outcome::Cancelled { .. } => None,
    }
}

#[test]
fn an_answered_execution_is_passed_with_symbols() -> TestResult {
    let value = output(Ok(report(answered()?)?), 5, 60)?;
    assert_eq!(status_of(&value), "passed");
    let Outcome::Passed { data, .. } = &value.outcome else {
        return Err("expected passed".into());
    };
    assert!(matches!(data.symbols, Some(schemas::Symbols::Document(_))));
    assert_eq!(data.omitted, 0);
    Ok(())
}

/// D3: every `AnalyzerFailure` this tool can observe maps to exactly the
/// closed `(status, error_code)` pair the tool contract publishes.
/// `Cancelled` and `PositionOutOfRange` are covered by their own tests below.
#[test]
fn every_analyzer_failure_maps_to_its_closed_code() -> TestResult {
    let table: &[(AnalyzerFailure, &str, Code)] = &[
        (
            AnalyzerFailure::FileNotInSnapshot,
            "blocked",
            Code::FileNotInSnapshot,
        ),
        (AnalyzerFailure::FileNotUtf8, "blocked", Code::FileNotUtf8),
        (
            AnalyzerFailure::UnsupportedProjectConfig,
            "blocked",
            Code::UnsupportedProjectConfig,
        ),
        (
            AnalyzerFailure::CapabilityMismatch,
            "unavailable",
            Code::AnalyzerCapabilityMismatch,
        ),
        (
            AnalyzerFailure::NotReady,
            "unavailable",
            Code::AnalyzerNotReady,
        ),
        (
            AnalyzerFailure::Crashed,
            "unavailable",
            Code::AnalyzerCrashed,
        ),
        (
            AnalyzerFailure::ProtocolViolation,
            "unavailable",
            Code::AnalyzerCrashed,
        ),
        (
            AnalyzerFailure::ServerError,
            "unavailable",
            Code::AnalyzerCrashed,
        ),
        (
            AnalyzerFailure::ProtocolLimit,
            "unavailable",
            Code::MessageLimit,
        ),
        (
            AnalyzerFailure::FrameTooLarge,
            "unavailable",
            Code::FrameLimit,
        ),
        (
            AnalyzerFailure::MalformedHeader,
            "unavailable",
            Code::FrameLimit,
        ),
        (
            AnalyzerFailure::TimeoutInitialize,
            "unavailable",
            Code::TimeoutInitialize,
        ),
        (
            AnalyzerFailure::TimeoutQuery,
            "unavailable",
            Code::TimeoutQuery,
        ),
        (
            AnalyzerFailure::TimeoutTotal,
            "unavailable",
            Code::TimeoutTotal,
        ),
    ];
    for (failure, expected_status, expected_code) in table.iter().copied() {
        let value = output(Ok(report(failed(failure)?)?), 1, 60)?;
        assert_eq!(status_of(&value), expected_status, "{failure:?}");
        assert_eq!(
            code_of(&value),
            Some(&expected_code),
            "{failure:?} error_code"
        );
    }
    Ok(())
}

#[test]
fn cancelled_mid_session_is_the_cancelled_status_with_no_code() -> TestResult {
    let value = output(Ok(report(failed(AnalyzerFailure::Cancelled)?)?), 1, 60)?;
    assert_eq!(status_of(&value), "cancelled");
    assert_eq!(code_of(&value), None);
    Ok(())
}

#[test]
fn position_out_of_range_is_an_internal_guard_not_a_wire_code() -> TestResult {
    let result = output(
        Ok(report(failed(AnalyzerFailure::PositionOutOfRange)?)?),
        1,
        60,
    );
    assert!(result.is_err(), "unreachable for this tool's own queries");
    Ok(())
}

#[test]
fn conflict_and_file_not_in_snapshot_are_blocked_with_no_data() -> TestResult {
    for (error, expected_code) in [
        (AnalyzerRequestError::Conflict, Code::Conflict),
        (
            AnalyzerRequestError::FileNotInSnapshot,
            Code::FileNotInSnapshot,
        ),
    ] {
        let value = output(Err(error), 1, 60)?;
        assert_eq!(status_of(&value), "blocked");
        assert_eq!(code_of(&value), Some(&expected_code));
        let Outcome::Blocked { data, .. } = &value.outcome else {
            return Err("expected blocked".into());
        };
        assert!(data.is_none(), "no stale data ever published");
    }
    Ok(())
}

#[test]
fn cancellation_before_any_execution_is_the_cancelled_status() -> TestResult {
    use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
    for error in [
        AnalyzerRequestError::Inspection(InspectionError::Project(ProjectError::Cancelled)),
        AnalyzerRequestError::Inspection(InspectionError::Execution(ExecutionError::Cancelled)),
    ] {
        let value = output(Err(error), 1, 60)?;
        assert_eq!(status_of(&value), "cancelled");
    }
    Ok(())
}

#[test]
fn runtime_unavailable_is_sandbox_denied() -> TestResult {
    use rust_engineering_application::{ExecutionError, InspectionError};
    let value = output(
        Err(AnalyzerRequestError::Inspection(
            InspectionError::Execution(ExecutionError::Unavailable),
        )),
        1,
        60,
    )?;
    assert_eq!(status_of(&value), "unavailable");
    assert_eq!(code_of(&value), Some(&Code::SandboxDenied));
    Ok(())
}

#[test]
fn no_stderr_or_kill_reap_text_ever_reaches_the_wire() -> TestResult {
    let mut execution = answered()?;
    execution.session = session(
        Some("SECRET_KILL_ERROR_should_never_leak"),
        Some("SECRET_REAP_ERROR_should_never_leak"),
    )?;
    let value = output(Ok(report(execution)?), 1, 60)?;
    let encoded = serde_json::to_string(&value)?;
    assert!(!encoded.contains("SECRET_KILL_ERROR_should_never_leak"));
    assert!(!encoded.contains("SECRET_REAP_ERROR_should_never_leak"));
    Ok(())
}

#[test]
fn oversized_symbols_are_trimmed_under_the_result_budget() -> TestResult {
    let mut execution = answered()?;
    // At the peer-text bound (V05 P2, `lsp_codec::MAX_PEER_NAME_CHARS`), not
    // the pre-V05b 2,000 chars: a longer name now fails the wire schema's own
    // `maxLength` before the trim loop ever runs. 400 bound-length names
    // still clear the `MAX_RESULT / 4` trim threshold below.
    let long_name = "s".repeat(256);
    let range = domain::TextRange::new(domain::Position::new(1, 1)?, domain::Position::new(1, 2)?)?;
    let mut symbols = Vec::with_capacity(400);
    for _ in 0..400 {
        symbols.push(domain::DocumentSymbol::new(
            NonEmptyText::try_from(long_name.clone())?,
            domain::SymbolKind::Function,
            None,
            false,
            range,
            range,
            0,
        )?);
    }
    execution.outcome = AnalyzerOutcome::Answered(AnalyzerResult::DocumentSymbols(symbols));
    let contract = Contract::<Input, Output>::new()?;
    let value = output(Ok(report(execution)?), 1, 60)?;
    let encoded = encode_bounded(&contract, value)?;
    let wire = serde_json::to_vec(&encoded)?;
    assert!(wire.len() <= MAX_RESULT, "{} bytes", wire.len());
    let structured = serde_json::to_value(&encoded)?;
    let completeness = &structured["structuredContent"]["data"]["completeness"];
    assert_eq!(completeness["state"], "incomplete");
    let reasons = completeness["reasons"]
        .as_array()
        .ok_or("expected reasons array")?;
    assert!(reasons.iter().any(|reason| reason == "result_limit"));
    Ok(())
}

/// V05 P3: the worst case a peer-text-bound-respecting session can produce —
/// every one of the 512 visible symbols at the maximal `name`/`detail`
/// length (V05 P2) — must still fit under [`MAX_RESULT`] once encoded.
#[test]
fn worst_case_512_maximal_symbols_still_fit_the_result_budget() -> TestResult {
    let mut execution = answered()?;
    let max_name = "n".repeat(256);
    let max_detail = "d".repeat(1_024);
    let range = domain::TextRange::new(domain::Position::new(1, 1)?, domain::Position::new(1, 2)?)?;
    let mut symbols = Vec::with_capacity(domain::MAX_VISIBLE_RESULTS);
    for _ in 0..domain::MAX_VISIBLE_RESULTS {
        symbols.push(domain::DocumentSymbol::new(
            NonEmptyText::try_from(max_name.clone())?,
            domain::SymbolKind::Function,
            Some(max_detail.clone()),
            false,
            range,
            range,
            0,
        )?);
    }
    execution.outcome = AnalyzerOutcome::Answered(AnalyzerResult::DocumentSymbols(symbols));
    let contract = Contract::<Input, Output>::new()?;
    let value = output(Ok(report(execution)?), 1, 60)?;
    let encoded = encode_bounded(&contract, value)?;
    let wire = serde_json::to_vec(&encoded)?;
    assert!(wire.len() <= MAX_RESULT, "{} bytes", wire.len());
    Ok(())
}

/// V05 P1: once every symbol is already trimmed, an encoded result still over
/// budget is `unavailable` — the adapter failing its own output contract, not
/// something the caller could have asked differently to avoid — never
/// `blocked`. A tiny test-only budget exercises the fallback without needing
/// real 512 KiB fixture data.
#[test]
fn result_limit_fallback_is_unavailable_not_blocked() -> TestResult {
    let contract = Contract::<Input, Output>::new()?;
    let value = output(Ok(report(answered()?)?), 1, 60)?;
    let encoded = encode_bounded_within(&contract, value, 16)?;
    let structured = serde_json::to_value(&encoded)?;
    assert_eq!(structured["structuredContent"]["status"], "unavailable");
    assert_eq!(
        structured["structuredContent"]["error_code"],
        "RESULT_LIMIT"
    );
    assert!(structured["structuredContent"]["data"].is_null());
    Ok(())
}

/// V05 P1: the bootstrap refusal is `blocked/SANDBOX_DENIED` with no `data`,
/// distinct from the `unavailable/SANDBOX_DENIED` [`operational`] publishes
/// once discovery has completed but the runtime itself is unavailable.
#[test]
fn bootstrap_refusal_is_blocked_sandbox_denied_with_no_data() -> TestResult {
    let value = bootstrap_refusal(7);
    assert_eq!(status_of(&value), "blocked");
    assert_eq!(code_of(&value), Some(&Code::SandboxDenied));
    let Outcome::Blocked { data, .. } = &value.outcome else {
        return Err("expected blocked".into());
    };
    assert!(data.is_none());
    assert_eq!(value.duration_ms, 7);
    Ok(())
}

/// V05 P2: `initialize`/`query` are the ADR-084 §8 ceilings only when the
/// caller's own `timeout_seconds` leaves room for them; `total_timeout_seconds`
/// always mirrors the caller's budget verbatim.
#[test]
fn effective_limits_are_capped_by_the_callers_total_timeout() -> TestResult {
    let short = output(Ok(report(answered()?)?), 1, 10)?;
    let Outcome::Passed { data, .. } = &short.outcome else {
        return Err("expected passed".into());
    };
    assert_eq!(data.limits.total_timeout_seconds, 10);
    assert_eq!(data.limits.initialize_timeout_seconds, 10);
    assert_eq!(data.limits.query_timeout_seconds, 10);

    let ample = output(Ok(report(answered()?)?), 1, 120)?;
    let Outcome::Passed { data, .. } = &ample.outcome else {
        return Err("expected passed".into());
    };
    assert_eq!(data.limits.total_timeout_seconds, 120);
    assert_eq!(data.limits.initialize_timeout_seconds, 60);
    assert_eq!(data.limits.query_timeout_seconds, 30);
    Ok(())
}

/// V05 P2: a `detail` the peer-boundary codec truncated is mirrored onto the
/// wire `DocumentSymbol` as `detail_truncated: true`; an entry the codec never
/// touched publishes `false`.
#[test]
fn detail_truncated_is_mirrored_onto_the_wire_document_symbol() -> TestResult {
    let mut execution = answered()?;
    let range = domain::TextRange::new(domain::Position::new(1, 1)?, domain::Position::new(1, 2)?)?;
    let truncated = domain::DocumentSymbol::new(
        NonEmptyText::try_from("truncated".to_owned())?,
        domain::SymbolKind::Function,
        Some("d".repeat(2_000)),
        false,
        range,
        range,
        0,
    )?
    .truncate_detail(1_024);
    let untouched = domain::DocumentSymbol::new(
        NonEmptyText::try_from("untouched".to_owned())?,
        domain::SymbolKind::Function,
        Some("short".to_owned()),
        false,
        range,
        range,
        0,
    )?;
    execution.outcome =
        AnalyzerOutcome::Answered(AnalyzerResult::DocumentSymbols(vec![truncated, untouched]));
    let value = output(Ok(report(execution)?), 1, 60)?;
    let encoded = serde_json::to_value(value)?;
    let symbols = encoded["data"]["symbols"]
        .as_array()
        .ok_or("expected symbols array")?;
    assert_eq!(symbols[0]["detail_truncated"], true);
    assert_eq!(symbols[1]["detail_truncated"], false);
    Ok(())
}
