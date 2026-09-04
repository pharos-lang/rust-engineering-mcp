//! Authoritative SQLite snapshots; no paths, network, extension loading or caller SQL.
pub mod bundle;
mod inspect;
mod records;
mod search;
use rusqlite::{Connection, MAIN_DB, OptionalExtension, config::DbConfig, limits::Limit, params};
use rust_engineering_application::CatalogRepository;
use rust_engineering_domain::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};

const SCHEMA: &str = include_str!("schema.sql");
pub const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SEARCH_PAYLOAD_BYTES: usize = 128 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// Must come from a trusted host channel. Deserialization does not authenticate
/// a publisher; signatures and trust roots are the responsibility of M1 import.
pub struct SnapshotManifest {
    pub format_version: u32,
    pub sequence: u64,
    pub byte_length: u64,
    pub fingerprint: CatalogFingerprint,
}
/// Integrity relative to the host's expected manifest, not a publisher signature.
pub struct Snapshot {
    pub manifest: SnapshotManifest,
    pub bytes: Vec<u8>,
}

pub struct SqliteCatalogRepository {
    connection: Connection,
    metadata: CatalogMetadata,
}

fn sql(error: rusqlite::Error) -> CatalogError {
    if matches!(error, rusqlite::Error::QueryReturnedNoRows) {
        return CatalogError::InvalidSnapshot;
    }
    match error.sqlite_error_code() {
        Some(
            rusqlite::ErrorCode::OperationInterrupted
            | rusqlite::ErrorCode::DiskFull
            | rusqlite::ErrorCode::TooBig,
        ) => CatalogError::Budget,
        Some(
            rusqlite::ErrorCode::OutOfMemory
            | rusqlite::ErrorCode::CannotOpen
            | rusqlite::ErrorCode::SystemIoFailure
            | rusqlite::ErrorCode::DatabaseBusy
            | rusqlite::ErrorCode::DatabaseLocked,
        ) => CatalogError::Unavailable,
        _ => CatalogError::Integrity,
    }
}
fn fingerprint(bytes: &[u8]) -> Result<CatalogFingerprint, CatalogError> {
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
    .parse()
    .map_err(|_| CatalogError::Integrity)
}
fn budget(connection: &Connection) -> Result<(), CatalogError> {
    let started = Instant::now();
    let mut callbacks = 0_u64;
    connection
        .progress_handler(
            1000,
            Some(move || {
                callbacks += 1;
                callbacks > 10_000 || started.elapsed() > Duration::from_secs(30)
            }),
        )
        .map_err(sql)
}
fn empty() -> Result<Connection, CatalogError> {
    let connection = Connection::open_in_memory().map_err(sql)?;
    connection.set_prepared_statement_cache_capacity(16);
    for (setting, value) in [
        (DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true),
        (DbConfig::SQLITE_DBCONFIG_TRUSTED_SCHEMA, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_TRIGGER, false),
        (DbConfig::SQLITE_DBCONFIG_ENABLE_VIEW, false),
    ] {
        if connection.set_db_config(setting, value).map_err(sql)? != value {
            return Err(CatalogError::Unavailable);
        }
    }
    for (setting, value) in [
        (Limit::SQLITE_LIMIT_ATTACHED, 0),
        (Limit::SQLITE_LIMIT_LENGTH, 1_048_576),
        (Limit::SQLITE_LIMIT_SQL_LENGTH, 32768),
        (Limit::SQLITE_LIMIT_COLUMN, 64),
        (Limit::SQLITE_LIMIT_EXPR_DEPTH, 64),
        (Limit::SQLITE_LIMIT_VARIABLE_NUMBER, 64),
    ] {
        connection.set_limit(setting, value).map_err(sql)?;
        if connection.limit(setting).map_err(sql)? > value {
            return Err(CatalogError::Unavailable);
        }
    }
    connection.execute_batch("PRAGMA temp_store=MEMORY; PRAGMA foreign_keys=ON; PRAGMA page_size=4096; PRAGMA max_page_count=16384;").map_err(sql)?;
    budget(&connection)?;
    Ok(connection)
}

fn migrate(connection: &mut Connection) -> Result<(), CatalogError> {
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sql)?;
    if version > 1 {
        return Err(CatalogError::UnsupportedSchema);
    }
    if version == 0 {
        let count: i64 = connection
            .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
            .map_err(sql)?;
        if count != 0 {
            return Err(CatalogError::InvalidSnapshot);
        }
        apply_v1_migration(connection, SCHEMA)?;
    }
    validate_ledger(connection)
}
fn apply_v1_migration(connection: &mut Connection, migration: &str) -> Result<(), CatalogError> {
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(sql)?;
    transaction.execute_batch(migration).map_err(sql)?;
    transaction
        .execute(
            "INSERT INTO migrations VALUES(1,?1)",
            [fingerprint(migration.as_bytes())?.to_string()],
        )
        .map_err(sql)?;
    transaction
        .pragma_update(None, "user_version", 1)
        .map_err(sql)?;
    transaction.commit().map_err(sql)
}
fn validate_ledger(connection: &Connection) -> Result<(), CatalogError> {
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sql)?;
    if version != 1 {
        return Err(CatalogError::UnsupportedSchema);
    }
    let entries: Vec<(u32, String)> = connection
        .prepare("SELECT version,checksum FROM migrations ORDER BY version")
        .map_err(sql)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    if entries != [(1, fingerprint(SCHEMA.as_bytes())?.to_string())] {
        return Err(CatalogError::InvalidSnapshot);
    }
    Ok(())
}
type SchemaRow = (String, String, String, Option<String>);

