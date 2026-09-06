//! M3-06 operator surface for the ADR-061 durable quality store.
//!
//! The library oracles for recovery and pruning live in the project adapter;
//! this file qualifies the two commands an operator actually runs during an
//! upgrade or a rollback, against the shipped binary and a real store.

#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use rust_engineering_application::{
    QualityArtifactInput, QualityArtifactStore, QualityClockSource, QualityOwnerFacts,
    QualityReservation,
};
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection, ArtifactSensitivity,
    ArtifactSource, GuestArtifactName, PayloadFormatVersion, PluginIdentity,
    QUALITY_MAX_TTL_SECONDS, QualityArtifactDescriptor, QualityArtifactDraft, QualityArtifactError,
    QualityArtifactId, QualityArtifactKind, QualityJobId, QualityMimeType, UtcInstant,
};
use rust_engineering_project::quality_artifact_store::NativeQualityArtifactStore;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

type Check = Result<(), Box<dyn std::error::Error>>;

// Harness only: run the Cargo-built bootstrap, never project-supplied commands.
fn run(args: &[&OsStr]) -> Result<Output, Box<dyn std::error::Error>> {
    Ok(Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"))
        .env_clear()
        .args(args)
        .output()?)
}

fn report(output: &Output) -> Result<Value, Box<dyn std::error::Error>> {
    assert!(output.stderr.is_empty(), "{output:?}");
    Ok(serde_json::from_slice(&output.stdout)?)
}

