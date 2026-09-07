use super::*;
use rust_engineering_application::job::JobPermit;
use rust_engineering_domain::{
    ArtifactCompleteness, ArtifactMetadata, ArtifactPlugin, ArtifactRuntime, ArtifactSelection,
    ArtifactSensitivity, ArtifactSource, GuestArtifactName, PayloadFormatVersion, PluginIdentity,
    QualityArtifactDescriptor, QualityArtifactDraft, QualityArtifactKind, QualityMimeType,
    UtcInstant,
};
use rust_engineering_project::{MonotonicClock, OsReferences, SecureProjects};

struct ContentionMaskedReader {
    registry: Arc<Mutex<Registry>>,
}
impl QualityResourceReader for ContentionMaskedReader {
    fn read_chunk(
        &self,
        _: &ProjectRef,
        _: &QualityArtifactId,
        _: u64,
        _: u32,
    ) -> Result<QualityArtifactChunk, ()> {
        self.registry.try_lock().map(|_| ()).map_err(|_| ())?;
        Err(())
    }
    fn read_index(
        &self,
        _: &ProjectRef,
        _: &QualityJobId,
        _: Option<&str>,
    ) -> Result<QualityArtifactIndexPage, ()> {
        self.registry.try_lock().map(|_| ()).map_err(|_| ())?;
        Err(())
    }
    fn is_live(&self, _: &ProjectRef, _: &QualityArtifactId) -> bool {
        self.registry.try_lock().is_ok()
    }
}

fn artifact(bytes: Vec<u8>) -> Result<AuthorizedArtifact, Box<dyn std::error::Error>> {
    Ok(AuthorizedArtifact {
        metadata: ArtifactMetadata {
            owner: "prj_00000000000000000000000000000001"
                .parse()
                .map_err(|_| "owner")?,
            id: "art_00000000000000000000000000000002".parse()?,
            sha256: [0xab; 32],
            size_bytes: bytes.len().try_into()?,
            truncated: true,
            created_seconds: 7,
            expires_seconds: 99,
        },
        content: bytes,
        retention_remaining_seconds: 12,
    })
}
#[test]
fn canonical_uris_round_trip_and_reject_every_additional_component()
-> Result<(), Box<dyn std::error::Error>> {
    let artifact = artifact(vec![])?;
    let canonical = uri(&artifact.metadata.owner, &artifact.metadata.id);
    let parsed = parse(&canonical)?;
    assert_eq!(parsed, (artifact.metadata.owner, artifact.metadata.id));
    for bad in [
        format!("{canonical}/"),
        format!("{canonical}?a=1"),
        format!("{canonical}#x"),
        canonical.replace("rust-artifact", "RUST-artifact"),
        canonical.replace("prj_", "prj_A"),
        canonical.replace("/art_", "/../art_"),
        canonical.replace("prj_", "%70rj_"),
        canonical.replace("art_", "prj_"),
        String::new(),
        "x".repeat(1024),
    ] {
        assert_eq!(parse(&bad).err(), Some(not_found()), "{bad}");
    }
    Ok(())
}

