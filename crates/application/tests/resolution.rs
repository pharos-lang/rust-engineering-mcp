#![allow(clippy::expect_used)]

use rust_engineering_application::{
    ExecutionCancellation, InspectionControl, InspectionError, ManifestEditor, MutationPublisher,
    OperationControl, ProjectBackend, ProjectError, ProjectIdentity, ProjectInspectionPort,
    ProjectRegistry, ProjectResolutionPort, ProjectSourceBackend, ReferenceGenerator,
    RegistryClock, ResolutionError, SemanticPreparationError, ValidatedProject,
};
use rust_engineering_domain::{
    CargoConfiguration, CargoVendorSnapshot, DependencyKind, DependencyName, DependencySpec,
    FeatureName, LintLevel, LintName, LintScope, LintTool, ManifestEdit, ManifestEditError,
    MutationCommit, MutationError, MutationId, MutationKind, MutationLockDisposition,
    MutationReceipt, MutationResolutionObservation, ProjectConfigPolicy,
    ProjectIdentityFingerprint, ProjectPackage, ProjectRef, ProjectStructure, RuntimeIdentity,
    RustEdition, SourceBundle, SourceFile, SourceFingerprint,
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
struct Clock;

impl RegistryClock for Clock {
    fn seconds(&self) -> u64 {
        0
    }
}

#[derive(Clone)]
struct Backend {
    source: SourceBundle,
    source_calls: Rc<Cell<usize>>,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl ProjectBackend for Backend {
    type Lease = u8;

    fn open(
        &self,
        _: &str,
        _: &dyn OperationControl,
    ) -> Result<ValidatedProject<Self::Lease>, ProjectError> {
        Ok(ValidatedProject {
            identity: identity(),
            lease: 1,
        })
    }

    fn revalidate(
        &self,
        _: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        self.events.borrow_mut().push("revalidate");
        Ok(identity())
    }
}

impl ProjectSourceBackend for Backend {
    fn source(
        &self,
        _: &Self::Lease,
        _: &dyn OperationControl,
    ) -> Result<SourceBundle, ProjectError> {
        self.events.borrow_mut().push("source");
        self.source_calls.set(self.source_calls.get() + 1);
        Ok(self.source.clone())
    }
}

#[derive(Default)]
struct Generator(Cell<u128>);

impl ReferenceGenerator for Generator {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        let value = self.0.get() + 1;
        self.0.set(value);
        format!("prj_{value:032x}")
            .parse()
            .map_err(|_| ProjectError::Internal)
    }
}

struct Publisher {
    error: Cell<Option<MutationError>>,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl MutationPublisher<u8> for Publisher {
    fn authorize(&self, _: &u8) -> Result<(), MutationError> {
        self.events.borrow_mut().push("authorize");
        self.error.get().map_or(Ok(()), Err)
    }

    fn commit(
        &self,
        _: &u8,
        _: &MutationCommit,
        _: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::Io)
    }

    fn replay(
        &self,
        _: &u8,
        _: &MutationId,
        _: &rust_engineering_domain::SourceFingerprint,
        _: &rust_engineering_domain::IdempotencyKey,
        _: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::Io)
    }

    fn receipt(&self, _: &u8, _: &MutationId) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::Io)
    }

    fn recover(&self, _: &u8, _: &MutationId) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::Io)
    }
}

