use super::*;
use rust_engineering_application::ProjectBackend;
use rust_engineering_domain::{
    IdempotencyKey, MutationCandidate, MutationCommit, MutationId, MutationKind, SourceFile,
};
use std::os::unix::fs::PermissionsExt;

const CRASH_PROJECT: &str = "RUST_MCP_NATIVE_CRASH_PROJECT";
const CRASH_STATE: &str = "RUST_MCP_NATIVE_CRASH_STATE";
const CRASH_READY: &str = "RUST_MCP_NATIVE_CRASH_READY";
const CRASH_PHASE: &str = "RUST_MCP_NATIVE_CRASH_PHASE";
const CRASH_KIND: &str = "RUST_MCP_NATIVE_CRASH_KIND";
const CRASH_SUFFIX: u128 = 140;

struct Fixture {
    base: PathBuf,
    project: PathBuf,
    state: PathBuf,
    owned: bool,
}
impl Fixture {
    fn new(label: &str) -> Result<Self, String> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|error| format!("{error:?}"))?;
        let base = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-mut-unit-{label}-{:032x}",
            u128::from_le_bytes(random)
        ));
        let project = base.join("project");
        let state = base.join("state");
        std::fs::create_dir_all(project.join("src")).map_err(|error| error.to_string())?;
        std::fs::create_dir(&state).map_err(|error| error.to_string())?;
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
        std::fs::write(
            project.join("Cargo.toml"),
            b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .map_err(|error| error.to_string())?;
        std::fs::write(project.join("src/lib.rs"), b"pub fn value() {}\n")
            .map_err(|error| error.to_string())?;
        std::fs::write(project.join("src/other.rs"), b"pub fn other()->u8{7}\n")
            .map_err(|error| error.to_string())?;
        Ok(Self {
            base,
            project,
            state,
            owned: true,
        })
    }

    fn request(
        &self,
        suffix: u128,
    ) -> Result<(SecureProjects, ProjectLease, MutationCommit), String> {
        let backend = SecureProjects::new(std::slice::from_ref(&self.project))
            .map_err(|error| format!("{error:?}"))?;
        let opened = backend
            .open(self.project.to_str().ok_or("utf8")?, &Continue)
            .map_err(|error| format!("{error:?}"))?;
        let before = backend
            .source(&opened.lease, &Continue)
            .map_err(|error| format!("{error:?}"))?;
        let files = before
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
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("{error:?}"))?;
        let after = SourceBundle::with_directories(files, before.directories().to_vec())
            .map_err(|error| format!("{error:?}"))?;
        let candidate = MutationCandidate {
            kind: MutationKind::ManifestPatch,
            before,
            after,
            validation: "unit-checkpoint".to_owned(),
        };
        let request = MutationCommit {
            id: MutationId::new(format!("mut_{suffix:032x}"))
                .map_err(|error| format!("{error:?}"))?,
            digest: mutation_digest(&candidate).map_err(|error| format!("{error:?}"))?,
            key: IdempotencyKey::new(format!("checkpoint-{suffix}"))
                .map_err(|error| format!("{error:?}"))?,
            candidate,
        };
        Ok((backend, opened.lease, request))
    }

    fn format_request(
        &self,
        suffix: u128,
    ) -> Result<(SecureProjects, ProjectLease, MutationCommit), String> {
        let backend = SecureProjects::new(std::slice::from_ref(&self.project))
            .map_err(|error| format!("{error:?}"))?;
        let opened = backend
            .open(self.project.to_str().ok_or("utf8")?, &Continue)
            .map_err(|error| format!("{error:?}"))?;
        let before = backend
            .source(&opened.lease, &Continue)
            .map_err(|error| format!("{error:?}"))?;
        let files = before
            .files()
            .iter()
            .map(|file| {
                let bytes = match file.path() {
                    "src/lib.rs" => b"pub fn value() {\n}\n".to_vec(),
                    "src/other.rs" => b"pub fn other() -> u8 {\n    7\n}\n".to_vec(),
                    _ => file.bytes().to_vec(),
                };
                SourceFile::new(file.path().to_owned(), bytes)
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("{error:?}"))?;
        let after = SourceBundle::with_directories(files, before.directories().to_vec())
            .map_err(|error| format!("{error:?}"))?;
        let candidate = MutationCandidate {
            kind: MutationKind::FormatApply,
            before,
            after,
            validation: "unit-format-checkpoint".to_owned(),
        };
        let request = MutationCommit {
            id: MutationId::new(format!("mut_{suffix:032x}"))
                .map_err(|error| format!("{error:?}"))?,
            digest: mutation_digest(&candidate).map_err(|error| format!("{error:?}"))?,
            key: IdempotencyKey::new(format!("format-checkpoint-{suffix}"))
                .map_err(|error| format!("{error:?}"))?,
            candidate,
        };
        Ok((backend, opened.lease, request))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }
}

fn encode_legacy_v1(body: &JournalBody) -> Result<Vec<u8>, String> {
    let file = body.files.first().ok_or("file")?;
    let phase = match body.phase {
        JournalPhase::Prepared => LegacyJournalPhaseV1::Prepared,
        JournalPhase::Scratch => LegacyJournalPhaseV1::Scratch,
        JournalPhase::Staged => LegacyJournalPhaseV1::Staged,
        JournalPhase::Published => LegacyJournalPhaseV1::Published,
        JournalPhase::AbortedCleanup => LegacyJournalPhaseV1::AbortedCleanup,
        JournalPhase::Committed => LegacyJournalPhaseV1::Committed,
        JournalPhase::NoChange => LegacyJournalPhaseV1::NoChange,
        JournalPhase::Aborted => LegacyJournalPhaseV1::Aborted,
        JournalPhase::RecoveryRequired => LegacyJournalPhaseV1::RecoveryRequired,
        JournalPhase::Applying => return Err("v1 has no applying phase".to_owned()),
    };
    let legacy = LegacyJournalBodyV1 {
        id: body.id.clone(),
        digest: body.digest.clone(),
        key: body.key.clone(),
        operation: body.operation.clone(),
        workspace_path: body.workspace_path.clone(),
        workspace_device: body.workspace_device,
        workspace_inode: body.workspace_inode,
        temp_path: legacy_temp_name(
            &MutationId::new(body.id.clone()).map_err(|error| format!("{error:?}"))?,
        ),
        source_node: file.source_node,
        staged_node: file.staged_node,
        validation: body.validation.clone(),
        sequence: body.sequence,
        phase,
        before: body.before.clone(),
        after: body.after.clone(),
    };
    let canonical = serde_json::to_vec(&legacy).map_err(|error| error.to_string())?;
    serde_json::to_vec(&LegacyJournalRecordV1 {
        format: "rust-engineering-mcp-mutation-journal-v1".to_owned(),
        checksum: sha256(&canonical),
        body: legacy,
    })
    .map_err(|error| error.to_string())
}

#[test]
fn legacy_v1_receipt_is_read_only_and_explicit_recovery_migrates_to_v2() -> Result<(), String> {
    let fixture = Fixture::new("legacy-v1")?;
    let (_backend, lease, request) = fixture.request(144)?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Staged {
                std::panic::resume_unwind(Box::new("abrupt legacy fixture"));
            }
            Ok(())
        });
    }));
    assert!(interrupted.is_err());
    let name = journal_name(&request.id);
    let raw = std::fs::read(fixture.state.join(&name)).map_err(|error| error.to_string())?;
    let body = decode(&raw).map_err(|error| format!("{error:?}"))?;
    let indexed = fixture.project.join(&body.files[0].temp_path);
    let legacy = fixture.project.join(legacy_temp_name(&request.id));
    std::fs::rename(indexed, &legacy).map_err(|error| error.to_string())?;
    let legacy_record = encode_legacy_v1(&body)?;
    std::fs::write(fixture.state.join(&name), &legacy_record).map_err(|error| error.to_string())?;
    std::fs::set_permissions(
        fixture.state.join(&name),
        std::fs::Permissions::from_mode(0o600),
    )
    .map_err(|error| error.to_string())?;

    assert_eq!(
        store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        store.list_records().map_err(|error| format!("{error:?}"))?[0].state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        std::fs::read(fixture.state.join(&name)).map_err(|error| error.to_string())?,
        legacy_record
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Aborted
    );
    assert!(!legacy.exists());
    let migrated = std::fs::read(fixture.state.join(name)).map_err(|error| error.to_string())?;
    let format: JournalFormatProbe =
        serde_json::from_slice(&migrated).map_err(|error| error.to_string())?;
    assert_eq!(format.format, "rust-engineering-mcp-mutation-journal-v2");
    Ok(())
}

