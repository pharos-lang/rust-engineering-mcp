use rust_engineering_application::*;
use rust_engineering_domain::*;
use std::sync::atomic::{AtomicUsize, Ordering};
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Time(u64);
impl Clock for Time {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0)
    }
}
struct Control {
    count: AtomicUsize,
    fail_at: usize,
}
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.count.fetch_add(1, Ordering::SeqCst) == self.fail_at {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}
impl ExecutionCancellation for Control {
    fn is_cancelled(&self) -> bool {
        false
    }
}
fn control(fail_at: usize) -> Control {
    Control {
        count: AtomicUsize::new(0),
        fail_at,
    }
}
struct Provider {
    value: CatalogContextObservation,
    calls: AtomicUsize,
}
impl CatalogStatusPort for Provider {
    fn observe(
        &self,
        _: &dyn InspectionControl,
    ) -> Result<CatalogContextObservation, ProjectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.value.clone())
    }
}
fn fp(n: u8) -> Result<CatalogFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{n:064x}").parse()?)
}
fn prov(kind: SourceKind, created: Option<u64>) -> Result<Provenance, Box<dyn std::error::Error>> {
    Ok(Provenance::new(
        kind,
        "fixture-source".parse()?,
        created.map(UnixSeconds),
        created.map(UnixSeconds),
        IntegrityStatus::Verified,
        false,
    )?)
}
fn unavailable<T>() -> Component<T> {
    Component::Unavailable {
        reason: CatalogComponentUnavailable::NotConfigured,
    }
}
fn observation() -> Result<CatalogContextObservation, Box<dyn std::error::Error>> {
    let model = EmbeddingIdentity {
        model: "intfloat/multilingual-e5-small".into(),
        revision: "fixed".into(),
        artifact_fingerprint: fp(3)?,
        runtime: "ort-fixed".into(),
        provenance: prov(SourceKind::EmbeddingModel, None)?,
        dimension: 384,
        max_tokens: 512,
        intra_threads: 2,
        pooling: PoolingKind::Mean,
        normalization: Normalization::L2,
    };
    Ok(CatalogContextObservation {
        catalog: Component::Available {
            value: CatalogContextCatalogObservation {
                publisher: "fixture-only".into(),
                channel: "test".into(),
                publisher_key_fingerprint: fp(1)?,
                bundle_fingerprint: fp(2)?,
                metadata: CatalogMetadata {
                    sequence: 2,
                    fingerprint: fp(4)?,
                    provenance: prov(SourceKind::RegistrySnapshot, Some(100))?,
                },
                schema_version: 1,
                crate_count: 1,
                bundled_rustsec_available: true,
            },
        },
        reservation: Some(CatalogReservation {
            publisher: "fixture-only".into(),
            channel: "test".into(),
            sequence: 2,
            bundle_fingerprint: fp(2)?,
        }),
        model: Component::Available {
            value: model.clone(),
        },
        semantic_index: Component::Available {
            value: CatalogIndexObservation {
                metadata: IndexMetadata {
                    schema_version: 1,
                    snapshot_fingerprint: fp(4)?,
                    model,
                },
                documents: 1,
            },
        },
        rustsec: Component::Available {
            value: CatalogRustsecObservation {
                fingerprint: fp(5)?,
                sequence: 8,
                record_count: 1,
                provenance: prov(SourceKind::RustsecSnapshot, Some(700_000))?,
            },
        },
    })
}
fn assessed(
    value: CatalogContextObservation,
    now: u64,
) -> Result<CatalogContextStatus, ProjectError> {
    catalog_context(
        &Provider {
            value,
            calls: AtomicUsize::new(0),
        },
        &Time(now),
        &control(usize::MAX),
    )
}
fn available<T>(part: Component<T>) -> Result<T, &'static str> {
    match part {
        Component::Available { value } => Ok(value),
        _ => Err("expected available"),
    }
}

