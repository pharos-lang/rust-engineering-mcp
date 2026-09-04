//! Public seed42/test publisher only. All data is illustrative and locally authored.
use ring::signature::Ed25519KeyPair;
use rust_engineering_catalog::{
    SqliteCatalogRepository,
    bundle::{BundleFile, BundleManifest, sha256},
};
use rust_engineering_domain::*;

fn version(value: &str, msrv: Option<&str>) -> VersionRecord {
    VersionRecord {
        version: value.into(),
        yanked: false,
        rust_version: msrv.map(str::to_owned),
        license: Some("MIT".into()),
        published_at: Some(100),
        features: vec![],
        dependencies: vec![],
        advisories: vec![],
    }
}
pub fn records() -> Vec<CrateRecord> {
    let mut old = version("1.0.0", Some("1.60"));
    old.advisories = vec!["RUSTSEC-2020-0001".into()];
    let mut yanked = version("3.0.0", Some("1.60"));
    yanked.yanked = true;
    [
        (
            "choice",
            "parser structured data parser",
            vec![
                old,
                version("2.0.0", Some("1.90")),
                yanked,
                version("4.0.0-alpha", Some("1.60")),
            ],
        ),
        (
            "alpha",
            "parser text tokens",
            vec![version("1.0.0", Some("1.60"))],
        ),
        (
            "beta",
            "parser text grammar",
            vec![version("1.0.0", Some("1.60"))],
        ),
        (
            "unknown",
            "parser unknown compatibility",
            vec![version("1.0.0", None)],
        ),
        (
            "unstable",
            "parser nightly compatibility",
            vec![version("1.0.0", Some("1.70.0-nightly"))],
        ),
        (
            "preview",
            "parser prerelease only",
            vec![version("1.0.0-alpha", Some("1.60"))],
        ),
        (
            "channel",
            "asynchronous channels bounded concurrency",
            vec![version("1.0.0", Some("1.60"))],
        ),
        (
            "unicode",
            "Unicode text normalization normalización español",
            vec![version("1.0.0", Some("1.60"))],
        ),
    ]
    .into_iter()
    .map(|(name, description, versions)| CrateRecord {
        name: name.into(),
        description: description.into(),
        repository: Some(format!("https://example.invalid/{name}")),
        updated_at: Some(100),
        versions,
    })
    .collect()
}
pub fn bundle() -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "local-search-fixture".parse()?,
        Some(UnixSeconds(100)),
        Some(UnixSeconds(100)),
        IntegrityStatus::Verified,
        false,
    )?;
    let snapshot = SqliteCatalogRepository::build(1, provenance.clone(), &records())?;
    let manifest = BundleManifest {
        snapshot_format_version: 1,
        catalog_schema_version: 1,
        semantic_index_version: None,
        embedding_model_id: None,
        publisher: "fixture-only".into(),
        channel: "test".into(),
        sequence: 1,
        catalog_provenance: provenance,
        files: vec![BundleFile {
            path: "catalog.sqlite".into(),
            byte_length: snapshot.bytes.len() as u64,
            sha256: sha256(&snapshot.bytes),
        }],
    };
    let manifest = serde_json::to_vec(&manifest)?;
    let mut message = b"rust-engineering-catalog-bundle-v1\0".to_vec();
    message.extend_from_slice(&manifest);
    let key = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).map_err(|_| "fixture key")?;
    let signature = key.sign(&message);
    let mut archive = Vec::new();
    for (name, bytes) in [
        ("manifest.json", manifest.as_slice()),
        ("signature.ed25519", signature.as_ref()),
        ("catalog.sqlite", snapshot.bytes.as_slice()),
    ] {
        archive.extend_from_slice(&header(name, bytes.len()));
        archive.extend_from_slice(bytes);
        archive.resize(archive.len().next_multiple_of(512), 0);
    }
    archive.resize(archive.len() + 1024, 0);
    Ok(zstd::stream::encode_all(archive.as_slice(), 1)?)
}
fn header(name: &str, size: usize) -> [u8; 512] {
    let mut h = [0; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    for (start, end, value) in [
        (100, 108, 0o600usize),
        (108, 116, 0),
        (116, 124, 0),
        (124, 136, size),
        (136, 148, 0),
        (329, 337, 0),
        (337, 345, 0),
    ] {
        h[start..end]
            .copy_from_slice(format!("{:0width$o}\0", value, width = end - start - 1).as_bytes());
    }
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].fill(b' ');
    let sum: u64 = h.iter().map(|b| u64::from(*b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    h
}
