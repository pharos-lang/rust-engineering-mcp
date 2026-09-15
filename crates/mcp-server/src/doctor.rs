//! Explicit diagnostics; passive observations never construct an execution gateway.
use crate::stdio::{self, CatalogProvider};
use rust_engineering_application::{
    InspectionControl, ProjectError, ToolchainInspectionPort, catalog_context,
};
use rust_engineering_domain::*;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

pub(crate) struct Invocation {
    pub host: stdio::HostConfig,
    pub active: bool,
    pub json: bool,
    /// D-5: unlike `serve`, `doctor` only ever *reads* journal metadata under
    /// `--state-root` (the same read `mutation list` performs), so it accepts
    /// the flag alone when the full Docker tuple `host_config` requires is
    /// absent. `None` when the full tuple parsed (`host.rust` covers it) or
    /// no `--state-root` was given at all.
    pub journal_state_root: Option<PathBuf>,
}
/// Exactly one `--state-root VALUE` pair among closed `flag, value` pairs; the
/// caller only invokes this once `crate::host_config::parse` has already
/// rejected the full arg list, so any other `--state-root` count or shape is
/// left to fail there rather than be silently reinterpreted here.
fn solo_state_root(host: &[OsString]) -> Option<PathBuf> {
    let mut found = None;
    for pair in host.as_chunks::<2>().0 {
        if pair[0] == OsStr::new("--state-root") {
            if found.is_some() {
                return None;
            }
            found = Some(PathBuf::from(&pair[1]));
        }
    }
    found.filter(|path: &PathBuf| path.is_absolute())
}
pub(crate) fn parse(mut args: impl Iterator<Item = OsString>) -> Option<Invocation> {
    let (mut active, mut json) = (false, false);
    let mut host = Vec::new();
    while let Some(flag) = args.next() {
        if flag == OsStr::new("--active") {
            if active {
                return None;
            }
            active = true;
        } else if flag == OsStr::new("--json") {
            if json {
                return None;
            }
            json = true;
        } else {
            host.push(flag);
            host.push(args.next()?);
        }
    }
    if let Some(config) = crate::host_config::parse(host.iter().cloned()) {
        return Some(Invocation {
            host: config,
            active,
            json,
            journal_state_root: None,
        });
    }
    let journal_state_root = solo_state_root(&host)?;
    let filtered = host
        .as_chunks::<2>()
        .0
        .iter()
        .filter(|pair| pair[0] != OsStr::new("--state-root"))
        .flatten()
        .cloned();
    Some(Invocation {
        host: crate::host_config::parse(filtered)?,
        active,
        json,
        journal_state_root: Some(journal_state_root),
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Passive,
    Active,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Passed,
    Warning,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Id {
    Catalog,
    Model,
    SemanticIndex,
    Rustsec,
    CatalogFreshness,
    ModelFreshness,
    RustsecFreshness,
    FilesystemRoots,
    Rustc,
    Cargo,
    Rustfmt,
    Clippy,
    Sandbox,
    HostTools,
    CargoAudit,
    AuditEngine,
    OptionalTools,
    Diagnostic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Scope {
    CatalogSnapshot,
    LocalModel,
    HostFilesystem,
    ApprovedRuntime,
    Host,
    CompiledFeature,
    Diagnostic,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum CheckStatus {
    Available,
    Unavailable,
    NotConfigured,
    NotChecked,
    NotUsed,
    Warning,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Reason {
    Verified,
    NotConfigured,
    ActiveRequired,
    Unavailable,
    UnsupportedPlatform,
    Unknown,
    Fresh,
    FreshnessNeedsReview,
    OwnedRustsecEngine,
    Interrupted,
    CleanupUncertain,
    Deadline,
    OutputLimit,
    Internal,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    None,
    ConfigureOptional,
    ReviewConfiguredFiles,
    RunActive,
    ReviewRuntime,
    RefreshSnapshotExplicitly,
    UseSupportedPlatform,
    ReviewDiagnostic,
}
impl Action {
    fn text(self) -> &'static str {
        match self {
            Self::None => "No action required",
            Self::ConfigureOptional => "Configure this optional facility when needed",
            Self::ReviewConfiguredFiles => {
                "Review configured files, identities and access permissions"
            }
            Self::RunActive => "Use doctor --active to observe the approved runtime",
            Self::ReviewRuntime => {
                "Review the approved runtime configuration and containment requirements"
            }
            Self::RefreshSnapshotExplicitly => {
                "Review snapshot age and explicitly acquire a newer trusted snapshot if needed"
            }
            Self::UseSupportedPlatform => {
                "Use a platform with the required secure filesystem adapter"
            }
            Self::ReviewDiagnostic => "Review configuration and rerun the diagnostic",
        }
    }
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Check {
    id: Id,
    scope: Scope,
    status: CheckStatus,
    reason: Reason,
    component_reason: Option<CatalogComponentUnavailable>,
    action: Action,
    severity: Status,
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct KindCounts {
    pending: u64,
    terminal: u64,
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct MutationJournalsReport {
    pending: u64,
    terminal: u64,
    unknown_format: u64,
    kinds: BTreeMap<&'static str, KindCounts>,
    downgrade_blocked: bool,
    downgrade_blocking_kinds: Vec<&'static str>,
    notes: Vec<&'static str>,
}
impl MutationJournalsReport {
    fn empty() -> Self {
        Self {
            pending: 0,
            terminal: 0,
            unknown_format: 0,
            kinds: BTreeMap::new(),
            downgrade_blocked: false,
            downgrade_blocking_kinds: vec![],
            notes: vec![],
        }
    }
}
/// The five M2 operation kinds a `0.3.0` binary's `operation_kind` recognizes
/// (ADR-088 §3); any other kind makes that binary refuse the journal with
/// `RecoveryRequired` before touching the workspace.
const KINDS_KNOWN_TO_0_3_0: [MutationKind; 5] = [
    MutationKind::ManifestPatch,
    MutationKind::FormatApply,
    MutationKind::FixApply,
    MutationKind::DependencyAdd,
    MutationKind::DependencyRemove,
];
fn kind_name(kind: MutationKind) -> &'static str {
    match kind {
        MutationKind::ManifestPatch => "manifest_patch",
        MutationKind::FormatApply => "format_apply",
        MutationKind::FixApply => "fix_apply",
        MutationKind::DependencyAdd => "dependency_add",
        MutationKind::DependencyRemove => "dependency_remove",
        MutationKind::AnalyzerActionApply => "analyzer_action_apply",
    }
}
const DOWNGRADE_NOTE: &str = "A binary older than 0.8.0 cannot interpret newer operation kinds such as analyzer_action_apply and returns RecoveryRequired before any effect; recover, complete or prune (`mutation prune`) with 0.8.0 before installing an older binary.";
/// Passive M2 journal preflight (D12 §3), additive since 0.8.0; `format_version`
/// is unchanged because no existing field changed shape. Reads only journal
/// metadata under `--state-root`, never workspace or source content.
fn mutation_journals(state_root: &std::path::Path) -> MutationJournalsReport {
    let journal_dir = state_root.join("rust-mcp-mutations-v1");
    if !journal_dir.is_dir() {
        return MutationJournalsReport::empty();
    }
    match rust_engineering_project::mutation_store::NativeMutationStore::open(&journal_dir, &[])
        .and_then(|store| store.list_records())
    {
        Ok(records) => {
            let (mut pending, mut terminal) = (0u64, 0u64);
            let mut kinds: BTreeMap<&'static str, KindCounts> = BTreeMap::new();
            let mut blocking_kinds = std::collections::BTreeSet::new();
            for record in &records {
                let entry = kinds.entry(kind_name(record.kind)).or_insert(KindCounts {
                    pending: 0,
                    terminal: 0,
                });
                match record.state {
                    MutationState::RecoveryRequired => {
                        pending += 1;
                        entry.pending += 1;
                    }
                    MutationState::Committed | MutationState::NoChange | MutationState::Aborted => {
                        terminal += 1;
                        entry.terminal += 1;
                    }
                }
                if !KINDS_KNOWN_TO_0_3_0.contains(&record.kind) {
                    blocking_kinds.insert(kind_name(record.kind));
                }
            }
            let downgrade_blocked = pending > 0 || !blocking_kinds.is_empty();
            MutationJournalsReport {
                pending,
                terminal,
                unknown_format: 0,
                kinds,
                downgrade_blocked,
                downgrade_blocking_kinds: blocking_kinds.into_iter().collect(),
                notes: if downgrade_blocked {
                    vec![DOWNGRADE_NOTE]
                } else {
                    vec![]
                },
            }
        }
        // A concurrent `serve` mutation holds the same non-blocking store
        // lock this passive read takes; distinct from an unreadable store.
        Err(MutationError::Busy) => MutationJournalsReport {
            pending: 0,
            terminal: 0,
            unknown_format: 0,
            kinds: BTreeMap::new(),
            downgrade_blocked: true,
            downgrade_blocking_kinds: vec![],
            notes: vec!["journal busy: a mutation is in progress; rerun doctor"],
        },
        // The envelope/format sniff fails the whole scan closed before any
        // per-record classification (see docs/validation/M8/03-formats-analysis.md
        // §2); `1` is a fail-closed lower-bound sentinel, not an exact tally.
        Err(MutationError::RecoveryRequired) => MutationJournalsReport {
            pending: 0,
            terminal: 0,
            unknown_format: 1,
            kinds: BTreeMap::new(),
            downgrade_blocked: true,
            downgrade_blocking_kinds: vec![],
            notes: vec![
                "At least one journal entry is unreadable or unknown (unrecognized envelope format, a broken checksum, a foreign file or an operation kind this binary cannot interpret); the store fails closed before any per-record detail is available.",
                DOWNGRADE_NOTE,
            ],
        },
        Err(_) => MutationJournalsReport {
            pending: 0,
            terminal: 0,
            unknown_format: 0,
            kinds: BTreeMap::new(),
            downgrade_blocked: true,
            downgrade_blocking_kinds: vec![],
            notes: vec![
                "The mutation journal store could not be read; treated as blocked until the condition is resolved.",
            ],
        },
    }
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    format_version: u32,
    operation: &'static str,
    mode: Mode,
    status: Status,
    pub duration_ms: u64,
    checks: Vec<Check>,
    catalog: Option<CatalogContextStatus>,
    runtime: Option<ToolchainObservation>,
    mutation_journals: Option<MutationJournalsReport>,
}
impl Report {
    fn new(active: bool) -> Self {
        Self {
            format_version: 1,
            operation: "doctor",
            mode: if active { Mode::Active } else { Mode::Passive },
            status: Status::Passed,
            duration_ms: 0,
            checks: vec![],
            catalog: None,
            runtime: None,
            mutation_journals: None,
        }
    }
    fn add(
        &mut self,
        id: Id,
        scope: Scope,
        status: CheckStatus,
        reason: Reason,
        action: Action,
        severity: Status,
    ) {
        if severity == Status::Failed || self.status == Status::Passed {
            self.status = severity;
        }
        self.checks.push(Check {
            id,
            scope,
            status,
            reason,
            component_reason: None,
            action,
            severity,
        });
    }
    pub(crate) fn is_active(&self) -> bool {
        self.mode == Mode::Active
    }
    pub(crate) fn exit_code(&self) -> u8 {
        u8::from(self.status == Status::Failed)
    }
    pub(crate) fn human(&self) -> String {
        let mut text = format!(
            "Doctor {:?} ({:?}); diagnostic scope only\n",
            self.status, self.mode
        );
        for check in &self.checks {
            text.push_str(&format!(
                "{:?} [{:?}]: {:?} ({:?}, component {:?}). {}\n",
                check.id,
                check.scope,
                check.status,
                check.reason,
                check.component_reason,
                check.action.text()
            ));
        }
        if let Some(journals) = &self.mutation_journals {
            text.push_str(&format!(
                "mutation_journals: pending={} terminal={} unknown_format={} downgrade_blocked={}\n",
                journals.pending,
                journals.terminal,
                journals.unknown_format,
                journals.downgrade_blocked
            ));
        }
        text
    }
    pub(crate) fn failure(active: bool, error: ProjectError) -> Self {
        let mut report = Self::new(active);
        report.record_failure(error);
        report
    }
    pub(crate) fn worker_failure(active: bool) -> Self {
        let mut report = Self::failure(active, ProjectError::Internal);
        // A panicked active worker cannot attest explicit gateway cleanup.
        if active {
            for check in &mut report.checks {
                check.reason = Reason::CleanupUncertain;
            }
        }
        report
    }
    pub(crate) fn record_failure(&mut self, error: ProjectError) {
        let reason = match error {
            ProjectError::Cancelled => Reason::Interrupted,
            ProjectError::Rejected(OperationalErrorCode::CommandTimeout) => Reason::Deadline,
            ProjectError::Rejected(OperationalErrorCode::OutputLimitExceeded) => {
                Reason::OutputLimit
            }
            _ => Reason::Internal,
        };
        if self
            .checks
            .iter()
            .any(|check| check.id == Id::Diagnostic && check.reason == reason)
        {
            return;
        }
        self.add(
            Id::Diagnostic,
            Scope::Diagnostic,
            CheckStatus::Unavailable,
            reason,
            Action::ReviewDiagnostic,
            Status::Failed,
        );
    }
    fn component<T>(&mut self, id: Id, scope: Scope, component: &Component<T>, configured: bool) {
        match component {
            Component::Available { .. } => self.add(
                id,
                scope,
                CheckStatus::Available,
                Reason::Verified,
                Action::None,
                Status::Passed,
            ),
            Component::Unavailable { reason } => {
                self.add(
                    id,
                    scope,
                    if configured {
                        CheckStatus::Unavailable
                    } else {
                        CheckStatus::NotConfigured
                    },
                    if configured {
                        Reason::Unavailable
                    } else {
                        Reason::NotConfigured
                    },
                    if configured {
                        Action::ReviewConfiguredFiles
                    } else {
                        Action::ConfigureOptional
                    },
                    if configured {
                        Status::Failed
                    } else {
                        Status::Warning
                    },
                );
                if let Some(check) = self.checks.last_mut() {
                    check.component_reason = Some(*reason);
                }
            }
        }
    }
    fn freshness(&mut self, id: Id, evidence: &SnapshotEvidence) {
        let fresh = matches!(
            evidence.freshness().state(),
            FreshnessState::Fresh | FreshnessState::Live
        );
        self.add(
            id,
            if id == Id::ModelFreshness {
                Scope::LocalModel
            } else {
                Scope::CatalogSnapshot
            },
            if fresh {
                CheckStatus::Available
            } else {
                CheckStatus::Warning
            },
            if fresh {
                Reason::Fresh
            } else {
                Reason::FreshnessNeedsReview
            },
            if fresh {
                Action::None
            } else {
                Action::RefreshSnapshotExplicitly
            },
            if fresh {
                Status::Passed
            } else {
                Status::Warning
            },
        );
    }
}
struct Now;
impl Clock for Now {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
        )
    }
}
pub(crate) fn inspect(
    invocation: &Invocation,
    control: &dyn InspectionControl,
) -> Result<Report, ProjectError> {
    control.check()?;
    let mut report = Report::new(invocation.active);
    let host = &invocation.host;
    let context = catalog_context(
        &CatalogProvider::new(host.catalog.clone(), host.audit.clone()),
        &Now,
        control,
    )?;
    report.component(
        Id::Catalog,
        Scope::CatalogSnapshot,
        &context.catalog,
        host.catalog.is_some(),
    );
    report.component(
        Id::Model,
        Scope::LocalModel,
        &context.model,
        host.catalog.as_ref().is_some_and(|c| c.model_dir.is_some()),
    );
    // Embedded indexes are discovered by the authenticated provider even without an external path.
    let index_configured = host
        .catalog
        .as_ref()
        .is_some_and(|c| c.index_store.is_some())
        || index_source_observed(&context.semantic_index);
    report.component(
        Id::SemanticIndex,
        Scope::CatalogSnapshot,
        &context.semantic_index,
        index_configured,
    );
    report.component(
        Id::Rustsec,
        Scope::CatalogSnapshot,
        &context.rustsec,
        host.audit.is_some(),
    );
    if let Component::Available { value } = &context.catalog {
        report.freshness(Id::CatalogFreshness, &value.evidence);
    }
    if let Component::Available { value } = &context.model {
        report.freshness(Id::ModelFreshness, &value.evidence);
    }
    if let Component::Available { value } = &context.rustsec {
        report.freshness(Id::RustsecFreshness, &value.evidence);
    }
    report.catalog = Some(context);
    control.check()?;
    report.mutation_journals = host
        .rust
        .as_ref()
        .map(|rust| rust.state_root.clone())
        .or_else(|| invocation.journal_state_root.clone())
        .map(|state_root| mutation_journals(&state_root));
    control.check()?;
    if host.roots.is_empty() {
        report.add(
            Id::FilesystemRoots,
            Scope::HostFilesystem,
            CheckStatus::NotConfigured,
            Reason::NotConfigured,
            Action::ConfigureOptional,
            Status::Warning,
        );
    } else if !cfg!(target_os = "macos") {
        report.add(
            Id::FilesystemRoots,
            Scope::HostFilesystem,
            CheckStatus::Unavailable,
            Reason::UnsupportedPlatform,
            Action::UseSupportedPlatform,
            Status::Failed,
        );
    } else {
        let valid = rust_engineering_project::SecureProjects::new(&host.roots).is_ok();
        report.add(
            Id::FilesystemRoots,
            Scope::HostFilesystem,
            if valid {
                CheckStatus::Available
            } else {
                CheckStatus::Unavailable
            },
            if valid {
                Reason::Verified
            } else {
                Reason::Unavailable
            },
            if valid {
                Action::None
            } else {
                Action::ReviewConfiguredFiles
            },
            if valid {
                Status::Passed
            } else {
                Status::Failed
            },
        );
    }
    control.check()?;
    let runtime_ids = [Id::Rustc, Id::Cargo, Id::Rustfmt, Id::Clippy, Id::Sandbox];
    if invocation.active && host.rust.is_some() {
        let source = diagnostic_source()?;
        let inspector = rust_engineering_execution::RustProjectInspector::new(host.rust.clone());
        let observed = inspector.inspect_toolchain(&source, control);
        match observed {
            Ok(observation) => {
                for id in runtime_ids {
                    report.add(
                        id,
                        Scope::ApprovedRuntime,
                        CheckStatus::Available,
                        Reason::Verified,
                        Action::None,
                        Status::Passed,
                    );
                }
                report.runtime = Some(observation);
            }
            Err(error) => {
                use rust_engineering_application::{ExecutionError, InspectionError};
                let reason = match error {
                    InspectionError::Execution(ExecutionError::CleanupUncertain) => {
                        Reason::CleanupUncertain
                    }
                    InspectionError::Execution(ExecutionError::Cancelled)
                    | InspectionError::Project(ProjectError::Cancelled) => match control.check() {
                        Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout)) => {
                            Reason::Deadline
                        }
                        _ => Reason::Interrupted,
                    },
                    InspectionError::Project(ProjectError::Rejected(
                        OperationalErrorCode::CommandTimeout,
                    )) => Reason::Deadline,
                    InspectionError::OutputLimit => Reason::OutputLimit,
                    _ => Reason::Unavailable,
                };
                for id in runtime_ids {
                    report.add(
                        id,
                        Scope::ApprovedRuntime,
                        CheckStatus::Unavailable,
                        reason,
                        Action::ReviewRuntime,
                        Status::Failed,
                    );
                }
            }
        }
    } else {
        for id in runtime_ids {
            report.add(
                id,
                Scope::ApprovedRuntime,
                if host.rust.is_some() {
                    CheckStatus::NotChecked
                } else {
                    CheckStatus::NotConfigured
                },
                if host.rust.is_some() {
                    Reason::ActiveRequired
                } else {
                    Reason::NotConfigured
                },
                if host.rust.is_some() {
                    Action::RunActive
                } else {
                    Action::ConfigureOptional
                },
                Status::Warning,
            );
        }
    }
    report.add(
        Id::HostTools,
        Scope::Host,
        CheckStatus::NotChecked,
        Reason::Unknown,
        Action::None,
        Status::Warning,
    );
    report.add(
        Id::CargoAudit,
        Scope::ApprovedRuntime,
        CheckStatus::NotUsed,
        Reason::OwnedRustsecEngine,
        Action::None,
        Status::Passed,
    );
    report.add(
        Id::AuditEngine,
        Scope::CompiledFeature,
        CheckStatus::Available,
        Reason::OwnedRustsecEngine,
        Action::None,
        Status::Passed,
    );
    report.add(
        Id::OptionalTools,
        Scope::Host,
        CheckStatus::NotChecked,
        Reason::Unknown,
        Action::None,
        Status::Warning,
    );
    if let Err(error) = control.check() {
        report.record_failure(error);
    }
    Ok(report)
}
fn diagnostic_source() -> Result<SourceBundle, ProjectError> {
    let files = [
        (
            "Cargo.toml",
            "[package]\nname='rust_mcp_doctor'\nversion='0.1.0'\nedition='2024'\n",
        ),
        (
            "Cargo.lock",
            "version = 4\n[[package]]\nname = 'rust_mcp_doctor'\nversion = '0.1.0'\n",
        ),
        ("src/lib.rs", "pub fn diagnostic() {}\n"),
    ]
    .into_iter()
    .map(|(path, text)| {
        SourceFile::new(path.into(), text.as_bytes().to_vec()).map_err(|_| ProjectError::Internal)
    })
    .collect::<Result<Vec<_>, _>>()?;
    SourceBundle::new(files).map_err(|_| ProjectError::Internal)
}
#[cfg(test)]
mod tests;

fn index_source_observed<T>(component: &Component<T>) -> bool {
    matches!(
        component,
        Component::Available { .. }
            | Component::Unavailable {
                reason: CatalogComponentUnavailable::Invalid
                    | CatalogComponentUnavailable::IdentityMismatch
                    | CatalogComponentUnavailable::Denied
                    | CatalogComponentUnavailable::Budget
                    | CatalogComponentUnavailable::IoUnavailable
                    | CatalogComponentUnavailable::UnsupportedPlatform
            }
    )
}