#[test]
fn unknown_journal_format_never_cleans_or_changes_source() -> Result<(), String> {
    let fixture = Fixture::new("unknown-v3")?;
    let (_backend, lease, request) = fixture.request(145)?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Staged {
                std::panic::resume_unwind(Box::new("abrupt unknown-format fixture"));
            }
            Ok(())
        });
    }));
    assert!(interrupted.is_err());
    let name = journal_name(&request.id);
    let raw = std::fs::read(fixture.state.join(&name)).map_err(|error| error.to_string())?;
    let text = std::str::from_utf8(&raw).map_err(|error| error.to_string())?;
    let unknown = text.replace(
        "rust-engineering-mcp-mutation-journal-v2",
        "rust-engineering-mcp-mutation-journal-v3",
    );
    std::fs::write(fixture.state.join(&name), unknown.as_bytes())
        .map_err(|error| error.to_string())?;
    let temp = fixture.project.join(temp_name(&request.id, 0));
    let before = source_file(&request.candidate.before, "Cargo.toml")
        .ok_or("before")?
        .bytes()
        .to_vec();
    assert_eq!(
        store.receipt(&lease, &request.id),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store.recover(&lease, &request.id),
        Err(MutationError::RecoveryRequired)
    );
    assert!(temp.exists());
    assert_eq!(
        std::fs::read(fixture.project.join("Cargo.toml")).map_err(|error| error.to_string())?,
        before
    );
    Ok(())
}

#[test]
fn abrupt_checkpoint_helper() -> Result<(), String> {
    let Some(project) = std::env::var_os(CRASH_PROJECT) else {
        return Ok(());
    };
    let project = PathBuf::from(project);
    let state = PathBuf::from(std::env::var_os(CRASH_STATE).ok_or("state")?);
    let ready = PathBuf::from(std::env::var_os(CRASH_READY).ok_or("ready")?);
    let requested = std::env::var(CRASH_PHASE).map_err(|error| error.to_string())?;
    let fixture = Fixture {
        base: project.parent().ok_or("base")?.to_path_buf(),
        project,
        state,
        owned: false,
    };
    let format = std::env::var(CRASH_KIND).is_ok_and(|kind| kind == "format");
    let (_backend, lease, request) = if format {
        fixture.format_request(CRASH_SUFFIX)?
    } else {
        fixture.request(CRASH_SUFFIX)?
    };
    let kind = if format {
        MutationKind::FormatApply
    } else {
        MutationKind::ManifestPatch
    };
    let store = NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        kind,
    )
    .map_err(|error| format!("{error:?}"))?;
    let target = match requested.as_str() {
        "clone_created" | "clone_unknown" => CommitCheckpoint::CloneCreated,
        "file_cloned_0" => CommitCheckpoint::FileCloned(0),
        "scratch" => CommitCheckpoint::Scratch,
        "applying" => CommitCheckpoint::Applying,
        "swapped" => CommitCheckpoint::Swapped,
        "file_swapped_0" => CommitCheckpoint::FileSwapped(0),
        "file_swapped_1" => CommitCheckpoint::FileSwapped(1),
        "file_cleaned_0" => CommitCheckpoint::FileCleaned(0),
        "published" => CommitCheckpoint::Published,
        _ => return Err("unknown crash phase".to_owned()),
    };
    let _ = store.commit_checked(&lease, &request, &Continue, |phase| {
        if phase == target {
            std::fs::write(&ready, b"ready").map_err(|_| MutationError::Io)?;
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        Ok(())
    });
    Err("checkpoint helper unexpectedly completed".to_owned())
}

