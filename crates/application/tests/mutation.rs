// Test-only construction/assertions for fixed fixtures; production retains expect_used=deny.
#![allow(clippy::expect_used)]
use rust_engineering_application::{
    ExecutionCancellation, InspectionControl, InspectionError, ManifestEditor, MutationPlans,
    MutationPreparationError, MutationPublisher, OperationControl, PreviewRetention,
    ProjectBackend, ProjectError, ProjectIdentity, ProjectInspectionPort, ProjectRegistry,
    ProjectSourceBackend, ReferenceGenerator, RegistryClock, ValidatedProject,
};
use rust_engineering_domain::{
    CargoConfiguration, IdempotencyKey, LintLevel, LintName, LintScope, LintTool, ManifestEdit,
    ManifestEditError, MutationCandidate, MutationCommit, MutationError, MutationId, MutationKind,
    MutationReceipt, MutationState, OperationalErrorCode, ProjectConfigPolicy, ProjectRef,
    ProjectStructure, RuntimeIdentity, SourceBundle, SourceFile, SourceFingerprint,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Default)]
struct Control(AtomicBool);

impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.0.load(Ordering::Relaxed) {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[derive(Clone, Default)]
struct Clock(Rc<Cell<u64>>);

impl RegistryClock for Clock {
    fn seconds(&self) -> u64 {
        self.0.get()
    }
}

#[derive(Clone)]
struct Backend {
    sources: Rc<RefCell<Vec<SourceBundle>>>,
    source_calls: Rc<Cell<usize>>,
    next_lease: Rc<Cell<usize>>,
}

impl Backend {
    fn new(sources: Vec<SourceBundle>) -> Self {
        Self {
            sources: Rc::new(RefCell::new(sources)),
            source_calls: Rc::new(Cell::new(0)),
            next_lease: Rc::new(Cell::new(0)),
        }
    }
}

fn identity() -> ProjectIdentity {
    ProjectIdentity {
        workspace_root: "/trusted/workspace".into(),
        fingerprint: fingerprint(1).parse().expect("identity fingerprint"),
    }
}

impl ProjectBackend for Backend {
    type Lease = usize;

    fn open(
        &self,
        _: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<Self::Lease>, ProjectError> {
        let lease = self.next_lease.get() + 1;
        self.next_lease.set(lease);
        Ok(ValidatedProject {
            identity: identity(),
            lease,
        })
    }

    fn revalidate(
        &self,
        _: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        Ok(identity())
    }
}

impl ProjectSourceBackend for Backend {
    fn source(
        &self,
        _: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<SourceBundle, ProjectError> {
        let call = self.source_calls.get();
        self.source_calls.set(call + 1);
        let sources = self.sources.borrow();
        sources
            .get(call.min(sources.len().saturating_sub(1)))
            .cloned()
            .ok_or(ProjectError::Internal)
    }
}

#[derive(Default)]
struct Generator(Cell<u64>);

impl ReferenceGenerator for Generator {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        let next = self.0.get() + 1;
        self.0.set(next);
        format!("prj_{next:032x}")
            .parse()
            .map_err(|_| ProjectError::Internal)
    }
}

#[derive(Default)]
struct Editor {
    calls: Cell<usize>,
    result: RefCell<Option<Result<Vec<u8>, ManifestEditError>>>,
}

impl Editor {
    fn returning(result: Result<Vec<u8>, ManifestEditError>) -> Self {
        Self {
            calls: Cell::new(0),
            result: RefCell::new(Some(result)),
        }
    }
}

impl ManifestEditor for Editor {
    fn apply(&self, _: &[u8], _: &ManifestEdit) -> Result<Vec<u8>, ManifestEditError> {
        self.calls.set(self.calls.get() + 1);
        self.result.borrow().clone().unwrap_or_else(|| {
            Ok(b"[workspace]\n[workspace.lints.rust]\nunsafe_code = \"deny\"\n".to_vec())
        })
    }
}

#[derive(Default)]
struct Inspector {
    calls: Cell<usize>,
    seen: RefCell<Vec<SourceBundle>>,
    error: Cell<Option<InspectionError>>,
}

impl ProjectInspectionPort for Inspector {
    fn inspect(
        &self,
        source: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        self.calls.set(self.calls.get() + 1);
        self.seen.borrow_mut().push(source.clone());
        if let Some(error) = self.error.get() {
            return Err(error);
        }
        Ok(structure())
    }
}

struct Publisher {
    authorize_error: Cell<Option<MutationError>>,
    commit_result: RefCell<Result<MutationReceipt, MutationError>>,
    authorize_leases: RefCell<Vec<usize>>,
    commits: Cell<usize>,
    receipts: RefCell<Vec<usize>>,
    replays: RefCell<Vec<(usize, MutationId, SourceFingerprint, IdempotencyKey)>>,
    recoveries: RefCell<Vec<usize>>,
}

impl Publisher {
    fn new() -> Self {
        Self {
            authorize_error: Cell::new(None),
            commit_result: RefCell::new(Ok(receipt())),
            authorize_leases: RefCell::new(Vec::new()),
            commits: Cell::new(0),
            receipts: RefCell::new(Vec::new()),
            replays: RefCell::new(Vec::new()),
            recoveries: RefCell::new(Vec::new()),
        }
    }
}

impl MutationPublisher<usize> for Publisher {
    fn authorize(&self, lease: &usize) -> Result<(), MutationError> {
        self.authorize_leases.borrow_mut().push(*lease);
        self.authorize_error.get().map_or(Ok(()), Err)
    }

    fn commit(
        &self,
        _: &usize,
        _: &MutationCommit,
        _: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        self.commits.set(self.commits.get() + 1);
        self.commit_result.borrow().clone()
    }

    fn replay(
        &self,
        lease: &usize,
        id: &MutationId,
        digest: &SourceFingerprint,
        key: &IdempotencyKey,
        _: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        self.replays
            .borrow_mut()
            .push((*lease, id.clone(), digest.clone(), key.clone()));
        self.commit_result.borrow().clone()
    }

    fn receipt(&self, lease: &usize, _: &MutationId) -> Result<MutationReceipt, MutationError> {
        self.receipts.borrow_mut().push(*lease);
        Ok(receipt())
    }

    fn recover(&self, lease: &usize, _: &MutationId) -> Result<MutationReceipt, MutationError> {
        self.recoveries.borrow_mut().push(*lease);
        Ok(receipt())
    }
}

fn fingerprint(byte: u8) -> String {
    format!("sha256:{byte:064x}")
}

fn mutation_id(number: u128) -> MutationId {
    MutationId::new(format!("mut_{number:032x}")).expect("mutation id")
}

fn manifest_edit() -> ManifestEdit {
    ManifestEdit::LintSet {
        scope: LintScope::Workspace,
        tool: LintTool::Rust,
        name: LintName::new("unsafe_code".into()).expect("lint name"),
        level: LintLevel::Deny,
        priority: None,
    }
}

fn source(manifest: Option<&[u8]>, extra: &[u8]) -> SourceBundle {
    let mut files = vec![SourceFile::new("src/lib.rs".into(), extra.to_vec()).expect("source")];
    if let Some(bytes) = manifest {
        files.push(SourceFile::new("Cargo.toml".into(), bytes.to_vec()).expect("manifest"));
    }
    SourceBundle::with_directories(files, vec!["empty".into()]).expect("bundle")
}

fn candidate() -> MutationCandidate {
    MutationCandidate {
        kind: MutationKind::ManifestPatch,
        before: source(Some(b"[workspace]\n"), b"before"),
        after: source(Some(b"[workspace]\n[lints]\n"), b"before"),
        validation: "validation".into(),
    }
}

fn maximum_candidate() -> MutationCandidate {
    let files = |prefix: &str| {
        (0..16)
            .map(|index| {
                SourceFile::new(format!("{prefix}{index}"), vec![0; 1024 * 1024])
                    .expect("maximum file")
            })
            .collect()
    };
    MutationCandidate {
        kind: MutationKind::ManifestPatch,
        before: SourceBundle::new(files("b")).expect("maximum before bundle"),
        after: SourceBundle::new(files("a")).expect("maximum after bundle"),
        validation: String::new(),
    }
}

fn request() -> MutationCommit {
    MutationCommit {
        id: mutation_id(1),
        digest: fingerprint(2).parse().expect("digest"),
        key: IdempotencyKey::new("request-1".into()).expect("key"),
        candidate: candidate(),
    }
}

fn receipt() -> MutationReceipt {
    MutationReceipt {
        validation: "test-validation".into(),
        id: mutation_id(1),
        digest: fingerprint(2).parse().expect("digest"),
        state: MutationState::Committed,
        files: vec![],
    }
}

fn structure() -> ProjectStructure {
    let fp: SourceFingerprint = fingerprint(3).parse().expect("source fingerprint");
    ProjectStructure {
        workspace_members: vec![],
        workspace_default_members: vec![],
        packages: vec![],
        profiles: vec![],
        cargo_configuration: CargoConfiguration {
            project_config_policy: ProjectConfigPolicy::Rejected,
            frozen: true,
            offline: true,
            incremental: false,
            target_directory_ephemeral: true,
        },
        runtime: RuntimeIdentity {
            platform: "linux/arm64".into(),
            image_id: "sha256:runtime".into(),
            configuration_fingerprint: fingerprint(4).parse().expect("configuration"),
            execution_fingerprint: fingerprint(5).parse().expect("execution"),
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        },
        source_fingerprint: fp,
    }
}

fn registry(backend: Backend) -> ProjectRegistry<Backend, Generator, Clock> {
    ProjectRegistry::new(backend, Generator::default(), Clock::default(), 60, 8).expect("registry")
}

#[test]
fn plans_enforce_ttl_boundary_and_reject_backward_clock() {
    let clock = Clock::default();
    let mut plans = MutationPlans::default();
    let id = mutation_id(1);
    let digest: SourceFingerprint = fingerprint(1).parse().expect("digest");
    plans
        .remember(
            id.clone(),
            digest.clone(),
            "/workspace".into(),
            candidate(),
            &clock,
        )
        .expect("remember");

    clock.0.set(599);
    assert!(
        plans
            .resolve(
                &id,
                &digest,
                IdempotencyKey::new("at-599".into()).expect("key"),
                &clock,
            )
            .is_ok()
    );
    clock.0.set(600);
    assert!(matches!(
        plans.resolve(
            &id,
            &digest,
            IdempotencyKey::new("at-600".into()).expect("key"),
            &clock,
        ),
        Err(MutationError::Expired)
    ));
    clock.0.set(0);
    plans
        .remember(
            mutation_id(2),
            fingerprint(2).parse().expect("digest"),
            "/workspace".into(),
            candidate(),
            &clock,
        )
        .expect("remember after pruning expired plan");
    clock.0.set(10);
    let backward_id = mutation_id(3);
    plans
        .remember(
            backward_id.clone(),
            fingerprint(3).parse().expect("digest"),
            "/workspace".into(),
            candidate(),
            &clock,
        )
        .expect("remember at later time");
    clock.0.set(9);
    assert!(matches!(
        plans.resolve(
            &backward_id,
            &fingerprint(3).parse().expect("digest"),
            IdempotencyKey::new("backward".into()).expect("key"),
            &clock,
        ),
        Err(MutationError::Expired)
    ));
}

#[test]
fn plans_enforce_capacity_digest_binding_and_id_uniqueness() {
    let clock = Clock::default();
    let mut plans = MutationPlans::default();
    for number in 1..=4 {
        plans
            .remember(
                mutation_id(number),
                fingerprint(number as u8).parse().expect("digest"),
                "/workspace".into(),
                candidate(),
                &clock,
            )
            .expect("within capacity");
    }
    assert_eq!(
        plans.remember(
            mutation_id(5),
            fingerprint(5).parse().expect("digest"),
            "/workspace".into(),
            candidate(),
            &clock,
        ),
        Err(MutationError::LimitExceeded)
    );
    assert!(matches!(
        plans.resolve(
            &mutation_id(1),
            &fingerprint(9).parse().expect("digest"),
            IdempotencyKey::new("wrong-digest".into()).expect("key"),
            &clock,
        ),
        Err(MutationError::Conflict)
    ));

    let mut collision = MutationPlans::default();
    collision
        .remember(
            mutation_id(7),
            fingerprint(7).parse().expect("digest"),
            "/workspace".into(),
            candidate(),
            &clock,
        )
        .expect("first id");
    assert_eq!(
        collision.remember(
            mutation_id(7),
            fingerprint(8).parse().expect("digest"),
            "/other".into(),
            candidate(),
            &clock,
        ),
        Err(MutationError::Conflict)
    );
}

#[test]
fn plans_enforce_aggregate_candidate_bytes() {
    let clock = Clock::default();
    let mut plans = MutationPlans::default();
    for number in 1..=2 {
        plans
            .remember(
                mutation_id(number),
                fingerprint(number as u8).parse().expect("digest"),
                "/workspace".into(),
                maximum_candidate(),
                &clock,
            )
            .expect("exact aggregate bound");
    }
    assert_eq!(
        plans.remember(
            mutation_id(3),
            fingerprint(3).parse().expect("digest"),
            "/workspace".into(),
            candidate(),
            &clock,
        ),
        Err(MutationError::LimitExceeded)
    );
}

#[test]
fn preview_retention_restores_slot_and_byte_budgets_but_delivered_plans_resolve() {
    let clock = Clock::default();
    let digest = |number: u8| -> SourceFingerprint { fingerprint(number).parse().expect("digest") };

    // Four undelivered previews consume every slot while their guards live.
    let mut slot_plans = MutationPlans::default();
    let mut slot_guards = Vec::new();
    for number in 1..=4 {
        let guard = PreviewRetention::default();
        slot_plans
            .remember_revocable(
                mutation_id(number),
                digest(number as u8),
                "/workspace".into(),
                candidate(),
                &clock,
                guard.token(),
            )
            .expect("live preview within slot bound");
        slot_guards.push(guard);
    }
    assert_eq!(
        slot_plans.remember(
            mutation_id(5),
            digest(5),
            "/workspace".into(),
            candidate(),
            &clock,
        ),
        Err(MutationError::LimitExceeded)
    );
    drop(slot_guards);
    assert!(matches!(
        slot_plans.resolve(
            &mutation_id(1),
            &digest(1),
            IdempotencyKey::new("revoked-slot".into()).expect("key"),
            &clock,
        ),
        Err(MutationError::NotFound)
    ));
    for number in 5..=8 {
        slot_plans
            .remember(
                mutation_id(number),
                digest(number as u8),
                "/workspace".into(),
                candidate(),
                &clock,
            )
            .expect("revoked slots pruned before admission");
    }

    // Two maximum source candidates exactly occupy the independent 64 MiB cap.
    let mut byte_plans = MutationPlans::default();
    let first = PreviewRetention::default();
    let second = PreviewRetention::default();
    for (number, token) in [(11, first.token()), (12, second.token())] {
        byte_plans
            .remember_revocable(
                mutation_id(number),
                digest(number as u8),
                "/workspace".into(),
                maximum_candidate(),
                &clock,
                token,
            )
            .expect("exact aggregate byte bound");
    }
    assert_eq!(
        byte_plans.remember(
            mutation_id(13),
            digest(13),
            "/workspace".into(),
            candidate(),
            &clock,
        ),
        Err(MutationError::LimitExceeded)
    );
    drop((first, second));
    for number in 13..=14 {
        byte_plans
            .remember(
                mutation_id(number),
                digest(number as u8),
                "/workspace".into(),
                maximum_candidate(),
                &clock,
            )
            .expect("revoked bytes pruned before admission");
    }

    let mut delivered = MutationPlans::default();
    let retained = PreviewRetention::default();
    delivered
        .remember_revocable(
            mutation_id(21),
            digest(21),
            "/workspace".into(),
            candidate(),
            &clock,
            retained.token(),
        )
        .expect("remember delivered preview");
    retained.retain();
    assert!(
        delivered
            .resolve(
                &mutation_id(21),
                &digest(21),
                IdempotencyKey::new("delivered".into()).expect("key"),
                &clock,
            )
            .is_ok(),
        "a successfully delivered preview must remain committable"
    );
}

#[test]
fn preview_checks_expected_identity_and_authority_before_edit_or_oracle() {
    let control = Control::default();
    let backend = Backend::new(vec![source(Some(b"[workspace]\n"), b"source")]);
    let mut registry = registry(backend.clone());
    let opened = registry.open("/trusted/workspace", &control).expect("open");
    let editor = Editor::default();
    let inspector = Inspector::default();
    let publisher = Publisher::new();

    assert_eq!(
        registry.preview_manifest(
            &opened.project_ref,
            &fingerprint(9).parse().expect("stale fingerprint"),
            &manifest_edit(),
            &editor,
            &inspector,
            &publisher,
            &control,
        ),
        Err(MutationPreparationError::Mutation(MutationError::Conflict))
    );
    assert_eq!(publisher.authorize_leases.borrow().len(), 0);
    assert_eq!(editor.calls.get(), 0);
    assert_eq!(inspector.calls.get(), 0);
    assert_eq!(backend.source_calls.get(), 0);

    publisher
        .authorize_error
        .set(Some(MutationError::PermissionDenied));
    assert_eq!(
        registry.preview_manifest(
            &opened.project_ref,
            &opened.identity.fingerprint,
            &manifest_edit(),
            &editor,
            &inspector,
            &publisher,
            &control,
        ),
        Err(MutationPreparationError::Mutation(
            MutationError::PermissionDenied
        ))
    );
    assert_eq!(editor.calls.get(), 0);
    assert_eq!(inspector.calls.get(), 0);
    assert_eq!(backend.source_calls.get(), 0);
}

#[test]
fn preview_oracle_receives_candidate_and_only_root_manifest_changes() {
    let before = source(Some(b"[workspace]\n"), b"unchanged");
    let replacement = b"[workspace]\n[workspace.lints.rust]\nunsafe_code = \"deny\"\n".to_vec();
    let backend = Backend::new(vec![before.clone()]);
    let mut registry = registry(backend);
    let control = Control::default();
    let opened = registry.open("/trusted/workspace", &control).expect("open");
    let editor = Editor::returning(Ok(replacement.clone()));
    let inspector = Inspector::default();
    let publisher = Publisher::new();

    let (workspace, preview) = registry
        .preview_manifest(
            &opened.project_ref,
            &opened.identity.fingerprint,
            &manifest_edit(),
            &editor,
            &inspector,
            &publisher,
            &control,
        )
        .expect("preview");
    assert_eq!(workspace, "/trusted/workspace");
    assert_eq!(preview.before, before);
    assert_eq!(preview.after.directories(), preview.before.directories());
    assert_eq!(
        preview
            .after
            .files()
            .iter()
            .find(|file| file.path() == "Cargo.toml")
            .expect("root manifest")
            .bytes(),
        replacement
    );
    assert_eq!(
        preview
            .after
            .files()
            .iter()
            .find(|file| file.path() == "src/lib.rs")
            .expect("other source")
            .bytes(),
        b"unchanged"
    );
    assert_eq!(
        inspector.seen.borrow().as_slice(),
        std::slice::from_ref(&preview.after)
    );
    assert!(preview.validation.contains("m2-manifest-lints-v1"));
    assert_eq!(publisher.commits.get(), 0);
}

#[test]
fn preview_refuses_source_changed_after_oracle() {
    let before = source(Some(b"[workspace]\n"), b"one");
    let changed = source(Some(b"[workspace]\n"), b"two");
    let backend = Backend::new(vec![before, changed]);
    let mut registry = registry(backend);
    let control = Control::default();
    let opened = registry.open("/trusted/workspace", &control).expect("open");
    let editor = Editor::default();
    let inspector = Inspector::default();
    let publisher = Publisher::new();

    assert_eq!(
        registry.preview_manifest(
            &opened.project_ref,
            &opened.identity.fingerprint,
            &manifest_edit(),
            &editor,
            &inspector,
            &publisher,
            &control,
        ),
        Err(MutationPreparationError::Mutation(MutationError::Conflict))
    );
    assert_eq!(inspector.calls.get(), 1);
    assert_eq!(publisher.commits.get(), 0);
}

#[test]
fn absent_manifest_and_editor_or_oracle_failures_never_commit() {
    let control = Control::default();
    let publisher = Publisher::new();

    let mut no_manifest = registry(Backend::new(vec![source(None, b"source")]));
    let opened = no_manifest
        .open("/trusted/workspace", &control)
        .expect("open");
    let editor = Editor::default();
    let inspector = Inspector::default();
    assert_eq!(
        no_manifest.preview_manifest(
            &opened.project_ref,
            &opened.identity.fingerprint,
            &manifest_edit(),
            &editor,
            &inspector,
            &publisher,
            &control,
        ),
        Err(MutationPreparationError::Mutation(MutationError::Invalid))
    );
    assert_eq!(editor.calls.get(), 0);
    assert_eq!(inspector.calls.get(), 0);

    let mut edit_failure = registry(Backend::new(vec![source(
        Some(b"[workspace]\n"),
        b"source",
    )]));
    let opened = edit_failure
        .open("/trusted/workspace", &control)
        .expect("open");
    let editor = Editor::returning(Err(ManifestEditError::UnsupportedLayout));
    assert_eq!(
        edit_failure.preview_manifest(
            &opened.project_ref,
            &opened.identity.fingerprint,
            &manifest_edit(),
            &editor,
            &inspector,
            &publisher,
            &control,
        ),
        Err(MutationPreparationError::Edit(
            ManifestEditError::UnsupportedLayout
        ))
    );

    let mut oracle_failure = registry(Backend::new(vec![source(
        Some(b"[workspace]\n"),
        b"source",
    )]));
    let opened = oracle_failure
        .open("/trusted/workspace", &control)
        .expect("open");
    let inspector = Inspector::default();
    inspector.error.set(Some(InspectionError::InvalidMetadata));
    assert_eq!(
        oracle_failure.preview_manifest(
            &opened.project_ref,
            &opened.identity.fingerprint,
            &manifest_edit(),
            &Editor::default(),
            &inspector,
            &publisher,
            &control,
        ),
        Err(MutationPreparationError::Inspection(
            InspectionError::InvalidMetadata
        ))
    );
    assert_eq!(publisher.commits.get(), 0);
}

#[test]
fn commit_rejects_wrong_workspace_and_missing_authority_before_publisher_commit() {
    let backend = Backend::new(vec![source(Some(b"[workspace]\n"), b"source")]);
    let mut registry = registry(backend);
    let control = Control::default();
    let opened = registry.open("/trusted/workspace", &control).expect("open");
    let publisher = Publisher::new();

    assert_eq!(
        registry.commit_mutation(
            &opened.project_ref,
            "/caller/chosen",
            &request(),
            &publisher,
            &control,
        ),
        Err(MutationError::PermissionDenied)
    );
    assert_eq!(publisher.authorize_leases.borrow().len(), 0);
    assert_eq!(publisher.commits.get(), 0);

    publisher
        .authorize_error
        .set(Some(MutationError::PermissionDenied));
    assert_eq!(
        registry.commit_mutation(
            &opened.project_ref,
            "/trusted/workspace",
            &request(),
            &publisher,
            &control,
        ),
        Err(MutationError::PermissionDenied)
    );
    assert_eq!(publisher.commits.get(), 0);
}

#[test]
fn successful_or_recovery_required_commit_invalidates_reference() {
    for result in [Ok(receipt()), Err(MutationError::RecoveryRequired)] {
        let backend = Backend::new(vec![source(Some(b"[workspace]\n"), b"source")]);
        let mut registry = registry(backend);
        let control = Control::default();
        let opened = registry.open("/trusted/workspace", &control).expect("open");
        let publisher = Publisher::new();
        *publisher.commit_result.borrow_mut() = result;
        let _ = registry.commit_mutation(
            &opened.project_ref,
            "/trusted/workspace",
            &request(),
            &publisher,
            &control,
        );
        assert_eq!(
            registry.resolve(&opened.project_ref, &control),
            Err(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound
            ))
        );
    }
}

#[test]
fn cancelled_precommit_preserves_live_reference() {
    let backend = Backend::new(vec![source(Some(b"[workspace]\n"), b"source")]);
    let mut registry = registry(backend);
    let control = Control::default();
    let opened = registry.open("/trusted/workspace", &control).expect("open");
    let publisher = Publisher::new();
    *publisher.commit_result.borrow_mut() = Err(MutationError::Cancelled);

    assert_eq!(
        registry.commit_mutation(
            &opened.project_ref,
            "/trusted/workspace",
            &request(),
            &publisher,
            &control,
        ),
        Err(MutationError::Cancelled)
    );
    assert_eq!(
        registry.resolve(&opened.project_ref, &control),
        Ok(identity())
    );
}

#[test]
fn receipt_requires_reopened_live_authority_and_recover_is_explicit() {
    let backend = Backend::new(vec![source(Some(b"[workspace]\n"), b"source")]);
    let mut registry = registry(backend);
    let control = Control::default();
    let first = registry.open("/trusted/workspace", &control).expect("open");
    let publisher = Publisher::new();
    registry
        .commit_mutation(
            &first.project_ref,
            "/trusted/workspace",
            &request(),
            &publisher,
            &control,
        )
        .expect("commit");
    assert_eq!(
        registry.mutation_receipt(
            &first.project_ref,
            &mutation_id(1),
            false,
            &publisher,
            &control,
        ),
        Err(MutationError::PermissionDenied)
    );

    let reopened = registry
        .open("/trusted/workspace", &control)
        .expect("reopen");
    registry
        .mutation_receipt(
            &reopened.project_ref,
            &mutation_id(1),
            false,
            &publisher,
            &control,
        )
        .expect("receipt");
    registry
        .mutation_receipt(
            &reopened.project_ref,
            &mutation_id(1),
            true,
            &publisher,
            &control,
        )
        .expect("recover");

    assert_eq!(publisher.receipts.borrow().as_slice(), &[2]);
    assert_eq!(publisher.recoveries.borrow().as_slice(), &[2]);
    assert_eq!(publisher.authorize_leases.borrow().as_slice(), &[1, 2, 2]);
}

#[test]
fn captured_preview_does_not_hold_registry_during_validation_and_rechecks_generation() {
    let control = Control::default();
    let before = source(Some(b"[workspace]\n"), b"source");
    let changed = source(Some(b"[workspace]\n"), b"edited");
    let backend = Backend::new(vec![before, changed]);
    let mut registry = registry(backend);
    let opened = registry.open("/trusted/workspace", &control).expect("open");
    let prepared = registry
        .prepare_manifest(
            &opened.project_ref,
            &opened.identity.fingerprint,
            &manifest_edit(),
            &Editor::default(),
            &Publisher::new(),
            &control,
        )
        .expect("prepare");
    // Other authority operations can use the registry while the candidate owns its bytes.
    let other = registry
        .open("/trusted/workspace", &control)
        .expect("second open");
    assert_ne!(other.project_ref, opened.project_ref);
    let (_, candidate) = prepared
        .validate(&Inspector::default(), &control)
        .expect("validate");
    assert_eq!(
        registry.finish_manifest_preview(&opened.project_ref, &candidate, &control),
        Err(MutationPreparationError::Mutation(MutationError::Conflict))
    );
}

struct Mutator {
    expected_command: rust_engineering_domain::RustMutationCommand,
    candidate: SourceBundle,
    calls: Cell<usize>,
}
impl rust_engineering_application::ProjectMutationPort for Mutator {
    fn mutate(
        &self,
        _: &SourceBundle,
        command: rust_engineering_domain::RustMutationCommand,
        _: &dyn InspectionControl,
    ) -> Result<rust_engineering_domain::RustMutationObservation, InspectionError> {
        assert_eq!(command, self.expected_command);
        self.calls.set(self.calls.get() + 1);
        Ok(rust_engineering_domain::RustMutationObservation {
            candidate: self.candidate.clone(),
            runtime: structure().runtime,
            mutation_execution_fingerprint: fingerprint(6).parse().expect("mutation fingerprint"),
            candidate_source_fingerprint: fingerprint(7).parse().expect("source fingerprint"),
        })
    }
}

#[test]
fn rust_previews_check_authority_and_bind_operation_and_both_executions() {
    for (command, kind, version) in [
        (
            rust_engineering_domain::RustMutationCommand::Format,
            MutationKind::FormatApply,
            "m2-fmt-apply-v1",
        ),
        (
            rust_engineering_domain::RustMutationCommand::Fix,
            MutationKind::FixApply,
            "m2-fix-apply-v1",
        ),
    ] {
        let before = source(Some(b"[workspace]\n"), b"fn example( ){}\n");
        let after = source(Some(b"[workspace]\n"), b"fn example() {}\n");
        let backend = Backend::new(vec![before.clone()]);
        let mut registry = registry(backend.clone());
        let control = Control::default();
        let opened = registry.open("/trusted/workspace", &control).expect("open");
        let publisher = Publisher::new();
        publisher
            .authorize_error
            .set(Some(MutationError::PermissionDenied));
        assert!(matches!(
            registry.prepare_format(
                &opened.project_ref,
                &opened.identity.fingerprint,
                &publisher,
                &control
            ),
            Err(MutationPreparationError::Mutation(
                MutationError::PermissionDenied
            ))
        ));
        assert_eq!(backend.source_calls.get(), 0);
        publisher.authorize_error.set(None);
        let prepared = registry
            .prepare_format(
                &opened.project_ref,
                &opened.identity.fingerprint,
                &publisher,
                &control,
            )
            .expect("prepare format");
        let mutator = Mutator {
            expected_command: command.clone(),
            candidate: after.clone(),
            calls: Cell::new(0),
        };
        let (workspace, candidate) = prepared
            .validate_command(command, &mutator, &control)
            .expect("validated");
        assert_eq!(workspace, "/trusted/workspace");
        assert_eq!(candidate.before, before);
        assert_eq!(candidate.after, after);
        assert_eq!(candidate.kind, kind);
        assert!(candidate.validation.contains(version));
        assert!(candidate.validation.contains(&fingerprint(5)));
        assert!(candidate.validation.contains(&fingerprint(6)));
        assert_eq!(mutator.calls.get(), 1);
        registry
            .finish_manifest_preview(&opened.project_ref, &candidate, &control)
            .expect("unchanged live generation");
        assert_eq!(publisher.commits.get(), 0);
    }
}

#[test]
fn format_preview_rejects_changed_manifest_missing_files_and_late_external_changes() {
    let before = source(Some(b"[workspace]\n"), b"fn example( ){}\n");
    let control = Control::default();
    for after in [
        source(Some(b"[workspace]\nmembers=[]\n"), b"fn example() {}\n"),
        source(None, b"fn example() {}\n"),
    ] {
        let mut registry = registry(Backend::new(vec![before.clone()]));
        let opened = registry.open("/trusted/workspace", &control).expect("open");
        let prepared = registry
            .prepare_format(
                &opened.project_ref,
                &opened.identity.fingerprint,
                &Publisher::new(),
                &control,
            )
            .expect("prepare");
        let mutator = Mutator {
            expected_command: rust_engineering_domain::RustMutationCommand::Format,
            candidate: after,
            calls: Cell::new(0),
        };
        assert!(prepared.validate(&mutator, &control).is_err());
    }
    let changed = source(Some(b"[workspace]\n"), b"external edit\n");
    let mut registry = registry(Backend::new(vec![before, changed]));
    let opened = registry.open("/trusted/workspace", &control).expect("open");
    let prepared = registry
        .prepare_format(
            &opened.project_ref,
            &opened.identity.fingerprint,
            &Publisher::new(),
            &control,
        )
        .expect("prepare");
    let mutator = Mutator {
        expected_command: rust_engineering_domain::RustMutationCommand::Format,
        candidate: source(Some(b"[workspace]\n"), b"fn example() {}\n"),
        calls: Cell::new(0),
    };
    let (_, candidate) = prepared.validate(&mutator, &control).expect("validated");
    assert_eq!(
        registry.finish_manifest_preview(&opened.project_ref, &candidate, &control),
        Err(MutationPreparationError::Mutation(MutationError::Conflict))
    );
}

#[test]
fn terminal_plans_release_capacity_without_increasing_pending_quota() {
    let clock = Clock::default();
    let mut plans = MutationPlans::default();
    for number in 1..=12 {
        let id = mutation_id(number);
        let digest = fingerprint(number as u8).parse().expect("digest");
        plans
            .remember(id.clone(), digest, "/workspace".into(), candidate(), &clock)
            .expect("sequential admission");
        let digest = fingerprint(number as u8).parse().expect("digest");
        let plan = plans
            .resolve(
                &id,
                &digest,
                IdempotencyKey::new("same-key".into()).expect("key"),
                &clock,
            )
            .expect("resolve");
        let mut terminal = receipt();
        terminal.id = id.clone();
        terminal.digest = digest.clone();
        terminal.state = match number % 3 {
            0 => MutationState::Aborted,
            1 => MutationState::Committed,
            _ => MutationState::NoChange,
        };
        plan.retire_if_terminal(&terminal);
        assert!(matches!(
            plans.resolve(
                &id,
                &digest,
                IdempotencyKey::new("same-key".into()).expect("key"),
                &clock
            ),
            Err(MutationError::NotFound)
        ));
        assert_eq!(
            plans.allocation_stats().plans,
            1,
            "allocated terminal bytes await next admission"
        );
    }
    for number in 20..24 {
        plans
            .remember(
                mutation_id(number),
                fingerprint(number as u8).parse().expect("digest"),
                "/workspace".into(),
                candidate(),
                &clock,
            )
            .expect("four pending");
    }
    assert_eq!(plans.allocation_stats().plans, 4);
    assert_eq!(
        plans.remember(
            mutation_id(24),
            fingerprint(24).parse().expect("digest"),
            "/workspace".into(),
            candidate(),
            &clock
        ),
        Err(MutationError::LimitExceeded)
    );
}

#[test]
fn retirement_requires_exact_terminal_receipt_binding() {
    let clock = Clock::default();
    let mut plans = MutationPlans::default();
    let request = request();
    plans
        .remember(
            request.id.clone(),
            request.digest.clone(),
            "/workspace".into(),
            request.candidate.clone(),
            &clock,
        )
        .expect("remember");
    let plan = plans
        .resolve(&request.id, &request.digest, request.key.clone(), &clock)
        .expect("resolve");
    for wrong in 0..3 {
        let mut observed = receipt();
        match wrong {
            0 => observed.id = mutation_id(9),
            1 => observed.digest = fingerprint(9).parse().expect("digest"),
            _ => observed.state = MutationState::RecoveryRequired,
        }
        plan.retire_if_terminal(&observed);
        assert!(
            plans
                .resolve(&request.id, &request.digest, request.key.clone(), &clock)
                .is_ok()
        );
    }
    plan.retire_if_terminal(&receipt());
    assert!(matches!(
        plans.resolve(&request.id, &request.digest, request.key.clone(), &clock),
        Err(MutationError::NotFound)
    ));
}

#[test]
fn durable_replay_requires_live_authority_and_invalidates_after_result() {
    let backend = Backend::new(vec![candidate().after]);
    let mut registry = registry(backend.clone());
    let control = Control::default();
    let publisher = Publisher::new();
    let request = request();
    let opened = registry.open("/trusted/workspace", &control).expect("open");
    publisher
        .authorize_error
        .set(Some(MutationError::PermissionDenied));
    assert_eq!(
        registry.replay_mutation(
            &opened.project_ref,
            &request.id,
            &request.digest,
            &request.key,
            &publisher,
            &control
        ),
        Err(MutationError::PermissionDenied)
    );
    assert!(publisher.replays.borrow().is_empty());
    publisher.authorize_error.set(None);
    assert_eq!(
        registry.replay_mutation(
            &opened.project_ref,
            &request.id,
            &request.digest,
            &request.key,
            &publisher,
            &control
        ),
        Ok(receipt())
    );
    assert_eq!(publisher.replays.borrow().len(), 1);
    assert!(
        publisher.receipts.borrow().is_empty(),
        "replay is not receipt lookup"
    );
    assert_eq!(publisher.replays.borrow()[0].1, request.id);
    assert_eq!(publisher.replays.borrow()[0].2, request.digest);
    assert_eq!(publisher.replays.borrow()[0].3, request.key);
    assert_eq!(
        publisher.commits.get(),
        0,
        "replay never dispatches candidate commit"
    );
    assert_eq!(
        backend.source_calls.get(),
        0,
        "replay needs no source capture or Cargo validation"
    );
    assert_eq!(
        registry.replay_mutation(
            &opened.project_ref,
            &request.id,
            &request.digest,
            &request.key,
            &publisher,
            &control
        ),
        Err(MutationError::PermissionDenied)
    );
    assert_eq!(publisher.replays.borrow().len(), 1);
    let reopened = registry
        .open("/trusted/workspace", &control)
        .expect("reopen");
    control.0.store(true, Ordering::Relaxed);
    assert_eq!(
        registry.replay_mutation(
            &reopened.project_ref,
            &request.id,
            &request.digest,
            &request.key,
            &publisher,
            &control
        ),
        Err(MutationError::Cancelled)
    );
    assert_eq!(publisher.replays.borrow().len(), 1);
}
