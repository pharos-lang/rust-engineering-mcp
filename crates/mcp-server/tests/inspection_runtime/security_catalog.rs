//! Native-test fixture authority only. The public seed 42 is never production trust.
use super::*;
use ring::signature::Ed25519KeyPair;
use rust_engineering_catalog::{
    SqliteCatalogRepository,
    bundle::{self, BundleFile, BundleManifest, PublisherTrust, SequenceFloor},
};
use rust_engineering_domain::*;
use std::path::Path;

pub(super) fn install(root: &Path, observed: u64) -> Result {
    fs::create_dir(root)?;
    fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    let trust = include_bytes!("../../../../fixtures/catalog/fixture-trust.json");
    let bytes = signed_bundle(1, observed)?;
    let verified = bundle::verify(&bytes, &PublisherTrust::parse(trust)?)?;
    for (name, bytes) in [
        ("trust.json", trust.to_vec()),
        ("floor.record", SequenceFloor::new(&verified).bytes()?),
        ("active.bundle", bytes),
    ] {
        fs::write(root.join(name), bytes)?;
        fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
fn records() -> Vec<CrateRecord> {
    vec![CrateRecord {
        name: "choice".into(),
        description: "Exact-version yanked fixture".into(),
        repository: Some("https://example.invalid/choice".into()),
        updated_at: Some(100),
        versions: vec![
            VersionRecord {
                version: "1.0.0".into(),
                yanked: false,
                rust_version: Some("1.60".into()),
                license: Some("MIT".into()),
                published_at: Some(100),
                features: vec![],
                dependencies: vec![],
                advisories: vec![],
            },
            VersionRecord {
                version: "3.0.0".into(),
                yanked: true,
                rust_version: Some("1.60".into()),
                license: Some("MIT".into()),
                published_at: Some(100),
                features: vec![],
                dependencies: vec![],
                advisories: vec![],
            },
        ],
    }]
}

fn signed_bundle(sequence: u64, observed: u64) -> Result<Vec<u8>> {
    let provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "supply-catalog-seed42".parse()?,
        Some(UnixSeconds(observed)),
        Some(UnixSeconds(observed)),
        IntegrityStatus::Verified,
        false,
    )?;
    let snapshot = SqliteCatalogRepository::build(sequence, provenance.clone(), &records())?;
    let manifest = BundleManifest {
        snapshot_format_version: 1,
        catalog_schema_version: 1,
        semantic_index_version: None,
        embedding_model_id: None,
        publisher: "fixture-only".into(),
        channel: "test".into(),
        sequence,
        catalog_provenance: provenance,
        files: vec![BundleFile {
            path: "catalog.sqlite".into(),
            byte_length: snapshot.bytes.len() as u64,
            sha256: bundle::sha256(&snapshot.bytes),
        }],
    };
    let manifest = serde_json::to_vec(&manifest)?;
    let mut message = b"rust-engineering-catalog-bundle-v1\0".to_vec();
    message.extend_from_slice(&manifest);
    let key = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).map_err(|_| "fixture key")?;
    let signature = key.sign(&message).as_ref().to_vec();
    let sqlite = snapshot.bytes;
    let mut archive = Vec::new();
    for (name, bytes) in [
        ("manifest.json", manifest.as_slice()),
        ("signature.ed25519", signature.as_slice()),
        ("catalog.sqlite", sqlite.as_slice()),
    ] {
        archive.extend_from_slice(&tar_header(name, bytes.len()));
        archive.extend_from_slice(bytes);
        archive.resize(archive.len().next_multiple_of(512), 0);
    }
    archive.resize(archive.len() + 1024, 0);
    Ok(zstd::stream::encode_all(archive.as_slice(), 1)?)
}

fn tar_header(name: &str, size: usize) -> [u8; 512] {
    let mut header = [0; 512];
    header[..name.len()].copy_from_slice(name.as_bytes());
    for (start, end, value) in [
        (100, 108, 0o600usize),
        (108, 116, 0),
        (116, 124, 0),
        (124, 136, size),
        (136, 148, 0),
        (329, 337, 0),
        (337, 345, 0),
    ] {
        header[start..end]
            .copy_from_slice(format!("{:0width$o}\0", value, width = end - start - 1).as_bytes());
    }
    header[156] = b'0';
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[148..156].fill(b' ');
    let sum: u64 = header.iter().map(|byte| u64::from(*byte)).sum();
    header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    header
}
