//! Typed staging observations are not proof of authority or verified storage.
//! Adapters authenticate/load components; application validates their consistency.
use crate::{
    CatalogFingerprint, CatalogMetadata, EmbeddingIdentity, IndexMetadata, Provenance,
    SnapshotEvidence,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogComponentUnavailable {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum Component<T> {
    Available { value: T },
    Unavailable { reason: CatalogComponentUnavailable },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogContextCatalogObservation {
    pub publisher: String,
    pub channel: String,
    pub publisher_key_fingerprint: CatalogFingerprint,
    pub bundle_fingerprint: CatalogFingerprint,
    pub metadata: CatalogMetadata,
    pub schema_version: u32,
    pub crate_count: u32,
    pub bundled_rustsec_available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogReservation {
    pub publisher: String,
    pub channel: String,
    pub sequence: u64,
    pub bundle_fingerprint: CatalogFingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogIndexObservation {
    pub metadata: IndexMetadata,
    pub documents: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogRustsecObservation {
    pub fingerprint: CatalogFingerprint,
    pub sequence: u64,
    pub record_count: u32,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogContextObservation {
    pub catalog: Component<CatalogContextCatalogObservation>,
    pub reservation: Option<CatalogReservation>,
    pub model: Component<EmbeddingIdentity>,
    pub semantic_index: Component<CatalogIndexObservation>,
    pub rustsec: Component<CatalogRustsecObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogContextCatalogStatus {
    pub publisher: String,
    pub channel: String,
    pub publisher_key_fingerprint: CatalogFingerprint,
    pub bundle_fingerprint: CatalogFingerprint,
    pub sequence: u64,
    pub fingerprint: CatalogFingerprint,
    pub schema_version: u32,
    pub crate_count: u32,
    pub bundled_rustsec_available: bool,
    pub evidence: SnapshotEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogModelStatus {
    pub identity: EmbeddingIdentity,
    pub evidence: SnapshotEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogRustsecStatus {
    pub fingerprint: CatalogFingerprint,
    pub sequence: u64,
    pub record_count: u32,
    pub evidence: SnapshotEvidence,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogReservationStatus {
    pub reservation: CatalogReservation,
    /// True when no verified active generation matches the reserved generation.
    pub pending: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogContextStatus {
    pub catalog: Component<CatalogContextCatalogStatus>,
    pub reservation: Option<CatalogReservationStatus>,
    pub model: Component<CatalogModelStatus>,
    pub semantic_index: Component<CatalogIndexObservation>,
    pub rustsec: Component<CatalogRustsecStatus>,
}
