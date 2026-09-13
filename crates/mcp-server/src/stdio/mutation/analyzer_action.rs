//! `rust.analyzer.action.apply` (M6-05, ADR-083 §6): one rust-analyzer code
//! action, applied through the single M2 writer.
//!
//! Everything that writes is M2's, unchanged: the `MutationPlans` shared with
//! the five M2 tools (so the four-plan and 64 MiB budgets stay global),
//! `preview_diff`, `mutation_digest`, `commit_mutation`, `replay_mutation`
//! and `mutation_receipt` through the `NativeMutationStore` opened for
//! `AnalyzerActionApply` (authorization, generation and idempotency
//! included), the delivery-revocable preview and the fixed audit event. Only
//! the contract is this tool's own: the analyzer validation view and closed
//! codes the frozen M2 schemas cannot carry. Owner decision A (2026-09-12):
//! the candidate is validated structurally only and never compiled.
use super::{
    AnalyzerActionValidationView, AuditRecord, AuditedOutput, CallAuditState, CallAuditWaiter,
    Change, Concurrency, Freshness, MAX_RESULT, MutationEvidence, Provider, ReceiptChange,
    ReceiptState, SharedPlans, SnapshotSemantics, Status, Truncation, WriteConfig,
    allocation_stats, analyzer_action_validation_for, audit, elapsed_millis, preview_diff,
    receipt_changes, receipt_state,
};
use crate::stdio::{
    analyzer::WireRange,
    contract::{Contract, ToolOutput},
    project::Registry,
    workers::{WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{
    ExecutionError, InspectionControl, InspectionError, MutationPlans, MutationPreparationError,
    MutationPublisher, OperationControl, PreviewRetention, PreviewToken, ProjectError,
    ReferenceGenerator,
    analyzer::{
        ActionCandidateError, ActionPreviewRequest, AnalyzerActionCandidate, AnalyzerPort,
        AnalyzerRequestError,
    },
};
use rust_engineering_domain::{
    ActionRejection, AnalyzerError, AnalyzerFailure, AnalyzerFile, IdempotencyKey, MutationError,
    MutationId, MutationKind, MutationReceipt, OperationalErrorCode, ProjectIdentityFingerprint,
    ProjectRef, SourceFingerprint, ToolStatus,
};
use rust_engineering_execution::RustProjectInspector;
use rust_engineering_project::{OsReferences, ProjectLease, mutation_store::mutation_digest};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(in crate::stdio) const ANALYZER_ACTION_APPLY_NAME: &str = "rust.analyzer.action.apply";
/// The analyzer session's own 180 s ceiling, plus the captures and the plan
/// around it; the same outer budget as the M2 write tools.
const DEADLINE: Duration = Duration::from_secs(240);
const MAX_TIMEOUT_SECONDS: u32 = 180;
const DESCRIPTION: &str = "Preview, commit or inspect one rust-analyzer code action through the journaled M2 writer. Host --allow-analyzer-action-write WORKSPACE_ROOT and the --rust runtime on the approved M6 image are required; without the grant the tool is unavailable (SANDBOX_DENIED). Preview re-runs rust-analyzer over a fresh capture of the same file and range and requires the action_digest listed by rust.analyzer.actions to match again, else ACTION_STALE; the digest binds the action's title, kind and edits to the analyzer version, binary, configuration and analyzed capture, so a changed capture or runtime makes the plan stale. Only structurally validated TextEdits are applied (existing captured .rs files, non-overlapping, bounded, matching version; never a Command, snippet, resource operation or external URI) and preview returns the exact diff without writing source. An action may rewrite several captured .rs files (up to 128), not only the requested file: review every entry in files and the complete diff before commit. The applied result is NOT compile-verified: no cargo check runs; call rust.check after commit. New effects require an unexpired approved plan and an idempotency key. Exact ID/digest/key can replay an existing journal under current authority. Commit invalidates its input project_ref: call rust.project.open and use its newly returned data.project_ref for ALL later calls, including receipt/recovery; never reuse the precommit reference. Local coordinated publication does not exclude external editors or provide multi-file atomicity.";

fn default_timeout_seconds() -> u32 {
    60
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AnalyzerActionInput {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    action: ApplyAction,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum ApplyAction {
    /// Re-resolve a listed action by digest over a fresh capture and plan its
    /// exact diff. Never writes source.
    Preview {
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        expected_project_fingerprint: ProjectIdentityFingerprint,
        /// An `action_digest` published by `rust.analyzer.actions`.
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        action_digest: SourceFingerprint,
        /// The file the action was listed for.
        #[schemars(with = "String", regex(pattern = r"^[A-Za-z0-9_./-]{1,100}\.rs$"))]
        file: AnalyzerFile,
        /// The range the action was listed for.
        range: WireRange,
        #[serde(default = "default_timeout_seconds")]
        #[schemars(range(min = 1, max = 180))]
        timeout_seconds: u32,
    },
    Commit {
        #[schemars(regex(pattern = "^mut_[0-9a-f]{32}$"))]
        plan_id: String,
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        plan_digest: SourceFingerprint,
        #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]+$"))]
        idempotency_key: String,
    },
    Receipt {
        #[schemars(regex(pattern = "^mut_[0-9a-f]{32}$"))]
        operation_id: String,
        /// Classify an interrupted journal; never overwrite unknown source bytes.
        recover: bool,
    },
}

/// A decoded action past the Rust-level invariants the schema cannot express.
enum ApplyRequest {
    Preview(ActionPreviewRequest),
    Commit {
        plan_id: String,
        plan_digest: SourceFingerprint,
        idempotency_key: String,
    },
    Receipt {
        operation_id: String,
        recover: bool,
    },
}

impl ApplyAction {
    /// `None` for a reversed range.
    fn request(self) -> Option<ApplyRequest> {
        Some(match self {
            Self::Preview {
                expected_project_fingerprint,
                action_digest,
                file,
                range,
                timeout_seconds,
            } => ApplyRequest::Preview(ActionPreviewRequest {
                expected_project_fingerprint,
                file,
                range: range.into_domain()?,
                action_digest,
                timeout_seconds: timeout_seconds.min(MAX_TIMEOUT_SECONDS),
            }),
            Self::Commit {
                plan_id,
                plan_digest,
                idempotency_key,
            } => ApplyRequest::Commit {
                plan_id,
                plan_digest,
                idempotency_key,
            },
            Self::Receipt {
                operation_id,
                recover,
            } => ApplyRequest::Receipt {
                operation_id,
                recover,
            },
        })
    }
}

fn phase(request: &ApplyRequest) -> audit::Phase {
    match request {
        ApplyRequest::Preview(_) => audit::Phase::Preview,
        ApplyRequest::Commit { .. } => audit::Phase::Commit,
        ApplyRequest::Receipt { recover: true, .. } => audit::Phase::Recover,
        ApplyRequest::Receipt { recover: false, .. } => audit::Phase::Receipt,
    }
}

/// The closed codes of this tool (ADR-083 §3), spelled like the other M6
/// tools rather than like the frozen M2 reasons.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ApplyCode {
    InvalidOperation,
    PermissionDenied,
    SandboxDenied,
    Conflict,
    ActionStale,
    ActionRejected,
    LockBusy,
    PlanExpired,
    NotFound,
    LimitExceeded,
    ResultLimit,
    UnsupportedPlatform,
    Io,
    RecoveryRequired,
    Cancelled,
    ProjectNotFound,
    InvalidProject,
    OutputLimitExceeded,
    FileNotInSnapshot,
    FileNotUtf8,
    PositionOutOfRange,
    UnsupportedProjectConfig,
    AnalyzerNotReady,
    AnalyzerCrashed,
    AnalyzerCapabilityMismatch,
    FrameLimit,
    MessageLimit,
    TimeoutInitialize,
    TimeoutQuery,
    TimeoutTotal,
}

impl ApplyCode {
    /// `unavailable` for what the host runtime or the analyzer session could
    /// not provide; `blocked` for everything a caller acts on; never `failed`,
    /// since no validation run judges the candidate.
    fn status(self) -> Status {
        match self {
            Self::Cancelled => Status::Cancelled,
            Self::SandboxDenied
            | Self::UnsupportedPlatform
            | Self::AnalyzerNotReady
            | Self::AnalyzerCrashed
            | Self::AnalyzerCapabilityMismatch
            | Self::FrameLimit
            | Self::MessageLimit
            | Self::TimeoutInitialize
            | Self::TimeoutQuery
            | Self::TimeoutTotal => Status::Unavailable,
            _ => Status::Blocked,
        }
    }

    /// The same code in the snake_case vocabulary of the M2 audit event.
    fn event(self) -> &'static str {
        match self {
            Self::InvalidOperation => "invalid_operation",
            Self::PermissionDenied => "permission_denied",
            Self::SandboxDenied => "sandbox_denied",
            Self::Conflict => "conflict",
            Self::ActionStale => "action_stale",
            Self::ActionRejected => "action_rejected",
            Self::LockBusy => "lock_busy",
            Self::PlanExpired => "plan_expired",
            Self::NotFound => "not_found",
            Self::LimitExceeded => "limit_exceeded",
            Self::ResultLimit => "result_limit",
            Self::UnsupportedPlatform => "unsupported_platform",
            Self::Io => "io",
            Self::RecoveryRequired => "recovery_required",
            Self::Cancelled => "cancelled",
            Self::ProjectNotFound => "project_not_found",
            Self::InvalidProject => "invalid_project",
            Self::OutputLimitExceeded => "output_limit_exceeded",
            Self::FileNotInSnapshot => "file_not_in_snapshot",
            Self::FileNotUtf8 => "file_not_utf8",
            Self::PositionOutOfRange => "position_out_of_range",
            Self::UnsupportedProjectConfig => "unsupported_project_config",
            Self::AnalyzerNotReady => "analyzer_not_ready",
            Self::AnalyzerCrashed => "analyzer_crashed",
            Self::AnalyzerCapabilityMismatch => "analyzer_capability_mismatch",
            Self::FrameLimit => "frame_limit",
            Self::MessageLimit => "message_limit",
            Self::TimeoutInitialize => "timeout_initialize",
            Self::TimeoutQuery => "timeout_query",
            Self::TimeoutTotal => "timeout_total",
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::InvalidOperation => {
                "Operation is outside this tool's supported mutation contract"
            }
            Self::PermissionDenied => {
                "A current project reference and the exact host write grant for this operation are required"
            }
            Self::SandboxDenied => {
                "Host runtime policy, failed calibration or current capacity denied analysis"
            }
            Self::Conflict => {
                "Plan digest, idempotency key or approval conflicts with this operation; request a new preview"
            }
            Self::ActionStale => {
                "The action or the source under it changed since it was listed or previewed; list actions again and request a new preview"
            }
            Self::ActionRejected => {
                "The action fails structural WorkspaceEdit validation and is never applied"
            }
            Self::LockBusy => "Another operation is active; retry when it finishes",
            Self::PlanExpired => "Preview expired; request a new preview and review its diff",
            Self::NotFound => {
                "Plan or journal was not found; reopen and use the exact operation ID/digest/key for durable replay, or create a new preview"
            }
            Self::LimitExceeded => "Mutation exceeds a bounded plan, output or journal budget",
            Self::ResultLimit => {
                "The exact diff is this plan's review surface and is never trimmed; the action is too large for the response budget, so no plan was retained"
            }
            Self::UnsupportedPlatform => {
                "This writer requires the qualified macOS ARM64 APFS adapter"
            }
            Self::Io => {
                "Mutation I/O failed; consult the original operation receipt before retrying a commit"
            }
            Self::RecoveryRequired => {
                "Preserve journal and mutation temporaries; reopen and request receipt with recover=true"
            }
            Self::Cancelled => "Operation cancelled before a successful receipt was returned",
            Self::ProjectNotFound => "Project reference is missing or expired",
            Self::InvalidProject => "Captured project is invalid or unsupported",
            Self::OutputLimitExceeded => "Project metadata exceeds the response budget",
            Self::FileNotInSnapshot => "Requested file is absent from the capture",
            Self::FileNotUtf8 => "Requested file's captured bytes are not valid UTF-8",
            Self::PositionOutOfRange => "Requested range is outside the captured file",
            Self::UnsupportedProjectConfig => {
                "Capture carries a rust-analyzer.toml workspace override"
            }
            Self::AnalyzerNotReady => "Analyzer did not reach quiescent readiness in time",
            Self::AnalyzerCrashed => "Analyzer session ended without a valid answer",
            Self::AnalyzerCapabilityMismatch => {
                "Analyzer negotiated an unsupported position encoding"
            }
            Self::FrameLimit => "Analyzer session exceeded the LSP frame budget",
            Self::MessageLimit => "Analyzer session exceeded its message or byte budget",
            Self::TimeoutInitialize => "Analyzer did not initialize in time",
            Self::TimeoutQuery => "Analyzer did not answer the query in time",
            Self::TimeoutTotal => "Analyzer call exceeded its total budget",
        }
    }
}

/// A closed code and its fixed message: never analyzer or project text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Failure {
    code: ApplyCode,
    message: &'static str,
}

impl Failure {
    const fn with(code: ApplyCode, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl From<ApplyCode> for Failure {
    fn from(code: ApplyCode) -> Self {
        Self::with(code, code.message())
    }
}

const GRANT_REQUIRED: Failure = Failure::with(
    ApplyCode::SandboxDenied,
    "Host --allow-analyzer-action-write for this workspace root and the approved M6 --rust runtime are required",
);
const SOURCE_CHANGED: Failure = Failure::with(
    ApplyCode::ActionStale,
    "The captured source changed since this action was resolved or previewed; list actions again and request a new preview",
);

#[derive(Clone, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ApplyData {
    Preview {
        plan_id: String,
        plan_digest: String,
        expires_in_seconds: u64,
        /// Every captured file this action rewrites: possibly several `.rs`
        /// files (up to 128), not only the requested `file`. Review each
        /// entry and the complete `diff` before commit.
        files: Vec<Change>,
        /// The exact diff of every rewritten file: the review surface of this
        /// plan. The result is not compile-verified.
        diff: String,
        validation: AnalyzerActionValidationView,
    },
    Receipt {
        operation_id: String,
        plan_digest: String,
        state: ReceiptState,
        validation: AnalyzerActionValidationView,
        files: Vec<ReceiptChange>,
    },
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ApplyExcludedGuarantee {
    /// No `cargo check` runs against the edited source (owner decision A).
    CompileVerification,
    OsExclusionOfExternalWriters,
    MultiFileAtomicity,
    MaliciousHostProtection,
    DemonstratedPowerLossSurvival,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ApplyOutput {
    status: Status,
    error_code: Option<ApplyCode>,
    error_message: Option<&'static str>,
    summary: &'static str,
    duration_ms: u64,
    data: Option<ApplyData>,
    diagnostics: [(); 0],
    truncation: Truncation,
    evidence: MutationEvidence,
    concurrency_contract: Concurrency,
    guarantees_not_provided: [ApplyExcludedGuarantee; 5],
}

impl ToolOutput for ApplyOutput {
    fn status(&self) -> ToolStatus {
        match self.status {
            Status::Passed => ToolStatus::Passed,
            Status::Failed => ToolStatus::Failed,
            Status::Blocked => ToolStatus::Blocked,
            Status::Unavailable => ToolStatus::Unavailable,
            Status::Cancelled => ToolStatus::Cancelled,
        }
    }
}

impl AuditedOutput for ApplyOutput {
    fn event(&self) -> audit::Record<'_> {
        let (result_id, files_changed) = match &self.data {
            Some(ApplyData::Preview { plan_id, files, .. }) => {
                (Some(plan_id.as_str()), files.len())
            }
            Some(ApplyData::Receipt {
                operation_id,
                files,
                ..
            }) => (Some(operation_id.as_str()), files.len()),
            None => (None, 0),
        };
        audit::Record {
            status: audit::status(self.status),
            reason: self.error_code.map(ApplyCode::event),
            duration_ms: self.duration_ms,
            result_id,
            files_changed,
        }
    }

    fn response_lost(self) -> Self {
        if matches!(self.data, Some(ApplyData::Preview { .. })) {
            Self::failure(ApplyCode::Cancelled.into(), self.duration_ms)
        } else {
            self
        }
    }
}

impl ApplyOutput {
    fn new(
        status: Status,
        failure: Option<Failure>,
        data: Option<ApplyData>,
        summary: &'static str,
        duration_ms: u64,
    ) -> Self {
        let evidence = match &data {
            Some(
                ApplyData::Preview { plan_digest, .. } | ApplyData::Receipt { plan_digest, .. },
            ) => MutationEvidence::MutationSnapshot {
                plan_digest: plan_digest.clone(),
                semantics: SnapshotSemantics::LatestKnown,
                freshness: Freshness::Unknown,
            },
            None => MutationEvidence::Local,
        };
        Self {
            status,
            error_code: failure.map(|failure| failure.code),
            error_message: failure.map(|failure| failure.message),
            summary,
            duration_ms,
            data,
            diagnostics: [],
            truncation: Truncation::default(),
            evidence,
            concurrency_contract: Concurrency::LocalCoordinated,
            guarantees_not_provided: [
                ApplyExcludedGuarantee::CompileVerification,
                ApplyExcludedGuarantee::OsExclusionOfExternalWriters,
                ApplyExcludedGuarantee::MultiFileAtomicity,
                ApplyExcludedGuarantee::MaliciousHostProtection,
                ApplyExcludedGuarantee::DemonstratedPowerLossSurvival,
            ],
        }
    }

    fn failure(failure: Failure, duration_ms: u64) -> Self {
        Self::new(
            failure.code.status(),
            Some(failure),
            None,
            failure.message,
            duration_ms,
        )
    }
}

/// Before discovery completes every call is refused as the analyzer tools
/// refuse it: `blocked`, retryable with a new request ID.
fn bootstrap_refusal(duration_ms: u64) -> ApplyOutput {
    let failure = Failure::with(
        ApplyCode::SandboxDenied,
        "Analyzer action apply requires completed discovery; retry with a new request ID",
    );
    ApplyOutput::new(
        Status::Blocked,
        Some(failure),
        None,
        failure.message,
        duration_ms,
    )
}

/// Whether the call entered the write authority, for the audit event: a
/// missing grant or a refused authorization never did.
fn admitted(output: &ApplyOutput) -> bool {
    !(output.error_code == Some(ApplyCode::PermissionDenied)
        || output.error_message == Some(GRANT_REQUIRED.message)
        || output.error_message == bootstrap_refusal(0).error_message)
}

fn preview_output(data: ApplyData, duration_ms: u64) -> ApplyOutput {
    ApplyOutput::new(
        Status::Passed,
        None,
        Some(data),
        "Review every entry in files and this exact diff before commit; the action may rewrite several captured files, is not compile-verified, and source has not been changed",
        duration_ms,
    )
}

fn joined_output(
    result: Result<ApplyData, Failure>,
    interrupted: bool,
    duration_ms: u64,
) -> ApplyOutput {
    // As for M2: a durable receipt is never relabelled cancelled after its
    // irreversible point.
    match result {
        Ok(
            data @ ApplyData::Receipt {
                state: ReceiptState::RecoveryRequired,
                ..
            },
        ) => ApplyOutput::new(
            Status::Blocked,
            Some(ApplyCode::RecoveryRequired.into()),
            Some(data),
            "Publication needs recovery; preserve the journal and avoid concurrent edits",
            duration_ms,
        ),
        Ok(
            data @ ApplyData::Receipt {
                state: ReceiptState::Aborted,
                ..
            },
        ) => ApplyOutput::new(
            Status::Blocked,
            Some(SOURCE_CHANGED),
            Some(data),
            "Operation aborted without applying the action; list actions again and request a new preview",
            duration_ms,
        ),
        Ok(data @ ApplyData::Receipt { .. }) => ApplyOutput::new(
            Status::Passed,
            None,
            Some(data),
            "Durable mutation receipt; the applied action is not compile-verified, run rust.check",
            duration_ms,
        ),
        Ok(data) if !interrupted => preview_output(data, duration_ms),
        Ok(_) => ApplyOutput::failure(ApplyCode::Cancelled.into(), duration_ms),
        Err(failure) => ApplyOutput::failure(failure, duration_ms),
    }
}

fn validate_preview_size(
    contract: &Contract<AnalyzerActionInput, ApplyOutput>,
    data: &ApplyData,
) -> Result<(), Failure> {
    // Bound the complete MCP encoding, duplicated text included, before a plan
    // whose exact diff the peer cannot receive is retained.
    let complete = contract
        .encode(preview_output(data.clone(), u64::MAX))
        .map_err(|_| Failure::from(ApplyCode::Io))?;
    if serde_json::to_vec(&complete)
        .map_err(|_| Failure::from(ApplyCode::Io))?
        .len()
        > MAX_RESULT
    {
        return Err(ApplyCode::ResultLimit.into());
    }
    Ok(())
}

/// The store is opened for `AnalyzerActionApply` alone, so every receipt it
/// returns is that kind's and takes only the analyzer view (V07 P3-4).
fn receipt_data(receipt: MutationReceipt) -> Result<ApplyData, MutationError> {
    Ok(ApplyData::Receipt {
        operation_id: receipt.id.as_str().into(),
        validation: analyzer_action_validation_for(
            MutationKind::AnalyzerActionApply,
            &receipt.validation,
        )?,
        plan_digest: receipt.digest.to_string(),
        state: receipt_state(receipt.state),
        files: receipt_changes(receipt.files),
    })
}

/// Opens the single M2 store for this kind, refusing without the host grant.
fn open_store(provider: &mut Provider) -> Result<(), Failure> {
    if provider.config.is_none() {
        return Err(GRANT_REQUIRED);
    }
    provider.store().map_err(mutation_failure)
}

/// The ports one call runs against: the registry, the plan buffers shared
/// with the M2 tools, the analyzer session and the single M2 writer.
struct Ports<'a, A, W> {
    registry: &'a Mutex<Registry>,
    plans: &'a Mutex<SharedPlans>,
    analyzer: &'a A,
    writer: &'a W,
    control: &'a dyn InspectionControl,
}

impl<A: AnalyzerPort, W: MutationPublisher<ProjectLease>> Ports<'_, A, W> {
    /// Candidate (W07) → fresh-capture check → M2 diff, digest and plan.
    fn preview(
        &self,
        project_ref: &ProjectRef,
        request: ActionPreviewRequest,
        contract: &Contract<AnalyzerActionInput, ApplyOutput>,
        retention: PreviewToken,
        cleanup_uncertain: &AtomicBool,
    ) -> Result<ApplyData, Failure> {
        let AnalyzerActionCandidate {
            workspace_root,
            candidate,
            ..
        } = self
            .registry
            .try_lock()
            .map_err(lock_failure)?
            .analyzer_action_candidate(
                project_ref,
                request,
                self.analyzer,
                self.writer,
                self.control,
            )
            .map_err(|error| observed_candidate_failure(error, cleanup_uncertain))?;
        self.registry
            .try_lock()
            .map_err(lock_failure)?
            .finish_manifest_preview(project_ref, &candidate, self.control)
            .map_err(preparation_failure)?;
        let (files, diff) = preview_diff(&candidate).map_err(mutation_failure)?;
        let digest = mutation_digest(&candidate).map_err(mutation_failure)?;
        let validation = analyzer_action_validation_for(candidate.kind, &candidate.validation)
            .map_err(mutation_failure)?;
        let reference = OsReferences
            .generate()
            .map_err(|_| Failure::from(ApplyCode::Io))?;
        let suffix = reference
            .as_str()
            .strip_prefix("prj_")
            .ok_or(ApplyCode::Io)?;
        let id = MutationId::new(format!("mut_{suffix}")).map_err(mutation_failure)?;
        let data = ApplyData::Preview {
            plan_id: id.as_str().into(),
            plan_digest: digest.to_string(),
            expires_in_seconds: MutationPlans::TTL_SECONDS,
            files,
            diff,
            validation,
        };
        validate_preview_size(contract, &data)?;
        let mut shared = self.plans.try_lock().map_err(lock_failure)?;
        let SharedPlans { plans, clock } = &mut *shared;
        plans
            .remember_revocable(id, digest, workspace_root, candidate, clock, retention)
            .map_err(mutation_failure)?;
        Ok(data)
    }

    /// The M2 commit, plus one read-only check first: a capture that no
    /// longer matches the plan is the action going stale, reported before the
    /// writer is asked for any effect. The writer repeats that comparison
    /// itself at publication.
    fn commit(
        &self,
        project_ref: &ProjectRef,
        plan_id: String,
        plan_digest: &SourceFingerprint,
        idempotency_key: String,
    ) -> Result<ApplyData, Failure> {
        let id = MutationId::new(plan_id).map_err(mutation_failure)?;
        let key = IdempotencyKey::new(idempotency_key).map_err(mutation_failure)?;
        let shared = self.plans.try_lock().map_err(lock_failure)?;
        // A foreign-kind plan is refused before `resolve` runs, so a commit
        // through this tool never binds its idempotency key to a plan it does
        // not own.
        if shared
            .plans
            .kind_of(&id)
            .is_some_and(|kind| kind != MutationKind::AnalyzerActionApply)
        {
            return Err(ApplyCode::PermissionDenied.into());
        }
        let resolved = shared
            .plans
            .resolve(&id, plan_digest, key.clone(), &shared.clock);
        drop(shared);
        let receipt = match resolved {
            Ok(plan) => {
                if plan.request.candidate.kind != MutationKind::AnalyzerActionApply {
                    return Err(ApplyCode::PermissionDenied.into());
                }
                let mut registry = self.registry.try_lock().map_err(lock_failure)?;
                registry
                    .finish_manifest_preview(project_ref, &plan.request.candidate, self.control)
                    .map_err(preparation_failure)?;
                let receipt = registry
                    .commit_mutation(
                        project_ref,
                        &plan.workspace_root,
                        &plan.request,
                        self.writer,
                        self.control,
                    )
                    .map_err(mutation_failure)?;
                plan.retire_if_terminal(&receipt);
                receipt
            }
            Err(missing @ (MutationError::NotFound | MutationError::Expired)) => self
                .registry
                .try_lock()
                .map_err(lock_failure)?
                .replay_mutation(
                    project_ref,
                    &id,
                    plan_digest,
                    &key,
                    self.writer,
                    self.control,
                )
                .map_err(|error| {
                    mutation_failure(if error == MutationError::NotFound {
                        missing
                    } else {
                        error
                    })
                })?,
            Err(error) => return Err(mutation_failure(error)),
        };
        receipt_data(receipt).map_err(mutation_failure)
    }

    fn receipt(
        &self,
        project_ref: &ProjectRef,
        operation_id: String,
        recover: bool,
    ) -> Result<ApplyData, Failure> {
        let id = MutationId::new(operation_id).map_err(mutation_failure)?;
        let receipt = self
            .registry
            .try_lock()
            .map_err(lock_failure)?
            .mutation_receipt(project_ref, &id, recover, self.writer, self.control)
            .map_err(mutation_failure)?;
        receipt_data(receipt).map_err(mutation_failure)
    }
}

fn lock_failure<T>(error: TryLockError<T>) -> Failure {
    match error {
        TryLockError::WouldBlock => ApplyCode::LockBusy.into(),
        TryLockError::Poisoned(_) => ApplyCode::Io.into(),
    }
}

/// A `commit`, `receipt` or `recover` may have durably progressed the
/// journal before the worker's total budget expired; unlike preview, which
/// never writes, the caller must check before retrying.
const WRITE_TIMEOUT: Failure = Failure::with(
    ApplyCode::TimeoutTotal,
    "The write may or may not have landed; request receipt with the same operation ID before retrying",
);

fn worker_failure(error: WorkerError, phase: audit::Phase) -> Failure {
    match error {
        WorkerError::Busy => ApplyCode::LockBusy.into(),
        WorkerError::Cancelled => ApplyCode::Cancelled.into(),
        WorkerError::TimedOut => match phase {
            audit::Phase::Preview => ApplyCode::TimeoutTotal.into(),
            audit::Phase::Commit | audit::Phase::Receipt | audit::Phase::Recover => WRITE_TIMEOUT,
        },
        WorkerError::Internal => ApplyCode::Io.into(),
    }
}

fn mutation_failure(error: MutationError) -> Failure {
    match error {
        MutationError::Invalid => ApplyCode::InvalidOperation,
        MutationError::PermissionDenied => ApplyCode::PermissionDenied,
        MutationError::Conflict => ApplyCode::Conflict,
        MutationError::Busy => ApplyCode::LockBusy,
        MutationError::Expired => ApplyCode::PlanExpired,
        MutationError::NotFound => ApplyCode::NotFound,
        MutationError::LimitExceeded => ApplyCode::LimitExceeded,
        MutationError::UnsupportedPlatform => ApplyCode::UnsupportedPlatform,
        MutationError::Cancelled => ApplyCode::Cancelled,
        MutationError::Io => ApplyCode::Io,
        MutationError::RecoveryRequired => ApplyCode::RecoveryRequired,
    }
    .into()
}

/// A fresh capture that no longer equals the planned `before` (or a project
/// whose manifests no longer validate) is the action going stale.
fn preparation_failure(error: MutationPreparationError) -> Failure {
    match error {
        MutationPreparationError::Mutation(MutationError::Conflict)
        | MutationPreparationError::Project(ProjectError::Rejected(
            OperationalErrorCode::InvalidProject,
        )) => SOURCE_CHANGED,
        MutationPreparationError::Mutation(error) => mutation_failure(error),
        MutationPreparationError::Edit(_) => ApplyCode::InvalidOperation.into(),
        MutationPreparationError::Project(ProjectError::Cancelled) => ApplyCode::Cancelled.into(),
        MutationPreparationError::Project(ProjectError::Rejected(
            OperationalErrorCode::CommandTimeout,
        )) => ApplyCode::TimeoutTotal.into(),
        MutationPreparationError::Project(ProjectError::Rejected(_)) => {
            ApplyCode::PermissionDenied.into()
        }
        MutationPreparationError::Project(ProjectError::Internal) => ApplyCode::Io.into(),
        MutationPreparationError::Inspection(error) => inspection_failure(error),
    }
}

fn observed_candidate_failure(
    error: ActionCandidateError,
    cleanup_uncertain: &AtomicBool,
) -> Failure {
    if matches!(
        error,
        ActionCandidateError::Request(AnalyzerRequestError::Inspection(
            InspectionError::Execution(ExecutionError::CleanupUncertain)
        ))
    ) {
        cleanup_uncertain.store(true, Ordering::Release);
    }
    candidate_failure(error)
}

fn candidate_failure(error: ActionCandidateError) -> Failure {
    match error {
        ActionCandidateError::Request(error) => request_failure(error),
        ActionCandidateError::Mutation(error) => mutation_failure(error),
        ActionCandidateError::NotAnswered(report) => report
            .execution
            .failure()
            .map_or(ApplyCode::Io.into(), analyzer_failure),
        ActionCandidateError::Stale(_) => Failure::with(
            ApplyCode::ActionStale,
            "rust-analyzer no longer offers this action_digest over a fresh capture of the same file and range; list actions again",
        ),
        ActionCandidateError::Rejected { reason, .. } => rejection_failure(reason),
        ActionCandidateError::EditsNotApplicable { error, .. } => edits_failure(error),
        ActionCandidateError::NoChange(_) => Failure::with(
            ApplyCode::ActionRejected,
            "The action's edits change no captured bytes",
        ),
    }
}

fn request_failure(error: AnalyzerRequestError) -> Failure {
    match error {
        AnalyzerRequestError::Conflict => Failure::with(
            ApplyCode::Conflict,
            "expected_project_fingerprint does not match the live project identity",
        ),
        AnalyzerRequestError::FileNotInSnapshot => ApplyCode::FileNotInSnapshot.into(),
        AnalyzerRequestError::PositionOutOfRange => ApplyCode::PositionOutOfRange.into(),
        AnalyzerRequestError::Inspection(error) => inspection_failure(error),
    }
}

fn inspection_failure(error: InspectionError) -> Failure {
    match error {
        InspectionError::Project(ProjectError::Rejected(code)) => operational_failure(code),
        InspectionError::Project(ProjectError::Cancelled)
        | InspectionError::Execution(ExecutionError::Cancelled) => ApplyCode::Cancelled.into(),
        InspectionError::Execution(ExecutionError::Unavailable) => Failure::with(
            ApplyCode::SandboxDenied,
            "Approved analyzer runtime is unavailable",
        ),
        InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        ) => ApplyCode::SandboxDenied.into(),
        InspectionError::OutputLimit => ApplyCode::OutputLimitExceeded.into(),
        InspectionError::InvalidMetadata => ApplyCode::InvalidProject.into(),
        InspectionError::Execution(ExecutionError::CleanupUncertain) => Failure::with(
            ApplyCode::Io,
            "Gateway cleanup could not be verified; further execution is quarantined",
        ),
        InspectionError::Internal
        | InspectionError::Project(ProjectError::Internal)
        | InspectionError::Execution(ExecutionError::Infrastructure) => ApplyCode::Io.into(),
    }
}

