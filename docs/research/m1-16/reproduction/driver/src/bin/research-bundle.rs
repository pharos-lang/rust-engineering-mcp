//! Research-only authoring CLI. Public seed42 is not a production publisher.
use ring::signature::{Ed25519KeyPair, KeyPair};
use rust_engineering_catalog::{
    SqliteCatalogRepository,
    bundle::{self, BundleFile, BundleManifest, PublisherTrust, sha256},
};
use rust_engineering_domain::{CrateRecord, Provenance};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn header(name: &str, size: usize) -> [u8; 512] {
    let mut h = [0; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    for (a, b, v) in [
        (100, 108, 0o600),
        (108, 116, 0),
        (116, 124, 0),
        (124, 136, size),
        (136, 148, 0),
        (329, 337, 0),
        (337, 345, 0),
    ] {
        h[a..b].copy_from_slice(format!("{:0width$o}\0", v, width = b - a - 1).as_bytes());
    }
    h[156] = b'0';
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].fill(b' ');
    let checksum: u64 = h.iter().map(|b| u64::from(*b)).sum();
    h[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
    h
}
type AuthoredBundle = (Vec<u8>, Vec<u8>, PublisherTrust, Vec<u8>);
fn create(records: &[CrateRecord], provenance: Provenance) -> Result<AuthoredBundle> {
    let snapshot = SqliteCatalogRepository::build(1, provenance.clone(), records)?;
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
    let key = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).map_err(|_| "public fixture key")?;
    let trust = PublisherTrust {
        publisher: "fixture-only".into(),
        channel: "test".into(),
        public_key: key
            .public_key()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    };
    let mut message = b"rust-engineering-catalog-bundle-v1\0".to_vec();
    message.extend_from_slice(&manifest);
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
    let encoded = zstd::stream::encode_all(archive.as_slice(), 1)?;
    bundle::verify(&encoded, &trust)?;
    Ok((encoded, snapshot.bytes, trust, manifest))
}
fn input(path: &Path) -> Result<Vec<u8>> {
    if !path.is_absolute() {
        return Err("absolute input path required".into());
    };
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("input budget".into());
    };
    Ok(bytes)
}
fn save(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn verify_sources(
    records_path: &Path,
    records: &[CrateRecord],
    raw_records: &[u8],
    raw_provenance: &[u8],
) -> Result<usize> {
    use serde_json::{Value, json};
    let projection = records_path.parent().ok_or("projection parent")?;
    let corpus = projection
        .parent()
        .ok_or("target parent")?
        .join("m1-16-corpus");
    let evidence: Value =
        serde_json::from_slice(&input(&projection.join("source-evidence.json"))?)?;
    let receipt: Value =
        serde_json::from_slice(&input(&projection.join("projection-receipt.json"))?)?;
    let check = |bytes: &[u8], expected: &Value| -> Result<()> {
        if expected.as_str() != Some(sha256(bytes).as_str()) {
            return Err("source digest mismatch".into());
        }
        Ok(())
    };
    check(raw_records, &receipt["records_sha256"])?;
    check(raw_provenance, &receipt["provenance_sha256"])?;
    let facts_bytes = input(&corpus.join("selection/facts.json"))?;
    check(&facts_bytes, &evidence["inputs"]["facts_sha256"])?;
    check(
        &input(&corpus.join("selection/tasks-and-labels.json"))?,
        &evidence["inputs"]["labels_sha256"],
    )?;
    let verify_row = |row: &Value| -> Result<()> {
        let path = Path::new(row["corpus_path"].as_str().ok_or("source path")?);
        if path
            .components()
            .any(|x| !matches!(x, std::path::Component::Normal(_)))
        {
            return Err("source path components".into());
        }
        check(&input(&corpus.join(path))?, &row["sha256"])
    };
    let sources = evidence["verified_sources"].as_array().ok_or("sources")?;
    for row in sources {
        verify_row(row)?;
    }
    for rows in evidence["annotation_sources"]
        .as_object()
        .ok_or("annotations")?
        .values()
    {
        for row in rows.as_array().ok_or("annotation rows")? {
            verify_row(row)?;
        }
    }
    let facts: Value = serde_json::from_slice(&facts_bytes)?;
    let facts = facts["facts"].as_array().ok_or("facts")?;
    let mut groups = std::collections::BTreeMap::<String, Vec<&Value>>::new();
    for fact in facts {
        groups
            .entry(fact["name"].as_str().ok_or("name")?.into())
            .or_default()
            .push(fact);
    }
    let scope = "Research projection, corpus 2026-09-04; only listed cached versions plus captured registry metadata, not a global or live registry. Dependency and advisory rows were not acquired and are omitted from this projection; empty recorded lists do not establish absence or safety. Declared package MSRV/license do not prove transitive compatibility, working integration or legal approval.";
    let mut expected = Vec::new();
    for (name, group) in groups {
        let first = group[0];
        let mut description = format!(
            "{} {}",
            first["description"]
                .as_str()
                .ok_or("description")?
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            scope
        );
        let mut versions = Vec::new();
        for f in group {
            let ident = format!("{}@{}", name, f["version"].as_str().ok_or("version")?);
            if let Some(annotation) = evidence["annotations"][&ident].as_str() {
                description.push_str(&format!(
                    " Authored source-grounded annotation: {annotation} Annotation sources: "
                ));
                let refs = evidence["annotation_sources"][&ident]
                    .as_array()
                    .ok_or("annotation sources")?
                    .iter()
                    .map(|r| {
                        Ok(format!(
                            "{} sha256:{}",
                            r["corpus_path"].as_str().ok_or("path")?,
                            r["sha256"].as_str().ok_or("digest")?
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                description.push_str(&refs.join("; "));
                description.push('.');
            }
            let mut features = f["features"]
                .as_array()
                .ok_or("features")?
                .iter()
                .map(|v| v.as_str().ok_or("feature"))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            features.sort();
            if f["repository"] != first["repository"] {
                return Err("repository divergence".into());
            }
            versions.push(json!({"version":f["version"],"yanked":f["yanked"],"rust_version":f["declared_msrv"],"license":f["license_expression"],"published_at":f["published_at"],"features":features,"dependencies":[],"advisories":[]}));
        }
        expected.push(json!({"name":name,"description":description,"repository":first["repository"],"updated_at":null,"versions":versions}));
    }
    if serde_json::to_value(records)? != json!(expected) {
        return Err("projection differs from retained facts".into());
    }
    Ok(sources.len())
}
fn main() -> Result<()> {
    let args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if args.len() != 3 || !args[2].is_absolute() {
        return Err(
            "usage: research-bundle ABS_RECORDS_JSON ABS_PROVENANCE_JSON ABS_NEW_OUTPUT_DIR".into(),
        );
    }
    let raw_records = input(&args[0])?;
    let raw_provenance = input(&args[1])?;
    let records: Vec<CrateRecord> = serde_json::from_slice(&raw_records)?;
    let provenance: Provenance = serde_json::from_slice(&raw_provenance)?;
    let source_count = verify_sources(&args[0], &records, &raw_records, &raw_provenance)?;
    let mut derived = serde_json::to_value(&provenance)?;
    derived["integrity"] = serde_json::json!("verified");
    let derived: Provenance = serde_json::from_value(derived)?;
    let (encoded, sqlite, trust, manifest) = create(&records, derived.clone())?;
    fs::DirBuilder::new().mode(0o700).create(&args[2])?;
    save(&args[2].join("research.tar.zst"), &encoded)?;
    save(&args[2].join("catalog.sqlite"), &sqlite)?;
    save(
        &args[2].join("trust.json"),
        &serde_json::to_vec_pretty(&trust)?,
    )?;
    save(&args[2].join("manifest.json"), &manifest)?;
    let receipt = serde_json::json!({"status":"locally_verified_research_bundle","production_publisher_approved":false,"public_signing_seed":42,"publisher":"fixture-only","channel":"test","sequence":1,"records_sha256":sha256(&raw_records),"input_provenance_sha256":sha256(&raw_provenance),"catalog_sha256":sha256(&sqlite),"bundle_sha256":sha256(&encoded),"manifest_sha256":sha256(&manifest),"trust_key_sha256":trust.key_fingerprint()?,"crate_count":records.len(),"version_count":records.iter().map(|r|r.versions.len()).sum::<usize>(),"input_provenance":provenance,"output_provenance":derived,"source_rows_rehashed":source_count,"integrity_scope":"Local retained source hashes and exact facts projection revalidated; SQLite hash and public fixture signature verified. No registry publisher authentication, legal approval or global/live completeness implied."});
    save(
        &args[2].join("baseline-projection.json"),
        &serde_json::to_vec_pretty(
            &serde_json::json!({"records":records,"provenance":derived,"snapshot_fingerprint":format!("sha256:{}",sha256(&sqlite))}),
        )?,
    )?;
    save(
        &args[2].join("provenance.json"),
        &serde_json::to_vec_pretty(&derived)?,
    )?;
    save(
        &args[2].join("receipt.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!("{}", serde_json::to_string(&receipt)?);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::*;
    #[test]
    fn signed_research_bundle_verifies_and_corruption_fails() -> Result<()> {
        let provenance = Provenance::new(
            SourceKind::RegistrySnapshot,
            "research-test".parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(100)),
            IntegrityStatus::Verified,
            false,
        )?;
        let records = vec![CrateRecord {
            name: "sample".into(),
            description: "test".into(),
            repository: None,
            updated_at: None,
            versions: vec![VersionRecord {
                version: "1.0.0".into(),
                yanked: false,
                rust_version: None,
                license: None,
                published_at: None,
                features: vec![],
                dependencies: vec![],
                advisories: vec![],
            }],
        }];
        let (mut bytes, _, trust, _) = create(&records, provenance)?;
        let verified = bundle::verify(&bytes, &trust)?;
        assert_eq!(verified.manifest().sequence, 1);
        bytes[0] ^= 1;
        assert!(bundle::verify(&bytes, &trust).is_err());
        Ok(())
    }
}
