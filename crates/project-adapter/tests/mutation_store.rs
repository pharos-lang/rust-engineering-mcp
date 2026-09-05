#![cfg(target_os = "macos")]

use rust_engineering_application::{
    OperationControl, ProjectBackend, ProjectError, ProjectSourceBackend,
};
use rust_engineering_domain::{
    IdempotencyKey, MutationCandidate, MutationCommit, MutationError, MutationId, MutationKind,
    MutationState, SourceBundle, SourceFile,
};
use rust_engineering_project::{
    SecureProjects,
    mutation_store::{NativeMutationStore, mutation_digest},
    prepare_mutation_state,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

type TestResult<T> = Result<T, String>;
macro_rules! ck {
    ($value:expr) => {
        $value.map_err(|error| format!("{error:?}"))?
    };
}

struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}

const LOCK_HELPER_STATE: &str = "RUST_MCP_TEST_LOCK_STATE";
const LOCK_HELPER_READY: &str = "RUST_MCP_TEST_LOCK_READY";
const LOCK_HELPER_RELEASE: &str = "RUST_MCP_TEST_LOCK_RELEASE";

#[test]
fn external_process_lock_helper() -> TestResult<()> {
    use rustix::fs::{CWD, FlockOperation, Mode, OFlags, flock, openat};
    let Some(state) = std::env::var_os(LOCK_HELPER_STATE) else {
        return Ok(());
    };
    let ready = PathBuf::from(std::env::var_os(LOCK_HELPER_READY).ok_or("ready")?);
    let release = PathBuf::from(std::env::var_os(LOCK_HELPER_RELEASE).ok_or("release")?);
    let lock = ck!(openat(
        CWD,
        PathBuf::from(state).join("mutation-store.lock"),
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty()
    ));
    ck!(flock(&lock, FlockOperation::NonBlockingLockExclusive));
    ck!(fs::write(&ready, b"ready"));
    for _ in 0..500 {
        if release.exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err("lock helper release timeout".to_owned())
}

struct Fixture {
    base: PathBuf,
    project: PathBuf,
    state: PathBuf,
}
impl Fixture {
    fn new(label: &str) -> TestResult<Self> {
        let mut random = [0_u8; 16];
        ck!(getrandom::fill(&mut random));
        let base = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-mut-{label}-{:032x}",
            u128::from_le_bytes(random)
        ));
        let project = base.join("project");
        let state = base.join("state");
        ck!(fs::create_dir_all(project.join("src")));
        ck!(fs::create_dir(&state));
        ck!(fs::set_permissions(
            &state,
            fs::Permissions::from_mode(0o700)
        ));
        ck!(fs::write(
            project.join("Cargo.toml"),
            b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
        ));
        ck!(fs::write(
            project.join("src/lib.rs"),
            b"pub fn answer() -> u8 { 42 }\n"
        ));
        Ok(Self {
            base,
            project,
            state,
        })
    }
    fn backend(&self) -> TestResult<SecureProjects> {
        Ok(ck!(SecureProjects::new(std::slice::from_ref(
            &self.project
        ))))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn changed(bundle: &SourceBundle) -> TestResult<SourceBundle> {
    let files = ck!(bundle
        .files()
        .iter()
        .map(|file| {
            SourceFile::new(
                file.path().to_owned(),
                if file.path() == "Cargo.toml" {
                    b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lints.rust]\nunsafe_code = \"forbid\"\n".to_vec()
                } else {
                    file.bytes().to_vec()
                },
            )
        })
        .collect::<Result<Vec<_>, _>>());
    Ok(ck!(SourceBundle::with_directories(
        files,
        bundle.directories().to_vec()
    )))
}

fn request(before: SourceBundle, suffix: u128, key: &str) -> TestResult<MutationCommit> {
    let candidate = MutationCandidate {
        kind: MutationKind::ManifestPatch,
        after: changed(&before)?,
        before,
        validation: "toml_edit=0.25.13;cargo=1.98.1;operation=lints".to_owned(),
    };
    Ok(MutationCommit {
        id: ck!(MutationId::new(format!("mut_{suffix:032x}"))),
        digest: ck!(mutation_digest(&candidate)),
        key: ck!(IdempotencyKey::new(key.to_owned())),
        candidate,
    })
}

