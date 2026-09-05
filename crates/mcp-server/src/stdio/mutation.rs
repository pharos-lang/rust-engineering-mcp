//! New M2 contract; does not extend any M1 error enum or schema.
use super::{
    contract::{Contract, ToolOutput},
    project::Registry,
    workers::{WorkerError, Workers},
};
use rmcp::{
    model::{CallToolRequestParams, CallToolResult, ErrorData, Tool, ToolAnnotations},
    service::{RequestContext, RoleServer},
};
use rust_engineering_application::{
    ExecutionError, InspectionError, MutationPlans, MutationPreparationError, PreviewRetention,
    ProjectError, ReferenceGenerator,
};
use rust_engineering_domain::{
    IdempotencyKey, LintLevel, LintName, LintScope, LintTool, ManifestEdit, MutationCandidate,
    MutationError, MutationId, MutationKind, MutationReceipt, MutationState,
    ProjectIdentityFingerprint, ProjectRef, SourceFingerprint, ToolStatus,
};
use rust_engineering_execution::RustProjectInspector;
use rust_engineering_project::{
    MonotonicClock, OsReferences, TomlManifestEditor, mutation_bytes_digest,
    mutation_store::{NativeMutationStore, mutation_digest},
    prepare_mutation_state,
};
use schemars::JsonSchema;
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
const DEADLINE: Duration = Duration::from_secs(240);
const MAX_RESULT: usize = 512 * 1024;