#[test]
fn quality_uris_have_closed_query_grammar_and_chunk_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let owner: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let job: QualityJobId = "job_00000000000000000000000000000002".parse()?;
    let artifact: QualityArtifactId = "qart_00000000000000000000000000000003".parse()?;
    assert!(matches!(
        parse_quality(&quality_index_uri(&owner, &job))?,
        QualityUri::Index(_, _, None)
    ));
    assert!(matches!(
        parse_quality(&quality_chunk_uri(
            &owner,
            &artifact,
            0,
            QUALITY_RESOURCE_CHUNK_BYTES as u32
        ))?,
        QualityUri::Chunk(_, _, 0, _)
    ));
    for bad in [
        format!(
            "{}?length=1&offset=0",
            quality_chunk_uri(&owner, &artifact, 0, 1)
        ),
        format!("rust-quality-artifact://{owner}/{job}?cursor="),
        format!("rust-quality-artifact://{owner}/{artifact}?offset=00&length=1"),
        format!(
            "rust-quality-artifact://{owner}/{artifact}?offset=0&length={}",
            QUALITY_RESOURCE_CHUNK_BYTES + 1
        ),
    ] {
        assert_eq!(parse_quality(&bad).err(), Some(not_found()));
    }
    Ok(())
}
#[test]
fn binary_content_has_exact_base64_and_private_uncacheable_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    for bytes in [vec![0, 255, 10, 13, 34, 92], vec![255; MAX_CONTENT]] {
        let expected = bytes.clone();
        let response = encode(artifact(bytes)?)?;
        let value = serde_json::to_value(&response)?;
        assert_eq!(value["cacheScope"], "private");
        assert_eq!(value["ttlMs"], 0);
        assert_eq!(value["resultType"], "complete");
        let content = &value["contents"][0];
        assert_eq!(content["mimeType"], "application/octet-stream");
        assert_eq!(
            STANDARD.decode(content["blob"].as_str().ok_or("blob")?)?,
            expected
        );
        assert_eq!(content["_meta"]["sha256"], "ab".repeat(32));
        assert_eq!(content["_meta"]["size_bytes"], expected.len());
        assert_eq!(content["_meta"]["truncated"], true);
        assert_eq!(content["_meta"]["retention_remaining_seconds"], 12);
        assert!(content["_meta"].get("expires_seconds").is_none());
        assert!(serde_json::to_vec(&response)?.len() < MAX_RESPONSE);
    }
    assert!(encode(artifact(vec![0; MAX_CONTENT + 1])?).is_err());
    Ok(())
}
#[tokio::test]
async fn bootstrap_blocks_before_touching_poisoned_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let backend = SecureProjects::new(&[]).map_err(|_| "backend")?;
    let registry = Registry::new(backend, OsReferences, MonotonicClock::default(), 10, 1)
        .map_err(|_| "registry")?;
    let registry = Arc::new(Mutex::new(registry));
    let resources = Resources::new(
        Arc::clone(&registry),
        Workers::new(),
        Arc::new(AtomicBool::new(false)),
    )?;
    // A poisoned authority would produce Internal if the bootstrap gate entered
    // worker execution. The gate must return uniform NotFound without access.
    let poisoned = Arc::clone(&registry);
    let _ = std::thread::spawn(move || {
        let _guard = poisoned.lock();
        std::panic::resume_unwind(Box::new("test poisoning"));
    })
    .join();
    let artifact = artifact(vec![])?;
    let value = uri(&artifact.metadata.owner, &artifact.metadata.id);
    assert_eq!(
        resources
            .read_uri(&value, CancellationToken::new())
            .await
            .err(),
        Some(not_found())
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn quality_read_and_tasks_get_stay_prompt_while_registry_is_contended()
-> Result<(), Box<dyn std::error::Error>> {
    let backend = SecureProjects::new(&[]).map_err(|_| "backend")?;
    let registry = Arc::new(Mutex::new(
        Registry::new(backend, OsReferences, MonotonicClock::default(), 10, 1)
            .map_err(|_| "registry")?,
    ));
    let resources = Resources::new(
        Arc::clone(&registry),
        Workers::new(),
        Arc::new(AtomicBool::new(true)),
    )?
    .with_quality_reader(Arc::new(ContentionMaskedReader {
        registry: Arc::clone(&registry),
    }));
    let (tasks, job_id) = super::super::tasks::tests::running_tasks_for_resource_test()?;
    let owner: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let job: QualityJobId = "job_00000000000000000000000000000002".parse()?;
    let held = Arc::clone(&registry);
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = held.lock().map_err(|_| "registry lock")?;
        locked_tx.send(()).map_err(|_| "locked signal")?;
        release_rx.recv().map_err(|_| "release signal")
    });
    locked_rx.recv()?;

    let read = tokio::time::timeout(
        Duration::from_millis(100),
        resources.read_uri(&quality_index_uri(&owner, &job), CancellationToken::new()),
    )
    .await?;
    assert_eq!(read.err(), Some(not_found()));
    let polled = tokio::time::timeout(Duration::from_millis(100), tasks.get(&job_id)).await??;
    assert_eq!(polled.task.status(), rmcp::model::TaskStatus::Working);
    release_tx.send(())?;
    holder.join().map_err(|_| "holder panic")??;
    Ok(())
}

struct TouchRecordingQualityReader(Arc<AtomicBool>);
impl QualityResourceReader for TouchRecordingQualityReader {
    fn read_chunk(
        &self,
        _: &ProjectRef,
        _: &QualityArtifactId,
        _: u64,
        _: u32,
    ) -> Result<QualityArtifactChunk, ()> {
        self.0.store(true, Ordering::Release);
        Err(())
    }
    fn read_index(
        &self,
        _: &ProjectRef,
        _: &QualityJobId,
        _: Option<&str>,
    ) -> Result<QualityArtifactIndexPage, ()> {
        self.0.store(true, Ordering::Release);
        Err(())
    }
    fn is_live(&self, _: &ProjectRef, _: &QualityArtifactId) -> bool {
        self.0.store(true, Ordering::Release);
        false
    }
}

