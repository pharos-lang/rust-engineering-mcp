#![cfg(target_os = "macos")]
//! M8-03 D12 §6(a)/(b) fixtures: real state left on disk through the crate's
//! public API, for an older (v0.3.0) binary to read.
//!
//! `leaves_a_committed_journal_of_a_kind_unknown_to_v0_3_0_and_a_known_control_journal`
//! leaves a real, terminal `analyzer_action_apply`
//! journal — a kind v0.3.0 does not recognize — plus a real, terminal
//! `manifest_patch` control journal in a separate state root, of a kind
//! v0.3.0 *does* recognize (the same public-API mechanism:
//! `SecureProjects`, `open_for_kind`, `commit`, `mutation_digest`). The
//! control journal is the positive control R-1 asked for: it proves that
//! v0.3.0's `mutation list` rejection of the `analyzer_action_apply` journal
//! (driven by `scripts/test-m8-rollback.py` scenario (a)) is caused by the
//! unrecognized `operation` kind specifically, and not by some unrelated
//! incompatibility (a lock, a permission, a malformed envelope) that would
//! produce the same `recovery_required` response regardless of kind. Run
//! v0.3.0's `mutation list` over a state root that holds *only* the control
//! journal, and it must list it cleanly.
//!
//! These are genuine integration tests (`tests/rollback_native.rs`), compiled
//! against each crate's public surface only — unlike
//! `tests/support/native_mutation.rs` (included into
//! `crates/project-adapter/src/filesystem/macos/mutation.rs` via `#[path]`
//! as a `#[cfg(test)] mod`), neither can reach the private `commit_checked`
//! checkpoint hook that those unit tests use to interrupt a commit mid-phase.
//! A full, committed `analyzer_action_apply` record demonstrates the same
//! downgrade hazard: `decode_envelope` rejects the unrecognized `operation`
//! string before any effect regardless of the journal's phase (see
//! `crates/project-adapter/src/filesystem/macos/mutation.rs`, `operation_kind`
//! and its use inside `decode_envelope`), so a terminal record on disk is
//! sufficient to exercise the scenario an older `mutation list` must fail
//! closed against, and the journal is never pruned automatically.
//!
//! `quality_artifact_fixture::leaves_a_validated_m3_quality_artifact_for_an_older_binary_to_read`
//! leaves a real, published ADR-061 M3 quality artifact for `scripts/test-m8-rollback.py`
//! scenario (b) to hand to `quality-artifacts recover --json` on both binaries
//! (R-2): the native M3 store (`NativeQualityArtifactStore`) is macOS+aarch64
//! only, unlike the M2 mutation store above, so that fixture is gated
//! separately and does not run on macOS/x86_64.
//!
//! Both are ignored by default; driven once by `scripts/test-m8-rollback.py`
//! via: `cargo test -p rust-engineering-project --locked --offline --test
//! rollback_native -- --ignored`.

use rust_engineering_application::{
    OperationControl, ProjectBackend, ProjectError, ProjectSourceBackend,
};
use rust_engineering_domain::{
    IdempotencyKey, MutationCandidate, MutationCommit, MutationId, MutationKind, MutationState,
    SourceBundle, SourceFile,
};
use rust_engineering_project::{
    SecureProjects,
    mutation_store::{NativeMutationStore, mutation_digest},
};
use std::{env, fs, os::unix::fs::PermissionsExt, path::PathBuf};

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

/// The value the driver later passes as `mutation list --state-root`; the
/// live journal lives one level below, at `<STATE_ROOT>/rust-mcp-mutations-v1`
/// (the same namespace `crates/mcp-server/src/host_config.rs` computes for
/// `serve`, and `crates/mcp-server/src/mutation_cli.rs` opens for `list`).
const STATE_ROOT_ENV: &str = "RUST_MCP_ROLLBACK_STATE_ROOT";
/// A second, disjoint state root holding only the `manifest_patch` control
/// journal, so an older binary can list it in isolation from the
/// unrecognized-kind journal above.
const CONTROL_STATE_ROOT_ENV: &str = "RUST_MCP_ROLLBACK_CONTROL_STATE_ROOT";
/// A sibling directory the fixture uses as the analyzer-authorized workspace;
/// never nested inside either state root (`NativeMutationStore::open_for_kind`
/// refuses an overlapping pair before any effect).
const PROJECT_ROOT_ENV: &str = "RUST_MCP_ROLLBACK_PROJECT_ROOT";

