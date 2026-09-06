//! ADR-061 durable quality artifact store oracles.
//!
//! Ordinary tests run wherever `/private/tmp` is APFS, which is the normal
//! macOS host. The four `native_apfs_quality_*` tests are `#[ignore]`d because
//! they need a second process, real free-space accounting or real capacity
//! coupling with the M2 store; the gate runs them explicitly with `--ignored`.
//!
//! The two that spawn a helper process must run **one at a time** — as the gate
//! does, with `--exact --ignored --test-threads=1`. Spawning from the parallel
//! harness makes sessions on unrelated fixtures observe `Busy` on their own
//! `store.lock`, because the spawn transiently duplicates whatever descriptors
//! sibling threads hold. `--include-ignored` at full parallelism therefore
//! fails at random; it is not a mode this suite claims to support.

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod unsupported {
    use rust_engineering_domain::QualityArtifactError;
    use rust_engineering_project::quality_artifact_store::{
        NativeQualityArtifactStore, prune_expired, recover,
    };
    use std::path::Path;

    /// Linux, Windows and unqualified macOS architectures reject before a
    /// reservation, a gateway or any output.
    #[test]
    fn unsupported_platform_rejects_before_any_effect() {
        let path = Path::new("/nonexistent-state-root");
        assert!(matches!(
            NativeQualityArtifactStore::open(path).err(),
            Some(QualityArtifactError::UnsupportedPlatform)
        ));
        assert!(matches!(
            recover(path).err(),
            Some(QualityArtifactError::UnsupportedPlatform)
        ));
        assert!(matches!(
            prune_expired(path).err(),
            Some(QualityArtifactError::UnsupportedPlatform)
        ));
        assert!(!path.exists(), "the unsupported path created state");
    }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod apfs {
    use rust_engineering_application::{
        QUALITY_CURSOR_MAX_BYTES, QUALITY_INDEX_PAGE_MEMBERS, QualityArtifactInput,
        QualityArtifactStore, QualityClockSource, QualityFaultInjection, QualityFaultPoint,
        QualityOwnerFacts, QualityReservation,
    };
    use rust_engineering_domain::{
        ArtifactCompleteness, ArtifactPlugin, ArtifactRuntime, ArtifactSelection,
        ArtifactSensitivity, ArtifactSource, GuestArtifactName, PayloadFormatVersion,
        PluginIdentity, QUALITY_MAX_ARTIFACT_BYTES, QUALITY_MAX_GLOBAL_BYTES,
        QUALITY_MAX_JOB_BYTES, QUALITY_MAX_JOB_MEMBERS, QUALITY_MAX_OWNER_BYTES,
        QUALITY_MAX_TTL_SECONDS, QualityArtifactDescriptor, QualityArtifactDraft,
        QualityArtifactError, QualityArtifactId, QualityArtifactKind, QualityJobId,
        QualityMimeType, UtcInstant,
    };
    use rust_engineering_project::quality_artifact_store::{
        NativeQualityArtifactStore, prune_expired, recover,
    };
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    type Check = Result<(), Box<dyn std::error::Error>>;
    const STORE: &str = "rust-mcp-quality-artifacts-v1";

    struct Fixture {
        base: PathBuf,
        state: PathBuf,
        project: PathBuf,
    }
    impl Fixture {
        fn new(label: &str) -> Result<Self, Box<dyn std::error::Error>> {
            Self::with_state_root_mode(label, 0o700)
        }
        /// The state root is operator-supplied, so its mode is a real variable:
        /// M2 accepts any root this uid owns that nobody else may write.
        fn with_state_root_mode(
            label: &str,
            mode: u32,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            let mut random = [0_u8; 16];
            getrandom::fill(&mut random)?;
            let base = PathBuf::from("/private/tmp").join(format!(
                "rust-mcp-qart-{label}-{:032x}",
                u128::from_le_bytes(random)
            ));
            let state = base.join("state");
            let project = base.join("project");
            fs::create_dir_all(project.join("src"))?;
            fs::create_dir_all(&state)?;
            fs::set_permissions(&state, fs::Permissions::from_mode(mode))?;
            fs::write(
                project.join("Cargo.toml"),
                b"[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
            )?;
            fs::write(
                project.join("src/lib.rs"),
                b"pub fn answer() -> u8 { 42 }\n",
            )?;
            Ok(Self {
                base,
                state,
                project,
            })
        }
        fn open(&self) -> Result<NativeQualityArtifactStore, QualityArtifactError> {
            NativeQualityArtifactStore::open(&self.state)
        }
        fn store_dir(&self, child: &str) -> PathBuf {
            self.state.join(STORE).join(child)
        }
        fn entries(&self, child: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
            let mut names = Vec::new();
            for entry in fs::read_dir(self.store_dir(child))? {
                names.push(entry?.file_name().to_string_lossy().into_owned());
            }
            names.sort();
            Ok(names)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.base);
        }
    }

    struct Bytes {
        remaining: Vec<u8>,
        chunk: usize,
    }
    impl Bytes {
        fn of(bytes: Vec<u8>) -> Self {
            Self {
                remaining: bytes,
                chunk: 8 * 1024,
            }
        }
    }
    impl QualityArtifactInput for Bytes {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, QualityArtifactError> {
            let take = self.remaining.len().min(buffer.len()).min(self.chunk);
            buffer[..take].copy_from_slice(&self.remaining[..take]);
            self.remaining.drain(..take);
            Ok(take)
        }
    }

    /// A simulated crash or ENOSPC at one deterministic point.
    struct Fault {
        point: QualityFaultPoint,
        error: QualityArtifactError,
        passes: AtomicUsize,
    }
    impl Fault {
        fn boxed(
            point: QualityFaultPoint,
            error: QualityArtifactError,
            passes: usize,
        ) -> Box<dyn QualityFaultInjection> {
            Box::new(Self {
                point,
                error,
                passes: AtomicUsize::new(passes),
            })
        }
    }
    impl QualityFaultInjection for Fault {
        fn arrive(&self, point: QualityFaultPoint) -> Result<(), QualityArtifactError> {
            if point != self.point {
                return Ok(());
            }
            if self.passes.load(Ordering::SeqCst) == 0 {
                return Err(self.error);
            }
            self.passes.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn wall_now() -> Result<UtcInstant, Box<dyn std::error::Error>> {
        Ok(UtcInstant::from_unix_seconds(
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        )?)
    }

    fn facts(inode: u64) -> QualityOwnerFacts {
        QualityOwnerFacts {
            granted_root_device: 16777232,
            granted_root_inode: inode,
            workspace_root: "/private/tmp/fixture-project".to_owned(),
        }
    }

    fn job(seed: u8) -> QualityJobId {
        QualityJobId::from_random_bytes([seed; 16])
    }

    fn claim(
        seed: u8,
        owner: [u8; 32],
        bytes: u64,
        members: u16,
    ) -> Result<QualityReservation, Box<dyn std::error::Error>> {
        Ok(QualityReservation {
            job_id: job(seed),
            owner_binding: owner,
            reserved_bytes: bytes,
            declared_members: members,
            expires_at_utc: wall_now()?.checked_add_seconds(600)?,
        })
    }

    fn draft(
        artifact: u8,
        member_index: u16,
        ttl: u64,
    ) -> Result<QualityArtifactDraft, Box<dyn std::error::Error>> {
        let created = wall_now()?;
        Ok(QualityArtifactDraft {
            artifact_id: QualityArtifactId::from_random_bytes([artifact; 16]),
            member_index,
            kind: QualityArtifactKind::ToolLog,
            mime_type: QualityMimeType::TextPlain,
            payload_format_version: PayloadFormatVersion::Utf8LogV1,
            completeness: ArtifactCompleteness::Complete,
            sensitivity: ArtifactSensitivity::Public,
            expires_at_utc: created.checked_add_seconds(ttl)?,
            created_at_utc: created,
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

    /// Streams one member and commits it, exactly as the application layer does.
    fn publish(
        store: &mut NativeQualityArtifactStore,
        reservation: &QualityReservation,
        draft: QualityArtifactDraft,
        cap: u64,
        payload: &[u8],
    ) -> Result<QualityArtifactDescriptor, QualityArtifactError> {
        let member_index = draft.member_index;
        let ingest = store.ingest_member(
            reservation,
            member_index,
            cap,
            &mut Bytes::of(payload.to_vec()),
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

    fn sha256(bytes: &[u8]) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(bytes).into()
    }

    /// Every byte this store occupies on the volume, whatever its state.
    fn store_bytes(fixture: &Fixture) -> Result<u64, Box<dyn std::error::Error>> {
        let mut total = 0_u64;
        for child in ["blob", "descriptor", "reservation", "quarantine"] {
            for entry in fs::read_dir(fixture.store_dir(child))? {
                total = total.saturating_add(entry?.metadata()?.len());
            }
        }
        Ok(total)
    }

    fn quarantine_reasons(fixture: &Fixture) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let mut reasons = Vec::new();
        for name in fixture.entries("quarantine")? {
            if name.ends_with(".note") {
                reasons.push(fs::read_to_string(
                    fixture.store_dir("quarantine").join(name),
                )?);
            }
        }
        Ok(reasons)
    }

    /// The clock a TTL test moves by hand, in place of sleeping.
    ///
    /// An instant is a whole second, so a claim or a draft written `n` seconds
    /// ahead of `SystemTime::now()` really expires `n - frac(now)` seconds
    /// later: a test that sleeps its way past a one-second TTL is racing the
    /// store's own fsyncs for the sub-second remainder it happened to start
    /// with, and loses whenever a publication does not fit inside it. Moving
    /// this source names the instant instead, so every TTL boundary below is
    /// exact and no assertion depends on how fast the volume is.
    struct TestClock(Arc<AtomicU64>);
    impl TestClock {
        fn at(instant: &UtcInstant) -> Self {
            Self(Arc::new(AtomicU64::new(instant.unix_seconds())))
        }
        fn set(&self, instant: &UtcInstant) {
            self.0.store(instant.unix_seconds(), Ordering::SeqCst);
        }
        /// A second handle on the same instant, for the store to hold.
        fn source(&self) -> Box<dyn QualityClockSource> {
            Box::new(Self(Arc::clone(&self.0)))
        }
    }
    impl QualityClockSource for TestClock {
        fn unix_seconds(&self) -> Result<u64, QualityArtifactError> {
            Ok(self.0.load(Ordering::SeqCst))
        }
    }

    /// `claim`, anchored on an explicit instant instead of the host clock.
    fn claim_at(
        base: &UtcInstant,
        seed: u8,
        owner: [u8; 32],
        bytes: u64,
        ttl: u64,
    ) -> Result<QualityReservation, Box<dyn std::error::Error>> {
        Ok(QualityReservation {
            expires_at_utc: base.checked_add_seconds(ttl)?,
            ..claim(seed, owner, bytes, 4)?
        })
    }

    /// `draft`, created at an explicit instant instead of the host clock.
    fn draft_at(
        base: &UtcInstant,
        artifact: u8,
        member_index: u16,
        ttl: u64,
    ) -> Result<QualityArtifactDraft, Box<dyn std::error::Error>> {
        Ok(QualityArtifactDraft {
            created_at_utc: base.clone(),
            expires_at_utc: base.checked_add_seconds(ttl)?,
            ..draft(artifact, member_index, ttl)?
        })
    }

    #[test]
    fn publishes_exact_bytes_and_serves_bounded_chunks() -> Check {
        let fixture = Fixture::new("publish")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(1, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let payload = (0..40_000_u32)
            .map(|value| (value % 251) as u8)
            .collect::<Vec<_>>();
        let descriptor = publish(
            &mut store,
            &reservation,
            draft(1, 0, 600)?,
            64 * 1024,
            &payload,
        )?;
        assert_eq!(descriptor.size_bytes, payload.len() as u64);
        assert_eq!(descriptor.sha256, sha256(&payload));

        let chunk = store.read_chunk(owner, &descriptor.artifact_id, 0, 4096)?;
        assert_eq!(chunk.bytes, payload[..4096]);
        assert_eq!(chunk.offset, 0);
        // A read past the declared size is clamped, never padded from surplus.
        let tail = store.read_chunk(owner, &descriptor.artifact_id, 39_000, 4096)?;
        assert_eq!(tail.bytes, payload[39_000..]);
        assert_eq!(tail.bytes.len(), 1_000);
        assert!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 40_001, 16)
                .is_err()
        );
        // Publication truncated the preallocated surplus away.
        let blob = fixture
            .store_dir("blob")
            .join(format!("{}.blob", descriptor.artifact_id));
        assert_eq!(fs::metadata(&blob)?.len(), payload.len() as u64);
        assert_eq!(fs::metadata(&blob)?.permissions().mode() & 0o7777, 0o600);
        assert_eq!(
            fixture.entries("reservation")?,
            [format!("{}.reserve", job(1))]
        );
        store.release(&reservation)?;
        assert!(fixture.entries("reservation")?.is_empty());
        // Releasing a claim never removes published evidence.
        assert!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)
                .is_ok()
        );
        Ok(())
    }

    #[test]
    fn flood_beyond_the_exact_cap_publishes_nothing() -> Check {
        let fixture = Fixture::new("flood")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(2, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let flood = vec![7_u8; 40_000];
        assert_eq!(
            store
                .ingest_member(&reservation, 0, 1024, &mut Bytes::of(flood))
                .err(),
            Some(QualityArtifactError::QuotaExceeded)
        );
        assert!(fixture.entries("descriptor")?.is_empty());
        assert!(fixture.entries("blob")?.is_empty());
        // Only the job's own temporary was released; the claim itself remains.
        assert_eq!(
            fixture.entries("reservation")?,
            [format!("{}.reserve", job(2))]
        );
        // The exact cap still publishes.
        let exact = vec![7_u8; 1024];
        let descriptor = publish(&mut store, &reservation, draft(2, 0, 600)?, 1024, &exact)?;
        assert_eq!(descriptor.size_bytes, 1024);
        assert_eq!(descriptor.sha256, sha256(&exact));
        Ok(())
    }

    #[test]
    fn enospc_mid_stream_releases_only_the_known_temporary() -> Check {
        let fixture = Fixture::new("enospc")?;
        let mut store = fixture.open()?.with_fault_injection(Fault::boxed(
            QualityFaultPoint::IngestWrite,
            QualityArtifactError::QuotaExceeded,
            1,
        ));
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(3, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        assert_eq!(
            store
                .ingest_member(
                    &reservation,
                    0,
                    64 * 1024,
                    &mut Bytes::of(vec![1_u8; 24 * 1024])
                )
                .err(),
            Some(QualityArtifactError::QuotaExceeded)
        );
        assert!(fixture.entries("descriptor")?.is_empty());
        assert!(fixture.entries("blob")?.is_empty());
        assert_eq!(
            fixture.entries("reservation")?,
            [format!("{}.reserve", job(3))]
        );

        // Positive control: a later within-budget reservation still publishes.
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let later = claim(4, owner, 1024 * 1024, 4)?;
        store.reserve(&later)?;
        let payload = b"later within budget".to_vec();
        let descriptor = publish(&mut store, &later, draft(4, 0, 600)?, 4096, &payload)?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 4096)?
                .bytes,
            payload
        );
        Ok(())
    }

    #[test]
    fn crash_between_blob_and_descriptor_serves_no_blob() -> Check {
        let fixture = Fixture::new("crash-blob")?;
        let mut store = fixture.open()?.with_fault_injection(Fault::boxed(
            QualityFaultPoint::AfterBlobRename,
            QualityArtifactError::Io,
            0,
        ));
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(5, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let identifier = draft(5, 0, 600)?.artifact_id;
        assert_eq!(
            publish(
                &mut store,
                &reservation,
                draft(5, 0, 600)?,
                4096,
                b"evidence"
            )
            .err(),
            Some(QualityArtifactError::Io)
        );
        assert_eq!(fixture.entries("blob")?, [format!("{identifier}.blob")]);
        assert!(fixture.entries("descriptor")?.is_empty());
        drop(store);

        // Restart discards the uncommitted blob; it is never served.
        let mut store = fixture.open()?;
        assert!(fixture.entries("blob")?.is_empty());
        assert_eq!(
            store.read_chunk(owner, &identifier, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        assert!(fixture.entries("quarantine")?.is_empty());

        // Positive control: the full pair reads its exact digest.
        let reservation = claim(6, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(
            &mut store,
            &reservation,
            draft(6, 0, 600)?,
            4096,
            b"evidence",
        )?;
        assert_eq!(descriptor.sha256, sha256(b"evidence"));
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 4096)?
                .bytes,
            b"evidence"
        );
        Ok(())
    }

    #[test]
    fn crash_between_descriptor_and_directory_fsync_survives_when_complete() -> Check {
        let fixture = Fixture::new("crash-descriptor")?;
        let mut store = fixture.open()?.with_fault_injection(Fault::boxed(
            QualityFaultPoint::AfterDescriptorRename,
            QualityArtifactError::Io,
            0,
        ));
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(7, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let payload = vec![3_u8; 5_000];
        let identifier = draft(7, 0, 600)?.artifact_id;
        assert_eq!(
            publish(
                &mut store,
                &reservation,
                draft(7, 0, 600)?,
                64 * 1024,
                &payload
            )
            .err(),
            Some(QualityArtifactError::Io)
        );
        // The truncation this store owed itself was recorded durably before
        // publication, which is what makes the surplus provably its own.
        assert!(
            fixture
                .entries("reservation")?
                .contains(&format!("{identifier}.trunc")),
            "{:?}",
            fixture.entries("reservation")?
        );
        assert!(
            fs::metadata(fixture.store_dir("blob").join(format!("{identifier}.blob")))?.len()
                > payload.len() as u64
        );
        drop(store);

        // Durable completion is proven by schema, size and digest, so the pair
        // survives and the interrupted truncation is completed.
        let mut store = fixture.open()?;
        let chunk = store.read_chunk(owner, &identifier, 0, 5_000)?;
        assert_eq!(chunk.bytes, payload);
        assert_eq!(chunk.descriptor.sha256, sha256(&payload));
        assert!(fixture.entries("quarantine")?.is_empty());
        assert_eq!(
            fs::metadata(fixture.store_dir("blob").join(format!("{identifier}.blob")))?.len(),
            payload.len() as u64
        );
        // The marker is consumed exactly once: it can never license a later
        // surplus on the same artifact.
        assert!(
            !fixture
                .entries("reservation")?
                .contains(&format!("{identifier}.trunc"))
        );
        Ok(())
    }

    #[test]
    fn corrupt_or_unknown_objects_are_quarantined_with_a_closed_reason() -> Check {
        let fixture = Fixture::new("quarantine")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(8, owner, 1024 * 1024, 8)?;
        store.reserve(&reservation)?;
        let first = publish(&mut store, &reservation, draft(8, 0, 600)?, 4096, b"first")?;
        let second = publish(&mut store, &reservation, draft(9, 1, 600)?, 4096, b"second")?;
        drop(store);

        // Same length, different bytes: only the digest can detect it.
        fs::write(
            fixture
                .store_dir("blob")
                .join(format!("{}.blob", first.artifact_id)),
            b"FIRST",
        )?;
        // A descriptor that is no longer strict v1 data.
        fs::write(
            fixture
                .store_dir("descriptor")
                .join(format!("{}.json", second.artifact_id)),
            b"{\"format_version\":1,",
        )?;
        // An object this store never created.
        fs::write(fixture.store_dir("descriptor").join("EVIL"), b"x")?;
        // The M2 sibling must be visible and untouched throughout.
        let sibling = fixture.state.join("rust-mcp-mutations-v1");
        fs::create_dir_all(&sibling)?;
        fs::write(sibling.join("journal-keep.json"), b"m2-bytes")?;

        let mut store = fixture.open()?;
        assert_eq!(
            store.read_chunk(owner, &first.artifact_id, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        assert_eq!(
            store.read_chunk(owner, &second.artifact_id, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        let quarantined = fixture.entries("quarantine")?;
        // Three offending objects, each with its own closed reason note, plus
        // the blob of the malformed descriptor.
        assert!(quarantined.len() >= 6, "{quarantined:?}");
        let mut reasons = Vec::new();
        for name in &quarantined {
            if name.ends_with(".note") {
                reasons.push(fs::read_to_string(
                    fixture.store_dir("quarantine").join(name),
                )?);
            }
        }
        assert!(
            reasons.iter().any(|note| note.contains("digest_mismatch")),
            "{reasons:?}"
        );
        assert!(
            reasons
                .iter()
                .any(|note| note.contains("malformed_descriptor")),
            "{reasons:?}"
        );
        assert!(
            reasons.iter().any(|note| note.contains("unknown_name")),
            "{reasons:?}"
        );
        // No note carries a host path, a URI or foreign text.
        for note in &reasons {
            assert!(!note.contains('/'), "{note}");
            assert!(!note.contains("EVIL"), "{note}");
        }
        assert_eq!(fs::read(sibling.join("journal-keep.json"))?, b"m2-bytes");
        assert_eq!(fs::read_dir(&sibling)?.count(), 1);
        Ok(())
    }

    #[test]
    fn a_hardlinked_or_shortened_blob_is_never_served() -> Check {
        let fixture = Fixture::new("identity")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(10, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(&mut store, &reservation, draft(10, 0, 600)?, 4096, b"bytes")?;
        let blob = fixture
            .store_dir("blob")
            .join(format!("{}.blob", descriptor.artifact_id));
        let link = fixture.base.join("second-link");
        fs::hard_link(&blob, &link)?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)
                .err(),
            Some(QualityArtifactError::NotFound)
        );
        fs::remove_file(&link)?;
        assert!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)
                .is_ok()
        );
        // A blob shorter than its descriptor is a size mismatch, not a short read.
        fs::write(&blob, b"by")?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)
                .err(),
            Some(QualityArtifactError::NotFound)
        );
        Ok(())
    }

    #[test]
    fn owner_binding_separates_state_root_uid_and_granted_root() -> Check {
        let first = Fixture::new("binding-one")?;
        let second = Fixture::new("binding-two")?;
        let one = first.open()?;
        let two = second.open()?;
        let base = one.owner_binding(&facts(11))?;
        // A different granted inode, device or workspace root is a different owner.
        assert_ne!(base, one.owner_binding(&facts(12))?);
        let mut other = facts(11);
        other.granted_root_device += 1;
        assert_ne!(base, one.owner_binding(&other)?);
        let mut renamed = facts(11);
        renamed.workspace_root.push('x');
        assert_ne!(base, one.owner_binding(&renamed)?);
        // A different state root is a different owner for identical facts.
        assert_ne!(base, two.owner_binding(&facts(11))?);
        // The binding is stable across restarts of the same state root.
        assert_eq!(base, first.open()?.owner_binding(&facts(11))?);
        // Length-prefixed fields: no concatenation collision.
        let mut left = facts(11);
        left.workspace_root = "/a/bc".to_owned();
        let mut right = facts(11);
        right.workspace_root = "/ab/c".to_owned();
        assert_ne!(one.owner_binding(&left)?, one.owner_binding(&right)?);
        assert_eq!(
            one.owner_binding(&QualityOwnerFacts {
                workspace_root: String::new(),
                ..facts(11)
            })
            .err(),
            Some(QualityArtifactError::Unauthorized)
        );
        Ok(())
    }

    #[test]
    fn two_different_roots_read_only_their_own_evidence() -> Check {
        let fixture = Fixture::new("two-roots")?;
        let mut store = fixture.open()?;
        let left = store.owner_binding(&facts(11))?;
        let right = store.owner_binding(&facts(22))?;
        let left_job = claim(11, left, 1024 * 1024, 4)?;
        let right_job = claim(12, right, 1024 * 1024, 4)?;
        store.reserve(&left_job)?;
        store.reserve(&right_job)?;
        let mine = publish(&mut store, &left_job, draft(11, 0, 600)?, 4096, b"left")?;
        let theirs = publish(&mut store, &right_job, draft(12, 0, 600)?, 4096, b"right")?;

        assert_eq!(
            store.read_chunk(left, &mine.artifact_id, 0, 16)?.bytes,
            b"left"
        );
        assert_eq!(
            store.read_chunk(right, &theirs.artifact_id, 0, 16)?.bytes,
            b"right"
        );
        // Same status and no index leak in either direction.
        assert_eq!(
            store.read_chunk(left, &theirs.artifact_id, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        assert_eq!(
            store.read_chunk(right, &mine.artifact_id, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        assert!(
            store
                .read_index_page(left, &right_job.job_id, None)?
                .rows
                .is_empty()
        );
        assert_eq!(
            store
                .read_index_page(left, &left_job.job_id, None)?
                .rows
                .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn a_second_session_reads_the_same_locator_with_a_fresh_reference() -> Check {
        let fixture = Fixture::new("restart")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(13, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(
            &mut store,
            &reservation,
            draft(13, 0, 600)?,
            4096,
            b"retained",
        )?;
        store.release(&reservation)?;
        drop(store);

        // A later session with the same uid, state root and granted root reads
        // the retained locator; a different granted root cannot.
        let mut store = fixture.open()?;
        let same = store.owner_binding(&facts(11))?;
        assert_eq!(same, owner);
        assert_eq!(
            store
                .read_chunk(same, &descriptor.artifact_id, 0, 64)?
                .bytes,
            b"retained"
        );
        let elsewhere = store.owner_binding(&facts(99))?;
        assert_eq!(
            store
                .read_chunk(elsewhere, &descriptor.artifact_id, 0, 64)
                .err(),
            Some(QualityArtifactError::NotFound)
        );
        Ok(())
    }

    #[test]
    fn expiry_reclaims_only_known_bytes_and_reads_never_renew() -> Check {
        let fixture = Fixture::new("ttl")?;
        // The session publishes as of an hour ago, so the operator prune at the
        // end judges these same TTLs with the real host clock — the short one
        // expired fifty minutes before it, the long one due twenty-three hours
        // after it. Nothing here turns on how long the volume takes to fsync.
        let base = UtcInstant::from_unix_seconds(wall_now()?.unix_seconds().saturating_sub(3_600))?;
        let clock = TestClock::at(&base);
        let mut store = fixture.open()?.with_clock_source(clock.source())?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim_at(&base, 14, owner, 1024 * 1024, QUALITY_MAX_TTL_SECONDS)?;
        store.reserve(&reservation)?;
        let short = publish(
            &mut store,
            &reservation,
            draft_at(&base, 14, 0, 600)?,
            4096,
            b"transient",
        )?;
        let long = publish(
            &mut store,
            &reservation,
            draft_at(&base, 15, 1, QUALITY_MAX_TTL_SECONDS)?,
            4096,
            b"kept",
        )?;
        let path = fixture
            .store_dir("descriptor")
            .join(format!("{}.json", long.artifact_id));
        let before = fs::read(&path)?;
        let stamp = fs::metadata(&path)?.modified()?;

        // A pre-expiry read renews neither the TTL nor any lease.
        let chunk = store.read_chunk(owner, &long.artifact_id, 0, 16)?;
        assert_eq!(chunk.descriptor.expires_at_utc, long.expires_at_utc);
        assert_eq!(fs::read(&path)?, before);
        assert_eq!(fs::metadata(&path)?.modified()?, stamp);

        // Half an hour on: the short TTL has passed and the long one has not.
        clock.set(&base.checked_add_seconds(1_800)?);
        assert_eq!(
            store.read_chunk(owner, &short.artifact_id, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        assert!(store.read_chunk(owner, &long.artifact_id, 0, 16).is_ok());
        assert_eq!(
            store
                .read_index_page(owner, &reservation.job_id, None)?
                .rows
                .len(),
            1
        );
        // Neither read moved the surviving descriptor's TTL by a single byte.
        assert_eq!(fs::read(&path)?, before);
        assert_eq!(fs::metadata(&path)?.modified()?, stamp);

        let report = prune_expired(&fixture.state)?;
        assert_eq!(report.removed, 1);
        assert_eq!(report.reclaimed_bytes, b"transient".len() as u64);
        assert_eq!(report.retained, 1);
        assert_eq!(fs::read(&path)?, before);
        assert!(
            !fixture
                .store_dir("blob")
                .join(format!("{}.blob", short.artifact_id))
                .exists()
        );
        Ok(())
    }

    #[test]
    fn owner_and_global_quotas_reject_before_the_gateway_and_evict_nothing() -> Check {
        let fixture = Fixture::new("quota")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let other = store.owner_binding(&facts(22))?;
        let mut seed = 20_u8;
        let mut reserve = |store: &mut NativeQualityArtifactStore, binding: [u8; 32]| {
            seed += 1;
            claim(seed, binding, QUALITY_MAX_JOB_BYTES, 4)
                .map_err(|error| error.to_string())
                .and_then(|reservation| {
                    store
                        .reserve(&reservation)
                        .map(|()| reservation)
                        .map_err(|error| error.to_string())
                })
        };
        // 128 MiB per owner is exactly two maximal jobs.
        let first = reserve(&mut store, owner)?;
        let _second = reserve(&mut store, owner)?;
        assert_eq!(
            (
                QUALITY_MAX_OWNER_BYTES / QUALITY_MAX_JOB_BYTES,
                QUALITY_MAX_GLOBAL_BYTES / QUALITY_MAX_JOB_BYTES
            ),
            (2, 4)
        );
        assert!(reserve(&mut store, owner).is_err());
        // The other owner still fits inside the global 256 MiB budget.
        let _third = reserve(&mut store, other)?;
        let _fourth = reserve(&mut store, other)?;
        assert!(reserve(&mut store, other).is_err());
        // Nothing was displaced: the first claim is still exactly as admitted.
        assert_eq!(fixture.entries("reservation")?.len(), 4);
        assert!(fixture.entries("blob")?.is_empty());
        // Releasing one claim makes exactly one slot available again.
        store.release(&first)?;
        assert!(reserve(&mut store, owner).is_ok());
        Ok(())
    }

    #[test]
    fn a_job_index_page_is_bounded_to_sixty_four_rows_with_a_canonical_cursor() -> Check {
        let fixture = Fixture::new("index")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(30, owner, 1024 * 1024, QUALITY_MAX_JOB_MEMBERS)?;
        store.reserve(&reservation)?;
        let mut page_ids = Vec::new();
        for member in 0..65_u16 {
            let mut draft = draft(31, member, 600)?;
            draft.artifact_id = QualityArtifactId::from_random_bytes([
                member as u8,
                1,
                2,
                3,
                4,
                5,
                6,
                7,
                8,
                9,
                10,
                11,
                12,
                13,
                14,
                15,
            ]);
            page_ids.push(draft.artifact_id.clone());
            publish(&mut store, &reservation, draft, 64, b"m")?;
        }
        let page = store.read_index_page(owner, &reservation.job_id, None)?;
        assert_eq!(page.rows.len(), QUALITY_INDEX_PAGE_MEMBERS);
        assert_eq!(page.rows[0].member_index, 0);
        assert_eq!(page.rows[63].member_index, 63);
        let cursor = page.next_cursor.ok_or("missing cursor")?;
        // The cursor is the whole ordering key: member index and artifact.
        let boundary = &page_ids[64];
        assert_eq!(cursor, format!("m0000000064_{boundary}").into_bytes());
        assert!(cursor.len() <= QUALITY_CURSOR_MAX_BYTES);
        let next = store.read_index_page(owner, &reservation.job_id, Some(&cursor))?;
        assert_eq!(next.rows.len(), 1);
        assert_eq!(next.rows[0].member_index, 64);
        assert_eq!(&next.rows[0].artifact_id, boundary);
        assert!(next.next_cursor.is_none());
        // Only the canonical cursor grammar is accepted.
        for bad in [
            b"m000000064".to_vec(),
            b"m00000000064".to_vec(),
            format!("x0000000064_{boundary}").into_bytes(),
            format!("m000000006x_{boundary}").into_bytes(),
            format!("m0000000064-{boundary}").into_bytes(),
            format!("m0000000064_qart_{}", "z".repeat(32)).into_bytes(),
            b"m0000000064".to_vec(),
            vec![0xff; 49],
        ] {
            assert_eq!(
                store
                    .read_index_page(owner, &reservation.job_id, Some(&bad))
                    .err(),
                Some(QualityArtifactError::NotFound),
                "{bad:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_failed_watermark_advance_publishes_nothing() -> Check {
        let fixture = Fixture::new("watermark")?;
        let mut store = fixture.open()?.with_fault_injection(Fault::boxed(
            QualityFaultPoint::WatermarkAdvance,
            QualityArtifactError::RecoveryRequired,
            0,
        ));
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(45, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        // The watermark advance guards the commit marker, so failing it leaves
        // no blob, no descriptor and a store that never served the bytes.
        assert_eq!(
            publish(
                &mut store,
                &reservation,
                draft(45, 0, 600)?,
                4096,
                b"unpublished"
            )
            .err(),
            Some(QualityArtifactError::RecoveryRequired)
        );
        assert!(fixture.entries("blob")?.is_empty());
        assert!(fixture.entries("descriptor")?.is_empty());
        // Positive control: the same job publishes once the advance succeeds.
        let mut store = fixture.open()?;
        let ingest =
            store.ingest_member(&reservation, 0, 4096, &mut Bytes::of(b"published".to_vec()))?;
        let descriptor = draft(45, 0, 600)?.into_descriptor(
            reservation.job_id.clone(),
            owner,
            ingest.sha256,
            ingest.size_bytes,
        )?;
        store.publish_descriptor(&reservation, &descriptor)?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 64)?
                .bytes,
            b"published"
        );
        Ok(())
    }

    #[test]
    fn a_durable_clock_regression_blocks_only_quality_until_recovery() -> Check {
        let fixture = Fixture::new("clock")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(40, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(
            &mut store,
            &reservation,
            draft(40, 0, 3_600)?,
            4096,
            b"before",
        )?;
        drop(store);

        // A watermark ahead of the wall clock is a regression.
        let watermark = fixture.state.join(STORE).join("clock-watermark.json");
        let future = wall_now()?.checked_add_seconds(86_400)?;
        fs::write(
            &watermark,
            format!("{{\"format_version\":1,\"observed_at_utc\":\"{future}\"}}"),
        )?;
        let mut blocked = fixture.open()?;
        for outcome in [
            blocked
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)
                .err(),
            blocked.reserve(&reservation).err(),
            blocked
                .read_index_page(owner, &reservation.job_id, None)
                .err(),
            blocked.prune_expired().err(),
            blocked.reconcile_recover().err(),
        ] {
            assert_eq!(outcome, Some(QualityArtifactError::RecoveryRequired));
        }
        // The M2 sibling and every other host state is untouched by the block.
        let sibling = fixture.state.join("rust-mcp-mutations-v1");
        fs::create_dir_all(&sibling)?;
        fs::write(sibling.join("journal-keep.json"), b"m2-bytes")?;
        drop(blocked);

        // Pruning never re-bases the clock: it fails closed until recovery.
        assert_eq!(
            prune_expired(&fixture.state).err(),
            Some(QualityArtifactError::RecoveryRequired)
        );
        let report = recover(&fixture.state)?;
        assert!(report.clock_regression);
        assert_eq!(report.validated, 1);
        assert_eq!(fs::read(sibling.join("journal-keep.json"))?, b"m2-bytes");
        // After the operator action the valid pair reads again.
        let mut store = fixture.open()?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)?
                .bytes,
            b"before"
        );
        Ok(())
    }

    #[test]
    fn readmission_of_a_job_is_idempotent_and_conflicts_fail_closed() -> Check {
        let fixture = Fixture::new("readmit")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(50, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        store.reserve(&reservation)?;
        assert_eq!(fixture.entries("reservation")?.len(), 1);
        let conflicting = QualityReservation {
            reserved_bytes: 2048,
            ..reservation.clone()
        };
        assert_eq!(
            store.reserve(&conflicting).err(),
            Some(QualityArtifactError::RecoveryRequired)
        );
        // An unauthorized job locator cannot stream into another job's claim.
        let foreign = QualityReservation {
            owner_binding: [1; 32],
            ..reservation.clone()
        };
        assert_eq!(
            store
                .ingest_member(&foreign, 0, 64, &mut Bytes::of(b"x".to_vec()))
                .err(),
            Some(QualityArtifactError::Unauthorized)
        );
        // Invalid limits are rejected before any effect.
        for (bytes, members) in [
            (0, 4),
            (QUALITY_MAX_JOB_BYTES + 1, 4),
            (1024, 0),
            (1024, QUALITY_MAX_JOB_MEMBERS + 1),
        ] {
            let invalid = QualityReservation {
                job_id: job(51),
                reserved_bytes: bytes,
                declared_members: members,
                ..reservation.clone()
            };
            assert_eq!(
                store.reserve(&invalid).err(),
                Some(QualityArtifactError::InvalidLimit)
            );
        }
        assert_eq!(fixture.entries("reservation")?.len(), 1);
        Ok(())
    }

    #[test]
    fn a_job_cannot_exceed_its_declared_bytes_or_members() -> Check {
        let fixture = Fixture::new("job-bound")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(60, owner, 4_096, 2)?;
        store.reserve(&reservation)?;
        publish(
            &mut store,
            &reservation,
            draft(60, 0, 600)?,
            2_048,
            &vec![1; 2_048],
        )?;
        publish(
            &mut store,
            &reservation,
            draft(61, 1, 600)?,
            2_048,
            &vec![2; 2_048],
        )?;
        // The declared member count is exhausted.
        assert_eq!(
            store
                .ingest_member(&reservation, 1, 64, &mut Bytes::of(b"x".to_vec()))
                .err(),
            Some(QualityArtifactError::QuotaExceeded)
        );
        let wider = QualityReservation {
            declared_members: 4,
            ..reservation.clone()
        };
        // A wider claim is a different record and is not honoured.
        assert_eq!(
            store
                .ingest_member(&wider, 2, 64, &mut Bytes::of(b"x".to_vec()))
                .err(),
            Some(QualityArtifactError::Unauthorized)
        );
        assert_eq!(fixture.entries("descriptor")?.len(), 2);
        Ok(())
    }

    #[test]
    fn an_operator_state_root_is_qualified_exactly_as_m2_qualifies_it() -> Check {
        // 0755 is a root M2 accepts, so the quality store must open on it too:
        // an operator `--state-root` is not required to be private itself.
        let permissive = Fixture::with_state_root_mode("root-0755", 0o755)?;
        let mut store = permissive.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(100, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(
            &mut store,
            &reservation,
            draft(100, 0, 600)?,
            4096,
            b"qualified",
        )?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 64)?
                .bytes,
            b"qualified"
        );
        // The directories this store creates are still exactly 0700.
        for child in ["blob", "descriptor", "reservation", "quarantine"] {
            assert_eq!(
                fs::metadata(permissive.store_dir(child))?
                    .permissions()
                    .mode()
                    & 0o7777,
                0o700,
                "{child}"
            );
        }

        // A root anyone may write to is not a state root, and the failure is
        // its own answer: the integrator can tell it from an I/O fault.
        for mode in [0o777, 0o770, 0o707] {
            let open = Fixture::with_state_root_mode("root-open", mode)?;
            assert_eq!(
                open.open().err(),
                Some(QualityArtifactError::UnsupportedStateRoot),
                "{mode:o}"
            );
            assert_eq!(
                recover(&open.state).err(),
                Some(QualityArtifactError::UnsupportedStateRoot),
                "{mode:o}"
            );
            assert_eq!(
                prune_expired(&open.state).err(),
                Some(QualityArtifactError::UnsupportedStateRoot),
                "{mode:o}"
            );
            // Rejection happens before any effect: nothing was created.
            assert!(!open.state.join(STORE).exists(), "{mode:o}");
        }
        Ok(())
    }

    #[test]
    fn expired_evidence_and_claims_stop_being_charged_and_leave_the_volume() -> Check {
        let fixture = Fixture::new("reclaim")?;
        let base = wall_now()?;
        let clock = TestClock::at(&base);
        let mut store = fixture.open()?.with_clock_source(clock.source())?;
        let owner = store.owner_binding(&facts(11))?;
        // One job publishes a member whose TTL matches its claim's.
        let short = claim_at(&base, 101, owner, 8 * 1024 * 1024, 300)?;
        store.reserve(&short)?;
        let member = publish(
            &mut store,
            &short,
            draft_at(&base, 101, 0, 300)?,
            8 * 1024 * 1024,
            &vec![9_u8; 4096],
        )?;
        // A second job is abandoned mid-stream, holding its whole `.part` cap.
        let abandoned = claim_at(&base, 102, owner, 8 * 1024 * 1024, 300)?;
        store.reserve(&abandoned)?;
        store.ingest_member(
            &abandoned,
            0,
            8 * 1024 * 1024,
            &mut Bytes::of(vec![1_u8; 4096]),
        )?;
        assert!(
            store_bytes(&fixture)? >= 8 * 1024 * 1024,
            "the abandoned temporary never occupied its cap"
        );

        // Both claims and the published member are now past their TTL.
        clock.set(&base.checked_add_seconds(600)?);
        // Admission reclaims first, so the expired pair and the abandoned
        // temporary are gone before the new claim is judged.
        let first = claim_at(&base, 103, owner, QUALITY_MAX_JOB_BYTES, 1_200)?;
        store.reserve(&first)?;
        assert!(fixture.entries("blob")?.is_empty());
        assert!(fixture.entries("descriptor")?.is_empty());
        assert_eq!(
            fixture.entries("reservation")?,
            [format!("{}.reserve", job(103))]
        );
        assert!(store_bytes(&fixture)? < 64 * 1024);
        assert_eq!(
            store.read_chunk(owner, &member.artifact_id, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        // The owner's whole 128 MiB is available again: the 4 KiB expired
        // descriptor alone would have denied this second maximal claim.
        let second = claim_at(&base, 104, owner, QUALITY_MAX_JOB_BYTES, 1_200)?;
        store.reserve(&second)?;
        assert_eq!(
            store.reserve(&claim_at(&base, 105, owner, QUALITY_MAX_JOB_BYTES, 1_200)?),
            Err(QualityArtifactError::QuotaExceeded)
        );
        // Within one session the declared caps still bound the volume.
        assert!(store_bytes(&fixture)? <= QUALITY_MAX_GLOBAL_BYTES);
        // Nothing live was displaced by the reclamation.
        assert_eq!(fixture.entries("reservation")?.len(), 2);
        Ok(())
    }

    #[test]
    fn two_members_of_one_job_cannot_share_a_member_index() -> Check {
        let fixture = Fixture::new("member-index")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(110, owner, 1024 * 1024, 8)?;
        store.reserve(&reservation)?;
        publish(
            &mut store,
            &reservation,
            draft(110, 0, 600)?,
            4096,
            b"first",
        )?;
        // The store is the closed authority on a job's index: a second member
        // at the same index is rejected before anything is committed.
        assert_eq!(
            publish(
                &mut store,
                &reservation,
                draft(111, 0, 600)?,
                4096,
                b"second"
            )
            .err(),
            Some(QualityArtifactError::InvalidDescriptor)
        );
        assert_eq!(fixture.entries("descriptor")?.len(), 1);
        assert_eq!(fixture.entries("blob")?.len(), 1);
        // The next index publishes, and the same index is free in another job.
        publish(
            &mut store,
            &reservation,
            draft(111, 1, 600)?,
            4096,
            b"second",
        )?;
        let other = claim(112, owner, 1024 * 1024, 8)?;
        store.reserve(&other)?;
        publish(&mut store, &other, draft(113, 0, 600)?, 4096, b"elsewhere")?;
        assert_eq!(fixture.entries("descriptor")?.len(), 3);
        Ok(())
    }

    #[test]
    fn a_page_boundary_advances_past_stored_objects_sharing_an_index() -> Check {
        let fixture = Fixture::new("page-boundary")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(120, owner, 1024 * 1024, QUALITY_MAX_JOB_MEMBERS)?;
        store.reserve(&reservation)?;
        let mut last = None;
        for member in 0..64_u16 {
            let mut candidate = draft(121, member, 600)?;
            candidate.artifact_id = QualityArtifactId::from_random_bytes([
                1,
                member as u8,
                2,
                3,
                4,
                5,
                6,
                7,
                8,
                9,
                10,
                11,
                12,
                13,
                14,
                15,
            ]);
            last = Some(publish(&mut store, &reservation, candidate, 64, b"m")?);
        }
        // A damaged or foreign store may still hold two objects at one index;
        // the reader must page past them instead of repeating the boundary.
        let last = last.ok_or("no member")?;
        let planted = QualityArtifactId::from_random_bytes([0xff; 16]);
        let mut duplicate = last.clone();
        duplicate.artifact_id = planted.clone();
        for (directory, name, bytes) in [
            (
                "descriptor",
                format!("{planted}.json"),
                serde_json::to_vec(&duplicate)?,
            ),
            ("blob", format!("{planted}.blob"), b"m".to_vec()),
        ] {
            let path = fixture.store_dir(directory).join(name);
            fs::write(&path, bytes)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }

        let page = store.read_index_page(owner, &reservation.job_id, None)?;
        assert_eq!(page.rows.len(), QUALITY_INDEX_PAGE_MEMBERS);
        assert_eq!(page.rows[63].artifact_id, last.artifact_id);
        let cursor = page.next_cursor.ok_or("missing cursor")?;
        assert_eq!(cursor, format!("m0000000063_{planted}").into_bytes());
        let next = store.read_index_page(owner, &reservation.job_id, Some(&cursor))?;
        assert_eq!(next.rows.len(), 1);
        assert_eq!(next.rows[0].artifact_id, planted);
        assert!(next.next_cursor.is_none());
        // No row is served twice and no page repeats itself.
        assert!(
            !page
                .rows
                .iter()
                .any(|row| row.artifact_id == next.rows[0].artifact_id)
        );
        Ok(())
    }

    #[test]
    fn a_blob_longer_than_its_descriptor_is_quarantined_without_a_marker() -> Check {
        use std::io::Write;
        let fixture = Fixture::new("surplus")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(130, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(
            &mut store,
            &reservation,
            draft(130, 0, 600)?,
            4096,
            b"exact",
        )?;
        // Publication paid its own truncation debt, so no marker survives it.
        assert_eq!(
            fixture.entries("reservation")?,
            [format!("{}.reserve", job(130))]
        );
        drop(store);

        // A same-uid append whose prefix still hashes is indistinguishable from
        // this store's own surplus without a marker, so it is not repaired.
        let blob = fixture
            .store_dir("blob")
            .join(format!("{}.blob", descriptor.artifact_id));
        fs::OpenOptions::new()
            .append(true)
            .open(&blob)?
            .write_all(b"appended")?;
        let mut store = fixture.open()?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)
                .err(),
            Some(QualityArtifactError::NotFound)
        );
        assert!(fixture.entries("blob")?.is_empty());
        assert!(fixture.entries("descriptor")?.is_empty());
        let reasons = quarantine_reasons(&fixture)?;
        assert!(
            reasons.iter().any(|note| note.contains("size_mismatch")),
            "{reasons:?}"
        );
        Ok(())
    }

    #[test]
    fn release_only_honours_the_exact_claim_it_was_given() -> Check {
        let fixture = Fixture::new("release")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(140, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        store.ingest_member(&reservation, 0, 64 * 1024, &mut Bytes::of(vec![4_u8; 8192]))?;
        // Knowing a job locator is not authority over its claim.
        for foreign in [
            QualityReservation {
                owner_binding: [7; 32],
                ..reservation.clone()
            },
            QualityReservation {
                reserved_bytes: 2048,
                ..reservation.clone()
            },
            QualityReservation {
                declared_members: 8,
                ..reservation.clone()
            },
        ] {
            assert_eq!(
                store.release(&foreign).err(),
                Some(QualityArtifactError::Unauthorized)
            );
        }
        // Neither the record nor the in-flight temporary was dropped.
        assert_eq!(
            fixture.entries("reservation")?,
            [
                format!("{}.part", job(140)),
                format!("{}.reserve", job(140))
            ]
        );
        store.release(&reservation)?;
        assert!(fixture.entries("reservation")?.is_empty());
        // Releasing an already-released claim is a no-op, not an error.
        store.release(&reservation)?;
        Ok(())
    }

    #[test]
    fn a_reader_attaches_while_another_session_holds_the_store_lock() -> Check {
        use rustix::fs::{CWD, FlockOperation, Mode, OFlags, flock, openat};
        let fixture = Fixture::new("attach")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(150, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(
            &mut store,
            &reservation,
            draft(150, 0, 600)?,
            4096,
            b"readable",
        )?;
        drop(store);

        let held = openat(
            CWD,
            fixture.state.join(STORE).join("store.lock"),
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        flock(&held, FlockOperation::NonBlockingLockExclusive)?;
        // Building a read-only view never waits and never reports Busy, so
        // ordinary contention cannot make evidence unreadable.
        let mut reader = NativeQualityArtifactStore::attach(&fixture.state)?;
        assert_eq!(
            reader
                .read_chunk(owner, &descriptor.artifact_id, 0, 64)?
                .bytes,
            b"readable"
        );
        assert_eq!(
            reader
                .read_index_page(owner, &reservation.job_id, None)?
                .rows
                .len(),
            1
        );
        // Publication and reconciliation still take the lock.
        assert_eq!(
            reader.reserve(&claim(151, owner, 4096, 1)?).err(),
            Some(QualityArtifactError::Busy)
        );
        assert_eq!(
            reader.reconcile_recover().err(),
            Some(QualityArtifactError::Busy)
        );
        assert_eq!(
            NativeQualityArtifactStore::open(&fixture.state).err(),
            Some(QualityArtifactError::Busy)
        );
        drop(held);
        // The same view publishes once the contender unlocks.
        store = fixture.open()?;
        store.reserve(&claim(151, owner, 4096, 1)?)?;
        Ok(())
    }

    #[test]
    fn a_planted_symlink_or_non_regular_object_is_quarantined_not_followed() -> Check {
        let fixture = Fixture::new("nonregular")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(160, owner, 1024 * 1024, 8)?;
        store.reserve(&reservation)?;
        let linked = publish(&mut store, &reservation, draft(160, 0, 600)?, 4096, b"one")?;
        let opaque = publish(&mut store, &reservation, draft(161, 1, 600)?, 4096, b"two")?;
        let marker = publish(
            &mut store,
            &reservation,
            draft(162, 2, 600)?,
            4096,
            b"three",
        )?;
        drop(store);

        // Bytes the store must never reach by following a planted name.
        let outside = fixture.base.join("outside.bin");
        fs::write(&outside, b"outside")?;
        let blob = |id: &QualityArtifactId| fixture.store_dir("blob").join(format!("{id}.blob"));
        fs::remove_file(blob(&linked.artifact_id))?;
        std::os::unix::fs::symlink(&outside, blob(&linked.artifact_id))?;
        fs::remove_file(blob(&opaque.artifact_id))?;
        fs::create_dir(blob(&opaque.artifact_id))?;
        let descriptor = fixture
            .store_dir("descriptor")
            .join(format!("{}.json", marker.artifact_id));
        fs::remove_file(&descriptor)?;
        std::os::unix::fs::symlink(&outside, &descriptor)?;

        let mut store = fixture.open()?;
        for artifact in [
            &linked.artifact_id,
            &opaque.artifact_id,
            &marker.artifact_id,
        ] {
            assert_eq!(
                store.read_chunk(owner, artifact, 0, 16).err(),
                Some(QualityArtifactError::NotFound)
            );
        }
        let reasons = quarantine_reasons(&fixture)?;
        assert!(reasons.len() >= 3, "{reasons:?}");
        assert!(
            reasons
                .iter()
                .all(|note| note.contains("not_private_regular_file")),
            "{reasons:?}"
        );
        // The planted names left the served directories and the outside file
        // was never opened, written or removed.
        assert!(fixture.entries("blob")?.is_empty());
        assert!(fixture.entries("descriptor")?.is_empty());
        assert_eq!(fs::read(&outside)?, b"outside");
        Ok(())
    }

    // ---------------------------------------------------------------------
    // M3-06 upgrade/rollback oracles for the versioned on-disk format.
    // ---------------------------------------------------------------------

    /// Every byte this store moved aside instead of removing.
    fn quarantined_bytes(fixture: &Fixture) -> Result<Vec<Vec<u8>>, Box<dyn std::error::Error>> {
        let mut retained = Vec::new();
        for name in fixture.entries("quarantine")? {
            if name.ends_with(".bin") {
                retained.push(fs::read(fixture.store_dir("quarantine").join(name))?);
            }
        }
        Ok(retained)
    }

    /// Rewrites one record's version to the next one, leaving every other byte
    /// alone: exactly what a newer binary's record looks like to this one.
    fn bump_record_version(path: &Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let current = fs::read_to_string(path)?;
        assert!(current.contains("\"format_version\":1"), "{current}");
        let newer = current.replace("\"format_version\":1", "\"format_version\":2");
        fs::write(path, &newer)?;
        Ok(newer.into_bytes())
    }

    fn m2_sibling(fixture: &Fixture) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let sibling = fixture.state.join("rust-mcp-mutations-v1");
        fs::create_dir_all(sibling.join("journal"))?;
        fs::write(sibling.join("journal/000001.json"), b"m2-journal-bytes")?;
        fs::write(sibling.join("state.json"), b"m2-state-bytes")?;
        Ok(sibling)
    }

    fn assert_m2_untouched(sibling: &Path) -> Check {
        assert_eq!(
            fs::read(sibling.join("journal/000001.json"))?,
            b"m2-journal-bytes"
        );
        assert_eq!(fs::read(sibling.join("state.json"))?, b"m2-state-bytes");
        assert_eq!(fs::read_dir(sibling.join("journal"))?.count(), 1);
        Ok(())
    }

    /// A record this binary cannot claim to understand is refused, never
    /// reinterpreted as v1 and never unlinked: the newer bytes stay on the
    /// volume so a roll-forward still has them.
    #[test]
    fn an_unknown_record_version_fails_closed_and_is_never_reinterpreted() -> Check {
        let fixture = Fixture::new("record-version")?;
        let sibling = m2_sibling(&fixture)?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let published = claim(170, owner, 1024 * 1024, 8)?;
        store.reserve(&published)?;
        let newer = publish(&mut store, &published, draft(170, 0, 600)?, 4096, b"newer")?;
        let current = publish(
            &mut store,
            &published,
            draft(171, 1, 600)?,
            4096,
            b"current",
        )?;
        // A second live claim, so both record kinds are covered by one pass.
        store.reserve(&claim(172, owner, 1024 * 1024, 4)?)?;
        drop(store);

        let descriptor_v2 = bump_record_version(
            &fixture
                .store_dir("descriptor")
                .join(format!("{}.json", newer.artifact_id)),
        )?;
        let reservation_v2 = bump_record_version(
            &fixture
                .store_dir("reservation")
                .join(format!("{}.reserve", job(172))),
        )?;

        let report = recover(&fixture.state)?;
        assert!(!report.clock_regression);
        assert_eq!(report.validated, 1, "only the v1 pair is trusted");
        assert_eq!(report.quarantined, 2);
        assert_eq!(report.discarded_uncommitted, 0);
        let reasons = quarantine_reasons(&fixture)?;
        assert_eq!(reasons.len(), 3, "{reasons:?}");
        assert!(
            reasons
                .iter()
                .all(|note| note.contains("malformed_descriptor")),
            "{reasons:?}"
        );

        // Neither record was rewritten back to v1 and neither was deleted.
        let retained = quarantined_bytes(&fixture)?;
        assert!(retained.contains(&descriptor_v2), "descriptor bytes lost");
        assert!(retained.contains(&reservation_v2), "claim bytes lost");
        assert!(retained.contains(&b"newer".to_vec()), "blob bytes lost");

        let mut store = fixture.open()?;
        assert_eq!(
            store.read_chunk(owner, &newer.artifact_id, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        assert_eq!(
            store.read_chunk(owner, &current.artifact_id, 0, 16)?.bytes,
            b"current"
        );
        assert_m2_untouched(&sibling)
    }

    /// The durable clock watermark is the one record every path reads first, so
    /// an unknown version there must block quality entirely rather than let any
    /// expiry judgement proceed on a record this binary cannot read.
    #[test]
    fn an_unknown_watermark_version_blocks_quality_and_rebases_nothing() -> Check {
        let fixture = Fixture::new("watermark-version")?;
        let sibling = m2_sibling(&fixture)?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(174, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(&mut store, &reservation, draft(174, 0, 600)?, 4096, b"kept")?;
        drop(store);

        let watermark = fixture.state.join(STORE).join("clock-watermark.json");
        let newer = bump_record_version(&watermark)?;
        let before = (
            fixture.entries("blob")?,
            fixture.entries("descriptor")?,
            fixture.entries("reservation")?,
        );

        for outcome in [
            fixture.open().err(),
            recover(&fixture.state).err(),
            prune_expired(&fixture.state).err(),
        ] {
            assert_eq!(outcome, Some(QualityArtifactError::InvalidDescriptor));
        }
        // Nothing was re-based, migrated or removed by the refusal, so rolling
        // the newer binary forward again finds exactly its own store.
        assert_eq!(fs::read(&watermark)?, newer);
        assert_eq!(
            (
                fixture.entries("blob")?,
                fixture.entries("descriptor")?,
                fixture.entries("reservation")?,
            ),
            before
        );
        assert!(fixture.entries("quarantine")?.is_empty());
        assert_m2_untouched(&sibling)?;

        // The operator remedy is to restore this binary's own record; it is not
        // a repair of the newer one.
        fs::write(
            &watermark,
            String::from_utf8(newer)?.replace("\"format_version\":2", "\"format_version\":1"),
        )?;
        let mut store = fixture.open()?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)?
                .bytes,
            b"kept"
        );
        Ok(())
    }

    /// The format version is in the directory name, so a future format is a
    /// sibling of this one. A v1 binary must leave it — and M2 — exactly as it
    /// found them across a whole lifecycle including both operator commands.
    #[test]
    fn a_future_sibling_store_and_the_m2_journal_are_never_read_migrated_or_removed() -> Check {
        let fixture = Fixture::new("sibling")?;
        let sibling = m2_sibling(&fixture)?;
        let future = fixture.state.join("rust-mcp-quality-artifacts-v2");
        fs::create_dir_all(future.join("descriptor"))?;
        fs::write(
            future.join("descriptor/one.json"),
            b"{\"format_version\":2}",
        )?;

        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(176, owner, 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(&mut store, &reservation, draft(176, 0, 600)?, 4096, b"v1")?;
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 0, 16)?
                .bytes,
            b"v1"
        );
        store.read_index_page(owner, &reservation.job_id, None)?;
        store.reconcile_recover()?;
        drop(store);
        recover(&fixture.state)?;
        prune_expired(&fixture.state)?;

        assert_eq!(
            fs::read(future.join("descriptor/one.json"))?,
            b"{\"format_version\":2}"
        );
        assert_eq!(fs::read_dir(future.join("descriptor"))?.count(), 1);
        assert_m2_untouched(&sibling)?;
        let mut roots = Vec::new();
        for entry in fs::read_dir(&fixture.state)? {
            roots.push(entry?.file_name().to_string_lossy().into_owned());
        }
        roots.sort();
        assert_eq!(
            roots,
            [
                "rust-mcp-mutations-v1",
                "rust-mcp-quality-artifacts-v1",
                "rust-mcp-quality-artifacts-v2",
            ]
        );
        Ok(())
    }

    /// `recover` and `prune` on one store holding a valid pair, an expired pair
    /// with its expired claim, and an object no version of this store wrote.
    /// Pruning is not an eviction and never quarantines; recovery quarantines
    /// and never removes live evidence.
    #[test]
    fn operator_recover_and_prune_separate_valid_expired_and_unknown_objects() -> Check {
        let fixture = Fixture::new("operator")?;
        let sibling = m2_sibling(&fixture)?;
        // Published an hour ago on this store's own clock, so the operator
        // commands — which use the host wall clock — judge real expiry.
        let past = UtcInstant::from_unix_seconds(wall_now()?.unix_seconds().saturating_sub(3_600))?;
        let clock = TestClock::at(&past);
        let mut store = fixture.open()?.with_clock_source(clock.source())?;
        let owner = store.owner_binding(&facts(11))?;
        let live = claim_at(&past, 178, owner, 8 * 1024 * 1024, QUALITY_MAX_TTL_SECONDS)?;
        store.reserve(&live)?;
        let valid = publish(
            &mut store,
            &live,
            draft_at(&past, 178, 0, QUALITY_MAX_TTL_SECONDS)?,
            4096,
            b"valid",
        )?;
        let stale = claim_at(&past, 179, owner, 8 * 1024 * 1024, 600)?;
        store.reserve(&stale)?;
        let expired = publish(
            &mut store,
            &stale,
            draft_at(&past, 179, 0, 600)?,
            4096,
            b"expired",
        )?;
        drop(store);
        // An object this store never wrote, in a directory it owns.
        fs::write(fixture.store_dir("descriptor").join("FOREIGN"), b"foreign")?;
        let occupied = store_bytes(&fixture)?;

        // Prune reclaims only what expired and reports what it retained.
        let pruned = prune_expired(&fixture.state)?;
        assert_eq!(pruned.removed, 2, "the expired pair and its expired claim");
        assert!(pruned.reclaimed_bytes > 0);
        assert!(pruned.retained >= 1);
        assert!(store_bytes(&fixture)? < occupied);
        assert!(
            fixture.entries("quarantine")?.is_empty(),
            "prune quarantined"
        );
        assert!(
            fixture
                .entries("descriptor")?
                .contains(&"FOREIGN".to_owned()),
            "prune removed an object it does not understand"
        );

        // Recovery is the pass that judges what it cannot read.
        let recovered = recover(&fixture.state)?;
        assert!(!recovered.clock_regression);
        assert_eq!(recovered.validated, 1);
        assert_eq!(recovered.quarantined, 1);
        assert_eq!(recovered.released_reservations, 0);
        let reasons = quarantine_reasons(&fixture)?;
        assert_eq!(reasons.len(), 1, "{reasons:?}");
        assert!(reasons[0].contains("unknown_name"), "{reasons:?}");
        assert!(quarantined_bytes(&fixture)?.contains(&b"foreign".to_vec()));

        let mut store = fixture.open()?;
        assert_eq!(
            store.read_chunk(owner, &valid.artifact_id, 0, 16)?.bytes,
            b"valid"
        );
        assert_eq!(
            store.read_chunk(owner, &expired.artifact_id, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        assert_m2_untouched(&sibling)
    }

    // ---------------------------------------------------------------------
    // Native-only qualification. These need a second process, a real volume or
    // real capacity coupling, so the gate runs them explicitly.
    // ---------------------------------------------------------------------

    const LOCK_STATE: &str = "RUST_MCP_TEST_QUALITY_LOCK_STATE";
    const LOCK_READY: &str = "RUST_MCP_TEST_QUALITY_LOCK_READY";
    const LOCK_RELEASE: &str = "RUST_MCP_TEST_QUALITY_LOCK_RELEASE";

    /// Helper process: holds `store.lock` until released. A no-op without env.
    #[test]
    fn external_quality_lock_helper() -> Check {
        use rustix::fs::{CWD, FlockOperation, Mode, OFlags, flock, openat};
        let Some(state) = std::env::var_os(LOCK_STATE) else {
            return Ok(());
        };
        let ready = PathBuf::from(std::env::var_os(LOCK_READY).ok_or("ready")?);
        let release = PathBuf::from(std::env::var_os(LOCK_RELEASE).ok_or("release")?);
        let lock = openat(
            CWD,
            PathBuf::from(state).join(STORE).join("store.lock"),
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive)?;
        fs::write(&ready, b"ready")?;
        for _ in 0..500 {
            if release.exists() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Err("quality lock helper release timeout".into())
    }

    #[test]
    #[ignore = "requires native APFS state root"]
    fn native_apfs_quality_two_process_store_lock_is_nonblocking() -> Check {
        use std::process::Command;
        let fixture = Fixture::new("process-lock")?;
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(70, owner, 1024 * 1024, 4)?;
        let ready = fixture.base.join("helper-ready");
        let release = fixture.base.join("helper-release");
        let mut child = Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("apfs::external_quality_lock_helper")
            .arg("--nocapture")
            .env(LOCK_STATE, &fixture.state)
            .env(LOCK_READY, &ready)
            .env(LOCK_RELEASE, &release)
            .spawn()?;
        let mut observed = false;
        for _ in 0..500 {
            if ready.exists() {
                observed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        // Contention is a bounded rejection, never a wait or a second view of
        // the global quota.
        let contended = if observed {
            store.reserve(&reservation).err()
        } else {
            None
        };
        fs::write(&release, b"release")?;
        let status = child.wait()?;
        assert!(observed, "external helper did not acquire the lock");
        assert!(status.success(), "external helper failed: {status}");
        assert_eq!(contended, Some(QualityArtifactError::Busy));
        assert!(fixture.entries("reservation")?.is_empty());
        // Positive control: admission succeeds once the contender unlocks.
        store.reserve(&reservation)?;
        assert_eq!(fixture.entries("reservation")?.len(), 1);
        Ok(())
    }

    const RESERVE_STATE: &str = "RUST_MCP_TEST_QUALITY_RESERVE_STATE";

    /// Helper process: spends one owner's whole budget. A no-op without env.
    #[test]
    fn external_quality_reserve_helper() -> Check {
        let Some(state) = std::env::var_os(RESERVE_STATE) else {
            return Ok(());
        };
        let mut store = NativeQualityArtifactStore::open(Path::new(&state))?;
        let owner = store.owner_binding(&facts(11))?;
        for seed in [170_u8, 171] {
            store.reserve(&claim(seed, owner, QUALITY_MAX_JOB_BYTES, 4)?)?;
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires native APFS state root and a second process"]
    fn native_apfs_quality_two_processes_share_one_global_quota_view() -> Check {
        use rust_engineering_domain::{M2_RECOVERY_HEADROOM_BYTES, QUALITY_CONTROL_HEADROOM_BYTES};
        use std::process::Command;
        let fixture = Fixture::new("shared-quota")?;
        let free = free_bytes(&fixture.state)?;
        let floor =
            QUALITY_MAX_JOB_BYTES + M2_RECOVERY_HEADROOM_BYTES + QUALITY_CONTROL_HEADROOM_BYTES;
        assert!(
            free >= floor,
            "volume has {free} bytes free; the oracle needs {floor}"
        );

        // Process A spends the owner's whole 128 MiB and exits.
        let status = Command::new(std::env::current_exe()?)
            .arg("--exact")
            .arg("apfs::external_quality_reserve_helper")
            .arg("--nocapture")
            .env(RESERVE_STATE, &fixture.state)
            .spawn()?
            .wait()?;
        assert!(status.success(), "reserving helper failed: {status}");
        assert_eq!(fixture.entries("reservation")?.len(), 2);

        // Process B derives the same binding and sees one global view: A's
        // charge is not accounted twice and is not invisible either.
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        assert_eq!(
            store.reserve(&claim(172, owner, QUALITY_MAX_JOB_BYTES, 4)?),
            Err(QualityArtifactError::QuotaExceeded)
        );
        let other = store.owner_binding(&facts(22))?;
        store.reserve(&claim(173, other, QUALITY_MAX_JOB_BYTES, 4)?)?;
        assert_eq!(fixture.entries("reservation")?.len(), 3);
        // A's claims were neither displaced nor evicted to admit B's.
        for seed in [170_u8, 171] {
            assert!(
                fixture
                    .entries("reservation")?
                    .contains(&format!("{}.reserve", job(seed)))
            );
        }
        Ok(())
    }

    #[test]
    #[ignore = "requires native APFS fstatfs capacity oracle"]
    fn native_apfs_quality_reservation_preserves_m2_headroom() -> Check {
        use rust_engineering_application::{
            OperationControl, ProjectBackend, ProjectSourceBackend,
        };
        use rust_engineering_domain::{
            IdempotencyKey, M2_RECOVERY_HEADROOM_BYTES, MutationCandidate, MutationCommit,
            MutationId, MutationKind, QUALITY_CONTROL_HEADROOM_BYTES, SourceBundle, SourceFile,
        };
        use rust_engineering_project::{
            SecureProjects,
            mutation_store::{NativeMutationStore, mutation_digest},
            prepare_mutation_state,
        };

        struct Continue;
        impl OperationControl for Continue {
            fn check(&self) -> Result<(), rust_engineering_application::ProjectError> {
                Ok(())
            }
        }

        let fixture = Fixture::new("m2-headroom")?;
        let free = free_bytes(&fixture.state)?;
        let floor =
            QUALITY_MAX_JOB_BYTES + M2_RECOVERY_HEADROOM_BYTES + QUALITY_CONTROL_HEADROOM_BYTES;
        assert!(
            free >= floor,
            "volume has {free} bytes free; the oracle needs {floor}"
        );

        // A maximal quality reservation is admitted by the real fstatfs floor.
        let mut store = fixture.open()?;
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(80, owner, QUALITY_MAX_JOB_BYTES, 4)?;
        store.reserve(&reservation)?;
        let payload = vec![0xa5_u8; 1024 * 1024];
        publish(
            &mut store,
            &reservation,
            draft(80, 0, 600)?,
            QUALITY_MAX_ARTIFACT_BYTES,
            &payload,
        )?;
        assert!(
            free_bytes(&fixture.state)? >= M2_RECOVERY_HEADROOM_BYTES,
            "the maximal reservation consumed M2's recovery headroom"
        );

        // The coupled oracle: an M2 commit still succeeds afterwards.
        let state = prepare_mutation_state(&fixture.state).map_err(|error| format!("{error:?}"))?;
        let backend = SecureProjects::new(std::slice::from_ref(&fixture.project))
            .map_err(|error| format!("{error:?}"))?;
        let opened = backend
            .open(fixture.project.to_str().ok_or("utf8")?, &Continue)
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
        let candidate = MutationCandidate {
            kind: MutationKind::ManifestPatch,
            after: SourceBundle::with_directories(files, before.directories().to_vec())
                .map_err(|error| format!("{error:?}"))?,
            before,
            validation: "toml_edit=0.25.13;cargo=1.98.1;operation=lints".to_owned(),
        };
        let commit = MutationCommit {
            id: MutationId::new(format!("mut_{:032x}", 81_u128))
                .map_err(|error| format!("{error:?}"))?,
            digest: mutation_digest(&candidate).map_err(|error| format!("{error:?}"))?,
            key: IdempotencyKey::new("quality-headroom".to_owned())
                .map_err(|error| format!("{error:?}"))?,
            candidate,
        };
        let m2 = NativeMutationStore::open(&state, std::slice::from_ref(&fixture.project))
            .map_err(|error| format!("{error:?}"))?;
        m2.commit(&opened.lease, &commit, &Continue)
            .map_err(|error| format!("{error:?}"))?;
        Ok(())
    }

    fn free_bytes(path: &Path) -> Result<u64, Box<dyn std::error::Error>> {
        use rustix::fs::{CWD, Mode, OFlags, fstatfs, openat};
        let fd = openat(
            CWD,
            path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )?;
        let stat = fstatfs(&fd)?;
        Ok(stat.f_bavail * u64::from(stat.f_bsize))
    }

    #[test]
    #[ignore = "requires native APFS crash fault injection"]
    fn native_apfs_quality_crash_between_blob_and_descriptor_is_not_served() -> Check {
        // Same oracle as the in-core test, but against a real state root with
        // real F_FULLFSYNC ordering and a real restart of the store.
        let fixture = Fixture::new("native-crash")?;
        let mut store = fixture.open()?.with_fault_injection(Fault::boxed(
            QualityFaultPoint::AfterBlobRename,
            QualityArtifactError::Io,
            0,
        ));
        let owner = store.owner_binding(&facts(11))?;
        let reservation = claim(90, owner, 8 * 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let payload = vec![0x5a_u8; 3 * 1024 * 1024];
        let identifier = draft(90, 0, 600)?.artifact_id;
        assert_eq!(
            publish(
                &mut store,
                &reservation,
                draft(90, 0, 600)?,
                4 * 1024 * 1024,
                &payload
            )
            .err(),
            Some(QualityArtifactError::Io)
        );
        assert_eq!(fixture.entries("blob")?, [format!("{identifier}.blob")]);
        drop(store);
        let mut store = fixture.open()?;
        assert!(fixture.entries("blob")?.is_empty());
        assert_eq!(
            store.read_chunk(owner, &identifier, 0, 16).err(),
            Some(QualityArtifactError::NotFound)
        );
        // Positive control: the fully synced pair reads its exact digest.
        let reservation = claim(91, owner, 8 * 1024 * 1024, 4)?;
        store.reserve(&reservation)?;
        let descriptor = publish(
            &mut store,
            &reservation,
            draft(91, 0, 600)?,
            4 * 1024 * 1024,
            &payload,
        )?;
        assert_eq!(descriptor.sha256, sha256(&payload));
        assert_eq!(
            store
                .read_chunk(owner, &descriptor.artifact_id, 1024, 4096)?
                .bytes,
            payload[1024..5120]
        );
        Ok(())
    }
}