struct Editor {
    replacement: Vec<u8>,
    calls: Cell<usize>,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl ManifestEditor for Editor {
    fn apply(&self, _: &[u8], _: &ManifestEdit) -> Result<Vec<u8>, ManifestEditError> {
        self.events.borrow_mut().push("edit");
        self.calls.set(self.calls.get() + 1);
        Ok(self.replacement.clone())
    }
}

struct Inspector {
    structure: ProjectStructure,
    calls: Cell<usize>,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl ProjectInspectionPort for Inspector {
    fn inspect(
        &self,
        _: &SourceBundle,
        _: &dyn InspectionControl,
    ) -> Result<ProjectStructure, InspectionError> {
        self.events.borrow_mut().push("inspect");
        self.calls.set(self.calls.get() + 1);
        Ok(self.structure.clone())
    }
}

struct Resolver {
    result: RefCell<Result<MutationResolutionObservation, ResolutionError>>,
    calls: Cell<usize>,
    seen: RefCell<Vec<SourceBundle>>,
    events: Rc<RefCell<Vec<&'static str>>>,
}

impl ProjectResolutionPort for Resolver {
    fn resolve(
        &self,
        edited: &SourceBundle,
        _: &CargoVendorSnapshot,
        _: &dyn InspectionControl,
    ) -> Result<MutationResolutionObservation, ResolutionError> {
        self.events.borrow_mut().push("resolve");
        self.calls.set(self.calls.get() + 1);
        self.seen.borrow_mut().push(edited.clone());
        self.result.borrow().clone()
    }
}

fn fingerprint(value: u8) -> SourceFingerprint {
    format!("sha256:{value:064x}").parse().expect("fingerprint")
}

fn identity() -> ProjectIdentity {
    ProjectIdentity {
        workspace_root: "/trusted/workspace".into(),
        fingerprint: fingerprint(1)
            .to_string()
            .parse::<ProjectIdentityFingerprint>()
            .expect("identity"),
    }
}

fn runtime() -> RuntimeIdentity {
    RuntimeIdentity {
        platform: "linux/aarch64".into(),
        image_id: fingerprint(2).to_string(),
        configuration_fingerprint: fingerprint(3).to_string().parse().expect("configuration"),
        execution_fingerprint: fingerprint(4)
            .to_string()
            .parse()
            .expect("frozen execution"),
        rust_version: "1.98.1".into(),
        cargo_version: "1.98.1".into(),
        declared_toolchain: None,
    }
}

fn structure(packages: &[(&str, bool)]) -> ProjectStructure {
    let mut members = Vec::new();
    let packages = packages
        .iter()
        .enumerate()
        .map(|(index, (manifest, member))| {
            let index = index as u32;
            if *member {
                members.push(index);
            }
            ProjectPackage {
                package_index: index,
                name: format!("package-{index}"),
                version: "0.1.0".into(),
                manifest_path: (*manifest).into(),
                edition: RustEdition::E2021,
                rust_version: None,
                targets: vec![],
                features: vec![],
                direct_dependencies: vec![],
            }
        })
        .collect();
    ProjectStructure {
        workspace_members: members.clone(),
        workspace_default_members: members,
        packages,
        profiles: vec![],
        cargo_configuration: CargoConfiguration {
            project_config_policy: ProjectConfigPolicy::Rejected,
            frozen: true,
            offline: true,
            incremental: false,
            target_directory_ephemeral: true,
        },
        runtime: runtime(),
        source_fingerprint: fingerprint(5),
    }
}

fn source(lock: Option<&[u8]>) -> SourceBundle {
    let mut files = vec![
        SourceFile::new("Cargo.toml".into(), b"[workspace]\n".to_vec()).expect("root manifest"),
        SourceFile::new(
            "crates/member/Cargo.toml".into(),
            b"[package]\nname='member'\nversion='0.1.0'\n".to_vec(),
        )
        .expect("member manifest"),
        SourceFile::new(
            "crates/other/Cargo.toml".into(),
            b"[package]\nname='other'\nversion='0.1.0'\n".to_vec(),
        )
        .expect("other manifest"),
        SourceFile::new(
            "crates/member/src/lib.rs".into(),
            b"pub fn value() {}\n".to_vec(),
        )
        .expect("source"),
    ];
    if let Some(bytes) = lock {
        files.push(SourceFile::new("Cargo.lock".into(), bytes.to_vec()).expect("lock"));
    }
    SourceBundle::with_directories(files, vec!["preserved-empty".into()]).expect("bundle")
}

fn replace_file(bundle: &SourceBundle, path: &str, bytes: &[u8]) -> SourceBundle {
    let files = bundle
        .files()
        .iter()
        .map(|file| {
            if file.path() == path {
                SourceFile::new(path.into(), bytes.to_vec()).expect("replacement")
            } else {
                file.clone()
            }
        })
        .collect();
    SourceBundle::with_directories(files, bundle.directories().to_vec())
        .expect("replacement bundle")
}

fn dataset() -> CargoVendorSnapshot {
    CargoVendorSnapshot {
        source: SourceBundle::new(vec![
            SourceFile::new("dep-1.0.0/Cargo.toml".into(), b"[package]\n".to_vec())
                .expect("vendor file"),
        ])
        .expect("vendor source"),
        tree_fingerprint: fingerprint(6),
        packages: vec![],
    }
}

fn observation(
    candidate: SourceBundle,
    disposition: MutationLockDisposition,
) -> MutationResolutionObservation {
    MutationResolutionObservation {
        candidate,
        runtime: runtime(),
        resolution_execution_fingerprint: fingerprint(7)
            .to_string()
            .parse()
            .expect("resolution execution"),
        dataset_fingerprint: fingerprint(6),
        resolved_lock_fingerprint: fingerprint(8),
        candidate_source_fingerprint: fingerprint(9),
        lock_disposition: disposition,
    }
}

fn dependency_add() -> ManifestEdit {
    ManifestEdit::DependencyAdd {
        kind: DependencyKind::Normal,
        target: None,
        name: DependencyName::new("dep".into()).expect("dependency"),
        spec: DependencySpec {
            requirement: "1".into(),
            package: None,
            features: vec![],
            optional: false,
            default_features: true,
        },
    }
}

fn lint_set() -> ManifestEdit {
    ManifestEdit::LintSet {
        scope: LintScope::Workspace,
        tool: LintTool::Rust,
        name: LintName::new("unsafe_code".into()).expect("lint"),
        level: LintLevel::Deny,
        priority: None,
    }
}

type Setup = (
    ProjectRegistry<Backend, Generator, Clock>,
    ProjectRef,
    Publisher,
    Rc<Cell<usize>>,
    Rc<RefCell<Vec<&'static str>>>,
);

fn setup(source: SourceBundle) -> Setup {
    let source_calls = Rc::new(Cell::new(0));
    let events = Rc::new(RefCell::new(vec![]));
    let backend = Backend {
        source,
        source_calls: source_calls.clone(),
        events: events.clone(),
    };
    let mut registry =
        ProjectRegistry::new(backend, Generator::default(), Clock, 60, 8).expect("registry");
    let opened = registry
        .open("/trusted/workspace", &Control::default())
        .expect("open");
    events.borrow_mut().clear();
    let publisher = Publisher {
        error: Cell::new(None),
        events: events.clone(),
    };
    (
        registry,
        opened.project_ref,
        publisher,
        source_calls,
        events,
    )
}

fn parse_frame(mut value: &str) -> Vec<String> {
    let mut fields = Vec::new();
    while !value.is_empty() {
        let colon = value.find(':').expect("length separator");
        let length: usize = value[..colon].parse().expect("field length");
        value = &value[colon + 1..];
        fields.push(value[..length].to_owned());
        value = &value[length..];
    }
    fields
}

#[test]
fn authorization_precedes_capture_and_denial_has_no_source_effect() {
    let (mut registry, reference, publisher, source_calls, events) = setup(source(None));
    publisher.error.set(Some(MutationError::PermissionDenied));
    let result = registry.prepare_semantic(
        &reference,
        &identity().fingerprint,
        "Cargo.toml",
        MutationKind::ManifestPatch,
        &lint_set(),
        &publisher,
        &Control::default(),
    );
    assert!(matches!(
        result,
        Err(SemanticPreparationError::Mutation(
            MutationError::PermissionDenied
        ))
    ));
    assert_eq!(source_calls.get(), 0);
    assert_eq!(events.borrow().as_slice(), ["revalidate", "authorize"]);
}

#[test]
fn prepare_rejects_open_kinds_mismatches_and_non_root_patch_before_capture() {
    let remove = ManifestEdit::DependencyRemove {
        kind: DependencyKind::Normal,
        target: None,
        name: DependencyName::new("dep".into()).expect("dependency"),
    };
    let cases = [
        (MutationKind::FormatApply, "Cargo.toml", lint_set()),
        (MutationKind::FixApply, "Cargo.toml", lint_set()),
        (MutationKind::DependencyAdd, "Cargo.toml", lint_set()),
        (MutationKind::ManifestPatch, "Cargo.toml", dependency_add()),
        (MutationKind::DependencyAdd, "Cargo.toml", remove),
        (
            MutationKind::DependencyRemove,
            "Cargo.toml",
            dependency_add(),
        ),
        (
            MutationKind::ManifestPatch,
            "crates/member/Cargo.toml",
            lint_set(),
        ),
        (
            MutationKind::DependencyAdd,
            "../Cargo.toml",
            dependency_add(),
        ),
        (MutationKind::DependencyAdd, "/Cargo.toml", dependency_add()),
        (
            MutationKind::DependencyAdd,
            "crates/member/not.toml",
            dependency_add(),
        ),
    ];
    for (kind, target, edit) in cases {
        let (mut registry, reference, publisher, source_calls, _) = setup(source(None));
        assert!(matches!(
            registry.prepare_semantic(
                &reference,
                &identity().fingerprint,
                target,
                kind,
                &edit,
                &publisher,
                &Control::default(),
            ),
            Err(SemanticPreparationError::Mutation(MutationError::Invalid))
        ));
        assert_eq!(source_calls.get(), 0);
    }
}

#[test]
fn virtual_workspace_root_is_not_a_default_dependency_member() {
    let before = source(None);
    let (mut registry, reference, publisher, _, events) = setup(before);
    let prepared = registry
        .prepare_semantic(
            &reference,
            &identity().fingerprint,
            "Cargo.toml",
            MutationKind::DependencyAdd,
            &dependency_add(),
            &publisher,
            &Control::default(),
        )
        .expect("prepare");
    let editor = Editor {
        replacement: b"edited".to_vec(),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let inspector = Inspector {
        structure: structure(&[("crates/member/Cargo.toml", true)]),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let resolver = Resolver {
        result: RefCell::new(Err(ResolutionError::Failed)),
        calls: Cell::new(0),
        seen: RefCell::new(vec![]),
        events,
    };
    assert!(matches!(
        prepared.validate(
            &editor,
            &inspector,
            &resolver,
            Some(&dataset()),
            &Control::default(),
        ),
        Err(SemanticPreparationError::Mutation(MutationError::Invalid))
    ));
    assert_eq!(editor.calls.get(), 0);
    assert_eq!(resolver.calls.get(), 0);
}

#[test]
fn dependency_non_member_is_rejected_before_edit_or_resolution() {
    let before = source(None);
    let (mut registry, reference, publisher, _, events) = setup(before);
    let prepared = registry
        .prepare_semantic(
            &reference,
            &identity().fingerprint,
            "crates/other/Cargo.toml",
            MutationKind::DependencyAdd,
            &dependency_add(),
            &publisher,
            &Control::default(),
        )
        .expect("prepare");
    let editor = Editor {
        replacement: b"edited".to_vec(),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let inspector = Inspector {
        structure: structure(&[
            ("crates/member/Cargo.toml", true),
            ("crates/other/Cargo.toml", false),
        ]),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let resolver = Resolver {
        result: RefCell::new(Err(ResolutionError::Failed)),
        calls: Cell::new(0),
        seen: RefCell::new(vec![]),
        events: events.clone(),
    };
    events.borrow_mut().clear();
    assert!(matches!(
        prepared.validate(
            &editor,
            &inspector,
            &resolver,
            Some(&dataset()),
            &Control::default(),
        ),
        Err(SemanticPreparationError::Mutation(
            MutationError::PermissionDenied
        ))
    ));
    assert_eq!(editor.calls.get(), 0);
    assert_eq!(resolver.calls.get(), 0);
    assert_eq!(events.borrow().as_slice(), ["inspect"]);
}

#[test]
fn dependency_resolution_requires_explicit_offline_data() {
    let before = source(None);
    let (mut registry, reference, publisher, _, events) = setup(before);
    let prepared = registry
        .prepare_semantic(
            &reference,
            &identity().fingerprint,
            "crates/member/Cargo.toml",
            MutationKind::DependencyAdd,
            &dependency_add(),
            &publisher,
            &Control::default(),
        )
        .expect("prepare");
    let editor = Editor {
        replacement: b"edited".to_vec(),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let inspector = Inspector {
        structure: structure(&[("crates/member/Cargo.toml", true)]),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let resolver = Resolver {
        result: RefCell::new(Err(ResolutionError::Failed)),
        calls: Cell::new(0),
        seen: RefCell::new(vec![]),
        events: events.clone(),
    };
    assert!(matches!(
        prepared.validate(&editor, &inspector, &resolver, None, &Control::default(),),
        Err(SemanticPreparationError::Resolution(
            ResolutionError::MissingOfflineData
        ))
    ));
    assert_eq!(editor.calls.get(), 0);
    assert_eq!(resolver.calls.get(), 0);
}

#[test]
fn existing_lock_resolution_preserves_scope_and_binds_fourteen_fields() {
    let before = source(Some(b"old lock"));
    let edited_manifest = b"[package]\nname='member'\nversion='0.1.0'\n[dependencies]\ndep='1'\n";
    let edited = replace_file(&before, "crates/member/Cargo.toml", edited_manifest);
    let candidate = replace_file(&edited, "Cargo.lock", b"resolved lock");
    let (mut registry, reference, publisher, _, events) = setup(before.clone());
    let prepared = registry
        .prepare_semantic(
            &reference,
            &identity().fingerprint,
            "crates/member/Cargo.toml",
            MutationKind::DependencyAdd,
            &dependency_add(),
            &publisher,
            &Control::default(),
        )
        .expect("prepare");
    let editor = Editor {
        replacement: edited_manifest.to_vec(),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let inspector = Inspector {
        structure: structure(&[("crates/member/Cargo.toml", true)]),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let resolver = Resolver {
        result: RefCell::new(Ok(observation(
            candidate.clone(),
            MutationLockDisposition::UpdatedExisting,
        ))),
        calls: Cell::new(0),
        seen: RefCell::new(vec![]),
        events: events.clone(),
    };
    events.borrow_mut().clear();
    let (workspace, result) = prepared
        .validate(
            &editor,
            &inspector,
            &resolver,
            Some(&dataset()),
            &Control::default(),
        )
        .expect("resolved candidate");
    assert_eq!(workspace, "/trusted/workspace");
    assert_eq!(result.kind, MutationKind::DependencyAdd);
    assert_eq!(result.before, before);
    assert_eq!(result.after, candidate);
    assert_eq!(resolver.seen.borrow().as_slice(), &[edited]);
    assert_eq!(events.borrow().as_slice(), ["inspect", "edit", "resolve"]);
    assert_eq!(
        parse_frame(&result.validation),
        vec![
            "m2-dependency-add-v1".to_owned(),
            "local_coordinated".to_owned(),
            "linux/aarch64".to_owned(),
            fingerprint(2).to_string(),
            fingerprint(3).to_string(),
            fingerprint(4).to_string(),
            "1.98.1".to_owned(),
            "1.98.1".to_owned(),
            fingerprint(9).to_string(),
            fingerprint(7).to_string(),
            fingerprint(6).to_string(),
            fingerprint(8).to_string(),
            "updated_existing".to_owned(),
            "crates/member/Cargo.toml".to_owned(),
        ]
    );
}

#[test]
fn absent_lock_remains_absent_and_noop_still_resolves() {
    let before = source(None);
    let (mut registry, reference, publisher, _, events) = setup(before.clone());
    let prepared = registry
        .prepare_semantic(
            &reference,
            &identity().fingerprint,
            "crates/member/Cargo.toml",
            MutationKind::DependencyRemove,
            &ManifestEdit::DependencyRemove {
                kind: DependencyKind::Normal,
                target: None,
                name: DependencyName::new("absent".into()).expect("dependency"),
            },
            &publisher,
            &Control::default(),
        )
        .expect("prepare");
    let member = before
        .files()
        .iter()
        .find(|file| file.path() == "crates/member/Cargo.toml")
        .expect("member");
    let editor = Editor {
        replacement: member.bytes().to_vec(),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let inspector = Inspector {
        structure: structure(&[("crates/member/Cargo.toml", true)]),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let resolver = Resolver {
        result: RefCell::new(Ok(observation(
            before.clone(),
            MutationLockDisposition::TransientUnpublished,
        ))),
        calls: Cell::new(0),
        seen: RefCell::new(vec![]),
        events,
    };
    let (_, result) = prepared
        .validate(
            &editor,
            &inspector,
            &resolver,
            Some(&dataset()),
            &Control::default(),
        )
        .expect("no-op still resolves");
    assert_eq!(resolver.calls.get(), 1);
    assert_eq!(result.before, before);
    assert_eq!(result.after, before);
    assert!(
        !result
            .after
            .files()
            .iter()
            .any(|file| file.path() == "Cargo.lock")
    );
    let fields = parse_frame(&result.validation);
    assert_eq!(fields.len(), 14);
    assert_eq!(fields[0], "m2-dependency-remove-v1");
    assert_eq!(fields[12], "transient_unpublished");
}

#[test]
fn resolver_cannot_change_other_source_or_lie_about_lock_or_dataset() {
    enum Case {
        OtherSource,
        WrongDisposition,
        WrongDataset,
    }
    for case in [
        Case::OtherSource,
        Case::WrongDisposition,
        Case::WrongDataset,
    ] {
        let before = source(None);
        let edited_manifest = b"edited";
        let edited = replace_file(&before, "crates/member/Cargo.toml", edited_manifest);
        let candidate = if matches!(case, Case::OtherSource) {
            replace_file(&edited, "crates/member/src/lib.rs", b"changed")
        } else {
            edited
        };
        let disposition = if matches!(case, Case::WrongDisposition) {
            MutationLockDisposition::UpdatedExisting
        } else {
            MutationLockDisposition::TransientUnpublished
        };
        let mut observed = observation(candidate, disposition);
        if matches!(case, Case::WrongDataset) {
            observed.dataset_fingerprint = fingerprint(42);
        }
        let (mut registry, reference, publisher, _, events) = setup(before);
        let prepared = registry
            .prepare_semantic(
                &reference,
                &identity().fingerprint,
                "crates/member/Cargo.toml",
                MutationKind::DependencyAdd,
                &dependency_add(),
                &publisher,
                &Control::default(),
            )
            .expect("prepare");
        let editor = Editor {
            replacement: edited_manifest.to_vec(),
            calls: Cell::new(0),
            events: events.clone(),
        };
        let inspector = Inspector {
            structure: structure(&[("crates/member/Cargo.toml", true)]),
            calls: Cell::new(0),
            events: events.clone(),
        };
        let resolver = Resolver {
            result: RefCell::new(Ok(observed)),
            calls: Cell::new(0),
            seen: RefCell::new(vec![]),
            events,
        };
        let result = prepared.validate(
            &editor,
            &inspector,
            &resolver,
            Some(&dataset()),
            &Control::default(),
        );
        match case {
            Case::OtherSource => assert!(matches!(
                result,
                Err(SemanticPreparationError::Mutation(
                    MutationError::PermissionDenied
                ))
            )),
            Case::WrongDisposition => assert!(matches!(
                result,
                Err(SemanticPreparationError::Mutation(MutationError::Invalid))
            )),
            Case::WrongDataset => assert!(matches!(
                result,
                Err(SemanticPreparationError::Resolution(
                    ResolutionError::InvalidOfflineData
                ))
            )),
        }
    }
}

#[test]
fn lint_uses_frozen_inspection_and_semantic_nine_field_validation() {
    let before = source(None);
    let edited = replace_file(
        &before,
        "Cargo.toml",
        b"[workspace]\n[workspace.lints.rust]\nunsafe_code='deny'\n",
    );
    let (mut registry, reference, publisher, _, events) = setup(before.clone());
    let prepared = registry
        .prepare_semantic(
            &reference,
            &identity().fingerprint,
            "Cargo.toml",
            MutationKind::ManifestPatch,
            &lint_set(),
            &publisher,
            &Control::default(),
        )
        .expect("prepare");
    let root = edited
        .files()
        .iter()
        .find(|file| file.path() == "Cargo.toml")
        .expect("root");
    let editor = Editor {
        replacement: root.bytes().to_vec(),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let inspector = Inspector {
        structure: structure(&[]),
        calls: Cell::new(0),
        events: events.clone(),
    };
    let resolver = Resolver {
        result: RefCell::new(Err(ResolutionError::Failed)),
        calls: Cell::new(0),
        seen: RefCell::new(vec![]),
        events,
    };
    let (_, result) = prepared
        .validate(&editor, &inspector, &resolver, None, &Control::default())
        .expect("semantic validation");
    assert_eq!(result.before, before);
    assert_eq!(result.after, edited);
    assert_eq!(inspector.calls.get(), 1);
    assert_eq!(resolver.calls.get(), 0);
    let fields = parse_frame(&result.validation);
    assert_eq!(fields.len(), 9);
    assert_eq!(fields[0], "m2-manifest-semantic-v1");
    assert_eq!(fields[8], fingerprint(5).to_string());
}

#[test]
fn feature_and_workspace_dependency_patches_require_resolution() {
    let edits = [
        ManifestEdit::FeatureSet {
            name: FeatureName::new("feature".into()).expect("feature"),
            values: vec![],
        },
        ManifestEdit::WorkspaceDependencyRemove {
            name: DependencyName::new("dep".into()).expect("dependency"),
        },
    ];
    for edit in edits {
        let before = source(None);
        let root = before
            .files()
            .iter()
            .find(|file| file.path() == "Cargo.toml")
            .expect("root")
            .bytes()
            .to_vec();
        let (mut registry, reference, publisher, _, events) = setup(before);
        let prepared = registry
            .prepare_semantic(
                &reference,
                &identity().fingerprint,
                "Cargo.toml",
                MutationKind::ManifestPatch,
                &edit,
                &publisher,
                &Control::default(),
            )
            .expect("prepare");
        let editor = Editor {
            replacement: root,
            calls: Cell::new(0),
            events: events.clone(),
        };
        let inspector = Inspector {
            structure: structure(&[]),
            calls: Cell::new(0),
            events: events.clone(),
        };
        let resolver = Resolver {
            result: RefCell::new(Err(ResolutionError::Failed)),
            calls: Cell::new(0),
            seen: RefCell::new(vec![]),
            events,
        };
        assert!(matches!(
            prepared.validate(&editor, &inspector, &resolver, None, &Control::default(),),
            Err(SemanticPreparationError::Resolution(
                ResolutionError::MissingOfflineData
            ))
        ));
        assert_eq!(inspector.calls.get(), 0);
        assert_eq!(resolver.calls.get(), 0);
    }
}

#[test]
fn missing_target_manifest_is_rejected_after_authorized_capture() {
    let (mut registry, reference, publisher, source_calls, events) = setup(source(None));
    assert!(matches!(
        registry.prepare_semantic(
            &reference,
            &identity().fingerprint,
            "missing/Cargo.toml",
            MutationKind::DependencyAdd,
            &dependency_add(),
            &publisher,
            &Control::default(),
        ),
        Err(SemanticPreparationError::Mutation(MutationError::Invalid))
    ));
    assert_eq!(source_calls.get(), 1);
    let events = events.borrow();
    let authorize = events
        .iter()
        .position(|event| *event == "authorize")
        .expect("authorize");
    let source = events
        .iter()
        .position(|event| *event == "source")
        .expect("source");
    assert!(authorize < source);
}

#[test]
fn identity_conflict_precedes_grant_and_capture() {
    let (mut registry, reference, publisher, source_calls, events) = setup(source(None));
    let wrong: ProjectIdentityFingerprint = fingerprint(99).to_string().parse().expect("identity");
    assert!(matches!(
        registry.prepare_semantic(
            &reference,
            &wrong,
            "Cargo.toml",
            MutationKind::ManifestPatch,
            &lint_set(),
            &publisher,
            &Control::default(),
        ),
        Err(SemanticPreparationError::Mutation(MutationError::Conflict))
    ));
    assert_eq!(source_calls.get(), 0);
    assert!(!events.borrow().contains(&"authorize"));
}
