//! Application port for the M6 analyzer tools, and the `rust.analyzer.symbols`
//! orchestration (ADR-083 §1-§4, §7; ADR-084 §2, §5, §8).
//!
//! No MCP protocol type, no untyped JSON value and no process API crosses
//! into this module: the adapter behind [`AnalyzerPort`] owns the guest
//! lifecycle, and the tool layer alone builds the wire envelope from
//! [`AnalyzerReport`].
use crate::{
    InspectionControl, InspectionError, MutationPublisher, ProjectError, ProjectIdentity,
    ProjectRegistry, ProjectSourceBackend, ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{
    ActionCandidate, ActionRejection, AnalyzerAction, AnalyzerError, AnalyzerExecution,
    AnalyzerFile, AnalyzerQuery, AnalyzerResult, CodeActionKind, EditsSummary,
    ExecutionFingerprint, ExecutionLimits, InspectionSemantics, LineIndex, MAX_EDITS,
    MAX_RESULT_BYTES, MutationCandidate, MutationError, MutationKind, NonEmptyText, Position,
    ProjectIdentityFingerprint, ProjectRef, SourceBundle, SourceFingerprint, SymbolQuery,
    TextRange, apply_action_to_bundle, validate_action_edits,
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

    /// One `textDocument/codeAction` session over `range` of `file`, with the
    /// `action_digest` of every applicable action (ADR-083 §2, §4). An empty
    /// `only` means no kind filter.
    fn resolve_actions(
        &self,
        source: &SourceBundle,
        file: &AnalyzerFile,
        range: TextRange,
        only: &[CodeActionKind],
        limits: ExecutionLimits,
        control: &dyn InspectionControl,
    ) -> Result<ActionsObservation, InspectionError>;

    /// One unfiltered `textDocument/codeAction` session over `range` of
    /// `file`, resolving the applicable action whose digest is
    /// `action_digest` (ADR-083 §6). The adapter never applies the edits.
    fn resolve_action_candidate(
        &self,
        source: &SourceBundle,
        file: &AnalyzerFile,
        range: TextRange,
        action_digest: &SourceFingerprint,
        limits: ExecutionLimits,
        control: &dyn InspectionControl,
    ) -> Result<ActionApplyObservation, InspectionError>;
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
        let limits = call_limits(call.timeout_seconds)?;
        let observation = port.analyze(&source, &call.query, limits, control)?;
        self.analyzer_report(reference, identity, &source, observation, control)
    }

    /// Revalidates `reference` after the session and publishes the domain-only
    /// report of `observation` over `source`.
    fn analyzer_report(
        &mut self,
        reference: &ProjectRef,
        identity: ProjectIdentity,
        source: &SourceBundle,
        observation: AnalyzerObservation,
        control: &dyn InspectionControl,
    ) -> Result<AnalyzerReport, AnalyzerRequestError> {
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

// ---- M6-04: code actions and the analyzer action candidate ----

fn call_limits(timeout_seconds: u32) -> Result<ExecutionLimits, AnalyzerRequestError> {
    ExecutionLimits::new_job(
        u64::from(timeout_seconds).saturating_mul(1_000),
        MAX_RESULT_BYTES,
    )
    .ok_or(AnalyzerRequestError::Inspection(InspectionError::Internal))
}

/// The `validation` version of an analyzer action candidate, sibling of
/// `m2-fmt-apply-v1`.
pub const ANALYZER_ACTION_VALIDATION_VERSION: &str = "m6-analyzer-action-v1";

/// A `CodeActions` observation plus every applicable action's identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionsObservation {
    pub observation: AnalyzerObservation,
    /// Index-aligned with the `CodeActions` answer in
    /// `observation.execution`: `Some(action_digest)` exactly for an
    /// `Applicable` candidate. Empty when the call produced no `CodeActions`
    /// answer.
    pub action_digests: Vec<Option<SourceFingerprint>>,
}

/// The runtime an action candidate was resolved in, in the vocabulary of the
/// M2 validation provenance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalyzerActionRuntime {
    pub platform: String,
    pub image_id: String,
    pub configuration_fingerprint: ExecutionFingerprint,
    /// The identity of the one rust-analyzer session request: gateway
    /// configuration, query, limits, analyzed snapshot and analyzer runtime.
    pub session_fingerprint: ExecutionFingerprint,
    pub rust_version: String,
    pub cargo_version: String,
}

/// What an apply-preview session found for the requested digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionResolution {
    /// The session produced no `CodeActions` answer; the execution's failure
    /// names why.
    NotAnswered,
    /// No applicable action of this fresh session carries the digest: the
    /// action the caller saw is gone, or the code, runtime or configuration
    /// under it changed (ADR-083 §2 `ACTION_STALE`).
    Stale,
    /// The matched action fails structural validation.
    Rejected(ActionRejection),
    Resolved(AnalyzerAction),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionApplyObservation {
    pub observation: AnalyzerObservation,
    pub runtime: AnalyzerActionRuntime,
    pub resolution: ActionResolution,
}

