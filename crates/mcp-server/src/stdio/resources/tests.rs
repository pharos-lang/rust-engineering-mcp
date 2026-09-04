use super::*;
use rust_engineering_domain::ArtifactMetadata;
use rust_engineering_project::{MonotonicClock, OsReferences, SecureProjects};

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
