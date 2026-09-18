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

#[cfg(test)]
mod required_nullable_contract {
    //! `Option` fields here mean "present but nullable", never "optional":
    //! the published output schemas list them as `required` with a `null` type.
    //! Adding `#[serde(default)]` to any of them must make these tests fail.
    //! The wire is handled as text: the domain keeps no dynamic JSON model.
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    const NULLABLE: [&str; 3] = ["created_at", "observed_at", "age_seconds"];

    fn unknown_age_snapshot() -> Result<String, Box<dyn std::error::Error>> {
        let provenance = Provenance::new(
            SourceKind::RustsecSnapshot,
            "required-nullable-fixture".parse()?,
            None,
            None,
            IntegrityStatus::Unverified,
            false,
        )?;
        let policy = FreshnessPolicy::new("policy".parse()?, 10, 20)?;
        struct Fixed;
        impl Clock for Fixed {
            fn now(&self) -> UnixSeconds {
                UnixSeconds(500)
            }
        }
        Ok(serde_json::to_string(&SnapshotEvidence::assess(
            provenance, policy, &Fixed,
        ))?)
    }

    /// Removes exactly one `"key":null,` member; every nullable field here is
    /// followed by another member, so the result stays well-formed JSON.
    fn without(wire: &str, key: &str) -> Result<String, String> {
        let member = format!("\"{key}\":null,");
        if wire.matches(&member).count() != 1 {
            return Err(format!("expected exactly one {member} in {wire}"));
        }
        Ok(wire.replacen(&member, "", 1))
    }

    #[test]
    fn explicit_null_deserializes_to_none() -> TestResult {
        let wire = unknown_age_snapshot()?;
        for key in NULLABLE {
            assert!(wire.contains(&format!("\"{key}\":null")), "{key}: {wire}");
        }
        let parsed: SnapshotEvidence = serde_json::from_str(&wire)?;
        assert_eq!(parsed.provenance().created_at(), None);
        assert_eq!(parsed.provenance().observed_at(), None);
        assert_eq!(parsed.freshness().age_seconds(), None);
        assert_eq!(parsed.freshness().state(), FreshnessState::Unknown);
        Ok(())
    }

    #[test]
    fn absent_field_is_rejected_instead_of_defaulting_to_none() -> TestResult {
        let wire = unknown_age_snapshot()?;
        for key in NULLABLE {
            let candidate = without(&wire, key)?;
            let error = serde_json::from_str::<SnapshotEvidence>(&candidate)
                .err()
                .ok_or(key)?;
            assert!(
                error
                    .to_string()
                    .contains(&format!("missing field `{key}`")),
                "{key}: {error}"
            );
        }
        // The same holds for a standalone provenance value.
        let provenance =
            serde_json::to_string(serde_json::from_str::<SnapshotEvidence>(&wire)?.provenance())?;
        assert!(serde_json::from_str::<Provenance>(&provenance).is_ok());
        for key in ["created_at", "observed_at"] {
            assert!(serde_json::from_str::<Provenance>(&without(&provenance, key)?).is_err());
        }
        Ok(())
    }
}
