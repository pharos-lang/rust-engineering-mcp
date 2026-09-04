//! Offline catalog use case. No database, filesystem or protocol dependency.
use rust_engineering_domain::{
    CatalogError, CatalogMetadata, CatalogPage, CatalogQuery, Clock, CrateRecord, CrateSummary,
    FreshnessPolicy, SnapshotEvidence,
};

pub trait CatalogRepository {
    fn metadata(&self) -> &CatalogMetadata;
    fn lexical(&self, query: &CatalogQuery) -> Result<Vec<CrateSummary>, CatalogError>;
    fn summary(&self, name: &str) -> Result<Option<CrateSummary>, CatalogError>;
    fn inspect(&self, name: &str) -> Result<Option<CrateRecord>, CatalogError>;
}

pub fn search_catalog(
    repository: &impl CatalogRepository,
    query: &CatalogQuery,
    policy: FreshnessPolicy,
    clock: &impl Clock,
) -> Result<CatalogPage, CatalogError> {
    let crates = repository.lexical(query)?;
    let metadata = repository.metadata();
    Ok(CatalogPage {
        snapshot_fingerprint: metadata.fingerprint.clone(),
        crates,
        evidence: SnapshotEvidence::assess(metadata.provenance.clone(), policy, clock),
    })
}