/// A validated `rust.analyzer.actions` request, already past wire parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionsRequest {
    /// Mandatory for this tool (ADR-083 §2).
    pub expected_project_fingerprint: ProjectIdentityFingerprint,
    pub file: AnalyzerFile,
    pub range: TextRange,
    pub only: Vec<CodeActionKind>,
    pub timeout_seconds: u32,
}

/// A validated `rust.analyzer.action.apply` preview, already past wire parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionPreviewRequest {
    pub expected_project_fingerprint: ProjectIdentityFingerprint,
    pub file: AnalyzerFile,
    pub range: TextRange,
    pub action_digest: SourceFingerprint,
    pub timeout_seconds: u32,
}

/// One element of a `rust.analyzer.actions` answer (ADR-083 §4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionSummary {
    Applicable {
        action_digest: SourceFingerprint,
        title: NonEmptyText,
        kind: Option<CodeActionKind>,
        is_preferred: bool,
        edits_summary: EditsSummary,
    },
    /// Refused by the codec, or by the same structural rules an apply preview
    /// enforces. `title` and `kind` are the peer's own values when known.
    Rejected {
        reason: ActionRejection,
        title: Option<NonEmptyText>,
        kind: Option<CodeActionKind>,
    },
}

#[derive(Clone, Debug)]
pub struct ActionsReport {
    pub report: AnalyzerReport,
    /// Empty when the session produced no `CodeActions` answer.
    pub actions: Vec<ActionSummary>,
}

/// An M2 candidate built from one resolved code action, ready for
/// `MutationPlans` and the single M2 writer.
#[derive(Clone, Debug)]
pub struct AnalyzerActionCandidate {
    pub workspace_root: String,
    pub candidate: MutationCandidate,
    pub action: AnalyzerAction,
    pub action_digest: SourceFingerprint,
    pub report: AnalyzerReport,
}

/// Why [`ProjectRegistry::analyzer_action_candidate`] produced no candidate.
/// Every variant decided after the session carries its report, so the tool
/// can still publish the analyzer identity it ran against.
#[derive(Clone, Debug)]
pub enum ActionCandidateError {
    Request(AnalyzerRequestError),
    Mutation(MutationError),
    NotAnswered(Box<AnalyzerReport>),
    Stale(Box<AnalyzerReport>),
    Rejected {
        reason: ActionRejection,
        report: Box<AnalyzerReport>,
    },
    /// The matched action's edits do not apply to the fresh capture.
    EditsNotApplicable {
        error: AnalyzerError,
        report: Box<AnalyzerReport>,
    },
    /// The action carries no edit, or its edits leave every byte unchanged.
    NoChange(Box<AnalyzerReport>),
}

impl From<AnalyzerRequestError> for ActionCandidateError {
    fn from(value: AnalyzerRequestError) -> Self {
        Self::Request(value)
    }
}

impl From<InspectionError> for ActionCandidateError {
    fn from(value: InspectionError) -> Self {
        Self::Request(value.into())
    }
}

impl From<ProjectError> for ActionCandidateError {
    fn from(value: ProjectError) -> Self {
        Self::Request(value.into())
    }
}

impl From<MutationError> for ActionCandidateError {
    fn from(value: MutationError) -> Self {
        Self::Mutation(value)
    }
}

fn captures_file(source: &SourceBundle, file: &AnalyzerFile) -> bool {
    source
        .files()
        .iter()
        .any(|candidate| candidate.path() == file.as_str())
}

/// Whether either end of `range` misses the captured bytes of `file`. Never
/// true for a file that is not valid UTF-8, which the port reports instead.
fn range_outside(source: &SourceBundle, file: &AnalyzerFile, range: TextRange) -> bool {
    source
        .files()
        .iter()
        .find(|candidate| candidate.path() == file.as_str())
        .and_then(|candidate| LineIndex::new(candidate.bytes()).ok())
        .is_some_and(|index| {
            index.utf8_from_position(range.start()).is_err()
                || index.utf8_from_position(range.end()).is_err()
        })
}

fn summarize_actions(
    source: &SourceBundle,
    observed: &ActionsObservation,
) -> Result<Vec<ActionSummary>, AnalyzerRequestError> {
    let misaligned = AnalyzerRequestError::Inspection(InspectionError::Internal);
    let Some(AnalyzerResult::CodeActions(candidates)) = observed.observation.execution.result()
    else {
        return if observed.action_digests.is_empty() {
            Ok(Vec::new())
        } else {
            Err(misaligned)
        };
    };
    if candidates.len() != observed.action_digests.len() {
        return Err(misaligned);
    }
    candidates
        .iter()
        .zip(&observed.action_digests)
        .map(|pair| match pair {
            (ActionCandidate::Applicable(action), Some(action_digest)) => {
                Ok(match listed_edits(source, action) {
                    Ok(edits_summary) => ActionSummary::Applicable {
                        action_digest: action_digest.clone(),
                        title: action.title.clone(),
                        kind: action.kind,
                        is_preferred: action.is_preferred,
                        edits_summary,
                    },
                    // Never offered as applicable when the preview over these
                    // very bytes would refuse it (V07).
                    Err(reason) => ActionSummary::Rejected {
                        reason,
                        title: Some(action.title.clone()),
                        kind: action.kind,
                    },
                })
            }
            (ActionCandidate::Rejected(rejected), None) => Ok(ActionSummary::Rejected {
                reason: rejected.reason,
                title: rejected.title.clone(),
                kind: rejected.kind,
            }),
            _ => Err(misaligned),
        })
        .collect()
}

