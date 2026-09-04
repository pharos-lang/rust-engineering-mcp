use std::error::Error;

use rust_engineering_domain::*;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

struct FixedClock(u64);
impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0)
    }
}

fn policy() -> Result<FreshnessPolicy, ContractError> {
    FreshnessPolicy::new("snapshot-policy-v1".parse()?, 10, 20)
}

fn provenance(created: Option<u64>, observed: Option<u64>) -> Result<Provenance, ContractError> {
    Provenance::new(
        SourceKind::RegistrySnapshot,
        "snapshot-2026-09-03.1".parse()?,
        created.map(UnixSeconds),
        observed.map(UnixSeconds),
        IntegrityStatus::Unverified,
        true,
    )
}

fn evidence(now: u64) -> Result<SnapshotEvidence, ContractError> {
    Ok(SnapshotEvidence::assess(
        provenance(Some(100), Some(100))?,
        policy()?,
        &FixedClock(now),
    ))
}

#[test]
fn exact_age_boundaries_are_reproducible() -> TestResult {
    for (now, expected) in [
        (100, FreshnessState::Fresh),
        (110, FreshnessState::Fresh),
        (111, FreshnessState::Aging),
        (120, FreshnessState::Aging),
        (121, FreshnessState::Stale),
    ] {
        let result = evidence(now)?;
        assert_eq!(result.freshness().state(), expected);
        assert_eq!(result.freshness().age_seconds(), Some(now - 100));
        assert_eq!(result.freshness().assessed_at(), UnixSeconds(now));
        assert_eq!(
            result.freshness().policy().id().as_str(),
            "snapshot-policy-v1"
        );
        assert_eq!(
            serde_json::from_str::<SnapshotEvidence>(&serde_json::to_string(&result)?)?,
            result
        );
    }
    Ok(())
}

#[test]
fn unknown_or_future_creation_is_never_false_freshness() -> TestResult {
    for source in [
        provenance(None, Some(100))?,
        provenance(Some(101), Some(101))?,
    ] {
        let result = SnapshotEvidence::assess(source, policy()?, &FixedClock(100));
        assert_eq!(result.freshness().state(), FreshnessState::Unknown);
        assert_eq!(result.freshness().age_seconds(), None);
        assert_eq!(
            serde_json::from_value::<SnapshotEvidence>(serde_json::to_value(&result)?)?,
            result
        );
    }
    let extreme = SnapshotEvidence::assess(
        provenance(Some(0), Some(0))?,
        policy()?,
        &FixedClock(u64::MAX),
    );
    assert_eq!(extreme.freshness().age_seconds(), Some(u64::MAX));
    assert_eq!(extreme.freshness().state(), FreshnessState::Stale);
    Ok(())
}

#[test]
fn observing_or_importing_a_snapshot_does_not_refresh_its_creation() -> TestResult {
    let result =
        SnapshotEvidence::assess(provenance(Some(0), Some(100))?, policy()?, &FixedClock(100));
    assert_eq!(result.freshness().state(), FreshnessState::Stale);
    assert_eq!(result.freshness().age_seconds(), Some(100));
    assert!(result.provenance().network_used());
    assert_eq!(result.provenance().integrity(), IntegrityStatus::Unverified);
    assert_ne!(result.freshness().state(), FreshnessState::Live);
    Ok(())
}

#[test]
fn policy_and_source_order_validate_constructor_and_deserialization() -> TestResult {
    for (fresh, stale) in [(10, 10), (11, 10), (u64::MAX, 0)] {
        assert_eq!(
            FreshnessPolicy::new("policy".parse()?, fresh, stale),
            Err(ContractError::InvalidFreshnessPolicy)
        );
        assert!(
            serde_json::from_value::<FreshnessPolicy>(json!({
                "id": "policy", "fresh_for_seconds": fresh, "stale_after_seconds": stale
            }))
            .is_err()
        );
    }
    assert!(FreshnessPolicy::new("zero-window".parse()?, 0, 1).is_ok());
    assert_eq!(
        provenance(Some(100), Some(99)),
        Err(ContractError::InvalidProvenance)
    );
    let mut invalid = serde_json::to_value(provenance(Some(100), Some(100))?)?;
    invalid["observed_at"] = json!(99);
    assert!(serde_json::from_value::<Provenance>(invalid).is_err());
    Ok(())
}