fn format_request(before: SourceBundle, suffix: u128, key: &str) -> TestResult<MutationCommit> {
    let files = ck!(before
        .files()
        .iter()
        .map(|file| {
            let bytes = match file.path() {
                "src/lib.rs" => b"pub fn answer() -> u8 {\n    42\n}\n".to_vec(),
                "src/other.rs" => b"pub fn other() -> u8 {\n    7\n}\n".to_vec(),
                _ => file.bytes().to_vec(),
            };
            SourceFile::new(file.path().to_owned(), bytes)
        })
        .collect::<Result<Vec<_>, _>>());
    let after = ck!(SourceBundle::with_directories(
        files,
        before.directories().to_vec()
    ));
    let candidate = MutationCandidate {
        kind: MutationKind::FormatApply,
        before,
        after,
        validation: "rustfmt=1.8.0;toolchain=1.98.1;operation=fmt".to_owned(),
    };
    Ok(MutationCommit {
        id: ck!(MutationId::new(format!("mut_{suffix:032x}"))),
        digest: ck!(mutation_digest(&candidate)),
        key: ck!(IdempotencyKey::new(key.to_owned())),
        candidate,
    })
}

fn mutation_request(
    before: SourceBundle,
    kind: MutationKind,
    suffix: u128,
    key: &str,
    replacements: &[(&str, &[u8])],
) -> TestResult<MutationCommit> {
    let files = ck!(before
        .files()
        .iter()
        .map(|file| {
            let bytes = replacements
                .iter()
                .find(|(path, _)| *path == file.path())
                .map_or_else(|| file.bytes().to_vec(), |(_, bytes)| bytes.to_vec());
            SourceFile::new(file.path().to_owned(), bytes)
        })
        .collect::<Result<Vec<_>, _>>());
    let after = ck!(SourceBundle::with_directories(
        files,
        before.directories().to_vec()
    ));
    let candidate = MutationCandidate {
        kind,
        before,
        after,
        validation: format!("native-semantic-delta:{kind:?}"),
    };
    Ok(MutationCommit {
        id: ck!(MutationId::new(format!("mut_{suffix:032x}"))),
        digest: ck!(mutation_digest(&candidate)),
        key: ck!(IdempotencyKey::new(key.to_owned())),
        candidate,
    })
}

#[test]
fn manifest_patch_commits_root_manifest_and_existing_lock_as_one_plan() -> TestResult<()> {
    let fixture = Fixture::new("manifest-lock")?;
    ck!(fs::write(
        fixture.project.join("Cargo.lock"),
        b"# lock before\nversion = 4\n"
    ));
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let manifest = b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[features]\ndefault = [\"dep:serde\"]\n";
    let lock = b"# lock after\nversion = 4\n";
    let commit = mutation_request(
        before,
        MutationKind::ManifestPatch,
        201,
        "manifest-lock-key",
        &[("Cargo.toml", manifest), ("Cargo.lock", lock)],
    )?;
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    let receipt = ck!(store.commit(&opened.lease, &commit, &Continue));
    assert_eq!(receipt.state, MutationState::Committed);
    assert_eq!(
        receipt
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["Cargo.lock", "Cargo.toml"]
    );
    assert_eq!(ck!(fs::read(fixture.project.join("Cargo.toml"))), manifest);
    assert_eq!(ck!(fs::read(fixture.project.join("Cargo.lock"))), lock);
    Ok(())
}

#[test]
fn dependency_add_commits_one_member_manifest_and_root_lock_under_its_kind() -> TestResult<()> {
    let fixture = Fixture::new("dependency-member")?;
    ck!(fs::write(
        fixture.project.join("Cargo.toml"),
        b"[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
    ));
    ck!(fs::create_dir_all(fixture.project.join("member/src")));
    ck!(fs::write(
        fixture.project.join("member/Cargo.toml"),
        b"[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
    ));
    ck!(fs::write(
        fixture.project.join("member/src/lib.rs"),
        b"pub fn member() {}\n"
    ));
    ck!(fs::write(
        fixture.project.join("Cargo.lock"),
        b"# lock before\nversion = 4\n"
    ));
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let member = b"[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[target.'cfg(unix)'.build-dependencies]\nsystem_api = { version = \"1\", package = \"libc\", features = [\"extra_traits\"], optional = true, default-features = false }\n";
    let lock = b"# lock after add\nversion = 4\n";
    let commit = mutation_request(
        before,
        MutationKind::DependencyAdd,
        202,
        "dependency-member-key",
        &[("member/Cargo.toml", member), ("Cargo.lock", lock)],
    )?;
    let wrong_kind = ck!(NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::DependencyRemove
    ));
    assert_eq!(
        wrong_kind.commit(&opened.lease, &commit, &Continue),
        Err(MutationError::PermissionDenied)
    );
    let store = ck!(NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::DependencyAdd
    ));
    let receipt = ck!(store.commit(&opened.lease, &commit, &Continue));
    assert_eq!(receipt.state, MutationState::Committed);
    assert_eq!(
        receipt
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["Cargo.lock", "member/Cargo.toml"]
    );
    assert_eq!(
        ck!(fs::read(fixture.project.join("member/Cargo.toml"))),
        member
    );
    Ok(())
}