#[test]
fn killed_process_recovers_each_durable_boundary() -> Result<(), String> {
    use std::process::Command;

    for phase in ["clone_created", "clone_unknown", "scratch", "swapped"] {
        let fixture = Fixture::new(&format!("process-crash-{phase}"))?;
        let (_backend, lease, request) = fixture.request(CRASH_SUFFIX)?;
        let ready = fixture.base.join("crash-ready");
        let mut child = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
            .arg("--exact")
            .arg("filesystem::macos::mutation::tests::abrupt_checkpoint_helper")
            .arg("--nocapture")
            .env(CRASH_PROJECT, &fixture.project)
            .env(CRASH_STATE, &fixture.state)
            .env(CRASH_READY, &ready)
            .env(CRASH_PHASE, phase)
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut observed = false;
        for _ in 0..500 {
            if ready.exists() {
                observed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !observed {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("crash helper {phase} did not reach its checkpoint"));
        }
        child.kill().map_err(|error| error.to_string())?;
        let status = child.wait().map_err(|error| error.to_string())?;
        assert!(!status.success(), "killed helper unexpectedly succeeded");

        let temp = fixture.project.join(temp_name(&request.id, 0));
        if phase == "clone_unknown" {
            std::fs::write(&temp, b"unknown unjournaled clone bytes")
                .map_err(|error| error.to_string())?;
        }

        let restarted =
            NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
                .map_err(|error| format!("{error:?}"))?;
        let receipt = restarted
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?;
        let expected = match phase {
            "clone_unknown" => MutationState::RecoveryRequired,
            "swapped" => MutationState::Committed,
            _ => MutationState::Aborted,
        };
        assert_eq!(receipt.state, expected);
        assert_eq!(temp.exists(), phase == "clone_unknown");
        if phase == "clone_unknown" {
            assert_eq!(
                std::fs::read(&temp).map_err(|error| error.to_string())?,
                b"unknown unjournaled clone bytes"
            );
        }
        let expected_bytes = if expected == MutationState::Committed {
            source_file(&request.candidate.after, "Cargo.toml")
        } else {
            source_file(&request.candidate.before, "Cargo.toml")
        }
        .ok_or("cargo")?
        .bytes();
        assert_eq!(
            std::fs::read(fixture.project.join("Cargo.toml")).map_err(|error| error.to_string())?,
            expected_bytes
        );
    }
    Ok(())
}

#[test]
fn killed_process_rolls_forward_known_format_prefix() -> Result<(), String> {
    use std::process::Command;

    for phase in [
        "file_cloned_0",
        "applying",
        "file_swapped_0",
        "file_swapped_1",
        "published",
        "file_cleaned_0",
    ] {
        let fixture = Fixture::new(&format!("format-prefix-crash-{phase}"))?;
        let (_backend, lease, request) = fixture.format_request(CRASH_SUFFIX)?;
        let ready = fixture.base.join("format-crash-ready");
        let mut child = Command::new(std::env::current_exe().map_err(|error| error.to_string())?)
            .arg("--exact")
            .arg("filesystem::macos::mutation::tests::abrupt_checkpoint_helper")
            .arg("--nocapture")
            .env(CRASH_PROJECT, &fixture.project)
            .env(CRASH_STATE, &fixture.state)
            .env(CRASH_READY, &ready)
            .env(CRASH_PHASE, phase)
            .env(CRASH_KIND, "format")
            .spawn()
            .map_err(|error| error.to_string())?;
        let mut observed = false;
        for _ in 0..500 {
            if ready.exists() {
                observed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if !observed {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("format crash helper did not reach {phase}"));
        }
        child.kill().map_err(|error| error.to_string())?;
        let status = child.wait().map_err(|error| error.to_string())?;
        assert!(!status.success(), "killed helper unexpectedly succeeded");

        let first_temp = fixture.project.join(temp_name(&request.id, 0));
        let second_temp = fixture.project.join(temp_name(&request.id, 1));
        if phase == "file_cloned_0" {
            assert!(first_temp.exists());
            assert!(!second_temp.exists());
        } else if phase == "file_cleaned_0" {
            assert!(!first_temp.exists());
            assert!(second_temp.exists());
        }

        let restarted = NativeMutationStore::open_for_kind(
            &fixture.state,
            std::slice::from_ref(&fixture.project),
            MutationKind::FormatApply,
        )
        .map_err(|error| format!("{error:?}"))?;
        let receipt = restarted
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?;
        let expected = if matches!(phase, "file_cloned_0" | "applying") {
            MutationState::Aborted
        } else {
            MutationState::Committed
        };
        assert_eq!(receipt.state, expected);
        for file in &receipt.files {
            let expected_file = if expected == MutationState::Committed {
                source_file(&request.candidate.after, &file.path)
            } else {
                source_file(&request.candidate.before, &file.path)
            }
            .ok_or("expected")?;
            assert_eq!(
                std::fs::read(fixture.project.join(&file.path))
                    .map_err(|error| error.to_string())?,
                expected_file.bytes()
            );
        }
        assert!(
            std::fs::read_dir(&fixture.project)
                .map_err(|error| error.to_string())?
                .all(|entry| entry
                    .ok()
                    .and_then(|entry| entry.file_name().into_string().ok())
                    .is_none_or(|name| !name.starts_with(".rust-mcp-mut-")))
        );
    }
    Ok(())
}

#[test]
fn unknown_untouched_bytes_stop_format_recovery_without_advancing_suffix() -> Result<(), String> {
    let fixture = Fixture::new("format-unknown-context")?;
    std::fs::write(fixture.project.join("README.md"), b"before context\n")
        .map_err(|error| error.to_string())?;
    let (_backend, lease, request) = fixture.format_request(141)?;
    let store = NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::FileSwapped(0) {
                std::fs::write(fixture.project.join("README.md"), b"external context\n")
                    .map_err(|_| MutationError::Io)?;
                return Err(MutationError::Io);
            }
            Ok(())
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        std::fs::read(fixture.project.join("README.md")).map_err(|error| error.to_string())?,
        b"external context\n"
    );
    let first = &request.candidate.after.files()[request
        .candidate
        .after
        .files()
        .iter()
        .position(|file| file.path() == "src/lib.rs")
        .ok_or("first")?];
    let second_before =
        source_file(&request.candidate.before, "src/other.rs").ok_or("second before")?;
    assert_eq!(
        std::fs::read(fixture.project.join("src/lib.rs")).map_err(|error| error.to_string())?,
        first.bytes()
    );
    assert_eq!(
        std::fs::read(fixture.project.join("src/other.rs")).map_err(|error| error.to_string())?,
        second_before.bytes()
    );
    Ok(())
}

#[test]
fn lost_temp_and_out_of_order_generation_remain_recovery_required() -> Result<(), String> {
    let lost = Fixture::new("format-lost-temp")?;
    let (_backend, lease, request) = lost.format_request(142)?;
    let store = NativeMutationStore::open_for_kind(
        &lost.state,
        std::slice::from_ref(&lost.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    let first_temp = lost.project.join(temp_name(&request.id, 0));
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Staged {
                std::fs::remove_file(&first_temp).map_err(|_| MutationError::Io)?;
                return Err(MutationError::Io);
            }
            Ok(())
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert!(lost.project.join(temp_name(&request.id, 1)).exists());

    let applying = Fixture::new("format-lost-applying-temp")?;
    let (_backend, lease, request) = applying.format_request(144)?;
    let store = NativeMutationStore::open_for_kind(
        &applying.state,
        std::slice::from_ref(&applying.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    let first_temp = applying.project.join(temp_name(&request.id, 0));
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Applying {
                std::fs::remove_file(&first_temp).map_err(|_| MutationError::Io)?;
                return Err(MutationError::Io);
            }
            Ok(())
        }),
        Err(MutationError::RecoveryRequired)
    );
    let pending = store
        .recover(&lease, &request.id)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(pending.state, MutationState::RecoveryRequired);
    assert!(
        pending
            .files
            .iter()
            .all(|file| file.effect_after.is_none() && file.effect_after_bytes.is_none())
    );
    assert!(applying.project.join(temp_name(&request.id, 1)).exists());

    let ordered = Fixture::new("format-out-of-order")?;
    let (_backend, lease, request) = ordered.format_request(143)?;
    let store = NativeMutationStore::open_for_kind(
        &ordered.state,
        std::slice::from_ref(&ordered.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::FileSwapped(0) {
                store.swap(&lease, "src/lib.rs", &temp_name(&request.id, 0))?;
                store.swap(&lease, "src/other.rs", &temp_name(&request.id, 1))?;
                return Err(MutationError::Io);
            }
            Ok(())
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        std::fs::read(ordered.project.join("src/lib.rs")).map_err(|error| error.to_string())?,
        source_file(&request.candidate.before, "src/lib.rs")
            .ok_or("first")?
            .bytes()
    );
    assert_eq!(
        std::fs::read(ordered.project.join("src/other.rs")).map_err(|error| error.to_string())?,
        source_file(&request.candidate.after, "src/other.rs")
            .ok_or("second")?
            .bytes()
    );
    Ok(())
}

#[test]
fn aborted_cleanup_resumes_after_a_durable_missing_temp_prefix() -> Result<(), String> {
    let fixture = Fixture::new("format-aborted-cleanup-prefix")?;
    let (_backend, lease, request) = fixture.format_request(219)?;
    let store = NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Staged {
                std::panic::resume_unwind(Box::new("simulated abrupt stop before recovery"));
            }
            Ok(())
        });
    }));
    assert!(interrupted.is_err());
    let body = store
        .load_repair(&lease, &request.id)
        .map_err(|error| format!("{error:?}"))?
        .ok_or("journal")?;
    assert_eq!(body.phase, JournalPhase::Staged);
    assert_eq!(
        store.recover_locked_checked(&lease, body, |checkpoint| {
            if checkpoint == CommitCheckpoint::FileCleaned(0) {
                Err(MutationError::Io)
            } else {
                Ok(())
            }
        }),
        Err(MutationError::Io)
    );
    assert!(!fixture.project.join(temp_name(&request.id, 0)).exists());
    assert!(fixture.project.join(temp_name(&request.id, 1)).exists());
    drop(store);

    let restarted = NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        restarted
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Aborted
    );
    for index in 0..2 {
        assert!(!fixture.project.join(temp_name(&request.id, index)).exists());
    }
    for path in ["src/lib.rs", "src/other.rs"] {
        assert_eq!(
            std::fs::read(fixture.project.join(path)).map_err(|error| error.to_string())?,
            source_file(&request.candidate.before, path)
                .ok_or("before")?
                .bytes()
        );
    }
    Ok(())
}

