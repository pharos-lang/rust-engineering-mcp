//! Pure paged inspection. The adapter owns SQL, exact SemVer parsing and ordering.
use crate::{CatalogRepository, InspectionControl, ProjectError};
use rust_engineering_domain::*;
use std::collections::BTreeSet;

pub trait CatalogInspectRepository: CatalogRepository {
    fn inspect_page(&self, request: &CrateInspectRequest) -> Result<InspectLookup, CatalogError>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogInspectError {
    Project(ProjectError),
    Catalog(CatalogError),
    Unavailable(CatalogComponentUnavailable),
    SnapshotMismatch,
}
impl From<ProjectError> for CatalogInspectError {
    fn from(error: ProjectError) -> Self {
        Self::Project(error)
    }
}
impl From<CatalogError> for CatalogInspectError {
    fn from(error: CatalogError) -> Self {
        Self::Catalog(error)
    }
}
impl std::fmt::Display for CatalogInspectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "catalog inspection failed: {self:?}")
    }
}
impl std::error::Error for CatalogInspectError {}

fn name_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}
fn text(value: &str, max: usize) -> bool {
    value.len() <= max && !value.contains('\0')
}
fn optional(value: &Option<String>, max: usize) -> bool {
    value
        .as_ref()
        .is_none_or(|v| !v.trim().is_empty() && text(v, max))
}
fn timestamp(value: Option<u64>) -> bool {
    value.is_none_or(|v| v <= i64::MAX as u64)
}
fn version_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn version_valid(value: &InspectVersion) -> bool {
    version_text(&value.version)
        && optional(&value.rust_version, 32)
        && optional(&value.license, 512)
        && timestamp(value.published_at)
        && value.feature_count <= 128
        && value.dependency_count <= 128
        && value.advisory_count <= 128
}
fn known_valid(value: &KnownVersion) -> bool {
    version_text(&value.version)
        && optional(&value.rust_version, 32)
        && optional(&value.license, 512)
}
fn version_consistent(overview: &InspectOverview, value: &InspectVersion) -> bool {
    version_valid(value)
        && overview.latest_known_stable.as_ref().is_none_or(|known| {
            known.version != value.version
                || (known.yanked == value.yanked
                    && known.rust_version == value.rust_version
                    && known.license == value.license)
        })
}
fn exact_version(
    request: &CrateInspectRequest,
    overview: &InspectOverview,
    value: &InspectVersion,
) -> bool {
    request.version.as_deref() == Some(value.version.as_str())
        && version_consistent(overview, value)
}
fn ordered<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}
fn dependency_key(value: &DependencyRecord) -> (&str, &str) {
    (
        value.name.as_str(),
        match value.kind {
            DependencyKind::Build => "build",
            DependencyKind::Dev => "dev",
            DependencyKind::Normal => "normal",
        },
    )
}
fn validate_page(page: &InspectPage, request: &CrateInspectRequest) -> Result<(), CatalogError> {
    let invalid = CatalogError::InvalidSnapshot;
    let overview = &page.overview;
    if overview.name != request.name
        || !text(&overview.description, 4096)
        || !optional(&overview.repository, 2048)
        || !timestamp(overview.updated_at)
        || !(1..=64).contains(&overview.version_count)
        || overview
            .latest_known_stable
            .as_ref()
            .is_some_and(|value| !known_valid(value))
    {
        return Err(invalid);
    }
    let (count, total) = match (&page.data, request.section) {
        (InspectPageData::Overview { selected_version }, InspectSection::Overview) => {
            if selected_version.is_some() != request.version.is_some()
                || selected_version
                    .as_ref()
                    .is_some_and(|value| !exact_version(request, overview, value))
            {
                return Err(invalid);
            }
            (1, 1)
        }
        (InspectPageData::Versions { items }, InspectSection::Versions) => {
            let mut seen = BTreeSet::new();
            if items.len() > 50
                || items.iter().any(|value| {
                    !version_consistent(overview, value) || !seen.insert(&value.version)
                })
            {
                return Err(invalid);
            }
            // SemVer order cannot be checked as lexicographic text; the adapter
            // guarantees descending order using the pinned parser before return.
            (items.len() as u32, overview.version_count)
        }
        (InspectPageData::Features { version, items }, InspectSection::Features) => {
            if !exact_version(request, overview, version)
                || items.len() > 50
                || !ordered(items)
                || items.iter().any(|value| {
                    value.is_empty() || !text(value, 64) || value.chars().any(char::is_control)
                })
            {
                return Err(invalid);
            }
            (items.len() as u32, version.feature_count)
        }
        (InspectPageData::Dependencies { version, items }, InspectSection::Dependencies) => {
            if !exact_version(request, overview, version)
                || items.len() > 50
                || items.iter().any(|value| {
                    !name_valid(&value.name)
                        || value.requirement.trim().is_empty()
                        || !text(&value.requirement, 128)
                })
                || !items
                    .windows(2)
                    .all(|pair| dependency_key(&pair[0]) < dependency_key(&pair[1]))
            {
                return Err(invalid);
            }
            (items.len() as u32, version.dependency_count)
        }
        (InspectPageData::Advisories { version, items }, InspectSection::Advisories) => {
            if !exact_version(request, overview, version)
                || items.len() > 50
                || !ordered(items)
                || items.iter().any(|value| !name_valid(value))
            {
                return Err(invalid);
            }
            (items.len() as u32, version.advisory_count)
        }
        _ => return Err(invalid),
    };
    let page_info = &page.pagination;
    if page_info.offset != request.offset
        || page_info.total != total
        || page_info.offset > total
        || page_info.returned != count
        || page_info.omitted_by_output != 0
    {
        return Err(invalid);
    }
    // The repository returns a complete bounded page. Only the outer MCP encoder
    // can omit whole trailing items for its byte cap and recalculate continuation.
    let expected = (total - page_info.offset).min(request.limit);
    let end = page_info.offset.checked_add(count).ok_or(invalid)?;
    if count != expected || page_info.next_offset != (end < total).then_some(end) {
        return Err(invalid);
    }
    Ok(())
}