fn operational_failure(code: OperationalErrorCode) -> Failure {
    match code {
        OperationalErrorCode::ProjectNotFound => ApplyCode::ProjectNotFound.into(),
        OperationalErrorCode::InvalidProject => ApplyCode::InvalidProject.into(),
        OperationalErrorCode::ToolNotInstalled => Failure::with(
            ApplyCode::SandboxDenied,
            "Approved analyzer runtime is unavailable",
        ),
        OperationalErrorCode::LockfileUpdateRequired | OperationalErrorCode::NetworkDenied => {
            Failure::with(
                ApplyCode::SandboxDenied,
                "Host runtime policy denied analyzer execution",
            )
        }
        OperationalErrorCode::CommandTimeout => ApplyCode::TimeoutTotal.into(),
        OperationalErrorCode::SandboxDenied => ApplyCode::SandboxDenied.into(),
        OperationalErrorCode::UnsupportedPlatform => Failure::with(
            ApplyCode::UnsupportedPlatform,
            "Secure analyzer session is unavailable on this platform",
        ),
        OperationalErrorCode::OutputLimitExceeded => ApplyCode::OutputLimitExceeded.into(),
    }
}

fn analyzer_failure(failure: AnalyzerFailure) -> Failure {
    match failure {
        AnalyzerFailure::FileNotInSnapshot => ApplyCode::FileNotInSnapshot,
        AnalyzerFailure::FileNotUtf8 => ApplyCode::FileNotUtf8,
        AnalyzerFailure::UnsupportedProjectConfig => ApplyCode::UnsupportedProjectConfig,
        AnalyzerFailure::PositionOutOfRange => ApplyCode::PositionOutOfRange,
        AnalyzerFailure::CapabilityMismatch => ApplyCode::AnalyzerCapabilityMismatch,
        AnalyzerFailure::NotReady => ApplyCode::AnalyzerNotReady,
        AnalyzerFailure::Crashed
        | AnalyzerFailure::ProtocolViolation
        | AnalyzerFailure::ServerError => ApplyCode::AnalyzerCrashed,
        AnalyzerFailure::ProtocolLimit => ApplyCode::MessageLimit,
        AnalyzerFailure::FrameTooLarge | AnalyzerFailure::MalformedHeader => ApplyCode::FrameLimit,
        AnalyzerFailure::TimeoutInitialize => ApplyCode::TimeoutInitialize,
        AnalyzerFailure::TimeoutQuery => ApplyCode::TimeoutQuery,
        AnalyzerFailure::TimeoutTotal => ApplyCode::TimeoutTotal,
        AnalyzerFailure::Cancelled => ApplyCode::Cancelled,
    }
    .into()
}

