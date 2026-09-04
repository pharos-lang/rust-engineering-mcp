#![cfg(feature = "local")]
//! Explicitly invoked with a verified local model. All filesystem access below is
//! development fixture loading, never part of the production adapter.
use rust_engineering_application::{
    CatalogRepository, EmbeddingProvider, SemanticIndex, search_hybrid,
};
use rust_engineering_catalog::SqliteCatalogRepository;
use rust_engineering_domain::*;
use rust_engineering_semantic::*;
use std::{fs::File, io::Read};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Now;
impl Clock for Now {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(2000)
    }
}
fn policy() -> Result<FreshnessPolicy> {
    Ok(FreshnessPolicy::new("test".parse()?, 100, 1000)?)
}

#[test]
#[ignore = "Requires RUST_MCP_E5_DIR and explicit network-isolation calibration via scripts/test-semantic.py"]
fn real_offline_e5_lance_sqlite_roundtrip() -> Result {
    if std::env::var("RUST_MCP_NETWORK_DENIED").as_deref() != Ok("1") {
        return Err("network gate required".into());
    }
    // Matching positive controls are executed by the harness before this process.
    for address in ["127.0.0.1:0", "[::1]:0"] {
        let error = std::net::TcpListener::bind(address)
            .err()
            .ok_or("network bind unexpectedly allowed")?;
        assert_eq!(error.raw_os_error(), Some(1));
    }
    let directory = std::env::var("RUST_MCP_E5_DIR")?;
    let mut files: [Vec<u8>; 5] = Default::default();
    for (target, (name, size, _)) in files.iter_mut().zip(E5_FILES) {
        let file = File::open(std::path::Path::new(&directory).join(name))?;
        assert_eq!(file.metadata()?.len(), size as u64);
        file.take(size as u64 + 1).read_to_end(target)?;
    }
    // Same-size corruption must fail the hash check, not just length validation.
    files[4][0] ^= 1;
    // This adversarial gate temporarily clones the 487MB bundle because verification
    // consumes owned bytes. Production does not make this test-only copy.
    let mut broken: [Vec<u8>; 5] = Default::default();
    for (dst, src) in broken.iter_mut().zip(files.iter()) {
        *dst = src.clone();
    }
    assert!(matches!(
        VerifiedE5Bundle::verify(broken),
        Err(SemanticError::InvalidArtifact)
    ));
    files[4][0] ^= 1;
    let bundle = VerifiedE5Bundle::verify(files)?;
    assert_eq!(bundle.byte_length(), 487352503);
    let runtime = OfflineRuntime::initialize()?;
    let mut provider = LocalEmbeddingProvider::load(&runtime, bundle)?;
    assert_eq!(provider.identity().dimension, 384);
    assert!(!provider.identity().provenance.network_used());
    let evidence =
        SnapshotEvidence::assess(provider.identity().provenance.clone(), policy()?, &Now);
    assert_eq!(evidence.freshness().state(), FreshnessState::Unknown);
    assert_eq!(
        provider.embed_passage("line one\nline two\tend")?,
        provider.embed_passage("line one line two end")?
    );
    assert!(provider.embed_query("").is_err());
    assert!(provider.embed_query(&"x".repeat(257)).is_err());
    let passages = [
        (
            "ownership",
            "Rust ownership and borrowing prevent memory safety errors. The borrow checker enforces reference lifetimes and exclusive mutable access.",
        ),
        (
            "serde_json",
            "Serde is a Rust framework for serialization and deserialization. serde_json converts Rust structs to JSON and parses JSON into typed values.",
        ),
        (
            "tokio",
            "Tokio is an asynchronous runtime for Rust. It schedules async tasks and provides timers and asynchronous I/O primitives.",
        ),
    ];
    let records: Vec<_> = passages
        .iter()
        .map(|(name, text)| CrateRecord {
            name: (*name).to_owned(),
            description: (*text).to_owned(),
            repository: None,
            updated_at: None,
            versions: vec![VersionRecord {
                version: "1.0.0".to_owned(),
                yanked: false,
                rust_version: None,
                license: Some("MIT".to_owned()),
                published_at: None,
                features: vec![],
                dependencies: vec![],
                advisories: vec![],
            }],
        })
        .collect();
    let provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "local-test".parse()?,
        Some(UnixSeconds(1000)),
        Some(UnixSeconds(1000)),
        IntegrityStatus::Verified,
        false,
    )?;
    let snapshot = SqliteCatalogRepository::build(1, provenance, &records)?;
    let repository = SqliteCatalogRepository::open(&snapshot.bytes, &snapshot.manifest)?;
    let metadata = IndexMetadata {
        schema_version: 1,
        snapshot_fingerprint: repository.metadata().fingerprint.clone(),
        model: provider.identity().clone(),
    };
    let rows: Vec<_> = passages
        .iter()
        .map(|(id, text)| Ok(((*id).to_owned(), provider.embed_passage(text)?)))
        .collect::<Result<_>>()?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async {
        let mut index = LanceMemoryIndex::build(metadata.clone(), rows.clone()).await?;
        for (text, expected) in [
            (
                "¿Cómo evita Rust los errores de memoria mediante préstamos y ownership?",
                "ownership",
            ),
            (
                "Which Rust library serializes structs into JSON?",
                "serde_json",
            ),
        ] {
            let query = provider.embed_query(text)?;
            validate_embedding(&query, 384)?;
            let found = index.candidates(&query, 3).await?;
            assert_eq!(found.len(), 3);
            assert_eq!(found[0].crate_name, expected);
            println!("E5/Lance result for {expected}: {found:?}");
        }
        let request = CatalogQuery::new("préstamos seguros".to_owned(), 3)?;
        let hybrid = search_hybrid(
            &repository,
            &request,
            Some(&mut provider),
            Some(&index),
            policy()?,
            &Now,
        )
        .await?;
        assert_eq!(hybrid.effective_mode, SearchMode::Hybrid);
        assert_eq!(hybrid.results.crates.len(), 3);
        assert_eq!(hybrid.results.crates[0].name, "ownership");
        assert!(
            hybrid
                .results
                .crates
                .iter()
                .all(|r| r.latest_known.version == "1.0.0")
        );
        let mut mismatch = metadata.clone();
        mismatch.model.revision = "different".to_owned();
        let bad = LanceMemoryIndex::build(mismatch, rows.clone()).await?;
        assert_eq!(
            search_hybrid(
                &repository,
                &request,
                Some(&mut provider),
                Some(&bad),
                policy()?,
                &Now
            )
            .await?
            .fallback,
            Some(SemanticError::IdentityMismatch)
        );
        let before = index
            .candidates(&provider.embed_query("JSON serialization")?, 3)
            .await?;
        assert!(
            index
                .rebuild(
                    metadata.clone(),
                    vec![("bad".to_owned(), vec![f32::NAN; 384])]
                )
                .await
                .is_err()
        );
        assert_eq!(index.metadata(), &metadata);
        index.rebuild(metadata, rows).await?;
        let after = index
            .candidates(&provider.embed_query("JSON serialization")?, 3)
            .await?;
        assert_eq!(
            before.iter().map(|r| &r.crate_name).collect::<Vec<_>>(),
            after.iter().map(|r| &r.crate_name).collect::<Vec<_>>()
        );
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    println!(
        "PASS real E5 + LanceDB + authoritative SQLite + rebuild + mismatch fallback, network denied"
    );
    Ok(())
}
