//! Public-API round trips through real bundled SQLite and FTS5, entirely in memory.
use rust_engineering_application::{CatalogRepository, search_catalog};
use rust_engineering_catalog::{
    MAX_SNAPSHOT_BYTES, Snapshot, SnapshotManifest, SqliteCatalogRepository,
};
use rust_engineering_domain::{
    CatalogError, CatalogQuery, Clock, CrateRecord, CrateSummary, DependencyKind, DependencyRecord,
    FreshnessPolicy, FreshnessState, IntegrityStatus, KnownVersion, Provenance, SourceKind,
    UnixSeconds, VersionRecord,
};
use sha2::{Digest, Sha256};
use std::cell::Cell;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn provenance() -> TestResult<Provenance> {
    Ok(Provenance::new(
        SourceKind::RegistrySnapshot,
        "fixture-registry-2026-09-03".parse()?,
        Some(UnixSeconds(1000)),
        Some(UnixSeconds(1010)),
        IntegrityStatus::Verified,
        false,
    )?)
}
fn version(version: &str) -> VersionRecord {
    VersionRecord {
        version: version.to_owned(),
        yanked: false,
        rust_version: None,
        license: None,
        published_at: None,
        features: vec![],
        dependencies: vec![],
        advisories: vec![],
    }
}
fn records() -> Vec<CrateRecord> {
    let mut rich = version("1.2.3");
    rich.rust_version = Some("1.81".to_owned());
    rich.license = Some("MIT OR Apache-2.0".to_owned());
    rich.published_at = Some(950);
    rich.features = vec!["derive".to_owned(), "std".to_owned()];
    rich.dependencies = vec![
        DependencyRecord {
            name: "shared".to_owned(),
            requirement: "^2.0".to_owned(),
            kind: DependencyKind::Build,
            optional: false,
        },
        DependencyRecord {
            name: "shared".to_owned(),
            requirement: "=2.1.0".to_owned(),
            kind: DependencyKind::Dev,
            optional: false,
        },
        DependencyRecord {
            name: "shared".to_owned(),
            requirement: ">=2, <3".to_owned(),
            kind: DependencyKind::Normal,
            optional: true,
        },
    ];
    rich.advisories = vec![
        "RUSTSEC-2025-0001".to_owned(),
        "RUSTSEC-2026-0010".to_owned(),
    ];
    let mut yanked = version("2.0.0-beta.1+fixture");
    yanked.yanked = true;
    yanked.rust_version = Some("1.85.0".to_owned());
    yanked.license = Some("Apache-2.0".to_owned());
    yanked.published_at = Some(990);
    vec![
        CrateRecord {
            name: "alpha_json".to_owned(),
            description: "json serialization reliable".to_owned(),
            repository: Some("https://example.invalid/alpha".to_owned()),
            updated_at: Some(999),
            versions: vec![rich, yanked],
        },
        CrateRecord {
            name: "beta_stream".to_owned(),
            description: "streaming io reliable".to_owned(),
            repository: None,
            updated_at: None,
            versions: vec![version("0.0.1")],
        },
        CrateRecord {
            name: "gamma_jsonify".to_owned(),
            description: "jsonify transformation reliable".to_owned(),
            repository: None,
            updated_at: Some(0),
            versions: vec![version("1.0.0")],
        },
    ]
}
fn alpha_summary() -> CrateSummary {
    CrateSummary {
        name: "alpha_json".to_owned(),
        description: "json serialization reliable".to_owned(),
        latest_known: KnownVersion {
            version: "2.0.0-beta.1+fixture".to_owned(),
            yanked: true,
            rust_version: Some("1.85.0".to_owned()),
            license: Some("Apache-2.0".to_owned()),
        },
        version_count: 2,
    }
}