#[test]
fn deterministic_pre_effect_interruptions_recover_without_source_write() -> Result<(), String> {
    for (index, interrupted) in [
        CommitCheckpoint::Prepared,
        CommitCheckpoint::CloneCreated,
        CommitCheckpoint::Scratch,
        CommitCheckpoint::WrittenBeforeSync,
        CommitCheckpoint::BeforeStagedPersist,
        CommitCheckpoint::Staged,
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new(&format!("pre-{index}"))?;
        let (_backend, lease, request) = fixture.request(index as u128 + 100)?;
        let store =
            NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
                .map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            store.commit_checked(&lease, &request, &Continue, |phase| {
                if phase == interrupted {
                    Err(MutationError::Cancelled)
                } else {
                    Ok(())
                }
            }),
            Err(MutationError::Cancelled)
        );
        let recovered = store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(recovered.state, MutationState::Aborted);
        assert_eq!(
            recovered.files[0].effect_after,
            Some(recovered.files[0].before.clone())
        );
        assert_ne!(recovered.files[0].before, recovered.files[0].after);
        assert_eq!(
            std::fs::read(fixture.project.join("Cargo.toml")).map_err(|error| error.to_string())?,
            source_file(&request.candidate.before, "Cargo.toml")
                .ok_or("cargo")?
                .bytes()
        );
    }
    Ok(())
}

#[test]
fn partial_scratch_and_sync_boundary_failures_abort_without_source_effect() -> Result<(), String> {
    for (index, checkpoint) in [
        CommitCheckpoint::Scratch,
        CommitCheckpoint::WrittenBeforeSync,
    ]
    .into_iter()
    .enumerate()
    {
        let fixture = Fixture::new(&format!("scratch-failure-{index}"))?;
        let (_backend, lease, request) = fixture.request(index as u128 + 120)?;
        let store =
            NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
                .map_err(|error| format!("{error:?}"))?;
        let temp = fixture.project.join(temp_name(&request.id, 0));
        let result = store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == checkpoint {
                if phase == CommitCheckpoint::Scratch {
                    std::fs::write(&temp, b"partial staged bytes")
                        .map_err(|_| MutationError::Io)?;
                }
                return Err(MutationError::Io);
            }
            Ok(())
        });
        assert_eq!(result, Err(MutationError::Io));
        assert!(!temp.exists());
        let receipt = store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(receipt.state, MutationState::Aborted);
        assert_eq!(
            std::fs::read(fixture.project.join("Cargo.toml")).map_err(|error| error.to_string())?,
            source_file(&request.candidate.before, "Cargo.toml")
                .ok_or("cargo")?
                .bytes()
        );
    }
    Ok(())
}

#[test]
fn newer_staged_record_left_by_persist_failure_is_recovered() -> Result<(), String> {
    let fixture = Fixture::new("staged-persist-failure")?;
    let (_backend, lease, request) = fixture.request(130)?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    let final_name = journal_name(&request.id);
    let staging_name = format!(".{final_name}.staging");
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::BeforeStagedPersist {
                let raw = std::fs::read(fixture.state.join(&final_name))
                    .map_err(|_| MutationError::Io)?;
                let mut staged = decode(&raw)?;
                staged.sequence = staged
                    .sequence
                    .checked_add(1)
                    .ok_or(MutationError::LimitExceeded)?;
                staged.phase = JournalPhase::Staged;
                std::fs::write(fixture.state.join(&staging_name), encode(staged)?)
                    .map_err(|_| MutationError::Io)?;
                std::fs::set_permissions(
                    fixture.state.join(&staging_name),
                    std::fs::Permissions::from_mode(0o600),
                )
                .map_err(|_| MutationError::Io)?;
                return Err(MutationError::Io);
            }
            Ok(())
        }),
        Err(MutationError::Io)
    );
    assert!(!fixture.state.join(staging_name).exists());
    assert!(!fixture.project.join(temp_name(&request.id, 0)).exists());
    assert_eq!(
        store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Aborted
    );
    Ok(())
}

#[test]
fn exact_query_and_recovery_ignore_unrelated_store_damage() -> Result<(), String> {
    let fixture = Fixture::new("reachable-recovery")?;
    let (backend, lease, request) = fixture.request(240)?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Published {
                Err(MutationError::Io)
            } else {
                Ok(())
            }
        }),
        Err(MutationError::RecoveryRequired)
    );
    let unrelated = fixture.state.join("unrelated-damage");
    let file = std::fs::File::create(&unrelated).map_err(|error| error.to_string())?;
    file.set_len(MAX_STORE_BYTES + 1)
        .map_err(|error| error.to_string())?;
    std::fs::set_permissions(&unrelated, std::fs::Permissions::from_mode(0o600))
        .map_err(|error| error.to_string())?;
    let pending = store
        .receipt(&lease, &request.id)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(pending.state, MutationState::RecoveryRequired);
    assert_eq!(pending.files[0].effect_after, None);
    assert_eq!(pending.files[0].effect_after_bytes, None);
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    assert_eq!(
        store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    let current = backend
        .source(&lease, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let next = {
        let candidate = MutationCandidate {
            kind: MutationKind::ManifestPatch,
            before: current.clone(),
            after: current,
            validation: "unit-checkpoint".to_owned(),
        };
        MutationCommit {
            id: MutationId::new("mut_00000000000000000000000000000241".to_owned())
                .map_err(|error| format!("{error:?}"))?,
            digest: mutation_digest(&candidate).map_err(|error| format!("{error:?}"))?,
            key: IdempotencyKey::new("checkpoint-241".to_owned())
                .map_err(|error| format!("{error:?}"))?,
            candidate,
        }
    };
    assert!(matches!(
        store.commit(&lease, &next, &Continue),
        Err(MutationError::RecoveryRequired | MutationError::LimitExceeded)
    ));
    Ok(())
}

#[test]
fn receipt_is_read_only_and_staging_sequence_must_increase() -> Result<(), String> {
    let fixture = Fixture::new("readonly-staging")?;
    let (_backend, lease, request) = fixture.request(250)?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    store
        .commit(&lease, &request, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let final_name = journal_name(&request.id);
    let final_bytes =
        std::fs::read(fixture.state.join(&final_name)).map_err(|error| error.to_string())?;
    let mut staged = decode(&final_bytes).map_err(|error| format!("{error:?}"))?;
    staged.sequence += 1;
    let staging_name = format!(".{final_name}.staging");
    let staging_bytes = encode(staged).map_err(|error| format!("{error:?}"))?;
    std::fs::write(fixture.state.join(&staging_name), &staging_bytes)
        .map_err(|error| error.to_string())?;
    std::fs::set_permissions(
        fixture.state.join(&staging_name),
        std::fs::Permissions::from_mode(0o600),
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        std::fs::read(fixture.state.join(&final_name)).map_err(|error| error.to_string())?,
        final_bytes
    );
    assert_eq!(
        std::fs::read(fixture.state.join(&staging_name)).map_err(|error| error.to_string())?,
        staging_bytes
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    assert!(!fixture.state.join(staging_name).exists());

    let invalid = Fixture::new("invalid-sequence")?;
    let (_backend, lease, request) = invalid.request(251)?;
    let store = NativeMutationStore::open(&invalid.state, std::slice::from_ref(&invalid.project))
        .map_err(|error| format!("{error:?}"))?;
    store
        .commit(&lease, &request, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let final_name = journal_name(&request.id);
    let bytes =
        std::fs::read(invalid.state.join(&final_name)).map_err(|error| error.to_string())?;
    let staging_name = format!(".{final_name}.staging");
    std::fs::write(invalid.state.join(&staging_name), bytes).map_err(|error| error.to_string())?;
    std::fs::set_permissions(
        invalid.state.join(&staging_name),
        std::fs::Permissions::from_mode(0o600),
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        store.receipt(&lease, &request.id),
        Err(MutationError::RecoveryRequired)
    );
    assert!(invalid.state.join(staging_name).exists());
    Ok(())
}

#[test]
fn cancellation_during_final_logical_recapture_aborts_and_cleans_temps() -> Result<(), String> {
    use std::sync::atomic::{AtomicBool, Ordering};

    struct CancelWhenSet<'a>(&'a AtomicBool);
    impl OperationControl for CancelWhenSet<'_> {
        fn check(&self) -> Result<(), ProjectError> {
            if self.0.load(Ordering::SeqCst) {
                Err(ProjectError::Cancelled)
            } else {
                Ok(())
            }
        }
    }

    let fixture = Fixture::new("format-cancel-final-capture")?;
    let (_backend, lease, request) = fixture.format_request(220)?;
    let store = NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    let cancelled = AtomicBool::new(false);
    let control = CancelWhenSet(&cancelled);
    assert_eq!(
        store.commit_checked(&lease, &request, &control, |phase| {
            if phase == CommitCheckpoint::Staged {
                cancelled.store(true, Ordering::SeqCst);
            }
            Ok(())
        }),
        Err(MutationError::Cancelled)
    );
    assert_eq!(
        store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Aborted
    );
    for (index, _) in request.candidate.after.files().iter().enumerate().take(2) {
        assert!(!fixture.project.join(temp_name(&request.id, index)).exists());
    }
    for path in ["src/lib.rs", "src/other.rs"] {
        assert_eq!(
            std::fs::read(fixture.project.join(path)).map_err(|error| error.to_string())?,
            source_file(&request.candidate.before, path)
                .ok_or("before")?
                .bytes()
        );
    }
    Ok(())
}

#[test]
fn logical_recapture_excludes_only_owned_temp_nodes() -> Result<(), String> {
    let fixture = Fixture::new("format-exact-temp-exclusion")?;
    let (_backend, lease, request) = fixture.format_request(221)?;
    let store = NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    let foreign = fixture.project.join(".rust-mcp-mut-foreign.swap");
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Staged {
                std::fs::write(&foreign, b"foreign bytes").map_err(|_| MutationError::Io)?;
            }
            Ok(())
        }),
        Err(MutationError::Conflict)
    );
    assert_eq!(
        std::fs::read(&foreign).map_err(|error| error.to_string())?,
        b"foreign bytes"
    );
    for index in 0..2 {
        assert!(!fixture.project.join(temp_name(&request.id, index)).exists());
    }
    for path in ["src/lib.rs", "src/other.rs"] {
        assert_eq!(
            std::fs::read(fixture.project.join(path)).map_err(|error| error.to_string())?,
            source_file(&request.candidate.before, path)
                .ok_or("before")?
                .bytes()
        );
    }
    Ok(())
}