fn rejection_failure(reason: ActionRejection) -> Failure {
    Failure::with(
        ApplyCode::ActionRejected,
        match reason {
            ActionRejection::Command => "The action carries a Command, which is never executed",
            ActionRejection::Snippet => "The action carries a snippet edit, which is never applied",
            ActionRejection::ResourceOperation => {
                "The action creates, renames or deletes a file, which is never applied"
            }
            ActionRejection::ExternalUri => {
                "The action edits a location outside the captured source"
            }
            ActionRejection::VersionMismatch => {
                "The action was computed against a different document version"
            }
            ActionRejection::OverlappingRanges => "The action's edits overlap or share a start",
            ActionRejection::EditLimit => "The action exceeds the 128-edit ceiling",
            ActionRejection::BytesLimit => "The action exceeds the edit byte ceiling",
            ActionRejection::NotUtf8 => {
                "The action edits a file whose captured bytes are not valid UTF-8"
            }
            ActionRejection::FileNotInSnapshot => "The action edits a file absent from the capture",
            ActionRejection::UnresolvedEdit => {
                "The action's edits could not be resolved against the capture"
            }
        },
    )
}

fn edits_failure(error: AnalyzerError) -> Failure {
    match error {
        AnalyzerError::FileNotInSnapshot => rejection_failure(ActionRejection::FileNotInSnapshot),
        AnalyzerError::OverlappingRanges => rejection_failure(ActionRejection::OverlappingRanges),
        AnalyzerError::NotUtf8 => rejection_failure(ActionRejection::NotUtf8),
        AnalyzerError::LimitExceeded => Failure::with(
            ApplyCode::ActionRejected,
            "The edited source would exceed a file or bundle limit",
        ),
        _ => rejection_failure(ActionRejection::UnresolvedEdit),
    }
}

