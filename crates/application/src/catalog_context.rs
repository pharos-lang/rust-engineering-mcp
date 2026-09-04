//! Component availability and freshness, without project capability or I/O policy.
use crate::{InspectionControl, ProjectError};
use rust_engineering_domain::*;

pub trait CatalogStatusPort {
    /// Available means authenticated/loaded by the adapter, not file existence.
    /// Observations are staging values and grant no authority to their consumer.
    fn observe(
        &self,
        control: &dyn InspectionControl,
    ) -> Result<CatalogContextObservation, ProjectError>;
}

pub fn catalog_context(
    provider: &(impl CatalogStatusPort + ?Sized),
    clock: &impl Clock,
    control: &dyn InspectionControl,
) -> Result<CatalogContextStatus, ProjectError> {
    control.check()?;
    let observation = provider.observe(control)?;
    control.check()?;
    validate(&observation)?;
    struct Now(UnixSeconds);
    impl Clock for Now {
        fn now(&self) -> UnixSeconds {
            self.0
        }
    }
    let now = Now(clock.now());
    let reservation = observation.reservation.map(|reservation| {
        let pending = match &observation.catalog {
            Component::Available { value } => value.metadata.sequence < reservation.sequence,
            Component::Unavailable { .. } => true,
        };
        CatalogReservationStatus {
            reservation,
            pending,
        }
    });
    let catalog = map(observation.catalog, |value| {
        Ok(CatalogContextCatalogStatus {
            publisher: value.publisher,
            channel: value.channel,
            publisher_key_fingerprint: value.publisher_key_fingerprint,
            bundle_fingerprint: value.bundle_fingerprint,
            sequence: value.metadata.sequence,
            fingerprint: value.metadata.fingerprint,
            schema_version: value.schema_version,
            crate_count: value.crate_count,
            bundled_rustsec_available: value.bundled_rustsec_available,
            evidence: assess(value.metadata.provenance, "catalog-snapshot-v1", &now)?,
        })
    })?;
    let model = map(observation.model, |identity| {
        Ok(CatalogModelStatus {
            evidence: assess(identity.provenance.clone(), "catalog-model-v1", &now)?,
            identity,
        })
    })?;
    let rustsec = map(observation.rustsec, |value| {
        Ok(CatalogRustsecStatus {
            fingerprint: value.fingerprint,
            sequence: value.sequence,
            record_count: value.record_count,
            evidence: assess(value.provenance, "rustsec-host-snapshot-v1", &now)?,
        })
    })?;
    control.check()?;
    Ok(CatalogContextStatus {
        catalog,
        reservation,
        model,
        semantic_index: observation.semantic_index,
        rustsec,
    })
}

fn map<T, U>(
    component: Component<T>,
    f: impl FnOnce(T) -> Result<U, ProjectError>,
) -> Result<Component<U>, ProjectError> {
    match component {
        Component::Available { value } => Ok(Component::Available { value: f(value)? }),
        Component::Unavailable { reason } => Ok(Component::Unavailable { reason }),
    }
}
fn assess(
    provenance: Provenance,
    policy: &str,
    clock: &impl Clock,
) -> Result<SnapshotEvidence, ProjectError> {
    let policy = FreshnessPolicy::new(
        policy.parse().map_err(|_| ProjectError::Internal)?,
        86_400,
        604_800,
    )
    .map_err(|_| ProjectError::Internal)?;
    Ok(SnapshotEvidence::assess(provenance, policy, clock))
}
fn identity(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}
fn sequence(value: u64) -> bool {
    value > 0 && value <= i64::MAX as u64
}
fn provenance(value: &Provenance, kind: SourceKind) -> bool {
    value.source_kind() == kind
        && value.integrity() == IntegrityStatus::Verified
        && value.source_id().as_str().len() <= 256
}
fn validate(value: &CatalogContextObservation) -> Result<(), ProjectError> {
    if let Some(floor) = &value.reservation
        && (!identity(&floor.publisher) || !identity(&floor.channel) || !sequence(floor.sequence))
    {
        return Err(ProjectError::Internal);
    }
    if let Component::Available { value: catalog } = &value.catalog {
        if !identity(&catalog.publisher)
            || !identity(&catalog.channel)
            || !sequence(catalog.metadata.sequence)
            || catalog.schema_version != 1
            || catalog.crate_count > 1000
            || !provenance(&catalog.metadata.provenance, SourceKind::RegistrySnapshot)
        {
            return Err(ProjectError::Internal);
        }
        let Some(floor) = &value.reservation else {
            return Err(ProjectError::Internal);
        };
        if floor.publisher != catalog.publisher
            || floor.channel != catalog.channel
            || floor.sequence < catalog.metadata.sequence
            || (floor.sequence == catalog.metadata.sequence
                && floor.bundle_fingerprint != catalog.bundle_fingerprint)
        {
            return Err(ProjectError::Internal);
        }
    }
    if let Component::Available { value: model } = &value.model
        && (model.validate().is_err()
            || !provenance(&model.provenance, SourceKind::EmbeddingModel)
            || [&model.model, &model.revision, &model.runtime]
                .iter()
                .any(|s| s.chars().any(char::is_control)))
    {
        return Err(ProjectError::Internal);
    }
    if let Component::Available { value: index } = &value.semantic_index {
        let (Component::Available { value: catalog }, Component::Available { value: model }) =
            (&value.catalog, &value.model)
        else {
            return Err(ProjectError::Internal);
        };
        if index.metadata.schema_version != 1
            || index.metadata.snapshot_fingerprint != catalog.metadata.fingerprint
            || index.metadata.model != *model
            || index.documents != catalog.crate_count
        {
            return Err(ProjectError::Internal);
        }
    }
    // RustSec accepts any positive u64 sequence, unlike the signed-SQLite catalog
    // floor. RustSecSnapshot::from_bytes enforces 1..=2048 records (ADR-038).
    if let Component::Available { value: rustsec } = &value.rustsec
        && (rustsec.sequence == 0
            || !(1..=2048).contains(&rustsec.record_count)
            || !provenance(&rustsec.provenance, SourceKind::RustsecSnapshot))
    {
        return Err(ProjectError::Internal);
    }
    Ok(())
}