fn snapshot(sequence: u64) -> TestResult<Snapshot> {
    Ok(SqliteCatalogRepository::build(
        sequence,
        provenance()?,
        &records(),
    )?)
}
fn open(snapshot: &Snapshot) -> TestResult<SqliteCatalogRepository> {
    Ok(SqliteCatalogRepository::open(
        &snapshot.bytes,
        &snapshot.manifest,
    )?)
}
fn query(text: &str, limit: u32) -> Result<CatalogQuery, CatalogError> {
    CatalogQuery::new(text.to_owned(), limit)
}
fn rehash(bytes: &[u8], manifest: &SnapshotManifest) -> TestResult<SnapshotManifest> {
    let mut manifest = manifest.clone();
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    manifest.fingerprint = format!("sha256:{hex}").parse()?;
    manifest.byte_length = bytes.len() as u64;
    Ok(manifest)
}
fn rejected(bytes: &[u8], manifest: &SnapshotManifest, expected: CatalogError) {
    assert!(
        matches!(SqliteCatalogRepository::open(bytes, manifest), Err(actual) if actual == expected),
        "expected {expected:?}"
    );
}

#[test]
fn sqlite_roundtrip_preserves_all_normalized_facts_and_provenance() -> TestResult {
    let built = snapshot(7)?;
    assert_eq!(&built.bytes[..16], b"SQLite format 3\0");
    assert_eq!(built.manifest.byte_length, built.bytes.len() as u64);
    let repository = open(&built)?;
    assert_eq!(repository.metadata().sequence, 7);
    assert_eq!(
        repository.metadata().fingerprint,
        built.manifest.fingerprint
    );
    assert_eq!(repository.metadata().provenance, provenance()?);
    for expected in records() {
        assert_eq!(repository.inspect(&expected.name)?, Some(expected));
    }
    assert_eq!(repository.inspect("unknown_crate")?, None);
    let matches = repository.lexical(&query("serialization", 50)?)?;
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0], alpha_summary());
    Ok(())
}

struct AdvancingClock(Cell<u64>);
impl Clock for AdvancingClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0.get())
    }
}

#[test]
fn search_use_case_reassesses_freshness_without_changing_snapshot_facts() -> TestResult {
    let built = snapshot(1)?;
    let repository = open(&built)?;
    let policy = FreshnessPolicy::new("catalog-test-policy".parse()?, 60, 120)?;
    let clock = AdvancingClock(Cell::new(1000));
    let query = query("json", 10)?;
    for (now, expected, age) in [
        (1060, FreshnessState::Fresh, Some(60)),
        (1061, FreshnessState::Aging, Some(61)),
        (1120, FreshnessState::Aging, Some(120)),
        (1121, FreshnessState::Stale, Some(121)),
        (999, FreshnessState::Unknown, None),
    ] {
        clock.0.set(now);
        let page = search_catalog(&repository, &query, policy.clone(), &clock)?;
        assert_eq!(page.snapshot_fingerprint, built.manifest.fingerprint);
        assert_eq!(page.crates, vec![alpha_summary()]);
        assert_eq!(page.evidence.provenance(), &provenance()?);
        assert_eq!(page.evidence.freshness().state(), expected);
        assert_eq!(page.evidence.freshness().age_seconds(), age);
        assert_eq!(page.evidence.freshness().assessed_at(), UnixSeconds(now));
        assert!(!page.evidence.provenance().network_used());
        let json = serde_json::to_value(&page)?;
        assert_ne!(json["evidence"]["freshness"]["state"], "live");
    }
    Ok(())
}