pub(in crate::stdio) struct AnalyzerActionApplyTool {
    pub(in crate::stdio) definition: Tool,
    contract: Contract<AnalyzerActionInput, ApplyOutput>,
    registry: Arc<Mutex<Registry>>,
    provider: Arc<Mutex<Provider>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
    plans: Arc<Mutex<SharedPlans>>,
    /// Set once at construction: whether `--allow-analyzer-action-write` was
    /// granted for this workspace root. Checked before the worker admits the
    /// call, so a missing grant never contends the provider lock and never
    /// surfaces as LOCK_BUSY, CANCELLED or TIMEOUT_TOTAL.
    grant_present: bool,
}

impl AnalyzerActionApplyTool {
    pub(in crate::stdio) fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
        config: Option<WriteConfig>,
        plans: Arc<Mutex<SharedPlans>>,
    ) -> Result<Self, ErrorData> {
        let contract = Contract::<AnalyzerActionInput, ApplyOutput>::new()?;
        let definition = Tool::new(
            ANALYZER_ACTION_APPLY_NAME,
            DESCRIPTION,
            (*contract.input_schema).clone(),
        )
        .with_raw_output_schema(Arc::clone(&contract.output_schema))
        .with_annotations(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(false),
        );
        let grant_present = config.is_some();
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
            plans,
            grant_present,
            provider: Arc::new(Mutex::new(Provider {
                config,
                kind: MutationKind::AnalyzerActionApply,
                store: None,
            })),
        })
    }

    pub(in crate::stdio) async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?;
        let project_ref = input.project_ref;
        let action = input
            .action
            .request()
            .ok_or_else(|| ErrorData::invalid_params("Invalid tool arguments", None))?;
        let started = Instant::now();
        let phase = phase(&action);
        if !self.ready.load(Ordering::Acquire) {
            let output = bootstrap_refusal(0);
            audit::emit(
                ANALYZER_ACTION_APPLY_NAME,
                phase,
                false,
                false,
                output.event(),
                allocation_stats(&self.plans),
            );
            return self.contract.encode(output);
        }
        if !self.grant_present {
            let output = ApplyOutput::failure(GRANT_REQUIRED, 0);
            audit::emit(
                ANALYZER_ACTION_APPLY_NAME,
                phase,
                false,
                false,
                output.event(),
                allocation_stats(&self.plans),
            );
            return self.contract.encode(output);
        }
        let registry = Arc::clone(&self.registry);
        let provider = Arc::clone(&self.provider);
        let inspector = Arc::clone(&self.inspector);
        let plans = Arc::clone(&self.plans);
        let worker_plans = Arc::clone(&plans);
        let audit_state = Arc::new(CallAuditState::new(ANALYZER_ACTION_APPLY_NAME, phase));
        let _audit_waiter = CallAuditWaiter(Arc::clone(&audit_state));
        let worker_audit = Arc::clone(&audit_state);
        let retention = PreviewRetention::default();
        let preview_token = retention.token();
        let preview_contract = Contract::<AnalyzerActionInput, ApplyOutput>::new()?;
        let result = self
            .workers
            .run_joined(context.ct, started + DEADLINE, move |control| {
                let cleanup_uncertain = AtomicBool::new(false);
                let result = (|| {
                    let mut provider = provider.try_lock().map_err(lock_failure)?;
                    open_store(&mut provider)?;
                    let store = provider.store.as_ref().ok_or(GRANT_REQUIRED)?;
                    let ports = Ports {
                        registry: registry.as_ref(),
                        plans: worker_plans.as_ref(),
                        analyzer: inspector.as_ref(),
                        writer: store,
                        control,
                    };
                    match action {
                        ApplyRequest::Preview(request) => ports.preview(
                            &project_ref,
                            request,
                            &preview_contract,
                            preview_token,
                            &cleanup_uncertain,
                        ),
                        ApplyRequest::Commit {
                            plan_id,
                            plan_digest,
                            idempotency_key,
                        } => ports.commit(&project_ref, plan_id, &plan_digest, idempotency_key),
                        ApplyRequest::Receipt {
                            operation_id,
                            recover,
                        } => ports.receipt(&project_ref, operation_id, recover),
                    }
                })();
                let output = joined_output(
                    result.clone(),
                    OperationControl::check(control).is_err(),
                    elapsed_millis(started),
                );
                worker_audit.worker_completed(AuditRecord {
                    admitted: admitted(&output),
                    cleanup_uncertain: cleanup_uncertain.load(Ordering::Acquire),
                    output,
                    allocation: allocation_stats(&worker_plans),
                });
                result
            })
            .await;
        let duration = elapsed_millis(started);
        let entered_worker = result.is_ok();
        let output = match result {
            Ok(joined) => joined_output(joined.result, joined.interrupted.is_some(), duration),
            Err(error) => ApplyOutput::failure(worker_failure(error, phase), duration),
        };
        let retain_preview = matches!(&output.data, Some(ApplyData::Preview { .. }));
        let encoded = self.contract.encode(output.clone())?;
        if serde_json::to_vec(&encoded)
            .map_err(|_| ErrorData::internal_error("Mutation encoding failed", None))?
            .len()
            > MAX_RESULT
        {
            let output = ApplyOutput::failure(ApplyCode::ResultLimit.into(), duration);
            audit_state.waiter_completed(AuditRecord {
                admitted: entered_worker,
                cleanup_uncertain: audit_state.worker_cleanup_uncertain(),
                output: output.clone(),
                allocation: allocation_stats(&plans),
            });
            return self.contract.encode(output);
        }
        if retain_preview {
            retention.retain();
        }
        audit_state.waiter_completed(AuditRecord {
            admitted: entered_worker && admitted(&output),
            cleanup_uncertain: audit_state.worker_cleanup_uncertain(),
            output,
            allocation: allocation_stats(&plans),
        });
        Ok(encoded)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)] // Fixed fixtures only.
mod tests {
    #[cfg(target_os = "macos")]
    use super::super::AnalyzerActionValidationMethod;
    use super::*;
    use rust_engineering_application::analyzer::{AnalyzerReport, AnalyzerSnapshot};
    #[cfg(target_os = "macos")]
    use rust_engineering_application::{
        ExecutionCancellation, OpenedProject,
        analyzer::{
            ActionApplyObservation, ActionResolution, ActionsObservation, AnalyzerActionRuntime,
            AnalyzerObservation,
        },
    };
    #[cfg(target_os = "macos")]
    use rust_engineering_domain::{
        AnalyzerAction, AnalyzerQuery, CodeActionKind, ExecutionFingerprint, ExecutionLimits,
        MutationCommit, MutationFileReceipt, MutationState, Position, SourceBundle, TextEdit,
        TextRange,
    };
    use rust_engineering_domain::{
        AnalyzerExecution, AnalyzerOutcome, AnalyzerReadiness, AnalyzerResult, AnalyzerRuntime,
        Completeness, ExecutionTermination, InspectionSemantics, NonEmptyText, PositionEncoding,
        ServerHealth, SessionStop, SessionSummary,
    };
    #[cfg(target_os = "macos")]
    use rust_engineering_domain::{MutationCandidate, SourceFile};
    use serde_json::{Value, json};

    fn hash(value: u8) -> SourceFingerprint {
        format!("sha256:{value:064x}").parse().unwrap()
    }

    #[cfg(target_os = "macos")]
    fn execution_hash(value: u8) -> ExecutionFingerprint {
        format!("sha256:{value:064x}").parse().unwrap()
    }

    fn execution(outcome: AnalyzerOutcome) -> AnalyzerExecution {
        AnalyzerExecution {
            identity: AnalyzerRuntime {
                version: NonEmptyText::try_from(
                    "rust-analyzer 1.98.1 (48a229c 2026-09-01)".to_owned(),
                )
                .unwrap(),
                binary_sha256: hash(1),
                image_id: NonEmptyText::try_from(hash(3).to_string()).unwrap(),
                config_digest: hash(2),
            },
            position_encoding: Some(PositionEncoding::Utf8),
            readiness: AnalyzerReadiness::Quiescent {
                elapsed_ms: 1,
                health: ServerHealth::Ok,
            },
            outcome,
            completeness: Completeness::complete(),
            session: SessionSummary {
                stop: SessionStop::Exited,
                exit_code: Some(0),
                messages_in: 5,
                messages_out: 6,
                bytes_in: 10,
                bytes_out: 10,
                stderr_bytes: 0,
                stderr_sha256: hash(8),
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
                duration_ms: 1,
            },
            termination: ExecutionTermination::Exited,
            oom_killed: Some(false),
            call_duration_ms: 1,
        }
    }

