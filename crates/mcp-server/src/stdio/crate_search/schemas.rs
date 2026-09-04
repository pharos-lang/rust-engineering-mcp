//! Search wire-schema mirrors; domain types alone serialize query results.
//! Output contracts use `for_serialize` so Option fields are present and nullable.
#![expect(
    dead_code,
    reason = "Schema-only mirrors are never instantiated; domain values serialize the response"
)]

use schemars::JsonSchema;

type Fingerprint = super::super::toolchain::schemas::Fingerprint;

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct CrateSearchResult {
    pub requested_mode: CrateSearchMode,
    pub effective_mode: CrateSearchMode,
    pub fallback: Option<SearchFallback>,
    pub snapshot_fingerprint: Fingerprint,
    pub evidence: SnapshotEvidence,
    pub semantic_index: Option<IndexMetadata>,
    pub model_evidence: Option<SnapshotEvidence>,
    pub results: Vec<RankedCrate>,
    pub window: SearchWindow,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum CrateSearchMode {
    Lexical,
    Semantic,
    Hybrid,
}

#[derive(JsonSchema)]
#[schemars(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum SearchFallback {
    Unavailable {
        component: SearchComponent,
        reason: CatalogComponentUnavailable,
    },
    Failed {
        reason: SemanticError,
    },
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum SearchComponent {
    Model,
    SemanticIndex,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum SemanticError {
    MissingModel,
    MissingIndex,
    InvalidArtifact,
    InvalidIndex,
    IdentityMismatch,
    InvalidInput,
    Budget,
    Inference,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct RankedCrate {
    pub facts: SearchCrateFacts,
    pub lexical: Option<LexicalScore>,
    pub semantic: Option<SemanticScore>,
    pub fusion_score: Option<f64>,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct LexicalScore {
    pub rank: u32,
    pub bm25: f64,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct SemanticScore {
    pub rank: u32,
    pub squared_l2: f32,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct SearchCrateFacts {
    pub name: String,
    pub description: String,
    pub repository: Option<String>,
    pub latest_known_stable: Option<KnownVersion>,
    pub selected_version: SearchVersionFacts,
    pub version_count: u32,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct KnownVersion {
    pub version: String,
    pub yanked: bool,
    pub rust_version: Option<String>,
    pub license: Option<String>,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct SearchVersionFacts {
    pub version: String,
    pub yanked: bool,
    pub rust_version: Option<String>,
    pub license: Option<String>,
    pub published_at: Option<u64>,
    pub known_advisory_ids: Vec<String>,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct SearchWindow {
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

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum CatalogComponentUnavailable {
    NotConfigured,
    Missing,
    Invalid,
    IdentityMismatch,
    UnsupportedPlatform,
    FeatureDisabled,
    Denied,
    IoUnavailable,
    Budget,
    DependencyUnavailable,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct IndexMetadata {
    pub schema_version: u32,
    pub snapshot_fingerprint: Fingerprint,
    pub model: EmbeddingIdentity,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct EmbeddingIdentity {
    pub model: String,
    pub revision: String,
    pub artifact_fingerprint: Fingerprint,
    pub runtime: String,
    pub provenance: Provenance,
    pub dimension: u32,
    pub max_tokens: u32,
    pub intra_threads: u16,
    pub pooling: PoolingKind,
    pub normalization: Normalization,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum PoolingKind {
    Mean,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum Normalization {
    L2,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct SnapshotEvidence {
    pub provenance: Provenance,
    pub freshness: Freshness,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct Provenance {
    pub source_kind: SourceKind,
    #[schemars(length(min = 1))]
    pub source_id: String,
    pub created_at: Option<u64>,
    pub observed_at: Option<u64>,
    pub integrity: IntegrityStatus,
    pub network_used: bool,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum SourceKind {
    RegistrySnapshot,
    ProjectSnapshot,
    RustsecSnapshot,
    EmbeddingModel,
    Artifact,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum IntegrityStatus {
    Verified,
    Unverified,
    Failed,
    Unknown,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct Freshness {
    pub state: FreshnessState,
    pub age_seconds: Option<u64>,
    pub assessed_at: u64,
    pub policy: FreshnessPolicy,
}

#[derive(JsonSchema)]
#[schemars(rename_all = "snake_case")]
pub(super) enum FreshnessState {
    Live,
    Fresh,
    Aging,
    Stale,
    Unknown,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct FreshnessPolicy {
    #[schemars(length(min = 1))]
    pub id: String,
    pub fresh_for_seconds: u64,
    pub stale_after_seconds: u64,
}
