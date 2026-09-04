//! Wire-schema mirrors only; domain types own serialization and semantic validation.
//! Keep nullable fields present: the output contract generates in `for_serialize` mode.
#![expect(
    dead_code,
    reason = "Schema-only mirrors are never instantiated; domain values serialize the response"
)]

use schemars::JsonSchema;

type Fingerprint = super::super::toolchain::schemas::Fingerprint;

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct CatalogContextStatus {
    pub catalog: Component<CatalogContextCatalogStatus>,
    pub reservation: Option<CatalogReservationStatus>,
    pub model: Component<CatalogModelStatus>,
    pub semantic_index: Component<CatalogIndexObservation>,
    pub rustsec: Component<CatalogRustsecStatus>,
}

#[derive(JsonSchema)]
#[schemars(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Component<T> {
    Available { value: T },
    Unavailable { reason: CatalogComponentUnavailable },
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
pub(super) struct CatalogContextCatalogStatus {
    pub publisher: String,
    pub channel: String,
    pub publisher_key_fingerprint: Fingerprint,
    pub bundle_fingerprint: Fingerprint,
    pub sequence: u64,
    pub fingerprint: Fingerprint,
    pub schema_version: u32,
    pub crate_count: u32,
    pub bundled_rustsec_available: bool,
    pub evidence: SnapshotEvidence,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct CatalogReservationStatus {
    pub reservation: CatalogReservation,
    pub pending: bool,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct CatalogReservation {
    pub publisher: String,
    pub channel: String,
    pub sequence: u64,
    pub bundle_fingerprint: Fingerprint,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct CatalogModelStatus {
    pub identity: EmbeddingIdentity,
    pub evidence: SnapshotEvidence,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct CatalogIndexObservation {
    pub metadata: IndexMetadata,
    pub documents: u32,
}

#[derive(JsonSchema)]
#[schemars(deny_unknown_fields)]
pub(super) struct CatalogRustsecStatus {
    pub fingerprint: Fingerprint,
    pub sequence: u64,
    pub record_count: u32,
    pub evidence: SnapshotEvidence,
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
