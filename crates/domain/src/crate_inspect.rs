//! Paged recorded facts; missing schema coverage is explicit, never inferred.
use crate::{CatalogError, CatalogFingerprint, DependencyRecord, KnownVersion, SnapshotEvidence};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectSection {
    #[default]
    Overview,
    Versions,
    Features,
    Dependencies,
    Advisories,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CrateInspectRequest {
    pub name: String,
    pub section: InspectSection,
    pub version: Option<String>,
    pub limit: u32,
    pub offset: u32,
    pub snapshot_fingerprint: Option<CatalogFingerprint>,
}
impl CrateInspectRequest {
    /// Validates shape and budgets. Exact SemVer syntax belongs to the adapter
    /// using its pinned parser, before it queries the authoritative snapshot.
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.name.is_empty()
            || self.name.len() > 64
            || !self
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || !(1..=50).contains(&self.limit)
            || self.offset > 128
            || (self.offset > 0 && self.snapshot_fingerprint.is_none())
            || self
                .version
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 128 || v.chars().any(char::is_control))
        {
            return Err(CatalogError::InvalidInput);
        }
        match self.section {
            InspectSection::Overview if self.offset != 0 => Err(CatalogError::InvalidInput),
            InspectSection::Versions if self.version.is_some() => Err(CatalogError::InvalidInput),
            InspectSection::Features
            | InspectSection::Dependencies
            | InspectSection::Advisories
                if self.version.is_none() =>
            {
                Err(CatalogError::InvalidInput)
            }
            _ => Ok(()),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectUnknownReason {
    NotRecordedInSnapshot,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum InspectUnknown {
    Unknown { reason: InspectUnknownReason },
}
impl Default for InspectUnknown {
    fn default() -> Self {
        Self::Unknown {
            reason: InspectUnknownReason::NotRecordedInSnapshot,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectOverview {
    pub name: String,
    pub description: String,
    pub repository: Option<String>,
    pub updated_at: Option<u64>,
    pub latest_known_stable: Option<KnownVersion>,
    pub version_count: u32,
    pub documentation: InspectUnknown,
    pub source: InspectUnknown,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectVersion {
    pub version: String,
    pub yanked: bool,
    pub rust_version: Option<String>,
    pub license: Option<String>,
    pub published_at: Option<u64>,
    pub feature_count: u32,
    pub dependency_count: u32,
    pub advisory_count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "section", rename_all = "snake_case", deny_unknown_fields)]
pub enum InspectPageData {
    Overview {
        selected_version: Option<InspectVersion>,
    },
    Versions {
        items: Vec<InspectVersion>,
    },
    Features {
        version: InspectVersion,
        items: Vec<String>,
    },
    Dependencies {
        version: InspectVersion,
        items: Vec<DependencyRecord>,
    },
    Advisories {
        version: InspectVersion,
        items: Vec<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectPagination {
    pub offset: u32,
    pub total: u32,
    pub returned: u32,
    pub next_offset: Option<u32>,
    pub omitted_by_output: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InspectPage {
    pub overview: InspectOverview,
    pub data: InspectPageData,
    pub pagination: InspectPagination,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InspectLookup {
    CrateNotFound,
    VersionNotFound,
    Found { page: Box<InspectPage> },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CrateInspectResult {
    pub name: String,
    pub snapshot_fingerprint: CatalogFingerprint,
    pub sequence: u64,
    pub evidence: SnapshotEvidence,
    pub lookup: InspectLookup,
}