#[test]
#[ignore]
fn leaves_a_committed_journal_of_a_kind_unknown_to_v0_3_0_and_a_known_control_journal()
-> TestResult<()> {
    let state_root = PathBuf::from(
        env::var_os(STATE_ROOT_ENV).ok_or_else(|| format!("{STATE_ROOT_ENV} is not set"))?,
    );
    let control_state_root = PathBuf::from(
        env::var_os(CONTROL_STATE_ROOT_ENV)
            .ok_or_else(|| format!("{CONTROL_STATE_ROOT_ENV} is not set"))?,
    );
    let project_root = PathBuf::from(
        env::var_os(PROJECT_ROOT_ENV).ok_or_else(|| format!("{PROJECT_ROOT_ENV} is not set"))?,
    );
    if !state_root.is_absolute() || !control_state_root.is_absolute() || !project_root.is_absolute()
    {
        return Err(format!(
            "{STATE_ROOT_ENV}, {CONTROL_STATE_ROOT_ENV} and {PROJECT_ROOT_ENV} must all be \
             absolute paths"
        ));
    }
    for (a, b) in [
        (&state_root, &project_root),
        (&control_state_root, &project_root),
        (&state_root, &control_state_root),
    ] {
        if a.starts_with(b) || b.starts_with(a) {
            return Err("state roots and the project root must not be nested".to_owned());
        }
    }

    ck!(fs::create_dir_all(project_root.join("src")));
    ck!(fs::write(
        project_root.join("Cargo.toml"),
        b"[package]\nname = \"rollback-fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
    ));
    ck!(fs::write(
        project_root.join("src/lib.rs"),
        b"pub fn value() -> u8 { 1 }\n"
    ));

    let mutations_dir = state_root.join("rust-mcp-mutations-v1");
    ck!(fs::create_dir_all(&mutations_dir));
    ck!(fs::set_permissions(
        &mutations_dir,
        fs::Permissions::from_mode(0o700)
    ));
    let control_mutations_dir = control_state_root.join("rust-mcp-mutations-v1");
    ck!(fs::create_dir_all(&control_mutations_dir));
    ck!(fs::set_permissions(
        &control_mutations_dir,
        fs::Permissions::from_mode(0o700)
    ));

    let backend = ck!(SecureProjects::new(std::slice::from_ref(&project_root)));
    let opened = ck!(backend.open(project_root.to_str().ok_or("utf8")?, &Continue));

    // The unknown-kind journal: a committed analyzer action.
    let before = ck!(backend.source(&opened.lease, &Continue));
    let files = ck!(before
        .files()
        .iter()
        .map(|file| {
            let bytes = if file.path() == "src/lib.rs" {
                b"pub fn value() -> u8 {\n    1\n}\n".to_vec()
            } else {
                file.bytes().to_vec()
            };
            SourceFile::new(file.path().to_owned(), bytes)
        })
        .collect::<Result<Vec<_>, _>>());
    let after = ck!(SourceBundle::with_directories(
        files,
        before.directories().to_vec()
    ));
    let candidate = MutationCandidate {
        kind: MutationKind::AnalyzerActionApply,
        before,
        after,
        validation: "rust-analyzer=native;operation=m8-rollback-fixture".to_owned(),
    };
    let commit = MutationCommit {
        id: ck!(MutationId::new(
            "mut_00000000000000000000000000d12a01".to_owned()
        )),
        digest: ck!(mutation_digest(&candidate)),
        key: ck!(IdempotencyKey::new(
            "m8-rollback-analyzer-action-apply".to_owned()
        )),
        candidate,
    };
    let store = ck!(NativeMutationStore::open_for_kind(
        &mutations_dir,
        std::slice::from_ref(&project_root),
        MutationKind::AnalyzerActionApply,
    ));
    let receipt = ck!(store.commit(&opened.lease, &commit, &Continue));
    if receipt.state != MutationState::Committed {
        return Err(format!(
            "expected a committed analyzer_action_apply journal, got {:?}",
            receipt.state
        ));
    }
    let journal = mutations_dir.join(format!("journal-{}.json", commit.id.as_str()));
    if !journal.is_file() {
        return Err(
            "expected the analyzer_action_apply journal to remain on disk after commit".to_owned(),
        );
    }

    // The control journal: a `manifest_patch`, a kind v0.3.0 recognizes,
    // written through its own store into its own state root. Only a trailing
    // comment changes, so `validate_manifest_patch`'s structural TOML
    // comparison still passes, but the raw bytes differ, so the journal
    // commits (rather than the semantic-no-op `NoChange` terminal state).
    let current = ck!(backend.source(&opened.lease, &Continue));
    let control_files = ck!(current
        .files()
        .iter()
        .map(|file| {
            let bytes = if file.path() == "Cargo.toml" {
                let mut updated = file.bytes().to_vec();
                updated.extend_from_slice(b"\n# m8-rollback-control\n");
                updated
            } else {
                file.bytes().to_vec()
            };
            SourceFile::new(file.path().to_owned(), bytes)
        })
        .collect::<Result<Vec<_>, _>>());
    let control_after = ck!(SourceBundle::with_directories(
        control_files,
        current.directories().to_vec()
    ));
    let control_candidate = MutationCandidate {
        kind: MutationKind::ManifestPatch,
        before: current,
        after: control_after,
        validation: "m8-rollback-control=manifest_patch".to_owned(),
    };
    let control_commit = MutationCommit {
        id: ck!(MutationId::new(
            "mut_00000000000000000000000000d12b02".to_owned()
        )),
        digest: ck!(mutation_digest(&control_candidate)),
        key: ck!(IdempotencyKey::new(
            "m8-rollback-manifest-patch-control".to_owned()
        )),
        candidate: control_candidate,
    };
    let control_store = ck!(NativeMutationStore::open_for_kind(
        &control_mutations_dir,
        std::slice::from_ref(&project_root),
        MutationKind::ManifestPatch,
    ));
    let control_receipt = ck!(control_store.commit(&opened.lease, &control_commit, &Continue));
    if control_receipt.state != MutationState::Committed {
        return Err(format!(
            "expected a committed manifest_patch control journal, got {:?}",
            control_receipt.state
        ));
    }
    let control_journal =
        control_mutations_dir.join(format!("journal-{}.json", control_commit.id.as_str()));
    if !control_journal.is_file() {
        return Err(
            "expected the manifest_patch control journal to remain on disk after commit".to_owned(),
        );
    }
    Ok(())
}

