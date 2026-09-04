//! Host-expected owned RustSec records; SQLite selects authoritative advisory facts.
//! No RustSec filesystem loaders, Git/HTTP clients, subprocesses or runtime refresh.
mod lock;
#[cfg(test)]
mod tests;
use rusqlite::{Connection, params};
use rust_engineering_application::{InspectionControl, ProjectError};
use rust_engineering_domain::*;
use rustsec::{Advisory, Collection, database::Query};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const MAX_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECORDS: usize = 2048;
const MAX_MARKDOWN: usize = 64 * 1024;
const MAX_FINDINGS: usize = 128;
const MAX_PAYLOAD: usize = 256 * 1024;
const MAX_MATCHES: usize = 131_072;

/// Explicit host transport, not a signed distribution/import format.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustSecSnapshotDocument {
    pub format_version: u32,
    pub sequence: u64,
    pub source_id: String,
    #[serde(deserialize_with = "nullable")]
    pub created_at: Option<u64>,
    #[serde(deserialize_with = "nullable")]
    pub observed_at: Option<u64>,
    pub records: Vec<RustSecSnapshotRecord>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustSecSnapshotRecord {
    pub path: String,
    pub markdown: String,
}
fn nullable<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Option::deserialize(d)
}

pub struct RustSecSnapshot {
    connection: Connection,
    fingerprint: CatalogFingerprint,
    provenance: Provenance,
    record_count: u32,
    sequence: u64,
}
fn check(control: &dyn InspectionControl) -> Result<(), AuditDataError> {
    control.check().map_err(|error| match error {
        ProjectError::Cancelled => AuditDataError::Cancelled,
        ProjectError::Rejected(OperationalErrorCode::CommandTimeout) => AuditDataError::Timeout,
        _ => AuditDataError::Internal,
    })
}
fn sql(error: rusqlite::Error) -> AuditDataError {
    match super::sql(error) {
        CatalogError::Budget => AuditDataError::Budget,
        CatalogError::Unavailable => AuditDataError::Unavailable,
        _ => AuditDataError::InvalidSnapshot,
    }
}
fn valid_text(text: &str, max: usize) -> bool {
    !text.is_empty() && text.len() <= max && !text.chars().any(|c| c.is_control() || matches!(c, '\u{00ad}' | '\u{061c}' | '\u{200b}' | '\u{feff}' | '\u{2028}' | '\u{2029}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}
fn validate(record: RustSecSnapshotRecord) -> Result<Advisory, AuditDataError> {
    if record.path.len() > 256 || record.markdown.is_empty() || record.markdown.len() > MAX_MARKDOWN
    {
        return Err(AuditDataError::Budget);
    }
    let mut advisory: Advisory = record
        .markdown
        .parse()
        .map_err(|_| AuditDataError::InvalidSnapshot)?;
    let id = advisory.id().to_string();
    let fields: Vec<_> = id.split('-').collect();
    if fields.len() != 3
        || fields[0] != "RUSTSEC"
        || fields[1..]
            .iter()
            .any(|p| p.len() != 4 || !p.bytes().all(|b| b.is_ascii_digit()))
        || advisory.id().is_placeholder()
        || record.path != format!("crates/{}/{}.md", advisory.metadata.package, id)
        || advisory
            .metadata
            .collection
            .is_some_and(|c| c != Collection::Crates)
        || !valid_text(advisory.metadata.package.as_str(), 128)
        || !advisory
            .metadata
            .package
            .as_str()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        || advisory
            .metadata
            .informational
            .as_ref()
            .is_some_and(|i| !valid_text(&i.to_string(), 128))
        || !valid_text(advisory.title(), 512)
        || advisory.versions.patched().len() + advisory.versions.unaffected().len() > 64
        || advisory
            .versions
            .patched()
            .iter()
            .chain(advisory.versions.unaffected())
            .any(|v| v.to_string().len() > 256)
    {
        return Err(AuditDataError::InvalidSnapshot);
    }
    // RustSec's path loader normally fills this field. Owned-byte parsing does not.
    advisory.metadata.collection = Some(Collection::Crates);
    Ok(advisory)
}
impl RustSecSnapshot {
    pub fn catalog_metadata(&self) -> CatalogMetadata {
        CatalogMetadata {
            sequence: self.sequence,
            fingerprint: self.fingerprint.clone(),
            provenance: self.provenance.clone(),
        }
    }
    pub fn record_count(&self) -> u32 {
        self.record_count
    }
    pub fn from_bytes(
        bytes: &[u8],
        expected: &CatalogFingerprint,
        control: &dyn InspectionControl,
    ) -> Result<Self, AuditDataError> {
        check(control)?;
        if bytes.is_empty() || bytes.len() > MAX_BYTES {
            return Err(AuditDataError::Budget);
        }
        let fingerprint = super::fingerprint(bytes).map_err(|_| AuditDataError::Internal)?;
        if &fingerprint != expected {
            return Err(AuditDataError::Integrity);
        }
        let document: RustSecSnapshotDocument =
            serde_json::from_slice(bytes).map_err(|_| AuditDataError::InvalidSnapshot)?;
        if document.format_version != 1
            || document.sequence == 0
            || document.records.is_empty()
            || !valid_text(&document.source_id, 256)
        {
            return Err(AuditDataError::InvalidSnapshot);
        }
        if document.records.len() > MAX_RECORDS {
            return Err(AuditDataError::Budget);
        }
        let record_count = document.records.len() as u32;
        let sequence = document.sequence;
        let provenance = Provenance::new(
            SourceKind::RustsecSnapshot,
            document
                .source_id
                .parse()
                .map_err(|_| AuditDataError::InvalidSnapshot)?,
            document.created_at.map(UnixSeconds),
            document.observed_at.map(UnixSeconds),
            IntegrityStatus::Verified,
            false,
        )
        .map_err(|_| AuditDataError::InvalidSnapshot)?;
        let mut connection = super::empty().map_err(|_| AuditDataError::Unavailable)?;
        connection.execute_batch("PRAGMA max_page_count=4096; CREATE TABLE rustsec_advisories(id TEXT PRIMARY KEY, package TEXT NOT NULL, registry TEXT NOT NULL, advisory_json TEXT NOT NULL) STRICT; CREATE INDEX rustsec_package ON rustsec_advisories(package,registry);").map_err(sql)?;
        let transaction = connection.transaction().map_err(sql)?;
        let mut per_package = BTreeMap::<String, usize>::new();
        for record in document.records {
            check(control)?;
            let advisory = validate(record)?;
            let count = per_package
                .entry(advisory.metadata.package.to_string())
                .or_default();
            *count += 1;
            if *count > 128 {
                return Err(AuditDataError::Budget);
            }
            // cargo-lock marks explicit registry URLs as "locked"; absent advisory
            // sources use its canonical default with no precise marker. Accept these
            // two known representations, never SourceId's relaxed equality.
            let canonical = rustsec::SourceId::default();
            let registry = match advisory.metadata.source.as_ref() {
                None => "crates_io",
                Some(origin)
                    if origin.kind() == canonical.kind()
                        && origin.url() == canonical.url()
                        && origin.precise() == Some("locked") =>
                {
                    "crates_io"
                }
                Some(_) => return Err(AuditDataError::InvalidSnapshot),
            };
            let json =
                serde_json::to_string(&advisory).map_err(|_| AuditDataError::InvalidSnapshot)?;
            if json.len() > 192 * 1024 {
                return Err(AuditDataError::Budget);
            }
            transaction
                .execute(
                    "INSERT INTO rustsec_advisories VALUES(?1,?2,?3,?4)",
                    params![
                        advisory.id().to_string(),
                        advisory.metadata.package.as_str(),
                        registry,
                        json
                    ],
                )
                .map_err(sql)?;
        }
        transaction.commit().map_err(sql)?;
        connection
            .pragma_update(None, "query_only", true)
            .map_err(sql)?;
        check(control)?;
        Ok(Self {
            connection,
            fingerprint,
            provenance,
            record_count,
            sequence,
        })
    }
    pub fn audit(
        &self,
        source: &SourceBundle,
        structure: &ProjectStructure,
        clock: &dyn Clock,
        control: &dyn InspectionControl,
    ) -> Result<AuditObservation, AuditDataError> {
        check(control)?;
        super::budget(&self.connection).map_err(|_| AuditDataError::Unavailable)?;
        let graph = lock::parse(source, structure, control)?;
        struct Now(UnixSeconds);
        impl Clock for Now {
            fn now(&self) -> UnixSeconds {
                self.0
            }
        }
        let now = clock.now();
        let policy = FreshnessPolicy::new(
            "rustsec-host-snapshot-v1"
                .parse()
                .map_err(|_| AuditDataError::Internal)?,
            86_400,
            604_800,
        )
        .map_err(|_| AuditDataError::Internal)?;
        let evidence = SnapshotEvidence::assess(self.provenance.clone(), policy, &Now(now));
        let times_known = self.provenance.created_at().is_some()
            && self.provenance.observed_at().is_some_and(|t| t <= now);
        let issue = if !times_known || evidence.freshness().state() == FreshnessState::Unknown {
            Some(AuditIssue::SnapshotUnknownAge)
        } else if evidence.freshness().state() != FreshnessState::Fresh {
            Some(AuditIssue::SnapshotStale)
        } else if !graph.unsupported.is_empty() {
            Some(AuditIssue::UnsupportedSources)
        } else {
            None
        };
        let mut output = AuditObservation {
            state: AuditState::Passed,
            issue,
            validation_complete: issue.is_none(),
            lock_fingerprint: Some(graph.fingerprint.clone()),
            snapshot_fingerprint: Some(self.fingerprint.clone()),
            snapshot: Some(evidence),
            snapshot_record_count: Some(self.record_count),
            snapshot_sequence: Some(self.sequence),
            packages_total: graph.packages.len() as u32,
            crates_io_scanned: 0,
            workspace_packages_excluded: graph.roots.len() as u32,
            unsupported_packages: graph.unsupported.clone(),
            findings: vec![],
            informational: vec![],
            findings_omitted: 0,
        };
        let mut matches = 0;
        let mut payload_exhausted = false;
        let mut size = serde_json::to_vec(&output)
            .map_err(|_| AuditDataError::Internal)?
            .len();
        if size > MAX_PAYLOAD {
            return Err(AuditDataError::Budget);
        }
        for &index in &graph.scanned_indices {
            check(control)?;
            let package = &graph.lock.packages[index];
            let mut query = self.connection.prepare_cached("SELECT advisory_json FROM rustsec_advisories WHERE package=?1 AND registry='crates_io' ORDER BY id LIMIT 129").map_err(sql)?;
            let mut rows = query.query([package.name.as_str()]).map_err(sql)?;
            let mut cached_paths: Option<(Vec<AuditPath>, u64)> = None;
            while let Some(row) = rows.next().map_err(sql)? {
                check(control)?;
                matches += 1;
                if matches > MAX_MATCHES {
                    return Err(AuditDataError::Budget);
                }
                let json: String = row.get(0).map_err(sql)?;
                let advisory: Advisory =
                    serde_json::from_str(&json).map_err(|_| AuditDataError::InvalidSnapshot)?;
                // Evaluate both security and informational records; withdrawn is always excluded.
                let security = Query::crate_scope().package(package).matches(&advisory);
                let information = Query::crate_scope()
                    .informational(true)
                    .package(package)
                    .matches(&advisory);
                if !security && !information {
                    continue;
                }
                if payload_exhausted
                    || output.findings.len() + output.informational.len() >= MAX_FINDINGS
                {
                    output.findings_omitted += 1;
                    output.validation_complete = false;
                    output.issue = output.issue.or(Some(AuditIssue::OutputBudget));
                    continue;
                }
                let (paths, paths_omitted) = match &cached_paths {
                    Some(paths) => paths.clone(),
                    None => {
                        let paths = graph.paths(index, control)?;
                        cached_paths = Some(paths.clone());
                        paths
                    }
                };
                if paths_omitted > 0 {
                    output.validation_complete = false;
                    output.issue = output.issue.or(Some(AuditIssue::OutputBudget));
                }
                let finding = AuditFinding {
                    advisory_id: advisory.id().to_string(),
                    url: format!("https://rustsec.org/advisories/{}.html", advisory.id()),
                    title: advisory.title().into(),
                    package: graph.packages[index].clone(),
                    patched_requirements: advisory
                        .versions
                        .patched()
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    unaffected_requirements: advisory
                        .versions
                        .unaffected()
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                    severity: advisory.severity().map(|s| match s {
                        rustsec::advisory::Severity::None => AuditSeverity::None,
                        rustsec::advisory::Severity::Low => AuditSeverity::Low,
                        rustsec::advisory::Severity::Medium => AuditSeverity::Medium,
                        rustsec::advisory::Severity::High => AuditSeverity::High,
                        rustsec::advisory::Severity::Critical => AuditSeverity::Critical,
                    }),
                    informational: advisory
                        .metadata
                        .informational
                        .as_ref()
                        .map(ToString::to_string),
                    paths,
                    paths_omitted,
                };
                let bytes = serde_json::to_vec(&finding)
                    .map_err(|_| AuditDataError::Internal)?
                    .len()
                    + 1;
                if output.findings.len() + output.informational.len() >= MAX_FINDINGS
                    || size + bytes > MAX_PAYLOAD
                {
                    payload_exhausted = true;
                    output.findings_omitted += 1;
                    output.validation_complete = false;
                    output.issue = output.issue.or(Some(AuditIssue::OutputBudget));
                    continue;
                }
                size += bytes;
                if security {
                    output.findings.push(finding);
                } else {
                    output.informational.push(finding);
                }
            }
            output.crates_io_scanned += 1;
        }
        output.state = if matches!(
            output.issue,
            Some(AuditIssue::SnapshotStale | AuditIssue::SnapshotUnknownAge)
        ) {
            AuditState::Unavailable
        } else if !output.findings.is_empty() {
            AuditState::Failed
        } else if !output.validation_complete {
            AuditState::Incomplete
        } else {
            AuditState::Passed
        };
        if serde_json::to_vec(&output)
            .map_err(|_| AuditDataError::Internal)?
            .len()
            > MAX_PAYLOAD
        {
            return Err(AuditDataError::Budget);
        }
        check(control)?;
        Ok(output)
    }
}