#[test]
fn dependency_remove_only_drops_the_selected_local_key() -> TestResult<()> {
    let fixture = Fixture::new("dependency-remove")?;
    let before_manifest = b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde = { workspace = true, features = [\"derive\"] }\nkeep = \"1\"\n[features]\ndefault = [\"serde/std\"]\n[workspace]\n[workspace.dependencies]\nserde = \"1\"\n";
    ck!(fs::write(
        fixture.project.join("Cargo.toml"),
        before_manifest
    ));
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let after_manifest = b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nkeep = \"1\"\n[features]\ndefault = [\"serde/std\"]\n[workspace]\n[workspace.dependencies]\nserde = \"1\"\n";
    let commit = mutation_request(
        before,
        MutationKind::DependencyRemove,
        203,
        "dependency-remove-key",
        &[("Cargo.toml", after_manifest)],
    )?;
    let store = ck!(NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::DependencyRemove
    ));
    assert_eq!(
        ck!(store.commit(&opened.lease, &commit, &Continue)).state,
        MutationState::Committed
    );
    assert_eq!(
        ck!(fs::read(fixture.project.join("Cargo.toml"))),
        after_manifest
    );
    Ok(())
}

#[test]
fn dependency_no_op_may_publish_only_an_existing_root_lock() -> TestResult<()> {
    let fixture = Fixture::new("dependency-lock-noop")?;
    ck!(fs::write(
        fixture.project.join("Cargo.lock"),
        b"# old lock\nversion = 4\n"
    ));
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let lock = b"# resolved lock\nversion = 4\n";
    let commit = mutation_request(
        before,
        MutationKind::DependencyAdd,
        204,
        "dependency-lock-noop-key",
        &[("Cargo.lock", lock)],
    )?;
    let store = ck!(NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::DependencyAdd
    ));
    let receipt = ck!(store.commit(&opened.lease, &commit, &Continue));
    assert_eq!(receipt.state, MutationState::Committed);
    assert_eq!(receipt.files.len(), 1);
    assert_eq!(receipt.files[0].path, "Cargo.lock");
    assert_eq!(ck!(fs::read(fixture.project.join("Cargo.lock"))), lock);
    Ok(())
}

#[test]
fn forged_dependency_adds_leave_source_and_journal_untouched() -> TestResult<()> {
    let fixture = Fixture::new("dependency-forgeries")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let original_manifest = ck!(fs::read(fixture.project.join("Cargo.toml")));
    let original_source = ck!(fs::read(fixture.project.join("src/lib.rs")));
    let store = ck!(NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::DependencyAdd
    ));
    let cases: Vec<Vec<(&str, &[u8])>> = vec![
        vec![(
            "Cargo.toml",
            b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nevil = { path = \"../evil\" }\n",
        )],
        vec![(
            "Cargo.toml",
            b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nevil = { git = \"https://example.invalid/evil\" }\n",
        )],
        vec![(
            "Cargo.toml",
            b"[package]\nname = \"changed\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde = \"1\"\n",
        )],
        vec![(
            "Cargo.toml",
            b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde = \"1\"\nregex = \"1\"\n",
        )],
        vec![
            (
                "Cargo.toml",
                b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde = \"1\"\n",
            ),
            ("src/lib.rs", b"pub fn forged() {}\n"),
        ],
    ];
    for (index, replacements) in cases.iter().enumerate() {
        let commit = mutation_request(
            before.clone(),
            MutationKind::DependencyAdd,
            210 + index as u128,
            &format!("dependency-forgery-{index}"),
            replacements,
        )?;
        assert_eq!(
            store.commit(&opened.lease, &commit, &Continue),
            Err(MutationError::Invalid),
            "case {index}"
        );
        assert_eq!(
            ck!(fs::read(fixture.project.join("Cargo.toml"))),
            original_manifest
        );
        assert_eq!(
            ck!(fs::read(fixture.project.join("src/lib.rs"))),
            original_source
        );
        assert!(ck!(store.list_records()).is_empty());
    }
    Ok(())
}

#[test]
fn dependency_add_rejects_changes_to_two_member_manifests() -> TestResult<()> {
    let fixture = Fixture::new("dependency-two-manifests")?;
    ck!(fs::write(
        fixture.project.join("Cargo.toml"),
        b"[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
    ));
    ck!(fs::create_dir_all(fixture.project.join("member/src")));
    ck!(fs::write(
        fixture.project.join("member/Cargo.toml"),
        b"[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
    ));
    ck!(fs::write(fixture.project.join("member/src/lib.rs"), b""));
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let root = b"[workspace]\nmembers = [\"member\"]\nresolver = \"3\"\n[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde = \"1\"\n";
    let member = b"[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nserde = \"1\"\n";
    let commit = mutation_request(
        before,
        MutationKind::DependencyAdd,
        220,
        "dependency-two-manifests-key",
        &[("Cargo.toml", root), ("member/Cargo.toml", member)],
    )?;
    let store = ck!(NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::DependencyAdd
    ));
    assert_eq!(
        store.commit(&opened.lease, &commit, &Continue),
        Err(MutationError::Invalid)
    );
    assert!(ck!(store.list_records()).is_empty());
    Ok(())
}