#[tokio::test]
async fn bootstrap_gate_precedes_the_quality_resource_branch()
-> Result<(), Box<dyn std::error::Error>> {
    let backend = SecureProjects::new(&[]).map_err(|_| "backend")?;
    let registry = Registry::new(backend, OsReferences, MonotonicClock::default(), 10, 1)
        .map_err(|_| "registry")?;
    let touched = Arc::new(AtomicBool::new(false));
    let resources = Resources::new(
        Arc::new(Mutex::new(registry)),
        Workers::new(),
        Arc::new(AtomicBool::new(false)),
    )?
    .with_quality_reader(Arc::new(TouchRecordingQualityReader(Arc::clone(&touched))));
    let owner: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let job: QualityJobId = "job_00000000000000000000000000000002".parse()?;
    assert_eq!(
        resources
            .read_uri(&quality_index_uri(&owner, &job), CancellationToken::new())
            .await
            .err(),
        Some(not_found())
    );
    assert!(!touched.load(Ordering::Acquire));
    Ok(())
}

#[tokio::test]
async fn active_job_permit_makes_worker_backed_resource_read_busy_without_queueing()
-> Result<(), Box<dyn std::error::Error>> {
    let backend = SecureProjects::new(&[]).map_err(|_| "backend")?;
    let registry = Registry::new(backend, OsReferences, MonotonicClock::default(), 10, 1)
        .map_err(|_| "registry")?;
    let workers = Workers::new();
    let permit = workers.admit_job().map_err(|_| "admit")?;
    let resources = Resources::new(
        Arc::new(Mutex::new(registry)),
        workers.clone(),
        Arc::new(AtomicBool::new(true)),
    )?;
    let value = uri(
        &"prj_00000000000000000000000000000001".parse()?,
        &"art_00000000000000000000000000000002".parse()?,
    );
    let error = resources
        .read_uri(&value, CancellationToken::new())
        .await
        .err()
        .ok_or("read unexpectedly succeeded")?;
    assert_eq!(error.code, rmcp::model::ErrorCode(-32000));
    assert_eq!(
        error.message,
        "Artifact worker is busy; retry after the active operation"
    );
    permit.release_after_cleanup();
    assert!(workers.shutdown(Duration::from_secs(1)).await);
    Ok(())
}

fn quality_descriptor(
    member_index: u16,
    size_bytes: u64,
) -> Result<QualityArtifactDescriptor, Box<dyn std::error::Error>> {
    let created: UtcInstant = "2026-09-06T00:00:00Z".parse()?;
    Ok(QualityArtifactDraft {
        artifact_id: format!("qart_{member_index:032x}").parse()?,
        member_index,
        kind: QualityArtifactKind::ToolLog,
        mime_type: QualityMimeType::TextPlain,
        payload_format_version: PayloadFormatVersion::Utf8LogV1,
        completeness: ArtifactCompleteness::Complete,
        sensitivity: ArtifactSensitivity::Public,
        expires_at_utc: created.checked_add_seconds(3_600)?,
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
    }
    .into_descriptor(
        "job_00000000000000000000000000000002".parse()?,
        [8; 32],
        [9; 32],
        size_bytes,
    )?)
}

