//! Application port for the M6 analyzer tools, and the `rust.analyzer.symbols`
//! orchestration (ADR-083 §1-§4, §7; ADR-084 §2, §5, §8).
//!
//! No MCP protocol type, no untyped JSON value and no process API crosses
//! into this module: the adapter behind [`AnalyzerPort`] owns the guest
//! lifecycle, and the tool layer alone builds the wire envelope from
//! [`AnalyzerReport`].
use crate::{
    InspectionControl, InspectionError, ProjectError, ProjectIdentity, ProjectRegistry,
    ProjectSourceBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    AnalyzerExecution, AnalyzerFile, AnalyzerQuery, ExecutionLimits, InspectionSemantics,
    LineIndex, MAX_RESULT_BYTES, Position, ProjectIdentityFingerprint, ProjectRef, SourceBundle,
    SourceFingerprint, SymbolQuery,
};

/// What the port's session ran against, next to what it answered.
///
/// `source_fingerprint` is the same bundle digest the M2 writer already
/// computes for a `SourceBundle` (ADR-083 §3): the adapter behind this port
/// reuses that helper rather than a second hashing scheme. It is carried here,
/// not on [`AnalyzerExecution`] itself, because that domain type publishes
/// only what a live LSP session can know about itself; the digest of the bytes
/// it was handed is a fact the adapter's capture layer knows instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalyzerObservation {
    pub source_fingerprint: SourceFingerprint,
    pub execution: AnalyzerExecution,
}

/// One bounded rust-analyzer session over an already-captured snapshot
/// (ADR-084 §2). The adapter owns the guest lifecycle end to end; this port is
/// the only door into it from application, and it is called at most once per
/// request — a different question is a new call with a new capture.
pub trait AnalyzerPort {
    fn analyze(
        &self,
        source: &SourceBundle,
        query: &AnalyzerQuery,
        limits: ExecutionLimits,
        control: &dyn InspectionControl,
    ) -> Result<AnalyzerObservation, InspectionError>;
}

/// `rust.analyzer.symbols`'s two scopes (ADR-083 §2): a flattened
/// `DocumentSymbol` tree for one captured file, or a fuzzy workspace search
/// under `/source`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolsScope {
    Document { file: AnalyzerFile },
    Workspace { query: SymbolQuery },
}

/// A validated `rust.analyzer.symbols` request, already past wire parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolsRequest {
    /// `Some` when the caller wants a live-identity guarantee (ADR-083 §2): a
    /// mismatch is [`AnalyzerRequestError::Conflict`], never stale data.
    pub expected_project_fingerprint: Option<ProjectIdentityFingerprint>,
    pub scope: SymbolsScope,
    /// The caller's total-call budget (ADR-084 §8), already bounded to
    /// `1..=180` seconds by the tool's own wire schema.
    pub timeout_seconds: u32,
}

impl SymbolsRequest {
    /// The captured file this request names, if any. `Workspace` names none.
    fn file(&self) -> Option<&AnalyzerFile> {
        match &self.scope {
            SymbolsScope::Document { file } => Some(file),
            SymbolsScope::Workspace { .. } => None,
        }
    }

    fn into_query(self) -> AnalyzerQuery {
        match self.scope {
            SymbolsScope::Document { file } => AnalyzerQuery::DocumentSymbols { file },
            SymbolsScope::Workspace { query } => AnalyzerQuery::WorkspaceSymbols { query },
        }
    }
}

/// A validated `rust.analyzer.references` request, already past wire parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferencesRequest {
    pub expected_project_fingerprint: Option<ProjectIdentityFingerprint>,
    pub file: AnalyzerFile,
    pub position: Position,
    pub include_declaration: bool,
    /// The caller's total-call budget (ADR-084 §8), already bounded to
    /// `1..=180` seconds by the tool's own wire schema.
    pub timeout_seconds: u32,
}

/// A validated `rust.analyzer.diagnostics` request, already past wire parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticsRequest {
    pub expected_project_fingerprint: Option<ProjectIdentityFingerprint>,
    pub file: AnalyzerFile,
    /// The caller's total-call budget (ADR-084 §8), already bounded to
    /// `1..=180` seconds by the tool's own wire schema.
    pub timeout_seconds: u32,
}