#[test]
fn rust_mutation_commit_replaces_nested_files_and_separates_all_operation_grants() -> TestResult<()>
{
    for kind in [MutationKind::FormatApply, MutationKind::FixApply] {
        let fixture = Fixture::new("rust-mutation-happy")?;
        ck!(fs::write(
            fixture.project.join("src/other.rs"),
            b"pub fn other()->u8{7}\n"
        ));
        let backend = fixture.backend()?;
        let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
        let before = ck!(backend.source(&opened.lease, &Continue));
        let mut commit = format_request(before, 90, "rust-mutation-happy-key")?;
        commit.candidate.kind = kind;
        commit.digest = ck!(mutation_digest(&commit.candidate));
        let manifest = ck!(NativeMutationStore::open(
            &fixture.state,
            std::slice::from_ref(&fixture.project)
        ));
        assert_eq!(
            manifest.commit(&opened.lease, &commit, &Continue),
            Err(MutationError::PermissionDenied)
        );
        let format = ck!(NativeMutationStore::open_for_kind(
            &fixture.state,
            std::slice::from_ref(&fixture.project),
            kind
        ));
        let other = ck!(NativeMutationStore::open_for_kind(
            &fixture.state,
            std::slice::from_ref(&fixture.project),
            if kind == MutationKind::FormatApply {
                MutationKind::FixApply
            } else {
                MutationKind::FormatApply
            }
        ));
        assert_eq!(
            other.commit(&opened.lease, &commit, &Continue),
            Err(MutationError::PermissionDenied)
        );
        let receipt = ck!(format.commit(&opened.lease, &commit, &Continue));
        assert_eq!(receipt.state, MutationState::Committed);
        assert_eq!(
            receipt
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            vec!["src/lib.rs", "src/other.rs"]
        );
        assert!(
            receipt
                .files
                .iter()
                .all(|file| file.effect_after == Some(file.after.clone()))
        );
        assert_eq!(
            manifest.receipt(&opened.lease, &commit.id),
            Err(MutationError::PermissionDenied)
        );
        assert_eq!(
            format.commit(&opened.lease, &commit, &Continue),
            Ok(receipt.clone())
        );
        assert_eq!(
            other.receipt(&opened.lease, &commit.id),
            Err(MutationError::PermissionDenied)
        );
        assert_eq!(
            other.recover(&opened.lease, &commit.id),
            Err(MutationError::PermissionDenied)
        );
        let operator = ck!(NativeMutationStore::open(&fixture.state, &[]));
        let records = ck!(operator.list_records());
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].id, commit.id);
        assert_eq!(records[0].digest, commit.digest);
        ck!(operator.prune_record(&commit.id, &commit.digest));
        assert!(ck!(operator.list_records()).is_empty());
        assert!(!ck!(fs::read_dir(&fixture.project)).any(|entry| {
            entry
                .ok()
                .and_then(|entry| entry.file_name().into_string().ok())
                .is_some_and(|name| name.starts_with(".rust-mcp-mut-"))
        }));
    }
    Ok(())
}