#[test]
fn proven_swap_failure_returns_cause_and_records_abort() -> Result<(), String> {
    let fixture = Fixture::new("swap-failure")?;
    let (_backend, lease, request) = fixture.request(260)?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    let temp = fixture.project.join(temp_name(&request.id, 0));
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Staged {
                std::fs::remove_file(&temp).map_err(|_| MutationError::Io)?;
            }
            Ok(())
        }),
        Err(MutationError::Io)
    );
    let receipt = store
        .receipt(&lease, &request.id)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(receipt.state, MutationState::Aborted);
    assert_eq!(
        receipt.files[0].effect_after,
        Some(receipt.files[0].before.clone())
    );
    Ok(())
}

#[test]
fn workspace_lock_names_are_bounded_shards() {
    let names: std::collections::BTreeSet<_> = (0..10_000_u64)
        .map(|inode| workspace_lock_name(Node { device: 7, inode }))
        .collect();
    assert!(names.len() <= WORKSPACE_LOCK_SHARDS as usize);
    assert!(names.iter().all(|name| is_workspace_lock_name(name)));
}

#[test]
fn operator_lists_and_prunes_only_exact_terminal_records() -> Result<(), String> {
    let terminal = Fixture::new("operator-terminal")?;
    let (_backend, lease, request) = terminal.request(270)?;
    let store = NativeMutationStore::open(&terminal.state, std::slice::from_ref(&terminal.project))
        .map_err(|error| format!("{error:?}"))?;
    store
        .commit(&lease, &request, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    drop(store);
    std::fs::remove_dir_all(&terminal.project).map_err(|error| error.to_string())?;
    let operator =
        NativeMutationStore::open(&terminal.state, &[]).map_err(|error| format!("{error:?}"))?;
    let records = operator
        .list_records()
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, request.id);
    assert_eq!(records[0].digest, request.digest);
    assert_eq!(records[0].state, MutationState::Committed);
    assert!(records[0].stored_bytes > 0);
    operator
        .prune_record(&request.id, &request.digest)
        .map_err(|error| format!("{error:?}"))?;
    assert!(
        operator
            .list_records()
            .map_err(|error| format!("{error:?}"))?
            .is_empty()
    );
    assert_eq!(
        operator.prune_record(&request.id, &request.digest),
        Err(MutationError::NotFound)
    );
    let absent = terminal.base.join("missing-state");
    assert!(NativeMutationStore::open(&absent, &[]).is_err());
    assert!(!absent.exists());

    let pending = Fixture::new("operator-pending")?;
    let (_backend, lease, request) = pending.request(271)?;
    let store = NativeMutationStore::open(&pending.state, std::slice::from_ref(&pending.project))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Published {
                Err(MutationError::Io)
            } else {
                Ok(())
            }
        }),
        Err(MutationError::RecoveryRequired)
    );
    drop(store);
    let operator =
        NativeMutationStore::open(&pending.state, &[]).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        operator
            .list_records()
            .map_err(|error| format!("{error:?}"))?[0]
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        operator.prune_record(&request.id, &request.digest),
        Err(MutationError::RecoveryRequired)
    );
    assert!(pending.state.join(journal_name(&request.id)).exists());

    let staged = Fixture::new("operator-staged")?;
    let (_backend, lease, request) = staged.request(272)?;
    let store = NativeMutationStore::open(&staged.state, std::slice::from_ref(&staged.project))
        .map_err(|error| format!("{error:?}"))?;
    store
        .commit(&lease, &request, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let final_name = journal_name(&request.id);
    let bytes = std::fs::read(staged.state.join(&final_name)).map_err(|error| error.to_string())?;
    let mut body = decode(&bytes).map_err(|error| format!("{error:?}"))?;
    body.sequence += 1;
    let staging_name = format!(".{final_name}.staging");
    std::fs::write(
        staged.state.join(&staging_name),
        encode(body).map_err(|error| format!("{error:?}"))?,
    )
    .map_err(|error| error.to_string())?;
    std::fs::set_permissions(
        staged.state.join(&staging_name),
        std::fs::Permissions::from_mode(0o600),
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        store.prune_record(&request.id, &request.digest),
        Err(MutationError::RecoveryRequired)
    );

    let refused = Fixture::new("operator-refused")?;
    let (_backend, lease, request) = refused.request(273)?;
    let store = NativeMutationStore::open(&refused.state, std::slice::from_ref(&refused.project))
        .map_err(|error| format!("{error:?}"))?;
    store
        .commit(&lease, &request, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let wrong: SourceFingerprint = format!("sha256:{}", "0".repeat(64))
        .parse()
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.prune_record(&request.id, &wrong),
        Err(MutationError::Conflict)
    );
    std::fs::write(refused.state.join(journal_name(&request.id)), b"corrupt")
        .map_err(|error| error.to_string())?;
    assert_eq!(
        store.prune_record(&request.id, &request.digest),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(store.list_records(), Err(MutationError::RecoveryRequired));
    Ok(())
}

#[test]
fn post_swap_interrupt_and_late_external_write_are_classified() -> Result<(), String> {
    let fixture = Fixture::new("post")?;
    let (_backend, lease, request) = fixture.request(200)?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Swapped {
                Err(MutationError::Io)
            } else {
                Ok(())
            }
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );

    let late = Fixture::new("late")?;
    let (_backend, lease, request) = late.request(201)?;
    let store = NativeMutationStore::open(&late.state, std::slice::from_ref(&late.project))
        .map_err(|error| format!("{error:?}"))?;
    let cargo = late.project.join("Cargo.toml");
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Swapped {
                std::fs::write(&cargo, b"external bytes").map_err(|_| MutationError::Io)?;
            }
            Ok(())
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        std::fs::read(cargo).map_err(|error| error.to_string())?,
        b"external bytes"
    );

    let mut next = request.clone();
    next.id = MutationId::new("mut_00000000000000000000000000000202".to_owned())
        .map_err(|error| format!("{error:?}"))?;
    next.key =
        IdempotencyKey::new("checkpoint-202".to_owned()).map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit(&lease, &next, &Continue),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        std::fs::read(late.project.join("Cargo.toml")).map_err(|error| error.to_string())?,
        b"external bytes"
    );

    let displaced = Fixture::new("late-displaced")?;
    let (_backend, lease, request) = displaced.request(203)?;
    let store =
        NativeMutationStore::open(&displaced.state, std::slice::from_ref(&displaced.project))
            .map_err(|error| format!("{error:?}"))?;
    let temp = displaced.project.join(temp_name(&request.id, 0));
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Published {
                std::fs::write(&temp, b"unknown displaced bytes").map_err(|_| MutationError::Io)?;
                return Err(MutationError::Io);
            }
            Ok(())
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        std::fs::read(&temp).map_err(|error| error.to_string())?,
        b"unknown displaced bytes"
    );
    assert_eq!(
        std::fs::read(displaced.project.join("Cargo.toml")).map_err(|error| error.to_string())?,
        source_file(&request.candidate.after, "Cargo.toml")
            .ok_or("cargo")?
            .bytes()
    );

    let rolled_back = Fixture::new("late-rollback")?;
    let (_backend, lease, request) = rolled_back.request(204)?;
    let store = NativeMutationStore::open(
        &rolled_back.state,
        std::slice::from_ref(&rolled_back.project),
    )
    .map_err(|error| format!("{error:?}"))?;
    let cargo = rolled_back.project.join("Cargo.toml");
    let temp = rolled_back.project.join(temp_name(&request.id, 0));
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Published {
                std::fs::rename(&temp, &cargo).map_err(|_| MutationError::Io)?;
                return Err(MutationError::Io);
            }
            Ok(())
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert!(!temp.exists());
    assert_eq!(
        std::fs::read(cargo).map_err(|error| error.to_string())?,
        source_file(&request.candidate.before, "Cargo.toml")
            .ok_or("cargo")?
            .bytes()
    );
    Ok(())
}