#[derive(Clone)]
pub(super) struct WriteConfig {
    pub roots: Vec<PathBuf>,
    pub state_parent: PathBuf,
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
    fn into_request(self) -> Input;
}
impl MutationInput for Input {
    const NAME: &'static str = NAME;
    const KIND: MutationKind = MutationKind::ManifestPatch;
    const DESCRIPTION: &'static str = "Preview, commit or inspect a journaled root Cargo.toml lint edit. Host --allow-manifest-write is required. Preview validates the exact candidate in the approved offline Cargo runtime and returns its diff/digest. Commit requires that plan, current project authority and an idempotency key. Reopen the project after commit to read its receipt. Local coordinated writes do not exclude external editors or provide multi-file atomicity. Only package/workspace rust/clippy lint set/remove is currently implemented.";
    fn into_request(self) -> Input {
        self
    }
}
impl MutationInput for FormatInput {
    const NAME: &'static str = FORMAT_NAME;
    const KIND: MutationKind = MutationKind::FormatApply;
    const DESCRIPTION: &'static str = "Preview, commit or inspect a journaled rustfmt operation on existing Rust source files. Host --allow-fmt-write is required. Preview runs rustfmt in the approved offline sandbox, verifies fmt.check against the complete exported candidate, and returns its exact diff/digest without source writes. Commit requires that unexpired plan, current project authority and an idempotency key. Reopen after commit for receipt or recovery. Local coordinated publication does not exclude external editors or provide multi-file atomicity.";
    fn into_request(self) -> Input {
        Input {
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
        }
    }
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Scope {
    Package,
    Workspace,
}
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Namespace {
    Rust,
    Clippy,
}
#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Level {
    Allow,
    Warn,
    Deny,
    Forbid,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Edit {
    LintSet {
        scope: Scope,
        tool: Namespace,
        #[schemars(length(min = 1, max = 128), regex(pattern = "^[A-Za-z0-9_]+$"))]
        name: String,
        level: Level,
        priority: Option<i64>,
    },
    LintRemove {
        scope: Scope,
        tool: Namespace,
        #[schemars(length(min = 1, max = 128), regex(pattern = "^[A-Za-z0-9_]+$"))]
        name: String,
    },
}
impl Edit {
    fn into_domain(self) -> Result<ManifestEdit, MutationError> {
        let scope = |s| match s {
            Scope::Package => LintScope::Package,
            Scope::Workspace => LintScope::Workspace,
        };
        let tool = |s| match s {
            Namespace::Rust => LintTool::Rust,
            Namespace::Clippy => LintTool::Clippy,
        };
        let name = |s| LintName::new(s).map_err(|_| MutationError::Invalid);
        Ok(match self {
            Self::LintSet {
                scope: s,
                tool: t,
                name: n,
                level,
                priority,
            } => ManifestEdit::LintSet {
                scope: scope(s),
                tool: tool(t),
                name: name(n)?,
                priority,
                level: match level {
                    Level::Allow => LintLevel::Allow,
                    Level::Warn => LintLevel::Warn,
                    Level::Deny => LintLevel::Deny,
                    Level::Forbid => LintLevel::Forbid,
                },
            },
            Self::LintRemove {
                scope: s,
                tool: t,
                name: n,
            } => ManifestEdit::LintRemove {
                scope: scope(s),
                tool: tool(t),
                name: name(n)?,
            },
        })
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
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum ValidationMethod {
    CargoMetadataFrozenNoDeps,
    RustfmtThenFmtCheck,
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
    let (method, mutation_execution_fingerprint) = match version.as_str() {
        "m2-manifest-lints-v1" if encoded.is_empty() => {
            (ValidationMethod::CargoMetadataFrozenNoDeps, None)
        }
        "m2-fmt-apply-v1" => {
            let (size, value) = encoded.split_once(':').ok_or(MutationError::Invalid)?;
            if size != "71" || value.len() != 71 {
                return Err(MutationError::Invalid);
            }
            value
                .parse::<SourceFingerprint>()
                .map_err(|_| MutationError::Invalid)?;
            (
                ValidationMethod::RustfmtThenFmtCheck,
                Some(value.to_owned()),
            )
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
struct Output {
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
            Reason::UnsupportedPlatform | Reason::ToolchainUnavailable => Status::Unavailable,
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
                    "Plan or operation was not found; a restarted session requires receipt lookup for persisted operations"
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

pub(super) type ManifestMutationTool = MutationTool<Input>;
pub(super) type FormatMutationTool = MutationTool<FormatInput>;

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
    pub fn new(
        registry: Arc<Mutex<Registry>>,
        workers: Workers,
        inspector: Arc<RustProjectInspector>,
        ready: Arc<AtomicBool>,
        config: Option<WriteConfig>,
        plans: Arc<Mutex<SharedPlans>>,
    ) -> Result<Self, ErrorData> {
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
        let input = self.contract.decode(request.arguments)?.into_request();
        let started = Instant::now();
        if !self.ready.load(Ordering::Acquire) {
            return self
                .contract
                .encode(Output::failure(Reason::PermissionDenied, 0));
        }
        let registry = Arc::clone(&self.registry);
        let provider = Arc::clone(&self.provider);
        let inspector = Arc::clone(&self.inspector);
        let plans = Arc::clone(&self.plans);
        let retention = PreviewRetention::default();
        let preview_token = retention.token();
        let preview_contract = Contract::<I, Output>::new()?;
        let result = self
            .workers
            .run_joined(context.ct, started + DEADLINE, move |control| {
                let mut provider = provider.try_lock().map_err(lock_reason)?;
                provider.store().map_err(reason)?;
                let Provider { store, .. } = &mut *provider;
                let store = store.as_ref().ok_or(Reason::PermissionDenied)?;
                match input.action {
                    action @ (Action::Preview { .. } | Action::FormatPreview { .. }) => {
                        let (workspace, candidate) = match action {
                            Action::Preview {
                                expected_project_fingerprint,
                                edit,
                            } => {
                                let edit = edit.into_domain().map_err(reason)?;
                                let prepared = registry
                                    .try_lock()
                                    .map_err(lock_reason)?
                                    .prepare_manifest(
                                        &input.project_ref,
                                        &expected_project_fingerprint,
                                        &edit,
                                        &TomlManifestEditor,
                                        store,
                                        control,
                                    )
                                    .map_err(preparation_reason)?;
                                // The prepared value owns captured bytes. No registry lock spans Cargo.
                                prepared
                                    .validate(inspector.as_ref(), control)
                                    .map_err(preparation_reason)?
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
                                    .map_err(preparation_reason)?;
                                prepared
                                    .validate(inspector.as_ref(), control)
                                    .map_err(preparation_reason)?
                            }
                            _ => return Err(Reason::InvalidOperation),
                        };
                        registry
                            .try_lock()
                            .map_err(lock_reason)?
                            .finish_manifest_preview(&input.project_ref, &candidate, control)
                            .map_err(preparation_reason)?;
                        let (files, diff) = preview_diff(&candidate).map_err(reason)?;
                        let digest = mutation_digest(&candidate).map_err(reason)?;
                        let reference = OsReferences.generate().map_err(|_| Reason::Io)?;
                        let suffix = reference.as_str().strip_prefix("prj_").ok_or(Reason::Io)?;
                        let id = MutationId::new(format!("mut_{suffix}")).map_err(reason)?;
                        let data = Data::Preview {
                            plan_id: id.as_str().into(),
                            plan_digest: digest.to_string(),
                            expires_in_seconds: MutationPlans::TTL_SECONDS,
                            files,
                            diff,
                            validation: validation_view(&candidate.validation).map_err(reason)?,
                        };
                        // Bound the complete MCP encoding, including duplicated text and JSON,
                        // before retaining a plan whose exact diff the peer cannot receive.
                        validate_preview_size(&preview_contract, &data)?;
                        let mut shared = plans.try_lock().map_err(lock_reason)?;
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
                        let shared = plans.try_lock().map_err(lock_reason)?;
                        let plan = shared
                            .plans
                            .resolve(&id, &plan_digest, key, &shared.clock)
                            .map_err(reason)?;
                        if plan.request.candidate.kind != I::KIND {
                            return Err(Reason::PermissionDenied);
                        }
                        drop(shared);
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
                        receipt_data(receipt).map_err(reason)
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
                        receipt_data(receipt).map_err(reason)
                    }
                }
            })
            .await;
        let duration = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        let output = match result {
            // Native commit finishes its journal after its irreversible point.
            // Do not relabel a durable receipt as cancelled after losing the race.
            Ok(joined) => match joined.result {
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
                Ok(data) if joined.interrupted.is_none() => preview_output(data, duration),
                Ok(_) => Output::failure(Reason::Cancelled, duration),
                Err(error) => Output::failure(error, duration),
            },
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
        let encoded = self.contract.encode(output)?;
        if serde_json::to_vec(&encoded)
            .map_err(|_| ErrorData::internal_error("Mutation encoding failed", None))?
            .len()
            > MAX_RESULT
        {
            return self
                .contract
                .encode(Output::failure(Reason::LimitExceeded, duration));
        }
        if retain_preview {
            retention.retain();
        }
        Ok(encoded)
    }
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

fn receipt_data(receipt: MutationReceipt) -> Result<Data, MutationError> {
    Ok(Data::Receipt {
        operation_id: receipt.id.as_str().into(),
        validation: validation_view(&receipt.validation)?,
        plan_digest: receipt.digest.to_string(),
        state: match receipt.state {
            MutationState::Committed => ReceiptState::Committed,
            MutationState::NoChange => ReceiptState::NoChange,
            MutationState::Aborted => ReceiptState::Aborted,
            MutationState::RecoveryRequired => ReceiptState::RecoveryRequired,
        },
        files: receipt
            .files
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
            .collect(),
    })
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
            MutationKind::ManifestPatch => before.path() == "Cargo.toml",
            MutationKind::FormatApply => before.path().ends_with(".rs"),
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
    fn candidate_failure_is_valid_result_but_recovery_is_operational()
    -> Result<(), Box<dyn std::error::Error>> {
        let contract = Contract::<Input, Output>::new()?;
        for (reason, is_error) in [
            (Reason::CandidateInvalid, false),
            (Reason::RecoveryRequired, true),
        ] {
            let response = contract.encode(Output::failure(reason, 1))?;
            let response = serde_json::to_value(response)?;
            assert_eq!(response["isError"], is_error);
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
}