#[test]
fn fts5_treats_operators_as_literal_terms_and_bounds_results() -> TestResult {
    let repository = open(&snapshot(1)?)?;
    assert_eq!(repository.lexical(&query("reliable", 1)?)?.len(), 1);
    assert_eq!(repository.lexical(&query("reliable", 50)?)?.len(), 3);
    assert_eq!(
        repository.lexical(&query("json*", 50)?)?,
        vec![alpha_summary()]
    );
    assert_eq!(
        repository.lexical(&query("\"json\"", 50)?)?,
        vec![alpha_summary()]
    );
    for special in [
        "json OR streaming",
        "name:alpha_json",
        "NEAR(json streaming)",
        "*",
        "\"",
        "json' OR 1=1 --",
    ] {
        assert!(
            repository.lexical(&query(special, 50)?)?.is_empty(),
            "FTS expression was interpreted: {special}"
        );
    }
    assert_eq!(
        repository.inspect("alpha_json")?,
        Some(records()[0].clone())
    );
    for invalid in ["", "' OR 1=1 --", "../alpha", "alpha\0"] {
        assert_eq!(repository.inspect(invalid), Err(CatalogError::InvalidInput));
    }
    for (text, limit) in [
        ("", 1),
        (" ", 1),
        ("json", 0),
        ("json", 51),
        ("json\n", 1),
        ("json\0", 1),
    ] {
        assert!(matches!(
            query(text, limit),
            Err(CatalogError::InvalidInput)
        ));
    }
    assert!(query(&"x".repeat(257), 1).is_err());
    assert!(query(&vec!["x"; 17].join(" "), 1).is_err());
    assert!(query(&"x".repeat(256), 50).is_ok());
    assert!(query(&vec!["x"; 16].join(" "), 50).is_ok());
    Ok(())
}

#[test]
fn rejects_corrupt_bytes_hash_size_wal_and_schema_manifest() -> TestResult {
    let built = snapshot(1)?;
    let mut damaged = built.bytes.clone();
    damaged[100] = 0; // Invalid first b-tree page type; digest is recomputed below.
    rejected(
        &damaged,
        &rehash(&damaged, &built.manifest)?,
        CatalogError::Integrity,
    );
    rejected(&damaged, &built.manifest, CatalogError::Integrity);
    let mut manifest = built.manifest.clone();
    manifest.byte_length += 1;
    rejected(&built.bytes, &manifest, CatalogError::Integrity);
    let mut manifest = built.manifest.clone();
    manifest.format_version = 2;
    rejected(&built.bytes, &manifest, CatalogError::UnsupportedSchema);
    let mut manifest = built.manifest.clone();
    manifest.sequence += 1;
    rejected(&built.bytes, &manifest, CatalogError::InvalidSnapshot);
    for byte in [18, 19] {
        let mut wal = built.bytes.clone();
        wal[byte] = 2;
        rejected(
            &wal,
            &rehash(&wal, &built.manifest)?,
            CatalogError::Integrity,
        );
    }
    let mut future_schema = built.bytes.clone();
    future_schema[60..64].copy_from_slice(&2_u32.to_be_bytes());
    rejected(
        &future_schema,
        &rehash(&future_schema, &built.manifest)?,
        CatalogError::UnsupportedSchema,
    );
    rejected(&built.bytes[..99], &built.manifest, CatalogError::Integrity);
    let too_large = vec![0; MAX_SNAPSHOT_BYTES + 1];
    rejected(&too_large, &built.manifest, CatalogError::Budget);
    Ok(())
}

#[test]
fn failed_activation_preserves_active_snapshot_and_rejects_rollback() -> TestResult {
    let original = snapshot(10)?;
    let mut active = open(&original)?;
    let metadata = active.metadata().clone();
    let mut new_records = records();
    new_records[0].description = "replacement catalog generation".to_owned();
    let next = SqliteCatalogRepository::build(11, provenance()?, &new_records)?;
    let mut corrupted = next.bytes.clone();
    corrupted[0] ^= 1;
    assert_eq!(
        active.activate(&corrupted, &next.manifest),
        Err(CatalogError::Integrity)
    );
    assert_eq!(active.metadata(), &metadata);
    assert_eq!(
        active.lexical(&query("serialization", 10)?)?,
        vec![alpha_summary()]
    );
    assert_eq!(
        active.activate(&original.bytes, &original.manifest),
        Err(CatalogError::Rollback)
    );
    let older = snapshot(9)?;
    assert_eq!(
        active.activate(&older.bytes, &older.manifest),
        Err(CatalogError::Rollback)
    );
    assert_eq!(active.metadata(), &metadata);
    active.activate(&next.bytes, &next.manifest)?;
    assert_eq!(active.metadata().sequence, 11);
    assert_eq!(active.metadata().fingerprint, next.manifest.fingerprint);
    assert!(active.lexical(&query("serialization", 10)?)?.is_empty());
    assert_eq!(
        active.lexical(&query("replacement", 10)?)?,
        vec![CrateSummary {
            description: new_records[0].description.clone(),
            ..alpha_summary()
        }]
    );
    Ok(())
}