#[test]
fn commit_replay_receipt_and_reopened_lease_are_bound_to_workspace() -> TestResult<()> {
    let fixture = Fixture::new("happy")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let commit = request(before, 1, "happy-key")?;
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    let first = ck!(store.commit(&opened.lease, &commit, &Continue));
    assert_eq!(first.state, MutationState::Committed);
    assert_eq!(
        first.files[0].effect_after,
        Some(first.files[0].after.clone())
    );
    assert_eq!(
        first.files[0].effect_after_bytes,
        Some(first.files[0].after_bytes)
    );
    assert_eq!(
        store.commit(&opened.lease, &commit, &Continue),
        Ok(first.clone())
    );
    assert_eq!(
        store.replay(
            &opened.lease,
            &commit.id,
            &commit.digest,
            &commit.key,
            &Continue
        ),
        Ok(first.clone())
    );
    let reopened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    assert_eq!(
        store.receipt(&reopened.lease, &commit.id),
        Ok(first.clone())
    );
    assert_eq!(store.recover(&reopened.lease, &commit.id), Ok(first));
    assert!(
        ck!(fs::read(fixture.project.join("Cargo.toml"))).ends_with(b"unsafe_code = \"forbid\"\n")
    );
    assert!(!ck!(fs::read_dir(&fixture.project)).any(|entry| {
        entry
            .ok()
            .and_then(|entry| entry.file_name().into_string().ok())
            .is_some_and(|name| name.starts_with(".rust-mcp-mut-"))
    }));
    Ok(())
}

#[test]
fn oversized_editor_output_and_ambiguous_state_parent_are_rejected_early() -> TestResult<()> {
    assert_eq!(
        prepare_mutation_state(std::path::Path::new("/private/tmp//rust-mcp-invalid")),
        Err(MutationError::Invalid)
    );
    let fixture = Fixture::new("editor-limit")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let mut manifest = b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lints.rust]\nunsafe_code = \"forbid\"\n#".to_vec();
    manifest.resize(256 * 1024, b'x');
    manifest.push(b'\n');
    let files = ck!(before
        .files()
        .iter()
        .map(|file| SourceFile::new(
            file.path().to_owned(),
            if file.path() == "Cargo.toml" {
                manifest.clone()
            } else {
                file.bytes().to_vec()
            }
        ))
        .collect::<Result<Vec<_>, _>>());
    let after = ck!(SourceBundle::with_directories(
        files,
        before.directories().to_vec()
    ));
    let candidate = MutationCandidate {
        kind: MutationKind::ManifestPatch,
        before: before.clone(),
        after,
        validation: "editor-limit".to_owned(),
    };
    let commit = MutationCommit {
        id: ck!(MutationId::new(
            "mut_00000000000000000000000000000006".to_owned()
        )),
        digest: ck!(mutation_digest(&candidate)),
        key: ck!(IdempotencyKey::new("editor-limit".to_owned())),
        candidate,
    };
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    assert_eq!(
        store.commit(&opened.lease, &commit, &Continue),
        Err(MutationError::LimitExceeded)
    );
    assert_eq!(
        ck!(fs::read(fixture.project.join("Cargo.toml"))),
        source_cargo(&before)
    );
    Ok(())
}

#[test]
fn grant_is_deny_by_default_and_root_move_invalidates_authority() -> TestResult<()> {
    let fixture = Fixture::new("authority")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let unrelated = fixture.base.join("unrelated");
    ck!(fs::create_dir(&unrelated));
    let denied = ck!(NativeMutationStore::open(&fixture.state, &[unrelated]));
    assert_eq!(
        denied.authorize(&opened.lease),
        Err(MutationError::PermissionDenied)
    );
    let allowed = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    let moved = fixture.base.join("moved");
    ck!(fs::rename(&fixture.project, &moved));
    ck!(fs::create_dir(&fixture.project));
    assert!(allowed.authorize(&opened.lease).is_err());
    Ok(())
}

#[test]
fn corrupt_journal_blocks_all_new_commits_before_source_write() -> TestResult<()> {
    let fixture = Fixture::new("corrupt")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let commit = request(before, 2, "corrupt-key")?;
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    let corrupt = fixture
        .state
        .join("journal-mut_ffffffffffffffffffffffffffffffff.json");
    ck!(fs::write(&corrupt, b"{not-json"));
    ck!(fs::set_permissions(
        &corrupt,
        fs::Permissions::from_mode(0o600)
    ));
    assert_eq!(
        store.commit(&opened.lease, &commit, &Continue),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        ck!(fs::read(fixture.project.join("Cargo.toml"))),
        commit.candidate.before.files()[0].bytes()
    );
    Ok(())
}

