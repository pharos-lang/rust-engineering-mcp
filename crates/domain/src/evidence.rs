use serde::{Deserialize, Serialize};

use crate::{ContractError, NonEmptyText};

/// UTC seconds since the Unix epoch; no host clock or date parser is embedded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnixSeconds(pub u64);

/// The only effect boundary needed by freshness evaluation.
pub trait Clock {
    fn now(&self) -> UnixSeconds;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    RegistrySnapshot,
    ProjectSnapshot,
    RustsecSnapshot,
    EmbeddingModel,
    Artifact,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrityStatus {
    Verified,
    Unverified,
    Failed,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawProvenance")]
pub struct Provenance {
    source_kind: SourceKind,
    source_id: NonEmptyText,
    created_at: Option<UnixSeconds>,
    observed_at: Option<UnixSeconds>,
    integrity: IntegrityStatus,
    network_used: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvenance {
    source_kind: SourceKind,
    source_id: NonEmptyText,
    #[serde(deserialize_with = "crate::required_nullable")]
    created_at: Option<UnixSeconds>,
    #[serde(deserialize_with = "crate::required_nullable")]
    observed_at: Option<UnixSeconds>,
    integrity: IntegrityStatus,
    network_used: bool,
}

impl Provenance {
    pub fn new(
        source_kind: SourceKind,
        source_id: NonEmptyText,
        created_at: Option<UnixSeconds>,
        observed_at: Option<UnixSeconds>,
        integrity: IntegrityStatus,
        network_used: bool,
    ) -> Result<Self, ContractError> {
        if matches!((created_at, observed_at), (Some(created), Some(observed)) if observed < created)
        {
            return Err(ContractError::InvalidProvenance);
        }
        Ok(Self {
            source_kind,
            source_id,
            created_at,
            observed_at,
            integrity,
            network_used,
        })
    }

    pub fn source_kind(&self) -> SourceKind {
        self.source_kind
    }
    pub fn source_id(&self) -> &NonEmptyText {
        &self.source_id
    }
    pub fn created_at(&self) -> Option<UnixSeconds> {
        self.created_at
    }
    pub fn observed_at(&self) -> Option<UnixSeconds> {
        self.observed_at
    }
    pub fn integrity(&self) -> IntegrityStatus {
        self.integrity
    }
    pub fn network_used(&self) -> bool {
        self.network_used
    }
}

impl TryFrom<RawProvenance> for Provenance {
    type Error = ContractError;
    fn try_from(raw: RawProvenance) -> Result<Self, Self::Error> {
        Self::new(
            raw.source_kind,
            raw.source_id,
            raw.created_at,
            raw.observed_at,
            raw.integrity,
            raw.network_used,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawPolicy")]
pub struct FreshnessPolicy {
    id: NonEmptyText,
    fresh_for_seconds: u64,
    stale_after_seconds: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPolicy {
    id: NonEmptyText,
    fresh_for_seconds: u64,
    stale_after_seconds: u64,
}

impl FreshnessPolicy {
    pub fn new(
        id: NonEmptyText,
        fresh_for_seconds: u64,
        stale_after_seconds: u64,
    ) -> Result<Self, ContractError> {
        if fresh_for_seconds >= stale_after_seconds {
            return Err(ContractError::InvalidFreshnessPolicy);
        }
        Ok(Self {
            id,
            fresh_for_seconds,
            stale_after_seconds,
        })
    }

    pub fn id(&self) -> &NonEmptyText {
        &self.id
    }
    pub fn fresh_for_seconds(&self) -> u64 {
        self.fresh_for_seconds
    }
    pub fn stale_after_seconds(&self) -> u64 {
        self.stale_after_seconds
    }
}

impl TryFrom<RawPolicy> for FreshnessPolicy {
    type Error = ContractError;
    fn try_from(raw: RawPolicy) -> Result<Self, Self::Error> {
        Self::new(raw.id, raw.fresh_for_seconds, raw.stale_after_seconds)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessState {
    Live,
    Fresh,
    Aging,
    Stale,
    Unknown,
}

/// Constructed only through assessment of snapshot evidence, never from a label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Freshness {
    state: FreshnessState,
    age_seconds: Option<u64>,
    assessed_at: UnixSeconds,
    policy: FreshnessPolicy,
}

impl Freshness {
    fn assess(provenance: &Provenance, policy: FreshnessPolicy, now: UnixSeconds) -> Self {
        let age_seconds = provenance
            .created_at
            .and_then(|created| now.0.checked_sub(created.0));
        let state = match age_seconds {
            None => FreshnessState::Unknown,
            Some(age) if age <= policy.fresh_for_seconds => FreshnessState::Fresh,
            Some(age) if age <= policy.stale_after_seconds => FreshnessState::Aging,
            Some(_) => FreshnessState::Stale,
        };
        Self {
            state,
            age_seconds,
            assessed_at: now,
            policy,
        }
    }

    pub fn state(&self) -> FreshnessState {
        self.state
    }
    pub fn age_seconds(&self) -> Option<u64> {
        self.age_seconds
    }
    pub fn assessed_at(&self) -> UnixSeconds {
        self.assessed_at
    }
    pub fn policy(&self) -> &FreshnessPolicy {
        &self.policy
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawSnapshotEvidence")]
pub struct SnapshotEvidence {
    provenance: Provenance,
    freshness: Freshness,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFreshness {
    state: FreshnessState,
    #[serde(deserialize_with = "crate::required_nullable")]
    age_seconds: Option<u64>,
    assessed_at: UnixSeconds,
    policy: FreshnessPolicy,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSnapshotEvidence {
    provenance: Provenance,
    freshness: RawFreshness,
}

impl SnapshotEvidence {
    pub fn assess(provenance: Provenance, policy: FreshnessPolicy, clock: &impl Clock) -> Self {
        let freshness = Freshness::assess(&provenance, policy, clock.now());
        Self {
            provenance,
            freshness,
        }
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
    pub fn freshness(&self) -> &Freshness {
        &self.freshness
    }
}

impl TryFrom<RawSnapshotEvidence> for SnapshotEvidence {
    type Error = ContractError;
    fn try_from(raw: RawSnapshotEvidence) -> Result<Self, Self::Error> {
        let expected = Freshness::assess(
            &raw.provenance,
            raw.freshness.policy,
            raw.freshness.assessed_at,
        );
        if expected.state != raw.freshness.state
            || expected.age_seconds != raw.freshness.age_seconds
        {
            return Err(ContractError::InconsistentFreshness);
        }
        Ok(Self {
            provenance: raw.provenance,
            freshness: expected,
        })
    }
}

/// Provenance and freshness cannot be independently omitted from a snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "details",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Evidence {
    Local,
    Snapshot(SnapshotEvidence),
}