#[test]
fn reordered_input_builds_identical_bytes_and_rebuild_preserves_facts() -> TestResult {
    let input = records();
    let mut reordered = input.clone();
    reordered.reverse();
    for krate in &mut reordered {
        krate.versions.reverse();
        for version in &mut krate.versions {
            version.features.reverse();
            version.dependencies.reverse();
            version.advisories.reverse();
        }
    }
    let first = SqliteCatalogRepository::build(3, provenance()?, &input)?;
    let second = SqliteCatalogRepository::build(3, provenance()?, &reordered)?;
    assert_eq!(first.bytes, second.bytes);
    assert_eq!(first.manifest.fingerprint, second.manifest.fingerprint);
    let active = open(&first)?;
    assert!(matches!(active.rebuild(3), Err(CatalogError::Rollback)));
    assert!(matches!(active.rebuild(2), Err(CatalogError::Rollback)));
    let rebuilt = active.rebuild(4)?;
    let expected = SqliteCatalogRepository::build(4, provenance()?, &input)?;
    assert_eq!(rebuilt.bytes, expected.bytes);
    assert_eq!(rebuilt.manifest.fingerprint, expected.manifest.fingerprint);
    let reopened = open(&rebuilt)?;
    assert_eq!(reopened.metadata().sequence, 4);
    assert_eq!(reopened.metadata().provenance, provenance()?);
    for krate in input {
        assert_eq!(reopened.inspect(&krate.name)?, Some(krate));
    }
    assert_eq!(active.metadata().sequence, 3);
    Ok(())
}

#[test]
fn schema_manifest_deserialization_rejects_unknown_fields() -> TestResult {
    let built = snapshot(1)?;
    let mut json = serde_json::to_value(&built.manifest)?;
    json["caller_override"] = serde_json::json!(true);
    assert!(serde_json::from_value::<SnapshotManifest>(json).is_err());
    Ok(())
}

#[test]
fn latest_known_and_inspect_use_semver_order_and_accept_feature_keys() -> TestResult {
    let mut newer = version("1.10.0");
    newer.yanked = true;
    newer.rust_version = Some("1.85".to_owned());
    newer.license = Some("MIT".to_owned());
    // These are feature KEYS; dependency-feature expressions such as dep:foo
    // belong to feature values and are deliberately not represented here.
    newer.features = vec!["c++".to_owned(), "simd-fast".to_owned(), "std".to_owned()];
    let older = version("1.9.0");
    let krate = CrateRecord {
        name: "semver_case".to_owned(),
        description: "semver discriminating ordering".to_owned(),
        repository: None,
        updated_at: None,
        versions: vec![newer.clone(), older.clone()],
    };
    let built = SqliteCatalogRepository::build(1, provenance()?, &[krate])?;
    let repository = open(&built)?;
    let found = repository
        .inspect("semver_case")?
        .ok_or("crate disappeared")?;
    assert_eq!(found.versions, vec![older, newer]);
    assert_eq!(
        repository.lexical(&query("discriminating", 1)?)?,
        vec![CrateSummary {
            name: "semver_case".to_owned(),
            description: "semver discriminating ordering".to_owned(),
            latest_known: KnownVersion {
                version: "1.10.0".to_owned(),
                yanked: true,
                rust_version: Some("1.85".to_owned()),
                license: Some("MIT".to_owned())
            },
            version_count: 2,
        }]
    );
    Ok(())
}

