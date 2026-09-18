//! New M2 contract; does not extend any M1 error enum or schema.
mod analyzer_action;
mod audit;
mod semantic_input;
use super::{
    contract::{Contract, ToolOutput},
    project::Registry,
    workers::{WorkerError, Workers},
};
pub(super) use analyzer_action::{
    ANALYZER_ACTION_APPLY_NAME, AnalyzerActionApplyTool, definition as analyzer_action_definition,
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{
    ExecutionError, InspectionError, MutationAllocationStats, MutationPlans,
    MutationPreparationError, PreviewRetention, ProjectError, ReferenceGenerator,
};
use rust_engineering_domain::{
    IdempotencyKey, ManifestEdit, MutationCandidate, MutationError, MutationFileReceipt,
    MutationId, MutationKind, MutationReceipt, MutationState, ProjectIdentityFingerprint,
    ProjectRef, SourceFingerprint, ToolStatus,
};
use rust_engineering_execution::RustProjectInspector;
use rust_engineering_project::{
    MonotonicClock, OsReferences, TomlManifestEditor, mutation_bytes_digest,
    mutation_store::{NativeMutationStore, mutation_digest},
    prepare_mutation_state,
};
use schemars::JsonSchema;
use semantic_input::{DependencyAddInput, DependencyRemoveInput, Edit};
use serde::{Deserialize, Serialize};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub const NAME: &str = "rust.manifest.patch";
pub const FORMAT_NAME: &str = "rust.fmt.apply";
pub const FIX_NAME: &str = "rust.fix.apply";
pub const DEPENDENCY_ADD_NAME: &str = semantic_input::DEPENDENCY_ADD_NAME;
pub const DEPENDENCY_REMOVE_NAME: &str = semantic_input::DEPENDENCY_REMOVE_NAME;
const DEADLINE: Duration = Duration::from_secs(240);
const MAX_RESULT: usize = 512 * 1024;

#[derive(Clone)]
pub(super) struct WriteConfig {
    pub roots: Vec<PathBuf>,
    pub state_parent: PathBuf,
    pub vendor: Option<super::HostCargoVendorConfig>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    action: Action,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum Action {
    #[serde(skip)]
    SemanticPreview {
        expected_project_fingerprint: ProjectIdentityFingerprint,
        target_manifest: String,
        edit: ManifestEdit,
    },
    #[serde(skip)]
    FormatPreview {
        expected_project_fingerprint: ProjectIdentityFingerprint,
    },
    Preview {
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        expected_project_fingerprint: ProjectIdentityFingerprint,
        edit: Edit,
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FormatInput {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    action: FormatAction,
}
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum FormatAction {
    Preview {
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        expected_project_fingerprint: ProjectIdentityFingerprint,
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
        recover: bool,
    },
}

pub(super) trait MutationInput:
    serde::de::DeserializeOwned + JsonSchema + Send + 'static
{
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    const KIND: MutationKind;
    fn into_request(self) -> Result<Input, ErrorData>;
}
impl MutationInput for Input {
    const NAME: &'static str = NAME;
    const KIND: MutationKind = MutationKind::ManifestPatch;
    const DESCRIPTION: &'static str = "Preview, commit or inspect a journaled semantic root Cargo.toml edit. Host --allow-manifest-write is required. Preview validates the exact candidate in the approved offline Cargo runtime and returns its diff/digest. New effects require an unexpired approved plan and an idempotency key. Exact ID/digest/key can replay an existing journal under current authority. Commit invalidates its input project_ref: call rust.project.open and use its newly returned data.project_ref for ALL later calls, including receipt/recovery; never reuse the precommit reference. Local coordinated writes do not exclude external editors or provide multi-file atomicity. Supports closed feature, built-in profile setting, workspace dependency and package/workspace rust/clippy lint set/remove. Dependency resolution requires host-approved offline vendor data and preserves whether Cargo.lock exists.";
    fn into_request(self) -> Result<Input, ErrorData> {
        Ok(self)
    }
}
impl MutationInput for FormatInput {
    const NAME: &'static str = FORMAT_NAME;
    const KIND: MutationKind = MutationKind::FormatApply;
    const DESCRIPTION: &'static str = "Preview, commit or inspect a journaled rustfmt operation on existing Rust source files. Host --allow-fmt-write is required. Preview runs rustfmt in the approved offline sandbox, verifies fmt.check against the complete exported candidate, and returns its exact diff/digest without source writes. New effects require an unexpired approved plan and an idempotency key. Exact ID/digest/key can replay an existing journal under current authority. Commit invalidates its input project_ref: call rust.project.open and use its newly returned data.project_ref for ALL later calls, including receipt/recovery; never reuse the precommit reference. Local coordinated publication does not exclude external editors or provide multi-file atomicity.";
    fn into_request(self) -> Result<Input, ErrorData> {
        Ok(Input {
            project_ref: self.project_ref,
            action: match self.action {
                FormatAction::Preview {
                    expected_project_fingerprint,
                } => Action::FormatPreview {
                    expected_project_fingerprint,
                },
                FormatAction::Commit {
                    plan_id,
                    plan_digest,
                    idempotency_key,
                } => Action::Commit {
                    plan_id,
                    plan_digest,
                    idempotency_key,
                },
                FormatAction::Receipt {
                    operation_id,
                    recover,
                } => Action::Receipt {
                    operation_id,
                    recover,
                },
            },
        })
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct FixInput {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    action: FormatAction,
}
impl MutationInput for FixInput {
    const NAME: &'static str = FIX_NAME;
    const KIND: MutationKind = MutationKind::FixApply;
    const DESCRIPTION: &'static str = "Preview, commit or inspect a journaled cargo fix operation on existing Rust source. Host --allow-fix-write is required. The fixed offline sandbox command selects all workspace targets and default features, then independently checks the complete candidate. Project build scripts and proc macros may influence permitted source changes; review the exact diff. No edition migration, broken-code or arbitrary flags. New effects require an unexpired approved plan and an idempotency key. Exact ID/digest/key can replay an existing journal under current authority. Commit invalidates its input project_ref: call rust.project.open and use its newly returned data.project_ref for ALL later calls, including receipt/recovery; never reuse the precommit reference. Local coordinated publication does not exclude external editors or provide multi-file atomicity.";
    fn into_request(self) -> Result<Input, ErrorData> {
        FormatInput {
            project_ref: self.project_ref,
            action: self.action,
        }
        .into_request()
    }
}

#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Status {
    Passed,
    Failed,
    Blocked,
    Unavailable,
    Cancelled,
}
#[derive(Clone, Copy, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Reason {
    InvalidOperation,
    PermissionDenied,
    Conflict,
    LockBusy,
    PlanExpired,
    NotFound,
    LimitExceeded,
    UnsupportedPlatform,
    Io,
    RecoveryRequired,
    ToolchainUnavailable,
    CandidateInvalid,
    Cancelled,
    CommandTimeout,
    OfflineDataMissing,
    OfflineDataInvalid,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Concurrency {
    LocalCoordinated,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ExcludedGuarantee {
    OsExclusionOfExternalWriters,
    MultiFileAtomicity,
    MaliciousHostProtection,
    DemonstratedPowerLossSurvival,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Change {
    path: String,
    before_sha256: String,
    after_sha256: String,
    before_bytes: u64,
    after_bytes: u64,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ValidationView {
    method: ValidationMethod,
    semantics: SnapshotSemantics,
    platform: String,
    image_id: String,
    configuration_fingerprint: String,
    execution_fingerprint: String,
    rust_version: String,
    cargo_version: String,
    candidate_source_fingerprint: String,
    mutation_execution_fingerprint: Option<String>,
    resolution: Option<ResolutionView>,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ValidationMethod {
    CargoMetadataFrozenNoDeps,
    RustfmtThenFmtCheck,
    CargoFixThenCheck,
    CargoMetadataOfflineThenFrozen,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum SnapshotSemantics {
    LatestKnown,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Freshness {
    Unknown,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum MutationEvidence {
    Local,
    MutationSnapshot {
        plan_digest: String,
        semantics: SnapshotSemantics,
        freshness: Freshness,
    },
}
#[derive(Clone, Default, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Truncation {
    stdout_truncated: bool,
    stderr_truncated: bool,
    diagnostics_omitted: u64,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ResolutionView {
    resolution_execution_fingerprint: String,
    dataset_fingerprint: String,
    resolved_lock_fingerprint: String,
    lock_policy: LockPolicy,
    lock_disposition: LockDisposition,
    manifest_path: String,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum LockPolicy {
    PreservePresence,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum LockDisposition {
    UpdatedExisting,
    TransientUnpublished,
}

fn read_validation_frame(encoded: &mut &str) -> Result<String, MutationError> {
    let (length, rest) = encoded.split_once(':').ok_or(MutationError::Invalid)?;
    if length.len() > 5 || !length.bytes().all(|b| b.is_ascii_digit()) {
        return Err(MutationError::Invalid);
    }
    let length = length
        .parse::<usize>()
        .map_err(|_| MutationError::Invalid)?;
    if length > 1024 {
        return Err(MutationError::Invalid);
    }
    let field = rest.get(..length).ok_or(MutationError::Invalid)?.to_owned();
    *encoded = rest.get(length..).ok_or(MutationError::Invalid)?;
    Ok(field)
}

fn validation_view(mut encoded: &str) -> Result<ValidationView, MutationError> {
    let mut fields = Vec::new();
    for _ in 0..9 {
        let (size, rest) = encoded.split_once(':').ok_or(MutationError::Invalid)?;
        if size.len() > 5 || !size.bytes().all(|b| b.is_ascii_digit()) {
            return Err(MutationError::Invalid);
        }
        let size = size.parse::<usize>().map_err(|_| MutationError::Invalid)?;
        if size > 1024 {
            return Err(MutationError::Invalid);
        }
        fields.push(rest.get(..size).ok_or(MutationError::Invalid)?.to_owned());
        encoded = rest.get(size..).ok_or(MutationError::Invalid)?;
    }
    let [
        version,
        mode,
        platform,
        image_id,
        configuration_fingerprint,
        execution_fingerprint,
        rust_version,
        cargo_version,
        candidate_source_fingerprint,
    ]: [String; 9] = fields.try_into().map_err(|_| MutationError::Invalid)?;
    let mut resolution = None;
    let (method, mutation_execution_fingerprint) = match version.as_str() {
        "m2-manifest-lints-v1" | "m2-manifest-semantic-v1" if encoded.is_empty() => {
            (ValidationMethod::CargoMetadataFrozenNoDeps, None)
        }
        "m2-fmt-apply-v1" | "m2-fix-apply-v1" => {
            let (size, value) = encoded.split_once(':').ok_or(MutationError::Invalid)?;
            if size != "71" || value.len() != 71 {
                return Err(MutationError::Invalid);
            }
            value
                .parse::<SourceFingerprint>()
                .map_err(|_| MutationError::Invalid)?;
            (
                if version == "m2-fix-apply-v1" {
                    ValidationMethod::CargoFixThenCheck
                } else {
                    ValidationMethod::RustfmtThenFmtCheck
                },
                Some(value.to_owned()),
            )
        }
        "m2-manifest-resolved-v1" | "m2-dependency-add-v1" | "m2-dependency-remove-v1" => {
            let resolution_execution_fingerprint = read_validation_frame(&mut encoded)?;
            let dataset_fingerprint = read_validation_frame(&mut encoded)?;
            let resolved_lock_fingerprint = read_validation_frame(&mut encoded)?;
            for fingerprint in [
                &resolution_execution_fingerprint,
                &dataset_fingerprint,
                &resolved_lock_fingerprint,
            ] {
                fingerprint
                    .parse::<SourceFingerprint>()
                    .map_err(|_| MutationError::Invalid)?;
            }
            let lock_disposition = match read_validation_frame(&mut encoded)?.as_str() {
                "updated_existing" => LockDisposition::UpdatedExisting,
                "transient_unpublished" => LockDisposition::TransientUnpublished,
                _ => return Err(MutationError::Invalid),
            };
            let manifest_path = read_validation_frame(&mut encoded)?;
            if !encoded.is_empty()
                || rust_engineering_domain::validate_source_path(&manifest_path).is_err()
                || !(manifest_path == "Cargo.toml" || manifest_path.ends_with("/Cargo.toml"))
            {
                return Err(MutationError::Invalid);
            }
            resolution = Some(ResolutionView {
                resolution_execution_fingerprint,
                dataset_fingerprint,
                resolved_lock_fingerprint,
                lock_policy: LockPolicy::PreservePresence,
                lock_disposition,
                manifest_path,
            });
            (ValidationMethod::CargoMetadataOfflineThenFrozen, None)
        }
        _ => return Err(MutationError::Invalid),
    };
    if mode != "local_coordinated" {
        return Err(MutationError::Invalid);
    }
    for fingerprint in [
        &image_id,
        &configuration_fingerprint,
        &execution_fingerprint,
        &candidate_source_fingerprint,
    ] {
        fingerprint
            .parse::<SourceFingerprint>()
            .map_err(|_| MutationError::Invalid)?;
    }
    Ok(ValidationView {
        resolution,
        method,
        mutation_execution_fingerprint,
        semantics: SnapshotSemantics::LatestKnown,
        platform,
        image_id,
        configuration_fingerprint,
        execution_fingerprint,
        rust_version,
        cargo_version,
        candidate_source_fingerprint,
    })
}

/// The frozen M2 view of the validation of a plan or receipt of `kind`. An
/// analyzer action's provenance never takes this view, whatever its bytes
/// (V07 P3-4); the plan digest already binds kind and validation together.
fn m2_validation_view(kind: MutationKind, encoded: &str) -> Result<ValidationView, MutationError> {
    if kind == MutationKind::AnalyzerActionApply {
        return Err(MutationError::Invalid);
    }
    validation_view(encoded)
}

/// The analyzer view of the validation of a plan or receipt of `kind`: only
/// an `AnalyzerActionApply` one ever takes it (V07 P3-4).
fn analyzer_action_validation_for(
    kind: MutationKind,
    encoded: &str,
) -> Result<AnalyzerActionValidationView, MutationError> {
    if kind != MutationKind::AnalyzerActionApply {
        return Err(MutationError::Invalid);
    }
    analyzer_action_validation_view(encoded)
}

/// Decoded `m6-analyzer-action-v1` provenance (ADR-083 §6).
///
/// Deliberately not a [`ValidationMethod`] of [`ValidationView`]: that enum is
/// part of the five frozen M2 tool output schemas (`fmt-apply-tool.json` and
/// its siblings), so a new variant would change all five. [`validation_view`]
/// therefore keeps refusing this version, and `rust.analyzer.action.apply`
/// publishes this view from its own schema.
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AnalyzerActionValidationView {
    method: AnalyzerActionValidationMethod,
    semantics: SnapshotSemantics,
    platform: String,
    image_id: String,
    configuration_fingerprint: String,
    /// The rust-analyzer session request that resolved the action.
    session_execution_fingerprint: String,
    rust_version: String,
    cargo_version: String,
    /// The fresh capture the action was resolved against and applied to.
    analyzed_source_fingerprint: String,
    analyzer: AnalyzerProvenanceView,
    action_digest: String,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum AnalyzerActionValidationMethod {
    /// Structural `WorkspaceEdit` validation only: no `cargo check` ran, so
    /// the applied result is not compile-verified (owner decision A,
    /// 2026-09-12).
    WorkspaceEditStructuralOnly,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AnalyzerProvenanceView {
    version: String,
    binary_sha256: String,
    config_digest: String,
}

fn analyzer_action_validation_view(
    mut encoded: &str,
) -> Result<AnalyzerActionValidationView, MutationError> {
    let version = read_validation_frame(&mut encoded)?;
    let mode = read_validation_frame(&mut encoded)?;
    let platform = read_validation_frame(&mut encoded)?;
    let image_id = read_validation_frame(&mut encoded)?;
    let configuration_fingerprint = read_validation_frame(&mut encoded)?;
    let session_execution_fingerprint = read_validation_frame(&mut encoded)?;
    let rust_version = read_validation_frame(&mut encoded)?;
    let cargo_version = read_validation_frame(&mut encoded)?;
    let analyzed_source_fingerprint = read_validation_frame(&mut encoded)?;
    let analyzer_version = read_validation_frame(&mut encoded)?;
    let binary_sha256 = read_validation_frame(&mut encoded)?;
    let config_digest = read_validation_frame(&mut encoded)?;
    let action_digest = read_validation_frame(&mut encoded)?;
    if version != rust_engineering_application::analyzer::ANALYZER_ACTION_VALIDATION_VERSION
        || mode != "local_coordinated"
        || !encoded.is_empty()
        || analyzer_version.trim().is_empty()
    {
        return Err(MutationError::Invalid);
    }
    for fingerprint in [
        &image_id,
        &configuration_fingerprint,
        &session_execution_fingerprint,
        &analyzed_source_fingerprint,
        &binary_sha256,
        &config_digest,
        &action_digest,
    ] {
        fingerprint
            .parse::<SourceFingerprint>()
            .map_err(|_| MutationError::Invalid)?;
    }
    Ok(AnalyzerActionValidationView {
        method: AnalyzerActionValidationMethod::WorkspaceEditStructuralOnly,
        semantics: SnapshotSemantics::LatestKnown,
        platform,
        image_id,
        configuration_fingerprint,
        session_execution_fingerprint,
        rust_version,
        cargo_version,
        analyzed_source_fingerprint,
        analyzer: AnalyzerProvenanceView {
            version: analyzer_version,
            binary_sha256,
            config_digest,
        },
        action_digest,
    })
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Data {
    Preview {
        plan_id: String,
        plan_digest: String,
        expires_in_seconds: u64,
        files: Vec<Change>,
        diff: String,
        validation: ValidationView,
    },
    Receipt {
        operation_id: String,
        plan_digest: String,
        state: ReceiptState,
        validation: ValidationView,
        files: Vec<ReceiptChange>,
    },
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ReceiptState {
    Committed,
    NoChange,
    Aborted,
    RecoveryRequired,
}

#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Output {
    status: Status,
    error_code: Option<Reason>,
    error_message: Option<&'static str>,
    summary: &'static str,
    duration_ms: u64,
    data: Option<Data>,
    diagnostics: [(); 0],
    truncation: Truncation,
    evidence: MutationEvidence,
    concurrency_contract: Concurrency,
    guarantees_not_provided: [ExcludedGuarantee; 4],
}
impl ToolOutput for Output {
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
impl Output {
    fn new(
        status: Status,
        reason: Option<Reason>,
        data: Option<Data>,
        summary: &'static str,
        duration_ms: u64,
    ) -> Self {
        let evidence = match &data {
            Some(Data::Preview { plan_digest, .. } | Data::Receipt { plan_digest, .. }) => {
                MutationEvidence::MutationSnapshot {
                    plan_digest: plan_digest.clone(),
                    semantics: SnapshotSemantics::LatestKnown,
                    freshness: Freshness::Unknown,
                }
            }
            None => MutationEvidence::Local,
        };
        Self {
            status,
            error_code: reason,
            error_message: reason.map(|_| summary),
            summary,
            duration_ms,
            data,
            diagnostics: [],
            truncation: Truncation::default(),
            evidence,
            concurrency_contract: Concurrency::LocalCoordinated,
            guarantees_not_provided: [
                ExcludedGuarantee::OsExclusionOfExternalWriters,
                ExcludedGuarantee::MultiFileAtomicity,
                ExcludedGuarantee::MaliciousHostProtection,
                ExcludedGuarantee::DemonstratedPowerLossSurvival,
            ],
        }
    }
    fn failure(reason: Reason, duration: u64) -> Self {
        let status = match reason {
            Reason::UnsupportedPlatform
            | Reason::ToolchainUnavailable
            | Reason::OfflineDataMissing => Status::Unavailable,
            Reason::CandidateInvalid | Reason::CommandTimeout => Status::Failed,
            Reason::Cancelled => Status::Cancelled,
            _ => Status::Blocked,
        };
        Self::new(
            status,
            Some(reason),
            None,
            match reason {
                Reason::InvalidOperation => {
                    "Operation is outside this tool's supported mutation contract"
                }
                Reason::PermissionDenied => {
                    "A current project reference and exact host write grant for this operation are required"
                }
                Reason::Conflict => {
                    "Source or approval changed; reopen the project and request a new preview"
                }
                Reason::LockBusy => "Another operation is active; retry when it finishes",
                Reason::PlanExpired => "Preview expired; request a new preview and review its diff",
                Reason::NotFound => {
                    "Plan or journal was not found; reopen and use the exact operation ID/digest/key for durable replay, or create a new preview"
                }
                Reason::LimitExceeded => {
                    "Mutation exceeds a bounded plan, output or journal budget"
                }
                Reason::UnsupportedPlatform => {
                    "This writer requires the qualified macOS ARM64 APFS adapter"
                }
                Reason::Io => {
                    "Mutation I/O failed; consult the original operation receipt before retrying a commit"
                }
                Reason::RecoveryRequired => {
                    "Preserve journal and mutation temporaries; reopen and request receipt with recover=true"
                }
                Reason::ToolchainUnavailable => {
                    "Configure the approved offline Cargo runtime before preview"
                }
                Reason::CandidateInvalid => {
                    "The isolated tool or postcondition check rejected the candidate; source was not changed"
                }
                Reason::Cancelled => "Operation cancelled before a successful receipt was returned",
                Reason::OfflineDataMissing => {
                    "Configure or update a host-approved Cargo vendor dataset for offline dependency resolution"
                }
                Reason::OfflineDataInvalid => {
                    "The configured Cargo vendor data could not be verified against the host fingerprint"
                }
                Reason::CommandTimeout => {
                    "The isolated operation exceeded its time budget; no candidate was approved"
                }
            },
            duration,
        )
    }
}

struct Provider {
    config: Option<WriteConfig>,
    kind: MutationKind,
    store: Option<NativeMutationStore>,
}
impl Provider {
    fn store(&mut self) -> Result<(), MutationError> {
        let config = self
            .config
            .as_ref()
            .ok_or(MutationError::PermissionDenied)?;
        if self.store.is_none() {
            let path = prepare_mutation_state(&config.state_parent)?;
            self.store = Some(NativeMutationStore::open_for_kind(
                &path,
                &config.roots,
                self.kind,
            )?);
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct SharedPlans {
    plans: MutationPlans,
    clock: MonotonicClock,
}

/// What the fixed audit event reads from a write tool's output, and what a
/// preview whose response was lost is recorded as. Shared by the five M2
/// tools and `rust.analyzer.action.apply`, whose outputs differ.
trait AuditedOutput: Clone + Send + 'static {
    fn event(&self) -> audit::Record<'_>;
    /// A preview the peer never received is a cancelled call without a
    /// produced ID; any other output is kept.
    fn response_lost(self) -> Self;
}

impl AuditedOutput for Output {
    fn event(&self) -> audit::Record<'_> {
        audit::record(self)
    }

    fn response_lost(self) -> Self {
        if matches!(self.data, Some(Data::Preview { .. })) {
            Output::failure(Reason::Cancelled, self.duration_ms)
        } else {
            self
        }
    }
}

#[derive(Clone)]
struct AuditRecord<O> {
    admitted: bool,
    cleanup_uncertain: bool,
    output: O,
    allocation: Option<MutationAllocationStats>,
}

struct CallAuditState<O> {
    tool: &'static str,
    phase: audit::Phase,
    dispatcher: tracing::Dispatch,
    waiter_dropped: AtomicBool,
    emitted: AtomicBool,
    worker_record: Mutex<Option<AuditRecord<O>>>,
}

impl<O: AuditedOutput> CallAuditState<O> {
    fn new(tool: &'static str, phase: audit::Phase) -> Self {
        Self {
            tool,
            phase,
            dispatcher: tracing::dispatcher::get_default(Clone::clone),
            waiter_dropped: AtomicBool::new(false),
            emitted: AtomicBool::new(false),
            worker_record: Mutex::new(None),
        }
    }

    fn worker_completed(&self, record: AuditRecord<O>) {
        let record_for_fallback = Self::response_lost(record.clone());
        match self.worker_record.lock() {
            Ok(mut slot) => *slot = Some(record),
            Err(poisoned) => *poisoned.into_inner() = Some(record),
        }
        if self.waiter_dropped.load(Ordering::Acquire) {
            self.emit_once(&record_for_fallback);
        }
    }

    fn waiter_completed(&self, record: AuditRecord<O>) {
        self.emit_once(&record);
    }

    fn worker_cleanup_uncertain(&self) -> bool {
        match self.worker_record.lock() {
            Ok(slot) => slot.as_ref().is_some_and(|record| record.cleanup_uncertain),
            Err(poisoned) => poisoned
                .into_inner()
                .as_ref()
                .is_some_and(|record| record.cleanup_uncertain),
        }
    }

    fn waiter_dropped(&self) {
        self.waiter_dropped.store(true, Ordering::Release);
        let record = match self.worker_record.lock() {
            Ok(slot) => slot.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(record) = record {
            self.emit_once(&Self::response_lost(record));
        }
    }

    fn response_lost(mut record: AuditRecord<O>) -> AuditRecord<O> {
        record.output = record.output.response_lost();
        record
    }

    fn emit_once(&self, record: &AuditRecord<O>) {
        if self
            .emitted
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        tracing::dispatcher::with_default(&self.dispatcher, || {
            audit::emit(
                self.tool,
                self.phase,
                record.admitted,
                record.cleanup_uncertain,
                record.output.event(),
                record.allocation,
            );
        });
    }
}

struct CallAuditWaiter<O: AuditedOutput>(Arc<CallAuditState<O>>);

impl<O: AuditedOutput> Drop for CallAuditWaiter<O> {
    fn drop(&mut self) {
        self.0.waiter_dropped();
    }
}

pub(super) type ManifestMutationTool = MutationTool<Input>;
pub(super) type FormatMutationTool = MutationTool<FormatInput>;
pub(super) type FixMutationTool = MutationTool<FixInput>;
pub(super) type DependencyAddTool = MutationTool<DependencyAddInput>;
pub(super) type DependencyRemoveTool = MutationTool<DependencyRemoveInput>;

pub(super) struct MutationTool<I> {
    pub definition: Tool,
    contract: Contract<I, Output>,
    registry: Arc<Mutex<Registry>>,
    provider: Arc<Mutex<Provider>>,
    workers: Workers,
    inspector: Arc<RustProjectInspector>,
    ready: Arc<AtomicBool>,
    plans: Arc<Mutex<SharedPlans>>,
}
impl<I: MutationInput> MutationTool<I> {
    pub(super) fn definition() -> Result<(Contract<I, Output>, Tool), ErrorData> {
        let contract = Contract::<I, Output>::new()?;
        let definition = Tool::new(I::NAME, I::DESCRIPTION, (*contract.input_schema).clone())
            .with_raw_output_schema(Arc::clone(&contract.output_schema))
            .with_annotations(
                ToolAnnotations::new()
                    .read_only(false)
                    .destructive(true)
                    .idempotent(false)
                    .open_world(false),
            );
        Ok((contract, definition))
    }

    pub fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
        config: Option<WriteConfig>,
        plans: Arc<Mutex<SharedPlans>>,
    ) -> Result<Self, ErrorData> {
        let (contract, definition) = Self::definition()?;
        Ok(Self {
            definition,
            contract,
            registry,
            workers,
            inspector,
            ready,
            plans,
            provider: Arc::new(Mutex::new(Provider {
                config,
                kind: I::KIND,
                store: None,
            })),
        })
    }

    pub async fn call(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let input = self.contract.decode(request.arguments)?.into_request()?;
        let started = Instant::now();
        let phase = audit::phase(&input.action);
        if !self.ready.load(Ordering::Acquire) {
            let output = Output::failure(Reason::PermissionDenied, 0);
            audit::emit(
                I::NAME,
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
        let audit_state = Arc::new(CallAuditState::new(I::NAME, phase));
        let _audit_waiter = CallAuditWaiter(Arc::clone(&audit_state));
        let worker_audit = Arc::clone(&audit_state);
        let retention = PreviewRetention::default();
        let preview_token = retention.token();
        let preview_contract = Contract::<I, Output>::new()?;
        let result = self
            .workers
            .run_joined(context.ct, started + DEADLINE, move |control| {
                let cleanup_uncertain = AtomicBool::new(false);
                let result = (|| {
                    let mut provider = provider.try_lock().map_err(lock_reason)?;
                    provider.store().map_err(reason)?;
                    let vendor_config = provider
                        .config
                        .as_ref()
                        .and_then(|config| config.vendor.clone());
                    let Provider { store, .. } = &mut *provider;
                    let store = store.as_ref().ok_or(Reason::PermissionDenied)?;
                    match input.action {
                        action @ (Action::Preview { .. }
                        | Action::FormatPreview { .. }
                        | Action::SemanticPreview { .. }) => {
                            let (workspace, candidate) = match action {
                                action @ (Action::Preview { .. }
                                | Action::SemanticPreview { .. }) => {
                                    let (expected_project_fingerprint, target_manifest, edit) =
                                        match action {
                                            Action::Preview {
                                                expected_project_fingerprint,
                                                edit,
                                            } => (
                                                expected_project_fingerprint,
                                                "Cargo.toml".to_owned(),
                                                edit.into_domain().map_err(reason)?,
                                            ),
                                            Action::SemanticPreview {
                                                expected_project_fingerprint,
                                                target_manifest,
                                                edit,
                                            } => (
                                                expected_project_fingerprint,
                                                target_manifest,
                                                edit,
                                            ),
                                            _ => return Err(Reason::InvalidOperation),
                                        };
                                    let prepared = registry
                                        .try_lock()
                                        .map_err(lock_reason)?
                                        .prepare_semantic(
                                            &input.project_ref,
                                            &expected_project_fingerprint,
                                            &target_manifest,
                                            I::KIND,
                                            &edit,
                                            store,
                                            control,
                                        )
                                        .map_err(|error| {
                                            observed_semantic_reason(error, &cleanup_uncertain)
                                        })?;
                                    let dataset = if matches!(
                                        edit,
                                        ManifestEdit::LintSet { .. }
                                            | ManifestEdit::LintRemove { .. }
                                            | ManifestEdit::ProfileSet { .. }
                                            | ManifestEdit::ProfileRemove { .. }
                                    ) {
                                        None
                                    } else {
                                        vendor_config
                                            .as_ref()
                                            .map(|config| {
                                                rust_engineering_project::capture_with_expected(
                                                    &config.directory,
                                                    &config.fingerprint,
                                                    control,
                                                )
                                            })
                                            .transpose()
                                            .map_err(|error| match error {
                                                ProjectError::Cancelled => Reason::Cancelled,
                                                ProjectError::Internal => Reason::Io,
                                                _ => Reason::OfflineDataInvalid,
                                            })?
                                    };
                                    prepared
                                        .validate(
                                            &TomlManifestEditor,
                                            inspector.as_ref(),
                                            inspector.as_ref(),
                                            dataset.as_ref(),
                                            control,
                                        )
                                        .map_err(|error| {
                                            observed_semantic_reason(error, &cleanup_uncertain)
                                        })?
                                }
                                Action::FormatPreview {
                                    expected_project_fingerprint,
                                } => {
                                    let prepared = registry
                                        .try_lock()
                                        .map_err(lock_reason)?
                                        .prepare_format(
                                            &input.project_ref,
                                            &expected_project_fingerprint,
                                            store,
                                            control,
                                        )
                                        .map_err(|error| {
                                            observed_preparation_reason(error, &cleanup_uncertain)
                                        })?;
                                    let command = match I::KIND {
                                        MutationKind::FormatApply => {
                                            rust_engineering_domain::RustMutationCommand::Format
                                        }
                                        MutationKind::FixApply => {
                                            rust_engineering_domain::RustMutationCommand::Fix
                                        }
                                        _ => return Err(Reason::InvalidOperation),
                                    };
                                    prepared
                                        .validate_command(command, inspector.as_ref(), control)
                                        .map_err(|error| {
                                            observed_preparation_reason(error, &cleanup_uncertain)
                                        })?
                                }
                                _ => return Err(Reason::InvalidOperation),
                            };
                            registry
                                .try_lock()
                                .map_err(lock_reason)?
                                .finish_manifest_preview(&input.project_ref, &candidate, control)
                                .map_err(|error| {
                                    observed_preparation_reason(error, &cleanup_uncertain)
                                })?;
                            let (files, diff) = preview_diff(&candidate).map_err(reason)?;
                            let digest = mutation_digest(&candidate).map_err(reason)?;
                            let reference = OsReferences.generate().map_err(|_| Reason::Io)?;
                            let suffix =
                                reference.as_str().strip_prefix("prj_").ok_or(Reason::Io)?;
                            let id = MutationId::new(format!("mut_{suffix}")).map_err(reason)?;
                            let data = Data::Preview {
                                plan_id: id.as_str().into(),
                                plan_digest: digest.to_string(),
                                expires_in_seconds: MutationPlans::TTL_SECONDS,
                                files,
                                diff,
                                validation: m2_validation_view(
                                    candidate.kind,
                                    &candidate.validation,
                                )
                                .map_err(reason)?,
                            };
                            // Bound the complete MCP encoding, including duplicated text and JSON,
                            // before retaining a plan whose exact diff the peer cannot receive.
                            validate_preview_size(&preview_contract, &data)?;
                            let mut shared = worker_plans.try_lock().map_err(lock_reason)?;
                            let SharedPlans { plans, clock } = &mut *shared;
                            plans
                                .remember_revocable(
                                    id,
                                    digest,
                                    workspace,
                                    candidate,
                                    clock,
                                    preview_token,
                                )
                                .map_err(reason)?;
                            Ok(data)
                        }
                        Action::Commit {
                            plan_id,
                            plan_digest,
                            idempotency_key,
                        } => {
                            let id = MutationId::new(plan_id).map_err(reason)?;
                            let key = IdempotencyKey::new(idempotency_key).map_err(reason)?;
                            let shared = worker_plans.try_lock().map_err(lock_reason)?;
                            let resolved =
                                shared
                                    .plans
                                    .resolve(&id, &plan_digest, key.clone(), &shared.clock);
                            drop(shared);
                            let receipt = match resolved {
                                Ok(plan) => {
                                    if plan.request.candidate.kind != I::KIND {
                                        return Err(Reason::PermissionDenied);
                                    }
                                    let receipt = registry
                                        .try_lock()
                                        .map_err(lock_reason)?
                                        .commit_mutation(
                                            &input.project_ref,
                                            &plan.workspace_root,
                                            &plan.request,
                                            store,
                                            control,
                                        )
                                        .map_err(reason)?;
                                    plan.retire_if_terminal(&receipt);
                                    receipt
                                }
                                Err(
                                    missing @ (MutationError::NotFound | MutationError::Expired),
                                ) => registry
                                    .try_lock()
                                    .map_err(lock_reason)?
                                    .replay_mutation(
                                        &input.project_ref,
                                        &id,
                                        &plan_digest,
                                        &key,
                                        store,
                                        control,
                                    )
                                    .map_err(|error| {
                                        reason(if error == MutationError::NotFound {
                                            missing
                                        } else {
                                            error
                                        })
                                    })?,
                                Err(error) => return Err(reason(error)),
                            };
                            receipt_data(receipt, I::KIND).map_err(reason)
                        }
                        Action::Receipt {
                            operation_id,
                            recover,
                        } => {
                            let id = MutationId::new(operation_id).map_err(reason)?;
                            let receipt = registry
                                .try_lock()
                                .map_err(lock_reason)?
                                .mutation_receipt(&input.project_ref, &id, recover, store, control)
                                .map_err(reason)?;
                            receipt_data(receipt, I::KIND).map_err(reason)
                        }
                    }
                })();
                let duration = elapsed_millis(started);
                let output = joined_output(
                    result.clone(),
                    rust_engineering_application::OperationControl::check(control).is_err(),
                    duration,
                );
                let admitted = !matches!(output.error_code, Some(Reason::PermissionDenied));
                worker_audit.worker_completed(AuditRecord {
                    admitted,
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
            Err(error) => Output::failure(
                match error {
                    WorkerError::Busy => Reason::LockBusy,
                    WorkerError::Cancelled => Reason::Cancelled,
                    WorkerError::TimedOut => Reason::CommandTimeout,
                    WorkerError::Internal => Reason::Io,
                },
                duration,
            ),
        };
        let retain_preview = matches!(&output.data, Some(Data::Preview { .. }));
        let encoded = self.contract.encode(output.clone())?;
        if serde_json::to_vec(&encoded)
            .map_err(|_| ErrorData::internal_error("Mutation encoding failed", None))?
            .len()
            > MAX_RESULT
        {
            let output = Output::failure(Reason::LimitExceeded, duration);
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
        let admitted =
            entered_worker && !matches!(output.error_code, Some(Reason::PermissionDenied));
        audit_state.waiter_completed(AuditRecord {
            admitted,
            cleanup_uncertain: audit_state.worker_cleanup_uncertain(),
            output,
            allocation: allocation_stats(&plans),
        });
        Ok(encoded)
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

fn allocation_stats(plans: &Arc<Mutex<SharedPlans>>) -> Option<MutationAllocationStats> {
    plans
        .try_lock()
        .ok()
        .map(|shared| shared.plans.allocation_stats())
}

fn joined_output(result: Result<Data, Reason>, interrupted: bool, duration: u64) -> Output {
    // Native commit finishes its journal after its irreversible point. Do not
    // relabel a durable receipt as cancelled after losing the response race.
    match result {
        Ok(
            data @ Data::Receipt {
                state: ReceiptState::RecoveryRequired,
                ..
            },
        ) => Output::new(
            Status::Blocked,
            Some(Reason::RecoveryRequired),
            Some(data),
            "Publication needs recovery; preserve the journal and avoid concurrent edits",
            duration,
        ),
        Ok(
            data @ Data::Receipt {
                state: ReceiptState::Aborted,
                ..
            },
        ) => Output::new(
            Status::Blocked,
            Some(Reason::Conflict),
            Some(data),
            "Operation aborted without applying the candidate; request a new preview",
            duration,
        ),
        Ok(data @ Data::Receipt { .. }) => Output::new(
            Status::Passed,
            None,
            Some(data),
            "Durable mutation receipt",
            duration,
        ),
        Ok(data) if !interrupted => preview_output(data, duration),
        Ok(_) => Output::failure(Reason::Cancelled, duration),
        Err(error) => Output::failure(error, duration),
    }
}

fn is_cleanup_uncertain(error: &InspectionError) -> bool {
    matches!(
        error,
        InspectionError::Execution(ExecutionError::CleanupUncertain)
    )
}

fn observed_preparation_reason(
    error: MutationPreparationError,
    cleanup_uncertain: &AtomicBool,
) -> Reason {
    if matches!(&error, MutationPreparationError::Inspection(error) if is_cleanup_uncertain(error))
    {
        cleanup_uncertain.store(true, Ordering::Release);
    }
    preparation_reason(error)
}

fn observed_semantic_reason(
    error: rust_engineering_application::SemanticPreparationError,
    cleanup_uncertain: &AtomicBool,
) -> Reason {
    use rust_engineering_application::{ResolutionError, SemanticPreparationError as E};
    if matches!(
        &error,
        E::Inspection(error) | E::Resolution(ResolutionError::Inspection(error))
            if is_cleanup_uncertain(error)
    ) {
        cleanup_uncertain.store(true, Ordering::Release);
    }
    semantic_reason(error)
}

fn reason(error: MutationError) -> Reason {
    match error {
        MutationError::Invalid => Reason::InvalidOperation,
        MutationError::PermissionDenied => Reason::PermissionDenied,
        MutationError::Conflict => Reason::Conflict,
        MutationError::Busy => Reason::LockBusy,
        MutationError::Expired => Reason::PlanExpired,
        MutationError::NotFound => Reason::NotFound,
        MutationError::LimitExceeded => Reason::LimitExceeded,
        MutationError::UnsupportedPlatform => Reason::UnsupportedPlatform,
        MutationError::Cancelled => Reason::Cancelled,
        MutationError::Io => Reason::Io,
        MutationError::RecoveryRequired => Reason::RecoveryRequired,
    }
}
fn preparation_reason(error: MutationPreparationError) -> Reason {
    match error {
        MutationPreparationError::Mutation(error) => reason(error),
        MutationPreparationError::Edit(_) => Reason::InvalidOperation,
        MutationPreparationError::Project(ProjectError::Cancelled) => Reason::Cancelled,
        MutationPreparationError::Project(ProjectError::Internal) => Reason::Io,
        MutationPreparationError::Project(ProjectError::Rejected(
            rust_engineering_domain::OperationalErrorCode::InvalidProject,
        )) => Reason::Conflict,
        MutationPreparationError::Project(_) => Reason::PermissionDenied,
        MutationPreparationError::Inspection(InspectionError::Execution(
            ExecutionError::Denied
            | ExecutionError::Unavailable
            | ExecutionError::InvalidConfiguration,
        )) => Reason::ToolchainUnavailable,
        MutationPreparationError::Inspection(
            InspectionError::Project(ProjectError::Cancelled)
            | InspectionError::Execution(ExecutionError::Cancelled),
        ) => Reason::Cancelled,
        MutationPreparationError::Inspection(InspectionError::Project(ProjectError::Rejected(
            rust_engineering_domain::OperationalErrorCode::InvalidProject,
        ))) => Reason::CandidateInvalid,
        MutationPreparationError::Inspection(InspectionError::Project(ProjectError::Rejected(
            rust_engineering_domain::OperationalErrorCode::CommandTimeout,
        ))) => Reason::CommandTimeout,
        MutationPreparationError::Inspection(InspectionError::OutputLimit) => Reason::LimitExceeded,
        MutationPreparationError::Inspection(_) => Reason::Io,
    }
}

fn semantic_reason(error: rust_engineering_application::SemanticPreparationError) -> Reason {
    use rust_engineering_application::{ResolutionError, SemanticPreparationError as E};
    match error {
        E::Mutation(error) => reason(error),
        E::Edit(rust_engineering_domain::ManifestEditError::Conflict) => Reason::Conflict,
        E::Edit(rust_engineering_domain::ManifestEditError::LimitExceeded) => Reason::LimitExceeded,
        E::Edit(_) => Reason::InvalidOperation,
        E::Project(error) => preparation_reason(MutationPreparationError::Project(error)),
        E::Inspection(error) | E::Resolution(ResolutionError::Inspection(error)) => {
            preparation_reason(MutationPreparationError::Inspection(error))
        }
        E::Resolution(ResolutionError::MissingOfflineData) => Reason::OfflineDataMissing,
        E::Resolution(ResolutionError::InvalidOfflineData) => Reason::OfflineDataInvalid,
        E::Resolution(ResolutionError::Failed) => Reason::CandidateInvalid,
    }
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ReceiptChange {
    path: String,
    before_sha256: String,
    intended_after_sha256: String,
    before_bytes: u64,
    intended_after_bytes: u64,
    /// Effect recorded at the terminal journal phase, not current live state.
    effect_after_sha256: Option<String>,
    effect_after_bytes: Option<u64>,
}

fn lock_reason<T>(error: TryLockError<T>) -> Reason {
    match error {
        TryLockError::WouldBlock => Reason::LockBusy,
        TryLockError::Poisoned(_) => Reason::Io,
    }
}
fn validate_preview_size<I: serde::de::DeserializeOwned + JsonSchema>(
    contract: &Contract<I, Output>,
    data: &Data,
) -> Result<(), Reason> {
    let complete = contract
        .encode(preview_output(data.clone(), u64::MAX))
        .map_err(|_| Reason::Io)?;
    if serde_json::to_vec(&complete).map_err(|_| Reason::Io)?.len() > MAX_RESULT {
        return Err(Reason::LimitExceeded);
    }
    Ok(())
}
fn preview_output(data: Data, duration: u64) -> Output {
    Output::new(
        Status::Passed,
        None,
        Some(data),
        "Review this exact diff before commit; source has not been changed",
        duration,
    )
}

/// `kind` is the writer kind the receipt was read through: the store is
/// opened for exactly one kind, so it is the receipt's own.
fn receipt_data(receipt: MutationReceipt, kind: MutationKind) -> Result<Data, MutationError> {
    Ok(Data::Receipt {
        operation_id: receipt.id.as_str().into(),
        validation: m2_validation_view(kind, &receipt.validation)?,
        plan_digest: receipt.digest.to_string(),
        state: receipt_state(receipt.state),
        files: receipt_changes(receipt.files),
    })
}

fn receipt_state(state: MutationState) -> ReceiptState {
    match state {
        MutationState::Committed => ReceiptState::Committed,
        MutationState::NoChange => ReceiptState::NoChange,
        MutationState::Aborted => ReceiptState::Aborted,
        MutationState::RecoveryRequired => ReceiptState::RecoveryRequired,
    }
}

fn receipt_changes(files: Vec<MutationFileReceipt>) -> Vec<ReceiptChange> {
    files
        .into_iter()
        .map(|file| ReceiptChange {
            path: file.path,
            before_sha256: file.before.to_string(),
            intended_after_sha256: file.after.to_string(),
            before_bytes: file.before_bytes,
            intended_after_bytes: file.after_bytes,
            effect_after_sha256: file.effect_after.map(|hash| hash.to_string()),
            effect_after_bytes: file.effect_after_bytes,
        })
        .collect()
}
fn preview_diff(candidate: &MutationCandidate) -> Result<(Vec<Change>, String), MutationError> {
    let mut changes = Vec::new();
    let mut diff = String::new();
    for before in candidate.before.files() {
        let after = candidate
            .after
            .files()
            .iter()
            .find(|file| file.path() == before.path())
            .ok_or(MutationError::Invalid)?;
        if before.bytes() == after.bytes() {
            continue;
        }
        if !match candidate.kind {
            MutationKind::ManifestPatch => matches!(before.path(), "Cargo.toml" | "Cargo.lock"),
            MutationKind::FormatApply
            | MutationKind::FixApply
            | MutationKind::AnalyzerActionApply => before.path().ends_with(".rs"),
            MutationKind::DependencyAdd | MutationKind::DependencyRemove => {
                matches!(before.path(), "Cargo.toml" | "Cargo.lock")
                    || before.path().ends_with("/Cargo.toml")
            }
        } {
            return Err(MutationError::PermissionDenied);
        }
        changes.push(Change {
            path: before.path().into(),
            before_sha256: mutation_bytes_digest(before.bytes())?.to_string(),
            after_sha256: mutation_bytes_digest(after.bytes())?.to_string(),
            before_bytes: before.bytes().len() as u64,
            after_bytes: after.bytes().len() as u64,
        });
        use std::fmt::Write;
        writeln!(diff, "--- a/{}\n+++ b/{}", before.path(), after.path())
            .map_err(|_| MutationError::Io)?;
        let lines = |bytes: &[u8]| {
            bytes.iter().filter(|b| **b == b'\n').count()
                + usize::from(!bytes.is_empty() && bytes.last() != Some(&b'\n'))
        };
        writeln!(
            diff,
            "@@ -1,{} +1,{} @@",
            lines(before.bytes()),
            lines(after.bytes())
        )
        .map_err(|_| MutationError::Io)?;
        for (prefix, bytes) in [("-", before.bytes()), ("+", after.bytes())] {
            let text = std::str::from_utf8(bytes).map_err(|_| MutationError::Invalid)?;
            for line in text.split_inclusive('\n') {
                diff.push_str(prefix);
                diff.push_str(line);
                if !line.ends_with('\n') {
                    diff.push_str("\n\\ No newline at end of file\n");
                }
                if diff.len() > 128 * 1024 {
                    return Err(MutationError::LimitExceeded);
                }
            }
        }
    }
    Ok((changes, diff))
}

#[cfg(test)]
mod tests {
    // Fixed test fixtures only; production keeps expect_used denied.
    #![allow(clippy::expect_used)]

    use super::*;
    use rust_engineering_domain::{SourceBundle, SourceFile};
    use serde_json::json;

    fn framed(fields: &[&str]) -> String {
        fields
            .iter()
            .map(|field| format!("{}:{field}", field.len()))
            .collect()
    }

    fn test_candidate() -> MutationCandidate {
        let bundle = |contents: &[u8]| {
            SourceBundle::new(vec![
                SourceFile::new("src/lib.rs".into(), contents.to_vec()).expect("test source"),
            ])
            .expect("test bundle")
        };
        MutationCandidate {
            kind: MutationKind::FormatApply,
            before: bundle(b"pub fn answer( )->u8 { 42 }\n"),
            after: bundle(b"pub fn answer() -> u8 {\n    42\n}\n"),
            validation: String::new(),
        }
    }

    fn test_mutation_id(number: u128) -> MutationId {
        MutationId::new(format!("mut_{number:032x}")).expect("test mutation id")
    }

    fn test_fingerprint(number: u8) -> SourceFingerprint {
        format!("sha256:{number:064x}")
            .parse()
            .expect("test fingerprint")
    }

    fn candidate(kind: MutationKind, path: &str, before: &[u8], after: &[u8]) -> MutationCandidate {
        let bundle = |contents: &[u8]| {
            SourceBundle::new(vec![
                SourceFile::new(path.into(), contents.to_vec()).expect("test source"),
            ])
            .expect("test bundle")
        };
        MutationCandidate {
            kind,
            before: bundle(before),
            after: bundle(after),
            validation: String::new(),
        }
    }

    fn reason_name(reason: Reason) -> String {
        serde_json::to_value(reason)
            .expect("serializable reason")
            .as_str()
            .expect("string reason")
            .to_owned()
    }

    fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
    }

    #[test]
    fn nested_contract_rejects_scope_escalation_and_arbitrary_edits()
    -> Result<(), Box<dyn std::error::Error>> {
        let contract = Contract::<Input, Output>::new()?;
        let base = json!({"project_ref":"prj_0123456789abcdef0123456789abcdef", "action":{
            "mode":"preview","expected_project_fingerprint":format!("sha256:{}", "a".repeat(64)),
            "edit":{"operation":"lint_set","scope":"package","tool":"rust","name":"unsafe_code","level":"deny","priority":null}
        }});
        assert!(
            contract
                .decode(Some(serde_json::from_value(base.clone())?))
                .is_ok()
        );
        let mut invalid = Vec::new();
        let mut value = base.clone();
        value["write_root"] = json!("/tmp");
        invalid.push(value);
        let mut value = base.clone();
        value["action"]["confirm"] = json!(true);
        invalid.push(value);
        let mut value = base.clone();
        value["action"]["edit"]["path"] = json!("package.build");
        invalid.push(value);
        let mut value = base.clone();
        value["action"]["edit"]["operation"] = json!("json_patch");
        invalid.push(value);
        let mut value = base.clone();
        value["action"]["edit"]["name"] = json!("unsafe_code\n[package]");
        invalid.push(value);
        let mut value = base;
        value["action"]["edit"]["tool"] = json!("cargo");
        invalid.push(value);
        for value in invalid {
            let error = contract
                .decode(Some(serde_json::from_value(value)?))
                .err()
                .ok_or("accepted invalid input")?;
            assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
            assert_eq!(error.message, "Invalid tool arguments");
        }
        Ok(())
    }

    #[test]
    fn preview_budget_counts_escaped_duplicate_mcp_content_before_plan_retention()
    -> Result<(), Box<dyn std::error::Error>> {
        let contract = Contract::<Input, Output>::new()?;
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let data = |diff: String| Data::Preview {
            plan_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: fingerprint.clone(),
            expires_in_seconds: 600,
            files: vec![],
            diff,
            validation: ValidationView {
                resolution: None,
                mutation_execution_fingerprint: None,
                method: ValidationMethod::CargoMetadataFrozenNoDeps,
                semantics: SnapshotSemantics::LatestKnown,
                platform: "linux/arm64".into(),
                image_id: fingerprint.clone(),
                configuration_fingerprint: fingerprint.clone(),
                execution_fingerprint: fingerprint.clone(),
                rust_version: "1.98.1".into(),
                cargo_version: "1.98.1".into(),
                candidate_source_fingerprint: fingerprint.clone(),
            },
        };
        assert!(validate_preview_size(&contract, &data("simple diff".into())).is_ok());
        // This is within the raw diff cap; JSON escaping and duplicated MCP text exceed the full cap.
        assert!(matches!(
            validate_preview_size(&contract, &data("\\".repeat(128 * 1024))),
            Err(Reason::LimitExceeded)
        ));
        Ok(())
    }

    #[test]
    fn late_worker_cancellation_revokes_the_undelivered_plan_before_next_admission()
    -> Result<(), Box<dyn std::error::Error>> {
        runtime()?.block_on(async {
            let workers = Workers::new();
            let request = tokio_util::sync::CancellationToken::new();
            let plans = Arc::new(Mutex::new(SharedPlans::default()));
            let retention = PreviewRetention::default();
            let token = retention.token();
            let id = test_mutation_id(1);
            let digest = test_fingerprint(1);
            let task_workers = workers.clone();
            let task_request = request.clone();
            let task_plans = Arc::clone(&plans);
            let task_id = id.clone();
            let task_digest = digest.clone();
            let (remembered_tx, remembered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::sync_channel(1);
            let task = tokio::spawn(async move {
                task_workers
                    .run_joined(
                        task_request,
                        Instant::now() + Duration::from_secs(5),
                        move |_| {
                            let mut shared = task_plans.lock().map_err(|_| Reason::Io)?;
                            let SharedPlans { plans, clock } = &mut *shared;
                            plans
                                .remember_revocable(
                                    task_id,
                                    task_digest,
                                    "/workspace".into(),
                                    test_candidate(),
                                    clock,
                                    token,
                                )
                                .map_err(reason)?;
                            drop(shared);
                            remembered_tx.send(()).map_err(|_| Reason::Io)?;
                            release_rx
                                .recv_timeout(Duration::from_secs(5))
                                .map_err(|_| Reason::Io)?;
                            Ok::<_, Reason>(())
                        },
                    )
                    .await
            });

            tokio::time::timeout(Duration::from_secs(5), remembered_rx).await??;
            request.cancel();
            release_tx.send(())?;
            let joined = task.await?.map_err(|_| "worker rejected")?;
            assert!(joined.result.is_ok());
            assert_eq!(joined.interrupted, Some(WorkerError::Cancelled));

            // This is the observable MCP outcome for a completed preview whose
            // cancellation is noticed only after the blocking closure returns.
            let contract = Contract::<FormatInput, Output>::new()?;
            let response =
                serde_json::to_value(contract.encode(Output::failure(Reason::Cancelled, 1))?)?;
            assert_eq!(response["structuredContent"]["status"], "cancelled");
            assert!(response["structuredContent"]["data"].is_null());

            // The response did not carry the preview, so dropping its delivery
            // guard must make the remembered plan unreachable and reclaimable.
            drop(retention);
            {
                let mut shared = plans.lock().map_err(|_| "plans poisoned")?;
                let SharedPlans { plans, clock } = &mut *shared;
                assert!(matches!(
                    plans.resolve(
                        &id,
                        &digest,
                        IdempotencyKey::new("cancelled-preview".into()).expect("idempotency key"),
                        clock,
                    ),
                    Err(MutationError::NotFound)
                ));
                for number in 2..=5 {
                    plans
                        .remember(
                            test_mutation_id(number),
                            test_fingerprint(number as u8),
                            "/workspace".into(),
                            test_candidate(),
                            clock,
                        )
                        .expect("revoked plan is pruned before all four slots are admitted");
                }
            }
            assert!(workers.shutdown(Duration::from_secs(1)).await);
            Ok(())
        })
    }

    #[test]
    fn format_provenance_requires_exact_version_and_terminal_fingerprint_frame() {
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let base = [
            "m2-fmt-apply-v1",
            "local_coordinated",
            "linux/arm64",
            fingerprint.as_str(),
            fingerprint.as_str(),
            fingerprint.as_str(),
            "1.98.1",
            "1.98.1",
            fingerprint.as_str(),
        ];
        let valid = format!("{}71:{fingerprint}", framed(&base));
        assert!(validation_view(&valid).is_ok());

        let mut wrong_version = base;
        wrong_version[0] = "m2-fmt-apply-v2";
        let malformed = [
            framed(&base),
            format!("{}70:{fingerprint}", framed(&base)),
            format!("{}71:{}", framed(&base), &fingerprint[..70]),
            format!("{}71:{fingerprint}x", framed(&base)),
            format!("{}71:sha256:{}", framed(&base), "g".repeat(64)),
            format!("{}71:{fingerprint}", framed(&wrong_version)),
        ];
        for encoded in malformed {
            assert!(matches!(
                validation_view(&encoded),
                Err(MutationError::Invalid)
            ));
        }
    }

    #[test]
    fn resolved_provenance_requires_exact_frames_and_preserve_presence_disposition() {
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        for version in [
            "m2-manifest-resolved-v1",
            "m2-dependency-add-v1",
            "m2-dependency-remove-v1",
        ] {
            for disposition in ["updated_existing", "transient_unpublished"] {
                let fields = [
                    version,
                    "local_coordinated",
                    "linux/arm64",
                    &fingerprint,
                    &fingerprint,
                    &fingerprint,
                    "1.98.1",
                    "1.98.1",
                    &fingerprint,
                    &fingerprint,
                    &fingerprint,
                    &fingerprint,
                    disposition,
                    "member/Cargo.toml",
                ];
                let valid = framed(&fields);
                let view = validation_view(&valid).expect("valid resolved provenance");
                assert!(view.resolution.is_some());
                assert!(view.mutation_execution_fingerprint.is_none());
                for (index, replacement) in [
                    (9, "sha256:bad"),
                    (10, "sha256:bad"),
                    (11, "sha256:bad"),
                    (12, "create_lock"),
                    (13, "../Cargo.toml"),
                    (13, "/Cargo.toml"),
                    (13, "src/lib.rs"),
                    (0, "m2-dependency-add-v2"),
                ] {
                    let mut invalid = fields;
                    invalid[index] = replacement;
                    assert!(matches!(
                        validation_view(&framed(&invalid)),
                        Err(MutationError::Invalid)
                    ));
                }
                for malformed in [
                    format!("{valid}0:"),
                    framed(&fields[..13]),
                    format!("{}99999:x", framed(&fields[..13])),
                    format!("{}1:é", framed(&fields[..13])),
                ] {
                    assert!(matches!(
                        validation_view(&malformed),
                        Err(MutationError::Invalid)
                    ));
                }
            }
        }
    }

    #[test]
    fn analyzer_action_provenance_decodes_exact_frames_and_stays_out_of_the_m2_view() {
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let action_digest = format!("sha256:{}", "d".repeat(64));
        let fields = [
            "m6-analyzer-action-v1",
            "local_coordinated",
            "linux/aarch64",
            fingerprint.as_str(),
            fingerprint.as_str(),
            fingerprint.as_str(),
            "1.98.1",
            "1.98.1",
            fingerprint.as_str(),
            "rust-analyzer 1.98.1 (48a229c 2026-09-01)",
            fingerprint.as_str(),
            fingerprint.as_str(),
            action_digest.as_str(),
        ];
        let valid = framed(&fields);
        let view = analyzer_action_validation_view(&valid).expect("valid analyzer provenance");
        assert!(matches!(
            view.method,
            AnalyzerActionValidationMethod::WorkspaceEditStructuralOnly
        ));
        assert_eq!(view.action_digest, action_digest);
        assert_eq!(
            view.analyzer.version,
            "rust-analyzer 1.98.1 (48a229c 2026-09-01)"
        );
        assert_eq!(view.analyzed_source_fingerprint, fingerprint);
        // The frozen M2 view never names an analyzer method.
        assert!(matches!(
            validation_view(&valid),
            Err(MutationError::Invalid)
        ));

        for (index, replacement) in [
            (0, "m6-analyzer-action-v2"),
            (0, "m2-fmt-apply-v1"),
            (1, "exclusive"),
            (3, "sha256:bad"),
            (4, "sha256:bad"),
            (5, "sha256:bad"),
            (8, "sha256:bad"),
            (9, " "),
            (10, "sha256:bad"),
            (11, "sha256:bad"),
            (12, "sha256:bad"),
        ] {
            let mut invalid = fields;
            invalid[index] = replacement;
            assert!(
                matches!(
                    analyzer_action_validation_view(&framed(&invalid)),
                    Err(MutationError::Invalid)
                ),
                "field {index} = {replacement:?}"
            );
        }
        let fmt = format!("{}71:{fingerprint}", framed(&fields[..9]));
        for malformed in [format!("{valid}0:"), framed(&fields[..12]), fmt] {
            assert!(matches!(
                analyzer_action_validation_view(&malformed),
                Err(MutationError::Invalid)
            ));
        }
    }

    /// V07 P3-3: the provenance the application encodes is decoded by this
    /// crate field for field. Every field carries a distinct value, so
    /// reordering frames on either side of the crate boundary swaps two of
    /// them and fails the exact comparison below.
    #[test]
    fn analyzer_provenance_survives_the_application_encoder_field_for_field()
    -> Result<(), Box<dyn std::error::Error>> {
        let hash = |value: u8| format!("sha256:{value:064x}");
        let (image, configuration, session, analyzed, binary, config, action) = (
            hash(1),
            hash(2),
            hash(3),
            hash(4),
            hash(5),
            hash(6),
            hash(7),
        );
        let encoded = rust_engineering_application::analyzer::AnalyzerActionProvenance {
            platform: "linux/aarch64",
            image_id: &image,
            configuration_fingerprint: &configuration,
            session_fingerprint: &session,
            rust_version: "1.98.1",
            cargo_version: "1.98.2",
            analyzed_source_fingerprint: &analyzed,
            analyzer_version: "rust-analyzer 1.98.1 (48a229c 2026-09-01)",
            binary_sha256: &binary,
            config_digest: &config,
            action_digest: &action,
        }
        .encode()
        .map_err(|error| format!("{error:?}"))?;
        let view = analyzer_action_validation_for(MutationKind::AnalyzerActionApply, &encoded)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            serde_json::to_value(&view)?,
            json!({
                "method": "workspace_edit_structural_only",
                "semantics": "latest_known",
                "platform": "linux/aarch64",
                "image_id": image,
                "configuration_fingerprint": configuration,
                "session_execution_fingerprint": session,
                "rust_version": "1.98.1",
                "cargo_version": "1.98.2",
                "analyzed_source_fingerprint": analyzed,
                "analyzer": {
                    "version": "rust-analyzer 1.98.1 (48a229c 2026-09-01)",
                    "binary_sha256": binary,
                    "config_digest": config
                },
                "action_digest": action
            })
        );
        Ok(())
    }

    /// V08 item 9(b): the fixtures elsewhere all use a synthetic `sha256:`
    /// hash for `image_id`; none decodes the real, production
    /// `APPROVED_M6_IMAGE` constant `gateway.image_id()` reports. Not
    /// `#[ignore]`d, so a change to that constant's shape (away from
    /// `sha256:<64hex>`) fails this test instead of only the ignored native
    /// gateway test, which never runs in CI.
    #[test]
    fn the_production_image_id_constant_decodes_as_a_source_fingerprint()
    -> Result<(), Box<dyn std::error::Error>> {
        let hash = |value: u8| format!("sha256:{value:064x}");
        let encoded = rust_engineering_application::analyzer::AnalyzerActionProvenance {
            platform: "linux/aarch64",
            image_id: rust_engineering_execution::APPROVED_M6_IMAGE,
            configuration_fingerprint: &hash(1),
            session_fingerprint: &hash(2),
            rust_version: "1.98.1",
            cargo_version: "1.98.1",
            analyzed_source_fingerprint: &hash(3),
            analyzer_version: "rust-analyzer 1.98.1 (48a229c 2026-09-01)",
            binary_sha256: &hash(4),
            config_digest: &hash(5),
            action_digest: &hash(6),
        }
        .encode()
        .map_err(|error| format!("{error:?}"))?;
        let view = analyzer_action_validation_for(MutationKind::AnalyzerActionApply, &encoded)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(view.image_id, rust_engineering_execution::APPROVED_M6_IMAGE);
        Ok(())
    }

    /// V07 P3-4: a view is chosen by the plan's writer kind; an analyzer
    /// provenance never takes the frozen M2 view, nor an M2 plan the analyzer
    /// view, even when the bytes themselves would decode.
    #[test]
    fn a_validation_view_is_chosen_by_the_writer_kind_and_never_crosses_over() {
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let analyzer = framed(&[
            "m6-analyzer-action-v1",
            "local_coordinated",
            "linux/aarch64",
            &fingerprint,
            &fingerprint,
            &fingerprint,
            "1.98.1",
            "1.98.1",
            &fingerprint,
            "rust-analyzer 1.98.1 (48a229c 2026-09-01)",
            &fingerprint,
            &fingerprint,
            &fingerprint,
        ]);
        let format = format!(
            "{}71:{fingerprint}",
            framed(&[
                "m2-fmt-apply-v1",
                "local_coordinated",
                "linux/arm64",
                &fingerprint,
                &fingerprint,
                &fingerprint,
                "1.98.1",
                "1.98.1",
                &fingerprint,
            ])
        );
        assert!(
            analyzer_action_validation_for(MutationKind::AnalyzerActionApply, &analyzer).is_ok()
        );
        assert!(m2_validation_view(MutationKind::FormatApply, &format).is_ok());
        for kind in [
            MutationKind::ManifestPatch,
            MutationKind::FormatApply,
            MutationKind::FixApply,
            MutationKind::DependencyAdd,
            MutationKind::DependencyRemove,
        ] {
            assert!(matches!(
                analyzer_action_validation_for(kind, &analyzer),
                Err(MutationError::Invalid)
            ));
        }
        assert!(matches!(
            m2_validation_view(MutationKind::AnalyzerActionApply, &format),
            Err(MutationError::Invalid)
        ));
        assert!(matches!(
            analyzer_action_validation_for(MutationKind::AnalyzerActionApply, &format),
            Err(MutationError::Invalid)
        ));
    }

    #[test]
    fn missing_grant_denies_before_creating_state() {
        let mut provider = Provider {
            config: None,
            store: None,
            kind: MutationKind::ManifestPatch,
        };
        assert_eq!(provider.store(), Err(MutationError::PermissionDenied));
        assert!(provider.store.is_none());
    }

    #[test]
    fn failure_status_and_mcp_error_distinguish_availability_validation_and_cargo()
    -> Result<(), Box<dyn std::error::Error>> {
        let contract = Contract::<Input, Output>::new()?;
        for (reason, status, is_error) in [
            (Reason::OfflineDataMissing, "unavailable", true),
            (Reason::OfflineDataInvalid, "blocked", true),
            (Reason::CandidateInvalid, "failed", false),
            (Reason::RecoveryRequired, "blocked", true),
        ] {
            let response = contract.encode(Output::failure(reason, 1))?;
            let response = serde_json::to_value(response)?;
            assert_eq!(response["isError"], is_error);
            assert_eq!(response["structuredContent"]["status"], status);
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    response["content"][0]["text"]
                        .as_str()
                        .ok_or("text absent")?
                )?,
                response["structuredContent"]
            );
        }
        Ok(())
    }

    #[test]
    fn format_and_fix_inputs_map_every_action_exactly() {
        let project_ref = "prj_0123456789abcdef0123456789abcdef";
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let mapped = |fix, action| {
            let input = json!({"project_ref":project_ref, "action":action});
            if fix {
                serde_json::from_value::<FixInput>(input)
                    .expect("fix input")
                    .into_request()
                    .expect("fix request")
            } else {
                serde_json::from_value::<FormatInput>(input)
                    .expect("format input")
                    .into_request()
                    .expect("format request")
            }
        };
        let preview = json!({"mode":"preview", "expected_project_fingerprint":fingerprint});
        for fix in [false, true] {
            assert!(
                matches!(mapped(fix, preview.clone()).action, Action::FormatPreview {
                expected_project_fingerprint
            } if expected_project_fingerprint.to_string() == fingerprint)
            );
        }
        let commit = json!({"mode":"commit", "plan_id":"mut_0123456789abcdef0123456789abcdef",
            "plan_digest":fingerprint, "idempotency_key":"key_1"});
        for fix in [false, true] {
            assert!(
                matches!(mapped(fix, commit.clone()).action, Action::Commit {
                plan_id, plan_digest, idempotency_key
            } if plan_id == "mut_0123456789abcdef0123456789abcdef"
                && plan_digest.to_string() == fingerprint && idempotency_key == "key_1")
            );
        }
        let receipt = json!({"mode":"receipt",
            "operation_id":"mut_0123456789abcdef0123456789abcdef", "recover":true});
        for fix in [false, true] {
            assert!(
                matches!(mapped(fix, receipt.clone()).action, Action::Receipt {
                operation_id, recover
            } if operation_id == "mut_0123456789abcdef0123456789abcdef" && recover)
            );
        }
    }

    #[test]
    fn every_failure_reason_has_an_exact_status_code_and_message() {
        for (reason, status, code, message) in [
            (
                Reason::InvalidOperation,
                "blocked",
                "invalid_operation",
                "Operation is outside this tool's supported mutation contract",
            ),
            (
                Reason::PermissionDenied,
                "blocked",
                "permission_denied",
                "A current project reference and exact host write grant for this operation are required",
            ),
            (
                Reason::Conflict,
                "blocked",
                "conflict",
                "Source or approval changed; reopen the project and request a new preview",
            ),
            (
                Reason::LockBusy,
                "blocked",
                "lock_busy",
                "Another operation is active; retry when it finishes",
            ),
            (
                Reason::PlanExpired,
                "blocked",
                "plan_expired",
                "Preview expired; request a new preview and review its diff",
            ),
            (
                Reason::NotFound,
                "blocked",
                "not_found",
                "Plan or journal was not found; reopen and use the exact operation ID/digest/key for durable replay, or create a new preview",
            ),
            (
                Reason::LimitExceeded,
                "blocked",
                "limit_exceeded",
                "Mutation exceeds a bounded plan, output or journal budget",
            ),
            (
                Reason::UnsupportedPlatform,
                "unavailable",
                "unsupported_platform",
                "This writer requires the qualified macOS ARM64 APFS adapter",
            ),
            (
                Reason::Io,
                "blocked",
                "io",
                "Mutation I/O failed; consult the original operation receipt before retrying a commit",
            ),
            (
                Reason::RecoveryRequired,
                "blocked",
                "recovery_required",
                "Preserve journal and mutation temporaries; reopen and request receipt with recover=true",
            ),
            (
                Reason::ToolchainUnavailable,
                "unavailable",
                "toolchain_unavailable",
                "Configure the approved offline Cargo runtime before preview",
            ),
            (
                Reason::CandidateInvalid,
                "failed",
                "candidate_invalid",
                "The isolated tool or postcondition check rejected the candidate; source was not changed",
            ),
            (
                Reason::Cancelled,
                "cancelled",
                "cancelled",
                "Operation cancelled before a successful receipt was returned",
            ),
            (
                Reason::CommandTimeout,
                "failed",
                "command_timeout",
                "The isolated operation exceeded its time budget; no candidate was approved",
            ),
            (
                Reason::OfflineDataMissing,
                "unavailable",
                "offline_data_missing",
                "Configure or update a host-approved Cargo vendor dataset for offline dependency resolution",
            ),
            (
                Reason::OfflineDataInvalid,
                "blocked",
                "offline_data_invalid",
                "The configured Cargo vendor data could not be verified against the host fingerprint",
            ),
        ] {
            let output =
                serde_json::to_value(Output::failure(reason, 17)).expect("serializable output");
            assert_eq!(output["status"], status);
            assert_eq!(output["error_code"], json!(code));
            assert_eq!(output["error_message"], message);
            assert_eq!(output["duration_ms"], 17);
        }
    }

    #[test]
    fn mutation_and_preparation_errors_map_to_exact_public_reasons() {
        for (error, expected) in [
            (MutationError::Invalid, "invalid_operation"),
            (MutationError::PermissionDenied, "permission_denied"),
            (MutationError::Conflict, "conflict"),
            (MutationError::Busy, "lock_busy"),
            (MutationError::Expired, "plan_expired"),
            (MutationError::NotFound, "not_found"),
            (MutationError::LimitExceeded, "limit_exceeded"),
            (MutationError::UnsupportedPlatform, "unsupported_platform"),
            (MutationError::Cancelled, "cancelled"),
            (MutationError::Io, "io"),
            (MutationError::RecoveryRequired, "recovery_required"),
        ] {
            assert_eq!(reason_name(reason(error)), expected);
        }

        use rust_engineering_domain::{ManifestEditError, OperationalErrorCode};
        for (error, expected) in [
            (
                MutationPreparationError::Edit(ManifestEditError::InvalidManifest),
                "invalid_operation",
            ),
            (
                MutationPreparationError::Project(ProjectError::Cancelled),
                "cancelled",
            ),
            (
                MutationPreparationError::Project(ProjectError::Internal),
                "io",
            ),
            (
                MutationPreparationError::Project(ProjectError::Rejected(
                    OperationalErrorCode::InvalidProject,
                )),
                "conflict",
            ),
            (
                MutationPreparationError::Project(ProjectError::Rejected(
                    OperationalErrorCode::ProjectNotFound,
                )),
                "permission_denied",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::Execution(
                    ExecutionError::Denied,
                )),
                "toolchain_unavailable",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::Execution(
                    ExecutionError::Unavailable,
                )),
                "toolchain_unavailable",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::Execution(
                    ExecutionError::InvalidConfiguration,
                )),
                "toolchain_unavailable",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::Project(
                    ProjectError::Cancelled,
                )),
                "cancelled",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::Execution(
                    ExecutionError::Cancelled,
                )),
                "cancelled",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::Project(
                    ProjectError::Rejected(OperationalErrorCode::InvalidProject),
                )),
                "candidate_invalid",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::Project(
                    ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
                )),
                "command_timeout",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::OutputLimit),
                "limit_exceeded",
            ),
            (
                MutationPreparationError::Inspection(InspectionError::Internal),
                "io",
            ),
        ] {
            assert_eq!(reason_name(preparation_reason(error)), expected);
        }
    }

    #[test]
    fn semantic_errors_map_to_exact_public_reasons() {
        use rust_engineering_application::{ResolutionError, SemanticPreparationError as E};
        use rust_engineering_domain::ManifestEditError;
        for (error, expected) in [
            (E::Mutation(MutationError::Conflict), "conflict"),
            (E::Edit(ManifestEditError::Conflict), "conflict"),
            (E::Edit(ManifestEditError::LimitExceeded), "limit_exceeded"),
            (
                E::Edit(ManifestEditError::InvalidManifest),
                "invalid_operation",
            ),
            (E::Project(ProjectError::Cancelled), "cancelled"),
            (
                E::Inspection(InspectionError::OutputLimit),
                "limit_exceeded",
            ),
            (
                E::Resolution(ResolutionError::Inspection(InspectionError::Internal)),
                "io",
            ),
            (
                E::Resolution(ResolutionError::MissingOfflineData),
                "offline_data_missing",
            ),
            (
                E::Resolution(ResolutionError::InvalidOfflineData),
                "offline_data_invalid",
            ),
            (E::Resolution(ResolutionError::Failed), "candidate_invalid"),
        ] {
            assert_eq!(reason_name(semantic_reason(error)), expected);
        }
    }

    #[test]
    fn joined_outputs_preserve_receipt_terminality_and_cancellation() {
        let receipt = |state| Data::Receipt {
            operation_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: format!("sha256:{}", "a".repeat(64)),
            state,
            validation: validation_view(&framed(&[
                "m2-manifest-lints-v1",
                "local_coordinated",
                "linux/aarch64",
                &format!("sha256:{}", "a".repeat(64)),
                &format!("sha256:{}", "b".repeat(64)),
                &format!("sha256:{}", "c".repeat(64)),
                "1.98.1",
                "1.98.1",
                &format!("sha256:{}", "d".repeat(64)),
            ]))
            .expect("validation"),
            files: vec![],
        };
        for (data, interrupted, status, code) in [
            (
                receipt(ReceiptState::RecoveryRequired),
                false,
                "blocked",
                Some("recovery_required"),
            ),
            (
                receipt(ReceiptState::Aborted),
                false,
                "blocked",
                Some("conflict"),
            ),
            (receipt(ReceiptState::Committed), true, "passed", None),
            (receipt(ReceiptState::NoChange), false, "passed", None),
        ] {
            let output = serde_json::to_value(joined_output(Ok(data), interrupted, 5))
                .expect("serializable output");
            assert_eq!(output["status"], status);
            assert_eq!(output["error_code"], json!(code));
            assert_eq!(output["duration_ms"], 5);
        }

        let preview = preview_output_data();
        assert_eq!(
            reason_name(
                joined_output(Ok(preview.clone()), true, 1)
                    .error_code
                    .expect("code")
            ),
            "cancelled"
        );
        assert!(matches!(
            joined_output(Ok(preview), false, 1).status,
            Status::Passed
        ));
        assert_eq!(
            reason_name(
                joined_output(Err(Reason::Io), false, 1)
                    .error_code
                    .expect("code")
            ),
            "io"
        );
    }

    fn preview_output_data() -> Data {
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        Data::Preview {
            plan_id: "mut_0123456789abcdef0123456789abcdef".into(),
            plan_digest: fingerprint.clone(),
            expires_in_seconds: 600,
            files: vec![],
            diff: String::new(),
            validation: validation_view(&framed(&[
                "m2-manifest-lints-v1",
                "local_coordinated",
                "linux/aarch64",
                &fingerprint,
                &fingerprint,
                &fingerprint,
                "1.98.1",
                "1.98.1",
                &fingerprint,
            ]))
            .expect("validation"),
        }
    }

    #[test]
    fn receipt_states_and_file_effects_are_preserved_exactly() {
        for (state, expected) in [
            (MutationState::Committed, "committed"),
            (MutationState::NoChange, "no_change"),
            (MutationState::Aborted, "aborted"),
            (MutationState::RecoveryRequired, "recovery_required"),
        ] {
            assert_eq!(
                serde_json::to_value(receipt_state(state)).expect("state"),
                expected
            );
        }
        let files = receipt_changes(vec![MutationFileReceipt {
            path: "src/lib.rs".into(),
            before: test_fingerprint(1),
            after: test_fingerprint(2),
            before_bytes: 7,
            after_bytes: 9,
            effect_after: Some(test_fingerprint(3)),
            effect_after_bytes: Some(11),
        }]);
        assert_eq!(
            serde_json::to_value(files).expect("files"),
            json!([{
                "path":"src/lib.rs", "before_sha256":format!("sha256:{:064x}", 1),
                "intended_after_sha256":format!("sha256:{:064x}", 2), "before_bytes":7,
                "intended_after_bytes":9, "effect_after_sha256":format!("sha256:{:064x}", 3),
                "effect_after_bytes":11
            }])
        );
    }

    #[test]
    fn preview_diff_enforces_kind_paths_utf8_and_output_budget() {
        let (changes, diff) = preview_diff(&candidate(
            MutationKind::FormatApply,
            "src/lib.rs",
            b"fn a() {}",
            b"fn a() {\n}\n",
        ))
        .expect("format diff");
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].path, "src/lib.rs");
        assert!(diff.contains("@@ -1,1 +1,2 @@"));
        assert!(diff.contains("\\ No newline at end of file"));

        assert!(
            preview_diff(&candidate(
                MutationKind::ManifestPatch,
                "Cargo.toml",
                b"[package]\n",
                b"[workspace]\n",
            ))
            .is_ok()
        );
        assert!(
            preview_diff(&candidate(
                MutationKind::DependencyAdd,
                "member/Cargo.toml",
                b"[dependencies]\n",
                b"a = \"1\"\n",
            ))
            .is_ok()
        );
        assert!(
            preview_diff(&candidate(
                MutationKind::DependencyRemove,
                "Cargo.lock",
                b"old\n",
                b"new\n",
            ))
            .is_ok()
        );
        assert!(matches!(
            preview_diff(&candidate(
                MutationKind::ManifestPatch,
                "src/lib.rs",
                b"old",
                b"new",
            )),
            Err(MutationError::PermissionDenied)
        ));
        assert!(matches!(
            preview_diff(&candidate(
                MutationKind::AnalyzerActionApply,
                "src/lib.rs",
                b"old",
                &[0xff],
            )),
            Err(MutationError::Invalid)
        ));
        assert!(
            preview_diff(&candidate(
                MutationKind::FixApply,
                "src/lib.rs",
                b"same",
                b"same",
            ))
            .expect("unchanged")
            .0
            .is_empty()
        );
        assert!(matches!(
            preview_diff(&candidate(
                MutationKind::FormatApply,
                "src/lib.rs",
                b"old\n",
                "new\n".repeat(70_000).as_bytes(),
            )),
            Err(MutationError::LimitExceeded)
        ));

        let missing_after = MutationCandidate {
            kind: MutationKind::FormatApply,
            before: SourceBundle::new(vec![
                SourceFile::new("src/lib.rs".into(), b"old".to_vec()).expect("source"),
            ])
            .expect("bundle"),
            after: SourceBundle::new(vec![
                SourceFile::new("src/main.rs".into(), b"new".to_vec()).expect("source"),
            ])
            .expect("bundle"),
            validation: String::new(),
        };
        assert!(matches!(
            preview_diff(&missing_after),
            Err(MutationError::Invalid)
        ));
    }

    #[test]
    fn cleanup_uncertain_is_observed_before_reason_mapping() {
        let observed = AtomicBool::new(false);
        let mapped = observed_preparation_reason(
            MutationPreparationError::Inspection(InspectionError::Execution(
                ExecutionError::CleanupUncertain,
            )),
            &observed,
        );
        assert!(matches!(mapped, Reason::Io));
        assert!(observed.load(Ordering::Acquire));

        let observed = AtomicBool::new(false);
        let mapped = observed_semantic_reason(
            rust_engineering_application::SemanticPreparationError::Resolution(
                rust_engineering_application::ResolutionError::Inspection(
                    InspectionError::Execution(ExecutionError::CleanupUncertain),
                ),
            ),
            &observed,
        );
        assert!(matches!(mapped, Reason::Io));
        assert!(observed.load(Ordering::Acquire));
    }

    #[test]
    fn lost_preview_is_a_cancelled_event_without_a_produced_id() {
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let record = AuditRecord {
            admitted: true,
            cleanup_uncertain: false,
            output: preview_output(
                Data::Preview {
                    plan_id: "mut_0123456789abcdef0123456789abcdef".into(),
                    plan_digest: fingerprint.clone(),
                    expires_in_seconds: 600,
                    files: vec![],
                    diff: String::new(),
                    validation: ValidationView {
                        resolution: None,
                        mutation_execution_fingerprint: None,
                        method: ValidationMethod::CargoMetadataFrozenNoDeps,
                        semantics: SnapshotSemantics::LatestKnown,
                        platform: "linux/arm64".into(),
                        image_id: fingerprint.clone(),
                        configuration_fingerprint: fingerprint.clone(),
                        execution_fingerprint: fingerprint.clone(),
                        rust_version: "1.98.1".into(),
                        cargo_version: "1.98.1".into(),
                        candidate_source_fingerprint: fingerprint,
                    },
                },
                9,
            ),
            allocation: None,
        };
        let lost = CallAuditState::response_lost(record);
        assert!(matches!(lost.output.status, Status::Cancelled));
        assert!(matches!(lost.output.error_code, Some(Reason::Cancelled)));
        assert!(lost.output.data.is_none());
        assert_eq!(lost.output.duration_ms, 9);
    }
}