#[test]
fn a_maximal_quality_chunk_stays_below_the_complete_response_cap()
-> Result<(), Box<dyn std::error::Error>> {
    let owner: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let descriptor = quality_descriptor(0, QUALITY_RESOURCE_CHUNK_BYTES as u64)?;
    let bytes = vec![0xff_u8; QUALITY_RESOURCE_CHUNK_BYTES];
    let response = encode_quality_chunk(
        &owner,
        QualityArtifactChunk {
            descriptor: descriptor.clone(),
            offset: 0,
            bytes: bytes.clone(),
        },
    )?;
    let serialized = serde_json::to_vec(&response)?;
    // 320 KiB raw is 436,908 base64 bytes, still under the 512 KiB cap.
    assert_eq!(STANDARD.encode(&bytes).len(), 436_908);
    assert!(serialized.len() <= MAX_RESPONSE, "{}", serialized.len());
    let value = serde_json::to_value(&response)?;
    assert_eq!(value["cacheScope"], "private");
    assert_eq!(value["ttlMs"], 0);
    let content = &value["contents"][0];
    assert_eq!(content["mimeType"], "application/octet-stream");
    assert_eq!(
        STANDARD.decode(content["blob"].as_str().ok_or("blob")?)?,
        bytes
    );
    // The metadata is exactly the closed descriptor-derived set: no owner
    // binding, sensitivity, source, runtime or host path crosses the boundary.
    let mut keys = content["_meta"]
        .as_object()
        .ok_or("meta")?
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    keys.sort();
    assert_eq!(keys, ["artifact_id", "job_id", "offset", "size_bytes"]);
    assert_eq!(
        content["uri"],
        quality_chunk_uri(
            &owner,
            &descriptor.artifact_id,
            0,
            QUALITY_RESOURCE_CHUNK_BYTES as u32
        )
    );

    // A chunk longer than its own descriptor claims is never serialized.
    assert!(
        encode_quality_chunk(
            &owner,
            QualityArtifactChunk {
                descriptor,
                offset: 0,
                bytes: vec![0; QUALITY_RESOURCE_CHUNK_BYTES + 1],
            },
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn a_quality_index_page_is_bounded_in_rows_and_cursor_bytes()
-> Result<(), Box<dyn std::error::Error>> {
    let owner: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let job: QualityJobId = "job_00000000000000000000000000000002".parse()?;
    let rows = (0..QUALITY_INDEX_PAGE_MEMBERS)
        .map(|index| quality_descriptor(index as u16, 4_096))
        .collect::<Result<Vec<_>, _>>()?;
    let page = QualityArtifactIndexPage {
        rows: rows.clone(),
        next_cursor: Some(b"m0000000064".to_vec()),
    };
    let response = encode_quality_index(&owner, &job, page)?;
    let serialized = serde_json::to_vec(&response)?;
    assert!(serialized.len() <= MAX_RESPONSE, "{}", serialized.len());
    let value = serde_json::to_value(&response)?;
    assert_eq!(value["contents"][0]["mimeType"], "application/json");
    assert_eq!(value["cacheScope"], "private");
    let body: serde_json::Value =
        serde_json::from_str(value["contents"][0]["text"].as_str().ok_or("text")?)?;
    assert_eq!(body["members"].as_array().map(Vec::len), Some(64));
    assert_eq!(body["next_cursor"], "m0000000064");
    // No owner binding, timestamp or digest crosses the index boundary.
    assert!(body["members"][0].get("owner_binding").is_none());
    assert!(body["members"][0].get("sha256").is_none());

    // One row too many and an over-long cursor are both refused.
    let mut over = rows.clone();
    over.push(quality_descriptor(64, 4_096)?);
    assert!(
        encode_quality_index(
            &owner,
            &job,
            QualityArtifactIndexPage {
                rows: over,
                next_cursor: None,
            },
        )
        .is_err()
    );
    assert!(
        encode_quality_index(
            &owner,
            &job,
            QualityArtifactIndexPage {
                rows,
                next_cursor: Some(vec![b'a'; QUALITY_CURSOR_MAX_BYTES + 1]),
            },
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn quality_uris_are_never_served_without_an_installed_reader()
-> Result<(), Box<dyn std::error::Error>> {
    let owner: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let job: QualityJobId = "job_00000000000000000000000000000002".parse()?;
    let backend = SecureProjects::new(&[]).map_err(|_| "backend")?;
    let registry = Registry::new(backend, OsReferences, MonotonicClock::default(), 10, 1)
        .map_err(|_| "registry")?;
    let resources = Resources::new(
        Arc::new(Mutex::new(registry)),
        Workers::new(),
        Arc::new(AtomicBool::new(true)),
    )?;
    let value = quality_index_uri(&owner, &job);
    let outcome = tokio::runtime::Builder::new_current_thread()
        .build()?
        .block_on(resources.read_uri(&value, CancellationToken::new()));
    assert_eq!(outcome.err(), Some(not_found()));
    Ok(())
}

#[test]
fn blob_metadata_describes_actual_stored_bytes() -> Result<(), Box<dyn std::error::Error>> {
    use rust_engineering_application::{ArtifactInput, ArtifactStore};
    struct Input(bool);
    impl ArtifactInput for Input {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, ArtifactError> {
            if self.0 {
                return Ok(0);
            }
            buffer[..3].copy_from_slice(b"abc");
            self.0 = true;
            Ok(3)
        }
    }
    let clock = ArtifactClock(Instant::now());
    let mut store = MemoryArtifactStore::new(clock.clone(), ArtifactLimits::default(), Vec::new())?;
    let owner = artifact(vec![])?.metadata.owner;
    let metadata = store.capture(&owner, &mut Input(false))?;
    let view = store.read(&owner, &metadata.id)?;
    let response = encode(AuthorizedArtifact {
        metadata: view.metadata.clone(),
        content: view.content.to_vec(),
        retention_remaining_seconds: view.metadata.expires_seconds - clock.seconds(),
    })?;
    let value = serde_json::to_value(response)?;
    assert_eq!(value["contents"][0]["blob"], "YWJj");
    assert_eq!(
        value["contents"][0]["_meta"]["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(value["contents"][0]["_meta"]["size_bytes"], 3);
    assert_eq!(value["contents"][0]["_meta"]["truncated"], false);
    Ok(())
}