/// Snapshot facts published next to every M6 analyzer answer (ADR-083 §3):
/// never `latest`, always the exact non-atomic capture the session ran
/// against.
#[derive(Clone, Debug)]
pub struct AnalyzerSnapshot {
    pub source_fingerprint: SourceFingerprint,
    pub files: u32,
    pub semantics: InspectionSemantics,
    pub atomic: bool,
}

/// The domain-only report `rust.analyzer.symbols` publishes. The tool layer
/// builds the wire envelope from this and from nothing else.
#[derive(Clone, Debug)]
pub struct AnalyzerReport {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub snapshot: AnalyzerSnapshot,
    pub execution: AnalyzerExecution,
}

/// Failure modes [`ProjectRegistry::analyzer_symbols`] adds on top of the
/// port's own [`InspectionError`].
///
/// Neither [`Self::Conflict`] nor [`Self::FileNotInSnapshot`] is a variant of
/// [`InspectionError`] itself: that enum is shared by every M1-M5 tool, its
/// defining file belongs to a different delegation, and it has no case for
/// "the caller's expected identity is stale" or "the caller named a file this
/// capture does not have" — both are M6-specific and decided before any
/// session opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyzerRequestError {
    Inspection(InspectionError),
    /// `expected_project_fingerprint` was supplied and did not match the
    /// project's live identity: never stale data (ADR-083 §2).
    Conflict,
    /// The queried `file` is absent from the capture. Refused before a
    /// session opens, so a caller's typo never spends a container — the same
    /// outcome the gateway's own defence in depth would reach, reached here
    /// without one.
    FileNotInSnapshot,
    /// The queried `position` is beyond the captured file's line or column
    /// count. Checked against the captured bytes before a session opens, for
    /// the same reason as [`Self::FileNotInSnapshot`]; left unchecked when the
    /// file is not valid UTF-8, which is the port's own
    /// `AnalyzerFailure::FileNotUtf8` to report instead.
    PositionOutOfRange,
}

impl From<InspectionError> for AnalyzerRequestError {
    fn from(value: InspectionError) -> Self {
        Self::Inspection(value)
    }
}

impl From<ProjectError> for AnalyzerRequestError {
    fn from(value: ProjectError) -> Self {
        Self::Inspection(value.into())
    }
}

/// The query and its budget, bundled so [`ProjectRegistry::analyzer_finish`]
/// takes one argument for both rather than two.
struct AnalyzerCall {
    query: AnalyzerQuery,
    timeout_seconds: u32,
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    /// Revalidates `reference`, captures its `SourceBundle` through the
    /// existing lease, checks the optional live-identity guarantee, and
    /// rejects a queried file this capture does not have. Shared by every M6
    /// analyzer tool ahead of its own, query-specific validation.
    fn analyzer_prelude(
        &mut self,
        reference: &ProjectRef,
        expected_project_fingerprint: Option<ProjectIdentityFingerprint>,
        file: Option<&AnalyzerFile>,
        control: &dyn InspectionControl,
    ) -> Result<(ProjectIdentity, SourceBundle), AnalyzerRequestError> {
        let identity = self.resolve_inner(reference, control, false)?;
        if let Some(expected) = expected_project_fingerprint
            && expected != identity.fingerprint
        {
            return Err(AnalyzerRequestError::Conflict);
        }
        let source = self.source_inner(reference, control, false)?;
        if let Some(file) = file {
            let present = source
                .files()
                .iter()
                .any(|candidate| candidate.path() == file.as_str());
            if !present {
                return Err(AnalyzerRequestError::FileNotInSnapshot);
            }
        }
        Ok((identity, source))
    }