#[test]
fn state_overlap_and_hardlinked_manifest_are_rejected() -> TestResult<()> {
    let fixture = Fixture::new("links")?;
    let state_inside = fixture.project.join("private-state");
    ck!(fs::create_dir(&state_inside));
    ck!(fs::set_permissions(
        &state_inside,
        fs::Permissions::from_mode(0o700)
    ));
    assert!(
        NativeMutationStore::open(&state_inside, std::slice::from_ref(&fixture.project)).is_err()
    );
    ck!(fs::hard_link(
        fixture.project.join("Cargo.toml"),
        fixture.project.join("Cargo-copy.toml")
    ));
    let backend = fixture.backend()?;
    assert!(
        backend
            .open(fixture.project.to_str().ok_or("utf8")?, &Continue)
            .is_err()
    );
    Ok(())
}

#[test]
fn late_manifest_symlink_never_writes_its_external_target() -> TestResult<()> {
    let fixture = Fixture::new("symlink")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let commit = request(before, 25, "symlink-key")?;
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    let canary = fixture.base.join("canary");
    ck!(fs::write(&canary, b"outside"));
    ck!(fs::rename(
        fixture.project.join("Cargo.toml"),
        fixture.project.join("Cargo.original")
    ));
    ck!(symlink(&canary, fixture.project.join("Cargo.toml")));
    assert!(store.commit(&opened.lease, &commit, &Continue).is_err());
    assert_eq!(ck!(fs::read(canary)), b"outside");
    Ok(())
}

#[test]
fn digest_binds_validation_and_directories() -> TestResult<()> {
    let fixture = Fixture::new("digest")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let source = ck!(backend.source(&opened.lease, &Continue));
    let mut first = request(source.clone(), 3, "digest-one")?.candidate;
    let original = ck!(mutation_digest(&first));
    first.validation.push('x');
    assert_ne!(original, ck!(mutation_digest(&first)));
    let mut second = request(source, 4, "digest-two")?.candidate;
    let files = ck!(second
        .after
        .files()
        .iter()
        .map(|file| SourceFile::new(file.path().to_owned(), file.bytes().to_vec()))
        .collect::<Result<Vec<_>, _>>());
    second.after = ck!(SourceBundle::with_directories(
        files,
        vec!["extra-empty".to_owned()]
    ));
    assert_ne!(original, ck!(mutation_digest(&second)));
    Ok(())
}

#[test]
fn commit_preserves_private_mode_and_extended_attributes() -> TestResult<()> {
    use rustix::buffer::spare_capacity;
    use rustix::fs::{XattrFlags, getxattr, setxattr};
    let fixture = Fixture::new("metadata")?;
    let manifest = fixture.project.join("Cargo.toml");
    ck!(fs::set_permissions(
        &manifest,
        fs::Permissions::from_mode(0o600)
    ));
    ck!(setxattr(
        &manifest,
        "com.rust-mcp.test",
        b"preserve",
        XattrFlags::empty()
    ));
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let commit = request(before, 5, "metadata-key")?;
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    assert_eq!(
        ck!(store.commit(&opened.lease, &commit, &Continue)).state,
        MutationState::Committed
    );
    assert_eq!(
        ck!(fs::metadata(&manifest)).permissions().mode() & 0o777,
        0o600
    );
    let mut value = Vec::with_capacity(32);
    ck!(getxattr(
        &manifest,
        "com.rust-mcp.test",
        spare_capacity(&mut value)
    ));
    assert_eq!(value, b"preserve");
    Ok(())
}

#[test]
fn source_conflict_and_reused_key_write_nothing() -> TestResult<()> {
    let fixture = Fixture::new("conflict")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let conflicted = request(before.clone(), 10, "shared-key")?;
    ck!(fs::write(
        fixture.project.join("src/lib.rs"),
        b"pub fn answer() -> u8 { 41 }\n"
    ));
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    assert_eq!(
        store.commit(&opened.lease, &conflicted, &Continue),
        Err(MutationError::Conflict)
    );
    assert_eq!(
        ck!(fs::read(fixture.project.join("Cargo.toml"))),
        source_cargo(&before)
    );

    let current = ck!(backend.source(&opened.lease, &Continue));
    let first = request(current, 11, "shared-key")?;
    ck!(store.commit(&opened.lease, &first, &Continue));
    let now = ck!(backend.source(&opened.lease, &Continue));
    let reused = request(now, 12, "shared-key")?;
    assert_eq!(
        store.commit(&opened.lease, &reused, &Continue),
        Err(MutationError::Conflict)
    );
    Ok(())
}