#[test]
fn cleanup_phase_interruptions_never_report_a_premature_terminal_receipt() -> Result<(), String> {
    let published = Fixture::new("published-cleanup")?;
    let (_backend, lease, request) = published.request(220)?;
    let store =
        NativeMutationStore::open(&published.state, std::slice::from_ref(&published.project))
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Published {
                Err(MutationError::Io)
            } else {
                Ok(())
            }
        }),
        Err(MutationError::RecoveryRequired)
    );
    let restarted =
        NativeMutationStore::open(&published.state, std::slice::from_ref(&published.project))
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        restarted
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert!(published.project.join(temp_name(&request.id, 0)).exists());
    assert_eq!(
        restarted
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    assert!(!published.project.join(temp_name(&request.id, 0)).exists());
    assert_eq!(
        std::fs::read(published.project.join("Cargo.toml")).map_err(|error| error.to_string())?,
        source_file(&request.candidate.after, "Cargo.toml")
            .ok_or("cargo")?
            .bytes()
    );

    let replayed = Fixture::new("published-replay")?;
    let (_backend, lease, request) = replayed.request(221)?;
    let store = NativeMutationStore::open(&replayed.state, std::slice::from_ref(&replayed.project))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Published {
                Err(MutationError::Io)
            } else {
                Ok(())
            }
        }),
        Err(MutationError::RecoveryRequired)
    );
    let restarted =
        NativeMutationStore::open(&replayed.state, std::slice::from_ref(&replayed.project))
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        restarted
            .commit(&lease, &request, &Continue)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    assert!(!replayed.project.join(temp_name(&request.id, 0)).exists());

    let cleaned = Fixture::new("cleaned-unrecorded")?;
    let (_backend, lease, request) = cleaned.request(222)?;
    let store = NativeMutationStore::open(&cleaned.state, std::slice::from_ref(&cleaned.project))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Cleaned {
                Err(MutationError::Io)
            } else {
                Ok(())
            }
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert!(!cleaned.project.join(temp_name(&request.id, 0)).exists());
    assert_eq!(
        store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );

    let committed = Fixture::new("committed-response")?;
    let (_backend, lease, request) = committed.request(223)?;
    let store =
        NativeMutationStore::open(&committed.state, std::slice::from_ref(&committed.project))
            .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store.commit_checked(&lease, &request, &Continue, |phase| {
            if phase == CommitCheckpoint::Committed {
                Err(MutationError::Io)
            } else {
                Ok(())
            }
        }),
        Err(MutationError::RecoveryRequired)
    );
    assert_eq!(
        store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    assert!(!committed.project.join(temp_name(&request.id, 0)).exists());
    Ok(())
}