/// The structural rules an apply preview enforces on an action's edits
/// (ADR-083 §5): the gateway's own edit and byte ceilings, then the domain's
/// per-file rules, mapped onto the closed rejection vocabulary.
///
/// Also mirrors the three checks `analyzer_action_candidate` runs only after
/// building the edited bundle, so nothing is listed `applicable` that a
/// preview would then refuse (V07): edits that change no captured bytes, an
/// edited path outside `.rs` and a bundle-level limit `validate_action_edits`
/// does not itself enforce. None of these three has its own wire reason, so
/// they share `UnresolvedEdit`, the closed vocabulary's catch-all.
fn listed_edits(
    source: &SourceBundle,
    action: &AnalyzerAction,
) -> Result<EditsSummary, ActionRejection> {
    if action.edits.len() > MAX_EDITS {
        return Err(ActionRejection::EditLimit);
    }
    let inserted = action.edits.iter().fold(0usize, |total, edit| {
        total.saturating_add(edit.new_text.len())
    });
    if inserted > MAX_RESULT_BYTES {
        return Err(ActionRejection::BytesLimit);
    }
    let map_rejection = |error| match error {
        AnalyzerError::OverlappingRanges => ActionRejection::OverlappingRanges,
        AnalyzerError::FileNotInSnapshot => ActionRejection::FileNotInSnapshot,
        AnalyzerError::NotUtf8 => ActionRejection::NotUtf8,
        AnalyzerError::LimitExceeded => ActionRejection::BytesLimit,
        _ => ActionRejection::UnresolvedEdit,
    };
    let summary = validate_action_edits(source, &action.edits).map_err(map_rejection)?;
    let after = apply_action_to_bundle(source, &action.edits).map_err(map_rejection)?;
    let mut changed = 0usize;
    for (before, edited) in source.files().iter().zip(after.files()) {
        if before.bytes() != edited.bytes() {
            if !before.path().ends_with(".rs") {
                return Err(ActionRejection::UnresolvedEdit);
            }
            changed += 1;
        }
    }
    if changed == 0 || changed > MAX_EDITS {
        return Err(ActionRejection::UnresolvedEdit);
    }
    Ok(summary)
}

/// The `m6-analyzer-action-v1` validation provenance of one candidate, in the
/// exact frame order `rust.analyzer.action.apply` decodes (ADR-083 §6).
///
/// The nine leading fields are `m2-fmt-apply-v1`'s, in its order. The ninth
/// is the snapshot the action was resolved against and applied to: no run
/// validated the result, so there is no candidate fingerprint to publish. The
/// tail binds the analyzer runtime and the action itself.
#[derive(Clone, Copy, Debug)]
pub struct AnalyzerActionProvenance<'a> {
    pub platform: &'a str,
    pub image_id: &'a str,
    pub configuration_fingerprint: &'a str,
    pub session_fingerprint: &'a str,
    pub rust_version: &'a str,
    pub cargo_version: &'a str,
    pub analyzed_source_fingerprint: &'a str,
    pub analyzer_version: &'a str,
    pub binary_sha256: &'a str,
    pub config_digest: &'a str,
    pub action_digest: &'a str,
}

impl AnalyzerActionProvenance<'_> {
    /// `len:bytes` frames, opened by [`ANALYZER_ACTION_VALIDATION_VERSION`]
    /// and `local_coordinated`, then every field in declaration order.
    pub fn encode(&self) -> Result<String, MutationError> {
        frame_validation(&[
            ANALYZER_ACTION_VALIDATION_VERSION,
            "local_coordinated",
            self.platform,
            self.image_id,
            self.configuration_fingerprint,
            self.session_fingerprint,
            self.rust_version,
            self.cargo_version,
            self.analyzed_source_fingerprint,
            self.analyzer_version,
            self.binary_sha256,
            self.config_digest,
            self.action_digest,
        ])
    }
}