fn source_cargo(bundle: &SourceBundle) -> &[u8] {
    bundle
        .files()
        .iter()
        .find(|file| file.path() == "Cargo.toml")
        .map(SourceFile::bytes)
        .unwrap_or_default()
}

#[test]
fn global_lock_is_nonblocking_across_store_instances() -> TestResult<()> {
    use rustix::fs::{CWD, FlockOperation, Mode, OFlags, flock, openat};
    let fixture = Fixture::new("busy")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let commit = request(before, 20, "busy-key")?;
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    let lock = ck!(openat(
        CWD,
        fixture.state.join("mutation-store.lock"),
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty()
    ));
    ck!(flock(&lock, FlockOperation::NonBlockingLockExclusive));
    assert_eq!(
        store.commit(&opened.lease, &commit, &Continue),
        Err(MutationError::Busy)
    );
    Ok(())
}

#[test]
fn global_lock_is_nonblocking_across_processes() -> TestResult<()> {
    use std::process::Command;
    let fixture = Fixture::new("process-busy")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let commit = request(before, 21, "process-busy-key")?;
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    let ready = fixture.base.join("helper-ready");
    let release = fixture.base.join("helper-release");
    let mut child = ck!(Command::new(ck!(std::env::current_exe()))
        .arg("--exact")
        .arg("external_process_lock_helper")
        .arg("--nocapture")
        .env(LOCK_HELPER_STATE, &fixture.state)
        .env(LOCK_HELPER_READY, &ready)
        .env(LOCK_HELPER_RELEASE, &release)
        .spawn());
    let mut observed = false;
    for _ in 0..500 {
        if ready.exists() {
            observed = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let result = if observed {
        store.commit(&opened.lease, &commit, &Continue)
    } else {
        Err(MutationError::Io)
    };
    ck!(fs::write(&release, b"release"));
    let status = ck!(child.wait());
    assert!(observed, "external helper did not acquire the lock");
    assert!(status.success(), "external helper failed: {status}");
    assert_eq!(result, Err(MutationError::Busy));
    Ok(())
}

#[test]
fn journal_count_quota_and_unknown_store_entry_fail_closed() -> TestResult<()> {
    let fixture = Fixture::new("quota")?;
    let backend = fixture.backend()?;
    let opened = ck!(backend.open(fixture.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let first = request(before, 30, "quota-first")?;
    let store = ck!(NativeMutationStore::open(
        &fixture.state,
        std::slice::from_ref(&fixture.project)
    ));
    ck!(store.commit(&opened.lease, &first, &Continue));
    for index in 1..128_u128 {
        let current = ck!(backend.source(&opened.lease, &Continue));
        let no_change = request(current, index + 1000, &format!("quota-{index}"))?;
        assert_eq!(
            ck!(store.commit(&opened.lease, &no_change, &Continue)).state,
            MutationState::NoChange
        );
    }
    let current = ck!(backend.source(&opened.lease, &Continue));
    let over = request(current, 31, "quota-over")?;
    assert_eq!(
        store.commit(&opened.lease, &over, &Continue),
        Err(MutationError::LimitExceeded)
    );

    let other = Fixture::new("unknown")?;
    let backend = other.backend()?;
    let opened = ck!(backend.open(other.project.to_str().ok_or("utf8")?, &Continue));
    let before = ck!(backend.source(&opened.lease, &Continue));
    let commit = request(before, 32, "unknown-entry")?;
    let store = ck!(NativeMutationStore::open(
        &other.state,
        std::slice::from_ref(&other.project)
    ));
    let unknown = other.state.join("unexpected");
    ck!(fs::write(&unknown, b"evidence"));
    ck!(fs::set_permissions(
        &unknown,
        fs::Permissions::from_mode(0o600)
    ));
    assert_eq!(
        store.commit(&opened.lease, &commit, &Continue),
        Err(MutationError::RecoveryRequired)
    );
    Ok(())
}