#[test]
fn catalog_context_assesses_independent_ages_without_project_reference() -> TestResult {
    let status = assessed(observation()?, 700_010).map_err(|e| format!("{e:?}"))?;
    let catalog = available(status.catalog)?;
    let model = available(status.model)?;
    let rustsec = available(status.rustsec)?;
    assert_eq!(catalog.evidence.freshness().state(), FreshnessState::Stale);
    assert_eq!(model.evidence.freshness().state(), FreshnessState::Unknown);
    assert_eq!(rustsec.evidence.freshness().state(), FreshnessState::Fresh);
    assert_eq!(rustsec.evidence.freshness().age_seconds(), Some(10));
    assert_eq!(
        rustsec.evidence.freshness().policy().id().as_str(),
        "rustsec-host-snapshot-v1"
    );
    assert_eq!(
        rustsec.evidence.freshness().policy().fresh_for_seconds(),
        86_400
    );
    assert_eq!(
        rustsec.evidence.freshness().policy().stale_after_seconds(),
        604_800
    );
    assert_eq!(
        catalog.evidence.provenance().observed_at(),
        Some(UnixSeconds(100))
    );
    assert!(!status.reservation.ok_or("missing reservation")?.pending);
    Ok(())
}

#[test]
fn catalog_context_retains_pending_floor_without_active_or_with_older_active() -> TestResult {
    let mut value = observation()?;
    value.reservation.as_mut().ok_or("floor")?.sequence = 3;
    value
        .reservation
        .as_mut()
        .ok_or("floor")?
        .bundle_fingerprint = fp(9)?;
    assert!(
        assessed(value.clone(), 700_010)
            .map_err(|e| format!("{e:?}"))?
            .reservation
            .ok_or("floor")?
            .pending
    );
    value.catalog = Component::Unavailable {
        reason: CatalogComponentUnavailable::Missing,
    };
    value.semantic_index = Component::Unavailable {
        reason: CatalogComponentUnavailable::DependencyUnavailable,
    };
    assert!(
        assessed(value, 700_010)
            .map_err(|e| format!("{e:?}"))?
            .reservation
            .ok_or("floor")?
            .pending
    );
    let value = CatalogContextObservation {
        catalog: unavailable(),
        reservation: None,
        model: unavailable(),
        semantic_index: unavailable(),
        rustsec: unavailable(),
    };
    assert!(
        assessed(value, 700_010)
            .map_err(|e| format!("{e:?}"))?
            .reservation
            .is_none()
    );
    Ok(())
}

#[test]
fn catalog_context_rejects_mixed_or_out_of_bounds_staging_observations() -> TestResult {
    for case in 0..16 {
        let mut value = observation()?;
        match case {
            0 => value.reservation = None,
            1 => value.reservation.as_mut().ok_or("floor")?.sequence = 1,
            2 => {
                value
                    .reservation
                    .as_mut()
                    .ok_or("floor")?
                    .bundle_fingerprint = fp(9)?
            }
            3 => value.reservation.as_mut().ok_or("floor")?.publisher = "other".into(),
            4 => value.reservation.as_mut().ok_or("floor")?.sequence = i64::MAX as u64 + 1,
            5 => {
                if let Component::Available { value } = &mut value.catalog {
                    value.crate_count = 1001;
                }
            }
            6 => {
                if let Component::Available { value } = &mut value.catalog {
                    value.metadata.sequence = 0;
                }
            }
            7 => {
                if let Component::Available { value } = &mut value.catalog {
                    value.schema_version = 2;
                }
            }
            8 => {
                if let Component::Available { value } = &mut value.semantic_index {
                    value.metadata.snapshot_fingerprint = fp(9)?;
                }
            }
            9 => {
                if let Component::Available { value } = &mut value.semantic_index {
                    value.metadata.model.revision = "other".into();
                }
            }
            10 => {
                if let Component::Available { value } = &mut value.semantic_index {
                    value.documents = 2;
                }
            }
            11 => value.model = unavailable(),
            12 => {
                if let Component::Available { value } = &mut value.rustsec {
                    value.record_count = 2049;
                }
            }
            13 => {
                if let Component::Available { value } = &mut value.rustsec {
                    value.provenance = prov(SourceKind::RegistrySnapshot, Some(100))?;
                }
            }
            14 => {
                if let Component::Available { value } = &mut value.model {
                    value.runtime = "bad\nlog".into();
                }
            }
            _ => value.catalog = unavailable(),
        }
        assert_eq!(
            assessed(value, 700_010),
            Err(ProjectError::Internal),
            "case {case}"
        );
    }
    Ok(())
}

