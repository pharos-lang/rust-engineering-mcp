use super::*;
use rust_engineering_application::analyzer::AnalyzerSnapshot;
use rust_engineering_domain::{
    AnalyzerFailure, AnalyzerOutcome, AnalyzerReadiness, AnalyzerResult, AnalyzerRuntime,
    Completeness, ExecutionTermination, InspectionSemantics, NonEmptyText, PositionEncoding,
    ServerHealth, SessionStop, SessionSummary,
};

pub(super) type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(super) fn fingerprint(
    value: u8,
) -> Result<domain::SourceFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{value:064x}").parse()?)
}

pub(super) fn identity() -> Result<AnalyzerRuntime, Box<dyn std::error::Error>> {
    Ok(AnalyzerRuntime {
        version: NonEmptyText::try_from("rust-analyzer 1.98.1 (48a229c 2026-09-01)".to_owned())?,
        binary_sha256: fingerprint(1)?,
        image_id: NonEmptyText::try_from("sha256:m6".to_owned())?,
        config_digest: fingerprint(2)?,
    })
}

pub(super) fn session(
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

pub(super) fn report(
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

pub(super) fn answered() -> Result<domain::AnalyzerExecution, Box<dyn std::error::Error>> {
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

pub(super) fn failed(
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

// =======================================================================
// `rust.analyzer.references` and `rust.analyzer.diagnostics` (W06)
// =======================================================================
//
// Scoped to its own module so the `expect`/`unwrap` allow below covers only
// these two tools' tests, not the M6-01 symbols tests above (V06 P3): fixed
// fixtures here are malformed only by mistake, and should fail immediately,
// but that leniency has no reason to reach the older tests.
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod new_tools {
    use super::*;

    fn reference(
        file: &str,
        line: u32,
        column: u32,
        len: u32,
        is_declaration: bool,
    ) -> domain::Reference {
        domain::Reference {
            file: AnalyzerFile::new(file.to_owned()).unwrap(),
            range: domain::TextRange::new(
                domain::Position::new(line, column).unwrap(),
                domain::Position::new(line, column + len).unwrap(),
            )
            .unwrap(),
            is_declaration,
        }
    }

    fn references_answered(
        items: Vec<domain::Reference>,
    ) -> Result<domain::AnalyzerExecution, Box<dyn std::error::Error>> {
        Ok(domain::AnalyzerExecution {
            outcome: AnalyzerOutcome::Answered(AnalyzerResult::References(items)),
            ..answered()?
        })
    }

    fn references_failed(failure: AnalyzerFailure) -> domain::AnalyzerExecution {
        failed(failure).unwrap()
    }

    fn references_status_of(value: &ReferencesOutput) -> &'static str {
        match value.outcome {
            ReferencesOutcome::Passed { .. } => "passed",
            ReferencesOutcome::Blocked { .. } => "blocked",
            ReferencesOutcome::Unavailable { .. } => "unavailable",
            ReferencesOutcome::Cancelled { .. } => "cancelled",
        }
    }

    fn references_code_of(value: &ReferencesOutput) -> Option<&ReferencesCode> {
        match &value.outcome {
            ReferencesOutcome::Blocked { error_code, .. }
            | ReferencesOutcome::Unavailable { error_code, .. } => Some(error_code),
            ReferencesOutcome::Passed { .. } | ReferencesOutcome::Cancelled { .. } => None,
        }
    }

    #[test]
    fn include_declaration_true_publishes_every_reference() -> TestResult {
        let items = vec![
            reference("src/lib.rs", 1, 8, 3, true),
            reference("src/lib.rs", 5, 1, 3, false),
        ];
        let value = references_output(Ok(report(references_answered(items)?)?), 5, 60, true)?;
        let ReferencesOutcome::Passed { data, .. } = &value.outcome else {
            return Err("expected passed".into());
        };
        let references = data.references.as_ref().ok_or("expected references")?;
        assert_eq!(references.len(), 2);
        assert_eq!(data.omitted_declarations, 0);
        Ok(())
    }

    #[test]
    fn include_declaration_false_removes_declarations_and_counts_them() -> TestResult {
        let items = vec![
            reference("src/lib.rs", 1, 8, 3, true),
            reference("src/lib.rs", 5, 1, 3, false),
            reference("src/lib.rs", 9, 1, 3, false),
        ];
        let value = references_output(Ok(report(references_answered(items)?)?), 5, 60, false)?;
        let ReferencesOutcome::Passed { data, .. } = &value.outcome else {
            return Err("expected passed".into());
        };
        let references = data.references.as_ref().ok_or("expected references")?;
        assert_eq!(references.len(), 2, "the declaration is removed");
        assert!(references.iter().all(|reference| !reference.is_declaration));
        assert_eq!(data.omitted_declarations, 1);
        Ok(())
    }

    /// D3: every `AnalyzerFailure` this tool can observe maps to exactly the
    /// closed `(status, error_code)` pair the tool contract publishes.
    /// `PositionOutOfRange` is reachable here (unlike `rust.analyzer.symbols`):
    /// the query itself carries a caller position.
    #[test]
    fn every_analyzer_failure_maps_to_its_closed_references_code() -> TestResult {
        let table: &[(AnalyzerFailure, &str, ReferencesCode)] = &[
            (
                AnalyzerFailure::FileNotInSnapshot,
                "blocked",
                ReferencesCode::FileNotInSnapshot,
            ),
            (
                AnalyzerFailure::FileNotUtf8,
                "blocked",
                ReferencesCode::FileNotUtf8,
            ),
            (
                AnalyzerFailure::UnsupportedProjectConfig,
                "blocked",
                ReferencesCode::UnsupportedProjectConfig,
            ),
            (
                AnalyzerFailure::PositionOutOfRange,
                "blocked",
                ReferencesCode::PositionOutOfRange,
            ),
            (
                AnalyzerFailure::CapabilityMismatch,
                "unavailable",
                ReferencesCode::AnalyzerCapabilityMismatch,
            ),
            (
                AnalyzerFailure::NotReady,
                "unavailable",
                ReferencesCode::AnalyzerNotReady,
            ),
            (
                AnalyzerFailure::Crashed,
                "unavailable",
                ReferencesCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ProtocolViolation,
                "unavailable",
                ReferencesCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ServerError,
                "unavailable",
                ReferencesCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ProtocolLimit,
                "unavailable",
                ReferencesCode::MessageLimit,
            ),
            (
                AnalyzerFailure::FrameTooLarge,
                "unavailable",
                ReferencesCode::FrameLimit,
            ),
            (
                AnalyzerFailure::MalformedHeader,
                "unavailable",
                ReferencesCode::FrameLimit,
            ),
            (
                AnalyzerFailure::TimeoutInitialize,
                "unavailable",
                ReferencesCode::TimeoutInitialize,
            ),
            (
                AnalyzerFailure::TimeoutQuery,
                "unavailable",
                ReferencesCode::TimeoutQuery,
            ),
            (
                AnalyzerFailure::TimeoutTotal,
                "unavailable",
                ReferencesCode::TimeoutTotal,
            ),
        ];
        for (failure, expected_status, expected_code) in table.iter().copied() {
            let value = references_output(Ok(report(references_failed(failure))?), 1, 60, true)?;
            assert_eq!(references_status_of(&value), expected_status, "{failure:?}");
            assert_eq!(
                references_code_of(&value),
                Some(&expected_code),
                "{failure:?} error_code"
            );
        }
        Ok(())
    }

    #[test]
    fn references_cancelled_mid_session_is_the_cancelled_status_with_no_code() -> TestResult {
        let value = references_output(
            Ok(report(references_failed(AnalyzerFailure::Cancelled))?),
            1,
            60,
            true,
        )?;
        assert_eq!(references_status_of(&value), "cancelled");
        assert_eq!(references_code_of(&value), None);
        Ok(())
    }

    #[test]
    fn references_conflict_file_not_in_snapshot_and_position_out_of_range_are_blocked_with_no_data()
    -> TestResult {
        for (error, expected_code) in [
            (AnalyzerRequestError::Conflict, ReferencesCode::Conflict),
            (
                AnalyzerRequestError::FileNotInSnapshot,
                ReferencesCode::FileNotInSnapshot,
            ),
            (
                AnalyzerRequestError::PositionOutOfRange,
                ReferencesCode::PositionOutOfRange,
            ),
        ] {
            let value = references_output(Err(error), 1, 60, true)?;
            assert_eq!(references_status_of(&value), "blocked");
            assert_eq!(references_code_of(&value), Some(&expected_code));
            let ReferencesOutcome::Blocked { data, .. } = &value.outcome else {
                return Err("expected blocked".into());
            };
            assert!(data.is_none(), "no stale data ever published");
        }
        Ok(())
    }

    #[test]
    fn references_no_stderr_or_kill_reap_text_ever_reaches_the_wire() -> TestResult {
        let mut execution = references_answered(vec![reference("src/lib.rs", 1, 8, 3, true)])?;
        execution.session = session(
            Some("SECRET_KILL_ERROR_should_never_leak"),
            Some("SECRET_REAP_ERROR_should_never_leak"),
        )?;
        let value = references_output(Ok(report(execution)?), 1, 60, true)?;
        let encoded = serde_json::to_string(&value)?;
        assert!(!encoded.contains("SECRET_KILL_ERROR_should_never_leak"));
        assert!(!encoded.contains("SECRET_REAP_ERROR_should_never_leak"));
        Ok(())
    }

    /// Unlike a `DocumentSymbol` (which carries a free-form `detail` up to 1,024
    /// scalars), a `Reference` is just a file, a range and a bool: even the
    /// domain's own `MAX_VISIBLE_RESULTS` cap of maximal-length entries fits
    /// comfortably under the result budget without ever reaching the trim loop.
    /// This is the same worst-case shape as `worst_case_512_maximal_symbols_...`,
    /// adapted to the leaner type.
    #[test]
    fn references_worst_case_512_still_fit_the_result_budget_without_trimming() -> TestResult {
        let max_file = format!("src/{}.rs", "f".repeat(93));
        let mut items = Vec::with_capacity(domain::MAX_VISIBLE_RESULTS);
        for index in 0..domain::MAX_VISIBLE_RESULTS as u32 {
            items.push(reference(&max_file, index + 1, 1, 3, false));
        }
        let value = references_output(Ok(report(references_answered(items)?)?), 1, 60, true)?;
        let contract = Contract::<ReferencesInput, ReferencesOutput>::new()?;
        let encoded = encode_references_bounded(&contract, value)?;
        let wire = serde_json::to_vec(&encoded)?;
        assert!(wire.len() <= MAX_RESULT, "{} bytes", wire.len());
        let structured = serde_json::to_value(&encoded)?;
        let completeness = &structured["structuredContent"]["data"]["completeness"];
        assert_eq!(completeness["state"], "complete");
        Ok(())
    }

    #[test]
    fn references_result_limit_fallback_is_unavailable_not_blocked() -> TestResult {
        let contract = Contract::<ReferencesInput, ReferencesOutput>::new()?;
        let value = references_output(
            Ok(report(references_answered(vec![reference(
                "src/lib.rs",
                1,
                8,
                3,
                true,
            )])?)?),
            1,
            60,
            true,
        )?;
        let encoded = encode_references_bounded_within(&contract, value, 16)?;
        let structured = serde_json::to_value(&encoded)?;
        assert_eq!(structured["structuredContent"]["status"], "unavailable");
        assert_eq!(
            structured["structuredContent"]["error_code"],
            "RESULT_LIMIT"
        );
        assert!(structured["structuredContent"]["data"].is_null());
        Ok(())
    }

    #[test]
    fn references_bootstrap_refusal_is_blocked_sandbox_denied_with_no_data() -> TestResult {
        let value = references_bootstrap_refusal(7);
        assert_eq!(references_status_of(&value), "blocked");
        assert_eq!(
            references_code_of(&value),
            Some(&ReferencesCode::SandboxDenied)
        );
        let ReferencesOutcome::Blocked { data, .. } = &value.outcome else {
            return Err("expected blocked".into());
        };
        assert!(data.is_none());
        assert_eq!(value.duration_ms, 7);
        Ok(())
    }

    // =======================================================================
    // `rust.analyzer.diagnostics`
    // =======================================================================

    fn analyzer_diagnostic(file: &str, line: u32, message: &str) -> domain::AnalyzerDiagnostic {
        domain::AnalyzerDiagnostic::new(
            AnalyzerFile::new(file.to_owned()).unwrap(),
            domain::TextRange::new(
                domain::Position::new(line, 1).unwrap(),
                domain::Position::new(line, 2).unwrap(),
            )
            .unwrap(),
            domain::DiagnosticSeverity::Error,
            Some("E0433".to_owned()),
            NonEmptyText::try_from(message.to_owned()).unwrap(),
            Vec::new(),
        )
        .unwrap()
    }

    fn diagnostics_answered(
        items: Vec<domain::AnalyzerDiagnostic>,
    ) -> Result<domain::AnalyzerExecution, Box<dyn std::error::Error>> {
        Ok(domain::AnalyzerExecution {
            outcome: AnalyzerOutcome::Answered(AnalyzerResult::Diagnostics(items)),
            ..answered()?
        })
    }

    fn diagnostics_failed(failure: AnalyzerFailure) -> domain::AnalyzerExecution {
        failed(failure).unwrap()
    }

    fn diagnostics_status_of(value: &DiagnosticsOutput) -> &'static str {
        match value.outcome {
            DiagnosticsOutcome::Passed { .. } => "passed",
            DiagnosticsOutcome::Blocked { .. } => "blocked",
            DiagnosticsOutcome::Unavailable { .. } => "unavailable",
            DiagnosticsOutcome::Cancelled { .. } => "cancelled",
        }
    }

    fn diagnostics_code_of(value: &DiagnosticsOutput) -> Option<&DiagnosticsCode> {
        match &value.outcome {
            DiagnosticsOutcome::Blocked { error_code, .. }
            | DiagnosticsOutcome::Unavailable { error_code, .. } => Some(error_code),
            DiagnosticsOutcome::Passed { .. } | DiagnosticsOutcome::Cancelled { .. } => None,
        }
    }

    #[test]
    fn an_answered_diagnostics_execution_is_passed_with_diagnostics() -> TestResult {
        let items = vec![analyzer_diagnostic("src/lib.rs", 1, "unresolved macro")];
        let value = diagnostics_output(Ok(report(diagnostics_answered(items)?)?), 5, 60)?;
        assert_eq!(diagnostics_status_of(&value), "passed");
        let DiagnosticsOutcome::Passed { data, .. } = &value.outcome else {
            return Err("expected passed".into());
        };
        let diagnostics = data.diagnostics.as_ref().ok_or("expected diagnostics")?;
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].source, "rust-analyzer");
        assert_eq!(diagnostics[0].code.as_deref(), Some("E0433"));
        assert_eq!(data.omitted, 0);
        Ok(())
    }

    /// Unlike `rust.analyzer.references`, `PositionOutOfRange` stays unreachable:
    /// no query this tool sends carries a caller position.
    #[test]
    fn every_analyzer_failure_maps_to_its_closed_diagnostics_code() -> TestResult {
        let table: &[(AnalyzerFailure, &str, DiagnosticsCode)] = &[
            (
                AnalyzerFailure::FileNotInSnapshot,
                "blocked",
                DiagnosticsCode::FileNotInSnapshot,
            ),
            (
                AnalyzerFailure::FileNotUtf8,
                "blocked",
                DiagnosticsCode::FileNotUtf8,
            ),
            (
                AnalyzerFailure::UnsupportedProjectConfig,
                "blocked",
                DiagnosticsCode::UnsupportedProjectConfig,
            ),
            (
                AnalyzerFailure::CapabilityMismatch,
                "unavailable",
                DiagnosticsCode::AnalyzerCapabilityMismatch,
            ),
            (
                AnalyzerFailure::NotReady,
                "unavailable",
                DiagnosticsCode::AnalyzerNotReady,
            ),
            (
                AnalyzerFailure::Crashed,
                "unavailable",
                DiagnosticsCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ProtocolViolation,
                "unavailable",
                DiagnosticsCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ServerError,
                "unavailable",
                DiagnosticsCode::AnalyzerCrashed,
            ),
            (
                AnalyzerFailure::ProtocolLimit,
                "unavailable",
                DiagnosticsCode::MessageLimit,
            ),
            (
                AnalyzerFailure::FrameTooLarge,
                "unavailable",
                DiagnosticsCode::FrameLimit,
            ),
            (
                AnalyzerFailure::MalformedHeader,
                "unavailable",
                DiagnosticsCode::FrameLimit,
            ),
            (
                AnalyzerFailure::TimeoutInitialize,
                "unavailable",
                DiagnosticsCode::TimeoutInitialize,
            ),
            (
                AnalyzerFailure::TimeoutQuery,
                "unavailable",
                DiagnosticsCode::TimeoutQuery,
            ),
            (
                AnalyzerFailure::TimeoutTotal,
                "unavailable",
                DiagnosticsCode::TimeoutTotal,
            ),
        ];
        for (failure, expected_status, expected_code) in table.iter().copied() {
            let value = diagnostics_output(Ok(report(diagnostics_failed(failure))?), 1, 60)?;
            assert_eq!(
                diagnostics_status_of(&value),
                expected_status,
                "{failure:?}"
            );
            assert_eq!(
                diagnostics_code_of(&value),
                Some(&expected_code),
                "{failure:?} error_code"
            );
        }
        Ok(())
    }

    #[test]
    fn diagnostics_position_out_of_range_is_an_internal_guard_not_a_wire_code() -> TestResult {
        let result = diagnostics_output(
            Ok(report(diagnostics_failed(
                AnalyzerFailure::PositionOutOfRange,
            ))?),
            1,
            60,
        );
        assert!(result.is_err(), "unreachable for this tool's own queries");
        let err = diagnostics_output(Err(AnalyzerRequestError::PositionOutOfRange), 1, 60);
        assert!(err.is_err(), "this tool never queries a position");
        Ok(())
    }

    #[test]
    fn diagnostics_cancelled_mid_session_is_the_cancelled_status_with_no_code() -> TestResult {
        let value = diagnostics_output(
            Ok(report(diagnostics_failed(AnalyzerFailure::Cancelled))?),
            1,
            60,
        )?;
        assert_eq!(diagnostics_status_of(&value), "cancelled");
        assert_eq!(diagnostics_code_of(&value), None);
        Ok(())
    }

    #[test]
    fn diagnostics_conflict_and_file_not_in_snapshot_are_blocked_with_no_data() -> TestResult {
        for (error, expected_code) in [
            (AnalyzerRequestError::Conflict, DiagnosticsCode::Conflict),
            (
                AnalyzerRequestError::FileNotInSnapshot,
                DiagnosticsCode::FileNotInSnapshot,
            ),
        ] {
            let value = diagnostics_output(Err(error), 1, 60)?;
            assert_eq!(diagnostics_status_of(&value), "blocked");
            assert_eq!(diagnostics_code_of(&value), Some(&expected_code));
            let DiagnosticsOutcome::Blocked { data, .. } = &value.outcome else {
                return Err("expected blocked".into());
            };
            assert!(data.is_none(), "no stale data ever published");
        }
        Ok(())
    }

    #[test]
    fn diagnostics_no_stderr_or_kill_reap_text_ever_reaches_the_wire() -> TestResult {
        let mut execution = diagnostics_answered(vec![analyzer_diagnostic(
            "src/lib.rs",
            1,
            "unresolved macro",
        )])?;
        execution.session = session(
            Some("SECRET_KILL_ERROR_should_never_leak"),
            Some("SECRET_REAP_ERROR_should_never_leak"),
        )?;
        let value = diagnostics_output(Ok(report(execution)?), 1, 60)?;
        let encoded = serde_json::to_string(&value)?;
        assert!(!encoded.contains("SECRET_KILL_ERROR_should_never_leak"));
        assert!(!encoded.contains("SECRET_REAP_ERROR_should_never_leak"));
        Ok(())
    }

    #[test]
    fn diagnostics_oversized_are_trimmed_under_the_result_budget() -> TestResult {
        let mut items = Vec::with_capacity(400);
        for index in 0..400u32 {
            items.push(analyzer_diagnostic(
                "src/lib.rs",
                index + 1,
                &"m".repeat(4_096),
            ));
        }
        let value = diagnostics_output(Ok(report(diagnostics_answered(items)?)?), 1, 60)?;
        let contract = Contract::<DiagnosticsInput, DiagnosticsOutput>::new()?;
        let encoded = encode_diagnostics_bounded(&contract, value)?;
        let wire = serde_json::to_vec(&encoded)?;
        assert!(wire.len() <= MAX_RESULT, "{} bytes", wire.len());
        let structured = serde_json::to_value(&encoded)?;
        let completeness = &structured["structuredContent"]["data"]["completeness"];
        assert_eq!(completeness["state"], "incomplete");
        Ok(())
    }

    /// V06 P2: `related` is bounded upstream (`lsp_codec`) to
    /// [`domain::MAX_RELATED_INFORMATION`], so the wire schema's own
    /// `maxItems: 32` can never be violated by construction; a diagnostic
    /// carrying exactly that many still converts and validates.
    #[test]
    fn a_diagnostic_with_32_related_entries_converts_and_validates_against_its_own_schema()
    -> TestResult {
        let file = AnalyzerFile::new("src/lib.rs".to_owned())?;
        let range =
            domain::TextRange::new(domain::Position::new(1, 1)?, domain::Position::new(1, 2)?)?;
        let related = (0..32)
            .map(|n| {
                Ok::<_, Box<dyn std::error::Error>>(domain::RelatedInformation {
                    file: file.clone(),
                    range,
                    message: NonEmptyText::try_from(format!("related {n}"))?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let diagnostic = domain::AnalyzerDiagnostic::new(
            file,
            range,
            domain::DiagnosticSeverity::Error,
            Some("E0308".to_owned()),
            NonEmptyText::try_from("mismatched types".to_owned())?,
            related,
        )?;
        let value =
            diagnostics_output(Ok(report(diagnostics_answered(vec![diagnostic])?)?), 5, 60)?;
        let contract = Contract::<DiagnosticsInput, DiagnosticsOutput>::new()?;
        let encoded = encode_diagnostics_bounded(&contract, value)?;
        let structured = serde_json::to_value(&encoded)?;
        let related_out = structured["structuredContent"]["data"]["diagnostics"][0]["related"]
            .as_array()
            .ok_or("expected related array")?;
        assert_eq!(related_out.len(), 32);
        Ok(())
    }

    #[test]
    fn diagnostics_result_limit_fallback_is_unavailable_not_blocked() -> TestResult {
        let contract = Contract::<DiagnosticsInput, DiagnosticsOutput>::new()?;
        let value = diagnostics_output(
            Ok(report(diagnostics_answered(vec![analyzer_diagnostic(
                "src/lib.rs",
                1,
                "unresolved macro",
            )])?)?),
            1,
            60,
        )?;
        let encoded = encode_diagnostics_bounded_within(&contract, value, 16)?;
        let structured = serde_json::to_value(&encoded)?;
        assert_eq!(structured["structuredContent"]["status"], "unavailable");
        assert_eq!(
            structured["structuredContent"]["error_code"],
            "RESULT_LIMIT"
        );
        assert!(structured["structuredContent"]["data"].is_null());
        Ok(())
    }

    #[test]
    fn diagnostics_bootstrap_refusal_is_blocked_sandbox_denied_with_no_data() -> TestResult {
        let value = diagnostics_bootstrap_refusal(7);
        assert_eq!(diagnostics_status_of(&value), "blocked");
        assert_eq!(
            diagnostics_code_of(&value),
            Some(&DiagnosticsCode::SandboxDenied)
        );
        let DiagnosticsOutcome::Blocked { data, .. } = &value.outcome else {
            return Err("expected blocked".into());
        };
        assert!(data.is_none());
        assert_eq!(value.duration_ms, 7);
        Ok(())
    }

    /// D25 §1.6 / D3: every control character other than newline/tab is replaced
    /// before a diagnostic `message` reaches the wire, and an over-4,096-scalar
    /// message is truncated and flagged rather than silently cut or refused.
    #[test]
    fn diagnostic_message_control_chars_are_replaced_and_overlong_messages_are_flagged() {
        let (sanitized, truncated) = bounded_message("line one\ttabbed\nline two\x07bell");
        assert_eq!(sanitized, "line one\ttabbed\nline two\u{fffd}bell");
        assert!(!truncated);

        let long = "a".repeat(5_000);
        let (bounded, truncated) = bounded_message(&long);
        assert_eq!(bounded.chars().count(), MAX_DIAGNOSTIC_MESSAGE_SCALARS);
        assert!(truncated);
    }

    /// W08b: beyond `char::is_control` (Cc), bidi overrides/isolates,
    /// zero-width characters and the line/paragraph separators are also
    /// neutralised before a diagnostic `message` or `code` reaches the wire.
    #[test]
    fn diagnostic_message_neutralises_every_peer_text_hazard_category() {
        for hazard in [
            '\u{202E}', // bidi override (RLO)
            '\u{2066}', // bidi isolate (LRI)
            '\u{200B}', // zero-width space
            '\u{FEFF}', // zero-width no-break space / BOM
            '\u{2028}', // line separator
            '\u{2029}', // paragraph separator
        ] {
            let (sanitized, truncated) = bounded_message(&format!("before{hazard}after"));
            assert!(!truncated);
            assert_eq!(sanitized, "before\u{fffd}after", "{hazard:?}");
        }
    }

    /// V06 P2: `code` gets the same control-character sanitization as
    /// `message` — previously only `message` was sanitized.
    #[test]
    fn diagnostic_code_control_chars_are_replaced() -> TestResult {
        let diagnostic = domain::AnalyzerDiagnostic::new(
            AnalyzerFile::new("src/lib.rs".to_owned())?,
            domain::TextRange::new(domain::Position::new(1, 1)?, domain::Position::new(1, 2)?)?,
            domain::DiagnosticSeverity::Error,
            Some("E\u{1b}0\u{0}308".to_owned()),
            NonEmptyText::try_from("mismatched types".to_owned())?,
            Vec::new(),
        )?;
        let wire = wire_diagnostic(&diagnostic);
        assert_eq!(wire.code.as_deref(), Some("E\u{fffd}0\u{fffd}308"));
        Ok(())
    }

    /// V06 P3: `related[].message_truncated` mirrors the parent diagnostic's
    /// own flag rather than being silently discarded.
    #[test]
    fn related_information_message_truncated_is_not_discarded() -> TestResult {
        let file = AnalyzerFile::new("src/lib.rs".to_owned())?;
        let range =
            domain::TextRange::new(domain::Position::new(1, 1)?, domain::Position::new(1, 2)?)?;
        let short = wire_related_information(&domain::RelatedInformation {
            file: file.clone(),
            range,
            message: NonEmptyText::try_from("prior definition".to_owned())?,
        });
        assert!(!short.message_truncated);
        let long = wire_related_information(&domain::RelatedInformation {
            file,
            range,
            message: NonEmptyText::try_from("m".repeat(5_000))?,
        });
        assert!(long.message_truncated);
        assert_eq!(long.message.chars().count(), MAX_DIAGNOSTIC_MESSAGE_SCALARS);
        Ok(())
    }
} // mod new_tools