    fn report(outcome: AnalyzerOutcome) -> Box<AnalyzerReport> {
        Box::new(AnalyzerReport {
            project_ref: "prj_00000000000000000000000000000001".parse().unwrap(),
            project_identity_fingerprint: format!("sha256:{:064x}", 9u8).parse().unwrap(),
            snapshot: AnalyzerSnapshot {
                source_fingerprint: hash(4),
                files: 2,
                semantics: InspectionSemantics::LatestKnown,
                atomic: false,
            },
            execution: execution(outcome),
        })
    }

    fn answered() -> AnalyzerOutcome {
        AnalyzerOutcome::Answered(AnalyzerResult::CodeActions(Vec::new()))
    }

    fn contract() -> Contract<AnalyzerActionInput, ApplyOutput> {
        Contract::new().unwrap()
    }

    fn validation() -> AnalyzerActionValidationView {
        analyzer_action_validation_for(MutationKind::AnalyzerActionApply, &encoded_validation())
            .unwrap()
    }

    fn encoded_validation() -> String {
        let hash = hash(5).to_string();
        rust_engineering_application::analyzer::AnalyzerActionProvenance {
            platform: "linux/aarch64",
            image_id: &hash,
            configuration_fingerprint: &hash,
            session_fingerprint: &hash,
            rust_version: "1.98.1",
            cargo_version: "1.98.1",
            analyzed_source_fingerprint: &hash,
            analyzer_version: "rust-analyzer 1.98.1 (48a229c 2026-09-01)",
            binary_sha256: &hash,
            config_digest: &hash,
            action_digest: &hash,
        }
        .encode()
        .unwrap()
    }

    fn structured(output: ApplyOutput) -> Value {
        serde_json::to_value(
            contract()
                .encode(output)
                .expect("output matches its schema"),
        )
        .unwrap()["structuredContent"]
            .clone()
    }

    #[test]
    fn every_analyzer_failure_maps_to_a_closed_code_and_status() {
        use AnalyzerFailure as F;
        for (failure, code, status) in [
            (F::FileNotInSnapshot, "FILE_NOT_IN_SNAPSHOT", "blocked"),
            (F::FileNotUtf8, "FILE_NOT_UTF8", "blocked"),
            (
                F::UnsupportedProjectConfig,
                "UNSUPPORTED_PROJECT_CONFIG",
                "blocked",
            ),
            (F::PositionOutOfRange, "POSITION_OUT_OF_RANGE", "blocked"),
            (
                F::CapabilityMismatch,
                "ANALYZER_CAPABILITY_MISMATCH",
                "unavailable",
            ),
            (F::NotReady, "ANALYZER_NOT_READY", "unavailable"),
            (F::Crashed, "ANALYZER_CRASHED", "unavailable"),
            (F::ProtocolViolation, "ANALYZER_CRASHED", "unavailable"),
            (F::ServerError, "ANALYZER_CRASHED", "unavailable"),
            (F::ProtocolLimit, "MESSAGE_LIMIT", "unavailable"),
            (F::FrameTooLarge, "FRAME_LIMIT", "unavailable"),
            (F::MalformedHeader, "FRAME_LIMIT", "unavailable"),
            (F::TimeoutInitialize, "TIMEOUT_INITIALIZE", "unavailable"),
            (F::TimeoutQuery, "TIMEOUT_QUERY", "unavailable"),
            (F::TimeoutTotal, "TIMEOUT_TOTAL", "unavailable"),
            (F::Cancelled, "CANCELLED", "cancelled"),
        ] {
            let unanswered =
                ActionCandidateError::NotAnswered(report(AnalyzerOutcome::Failed(failure)));
            let value = structured(ApplyOutput::failure(candidate_failure(unanswered), 1));
            assert_eq!(value["error_code"], code, "{failure:?}");
            assert_eq!(value["status"], status, "{failure:?}");
            assert!(value["data"].is_null());
        }
    }