    /// Calls `port` exactly once against the already-captured `source` and
    /// publishes a domain-only report. Never called before
    /// [`Self::analyzer_prelude`]'s checks, and any query-specific validation
    /// of its own, have passed.
    fn analyzer_finish(
        &mut self,
        reference: &ProjectRef,
        identity: ProjectIdentity,
        source: SourceBundle,
        call: AnalyzerCall,
        port: &impl AnalyzerPort,
        control: &dyn InspectionControl,
    ) -> Result<AnalyzerReport, AnalyzerRequestError> {
        let limits = ExecutionLimits::new_job(
            u64::from(call.timeout_seconds).saturating_mul(1_000),
            MAX_RESULT_BYTES,
        )
        .ok_or(AnalyzerRequestError::Inspection(InspectionError::Internal))?;
        let observation = port.analyze(&source, &call.query, limits, control)?;
        // No snapshot is published or lease renewed after a cancelled or
        // stale-identity revalidation; the same discipline `inspect` follows.
        self.resolve_inner(reference, control, true)?;
        let files = u32::try_from(source.files().len()).unwrap_or(u32::MAX);
        Ok(AnalyzerReport {
            project_ref: reference.clone(),
            project_identity_fingerprint: identity.fingerprint,
            snapshot: AnalyzerSnapshot {
                source_fingerprint: observation.source_fingerprint,
                files,
                semantics: InspectionSemantics::LatestKnown,
                atomic: false,
            },
            execution: observation.execution,
        })
    }

    /// Revalidates `reference`, captures its `SourceBundle` through the
    /// existing lease, checks the optional live-identity guarantee, rejects a
    /// queried file this capture does not have, calls `port` exactly once and
    /// publishes a domain-only report.
    pub fn analyzer_symbols(
        &mut self,
        reference: &ProjectRef,
        request: SymbolsRequest,
        port: &impl AnalyzerPort,
        control: &dyn InspectionControl,
    ) -> Result<AnalyzerReport, AnalyzerRequestError> {
        let (identity, source) = self.analyzer_prelude(
            reference,
            request.expected_project_fingerprint.clone(),
            request.file(),
            control,
        )?;
        let call = AnalyzerCall {
            timeout_seconds: request.timeout_seconds,
            query: request.into_query(),
        };
        self.analyzer_finish(reference, identity, source, call, port, control)
    }

    /// The M6-02 counterpart of [`Self::analyzer_symbols`]: `position` is
    /// validated against the captured bytes before any session opens (never
    /// checked when the file is not valid UTF-8, which the port's own
    /// `FileNotUtf8` reports instead), and the gateway sends a second,
    /// declaration-only request in the same session to mark `is_declaration`
    /// (ADR-084 §2 phase 6, amended) — this method itself is unaware of that
    /// second request.
    pub fn analyzer_references(
        &mut self,
        reference: &ProjectRef,
        request: ReferencesRequest,
        port: &impl AnalyzerPort,
        control: &dyn InspectionControl,
    ) -> Result<AnalyzerReport, AnalyzerRequestError> {
        let (identity, source) = self.analyzer_prelude(
            reference,
            request.expected_project_fingerprint,
            Some(&request.file),
            control,
        )?;
        let out_of_range = source
            .files()
            .iter()
            .find(|candidate| candidate.path() == request.file.as_str())
            .map(|candidate| candidate.bytes())
            .and_then(|bytes| LineIndex::new(bytes).ok())
            .is_some_and(|index| index.utf8_from_position(request.position).is_err());
        if out_of_range {
            return Err(AnalyzerRequestError::PositionOutOfRange);
        }
        let call = AnalyzerCall {
            timeout_seconds: request.timeout_seconds,
            query: AnalyzerQuery::References {
                file: request.file,
                position: request.position,
            },
        };
        self.analyzer_finish(reference, identity, source, call, port, control)
    }