/// The M3 quality-artifact store (ADR-061) is macOS+aarch64 only
/// (`crates/project-adapter/src/quality_artifact_store.rs`), unlike the M2
/// mutation store above, which is macOS-only regardless of architecture.
#[cfg(target_arch = "aarch64")]
mod quality_artifact_fixture {
    use rust_engineering_application::{
        QualityArtifactInput, QualityArtifactStore, QualityOwnerFacts, QualityReservation,
    };
    use rust_engineering_domain::{
        ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection,
        ArtifactSensitivity, ArtifactSource, GuestArtifactName, PayloadFormatVersion,
        PluginIdentity, QualityArtifactDraft, QualityArtifactId, QualityArtifactKind, QualityJobId,
        QualityMimeType, UtcInstant,
    };
    use rust_engineering_project::quality_artifact_store::{NativeQualityArtifactStore, recover};
    use std::{
        env, fs,
        os::unix::fs::PermissionsExt,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::TestResult;

    macro_rules! ck {
        ($value:expr) => {
            $value.map_err(|error| format!("{error:?}"))?
        };
    }

    /// The state root `scripts/test-m8-rollback.py` scenario (b) later passes
    /// as `quality-artifacts recover --state-root` to both binaries.
    const QUALITY_STATE_ROOT_ENV: &str = "RUST_MCP_ROLLBACK_QUALITY_STATE_ROOT";

    struct Payload(Vec<u8>);
    impl QualityArtifactInput for Payload {
        fn read(
            &mut self,
            buffer: &mut [u8],
        ) -> Result<usize, rust_engineering_domain::QualityArtifactError> {
            let take = self.0.len().min(buffer.len());
            buffer[..take].copy_from_slice(&self.0[..take]);
            self.0.drain(..take);
            Ok(take)
        }
    }

    #[test]
    #[ignore]
    fn leaves_a_validated_m3_quality_artifact_for_an_older_binary_to_read() -> TestResult<()> {
        let state_root = PathBuf::from(
            env::var_os(QUALITY_STATE_ROOT_ENV)
                .ok_or_else(|| format!("{QUALITY_STATE_ROOT_ENV} is not set"))?,
        );
        if !state_root.is_absolute() {
            return Err(format!("{QUALITY_STATE_ROOT_ENV} must be an absolute path"));
        }
        ck!(fs::create_dir_all(&state_root));
        ck!(fs::set_permissions(
            &state_root,
            fs::Permissions::from_mode(0o700)
        ));

        let mut store = ck!(NativeQualityArtifactStore::open(&state_root));
        // Owner binding is a pure domain-separated hash over these facts (see
        // `owner_binding` in `crates/project-adapter/src/filesystem/macos/quality.rs`);
        // it needs no real `SecureProjects` grant to exercise `recover`, which
        // reconciles the whole store regardless of owner.
        let facts = QualityOwnerFacts {
            granted_root_device: 1,
            granted_root_inode: 1,
            workspace_root: "/m8-rollback-fixture".to_owned(),
        };
        let owner = ck!(store.owner_binding(&facts));
        let now_unix = ck!(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| error.to_string()))
        .as_secs();
        let now = ck!(UtcInstant::from_unix_seconds(now_unix));
        let reservation = QualityReservation {
            job_id: QualityJobId::from_random_bytes([0xd1; 16]),
            owner_binding: owner,
            reserved_bytes: 64 * 1024,
            declared_members: 1,
            expires_at_utc: ck!(now.checked_add_seconds(600)),
        };
        ck!(store.reserve(&reservation));
        let payload = b"m8-rollback fixture evidence".to_vec();
        let ingest =
            ck!(store.ingest_member(&reservation, 0, 64 * 1024, &mut Payload(payload.clone())));
        let draft = QualityArtifactDraft {
            artifact_id: QualityArtifactId::from_random_bytes([0xd2; 16]),
            member_index: 0,
            kind: QualityArtifactKind::ToolLog,
            mime_type: QualityMimeType::TextPlain,
            payload_format_version: PayloadFormatVersion::Utf8LogV1,
            completeness: ArtifactCompleteness::Complete,
            sensitivity: ArtifactSensitivity::Public,
            created_at_utc: now.clone(),
            expires_at_utc: ck!(now.checked_add_seconds(600)),
            source: ArtifactSource {
                captured_source_sha256: [0xd3; 32],
                guest_name: GuestArtifactName::ToolLog,
                selection: ArtifactSelection::Workspace,
            },
            runtime: ArtifactRuntime {
                image_digest: [0xd4; 32],
                toolchain_identity: [0xd5; 32],
                plugin: ArtifactPlugin {
                    identity: PluginIdentity::Builtin,
                    version: 1,
                    digest: [0xd6; 32],
                },
                implementation_digest: [0xd7; 32],
            },
        };
        let descriptor = ck!(draft.into_descriptor(
            reservation.job_id.clone(),
            owner,
            ingest.sha256,
            ingest.size_bytes
        ));
        ck!(store.publish_descriptor(&reservation, &descriptor));

        let confirmed =
            ck!(store.read_chunk(owner, &descriptor.artifact_id, 0, payload.len() as u32));
        if confirmed.bytes != payload {
            return Err("published artifact did not read back its own bytes".to_owned());
        }
        drop(store);

        let report = ck!(recover(&state_root));
        if report.validated < 1 || report.quarantined != 0 {
            return Err(format!(
                "expected a validated M3 artifact and no quarantine, got validated={} \
                 quarantined={}",
                report.validated, report.quarantined
            ));
        }
        Ok(())
    }
}