#[test]
fn terminal_record_with_an_occupied_reserved_name_stays_recovery_required() -> Result<(), String> {
    let fixture = Fixture::new("terminal-reserved-name")?;
    let (_backend, lease, request) = fixture.request(224)?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        store
            .commit(&lease, &request, &Continue)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    let reserved = fixture.project.join(temp_name(&request.id, 0));
    std::fs::write(&reserved, b"foreign terminal bytes").map_err(|error| error.to_string())?;
    assert_eq!(
        store
            .receipt(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        store
            .recover(&lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::RecoveryRequired
    );
    assert_eq!(
        std::fs::read(&reserved).map_err(|error| error.to_string())?,
        b"foreign terminal bytes"
    );
    Ok(())
}

#[test]
fn another_workspace_cannot_promote_a_staging_journal() -> Result<(), String> {
    let first = Fixture::new("staging-authority-a")?;
    let second = Fixture::new("staging-authority-b")?;
    let (_first_backend, first_lease, _first_request) = first.request(230)?;
    let (_second_backend, second_lease, second_request) = second.request(231)?;
    let roots = [first.project.clone(), second.project.clone()];
    let store =
        NativeMutationStore::open(&first.state, &roots).map_err(|error| format!("{error:?}"))?;
    store
        .commit(&second_lease, &second_request, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let final_name = journal_name(&second_request.id);
    let staging_name = format!(".{final_name}.staging");
    std::fs::rename(
        first.state.join(&final_name),
        first.state.join(&staging_name),
    )
    .map_err(|error| error.to_string())?;

    let snapshot = || -> Result<Vec<(String, Vec<u8>)>, String> {
        let mut files = std::fs::read_dir(&first.state)
            .map_err(|error| error.to_string())?
            .map(|entry| {
                let entry = entry.map_err(|error| error.to_string())?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| "utf8".to_owned())?;
                let bytes = std::fs::read(entry.path()).map_err(|error| error.to_string())?;
                Ok((name, bytes))
            })
            .collect::<Result<Vec<_>, String>>()?;
        files.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(files)
    };
    let before = snapshot()?;
    assert_eq!(
        store.receipt(&first_lease, &second_request.id),
        Err(MutationError::PermissionDenied)
    );
    assert_eq!(snapshot()?, before);
    assert!(first.state.join(staging_name).exists());
    assert!(!first.state.join(final_name).exists());
    Ok(())
}

#[test]
fn second_store_observes_global_lock_as_busy() -> Result<(), String> {
    let fixture = Fixture::new("two-stores")?;
    let (_backend, lease, request) = fixture.request(300)?;
    let first = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    let second = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    let mut checked = false;
    let result = first.commit_checked(&lease, &request, &Continue, |phase| {
        if phase == CommitCheckpoint::Prepared {
            checked = true;
            assert_eq!(
                second.commit(&lease, &request, &Continue),
                Err(MutationError::Busy)
            );
        }
        Ok(())
    });
    assert!(checked);
    assert_eq!(
        result.map_err(|error| format!("{error:?}"))?.state,
        MutationState::Committed
    );
    Ok(())
}

#[test]
fn workspace_patch_preserves_unchanged_inherited_local_lints() {
    let before = br#"[workspace]
[lints]
workspace = true
[workspace.lints.rust]
unsafe_code = "warn"
"#;
    let after = br#"[workspace]
[lints]
workspace = true
[workspace.lints.rust]
unsafe_code = "forbid"
"#;
    assert_eq!(validate_manifest_patch(before, after), Ok(()));
}

#[test]
fn canonical_bundle_order_handles_module_file_and_sibling_module_directory() -> Result<(), String> {
    let fixture = Fixture::new("format-module-order")?;
    std::fs::create_dir(fixture.project.join("src/parser")).map_err(|error| error.to_string())?;
    std::fs::write(fixture.project.join("src/parser.rs"), b"pub mod ast;\n")
        .map_err(|error| error.to_string())?;
    std::fs::write(
        fixture.project.join("src/parser/ast.rs"),
        b"pub fn ast( )->u8{1}\n",
    )
    .map_err(|error| error.to_string())?;
    let backend = SecureProjects::new(std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    let opened = backend
        .open(fixture.project.to_str().ok_or("utf8")?, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let before = backend
        .source(&opened.lease, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let module = before
        .files()
        .iter()
        .position(|file| file.path() == "src/parser.rs")
        .ok_or("module")?;
    let child = before
        .files()
        .iter()
        .position(|file| file.path() == "src/parser/ast.rs")
        .ok_or("child")?;
    assert!(module < child);
    let after_files = before
        .files()
        .iter()
        .map(|file| {
            let bytes = match file.path() {
                "src/parser.rs" => b"pub mod ast;\n\n".to_vec(),
                "src/parser/ast.rs" => b"pub fn ast() -> u8 {\n    1\n}\n".to_vec(),
                _ => file.bytes().to_vec(),
            };
            SourceFile::new(file.path().to_owned(), bytes)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{error:?}"))?;
    let after = SourceBundle::with_directories(after_files, before.directories().to_vec())
        .map_err(|error| format!("{error:?}"))?;
    let candidate = MutationCandidate {
        kind: MutationKind::FormatApply,
        before,
        after,
        validation: "module-order".to_owned(),
    };
    let request = MutationCommit {
        id: MutationId::new("mut_00000000000000000000000000000225".to_owned())
            .map_err(|error| format!("{error:?}"))?,
        digest: mutation_digest(&candidate).map_err(|error| format!("{error:?}"))?,
        key: IdempotencyKey::new("module-order".to_owned())
            .map_err(|error| format!("{error:?}"))?,
        candidate,
    };
    let store = NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    let receipt = store
        .commit(&opened.lease, &request, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(receipt.state, MutationState::Committed);
    assert_eq!(receipt.files.len(), 2);
    assert_eq!(receipt.files[0].path, "src/parser.rs");
    assert_eq!(receipt.files[1].path, "src/parser/ast.rs");
    Ok(())
}

#[test]
fn protected_nested_swap_rejects_a_symlinked_parent_that_plain_swap_follows() -> Result<(), String>
{
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new("rename-flags-negative")?;
    let store = NativeMutationStore::open(&fixture.state, std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    std::fs::create_dir(fixture.project.join("real")).map_err(|error| error.to_string())?;
    std::fs::write(fixture.project.join("real/target"), b"target")
        .map_err(|error| error.to_string())?;
    std::fs::write(fixture.project.join("probe.swap"), b"probe")
        .map_err(|error| error.to_string())?;
    symlink("real", fixture.project.join("alias")).map_err(|error| error.to_string())?;
    let root = store
        .projects
        .roots
        .iter()
        .find(|root| root.path == fixture.project)
        .ok_or("root")?;
    assert!(
        renameat_with(
            &root.directory,
            "probe.swap",
            &root.directory,
            "alias/target",
            RenameFlags::from_bits_retain(RENAME_SAFE_SWAP),
        )
        .is_err()
    );
    assert_eq!(
        std::fs::read(fixture.project.join("probe.swap")).map_err(|error| error.to_string())?,
        b"probe"
    );
    assert_eq!(
        std::fs::read(fixture.project.join("real/target")).map_err(|error| error.to_string())?,
        b"target"
    );
    renameat_with(
        &root.directory,
        "probe.swap",
        &root.directory,
        "alias/target",
        RenameFlags::EXCHANGE,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(
        std::fs::read(fixture.project.join("probe.swap")).map_err(|error| error.to_string())?,
        b"target"
    );
    assert_eq!(
        std::fs::read(fixture.project.join("real/target")).map_err(|error| error.to_string())?,
        b"probe"
    );
    Ok(())
}

#[test]
fn maximum_format_journal_cost_is_bounded_per_global_phase() -> Result<(), String> {
    use std::time::Instant;

    const FILES: usize = 128;
    const BYTES_PER_FILE: usize = (16 * 1024 * 1024) / FILES;
    let mut before_files = Vec::with_capacity(FILES);
    let mut after_files = Vec::with_capacity(FILES);
    for index in 0..FILES {
        let path = format!("f{index:03}.rs");
        let before = vec![b'a'; BYTES_PER_FILE];
        let mut after = before.clone();
        after[0] = b'b';
        before_files
            .push(SourceFile::new(path.clone(), before).map_err(|error| format!("{error:?}"))?);
        after_files.push(SourceFile::new(path, after).map_err(|error| format!("{error:?}"))?);
    }
    let before = SourceBundle::new(before_files).map_err(|error| format!("{error:?}"))?;
    let after = SourceBundle::new(after_files).map_err(|error| format!("{error:?}"))?;
    let candidate = MutationCandidate {
        kind: MutationKind::FormatApply,
        before: before.clone(),
        after: after.clone(),
        validation: "maximum-format-journal".to_owned(),
    };
    let id = MutationId::new("mut_ffffffffffffffffffffffffffffffff".to_owned())
        .map_err(|error| format!("{error:?}"))?;
    let digest = mutation_digest(&candidate).map_err(|error| format!("{error:?}"))?;
    let files = before
        .files()
        .iter()
        .enumerate()
        .map(|(index, file)| JournalMutationFile {
            path: file.path().to_owned(),
            temp_path: temp_name(&id, index),
            source_node: JournalNode {
                device: 1,
                inode: index as u64 + 1,
            },
            staged_node: Some(JournalNode {
                device: 1,
                inode: index as u64 + FILES as u64 + 1,
            }),
        })
        .collect();
    let mut body = JournalBody {
        id: id.as_str().to_owned(),
        digest: digest.as_str().to_owned(),
        key: "maximum-format-journal".to_owned(),
        operation: "format_apply".to_owned(),
        workspace_path: "/private/tmp/rust-mcp-format-performance".to_owned(),
        workspace_device: 1,
        workspace_inode: 1,
        files,
        validation: candidate.validation,
        sequence: 0,
        phase: JournalPhase::Prepared,
        before: JournalBundle::from(&before),
        after: JournalBundle::from(&after),
        legacy_v1: false,
        format: JournalFormat::V2,
    };
    let worst_case_len = worst_case_record_len(&body).map_err(|error| format!("{error:?}"))?;
    assert!(worst_case_len <= MAX_JOURNAL_BYTES);
    let started = Instant::now();
    let mut maximum_encoded = Vec::new();
    let mut total_encoded = 0_u64;
    for phase in [
        JournalPhase::Prepared,
        JournalPhase::Scratch,
        JournalPhase::Staged,
        JournalPhase::Applying,
        JournalPhase::Published,
        JournalPhase::Committed,
    ] {
        body.phase = phase;
        body.sequence += 1;
        let encoded = encode(body.clone()).map_err(|error| format!("{error:?}"))?;
        assert!(encoded.len() <= worst_case_len);
        total_encoded += encoded.len() as u64;
        if encoded.len() > maximum_encoded.len() {
            maximum_encoded = encoded;
        }
    }
    let encode_millis = started.elapsed().as_millis();
    let decode_started = Instant::now();
    let decoded = decode(&maximum_encoded).map_err(|error| format!("{error:?}"))?;
    let decode_millis = decode_started.elapsed().as_millis();
    assert_eq!(decoded.files.len(), FILES);
    assert_eq!(
        decoded
            .before
            .decode()
            .map_err(|error| format!("{error:?}"))?,
        before
    );
    assert_eq!(
        decoded
            .after
            .decode()
            .map_err(|error| format!("{error:?}"))?,
        after
    );
    eprintln!(
        "M2_02_JOURNAL_COST encoded_max={} worst_case={} encoded_six_phases={} encode_six_ms={} decode_one_ms={}",
        maximum_encoded.len(),
        worst_case_len,
        total_encoded,
        encode_millis,
        decode_millis
    );
    Ok(())
}

#[test]
#[ignore = "explicit release-profile APFS ceiling measurement"]
fn measure_real_format_commit_replay_recovery_and_index_ceiling() -> Result<(), String> {
    use rust_engineering_domain::SOURCE_MAX_TOTAL_BYTES;
    use std::time::Instant;

    const FILES: usize = 128;
    let fixture = Fixture::new("format-real-ceiling")?;
    std::fs::remove_dir_all(fixture.project.join("src")).map_err(|error| error.to_string())?;
    std::fs::create_dir(fixture.project.join("src")).map_err(|error| error.to_string())?;
    let manifest_bytes = std::fs::metadata(fixture.project.join("Cargo.toml"))
        .map_err(|error| error.to_string())?
        .len() as usize;
    let rust_budget = SOURCE_MAX_TOTAL_BYTES
        .checked_sub(manifest_bytes)
        .ok_or("manifest budget")?;
    let base_size = rust_budget / FILES;
    let remainder = rust_budget % FILES;
    for index in 0..FILES {
        let size = base_size + usize::from(index < remainder);
        let name = if index == 0 {
            "lib.rs".to_owned()
        } else {
            format!("f{index:03}.rs")
        };
        std::fs::write(fixture.project.join("src").join(name), vec![b'a'; size])
            .map_err(|error| error.to_string())?;
    }
    let backend = SecureProjects::new(std::slice::from_ref(&fixture.project))
        .map_err(|error| format!("{error:?}"))?;
    let opened = backend
        .open(fixture.project.to_str().ok_or("utf8")?, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let before = backend
        .source(&opened.lease, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        before
            .files()
            .iter()
            .map(|file| file.bytes().len())
            .sum::<usize>(),
        SOURCE_MAX_TOTAL_BYTES
    );
    let after_files = before
        .files()
        .iter()
        .map(|file| {
            let mut bytes = file.bytes().to_vec();
            if file.path().ends_with(".rs") {
                bytes[0] = b'b';
            }
            SourceFile::new(file.path().to_owned(), bytes)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("{error:?}"))?;
    let after = SourceBundle::with_directories(after_files, before.directories().to_vec())
        .map_err(|error| format!("{error:?}"))?;
    let candidate = MutationCandidate {
        kind: MutationKind::FormatApply,
        before,
        after,
        validation: "real-format-ceiling".to_owned(),
    };
    let request = MutationCommit {
        id: MutationId::new("mut_eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee".to_owned())
            .map_err(|error| format!("{error:?}"))?,
        digest: mutation_digest(&candidate).map_err(|error| format!("{error:?}"))?,
        key: IdempotencyKey::new("real-format-ceiling".to_owned())
            .map_err(|error| format!("{error:?}"))?,
        candidate,
    };
    let store = NativeMutationStore::open_for_kind(
        &fixture.state,
        std::slice::from_ref(&fixture.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    let commit_started = Instant::now();
    let committed = store
        .commit(&opened.lease, &request, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let commit_millis = commit_started.elapsed().as_millis();
    assert_eq!(committed.state, MutationState::Committed);
    assert_eq!(committed.files.len(), FILES);
    let replay_started = Instant::now();
    assert_eq!(
        store
            .commit(&opened.lease, &request, &Continue)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    let replay_millis = replay_started.elapsed().as_millis();
    let recovery_started = Instant::now();
    assert_eq!(
        store
            .recover(&opened.lease, &request.id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::Committed
    );
    let recovery_millis = recovery_started.elapsed().as_millis();
    let stored_bytes = store.list_records().map_err(|error| format!("{error:?}"))?[0].stored_bytes;

    let index_fixture = Fixture::new("format-index-ceiling")?;
    let (index_backend, index_lease, new_request) = index_fixture.format_request(999)?;
    let index_store = NativeMutationStore::open_for_kind(
        &index_fixture.state,
        std::slice::from_ref(&index_fixture.project),
        MutationKind::FormatApply,
    )
    .map_err(|error| format!("{error:?}"))?;
    let index_before = index_backend
        .source(&index_lease, &Continue)
        .map_err(|error| format!("{error:?}"))?;
    let no_change = MutationCandidate {
        kind: MutationKind::FormatApply,
        before: index_before.clone(),
        after: index_before,
        validation: "index-ceiling".to_owned(),
    };
    let digest = mutation_digest(&no_change).map_err(|error| format!("{error:?}"))?;
    for index in 0..MAX_JOURNALS {
        let id =
            MutationId::new(format!("mut_{index:032x}")).map_err(|error| format!("{error:?}"))?;
        let body = JournalBody {
            id: id.as_str().to_owned(),
            digest: digest.as_str().to_owned(),
            key: format!("index-ceiling-{index}"),
            operation: "format_apply".to_owned(),
            workspace_path: index_lease.path.to_str().ok_or("utf8")?.to_owned(),
            workspace_device: index_lease.node.device,
            workspace_inode: index_lease.node.inode,
            files: Vec::new(),
            validation: no_change.validation.clone(),
            sequence: 0,
            phase: JournalPhase::NoChange,
            before: JournalBundle::from(&no_change.before),
            after: JournalBundle::from(&no_change.after),
            legacy_v1: false,
            format: JournalFormat::V2,
        };
        index_store
            .state
            .write_new(
                &journal_name(&id),
                &encode(body).map_err(|error| format!("{error:?}"))?,
            )
            .map_err(|error| format!("{error:?}"))?;
    }
    let list_started = Instant::now();
    assert_eq!(
        index_store
            .list_records()
            .map_err(|error| format!("{error:?}"))?
            .len(),
        MAX_JOURNALS
    );
    let list_millis = list_started.elapsed().as_millis();
    let exact_id = MutationId::new(format!("mut_{:032x}", MAX_JOURNALS / 2))
        .map_err(|error| format!("{error:?}"))?;
    let exact_started = Instant::now();
    assert_eq!(
        index_store
            .receipt(&index_lease, &exact_id)
            .map_err(|error| format!("{error:?}"))?
            .state,
        MutationState::NoChange
    );
    let exact_receipt_millis = exact_started.elapsed().as_millis();
    let refusal_started = Instant::now();
    assert_eq!(
        index_store.commit(&index_lease, &new_request, &Continue),
        Err(MutationError::LimitExceeded)
    );
    let quota_refusal_millis = refusal_started.elapsed().as_millis();
    eprintln!(
        "M2_02_REAL_COST commit_ms={commit_millis} replay_ms={replay_millis} recovery_ms={recovery_millis} stored_bytes={stored_bytes} index_list_ms={list_millis} exact_receipt_ms={exact_receipt_millis} quota_refusal_ms={quota_refusal_millis}"
    );
    Ok(())
}