#[test]
fn catalog_context_checks_cancellation_before_after_and_before_publication() -> TestResult {
    for fail_at in 0..3 {
        let provider = Provider {
            value: observation()?,
            calls: AtomicUsize::new(0),
        };
        assert_eq!(
            catalog_context(&provider, &Time(700_010), &control(fail_at)),
            Err(ProjectError::Cancelled)
        );
        assert_eq!(
            provider.calls.load(Ordering::SeqCst),
            usize::from(fail_at != 0)
        );
    }
    Ok(())
}

#[test]
fn catalog_context_bundled_rustsec_does_not_become_audit_source() -> TestResult {
    let mut value = observation()?;
    value.rustsec = unavailable();
    let status = assessed(value, 700_010).map_err(|e| format!("{e:?}"))?;
    assert!(available(status.catalog)?.bundled_rustsec_available);
    assert_eq!(status.rustsec, unavailable());
    Ok(())
}

#[test]
fn catalog_context_reassesses_retained_observation_at_policy_boundaries() -> TestResult {
    let provider = Provider {
        value: observation()?,
        calls: AtomicUsize::new(0),
    };
    for (age, expected) in [
        (86_400, FreshnessState::Fresh),
        (86_401, FreshnessState::Aging),
        (604_800, FreshnessState::Aging),
        (604_801, FreshnessState::Stale),
    ] {
        let now = 700_000 + age;
        let status = catalog_context(&provider, &Time(now), &control(usize::MAX))
            .map_err(|e| format!("{e:?}"))?;
        let rustsec = available(status.rustsec)?;
        assert_eq!(rustsec.evidence.freshness().state(), expected);
        assert_eq!(rustsec.evidence.freshness().assessed_at(), UnixSeconds(now));
        assert_eq!(
            rustsec.evidence.provenance().observed_at(),
            Some(UnixSeconds(700_000))
        );
        assert_eq!(
            available(status.model)?.evidence.freshness().state(),
            FreshnessState::Unknown
        );
    }
    assert_eq!(provider.calls.load(Ordering::SeqCst), 4);
    Ok(())
}

#[test]
fn rustsec_preserves_existing_positive_u64_sequence_contract() -> TestResult {
    let mut value = observation()?;
    if let Component::Available { value: rustsec } = &mut value.rustsec {
        rustsec.sequence = u64::MAX;
    }
    let provider = Provider {
        value,
        calls: AtomicUsize::new(0),
    };
    let result = catalog_context(&provider, &Time(102), &control(usize::MAX))
        .map_err(|e| format!("{e:?}"))?;
    let Component::Available { value } = result.rustsec else {
        return Err("RustSec unavailable".into());
    };
    assert_eq!(value.sequence, u64::MAX);
    Ok(())
}

#[test]
fn publisher_network_history_does_not_grant_runtime_acquisition() -> TestResult {
    let mut value = observation()?;
    if let Component::Available { value: catalog } = &mut value.catalog {
        catalog.metadata.provenance = Provenance::new(
            SourceKind::RegistrySnapshot,
            "publisher-fetch".parse()?,
            Some(UnixSeconds(100)),
            Some(UnixSeconds(100)),
            IntegrityStatus::Verified,
            true,
        )?;
    }
    let result = assessed(value, 700_010).map_err(|e| format!("{e:?}"))?;
    assert!(
        available(result.catalog)?
            .evidence
            .provenance()
            .network_used()
    );
    Ok(())
}