struct Fixture {
    base: PathBuf,
    state: PathBuf,
    m2: PathBuf,
}
impl Fixture {
    fn new(label: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random)?;
        let base = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-qart-cli-{label}-{:032x}",
            u128::from_le_bytes(random)
        ));
        let state = base.join("state");
        fs::create_dir_all(&state)?;
        fs::set_permissions(&state, fs::Permissions::from_mode(0o700))?;
        // The M2 sibling on the same operator state root must survive both
        // commands untouched.
        let m2 = state.join("rust-mcp-mutations-v1");
        fs::create_dir_all(m2.join("journal"))?;
        fs::write(m2.join("journal/000001.json"), b"m2-journal-bytes")?;
        Ok(Self { base, state, m2 })
    }
    fn assert_m2_untouched(&self) -> Check {
        assert_eq!(
            fs::read(self.m2.join("journal/000001.json"))?,
            b"m2-journal-bytes"
        );
        assert_eq!(fs::read_dir(self.m2.join("journal"))?.count(), 1);
        Ok(())
    }
    fn store_dir(&self, child: &str) -> PathBuf {
        self.state.join("rust-mcp-quality-artifacts-v1").join(child)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

struct Bytes(Vec<u8>);
impl QualityArtifactInput for Bytes {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, QualityArtifactError> {
        let take = self.0.len().min(buffer.len());
        buffer[..take].copy_from_slice(&self.0[..take]);
        self.0.drain(..take);
        Ok(take)
    }
}

/// A clock the fixture names, so the published objects have exact ages when the
/// commands judge them against the host wall clock.
struct FixedClock(u64);
impl QualityClockSource for FixedClock {
    fn unix_seconds(&self) -> Result<u64, QualityArtifactError> {
        Ok(self.0)
    }
}

fn facts() -> QualityOwnerFacts {
    QualityOwnerFacts {
        granted_root_device: 16_777_232,
        granted_root_inode: 11,
        workspace_root: "/private/tmp/fixture-project".to_owned(),
    }
}

fn claim(
    seed: u8,
    owner: [u8; 32],
    expires_at_utc: UtcInstant,
) -> Result<QualityReservation, Box<dyn std::error::Error>> {
    Ok(QualityReservation {
        job_id: QualityJobId::from_random_bytes([seed; 16]),
        owner_binding: owner,
        reserved_bytes: 8 * 1024 * 1024,
        declared_members: 4,
        expires_at_utc,
    })
}

fn draft(
    seed: u8,
    created_at_utc: &UtcInstant,
    ttl: u64,
) -> Result<QualityArtifactDraft, Box<dyn std::error::Error>> {
    Ok(QualityArtifactDraft {
        artifact_id: QualityArtifactId::from_random_bytes([seed; 16]),
        member_index: 0,
        kind: QualityArtifactKind::ToolLog,
        mime_type: QualityMimeType::TextPlain,
        payload_format_version: PayloadFormatVersion::Utf8LogV1,
        completeness: ArtifactCompleteness::Complete,
        sensitivity: ArtifactSensitivity::Public,
        created_at_utc: created_at_utc.clone(),
        expires_at_utc: created_at_utc.checked_add_seconds(ttl)?,
        source: ArtifactSource {
            captured_source_sha256: [2; 32],
            guest_name: GuestArtifactName::ToolLog,
            selection: ArtifactSelection::Workspace,
        },
        runtime: ArtifactRuntime {
            image_digest: [3; 32],
            toolchain_identity: [4; 32],
            plugin: ArtifactPlugin {
                identity: PluginIdentity::Builtin,
                version: 1,
                digest: [5; 32],
            },
            implementation_digest: [6; 32],
        },
    })
}

fn publish(
    store: &mut NativeQualityArtifactStore,
    reservation: &QualityReservation,
    draft: QualityArtifactDraft,
    payload: &[u8],
) -> Result<QualityArtifactDescriptor, Box<dyn std::error::Error>> {
    let ingest = store.ingest_member(
        reservation,
        draft.member_index,
        4096,
        &mut Bytes(payload.to_vec()),
    )?;
    let descriptor = draft.into_descriptor(
        reservation.job_id.clone(),
        reservation.owner_binding,
        ingest.sha256,
        ingest.size_bytes,
    )?;
    store.publish_descriptor(reservation, &descriptor)?;
    Ok(descriptor)
}

/// One store holding a valid pair, an expired pair with its expired claim, and
/// an object no version of this store wrote.
fn populate(fixture: &Fixture) -> Check {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let past = UtcInstant::from_unix_seconds(now.saturating_sub(3_600))?;
    let mut store = NativeQualityArtifactStore::open(&fixture.state)?
        .with_clock_source(Box::new(FixedClock(past.unix_seconds())))?;
    let owner = store.owner_binding(&facts())?;
    let live = claim(1, owner, past.checked_add_seconds(QUALITY_MAX_TTL_SECONDS)?)?;
    store.reserve(&live)?;
    publish(
        &mut store,
        &live,
        draft(1, &past, QUALITY_MAX_TTL_SECONDS)?,
        b"valid",
    )?;
    let stale = claim(2, owner, past.checked_add_seconds(600)?)?;
    store.reserve(&stale)?;
    publish(&mut store, &stale, draft(2, &past, 600)?, b"expired")?;
    drop(store);
    fs::write(fixture.store_dir("descriptor").join("FOREIGN"), b"foreign")?;
    Ok(())
}

fn quality_artifacts(
    action: &str,
    state: &Path,
    json: bool,
) -> Result<Output, Box<dyn std::error::Error>> {
    let mut args: Vec<&OsStr> = vec![
        OsStr::new("quality-artifacts"),
        OsStr::new(action),
        OsStr::new("--state-root"),
        state.as_os_str(),
    ];
    if json {
        args.push(OsStr::new("--json"));
    }
    run(&args)
}

#[test]
fn prune_reclaims_only_expired_objects_and_recover_quarantines_the_unknown_one() -> Check {
    let fixture = Fixture::new("operator")?;
    populate(&fixture)?;

    let pruned = quality_artifacts("prune", &fixture.state, true)?;
    assert_eq!(pruned.status.code(), Some(0));
    let pruned = report(&pruned)?;
    assert_eq!(pruned["format_version"], 1);
    assert_eq!(pruned["status"], "passed");
    assert_eq!(pruned["action"], "prune");
    assert_eq!(pruned["error_code"], Value::Null);
    assert_eq!(pruned["data"]["removed"], 2, "{pruned}");
    assert!(pruned["data"]["reclaimed_bytes"].as_u64().unwrap_or(0) > 0);
    assert!(pruned["data"]["retained"].as_u64().unwrap_or(0) >= 1);
    // Pruning is not an eviction: it never quarantines and never removes an
    // object it does not understand.
    assert_eq!(fs::read_dir(fixture.store_dir("quarantine"))?.count(), 0);
    assert!(fixture.store_dir("descriptor").join("FOREIGN").exists());

    let recovered = quality_artifacts("recover", &fixture.state, true)?;
    assert_eq!(recovered.status.code(), Some(0));
    let recovered = report(&recovered)?;
    assert_eq!(recovered["status"], "passed");
    assert_eq!(recovered["action"], "recover");
    assert_eq!(recovered["data"]["validated"], 1, "{recovered}");
    assert_eq!(recovered["data"]["quarantined"], 1, "{recovered}");
    assert_eq!(recovered["data"]["clock_regression"], false);
    assert!(!fixture.store_dir("descriptor").join("FOREIGN").exists());

    // The default output is one bounded line, not the report.
    let text = quality_artifacts("prune", &fixture.state, false)?;
    assert_eq!(text.status.code(), Some(0));
    assert_eq!(text.stdout, b"prune: passed\n");
    fixture.assert_m2_untouched()
}

#[test]
fn a_clock_regression_blocks_prune_with_a_closed_code_until_recover_rebases_it() -> Check {
    let fixture = Fixture::new("regression")?;
    populate(&fixture)?;
    let watermark = fixture
        .state
        .join("rust-mcp-quality-artifacts-v1")
        .join("clock-watermark.json");
    let future = UtcInstant::from_unix_seconds(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs()
            .saturating_add(86_400),
    )?;
    fs::write(
        &watermark,
        format!("{{\"format_version\":1,\"observed_at_utc\":\"{future}\"}}"),
    )?;

    let blocked = quality_artifacts("prune", &fixture.state, true)?;
    assert_eq!(blocked.status.code(), Some(1));
    let blocked = report(&blocked)?;
    assert_eq!(blocked["status"], "blocked");
    assert_eq!(blocked["error_code"], "recovery_required");
    assert_eq!(blocked["data"], Value::Null);

    let recovered = quality_artifacts("recover", &fixture.state, true)?;
    assert_eq!(recovered.status.code(), Some(0));
    let recovered = report(&recovered)?;
    assert_eq!(recovered["data"]["clock_regression"], true);
    // Only the operator command re-bases the clock, and pruning works again
    // afterwards without any object having been guessed at.
    let pruned = quality_artifacts("prune", &fixture.state, true)?;
    assert_eq!(pruned.status.code(), Some(0));
    fixture.assert_m2_untouched()
}