fn frame_validation(fields: &[&str]) -> Result<String, MutationError> {
    let mut validation = String::new();
    for field in fields {
        use std::fmt::Write;
        write!(validation, "{}:{field}", field.len()).map_err(|_| MutationError::Invalid)?;
    }
    Ok(validation)
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    /// `rust.analyzer.actions`: the M6-04 counterpart of
    /// [`Self::analyzer_references`], publishing every action with its
    /// digest, applicability and edits summary.
    pub fn analyzer_actions(
        &mut self,
        reference: &ProjectRef,
        request: ActionsRequest,
        port: &impl AnalyzerPort,
        control: &dyn InspectionControl,
    ) -> Result<ActionsReport, AnalyzerRequestError> {
        let (identity, source) = self.analyzer_prelude(
            reference,
            Some(request.expected_project_fingerprint),
            Some(&request.file),
            control,
        )?;
        if range_outside(&source, &request.file, request.range) {
            return Err(AnalyzerRequestError::PositionOutOfRange);
        }
        let limits = call_limits(request.timeout_seconds)?;
        let observed = port.resolve_actions(
            &source,
            &request.file,
            request.range,
            &request.only,
            limits,
            control,
        )?;
        let actions = summarize_actions(&source, &observed)?;
        let report =
            self.analyzer_report(reference, identity, &source, observed.observation, control)?;
        Ok(ActionsReport { report, actions })
    }

    /// The candidate half of `rust.analyzer.action.apply` preview
    /// (ADR-083 §6).
    ///
    /// Revalidates the project, requires the live identity, authorizes the
    /// host grant, captures a **fresh** bundle, resolves the action by digest
    /// in a new session, applies its edits to that capture and builds the
    /// candidate exactly as `rust.fmt.apply` does: `before` is the complete
    /// fresh capture (so `finish_manifest_preview` and the writer's
    /// same-shape check hold unchanged) and `after` differs from it only in
    /// the edited `.rs` files. Nothing compiles the result: the provenance is
    /// structural `WorkspaceEdit` validation only.
    pub fn analyzer_action_candidate(
        &mut self,
        reference: &ProjectRef,
        request: ActionPreviewRequest,
        port: &impl AnalyzerPort,
        publisher: &impl MutationPublisher<B::Lease>,
        control: &dyn InspectionControl,
    ) -> Result<AnalyzerActionCandidate, ActionCandidateError> {
        let identity = self.resolve_inner(reference, control, false)?;
        if identity.fingerprint != request.expected_project_fingerprint {
            return Err(AnalyzerRequestError::Conflict.into());
        }
        let entry = self.entries.get(reference).ok_or(MutationError::NotFound)?;
        publisher.authorize(&entry.project.lease)?;
        let source = self.source_inner(reference, control, false)?;
        if !captures_file(&source, &request.file) {
            return Err(AnalyzerRequestError::FileNotInSnapshot.into());
        }
        if range_outside(&source, &request.file, request.range) {
            return Err(AnalyzerRequestError::PositionOutOfRange.into());
        }
        let limits = call_limits(request.timeout_seconds)?;
        let ActionApplyObservation {
            observation,
            runtime,
            resolution,
        } = port.resolve_action_candidate(
            &source,
            &request.file,
            request.range,
            &request.action_digest,
            limits,
            control,
        )?;
        let analyzed = observation.source_fingerprint.clone();
        let analyzer = observation.execution.identity.clone();
        let workspace_root = identity.workspace_root.clone();
        let report = self.analyzer_report(reference, identity, &source, observation, control)?;
        let action = match resolution {
            ActionResolution::Resolved(action) => action,
            ActionResolution::NotAnswered => {
                return Err(ActionCandidateError::NotAnswered(Box::new(report)));
            }
            ActionResolution::Stale => return Err(ActionCandidateError::Stale(Box::new(report))),
            ActionResolution::Rejected(reason) => {
                return Err(ActionCandidateError::Rejected {
                    reason,
                    report: Box::new(report),
                });
            }
        };
        // Both identities come from the same admitted image; a disagreement is
        // an adapter fault, never a candidate.
        if runtime.image_id != analyzer.image_id.as_str() {
            return Err(AnalyzerRequestError::Inspection(InspectionError::Internal).into());
        }
        let after = match apply_action_to_bundle(&source, &action.edits) {
            Ok(after) => after,
            Err(AnalyzerError::NoEdits) => {
                return Err(ActionCandidateError::NoChange(Box::new(report)));
            }
            Err(error) => {
                return Err(ActionCandidateError::EditsNotApplicable {
                    error,
                    report: Box::new(report),
                });
            }
        };
        control.check()?;
        // The application independently enforces the writer's closed scope
        // for this kind; the native publisher repeats it at persistence.
        let mut changed = 0usize;
        for (before, edited) in source.files().iter().zip(after.files()) {
            if before.path() != edited.path() {
                return Err(MutationError::Invalid.into());
            }
            if before.bytes() != edited.bytes() {
                if !before.path().ends_with(".rs") {
                    return Err(MutationError::PermissionDenied.into());
                }
                changed += 1;
            }
        }
        if changed == 0 {
            return Err(ActionCandidateError::NoChange(Box::new(report)));
        }
        if changed > 128 {
            return Err(MutationError::LimitExceeded.into());
        }
        let configuration = runtime.configuration_fingerprint.to_string();
        let session = runtime.session_fingerprint.to_string();
        let analyzed = analyzed.to_string();
        let binary = analyzer.binary_sha256.to_string();
        let config_digest = analyzer.config_digest.to_string();
        let action_digest = request.action_digest.to_string();
        let validation = AnalyzerActionProvenance {
            platform: &runtime.platform,
            image_id: &runtime.image_id,
            configuration_fingerprint: &configuration,
            session_fingerprint: &session,
            rust_version: &runtime.rust_version,
            cargo_version: &runtime.cargo_version,
            analyzed_source_fingerprint: &analyzed,
            analyzer_version: analyzer.version.as_str(),
            binary_sha256: &binary,
            config_digest: &config_digest,
            action_digest: &action_digest,
        }
        .encode()?;
        Ok(AnalyzerActionCandidate {
            workspace_root,
            candidate: MutationCandidate {
                kind: MutationKind::AnalyzerActionApply,
                before: source,
                after,
                validation,
            },
            action,
            action_digest: request.action_digest,
            report,
        })
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
        fn resolve_actions(
            &self,
            _source: &SourceBundle,
            _file: &AnalyzerFile,
            _range: TextRange,
            _only: &[CodeActionKind],
            _limits: ExecutionLimits,
            _control: &dyn InspectionControl,
        ) -> Result<ActionsObservation, InspectionError> {
            Err(InspectionError::Internal)
        }
        fn resolve_action_candidate(
            &self,
            _source: &SourceBundle,
            _file: &AnalyzerFile,
            _range: TextRange,
            _action_digest: &SourceFingerprint,
            _limits: ExecutionLimits,
            _control: &dyn InspectionControl,
        ) -> Result<ActionApplyObservation, InspectionError> {
            Err(InspectionError::Internal)
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

    // ---- analyzer_actions / analyzer_action_candidate ----

    use crate::MutationPublisher;
    use crate::benchmark::tests::execution_fingerprint;
    use rust_engineering_domain::{
        IdempotencyKey, MutationCommit, MutationId, MutationReceipt, RejectedAction, TextEdit,
    };

    fn text_range(sl: u32, sc: u32, el: u32, ec: u32) -> TextRange {
        TextRange::new(
            Position::new(sl, sc).unwrap(),
            Position::new(el, ec).unwrap(),
        )
        .unwrap()
    }

    fn lib_rs() -> AnalyzerFile {
        AnalyzerFile::new("src/lib.rs".into()).unwrap()
    }

    /// `pub fn answer() -> u8 { 42 }`: `42` spans columns 25..27.
    fn answer_edit(file: &str, range: TextRange, new_text: &str) -> TextEdit {
        TextEdit {
            file: AnalyzerFile::new(file.into()).unwrap(),
            range,
            new_text: new_text.into(),
        }
    }

    fn action_with(edits: Vec<TextEdit>) -> AnalyzerAction {
        AnalyzerAction {
            title: NonEmptyText::try_from("Replace the answer".to_owned()).unwrap(),
            kind: Some(CodeActionKind::QuickFix),
            is_preferred: true,
            edits,
        }
    }

    fn answer_action(new_text: &str) -> AnalyzerAction {
        action_with(vec![answer_edit(
            "src/lib.rs",
            text_range(1, 25, 1, 27),
            new_text,
        )])
    }

    fn answered_actions(candidates: Vec<ActionCandidate>) -> AnalyzerExecution {
        let mut execution = answered();
        execution.outcome = AnalyzerOutcome::Answered(AnalyzerResult::CodeActions(candidates));
        execution
    }

    fn action_runtime() -> AnalyzerActionRuntime {
        AnalyzerActionRuntime {
            platform: "linux/aarch64".into(),
            image_id: "sha256:m6".into(),
            configuration_fingerprint: execution_fingerprint(4),
            session_fingerprint: execution_fingerprint(5),
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
        }
    }

    #[derive(Default)]
    struct ActionPort {
        calls: AtomicUsize,
        actions: std::sync::Mutex<Option<Result<ActionsObservation, InspectionError>>>,
        candidate: std::sync::Mutex<Option<Result<ActionApplyObservation, InspectionError>>>,
        requested: std::sync::Mutex<Option<SourceFingerprint>>,
    }
    impl ActionPort {
        fn listing(
            candidates: Vec<ActionCandidate>,
            digests: Vec<Option<SourceFingerprint>>,
        ) -> Self {
            let port = Self::default();
            *port.actions.lock().unwrap() = Some(Ok(ActionsObservation {
                observation: AnalyzerObservation {
                    source_fingerprint: fingerprint(9),
                    execution: answered_actions(candidates),
                },
                action_digests: digests,
            }));
            port
        }
        fn resolving(resolution: ActionResolution) -> Self {
            let port = Self::default();
            *port.candidate.lock().unwrap() = Some(Ok(ActionApplyObservation {
                observation: AnalyzerObservation {
                    source_fingerprint: fingerprint(9),
                    execution: answered_actions(Vec::new()),
                },
                runtime: action_runtime(),
                resolution,
            }));
            port
        }
    }
    impl AnalyzerPort for ActionPort {
        fn analyze(
            &self,
            _source: &SourceBundle,
            _query: &AnalyzerQuery,
            _limits: ExecutionLimits,
            _control: &dyn InspectionControl,
        ) -> Result<AnalyzerObservation, InspectionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(InspectionError::Internal)
        }
        fn resolve_actions(
            &self,
            _source: &SourceBundle,
            _file: &AnalyzerFile,
            _range: TextRange,
            _only: &[CodeActionKind],
            _limits: ExecutionLimits,
            _control: &dyn InspectionControl,
        ) -> Result<ActionsObservation, InspectionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.actions.lock().unwrap().take().expect("called once")
        }
        fn resolve_action_candidate(
            &self,
            _source: &SourceBundle,
            _file: &AnalyzerFile,
            _range: TextRange,
            action_digest: &SourceFingerprint,
            _limits: ExecutionLimits,
            _control: &dyn InspectionControl,
        ) -> Result<ActionApplyObservation, InspectionError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.requested.lock().unwrap() = Some(action_digest.clone());
            self.candidate.lock().unwrap().take().expect("called once")
        }
    }

    /// The host grant: `None` authorizes, `Some` refuses with that error.
    struct Grant(Option<MutationError>);
    impl MutationPublisher<()> for Grant {
        fn authorize(&self, _lease: &()) -> Result<(), MutationError> {
            self.0.map_or(Ok(()), Err)
        }
        fn commit(
            &self,
            _lease: &(),
            _request: &MutationCommit,
            _control: &dyn crate::OperationControl,
        ) -> Result<MutationReceipt, MutationError> {
            Err(MutationError::Invalid)
        }
        fn replay(
            &self,
            _lease: &(),
            _id: &MutationId,
            _digest: &SourceFingerprint,
            _key: &IdempotencyKey,
            _control: &dyn crate::OperationControl,
        ) -> Result<MutationReceipt, MutationError> {
            Err(MutationError::Invalid)
        }
        fn receipt(&self, _lease: &(), _id: &MutationId) -> Result<MutationReceipt, MutationError> {
            Err(MutationError::Invalid)
        }
        fn recover(&self, _lease: &(), _id: &MutationId) -> Result<MutationReceipt, MutationError> {
            Err(MutationError::Invalid)
        }
    }

    fn preview_request() -> ActionPreviewRequest {
        ActionPreviewRequest {
            expected_project_fingerprint: identity_fingerprint(1),
            file: lib_rs(),
            range: text_range(1, 25, 1, 27),
            action_digest: fingerprint(7),
            timeout_seconds: 60,
        }
    }

    fn preview(
        port: &ActionPort,
        request: ActionPreviewRequest,
        grant: &Grant,
    ) -> Result<AnalyzerActionCandidate, ActionCandidateError> {
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        registry.analyzer_action_candidate(
            &opened.project_ref,
            request,
            port,
            grant,
            &Control::default(),
        )
    }

    #[test]
    fn action_candidate_binds_the_fresh_capture_the_edit_and_the_provenance() {
        let port = ActionPort::resolving(ActionResolution::Resolved(answer_action("43")));
        let produced = preview(&port, preview_request(), &Grant(None)).expect("candidate");
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
        assert_eq!(port.requested.lock().unwrap().clone(), Some(fingerprint(7)));
        assert_eq!(produced.workspace_root, "/trusted/project");
        assert_eq!(produced.action_digest, fingerprint(7));
        assert_eq!(produced.report.snapshot.source_fingerprint, fingerprint(9));

        let candidate = &produced.candidate;
        assert_eq!(candidate.kind, MutationKind::AnalyzerActionApply);
        assert_eq!(
            candidate.before,
            crate::benchmark::tests::source(),
            "before is the complete fresh capture"
        );
        assert_ne!(candidate.after, candidate.before);
        for (before, after) in candidate.before.files().iter().zip(candidate.after.files()) {
            assert_eq!(before.path(), after.path());
            if before.path() == "src/lib.rs" {
                assert_eq!(after.bytes(), b"pub fn answer() -> u8 { 43 }\n");
            } else {
                assert_eq!(after.bytes(), before.bytes(), "{}", before.path());
            }
        }
        let expected = [
            "m6-analyzer-action-v1".to_owned(),
            "local_coordinated".to_owned(),
            "linux/aarch64".to_owned(),
            "sha256:m6".to_owned(),
            execution_fingerprint(4).to_string(),
            execution_fingerprint(5).to_string(),
            "1.98.1".to_owned(),
            "1.98.1".to_owned(),
            fingerprint(9).to_string(),
            "rust-analyzer 1.98.1 (48a229c 2026-09-01)".to_owned(),
            fingerprint(1).to_string(),
            fingerprint(2).to_string(),
            fingerprint(7).to_string(),
        ]
        .iter()
        .map(|field| format!("{}:{field}", field.len()))
        .collect::<String>();
        assert_eq!(candidate.validation, expected);
    }

    #[test]
    fn action_candidate_refuses_a_stale_expected_fingerprint_before_the_port() {
        let port = ActionPort::resolving(ActionResolution::Resolved(answer_action("43")));
        let mut request = preview_request();
        request.expected_project_fingerprint = identity_fingerprint(99);
        let error = preview(&port, request, &Grant(None)).expect_err("conflict");
        assert!(matches!(
            error,
            ActionCandidateError::Request(AnalyzerRequestError::Conflict)
        ));
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn action_candidate_is_denied_without_the_host_grant_before_the_port() {
        let port = ActionPort::resolving(ActionResolution::Resolved(answer_action("43")));
        let error = preview(
            &port,
            preview_request(),
            &Grant(Some(MutationError::PermissionDenied)),
        )
        .expect_err("denied");
        assert!(matches!(
            error,
            ActionCandidateError::Mutation(MutationError::PermissionDenied)
        ));
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn action_candidate_reports_a_digest_the_fresh_session_no_longer_offers() {
        let port = ActionPort::resolving(ActionResolution::Stale);
        let error = preview(&port, preview_request(), &Grant(None)).expect_err("stale");
        assert!(
            matches!(
                &error,
                ActionCandidateError::Stale(report)
                    if report.snapshot.source_fingerprint == fingerprint(9)
            ),
            "{error:?}"
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn action_candidate_propagates_a_rejection_and_an_unanswered_session() {
        let port = ActionPort::resolving(ActionResolution::Rejected(
            ActionRejection::OverlappingRanges,
        ));
        assert!(matches!(
            preview(&port, preview_request(), &Grant(None)),
            Err(ActionCandidateError::Rejected {
                reason: ActionRejection::OverlappingRanges,
                ..
            })
        ));
        let port = ActionPort::resolving(ActionResolution::NotAnswered);
        assert!(matches!(
            preview(&port, preview_request(), &Grant(None)),
            Err(ActionCandidateError::NotAnswered(_))
        ));
    }

    #[test]
    fn action_candidate_refuses_a_file_outside_the_capture_before_the_port() {
        let port = ActionPort::resolving(ActionResolution::Resolved(answer_action("43")));
        let mut request = preview_request();
        request.file = AnalyzerFile::new("src/missing.rs".into()).unwrap();
        let error = preview(&port, request, &Grant(None)).expect_err("file not in snapshot");
        assert!(matches!(
            error,
            ActionCandidateError::Request(AnalyzerRequestError::FileNotInSnapshot)
        ));
        let mut request = preview_request();
        request.range = text_range(9, 1, 9, 1);
        let error = preview(&port, request, &Grant(None)).expect_err("range out of the file");
        assert!(matches!(
            error,
            ActionCandidateError::Request(AnalyzerRequestError::PositionOutOfRange)
        ));
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn action_candidate_refuses_edits_that_do_not_apply_to_the_capture() {
        for (edit, expected) in [
            (
                answer_edit("src/lib.rs", text_range(5, 1, 5, 2), "x"),
                AnalyzerError::OutOfRange,
            ),
            (
                answer_edit("src/other.rs", text_range(1, 1, 1, 1), "x"),
                AnalyzerError::FileNotInSnapshot,
            ),
        ] {
            let port = ActionPort::resolving(ActionResolution::Resolved(action_with(vec![edit])));
            let error =
                preview(&port, preview_request(), &Grant(None)).expect_err("not applicable");
            assert!(
                matches!(
                    &error,
                    ActionCandidateError::EditsNotApplicable { error, .. } if *error == expected
                ),
                "{error:?}"
            );
        }
        let overlapping = action_with(vec![
            answer_edit("src/lib.rs", text_range(1, 25, 1, 27), "4"),
            answer_edit("src/lib.rs", text_range(1, 26, 1, 28), "2"),
        ]);
        let port = ActionPort::resolving(ActionResolution::Resolved(overlapping));
        assert!(matches!(
            preview(&port, preview_request(), &Grant(None)),
            Err(ActionCandidateError::EditsNotApplicable {
                error: AnalyzerError::OverlappingRanges,
                ..
            })
        ));
    }

    #[test]
    fn action_candidate_refuses_an_action_that_changes_nothing() {
        for action in [answer_action("42"), action_with(Vec::new())] {
            let port = ActionPort::resolving(ActionResolution::Resolved(action));
            assert!(matches!(
                preview(&port, preview_request(), &Grant(None)),
                Err(ActionCandidateError::NoChange(_))
            ));
        }
    }

    fn actions_request() -> ActionsRequest {
        ActionsRequest {
            expected_project_fingerprint: identity_fingerprint(1),
            file: lib_rs(),
            range: text_range(1, 8, 1, 8),
            only: Vec::new(),
            timeout_seconds: 60,
        }
    }

    #[test]
    fn actions_publish_a_digest_and_an_edits_summary_per_applicable_action() {
        let port = ActionPort::listing(
            vec![
                ActionCandidate::Applicable(answer_action("4_200")),
                ActionCandidate::Rejected(ActionRejection::Command.into()),
            ],
            vec![Some(fingerprint(7)), None],
        );
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let answer = registry
            .analyzer_actions(
                &opened.project_ref,
                actions_request(),
                &port,
                &Control::default(),
            )
            .expect("actions");
        assert_eq!(port.calls.load(Ordering::SeqCst), 1);
        assert_eq!(answer.report.snapshot.source_fingerprint, fingerprint(9));
        assert_eq!(
            answer.actions,
            vec![
                ActionSummary::Applicable {
                    action_digest: fingerprint(7),
                    title: NonEmptyText::try_from("Replace the answer".to_owned()).unwrap(),
                    kind: Some(CodeActionKind::QuickFix),
                    is_preferred: true,
                    edits_summary: EditsSummary {
                        files: 1,
                        edits: 1,
                        bytes_delta: 3,
                    },
                },
                ActionSummary::Rejected {
                    reason: ActionRejection::Command,
                    title: None,
                    kind: None,
                },
            ]
        );
    }

    #[test]
    fn actions_list_a_structurally_invalid_action_as_rejected_and_keep_every_label() {
        let overlapping = action_with(vec![
            answer_edit("src/lib.rs", text_range(1, 25, 1, 27), "4"),
            answer_edit("src/lib.rs", text_range(1, 26, 1, 28), "2"),
        ]);
        let snippet = RejectedAction {
            reason: ActionRejection::Snippet,
            title: Some(NonEmptyText::try_from("Insert a snippet".to_owned()).unwrap()),
            kind: Some(CodeActionKind::RefactorRewrite),
        };
        let port = ActionPort::listing(
            vec![
                ActionCandidate::Applicable(overlapping),
                ActionCandidate::Rejected(snippet),
            ],
            vec![Some(fingerprint(7)), None],
        );
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let answer = registry
            .analyzer_actions(
                &opened.project_ref,
                actions_request(),
                &port,
                &Control::default(),
            )
            .expect("actions");
        assert_eq!(
            answer.actions,
            vec![
                ActionSummary::Rejected {
                    reason: ActionRejection::OverlappingRanges,
                    title: Some(NonEmptyText::try_from("Replace the answer".to_owned()).unwrap()),
                    kind: Some(CodeActionKind::QuickFix),
                },
                ActionSummary::Rejected {
                    reason: ActionRejection::Snippet,
                    title: Some(NonEmptyText::try_from("Insert a snippet".to_owned()).unwrap()),
                    kind: Some(CodeActionKind::RefactorRewrite),
                },
            ]
        );
    }

    #[test]
    fn actions_never_list_a_no_op_edit_as_applicable() {
        // `answer_action("42")` replaces "42" with itself: the same case
        // `action_candidate_refuses_an_action_that_changes_nothing` refuses at
        // preview. The listing must refuse it too (V07).
        let port = ActionPort::listing(
            vec![ActionCandidate::Applicable(answer_action("42"))],
            vec![Some(fingerprint(7))],
        );
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let answer = registry
            .analyzer_actions(
                &opened.project_ref,
                actions_request(),
                &port,
                &Control::default(),
            )
            .expect("actions");
        assert_eq!(
            answer.actions,
            vec![ActionSummary::Rejected {
                reason: ActionRejection::UnresolvedEdit,
                title: Some(NonEmptyText::try_from("Replace the answer".to_owned()).unwrap()),
                kind: Some(CodeActionKind::QuickFix),
            }]
        );
    }

    #[test]
    fn actions_refuse_a_stale_identity_or_a_range_beyond_the_file_before_the_port() {
        let port = ActionPort::listing(Vec::new(), Vec::new());
        let mut registry = registry(Backend::default(), TestClock::at(100));
        let opened = registry
            .open("/trusted/project", &Control::default())
            .unwrap();
        let mut stale = actions_request();
        stale.expected_project_fingerprint = identity_fingerprint(99);
        assert_eq!(
            registry
                .analyzer_actions(&opened.project_ref, stale, &port, &Control::default())
                .expect_err("conflict"),
            AnalyzerRequestError::Conflict
        );
        let mut beyond = actions_request();
        beyond.range = text_range(1, 8, 1, 31);
        assert_eq!(
            registry
                .analyzer_actions(&opened.project_ref, beyond, &port, &Control::default())
                .expect_err("range out of the file"),
            AnalyzerRequestError::PositionOutOfRange
        );
        assert_eq!(port.calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn actions_refuse_digests_misaligned_with_the_candidates() {
        for digests in [vec![None], vec![Some(fingerprint(7)), None], Vec::new()] {
            let port = ActionPort::listing(
                vec![ActionCandidate::Applicable(answer_action("43"))],
                digests,
            );
            let mut registry = registry(Backend::default(), TestClock::at(100));
            let opened = registry
                .open("/trusted/project", &Control::default())
                .unwrap();
            assert_eq!(
                registry
                    .analyzer_actions(
                        &opened.project_ref,
                        actions_request(),
                        &port,
                        &Control::default(),
                    )
                    .expect_err("misaligned"),
                AnalyzerRequestError::Inspection(InspectionError::Internal)
            );
        }
    }
}