fn schema_rows(connection: &Connection) -> Result<Vec<SchemaRow>, CatalogError> {
    connection
        .prepare("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .map_err(sql)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)
}
fn validate_database(connection: &Connection) -> Result<(), CatalogError> {
    budget(connection)?;
    let integrity: Vec<String> = connection
        .prepare("PRAGMA integrity_check")
        .map_err(sql)?
        .query_map([], |r| r.get(0))
        .map_err(sql)?
        .collect::<Result<_, _>>()
        .map_err(sql)?;
    if integrity != ["ok"] {
        return Err(CatalogError::Integrity);
    }
    let version: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(sql)?;
    if version != 1 {
        return Err(CatalogError::UnsupportedSchema);
    }
    let mut expected = empty()?;
    migrate(&mut expected)?;
    if schema_rows(connection)? != schema_rows(&expected)? {
        return Err(CatalogError::InvalidSnapshot);
    }
    validate_ledger(connection)?;
    let foreign_key_failure = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(sql)?
        .query([])
        .map_err(sql)?
        .next()
        .map_err(sql)?
        .is_some();
    if foreign_key_failure {
        return Err(CatalogError::Integrity);
    }
    connection
        .execute(
            "INSERT INTO crate_fts(crate_fts,rank) VALUES('integrity-check',1)",
            [],
        )
        .map_err(sql)?;
    let records = records::all(connection)?;
    records::validate(&records)?;
    Ok(())
}

impl SqliteCatalogRepository {
    pub fn build(
        sequence: u64,
        provenance: Provenance,
        crates: &[CrateRecord],
    ) -> Result<Snapshot, CatalogError> {
        if sequence == 0
            || sequence > i64::MAX as u64
            || provenance.source_kind() != SourceKind::RegistrySnapshot
            || provenance.source_id().as_str().len() > 256
        {
            return Err(CatalogError::InvalidInput);
        }
        records::validate(crates)?;
        let mut connection = empty()?;
        migrate(&mut connection)?;
        budget(&connection)?;
        let transaction = connection.transaction().map_err(sql)?;
        transaction
            .execute(
                "INSERT INTO snapshots VALUES(1,?1,1,?2)",
                params![
                    i64::try_from(sequence).map_err(|_| CatalogError::InvalidInput)?,
                    serde_json::to_string(&provenance).map_err(|_| CatalogError::InvalidInput)?
                ],
            )
            .map_err(sql)?;
        records::insert(&transaction, crates)?;
        transaction
            .execute("INSERT INTO crate_fts(crate_fts) VALUES('rebuild')", [])
            .map_err(sql)?;
        transaction.commit().map_err(sql)?;
        validate_database(&connection)?;
        let data = connection.serialize(MAIN_DB).map_err(sql)?;
        if data.len() > MAX_SNAPSHOT_BYTES {
            return Err(CatalogError::Budget);
        }
        let bytes = data.to_vec();
        Ok(Snapshot {
            manifest: SnapshotManifest {
                format_version: 1,
                sequence,
                byte_length: bytes.len() as u64,
                fingerprint: fingerprint(&bytes)?,
            },
            bytes,
        })
    }

    /// Validates integrity relative to a trusted expected manifest, not authenticity.
    pub fn open(bytes: &[u8], expected: &SnapshotManifest) -> Result<Self, CatalogError> {
        if expected.format_version != 1 {
            return Err(CatalogError::UnsupportedSchema);
        }
        if bytes.len() > MAX_SNAPSHOT_BYTES {
            return Err(CatalogError::Budget);
        }
        if bytes.len() < 100
            || expected.byte_length != bytes.len() as u64
            || fingerprint(bytes)? != expected.fingerprint
            || &bytes[..16] != b"SQLite format 3\0"
            || bytes[18] != 1
            || bytes[19] != 1
        {
            return Err(CatalogError::Integrity);
        }
        let mut staging = empty()?;
        staging
            .deserialize_read_exact(MAIN_DB, bytes, bytes.len(), false)
            .map_err(sql)?;
        image_limits(&staging, bytes.len())?;
        validate_database(&staging)?;
        let (sequence, provenance): (i64, String) = staging
            .query_row(
                "SELECT sequence,provenance FROM snapshots WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(sql)?;
        let sequence = u64::try_from(sequence).map_err(|_| CatalogError::InvalidSnapshot)?;
        let provenance: Provenance =
            serde_json::from_str(&provenance).map_err(|_| CatalogError::InvalidSnapshot)?;
        if sequence == 0
            || sequence != expected.sequence
            || provenance.source_kind() != SourceKind::RegistrySnapshot
            || provenance.source_id().as_str().len() > 256
        {
            return Err(CatalogError::InvalidSnapshot);
        }
        drop(staging);
        let mut connection = empty()?;
        connection
            .deserialize_read_exact(MAIN_DB, bytes, bytes.len(), true)
            .map_err(sql)?;
        image_limits(&connection, bytes.len())?;
        connection
            .pragma_update(None, "query_only", true)
            .map_err(sql)?;
        Ok(Self {
            connection,
            metadata: CatalogMetadata {
                sequence,
                fingerprint: expected.fingerprint.clone(),
                provenance,
            },
        })
    }

    /// Bounded authoritative documents for explicit local index rebuild.
    pub fn embedding_documents(&self) -> Result<Vec<(String, String)>, CatalogError> {
        budget(&self.connection)?;
        Ok(records::all(&self.connection)?
            .into_iter()
            .map(|c| {
                let text = format!("{} {}", c.name, c.description)
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                (c.name, text)
            })
            .collect())
    }

    pub fn activate(
        &mut self,
        bytes: &[u8],
        expected: &SnapshotManifest,
    ) -> Result<(), CatalogError> {
        if expected.sequence <= self.metadata.sequence {
            return Err(CatalogError::Rollback);
        }
        let next = Self::open(bytes, expected)?;
        *self = next;
        Ok(())
    }

    /// Rebuild a new, independently verified snapshot generation from authoritative facts.
    pub fn rebuild(&self, next_sequence: u64) -> Result<Snapshot, CatalogError> {
        if next_sequence <= self.metadata.sequence {
            return Err(CatalogError::Rollback);
        }
        budget(&self.connection)?;
        Self::build(
            next_sequence,
            self.metadata.provenance.clone(),
            &records::all(&self.connection)?,
        )
    }
}
impl CatalogRepository for SqliteCatalogRepository {
    fn metadata(&self) -> &CatalogMetadata {
        &self.metadata
    }
    fn lexical(&self, query: &CatalogQuery) -> Result<Vec<CrateSummary>, CatalogError> {
        budget(&self.connection)?;
        let literal = query
            .text()
            .split_whitespace()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let ids: Vec<i64> = self.connection.prepare("SELECT rowid FROM crate_fts WHERE crate_fts MATCH ?1 ORDER BY rank, rowid LIMIT ?2").map_err(sql)?.query_map(params![literal,query.limit()],|r|r.get(0)).map_err(sql)?.collect::<Result<_,_>>().map_err(sql)?;
        ids.into_iter()
            .map(|id| records::summary(&self.connection, id))
            .collect::<Result<Vec<_>, _>>()
            .and_then(|summaries| {
                if serde_json::to_vec(&summaries)
                    .map_err(|_| CatalogError::Integrity)?
                    .len()
                    > MAX_SEARCH_PAYLOAD_BYTES
                {
                    return Err(CatalogError::Budget);
                }
                Ok(summaries)
            })
    }
    fn summary(&self, name: &str) -> Result<Option<CrateSummary>, CatalogError> {
        if !records::valid_name(name) {
            return Err(CatalogError::InvalidInput);
        }
        budget(&self.connection)?;
        let id = self
            .connection
            .query_row("SELECT id FROM crates WHERE name=?1", [name], |r| {
                r.get::<_, i64>(0)
            })
            .optional()
            .map_err(sql)?;
        id.map(|id| records::summary(&self.connection, id))
            .transpose()
    }
    fn inspect(&self, name: &str) -> Result<Option<CrateRecord>, CatalogError> {
        if !records::valid_name(name) {
            return Err(CatalogError::InvalidInput);
        }
        budget(&self.connection)?;
        let id = self
            .connection
            .query_row("SELECT id FROM crates WHERE name=?1", [name], |r| {
                r.get::<_, i64>(0)
            })
            .optional()
            .map_err(sql)?;
        id.map(|id| records::get(&self.connection, id)).transpose()
    }
}

#[cfg(test)]
mod tests;

fn image_limits(connection: &Connection, length: usize) -> Result<(), CatalogError> {
    let size: u32 = connection
        .pragma_query_value(None, "page_size", |r| r.get(0))
        .map_err(sql)?;
    let pages: u32 = connection
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .map_err(sql)?;
    if !(512..=65536).contains(&size)
        || !size.is_power_of_two()
        || u64::from(size) * u64::from(pages) != length as u64
    {
        return Err(CatalogError::Integrity);
    }
    let limit = MAX_SNAPSHOT_BYTES as u32 / size;
    connection
        .pragma_update(None, "max_page_count", limit)
        .map_err(sql)?;
    if connection
        .pragma_query_value(None, "max_page_count", |r| r.get::<_, u32>(0))
        .map_err(sql)?
        != limit
    {
        return Err(CatalogError::Unavailable);
    }
    Ok(())
}

mod audit;
pub use audit::{RustSecSnapshot, RustSecSnapshotDocument, RustSecSnapshotRecord};
