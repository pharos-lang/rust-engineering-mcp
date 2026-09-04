use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};
use rust_engineering_domain::{SourceKind, UnixSeconds};
type ArchiveEntries = Vec<(String, Vec<u8>, u8)>;
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn pair() -> Result<Ed25519KeyPair, BundleError> {
    Ed25519KeyPair::from_seed_unchecked(&[42; 32]).map_err(|_| BundleError::InvalidTrust)
}
fn trust() -> Result<PublisherTrust, BundleError> {
    Ok(PublisherTrust {
        publisher: "fixture-only".into(),
        channel: "test".into(),
        public_key: pair()?
            .public_key()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    })
}
fn snapshot(sequence: u64) -> Result<crate::Snapshot, Box<dyn std::error::Error>> {
    Ok(SqliteCatalogRepository::build(
        sequence,
        Provenance::new(
            SourceKind::RegistrySnapshot,
            "fixture-only-registry".parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(100)),
            IntegrityStatus::Verified,
            false,
        )?,
        &[rust_engineering_domain::CrateRecord {
            name: "serde".into(),
            description: "Serialization framework".into(),
            repository: Some("https://github.com/serde-rs/serde".into()),
            updated_at: Some(100),
            versions: vec![rust_engineering_domain::VersionRecord {
                version: "1.0.0".into(),
                yanked: false,
                rust_version: Some("1.56".into()),
                license: Some("MIT OR Apache-2.0".into()),
                published_at: Some(100),
                features: vec!["derive".into()],
                dependencies: vec![],
                advisories: vec![],
            }],
        }],
    )?)
}
fn manifest(sequence: u64, bytes: &[u8]) -> Result<BundleManifest, Box<dyn std::error::Error>> {
    let snapshot = snapshot(sequence)?;
    let repository = SqliteCatalogRepository::open(&snapshot.bytes, &snapshot.manifest)?;
    Ok(BundleManifest {
        snapshot_format_version: 1,
        catalog_schema_version: 1,
        semantic_index_version: None,
        embedding_model_id: None,
        publisher: "fixture-only".into(),
        channel: "test".into(),
        sequence,
        catalog_provenance: repository.metadata().provenance.clone(),
        files: vec![BundleFile {
            path: "catalog.sqlite".into(),
            byte_length: bytes.len() as u64,
            sha256: sha256(bytes),
        }],
    })
}
fn header(path: &str, size: usize, kind: u8) -> [u8; 512] {
    let mut h = [0u8; 512];
    h[..path.len().min(100)].copy_from_slice(&path.as_bytes()[..path.len().min(100)]);
    for (start, end, value) in [
        (100, 108, 0o600usize),
        (108, 116, 0),
        (116, 124, 0),
        (124, 136, size),
        (136, 148, 0),
        (329, 337, 0),
        (337, 345, 0),
    ] {
        let text = format!("{:0width$o}\0", value, width = end - start - 1);
        h[start..end].copy_from_slice(text.as_bytes());
    }
    h[156] = kind;
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    checksum(&mut h);
    h
}
fn checksum(h: &mut [u8; 512]) {
    h[148..156].fill(b' ');
    let sum: u64 = h.iter().map(|b| u64::from(*b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
}
fn tar(entries: &[(String, Vec<u8>, u8)]) -> Vec<u8> {
    let mut bytes = vec![];
    for (name, data, kind) in entries {
        bytes.extend(header(name, data.len(), *kind));
        bytes.extend(data);
        bytes.resize(bytes.len().next_multiple_of(512), 0);
    }
    bytes.resize(bytes.len() + 1024, 0);
    bytes
}
fn compressed(entries: &[(String, Vec<u8>, u8)]) -> Result<Vec<u8>, std::io::Error> {
    zstd::stream::encode_all(tar(entries).as_slice(), 1)
}
fn entries(sequence: u64) -> Result<ArchiveEntries, Box<dyn std::error::Error>> {
    let s = snapshot(sequence)?;
    let m = serde_json::to_vec(&manifest(sequence, &s.bytes)?)?;
    signed_entries(m, s.bytes)
}
fn signed_entries(m: Vec<u8>, data: Vec<u8>) -> Result<ArchiveEntries, Box<dyn std::error::Error>> {
    let mut msg = SIGNING_CONTEXT.to_vec();
    msg.extend(&m);
    let signature = pair()?.sign(&msg);
    Ok(vec![
        ("manifest.json".into(), m, b'0'),
        (
            "signature.ed25519".into(),
            signature.as_ref().to_vec(),
            b'0',
        ),
        ("catalog.sqlite".into(), data, b'0'),
    ])
}

fn signed_payloads(
    m: &BundleManifest,
    payloads: &ArchiveEntries,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut archive = signed_entries(serde_json::to_vec(m)?, vec![])?;
    archive.truncate(2);
    archive.extend(payloads.iter().cloned());
    Ok(compressed(&archive)?)
}

fn describe_payloads(m: &mut BundleManifest, payloads: &ArchiveEntries) {
    m.files = payloads
        .iter()
        .map(|(path, bytes, _)| BundleFile {
            path: path.clone(),
            byte_length: bytes.len() as u64,
            sha256: sha256(bytes),
        })
        .collect();
}

fn multiple_payloads() -> Result<(BundleManifest, ArchiveEntries), Box<dyn std::error::Error>> {
    let catalog = snapshot(1)?;
    let rustsec = serde_json::to_vec(&crate::RustSecSnapshotDocument {
        format_version: 1,
        sequence: 1,
        source_id: "fixture-rustsec-rsa".into(),
        created_at: Some(100),
        observed_at: Some(100),
        records: vec![crate::RustSecSnapshotRecord {
            path: "crates/rsa/RUSTSEC-2023-0071.md".into(),
            markdown: include_str!("../../tests/fixtures/rustsec/RUSTSEC-2023-0071.md").into(),
        }],
    })?;
    let mut m = manifest(1, &catalog.bytes)?;
    m.semantic_index_version = Some(1);
    m.embedding_model_id = Some("intfloat/multilingual-e5-small".into());
    // The bundle layer authenticates opaque transport bytes, not a usable index.
    let payloads = vec![
        ("catalog.sqlite".into(), catalog.bytes, b'0'),
        ("rustsec.json".into(), rustsec, b'0'),
        (
            "semantic.index".into(),
            b"opaque semantic transport fixture".to_vec(),
            b'0',
        ),
    ];
    describe_payloads(&mut m, &payloads);
    Ok((m, payloads))
}

#[test]
fn authenticates_multiple_payloads_and_retains_exact_owned_bytes() -> TestResult {
    let (m, payloads) = multiple_payloads()?;
    let verified = verify(&signed_payloads(&m, &payloads)?, &trust()?)?;
    assert!(verified.repository().inspect("serde")?.is_some());
    assert_eq!(verified.rustsec_bytes(), Some(payloads[1].1.as_slice()));
    assert_eq!(
        verified.semantic_index_bytes(),
        Some(payloads[2].1.as_slice())
    );
    assert_eq!(verified.manifest().files.len(), 3);

    // A re-signed/hash-consistent but malformed advisory must reach and fail the
    // RustSec parser, rather than being accepted as opaque authenticated bytes.
    let mut bad_payloads = payloads;
    bad_payloads[1].1 = b"{}".to_vec();
    let mut bad_manifest = m;
    describe_payloads(&mut bad_manifest, &bad_payloads);
    assert!(matches!(
        verify(&signed_payloads(&bad_manifest, &bad_payloads)?, &trust()?),
        Err(BundleError::InvalidCatalog)
    ));
    Ok(())
}

#[test]
fn rejects_payload_order_and_signed_manifest_order_independently() -> TestResult {
    let (m, payloads) = multiple_payloads()?;
    // Independently swap the manifest, archive, and both: the latter preserves
    // positional hash equality and therefore discriminates the ascending rule.
    for (swap_manifest, swap_archive) in [(true, false), (false, true), (true, true)] {
        let mut bad_manifest = m.clone();
        let mut bad_payloads = payloads.clone();
        if swap_manifest {
            bad_manifest.files.swap(1, 2);
        }
        if swap_archive {
            bad_payloads.swap(1, 2);
        }
        assert!(
            matches!(
                verify(&signed_payloads(&bad_manifest, &bad_payloads)?, &trust()?),
                Err(BundleError::Integrity)
            ),
            "manifest={swap_manifest}, archive={swap_archive}"
        );
    }
    // A well-ordered, signed set without the mandatory catalog must not treat
    // RustSec's first payload/hash as SQLite.
    let mut no_catalog = m;
    let payloads = payloads[1..].to_vec();
    describe_payloads(&mut no_catalog, &payloads);
    assert!(matches!(
        verify(&signed_payloads(&no_catalog, &payloads)?, &trust()?),
        Err(BundleError::InvalidCatalog)
    ));
    Ok(())
}

#[test]
fn semantic_transport_requires_complete_matching_metadata_and_size() -> TestResult {
    let (m, payloads) = multiple_payloads()?;
    // Exhaust the absent/present/version/model cross product. The all-absent
    // state and the exact declared transport triple are the only valid states.
    for present in [false, true] {
        for version in [None, Some(1), Some(2)] {
            for model in [
                None,
                Some("intfloat/multilingual-e5-small"),
                Some("wrong-model"),
            ] {
                let mut candidate = m.clone();
                candidate.semantic_index_version = version;
                candidate.embedding_model_id = model.map(str::to_owned);
                let mut candidate_payloads = payloads.clone();
                if !present {
                    candidate_payloads.pop();
                }
                describe_payloads(&mut candidate, &candidate_payloads);
                let result = verify(
                    &signed_payloads(&candidate, &candidate_payloads)?,
                    &trust()?,
                );
                let accepted = (!present && version.is_none() && model.is_none())
                    || (present
                        && version == Some(1)
                        && model == Some("intfloat/multilingual-e5-small"));
                if accepted {
                    assert!(
                        result.is_ok(),
                        "present={present}, version={version:?}, model={model:?}"
                    );
                } else {
                    assert!(
                        matches!(result, Err(BundleError::UnsupportedFormat)),
                        "present={present}, version={version:?}, model={model:?}"
                    );
                }
            }
        }
    }
    for size in [16 * 1024 * 1024, 16 * 1024 * 1024 + 1] {
        let mut candidate_payloads = payloads.clone();
        candidate_payloads[2].1 = vec![0; size];
        let mut candidate = m.clone();
        describe_payloads(&mut candidate, &candidate_payloads);
        let result = verify(
            &signed_payloads(&candidate, &candidate_payloads)?,
            &trust()?,
        );
        if size == 16 * 1024 * 1024 {
            assert_eq!(result?.semantic_index_bytes().map(<[u8]>::len), Some(size));
        } else {
            assert!(matches!(result, Err(BundleError::UnsupportedFormat)));
        }
    }
    Ok(())
}

#[test]
fn rejects_bare_manifest_signature_without_domain_context() -> TestResult {
    let mut archive = entries(1)?;
    archive[1].1 = pair()?.sign(&archive[0].1).as_ref().to_vec();
    assert!(matches!(
        verify(&compressed(&archive)?, &trust()?),
        Err(BundleError::InvalidSignature)
    ));
    Ok(())
}

#[test]
fn rejects_signed_manifest_publisher_and_channel_mismatches() -> TestResult {
    let catalog = snapshot(1)?;
    for publisher_mismatch in [true, false] {
        let mut m = manifest(1, &catalog.bytes)?;
        if publisher_mismatch {
            m.publisher = "other-publisher".into();
        } else {
            m.channel = "other-channel".into();
        }
        let archive = signed_entries(serde_json::to_vec(&m)?, catalog.bytes.clone())?;
        assert!(matches!(
            verify(&compressed(&archive)?, &trust()?),
            Err(BundleError::UntrustedPublisher)
        ));
    }
    Ok(())
}

#[test]
fn rejects_signed_sequence_above_sqlite_integer_range_before_catalog_parsing() -> TestResult {
    let catalog = snapshot(1)?;
    for sequence in [i64::MAX as u64 + 1, u64::MAX] {
        let mut m = manifest(1, &catalog.bytes)?;
        m.sequence = sequence;
        let archive = signed_entries(serde_json::to_vec(&m)?, catalog.bytes.clone())?;
        assert!(matches!(
            verify(&compressed(&archive)?, &trust()?),
            Err(BundleError::Budget)
        ));
    }
    Ok(())
}
#[test]
fn authenticates_real_sqlite_and_enforces_sequence() -> TestResult {
    let bytes = compressed(&entries(2)?)?;
    let verified = verify(&bytes, &trust()?)?;
    assert_eq!(verified.manifest().sequence, 2);
    assert!(verified.repository().inspect("serde")?.is_some());
    assert!(verified.require_newer_than(1).is_ok());
    assert_eq!(verified.require_newer_than(2), Err(BundleError::Rollback));
    assert_eq!(verified.require_newer_than(3), Err(BundleError::Rollback));
    assert_eq!(
        verified.manifest().catalog_provenance.observed_at(),
        Some(UnixSeconds(100))
    );
    Ok(())
}
#[test]
fn rejects_signature_hash_identity_and_trust_errors() -> TestResult {
    let e = entries(1)?;
    let t = trust()?;
    for index in [0, 1, 2] {
        let mut bad = e.clone();
        bad[index].1[0] ^= 1;
        assert!(verify(&compressed(&bad)?, &t).is_err());
    }
    let mut wrong = t.clone();
    wrong.publisher = "impostor".into();
    assert!(matches!(
        verify(&compressed(&e)?, &wrong),
        Err(BundleError::UntrustedPublisher)
    ));
    wrong = t.clone();
    wrong.channel = "stable".into();
    assert!(matches!(
        verify(&compressed(&e)?, &wrong),
        Err(BundleError::UntrustedPublisher)
    ));
    wrong = t.clone();
    wrong.public_key = "0".repeat(64);
    assert!(matches!(
        verify(&compressed(&e)?, &wrong),
        Err(BundleError::InvalidSignature)
    ));
    for key in ["A".repeat(64), "a".repeat(63), "z".repeat(64)] {
        wrong.public_key = key;
        assert!(matches!(
            verify(&compressed(&e)?, &wrong),
            Err(BundleError::InvalidTrust)
        ));
    }
    Ok(())
}
#[test]
fn rejects_noncanonical_signed_manifest_and_unknown_schema() -> TestResult {
    let s = snapshot(1)?;
    let m = manifest(1, &s.bytes)?;
    let pretty = serde_json::to_vec_pretty(&m)?;
    assert!(matches!(
        verify(
            &compressed(&signed_entries(pretty, s.bytes.clone())?)?,
            &trust()?
        ),
        Err(BundleError::NoncanonicalManifest)
    ));
    for field in 0..3 {
        let mut bad = m.clone();
        match field {
            0 => bad.catalog_schema_version = 2,
            1 => bad.snapshot_format_version = 2,
            _ => bad.sequence = 0,
        };
        assert!(
            verify(
                &compressed(&signed_entries(serde_json::to_vec(&bad)?, s.bytes.clone())?)?,
                &trust()?
            )
            .is_err()
        );
    }
    Ok(())
}
#[test]
fn rejects_archive_paths_links_devices_extensions_and_duplicates() -> TestResult {
    let e = entries(1)?;
    let t = trust()?;
    for path in [
        "/tmp/x", "../x", "x/../y", "x//y", "./x", "C:\\x", "x/", "x\\y", "x:y",
    ] {
        let mut bad = e.clone();
        bad[2].0 = path.into();
        assert!(
            matches!(
                verify(&compressed(&bad)?, &t),
                Err(BundleError::InvalidArchive)
            ),
            "{path}"
        );
    }
    for kind in *b"1234567xgSLK" {
        let mut bad = e.clone();
        bad[2].2 = kind;
        assert!(matches!(
            verify(&compressed(&bad)?, &t),
            Err(BundleError::InvalidArchive)
        ));
    }
    let mut bad = e.clone();
    bad.push(e[2].clone());
    assert!(verify(&compressed(&bad)?, &t).is_err());
    let mut bad = e;
    bad.push(("extra".into(), vec![], b'0'));
    assert!(matches!(
        verify(&compressed(&bad)?, &t),
        Err(BundleError::Integrity)
    ));
    Ok(())
}
#[test]
fn rejects_tar_truncation_checksum_base256_and_trailing_data() -> TestResult {
    let original = tar(&entries(1)?);
    for size in [0, 512, original.len() - 512, original.len() - 1] {
        assert!(archive::entries(&original[..size]).is_err());
    }
    for position in [0, 148, 257] {
        let mut bad = original.clone();
        bad[position] ^= 1;
        assert!(archive::entries(&bad).is_err());
    }
    let mut bad = original.clone();
    bad[124] = 0x80;
    let h: &mut [u8; 512] = (&mut bad[..512]).try_into()?;
    checksum(h);
    assert!(archive::entries(&bad).is_err());
    let mut bad = original.clone();
    bad.extend([1; 512]);
    assert!(archive::entries(&bad).is_err());
    let mut bad = original;
    bad[157] = b'x';
    let h: &mut [u8; 512] = (&mut bad[..512]).try_into()?;
    checksum(h);
    assert!(archive::entries(&bad).is_err());
    Ok(())
}
#[test]
fn rejects_bombs_and_excess_members_before_catalog_parsing() -> TestResult {
    let excessive = (0..17)
        .map(|i| (format!("file{i}"), vec![], b'0'))
        .collect::<Vec<_>>();
    assert!(matches!(
        archive::entries(&tar(&excessive)),
        Err(BundleError::Budget)
    ));
    let mut huge = header("huge", 0o77777777777, b'0').to_vec();
    huge.extend([0; 1024]);
    assert!(archive::entries(&huge).is_err());
    let bomb = zstd::stream::encode_all(std::io::repeat(0).take((MAX_BUNDLE_BYTES + 1) as u64), 1)?;
    assert!(matches!(verify(&bomb, &trust()?), Err(BundleError::Budget)));
    assert!(matches!(
        verify(&vec![0; MAX_BUNDLE_BYTES + 1], &trust()?),
        Err(BundleError::Budget)
    ));
    Ok(())
}
#[test]
fn signed_catalog_tampering_and_provenance_mismatch_fail() -> TestResult {
    let mut s = snapshot(1)?;
    s.bytes[60..64].copy_from_slice(&2u32.to_be_bytes());
    let m = manifest(1, &s.bytes)?;
    assert!(matches!(
        verify(
            &compressed(&signed_entries(serde_json::to_vec(&m)?, s.bytes)?)?,
            &trust()?
        ),
        Err(BundleError::UnsupportedFormat)
    ));
    let s = snapshot(1)?;
    let mut m = manifest(1, &s.bytes)?;
    m.catalog_provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "false-source".parse()?,
        Some(UnixSeconds(100)),
        Some(UnixSeconds(100)),
        IntegrityStatus::Verified,
        false,
    )?;
    assert!(matches!(
        verify(
            &compressed(&signed_entries(serde_json::to_vec(&m)?, s.bytes)?)?,
            &trust()?
        ),
        Err(BundleError::Integrity)
    ));
    Ok(())
}
#[test]
#[ignore = "explicit maintainer fixture emission; never a distribution identity"]
fn emit_development_fixtures() -> TestResult {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/catalog");
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("fixture-trust.json"),
        serde_json::to_vec_pretty(&trust()?)?,
    )?;
    for seq in [1, 2] {
        std::fs::write(
            dir.join(format!("fixture-{seq}.tar.zst")),
            compressed(&entries(seq)?)?,
        )?;
    }
    Ok(())
}
