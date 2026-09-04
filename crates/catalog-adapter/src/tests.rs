use super::*;
fn fixture() -> Result<Snapshot, Box<dyn std::error::Error>> {
    let provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "fixture".parse()?,
        Some(UnixSeconds(100)),
        Some(UnixSeconds(100)),
        IntegrityStatus::Verified,
        false,
    )?;
    Ok(SqliteCatalogRepository::build(
        1,
        provenance,
        &[CrateRecord {
            name: "serde".into(),
            description: "serialization".into(),
            repository: None,
            updated_at: Some(100),
            versions: vec![VersionRecord {
                version: "1.0.0".into(),
                yanked: false,
                rust_version: Some("1.98".into()),
                license: Some("MIT".into()),
                published_at: Some(100),
                features: vec!["derive".into()],
                dependencies: vec![],
                advisories: vec![],
            }],
        }],
    )?)
}
fn altered(statement: &str) -> Result<Snapshot, Box<dyn std::error::Error>> {
    let snapshot = fixture()?;
    let mut conn = Connection::open_in_memory()?;
    conn.deserialize_read_exact(
        MAIN_DB,
        snapshot.bytes.as_slice(),
        snapshot.bytes.len(),
        false,
    )?;
    conn.execute_batch(statement)?;
    let bytes = conn.serialize(MAIN_DB)?.to_vec();
    let mut manifest = snapshot.manifest;
    manifest.byte_length = bytes.len() as u64;
    manifest.fingerprint = fingerprint(&bytes)?;
    Ok(Snapshot { manifest, bytes })
}

#[test]
fn migration_is_atomic_idempotent_and_rejects_unknown_schema()
-> Result<(), Box<dyn std::error::Error>> {
    let mut conn = empty()?;
    assert!(apply_v1_migration(&mut conn, &format!("{SCHEMA}\nINVALID SQL")).is_err());
    assert!(schema_rows(&conn)?.is_empty());
    assert_eq!(
        conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0))?,
        0
    );
    migrate(&mut conn)?;
    let before = schema_rows(&conn)?;
    migrate(&mut conn)?;
    assert_eq!(before, schema_rows(&conn)?);
    conn.pragma_update(None, "user_version", 2)?;
    assert_eq!(migrate(&mut conn), Err(CatalogError::UnsupportedSchema));
    Ok(())
}

#[test]
fn rehashed_hostile_images_fail_validation_beyond_digest() -> Result<(), Box<dyn std::error::Error>>
{
    for statement in [
        "CREATE TABLE surprise(x TEXT)",
        "CREATE VIEW surprise AS SELECT * FROM crates",
        "CREATE TRIGGER surprise AFTER INSERT ON crates BEGIN DELETE FROM versions; END",
        "INSERT INTO migrations VALUES(2,'unexpected')",
        "DELETE FROM snapshots",
        "UPDATE migrations SET checksum='sha256:wrong'",
        "PRAGMA user_version=99",
        "PRAGMA foreign_keys=OFF; INSERT INTO features VALUES(999,'dangling')",
        "UPDATE crates SET description='unindexed changed description'",
        "UPDATE versions SET version='not-semver'",
        "UPDATE crates SET updated_at=-1",
        "UPDATE snapshots SET provenance='{}'",
    ] {
        let snapshot = altered(statement)?;
        assert!(
            SqliteCatalogRepository::open(&snapshot.bytes, &snapshot.manifest).is_err(),
            "accepted {statement}"
        );
    }
    Ok(())
}

#[test]
fn runtime_read_only_and_attach_disabled_and_real_fts() -> Result<(), Box<dyn std::error::Error>> {
    let snapshot = fixture()?;
    let repository = SqliteCatalogRepository::open(&snapshot.bytes, &snapshot.manifest)?;
    assert!(
        repository
            .connection
            .execute("DELETE FROM crates", [])
            .is_err()
    );
    assert!(
        repository
            .connection
            .execute_batch("ATTACH ':memory:' AS extra")
            .is_err()
    );
    assert_eq!(
        repository
            .connection
            .pragma_query_value(None, "temp_store", |r| r.get::<_, u32>(0))?,
        2
    );
    assert_eq!(
        repository.lexical(&CatalogQuery::new("serialization".into(), 1)?)?[0].name,
        "serde"
    );
    assert!(
        repository
            .connection
            .db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE)?
    );
    assert!(
        !repository
            .connection
            .db_config(DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA)?
    );
    Ok(())
}

#[test]
fn sqlite_progress_handler_interrupts_expensive_query() -> Result<(), Box<dyn std::error::Error>> {
    let conn = empty()?;
    budget(&conn)?;
    let error = conn.query_row("WITH RECURSIVE x(v) AS (VALUES(0) UNION ALL SELECT v+1 FROM x WHERE v<10000000) SELECT sum(v) FROM x",[],|r|r.get::<_,i64>(0)).err().ok_or("query was not interrupted")?;
    assert_eq!(sql(error), CatalogError::Budget);
    Ok(())
}

#[test]
fn search_payload_is_bounded_and_import_rejects_trailing_pages()
-> Result<(), Box<dyn std::error::Error>> {
    let seed = fixture()?;
    let repo = SqliteCatalogRepository::open(&seed.bytes, &seed.manifest)?;
    let template = repo.inspect("serde")?.ok_or("missing fixture")?;
    let records = (0..50)
        .map(|index| {
            let mut record = template.clone();
            record.name = format!("crate_{index}");
            record.description = format!("search {}", "x".repeat(4000));
            record
        })
        .collect::<Vec<_>>();
    let snapshot = SqliteCatalogRepository::build(2, repo.metadata.provenance.clone(), &records)?;
    let active = SqliteCatalogRepository::open(&snapshot.bytes, &snapshot.manifest)?;
    assert_eq!(
        active.lexical(&CatalogQuery::new("search".into(), 50)?),
        Err(CatalogError::Budget)
    );
    assert_eq!(
        active
            .lexical(&CatalogQuery::new("search".into(), 1)?)?
            .len(),
        1
    );
    assert_eq!(
        active
            .connection
            .pragma_query_value(None, "max_page_count", |r| r.get::<_, u32>(0))?,
        16384
    );
    let mut extra = seed.bytes.clone();
    extra.extend_from_slice(&[0; 4096]);
    let mut manifest = seed.manifest;
    manifest.byte_length = extra.len() as u64;
    manifest.fingerprint = fingerprint(&extra)?;
    assert!(SqliteCatalogRepository::open(&extra, &manifest).is_err());
    Ok(())
}
