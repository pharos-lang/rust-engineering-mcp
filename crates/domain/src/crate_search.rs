//! Bounded retrieval evidence; ranking never establishes catalog facts or quality.
use crate::*;
use serde::{Deserialize, Serialize};

pub const SEARCH_CHANNEL_LIMIT: u32 = 50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrateSearchMode {
    Lexical,
    Semantic,
    #[default]
    Hybrid,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct MsrvVersion(u64, u64, u64);
impl MsrvVersion {
    pub fn parse(value: &str) -> Result<Self, CatalogError> {
        if value.len() > 32 {
            return Err(CatalogError::InvalidInput);
        }
        let parts = value.split('.').collect::<Vec<_>>();
        if !(2..=3).contains(&parts.len()) {
            return Err(CatalogError::InvalidInput);
        }
        let parse = |part: &str| {
            if part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                return Err(CatalogError::InvalidInput);
            }
            part.parse::<u64>().map_err(|_| CatalogError::InvalidInput)
        };
        Ok(Self(
            parse(parts[0])?,
            parse(parts[1])?,
            if parts.len() == 3 {
                parse(parts[2])?
            } else {
                0
            },
        ))
    }
    pub fn components(&self) -> (u64, u64, u64) {
        (self.0, self.1, self.2)
    }
}
impl TryFrom<String> for MsrvVersion {
    type Error = CatalogError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}
impl std::fmt::Display for MsrvVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}
impl Serialize for MsrvVersion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CrateSearchFilters {
    #[serde(default)]
    pub msrv_lte: Option<MsrvVersion>,
    #[serde(default)]
    pub allow_yanked: bool,
    #[serde(default)]
    pub include_prerelease: bool,
}
#[derive(Clone, Debug)]
pub struct CrateSearchRequest {
    pub query: CatalogQuery,
    pub mode: CrateSearchMode,
    pub filters: CrateSearchFilters,
}
#[derive(Clone, Debug, PartialEq)]
pub struct LexicalCandidate {
    pub name: String,
    pub bm25: f64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchVersionFacts {
    pub version: String,
    pub yanked: bool,
    pub rust_version: Option<String>,
    pub license: Option<String>,
    pub published_at: Option<u64>,
    pub known_advisory_ids: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchCrateFacts {
    pub name: String,
    pub description: String,
    pub repository: Option<String>,
    pub latest_known_stable: Option<KnownVersion>,
    pub selected_version: SearchVersionFacts,
    pub version_count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrateSelection {
    Missing,
    FilteredOut,
    Eligible(Box<SearchCrateFacts>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchComponent {
    Model,
    SemanticIndex,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SearchFallback {
    Unavailable {
        component: SearchComponent,
        reason: CatalogComponentUnavailable,
    },
    Failed {
        reason: SemanticError,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LexicalScore {
    pub rank: u32,
    pub bm25: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticScore {
    pub rank: u32,
    pub squared_l2: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RankedCrate {
    pub facts: SearchCrateFacts,
    pub lexical: Option<LexicalScore>,
    pub semantic: Option<SemanticScore>,
    pub fusion_score: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SearchWindow {
    pub candidate_limit_per_channel: u32,
    pub lexical_candidates: u32,
    pub semantic_candidates: u32,
    pub examined: u32,
    pub filtered_out: u32,
    pub eligible: u32,
    pub returned: u32,
    pub limit_truncated: u32,
    pub omitted_by_output: u32,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CrateSearchResult {
    pub requested_mode: CrateSearchMode,
    pub effective_mode: CrateSearchMode,
    pub fallback: Option<SearchFallback>,
    pub snapshot_fingerprint: CatalogFingerprint,
    pub evidence: SnapshotEvidence,
    pub semantic_index: Option<IndexMetadata>,
    pub model_evidence: Option<SnapshotEvidence>,
    pub results: Vec<RankedCrate>,
    pub window: SearchWindow,
}