    #[test]
    fn candidate_and_preparation_errors_map_to_stale_rejected_and_request_codes() {
        let cleanup = AtomicBool::new(false);
        for (error, code, status) in [
            (
                ActionCandidateError::Stale(report(answered())),
                ApplyCode::ActionStale,
                "blocked",
            ),
            (
                ActionCandidateError::Rejected {
                    reason: ActionRejection::Command,
                    report: report(answered()),
                },
                ApplyCode::ActionRejected,
                "blocked",
            ),
            (
                ActionCandidateError::EditsNotApplicable {
                    error: AnalyzerError::OverlappingRanges,
                    report: report(answered()),
                },
                ApplyCode::ActionRejected,
                "blocked",
            ),
            (
                ActionCandidateError::NoChange(report(answered())),
                ApplyCode::ActionRejected,
                "blocked",
            ),
            (
                ActionCandidateError::Request(AnalyzerRequestError::Conflict),
                ApplyCode::Conflict,
                "blocked",
            ),
            (
                ActionCandidateError::Request(AnalyzerRequestError::Inspection(
                    InspectionError::Project(ProjectError::Rejected(
                        OperationalErrorCode::ProjectNotFound,
                    )),
                )),
                ApplyCode::ProjectNotFound,
                "blocked",
            ),
            (
                ActionCandidateError::Request(AnalyzerRequestError::Inspection(
                    InspectionError::Execution(ExecutionError::Unavailable),
                )),
                ApplyCode::SandboxDenied,
                "unavailable",
            ),
            (
                ActionCandidateError::Mutation(MutationError::PermissionDenied),
                ApplyCode::PermissionDenied,
                "blocked",
            ),
        ] {
            let failure = observed_candidate_failure(error, &cleanup);
            assert_eq!(failure.code, code);
            assert_eq!(
                structured(ApplyOutput::failure(failure, 1))["status"],
                status
            );
        }
        assert!(!cleanup.load(Ordering::Acquire));
        let uncertain = observed_candidate_failure(
            ActionCandidateError::Request(AnalyzerRequestError::Inspection(
                InspectionError::Execution(ExecutionError::CleanupUncertain),
            )),
            &cleanup,
        );
        assert_eq!(uncertain.code, ApplyCode::Io);
        assert!(cleanup.load(Ordering::Acquire));
        assert_eq!(
            preparation_failure(MutationPreparationError::Mutation(MutationError::Conflict)),
            SOURCE_CHANGED
        );
        assert_eq!(
            preparation_failure(MutationPreparationError::Project(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound
            )))
            .code,
            ApplyCode::PermissionDenied
        );
    }

    #[test]
    fn every_rejection_reason_has_its_own_fixed_message() {
        use ActionRejection as R;
        let reasons = [
            R::Command,
            R::Snippet,
            R::ResourceOperation,
            R::ExternalUri,
            R::VersionMismatch,
            R::OverlappingRanges,
            R::EditLimit,
            R::BytesLimit,
            R::NotUtf8,
            R::FileNotInSnapshot,
            R::UnresolvedEdit,
        ];
        let messages: std::collections::BTreeSet<_> = reasons
            .iter()
            .map(|reason| rejection_failure(*reason).message)
            .collect();
        assert_eq!(messages.len(), reasons.len());
    }

    #[test]
    fn a_commit_or_receipt_timeout_tells_the_caller_to_check_the_receipt_but_preview_does_not() {
        for phase in [
            audit::Phase::Commit,
            audit::Phase::Receipt,
            audit::Phase::Recover,
        ] {
            let failure = worker_failure(WorkerError::TimedOut, phase);
            assert_eq!(failure.code, ApplyCode::TimeoutTotal);
            assert!(
                failure.message.contains("may or may not have landed"),
                "{phase:?}: {}",
                failure.message
            );
        }
        let preview_timeout = worker_failure(WorkerError::TimedOut, audit::Phase::Preview);
        assert_eq!(preview_timeout.code, ApplyCode::TimeoutTotal);
        assert_eq!(preview_timeout.message, ApplyCode::TimeoutTotal.message());
    }

    #[test]
    fn an_oversized_preview_is_blocked_result_limit_not_a_mutation_budget_code() {
        let oversized = ApplyData::Preview {
            plan_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: hash(1).to_string(),
            expires_in_seconds: 600,
            files: Vec::new(),
            // The exact diff is the review surface (ADR-083 §3): apply
            // refuses rather than trims, so this must never retain a plan.
            diff: "x".repeat(2 * MAX_RESULT),
            validation: validation(),
        };
        let failure = validate_preview_size(&contract(), &oversized).expect_err("oversized");
        assert_eq!(failure.code, ApplyCode::ResultLimit);
        assert_eq!(
            structured(ApplyOutput::failure(failure, 1))["error_code"],
            "RESULT_LIMIT"
        );
    }

    #[test]
    fn without_the_grant_the_tool_is_unavailable_sandbox_denied_and_not_admitted() {
        let mut provider = Provider {
            config: None,
            kind: MutationKind::AnalyzerActionApply,
            store: None,
        };
        let refused = open_store(&mut provider).expect_err("no grant");
        assert!(provider.store.is_none(), "no state is created");
        let output = ApplyOutput::failure(refused, 3);
        assert!(!admitted(&output));
        let event = output.event();
        assert_eq!(event.status, "unavailable");
        assert_eq!(event.reason, Some("sandbox_denied"));
        let value = structured(output);
        assert_eq!(value["status"], "unavailable");
        assert_eq!(value["error_code"], "SANDBOX_DENIED");
        assert_eq!(value["evidence"], json!({"kind": "local"}));
        assert!(
            value["guarantees_not_provided"]
                .as_array()
                .unwrap()
                .contains(&json!("compile_verification"))
        );
    }

    #[test]
    fn bootstrap_refusal_is_blocked_sandbox_denied_and_not_admitted() {
        let output = bootstrap_refusal(0);
        assert!(!admitted(&output));
        let value = structured(output);
        assert_eq!(value["status"], "blocked");
        assert_eq!(value["error_code"], "SANDBOX_DENIED");
    }

    const EVERY_CODE: [ApplyCode; 30] = [
        ApplyCode::InvalidOperation,
        ApplyCode::PermissionDenied,
        ApplyCode::SandboxDenied,
        ApplyCode::Conflict,
        ApplyCode::ActionStale,
        ApplyCode::ActionRejected,
        ApplyCode::LockBusy,
        ApplyCode::PlanExpired,
        ApplyCode::NotFound,
        ApplyCode::LimitExceeded,
        ApplyCode::ResultLimit,
        ApplyCode::UnsupportedPlatform,
        ApplyCode::Io,
        ApplyCode::RecoveryRequired,
        ApplyCode::Cancelled,
        ApplyCode::ProjectNotFound,
        ApplyCode::InvalidProject,
        ApplyCode::OutputLimitExceeded,
        ApplyCode::FileNotInSnapshot,
        ApplyCode::FileNotUtf8,
        ApplyCode::PositionOutOfRange,
        ApplyCode::UnsupportedProjectConfig,
        ApplyCode::AnalyzerNotReady,
        ApplyCode::AnalyzerCrashed,
        ApplyCode::AnalyzerCapabilityMismatch,
        ApplyCode::FrameLimit,
        ApplyCode::MessageLimit,
        ApplyCode::TimeoutInitialize,
        ApplyCode::TimeoutQuery,
        ApplyCode::TimeoutTotal,
    ];

    #[test]
    fn every_code_audits_as_its_wire_spelling_with_its_own_fixed_message() {
        let mut messages = std::collections::BTreeSet::new();
        for code in EVERY_CODE {
            let wire = serde_json::to_value(code).unwrap();
            assert_eq!(
                wire.as_str().unwrap().to_ascii_lowercase(),
                code.event(),
                "{code:?}"
            );
            assert!(!code.message().is_empty(), "{code:?}");
            messages.insert(code.message());
            let failure = Failure::from(code);
            assert_eq!(failure, Failure::with(code, code.message()));
            let output = ApplyOutput::failure(failure, 7);
            assert_eq!(output.event().reason, Some(code.event()));
            assert!(
                !matches!(
                    ToolOutput::status(&output),
                    ToolStatus::Passed | ToolStatus::Failed
                ),
                "no validation run judges the candidate: {code:?}"
            );
            let value = structured(output);
            assert_eq!(value["error_code"], wire, "{code:?}");
            assert_eq!(value["error_message"], code.message(), "{code:?}");
        }
        assert_eq!(
            messages.len(),
            EVERY_CODE.len(),
            "no two codes share a message"
        );
    }

    #[test]
    fn every_status_reaches_the_tool_status_unchanged() {
        for (status, expected) in [
            (Status::Passed, ToolStatus::Passed),
            (Status::Failed, ToolStatus::Failed),
            (Status::Blocked, ToolStatus::Blocked),
            (Status::Unavailable, ToolStatus::Unavailable),
            (Status::Cancelled, ToolStatus::Cancelled),
        ] {
            let output = ApplyOutput::new(status, None, None, "summary", 0);
            assert_eq!(ToolOutput::status(&output), expected);
            assert!(output.error_code.is_none());
            assert!(matches!(output.evidence, MutationEvidence::Local));
        }
    }

    #[test]
    fn commit_and_receipt_actions_decode_to_their_own_audit_phases() {
        let commit = ApplyAction::Commit {
            plan_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: hash(1),
            idempotency_key: "key-1".into(),
        }
        .request()
        .unwrap();
        assert!(matches!(phase(&commit), audit::Phase::Commit));
        let ApplyRequest::Commit {
            plan_id,
            plan_digest,
            idempotency_key,
        } = commit
        else {
            panic!("expected a commit request");
        };
        assert_eq!(plan_id, "mut_0123456789abcdef0123456789abcdef");
        assert_eq!(plan_digest, hash(1));
        assert_eq!(idempotency_key, "key-1");

        for recover in [true, false] {
            let receipt = ApplyAction::Receipt {
                operation_id: "mut_0123456789abcdef0123456789abcdef".into(),
                recover,
            }
            .request()
            .unwrap();
            assert!(matches!(
                (recover, phase(&receipt)),
                (true, audit::Phase::Recover) | (false, audit::Phase::Receipt)
            ));
            assert!(matches!(
                receipt,
                ApplyRequest::Receipt { recover: decoded, .. } if decoded == recover
            ));
        }

        let preview = ApplyAction::Preview {
            expected_project_fingerprint: format!("sha256:{:064x}", 9u8).parse().unwrap(),
            action_digest: hash(7),
            file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
            range: serde_json::from_value(json!({
                "start": {"line": 1, "column": 1},
                "end": {"line": 1, "column": 3},
            }))
            .unwrap(),
            timeout_seconds: 900,
        }
        .request()
        .unwrap();
        assert!(matches!(phase(&preview), audit::Phase::Preview));
        let ApplyRequest::Preview(request) = preview else {
            panic!("expected a preview request");
        };
        assert_eq!(request.timeout_seconds, MAX_TIMEOUT_SECONDS);
    }

    fn change() -> Change {
        Change {
            path: "src/lib.rs".into(),
            before_sha256: hash(2).to_string(),
            after_sha256: hash(3).to_string(),
            before_bytes: 29,
            after_bytes: 29,
        }
    }

    #[test]
    fn the_audit_event_names_the_plan_or_operation_and_its_files() {
        let preview = ApplyData::Preview {
            plan_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: hash(1).to_string(),
            expires_in_seconds: 600,
            files: vec![change(), change()],
            diff: String::new(),
            validation: validation(),
        };
        let output = preview_output(preview, 5);
        let event = output.event();
        assert_eq!(event.status, "passed");
        assert_eq!(event.reason, None);
        assert_eq!(
            event.result_id,
            Some("mut_0123456789abcdef0123456789abcdef")
        );
        assert_eq!(event.files_changed, 2);
        assert_eq!(event.duration_ms, 5);
        assert!(admitted(&output));

        let receipt = ApplyData::Receipt {
            operation_id: "mut_00000000000000000000000000000002".into(),
            plan_digest: hash(1).to_string(),
            state: ReceiptState::Committed,
            validation: validation(),
            files: Vec::new(),
        };
        let kept = joined_output(Ok(receipt), false, 3).response_lost();
        assert!(
            kept.data.is_some(),
            "a durable receipt is never withdrawn when its response is lost"
        );
        let event = kept.event();
        assert_eq!(
            event.result_id,
            Some("mut_00000000000000000000000000000002")
        );
        assert_eq!(event.files_changed, 0);

        let refused = joined_output(Err(ApplyCode::PlanExpired.into()), false, 2);
        assert!(admitted(&refused));
        assert_eq!(refused.event().result_id, None);
        assert_eq!(structured(refused)["error_code"], "PLAN_EXPIRED");
        let denied = ApplyOutput::failure(ApplyCode::PermissionDenied.into(), 1);
        assert!(!admitted(&denied), "a refused authorization never entered");
    }

    #[test]
    fn a_store_receipt_publishes_the_analyzer_view_or_refuses_a_foreign_one() {
        let receipt = |validation: String| MutationReceipt {
            id: MutationId::new(format!("mut_{:032x}", 1)).unwrap(),
            digest: hash(1),
            state: rust_engineering_domain::MutationState::Committed,
            files: vec![rust_engineering_domain::MutationFileReceipt {
                path: "src/lib.rs".into(),
                before: hash(8),
                after: hash(9),
                before_bytes: 29,
                after_bytes: 30,
                effect_after: Some(hash(9)),
                effect_after_bytes: Some(30),
            }],
            validation,
        };
        let data = receipt_data(receipt(encoded_validation())).unwrap();
        let ApplyData::Receipt {
            operation_id,
            plan_digest,
            state,
            files,
            ..
        } = &data
        else {
            panic!("expected a receipt");
        };
        assert_eq!(operation_id, &format!("mut_{:032x}", 1));
        assert_eq!(plan_digest, &hash(1).to_string());
        assert!(matches!(state, ReceiptState::Committed));
        assert_eq!(files.len(), 1);
        let value = structured(joined_output(Ok(data), false, 1));
        assert_eq!(value["status"], "passed");
        assert_eq!(value["data"]["kind"], "receipt");

        assert!(receipt_data(receipt("not a provenance".into())).is_err());
    }

    #[test]
    fn lock_and_worker_failures_map_to_closed_codes() {
        assert_eq!(
            lock_failure(TryLockError::<()>::WouldBlock).code,
            ApplyCode::LockBusy
        );
        assert_eq!(
            lock_failure(TryLockError::Poisoned(std::sync::PoisonError::new(()))).code,
            ApplyCode::Io
        );
        let held = Mutex::new(());
        let _guard = held.lock().unwrap();
        assert_eq!(
            lock_failure(held.try_lock().unwrap_err()).code,
            ApplyCode::LockBusy
        );

        for phase in [
            audit::Phase::Preview,
            audit::Phase::Commit,
            audit::Phase::Receipt,
            audit::Phase::Recover,
        ] {
            assert_eq!(
                worker_failure(WorkerError::Busy, phase),
                ApplyCode::LockBusy.into()
            );
            assert_eq!(
                worker_failure(WorkerError::Cancelled, phase),
                ApplyCode::Cancelled.into()
            );
            assert_eq!(
                worker_failure(WorkerError::Internal, phase),
                ApplyCode::Io.into()
            );
        }
    }

    #[test]
    fn every_mutation_error_maps_to_its_closed_code() {
        for (error, code) in [
            (MutationError::Invalid, ApplyCode::InvalidOperation),
            (MutationError::PermissionDenied, ApplyCode::PermissionDenied),
            (MutationError::Conflict, ApplyCode::Conflict),
            (MutationError::Busy, ApplyCode::LockBusy),
            (MutationError::Expired, ApplyCode::PlanExpired),
            (MutationError::NotFound, ApplyCode::NotFound),
            (MutationError::LimitExceeded, ApplyCode::LimitExceeded),
            (
                MutationError::UnsupportedPlatform,
                ApplyCode::UnsupportedPlatform,
            ),
            (MutationError::Cancelled, ApplyCode::Cancelled),
            (MutationError::Io, ApplyCode::Io),
            (MutationError::RecoveryRequired, ApplyCode::RecoveryRequired),
        ] {
            assert_eq!(mutation_failure(error), code.into(), "{error:?}");
        }
    }

    #[test]
    fn every_preparation_error_is_stale_or_a_closed_code() {
        for (error, expected) in [
            (
                MutationPreparationError::Project(ProjectError::Rejected(
                    OperationalErrorCode::InvalidProject,
                )),
                SOURCE_CHANGED,
            ),
            (
                MutationPreparationError::Mutation(MutationError::Busy),
                ApplyCode::LockBusy.into(),
            ),
            (
                MutationPreparationError::Edit(
                    rust_engineering_domain::ManifestEditError::InvalidManifest,
                ),
                ApplyCode::InvalidOperation.into(),
            ),
            (
                MutationPreparationError::Project(ProjectError::Cancelled),
                ApplyCode::Cancelled.into(),
            ),
            (
                MutationPreparationError::Project(ProjectError::Rejected(
                    OperationalErrorCode::CommandTimeout,
                )),
                ApplyCode::TimeoutTotal.into(),
            ),
            (
                MutationPreparationError::Project(ProjectError::Internal),
                ApplyCode::Io.into(),
            ),
            (
                MutationPreparationError::Inspection(InspectionError::OutputLimit),
                ApplyCode::OutputLimitExceeded.into(),
            ),
        ] {
            assert_eq!(preparation_failure(error), expected);
        }
    }

    #[test]
    fn every_inspection_and_request_error_maps_to_a_closed_code() {
        for (error, code) in [
            (
                InspectionError::Project(ProjectError::Cancelled),
                ApplyCode::Cancelled,
            ),
            (
                InspectionError::Execution(ExecutionError::Cancelled),
                ApplyCode::Cancelled,
            ),
            (
                InspectionError::Execution(ExecutionError::Unavailable),
                ApplyCode::SandboxDenied,
            ),
            (
                InspectionError::Execution(ExecutionError::Denied),
                ApplyCode::SandboxDenied,
            ),
            (
                InspectionError::Execution(ExecutionError::Busy),
                ApplyCode::SandboxDenied,
            ),
            (
                InspectionError::Execution(ExecutionError::InvalidConfiguration),
                ApplyCode::SandboxDenied,
            ),
            (InspectionError::OutputLimit, ApplyCode::OutputLimitExceeded),
            (InspectionError::InvalidMetadata, ApplyCode::InvalidProject),
            (
                InspectionError::Execution(ExecutionError::CleanupUncertain),
                ApplyCode::Io,
            ),
            (InspectionError::Internal, ApplyCode::Io),
            (
                InspectionError::Project(ProjectError::Internal),
                ApplyCode::Io,
            ),
            (
                InspectionError::Execution(ExecutionError::Infrastructure),
                ApplyCode::Io,
            ),
        ] {
            assert_eq!(inspection_failure(error).code, code);
        }

        use OperationalErrorCode as O;
        for (operational, code) in [
            (O::ProjectNotFound, ApplyCode::ProjectNotFound),
            (O::InvalidProject, ApplyCode::InvalidProject),
            (O::ToolNotInstalled, ApplyCode::SandboxDenied),
            (O::LockfileUpdateRequired, ApplyCode::SandboxDenied),
            (O::NetworkDenied, ApplyCode::SandboxDenied),
            (O::CommandTimeout, ApplyCode::TimeoutTotal),
            (O::SandboxDenied, ApplyCode::SandboxDenied),
            (O::UnsupportedPlatform, ApplyCode::UnsupportedPlatform),
            (O::OutputLimitExceeded, ApplyCode::OutputLimitExceeded),
        ] {
            let failure = inspection_failure(InspectionError::Project(ProjectError::Rejected(
                operational,
            )));
            assert_eq!(failure.code, code, "{operational:?}");
            assert_eq!(operational_failure(operational), failure);
        }

        assert_eq!(
            request_failure(AnalyzerRequestError::FileNotInSnapshot),
            ApplyCode::FileNotInSnapshot.into()
        );
        assert_eq!(
            request_failure(AnalyzerRequestError::PositionOutOfRange),
            ApplyCode::PositionOutOfRange.into()
        );
    }

    #[test]
    fn inapplicable_edits_are_rejected_with_the_matching_reason() {
        assert_eq!(
            edits_failure(AnalyzerError::FileNotInSnapshot),
            rejection_failure(ActionRejection::FileNotInSnapshot)
        );
        assert_eq!(
            edits_failure(AnalyzerError::NotUtf8),
            rejection_failure(ActionRejection::NotUtf8)
        );
        let limit = edits_failure(AnalyzerError::LimitExceeded);
        assert_eq!(limit.code, ApplyCode::ActionRejected);
        assert!(limit.message.contains("limit"), "{}", limit.message);
        assert_eq!(
            edits_failure(AnalyzerError::Invalid),
            rejection_failure(ActionRejection::UnresolvedEdit)
        );
    }

    #[test]
    fn the_tool_is_a_destructive_write_and_records_a_missing_grant() {
        let backend = rust_engineering_project::SecureProjects::new(&[])
            .map_err(|_| "backend")
            .unwrap();
        let registry = Registry::new(
            backend,
            OsReferences,
            rust_engineering_project::MonotonicClock::default(),
            10,
            1,
        )
        .map_err(|_| "registry")
        .unwrap();
        let tool = AnalyzerActionApplyTool::new(
            Arc::new(Mutex::new(registry)),
            Workers::new(),
            Arc::new(RustProjectInspector::new(None)),
            Arc::new(AtomicBool::new(false)),
            None,
            Arc::new(Mutex::new(SharedPlans::default())),
        )
        .unwrap();
        assert_eq!(tool.definition.name, ANALYZER_ACTION_APPLY_NAME);
        let annotations = tool.definition.annotations.as_ref().unwrap();
        assert_eq!(annotations.read_only_hint, Some(false));
        assert_eq!(annotations.destructive_hint, Some(true));
        assert!(!tool.grant_present);
        let provider = tool.provider.lock().unwrap();
        assert!(provider.config.is_none() && provider.store.is_none());
    }

    #[test]
    fn previews_and_receipts_validate_and_a_lost_preview_is_cancelled() {
        let preview = ApplyData::Preview {
            plan_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: hash(1).to_string(),
            expires_in_seconds: 600,
            files: vec![Change {
                path: "src/lib.rs".into(),
                before_sha256: hash(2).to_string(),
                after_sha256: hash(3).to_string(),
                before_bytes: 29,
                after_bytes: 29,
            }],
            diff: "--- a/src/lib.rs\n+++ b/src/lib.rs\n".into(),
            validation: validation(),
        };
        let value = structured(joined_output(Ok(preview.clone()), false, 4));
        assert_eq!(value["status"], "passed");
        assert_eq!(
            value["data"]["validation"]["method"],
            "workspace_edit_structural_only"
        );
        assert_eq!(value["evidence"]["kind"], "mutation_snapshot");
        let lost = preview_output(preview.clone(), 9).response_lost();
        assert!(lost.data.is_none());
        assert_eq!(lost.error_code, Some(ApplyCode::Cancelled));
        assert_eq!(
            structured(joined_output(Ok(preview), true, 4))["status"],
            "cancelled"
        );

        let receipt = |state| ApplyData::Receipt {
            operation_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: hash(1).to_string(),
            state,
            validation: validation(),
            files: Vec::new(),
        };
        let aborted = structured(joined_output(Ok(receipt(ReceiptState::Aborted)), false, 1));
        assert_eq!(aborted["status"], "blocked");
        assert_eq!(aborted["error_code"], "ACTION_STALE");
        assert_eq!(aborted["data"]["state"], "aborted");
        let committed = structured(joined_output(Ok(receipt(ReceiptState::Committed)), true, 1));
        assert_eq!(committed["status"], "passed", "a durable receipt is kept");
        let recovery = structured(joined_output(
            Ok(receipt(ReceiptState::RecoveryRequired)),
            false,
            1,
        ));
        assert_eq!(recovery["error_code"], "RECOVERY_REQUIRED");
    }

    #[test]
    fn the_input_contract_is_closed_and_a_reversed_range_is_invalid() {
        let contract = contract();
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let valid_ref = "prj_00000000000000000000000000000001";
        let preview = |range: Value| {
            json!({"project_ref": valid_ref, "action": {
                "mode": "preview", "expected_project_fingerprint": fingerprint,
                "action_digest": fingerprint, "file": "src/lib.rs", "range": range
            }})
        };
        let decode = |value: Value| contract.decode(Some(serde_json::from_value(value).unwrap()));
        let forward = json!({"start": {"line": 2, "column": 9}, "end": {"line": 2, "column": 12}});
        let reversed = json!({"start": {"line": 2, "column": 9}, "end": {"line": 1, "column": 1}});
        let decoded = decode(preview(forward.clone())).expect("valid preview");
        let Some(ApplyRequest::Preview(request)) = decoded.action.request() else {
            panic!("expected a preview request");
        };
        assert_eq!(request.timeout_seconds, 60);
        assert!(
            decode(preview(reversed))
                .expect("the schema cannot see the order")
                .action
                .request()
                .is_none()
        );
        let mut extra = preview(forward.clone());
        extra["action"]["edit"] = json!({"path": "src/lib.rs"});
        let mut missing = preview(forward);
        missing["action"]
            .as_object_mut()
            .unwrap()
            .remove("action_digest");
        for invalid in [
            extra,
            missing,
            json!({"project_ref": valid_ref, "action": {"mode": "apply"}}),
            json!({"project_ref": valid_ref, "action": {"mode": "commit", "plan_id": "mut_bad",
                "plan_digest": fingerprint, "idempotency_key": "key"}}),
            json!({"project_ref": valid_ref, "action": {"mode": "receipt",
                "operation_id": "mut_00000000000000000000000000000001", "recover": false, "force": true}}),
        ] {
            let error = decode(invalid).err().expect("closed contract");
            assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        }
    }

    // ---- The preview → commit → receipt lifecycle over a real registry ----

    #[cfg(target_os = "macos")]
    struct Proceed;
    #[cfg(target_os = "macos")]
    impl OperationControl for Proceed {
        fn check(&self) -> Result<(), ProjectError> {
            Ok(())
        }
    }
    #[cfg(target_os = "macos")]
    impl ExecutionCancellation for Proceed {
        fn is_cancelled(&self) -> bool {
            false
        }
    }

    /// The analyzer port: resolves whatever the test configured, once.
    #[cfg(target_os = "macos")]
    struct Port(Mutex<Option<ActionResolution>>);
    #[cfg(target_os = "macos")]
    impl Port {
        fn resolving(resolution: ActionResolution) -> Self {
            Self(Mutex::new(Some(resolution)))
        }
    }
    #[cfg(target_os = "macos")]
    impl AnalyzerPort for Port {
        fn analyze(
            &self,
            _: &SourceBundle,
            _: &AnalyzerQuery,
            _: ExecutionLimits,
            _: &dyn InspectionControl,
        ) -> Result<AnalyzerObservation, InspectionError> {
            Err(InspectionError::Internal)
        }
        fn resolve_actions(
            &self,
            _: &SourceBundle,
            _: &AnalyzerFile,
            _: TextRange,
            _: &[CodeActionKind],
            _: ExecutionLimits,
            _: &dyn InspectionControl,
        ) -> Result<ActionsObservation, InspectionError> {
            Err(InspectionError::Internal)
        }
        fn resolve_action_candidate(
            &self,
            _: &SourceBundle,
            _: &AnalyzerFile,
            _: TextRange,
            _: &SourceFingerprint,
            _: ExecutionLimits,
            _: &dyn InspectionControl,
        ) -> Result<ActionApplyObservation, InspectionError> {
            let resolution = self
                .0
                .lock()
                .unwrap()
                .take()
                .ok_or(InspectionError::Internal)?;
            Ok(ActionApplyObservation {
                observation: AnalyzerObservation {
                    source_fingerprint: hash(4),
                    execution: execution(answered()),
                },
                runtime: AnalyzerActionRuntime {
                    platform: "linux/aarch64".into(),
                    image_id: hash(3).to_string(),
                    configuration_fingerprint: execution_hash(5),
                    session_fingerprint: execution_hash(6),
                    rust_version: "1.98.1".into(),
                    cargo_version: "1.98.1".into(),
                },
                resolution,
            })
        }
    }

    /// The writer: authorizes, records every commit and never touches disk.
    #[cfg(target_os = "macos")]
    #[derive(Default)]
    struct Writer {
        commits: Mutex<Vec<MutationCommit>>,
        receipts: Mutex<Vec<MutationReceipt>>,
    }
    #[cfg(target_os = "macos")]
    impl MutationPublisher<ProjectLease> for Writer {
        fn authorize(&self, _: &ProjectLease) -> Result<(), MutationError> {
            Ok(())
        }
        fn commit(
            &self,
            _: &ProjectLease,
            request: &MutationCommit,
            _: &dyn OperationControl,
        ) -> Result<MutationReceipt, MutationError> {
            let receipt = MutationReceipt {
                id: request.id.clone(),
                digest: request.digest.clone(),
                state: MutationState::Committed,
                files: vec![MutationFileReceipt {
                    path: "src/lib.rs".into(),
                    before: hash(8),
                    after: hash(9),
                    before_bytes: 29,
                    after_bytes: 29,
                    effect_after: Some(hash(9)),
                    effect_after_bytes: Some(29),
                }],
                validation: request.candidate.validation.clone(),
            };
            self.commits.lock().unwrap().push(request.clone());
            self.receipts.lock().unwrap().push(receipt.clone());
            Ok(receipt)
        }
        fn replay(
            &self,
            _: &ProjectLease,
            _: &MutationId,
            _: &SourceFingerprint,
            _: &IdempotencyKey,
            _: &dyn OperationControl,
        ) -> Result<MutationReceipt, MutationError> {
            Err(MutationError::NotFound)
        }
        fn receipt(
            &self,
            _: &ProjectLease,
            id: &MutationId,
        ) -> Result<MutationReceipt, MutationError> {
            self.receipts
                .lock()
                .unwrap()
                .iter()
                .find(|receipt| &receipt.id == id)
                .cloned()
                .ok_or(MutationError::NotFound)
        }
        fn recover(
            &self,
            lease: &ProjectLease,
            id: &MutationId,
        ) -> Result<MutationReceipt, MutationError> {
            self.receipt(lease, id)
        }
    }

    #[cfg(target_os = "macos")]
    const SOURCE: &str = "pub fn answer() -> u8 { 42 }\n";

    #[cfg(target_os = "macos")]
    struct Project(std::path::PathBuf);
    #[cfg(target_os = "macos")]
    impl Project {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "rust-mcp-action-apply-{}-{tag}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(
                root.join("Cargo.toml"),
                "[package]\nname = \"action-apply\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )
            .unwrap();
            std::fs::write(root.join("src/lib.rs"), SOURCE).unwrap();
            Self(root)
        }
        fn path(&self) -> &str {
            self.0.to_str().unwrap()
        }
        fn source(&self) -> String {
            std::fs::read_to_string(self.0.join("src/lib.rs")).unwrap()
        }
        fn registry(&self) -> Mutex<Registry> {
            let backend =
                rust_engineering_project::SecureProjects::new(std::slice::from_ref(&self.0))
                    .map_err(|_| "backend")
                    .unwrap();
            Mutex::new(
                Registry::new(
                    backend,
                    OsReferences,
                    rust_engineering_project::MonotonicClock::default(),
                    600,
                    4,
                )
                .map_err(|_| "registry")
                .unwrap(),
            )
        }
    }
    #[cfg(target_os = "macos")]
    impl Drop for Project {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(target_os = "macos")]
    fn range() -> TextRange {
        TextRange::new(Position::new(1, 25).unwrap(), Position::new(1, 27).unwrap()).unwrap()
    }

    #[cfg(target_os = "macos")]
    fn answer_action() -> AnalyzerAction {
        AnalyzerAction {
            title: NonEmptyText::try_from("Replace the answer".to_owned()).unwrap(),
            kind: Some(CodeActionKind::QuickFix),
            is_preferred: true,
            edits: vec![TextEdit {
                file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
                range: range(),
                new_text: "43".into(),
            }],
        }
    }

    #[cfg(target_os = "macos")]
    fn preview_request(opened: &OpenedProject) -> ActionPreviewRequest {
        ActionPreviewRequest {
            expected_project_fingerprint: opened.identity.fingerprint.clone(),
            file: AnalyzerFile::new("src/lib.rs".into()).unwrap(),
            range: range(),
            action_digest: hash(7),
            timeout_seconds: 60,
        }
    }

    #[cfg(target_os = "macos")]
    fn run_preview(
        ports: &Ports<'_, Port, Writer>,
        opened: &OpenedProject,
    ) -> Result<ApplyData, Failure> {
        let retention = PreviewRetention::default();
        let data = ports.preview(
            &opened.project_ref,
            preview_request(opened),
            &contract(),
            retention.token(),
            &AtomicBool::new(false),
        );
        if data.is_ok() {
            retention.retain();
        }
        data
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn preview_commit_receipt_go_through_the_writer_and_invalidate_the_pre_commit_reference() {
        let project = Project::new("lifecycle");
        let registry = project.registry();
        let plans = Mutex::new(SharedPlans::default());
        let port = Port::resolving(ActionResolution::Resolved(answer_action()));
        let writer = Writer::default();
        let ports = Ports {
            registry: &registry,
            plans: &plans,
            analyzer: &port,
            writer: &writer,
            control: &Proceed,
        };
        let opened = registry
            .lock()
            .unwrap()
            .open(project.path(), &Proceed)
            .unwrap();

        let Ok(ApplyData::Preview {
            plan_id,
            plan_digest,
            files,
            diff,
            validation,
            ..
        }) = run_preview(&ports, &opened)
        else {
            panic!("expected a preview");
        };
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/lib.rs");
        assert!(diff.contains("-pub fn answer() -> u8 { 42 }"), "{diff}");
        assert!(diff.contains("+pub fn answer() -> u8 { 43 }"), "{diff}");
        assert!(matches!(
            validation.method,
            AnalyzerActionValidationMethod::WorkspaceEditStructuralOnly
        ));
        assert_eq!(validation.action_digest, hash(7).to_string());
        assert_eq!(project.source(), SOURCE, "preview never writes source");
        assert!(writer.commits.lock().unwrap().is_empty());

        let wrong = ports.commit(
            &opened.project_ref,
            plan_id.clone(),
            &hash(99),
            "key-1".into(),
        );
        assert_eq!(wrong.map(|_| ()), Err(ApplyCode::Conflict.into()));
        let plan_digest: SourceFingerprint = plan_digest.parse().unwrap();
        let Ok(ApplyData::Receipt {
            operation_id,
            state,
            validation,
            ..
        }) = ports.commit(
            &opened.project_ref,
            plan_id.clone(),
            &plan_digest,
            "key-1".into(),
        )
        else {
            panic!("expected a receipt");
        };
        assert!(matches!(state, ReceiptState::Committed));
        assert_eq!(operation_id, plan_id);
        assert_eq!(validation.action_digest, hash(7).to_string());
        let commits = writer.commits.lock().unwrap().clone();
        assert_eq!(commits.len(), 1, "exactly one effect, through the writer");
        assert_eq!(commits[0].candidate.kind, MutationKind::AnalyzerActionApply);
        assert_eq!(commits[0].digest, plan_digest);

        let stale = ports.receipt(&opened.project_ref, operation_id.clone(), false);
        assert_eq!(
            stale.map(|_| ()),
            Err(ApplyCode::PermissionDenied.into()),
            "the pre-commit project_ref is invalidated"
        );
        let reopened = registry
            .lock()
            .unwrap()
            .open(project.path(), &Proceed)
            .unwrap();
        assert!(matches!(
            ports.receipt(&reopened.project_ref, operation_id, false),
            Ok(ApplyData::Receipt {
                state: ReceiptState::Committed,
                ..
            })
        ));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_stale_digest_or_a_source_changed_after_preview_is_action_stale_and_never_committed() {
        let project = Project::new("stale");
        let registry = project.registry();
        let plans = Mutex::new(SharedPlans::default());
        let writer = Writer::default();
        let opened = registry
            .lock()
            .unwrap()
            .open(project.path(), &Proceed)
            .unwrap();

        let stale_port = Port::resolving(ActionResolution::Stale);
        let ports = Ports {
            registry: &registry,
            plans: &plans,
            analyzer: &stale_port,
            writer: &writer,
            control: &Proceed,
        };
        let refused = run_preview(&ports, &opened).map(|_| ());
        assert!(
            matches!(
                refused,
                Err(Failure {
                    code: ApplyCode::ActionStale,
                    ..
                })
            ),
            "{refused:?}"
        );
        assert_eq!(plans.lock().unwrap().plans.allocation_stats().plans, 0);

        let port = Port::resolving(ActionResolution::Resolved(answer_action()));
        let ports = Ports {
            analyzer: &port,
            ..ports
        };
        let Ok(ApplyData::Preview {
            plan_id,
            plan_digest,
            ..
        }) = run_preview(&ports, &opened)
        else {
            panic!("expected a preview");
        };
        let edited = "pub fn answer() -> u8 { 41 }\n";
        std::fs::write(project.0.join("src/lib.rs"), edited).unwrap();
        let committed = ports.commit(
            &opened.project_ref,
            plan_id,
            &plan_digest.parse().unwrap(),
            "key-stale".into(),
        );
        assert_eq!(committed.map(|_| ()), Err(SOURCE_CHANGED));
        assert!(writer.commits.lock().unwrap().is_empty());
        assert_eq!(project.source(), edited);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_plan_of_another_writer_kind_is_refused_before_the_writer() {
        let project = Project::new("kind");
        let registry = project.registry();
        let plans = Mutex::new(SharedPlans::default());
        let port = Port::resolving(ActionResolution::Stale);
        let writer = Writer::default();
        let opened = registry
            .lock()
            .unwrap()
            .open(project.path(), &Proceed)
            .unwrap();
        let bundle = SourceBundle::new(vec![
            SourceFile::new("src/lib.rs".into(), SOURCE.as_bytes().to_vec()).unwrap(),
        ])
        .unwrap();
        let id = MutationId::new(format!("mut_{:032x}", 1)).unwrap();
        {
            let mut shared = plans.lock().unwrap();
            let SharedPlans { plans, clock } = &mut *shared;
            plans
                .remember(
                    id.clone(),
                    hash(11),
                    project.path().into(),
                    MutationCandidate {
                        kind: MutationKind::FormatApply,
                        before: bundle.clone(),
                        after: bundle,
                        validation: String::new(),
                    },
                    clock,
                )
                .unwrap();
        }
        let ports = Ports {
            registry: &registry,
            plans: &plans,
            analyzer: &port,
            writer: &writer,
            control: &Proceed,
        };
        let refused = ports.commit(
            &opened.project_ref,
            id.as_str().into(),
            &hash(11),
            "key".into(),
        );
        assert_eq!(refused.map(|_| ()), Err(ApplyCode::PermissionDenied.into()));
        assert!(writer.commits.lock().unwrap().is_empty());
        let shared = plans.lock().unwrap();
        assert!(
            shared
                .plans
                .resolve(
                    &id,
                    &hash(11),
                    IdempotencyKey::new("key".into()).unwrap(),
                    &shared.clock,
                )
                .is_ok(),
            "the refused call never bound this idempotency key: the plan still resolves for its owning tool"
        );
    }

    /// V08 item 9(c): the real writer refuses a grant for a different root,
    /// not only the application-level mock (`Grant`, in `application::analyzer`
    /// tests). This exercises the tool's own `Ports::preview` against a real
    /// `NativeMutationStore` opened for a root the previewed project is not
    /// under.
    #[cfg(target_os = "macos")]
    #[test]
    fn a_grant_for_a_different_root_stays_permission_denied_at_the_tool_layer() {
        let project = Project::new("wrong-root-grant");
        let base = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "rust-mcp-action-apply-wrong-root-grant-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let granted_root = base.join("granted");
        let state = base.join("state");
        std::fs::create_dir_all(&granted_root).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        // The store requires its state directory private (`require_private_directory`).
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
        }

        let registry = project.registry();
        let opened = registry
            .lock()
            .unwrap()
            .open(project.path(), &Proceed)
            .unwrap();
        let store = rust_engineering_project::mutation_store::NativeMutationStore::open_for_kind(
            &state,
            std::slice::from_ref(&granted_root),
            MutationKind::AnalyzerActionApply,
        )
        .unwrap();
        let plans = Mutex::new(SharedPlans::default());
        // Never reached: authorize refuses before any analyzer session.
        let port = Port::resolving(ActionResolution::Stale);
        let ports = Ports {
            registry: &registry,
            plans: &plans,
            analyzer: &port,
            writer: &store,
            control: &Proceed,
        };
        let retention = PreviewRetention::default();
        let refused = ports
            .preview(
                &opened.project_ref,
                preview_request(&opened),
                &contract(),
                retention.token(),
                &AtomicBool::new(false),
            )
            .map(|_| ());
        assert_eq!(refused, Err(ApplyCode::PermissionDenied.into()));
        let _ = std::fs::remove_dir_all(&base);
    }
}