    /// The M6-03 counterpart of [`Self::analyzer_symbols`].
    pub fn analyzer_diagnostics(
        &mut self,
        reference: &ProjectRef,
        request: DiagnosticsRequest,
        port: &impl AnalyzerPort,
        control: &dyn InspectionControl,
    ) -> Result<AnalyzerReport, AnalyzerRequestError> {
        let (identity, source) = self.analyzer_prelude(
            reference,
            request.expected_project_fingerprint,
            Some(&request.file),
            control,
        )?;
        let call = AnalyzerCall {
            timeout_seconds: request.timeout_seconds,
            query: AnalyzerQuery::Diagnostics { file: request.file },
        };
        self.analyzer_finish(reference, identity, source, call, port, control)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;
    use crate::benchmark::tests::{Backend, Control, TestClock, identity_fingerprint, registry};
    use rust_engineering_domain::{
        AnalyzerOutcome, AnalyzerReadiness, AnalyzerResult, AnalyzerRuntime, Completeness,
        ExecutionTermination, NonEmptyText, SessionStop, SessionSummary,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn fingerprint(value: u8) -> SourceFingerprint {
        format!("sha256:{value:064x}").parse().unwrap()
    }

    fn runtime() -> AnalyzerRuntime {
        AnalyzerRuntime {
            version: NonEmptyText::try_from("rust-analyzer 1.98.1 (48a229c 2026-09-01)".to_owned())
                .unwrap(),
            binary_sha256: fingerprint(1),
            image_id: NonEmptyText::try_from("sha256:m6".to_owned()).unwrap(),
            config_digest: fingerprint(2),
        }
    }

    fn absent_session() -> SessionSummary {
        SessionSummary {
            stop: SessionStop::Exited,
            exit_code: Some(0),
            messages_in: 5,
            messages_out: 6,
            bytes_in: 3_804,
            bytes_out: 1_934,
            stderr_bytes: 0,
            stderr_sha256: fingerprint(3),
            stderr_truncated: false,
            server_requests_refused: 0,
            notifications_dropped: 0,
            late_responses: 0,
            status_transcript: Vec::new(),
            status_notifications: 1,
            fault: None,
            declared_frame_bytes: None,
            kill_error: None,
            reap_error: None,
            duration_ms: 400,
        }
    }

    fn answered() -> AnalyzerExecution {
        AnalyzerExecution {
            identity: runtime(),
            position_encoding: Some(rust_engineering_domain::PositionEncoding::Utf8),
            readiness: AnalyzerReadiness::Quiescent {
                elapsed_ms: 362,
                health: rust_engineering_domain::ServerHealth::Ok,
            },
            outcome: AnalyzerOutcome::Answered(AnalyzerResult::DocumentSymbols(Vec::new())),
            completeness: Completeness::complete(),
            session: absent_session(),
            termination: ExecutionTermination::Exited,
            oom_killed: Some(false),
            call_duration_ms: 1_039,
        }
    }

    fn document_request() -> SymbolsRequest {
        SymbolsRequest {
            expected_project_fingerprint: None,
            scope: SymbolsScope::Document {
                file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
            },
            timeout_seconds: 60,
        }
    }

    /// A fake port that hands back a fixed observation, or a named error, and
    /// counts how many times it was actually called.
    #[derive(Default)]
    struct FakePort {
        calls: AtomicUsize,
        result: std::sync::Mutex<Option<Result<AnalyzerObservation, InspectionError>>>,
        last_query: std::sync::Mutex<Option<AnalyzerQuery>>,
    }
    impl FakePort {
        fn answering(execution: AnalyzerExecution) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                result: std::sync::Mutex::new(Some(Ok(AnalyzerObservation {
                    source_fingerprint: fingerprint(9),
                    execution,
                }))),
                last_query: std::sync::Mutex::new(None),
            }
        }
        fn failing(error: InspectionError) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                result: std::sync::Mutex::new(Some(Err(error))),
                last_query: std::sync::Mutex::new(None),
            }
        }
    }
    impl AnalyzerPort for FakePort {
        fn analyze(
            &self,
            _source: &SourceBundle,
            query: &AnalyzerQuery,
            _limits: ExecutionLimits,
            _control: &dyn InspectionControl,
        ) -> Result<AnalyzerObservation, InspectionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.last_query.lock().unwrap() = Some(query.clone());
            self.result.lock().unwrap().take().expect("called once")
        }
    }

    #[test]
    fn happy_path_calls_the_port_once_and_publishes_the_snapshot() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let report = registry
            .analyzer_symbols(
                &opened.project_ref,
                document_request(),
                &port,
                &Control::default(),
            )
            .expect("happy path");
        assert_eq!(report.project_ref, opened.project_ref);
        assert_eq!(report.project_identity_fingerprint, identity_fingerprint(1));
        assert_eq!(report.snapshot.source_fingerprint, fingerprint(9));
        assert_eq!(report.snapshot.files, 2);
        assert!(matches!(
            report.snapshot.semantics,
            InspectionSemantics::LatestKnown
        ));
        assert!(!report.snapshot.atomic);
        assert!(matches!(
            report.execution.outcome,
            AnalyzerOutcome::Answered(_)
        ));
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_stale_expected_fingerprint_is_a_conflict_before_any_capture() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let request = SymbolsRequest {
            expected_project_fingerprint: Some(identity_fingerprint(99)),
            scope: SymbolsScope::Document {
                file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
            },
            timeout_seconds: 60,
        };
        let error = registry
            .analyzer_symbols(&opened.project_ref, request, &port, &Control::default())
            .expect_err("conflict");
        assert_eq!(error, AnalyzerRequestError::Conflict);
        assert_eq!(
            port.calls.load(Ordering::SeqCst),
            0,
            "a stale expected fingerprint never reaches the port"
        );
    }

    #[test]
    fn a_matching_expected_fingerprint_still_answers() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let request = SymbolsRequest {
            expected_project_fingerprint: Some(identity_fingerprint(1)),
            scope: SymbolsScope::Document {
                file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
            },
            timeout_seconds: 60,
        };
        assert!(
            registry
                .analyzer_symbols(&opened.project_ref, request, &port, &Control::default())
                .is_ok()
        );
    }

    #[test]
    fn a_file_outside_the_capture_never_reaches_the_port() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let request = SymbolsRequest {
            expected_project_fingerprint: None,
            scope: SymbolsScope::Document {
                file: AnalyzerFile::new("src/missing.rs".into()).unwrap(),
            },
            timeout_seconds: 60,
        };
        let error = registry
            .analyzer_symbols(&opened.project_ref, request, &port, &Control::default())
            .expect_err("file not in snapshot");
        assert_eq!(error, AnalyzerRequestError::FileNotInSnapshot);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn workspace_scope_names_no_file_and_is_never_rejected_for_one() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let request = SymbolsRequest {
            expected_project_fingerprint: None,
            scope: SymbolsScope::Workspace {
                query: SymbolQuery::new("answer".into()).unwrap(),
            },
            timeout_seconds: 60,
        };
        assert!(
            registry
                .analyzer_symbols(&opened.project_ref, request, &port, &Control::default())
                .is_ok()
        );
    }

    #[test]
    fn a_port_error_propagates_through_the_request_error() {
        let port = FakePort::failing(InspectionError::Internal);
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let error = registry
            .analyzer_symbols(
                &opened.project_ref,
                document_request(),
                &port,
                &Control::default(),
            )
            .expect_err("port error propagates");
        assert_eq!(
            error,
            AnalyzerRequestError::Inspection(InspectionError::Internal)
        );
    }

    #[test]
    fn cancellation_is_observed_before_the_port_is_called() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let control = Control::default();
        control.cancel();
        let error = registry
            .analyzer_symbols(&opened.project_ref, document_request(), &port, &control)
            .expect_err("cancellation is observed");
        assert_eq!(
            error,
            AnalyzerRequestError::Inspection(InspectionError::Project(ProjectError::Cancelled))
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    // ---- analyzer_references ----

    fn references_request() -> ReferencesRequest {
        ReferencesRequest {
            expected_project_fingerprint: None,
            file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
            position: Position::new(1, 8).unwrap(),
            include_declaration: true,
            timeout_seconds: 60,
        }
    }

    #[test]
    fn references_happy_path_calls_the_port_once_and_publishes_the_snapshot() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let report = registry
            .analyzer_references(
                &opened.project_ref,
                references_request(),
                &port,
                &Control::default(),
            )
            .expect("happy path");
        assert_eq!(report.project_ref, opened.project_ref);
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }

    /// The gateway always fetches both the `includeDeclaration: true` and
    /// `false` answers in the same session and marks `is_declaration` from
    /// their difference (ADR-084 §2 phase 6, amended), so
    /// `AnalyzerQuery::References` carries no `include_declaration` field
    /// (V06 P3): the query this port receives is identical either way, and
    /// the caller's choice is honoured later, by the tool layer removing
    /// declarations and counting them (`omitted_declarations`), never here.
    #[test]
    fn references_include_declaration_no_longer_reaches_the_query_the_port_receives() {
        for include_declaration in [true, false] {
            let port = FakePort::answering(answered());
            let mut registry = registry(Backend::default(), TestClock::at(100));
            let opened = registry
                .open("/trusted/project", &Control::default())
                .unwrap();
            let mut request = references_request();
            request.include_declaration = include_declaration;
            registry
                .analyzer_references(&opened.project_ref, request, &port, &Control::default())
                .expect("happy path");
            let sent = port
                .last_query
                .lock()
                .unwrap()
                .clone()
                .expect("port was called");
            assert_eq!(
                sent,
                AnalyzerQuery::References {
                    file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
                    position: Position::new(1, 8).unwrap(),
                }
            );
        }
    }

    #[test]
    fn a_position_beyond_the_captured_line_never_reaches_the_port() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let mut request = references_request();
        request.position = Position::new(99, 1).unwrap();
        let error = registry
            .analyzer_references(&opened.project_ref, request, &port, &Control::default())
            .expect_err("position out of range");
        assert_eq!(error, AnalyzerRequestError::PositionOutOfRange);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_column_beyond_the_captured_line_never_reaches_the_port() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let mut request = references_request();
        // "pub fn answer() -> u8 { 42 }" is 29 scalars long: column 31 is one
        // past the last valid position on that line.
        request.position = Position::new(1, 31).unwrap();
        let error = registry
            .analyzer_references(&opened.project_ref, request, &port, &Control::default())
            .expect_err("position out of range");
        assert_eq!(error, AnalyzerRequestError::PositionOutOfRange);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn references_to_a_file_outside_the_capture_never_reach_the_port() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let mut request = references_request();
        request.file = AnalyzerFile::new("src/missing.rs".into()).unwrap();
        let error = registry
            .analyzer_references(&opened.project_ref, request, &port, &Control::default())
            .expect_err("file not in snapshot");
        assert_eq!(error, AnalyzerRequestError::FileNotInSnapshot);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_stale_expected_fingerprint_is_a_conflict_before_any_reference_capture() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let mut request = references_request();
        request.expected_project_fingerprint = Some(identity_fingerprint(99));
        let error = registry
            .analyzer_references(&opened.project_ref, request, &port, &Control::default())
            .expect_err("conflict");
        assert_eq!(error, AnalyzerRequestError::Conflict);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    // ---- analyzer_diagnostics ----

    fn diagnostics_request() -> DiagnosticsRequest {
        DiagnosticsRequest {
            expected_project_fingerprint: None,
            file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
            timeout_seconds: 60,
        }
    }

    #[test]
    fn diagnostics_happy_path_calls_the_port_once_and_publishes_the_snapshot() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let report = registry
            .analyzer_diagnostics(
                &opened.project_ref,
                diagnostics_request(),
                &port,
                &Control::default(),
            )
            .expect("happy path");
        assert_eq!(report.project_ref, opened.project_ref);
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            port.last_query.lock().unwrap().clone(),
            Some(AnalyzerQuery::Diagnostics {
                file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
            })
        );
    }

    #[test]
    fn diagnostics_for_a_file_outside_the_capture_never_reach_the_port() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let mut request = diagnostics_request();
        request.file = AnalyzerFile::new("src/missing.rs".into()).unwrap();
        let error = registry
            .analyzer_diagnostics(&opened.project_ref, request, &port, &Control::default())
            .expect_err("file not in snapshot");
        assert_eq!(error, AnalyzerRequestError::FileNotInSnapshot);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_stale_expected_fingerprint_is_a_conflict_before_any_diagnostics_capture() {
        let port = FakePort::answering(answered());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let mut request = diagnostics_request();
        request.expected_project_fingerprint = Some(identity_fingerprint(99));
        let error = registry
            .analyzer_diagnostics(&opened.project_ref, request, &port, &Control::default())
            .expect_err("conflict");
        assert_eq!(error, AnalyzerRequestError::Conflict);
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }
}