#[test]
fn realistic_capacity_build_open_rebuild_and_bounded_search() -> TestResult {
    let mut crates = Vec::with_capacity(1000);
    for number in 0..1000 {
        let mut versions = Vec::with_capacity(10);
        for minor in 0..10 {
            let mut entry = version(&format!("1.{minor}.0"));
            entry.features = vec!["default".to_owned(), "std".to_owned()];
            entry.dependencies = vec![
                DependencyRecord {
                    name: "build_dep".to_owned(),
                    requirement: "^1".to_owned(),
                    kind: DependencyKind::Build,
                    optional: false,
                },
                DependencyRecord {
                    name: "normal_dep".to_owned(),
                    requirement: "^2".to_owned(),
                    kind: DependencyKind::Normal,
                    optional: true,
                },
                DependencyRecord {
                    name: "test_dep".to_owned(),
                    requirement: "^3".to_owned(),
                    kind: DependencyKind::Dev,
                    optional: false,
                },
            ];
            versions.push(entry);
        }
        crates.push(CrateRecord {
            name: format!("capacity_{number:04}"),
            description: "capacity shared indexed".to_owned(),
            repository: None,
            updated_at: Some(999),
            versions,
        });
    }
    // 1000 * 10 * (version + 2 features + 3 dependencies) = 60,000 entries.
    let built = SqliteCatalogRepository::build(1, provenance()?, &crates)?;
    assert!(built.bytes.len() <= MAX_SNAPSHOT_BYTES);
    let repository = open(&built)?;
    let rows = repository.lexical(&query("capacity", 50)?)?;
    assert_eq!(rows.len(), 50);
    assert!(
        rows.iter()
            .all(|row| row.version_count == 10 && row.latest_known.version == "1.9.0")
    );
    assert_eq!(repository.lexical(&query("capacity", 1)?)?.len(), 1);
    assert_eq!(
        repository.inspect("capacity_0999")?,
        Some(crates[999].clone())
    );
    let page = search_catalog(
        &repository,
        &query("capacity", 50)?,
        FreshnessPolicy::new("capacity-policy".parse()?, 60, 120)?,
        &AdvancingClock(Cell::new(1050)),
    )?;
    assert_eq!(page.crates.len(), 50);
    let serialized = serde_json::to_value(&page)?;
    let items = serialized["crates"]
        .as_array()
        .ok_or("missing search summaries")?;
    assert!(
        items
            .iter()
            .all(|item| item.get("versions").is_none() && item.get("latest_known").is_some())
    );
    let rebuilt = repository.rebuild(2)?;
    let reopened = open(&rebuilt)?;
    assert_eq!(reopened.metadata().sequence, 2);
    assert_eq!(reopened.inspect("capacity_0000")?, Some(crates[0].clone()));
    assert_eq!(
        reopened.inspect("capacity_0999")?,
        Some(crates[999].clone())
    );
    assert_eq!(reopened.lexical(&query("capacity", 50)?)?, rows);
    Ok(())
}

#[test]
fn empty_catalog_is_a_valid_searchable_and_rebuildable_snapshot() -> TestResult {
    let built = SqliteCatalogRepository::build(1, provenance()?, &[])?;
    let repository = open(&built)?;
    assert!(repository.lexical(&query("anything", 50)?)?.is_empty());
    assert_eq!(repository.inspect("anything")?, None);
    let rebuilt = repository.rebuild(2)?;
    let reopened = open(&rebuilt)?;
    assert!(reopened.lexical(&query("anything", 1)?)?.is_empty());
    assert_eq!(reopened.metadata().sequence, 2);
    Ok(())
}