#[test]
fn stored_freshness_cannot_be_relabeled_or_detached_from_provenance() -> TestResult {
    let original = serde_json::to_value(evidence(121)?)?;
    for (field, value) in [
        ("state", json!("fresh")),
        ("state", json!("live")),
        ("age_seconds", json!(0)),
        ("assessed_at", json!(100)),
    ] {
        let mut candidate = original.clone();
        candidate["freshness"][field] = value;
        assert!(
            serde_json::from_value::<SnapshotEvidence>(candidate).is_err(),
            "{field}"
        );
    }
    for missing in ["provenance", "freshness"] {
        let mut candidate = original.clone();
        candidate
            .as_object_mut()
            .ok_or("object expected")?
            .remove(missing);
        assert!(serde_json::from_value::<SnapshotEvidence>(candidate).is_err());
    }
    let mut unknown = original;
    unknown["freshness"]["extra"] = json!(false);
    assert!(serde_json::from_value::<SnapshotEvidence>(unknown).is_err());
    Ok(())
}

#[test]
fn snapshot_evidence_has_required_nullable_fields_and_closed_discriminator() -> TestResult {
    let source = provenance(None, None)?;
    let original = serde_json::to_value(&source)?;
    assert_eq!(original["created_at"], Value::Null);
    for field in [
        "created_at",
        "observed_at",
        "source_id",
        "integrity",
        "network_used",
    ] {
        let mut candidate = original.clone();
        candidate
            .as_object_mut()
            .ok_or("object expected")?
            .remove(field);
        assert!(
            serde_json::from_value::<Provenance>(candidate).is_err(),
            "{field}"
        );
    }
    let snapshot = Evidence::Snapshot(evidence(111)?);
    assert_eq!(
        serde_json::from_value::<Evidence>(serde_json::to_value(&snapshot)?)?,
        snapshot
    );
    for invalid in [
        json!({"kind":"snapshot"}),
        json!({"kind":"live"}),
        json!({"kind":"local", "unknown":true}),
        json!({"kind":"snapshot", "details":{"provenance":source}}),
    ] {
        assert!(serde_json::from_value::<Evidence>(invalid).is_err());
    }
    let mut no_age = serde_json::to_value(evidence(99)?)?;
    no_age["freshness"]
        .as_object_mut()
        .ok_or("object expected")?
        .remove("age_seconds");
    assert!(serde_json::from_value::<SnapshotEvidence>(no_age).is_err());
    Ok(())
}

#[test]
fn typed_snapshot_result_roundtrip_keeps_evidence_and_truncation() -> TestResult {
    #[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CrateFacts {
        latest_known: NonEmptyText,
    }

    let snapshot = evidence(121)?;
    let result = OutputEnvelope::passed(Report {
        summary: "Local snapshot facts".parse()?,
        duration_ms: 0,
        data: CrateFacts {
            latest_known: "1.2.3".parse()?,
        },
        evidence: Evidence::Snapshot(snapshot.clone()),
        diagnostics: vec![],
        truncation: Truncation {
            stdout_truncated: false,
            stderr_truncated: true,
            diagnostics_omitted: 0,
        },
    });
    assert!(result.truncation().is_truncated());
    assert!(!Truncation::default().is_truncated());
    let wire = serde_json::to_string(&result)?;
    let decoded: OutputEnvelope<CrateFacts> = serde_json::from_str(&wire)?;
    assert_eq!(decoded, result);
    assert_eq!(decoded.evidence(), &Evidence::Snapshot(snapshot));
    Ok(())
}
