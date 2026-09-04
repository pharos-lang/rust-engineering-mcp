//! Snapshot facts. Public record fields are staging input, never authorization.
use crate::{CatalogFingerprint, Provenance, SnapshotEvidence};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogError {
    InvalidInput,
    InvalidSnapshot,
    UnsupportedSchema,
    Integrity,
    Rollback,
    Unavailable,
    Budget,
}
impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "catalog operation rejected: {self:?}")
    }
}
impl std::error::Error for CatalogError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DependencyRecord {
    pub name: String,
    pub requirement: String,
    pub kind: DependencyKind,
    pub optional: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DependencyKind {
    Normal,
    Build,
    Dev,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionRecord {
    pub version: String,
    pub yanked: bool,
    pub rust_version: Option<String>,
    pub license: Option<String>,
    pub published_at: Option<u64>,
    pub features: Vec<String>,
    pub dependencies: Vec<DependencyRecord>,
    pub advisories: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateRecord {
    pub name: String,
    pub description: String,
    pub repository: Option<String>,
    pub updated_at: Option<u64>,
    pub versions: Vec<VersionRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogMetadata {
    pub sequence: u64,
    pub fingerprint: CatalogFingerprint,
    pub provenance: Provenance,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogPage {
    pub snapshot_fingerprint: CatalogFingerprint,
    pub crates: Vec<CrateSummary>,
    pub evidence: SnapshotEvidence,
}

#[derive(Clone, Debug)]
pub struct CatalogQuery {
    text: String,
    limit: u32,
}
impl CatalogQuery {
    pub fn new(text: String, limit: u32) -> Result<Self, CatalogError> {
        if text.trim().is_empty()
            || text.len() > 256
            || text.chars().any(char::is_control)
            || text.split_whitespace().count() > 16
            || !(1..=50).contains(&limit)
        {
            return Err(CatalogError::InvalidInput);
        }
        Ok(Self { text, limit })
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn limit(&self) -> u32 {
        self.limit
    }
}

/// Compact lexical/semantic candidate facts, always rehydrated from SQLite.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateSummary {
    pub name: String,
    pub description: String,
    pub latest_known: KnownVersion,
    pub version_count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnownVersion {
    pub version: String,
    pub yanked: bool,
    pub rust_version: Option<String>,
    pub license: Option<String>,
}