pub fn inspect_crate(
    repository: &(impl CatalogInspectRepository + ?Sized),
    request: &CrateInspectRequest,
    clock: &impl Clock,
    control: &dyn InspectionControl,
) -> Result<CrateInspectResult, CatalogInspectError> {
    control.check()?;
    request.validate()?;
    let metadata = repository.metadata().clone();
    if metadata.sequence == 0
        || metadata.sequence > i64::MAX as u64
        || metadata.provenance.source_kind() != SourceKind::RegistrySnapshot
        || metadata.provenance.integrity() != IntegrityStatus::Verified
        || metadata.provenance.source_id().as_str().len() > 256
    {
        return Err(CatalogError::InvalidSnapshot.into());
    }
    if request
        .snapshot_fingerprint
        .as_ref()
        .is_some_and(|expected| expected != &metadata.fingerprint)
    {
        return Err(CatalogInspectError::SnapshotMismatch);
    }
    control.check()?;
    let lookup = repository.inspect_page(request);
    control.check()?;
    let lookup = lookup?;
    let after = repository.metadata();
    if after.sequence != metadata.sequence
        || after.fingerprint != metadata.fingerprint
        || after.provenance != metadata.provenance
    {
        return Err(CatalogError::InvalidSnapshot.into());
    }
    match &lookup {
        InspectLookup::Found { page } => validate_page(page, request)?,
        InspectLookup::VersionNotFound if request.version.is_none() => {
            return Err(CatalogError::InvalidSnapshot.into());
        }
        InspectLookup::VersionNotFound | InspectLookup::CrateNotFound => {}
    }
    control.check()?;
    let policy = FreshnessPolicy::new(
        "catalog-snapshot-v1"
            .parse()
            .map_err(|_| ProjectError::Internal)?,
        86_400,
        604_800,
    )
    .map_err(|_| ProjectError::Internal)?;
    let result = CrateInspectResult {
        name: request.name.clone(),
        snapshot_fingerprint: metadata.fingerprint,
        sequence: metadata.sequence,
        evidence: SnapshotEvidence::assess(metadata.provenance, policy, clock),
        lookup,
    };
    control.check()?;
    Ok(result)
}
