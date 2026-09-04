use rust_engineering_application::*;
use rust_engineering_domain::*;
use std::{
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Control {
    checks: AtomicUsize,
    cancel_at: usize,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            checks: AtomicUsize::new(0),
            cancel_at: usize::MAX,
        }
    }
}
impl OperationControl for Control {
    fn check(&self) -> Result<(), ProjectError> {
        if self.checks.fetch_add(1, Ordering::SeqCst) == self.cancel_at {
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
struct Time(u64);
impl Clock for Time {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.0)
    }
}
fn fingerprint(value: u32) -> Result<CatalogFingerprint, Box<dyn std::error::Error>> {
    Ok(format!("sha256:{value:064x}").parse()?)
}
fn request(section: InspectSection) -> CrateInspectRequest {
    CrateInspectRequest {
        name: "serde".into(),
        section,
        version: match section {
            InspectSection::Features
            | InspectSection::Dependencies
            | InspectSection::Advisories => Some("1.0.0".into()),
            _ => None,
        },
        limit: 2,
        offset: 0,
        snapshot_fingerprint: None,
    }
}
fn version(value: &str) -> InspectVersion {
    InspectVersion {
        version: value.into(),
        yanked: true,
        rust_version: None,
        license: Some("MIT".into()),
        published_at: Some(1),
        feature_count: 3,
        dependency_count: 3,
        advisory_count: 3,
    }
}
fn page(section: InspectSection) -> InspectPage {
    let overview = InspectOverview {
        name: "serde".into(),
        description: "declared snapshot text".into(),
        repository: Some("declared not-validated URL".into()),
        updated_at: None,
        latest_known_stable: None,
        version_count: 3,
        documentation: InspectUnknown::default(),
        source: InspectUnknown::default(),
    };
    let data = match section {
        InspectSection::Overview => InspectPageData::Overview {
            selected_version: None,
        },
        InspectSection::Versions => InspectPageData::Versions {
            items: vec![version("3.0.0"), version("2.0.0")],
        },
        InspectSection::Features => InspectPageData::Features {
            version: version("1.0.0"),
            items: vec!["default".into(), "std".into()],
        },
        InspectSection::Dependencies => InspectPageData::Dependencies {
            version: version("1.0.0"),
            items: vec![
                DependencyRecord {
                    name: "same".into(),
                    requirement: "^1".into(),
                    kind: DependencyKind::Build,
                    optional: false,
                },
                DependencyRecord {
                    name: "same".into(),
                    requirement: "^2".into(),
                    kind: DependencyKind::Dev,
                    optional: true,
                },
            ],
        },
        InspectSection::Advisories => InspectPageData::Advisories {
            version: version("1.0.0"),
            items: vec!["RUSTSEC-2020-0001".into(), "RUSTSEC-2020-0002".into()],
        },
    };
    let (total, returned, next_offset) = if section == InspectSection::Overview {
        (1, 1, None)
    } else {
        (3, 2, Some(2))
    };
    InspectPage {
        overview,
        data,
        pagination: InspectPagination {
            offset: 0,
            total,
            returned,
            next_offset,
            omitted_by_output: 0,
        },
    }
}
struct Repository {
    metadata: CatalogMetadata,
    other: CatalogMetadata,
    changed: Cell<bool>,
    mutate: bool,
    queries: Cell<u32>,
    lookup: Result<InspectLookup, CatalogError>,
}
impl Repository {
    fn new(section: InspectSection) -> Result<Self, Box<dyn std::error::Error>> {
        let metadata = CatalogMetadata {
            sequence: 3,
            fingerprint: fingerprint(1)?,
            provenance: Provenance::new(
                SourceKind::RegistrySnapshot,
                "catalog-fixture".parse()?,
                Some(UnixSeconds(100)),
                Some(UnixSeconds(200)),
                IntegrityStatus::Verified,
                true,
            )?,
        };
        let mut other = metadata.clone();
        other.sequence = 4;
        Ok(Self {
            metadata,
            other,
            changed: Cell::new(false),
            mutate: false,
            queries: Cell::new(0),
            lookup: Ok(InspectLookup::Found {
                page: Box::new(page(section)),
            }),
        })
    }
}
impl CatalogRepository for Repository {
    fn metadata(&self) -> &CatalogMetadata {
        if self.changed.get() {
            &self.other
        } else {
            &self.metadata
        }
    }
    fn lexical(&self, _: &CatalogQuery) -> Result<Vec<CrateSummary>, CatalogError> {
        Err(CatalogError::InvalidInput)
    }
    fn summary(&self, _: &str) -> Result<Option<CrateSummary>, CatalogError> {
        Err(CatalogError::InvalidInput)
    }
    fn inspect(&self, _: &str) -> Result<Option<CrateRecord>, CatalogError> {
        Err(CatalogError::InvalidInput)
    }
}
impl CatalogInspectRepository for Repository {
    fn inspect_page(&self, _: &CrateInspectRequest) -> Result<InspectLookup, CatalogError> {
        self.queries.set(self.queries.get() + 1);
        if self.mutate {
            self.changed.set(true);
        }
        self.lookup.clone()
    }
}
#[test]
fn each_section_preserves_bounded_authoritative_facts_and_explicit_absence() -> TestResult {
    for section in [
        InspectSection::Overview,
        InspectSection::Versions,
        InspectSection::Features,
        InspectSection::Dependencies,
        InspectSection::Advisories,
    ] {
        let repository = Repository::new(section)?;
        let result = inspect_crate(
            &repository,
            &request(section),
            &Time(300),
            &Control::default(),
        )?;
        assert_eq!(result.lookup, repository.lookup?);
        assert_eq!(result.name, "serde");
        assert_eq!(result.sequence, 3);
        assert_eq!(result.snapshot_fingerprint, fingerprint(1)?);
        assert!(result.evidence.provenance().network_used());
        assert_eq!(repository.queries.get(), 1);
    }
    Ok(())
}
#[test]
fn fingerprint_mismatch_invalid_input_and_precancellation_prevent_queries() -> TestResult {
    let repository = Repository::new(InspectSection::Overview)?;
    let mut query = request(InspectSection::Overview);
    query.snapshot_fingerprint = Some(fingerprint(2)?);
    assert_eq!(
        inspect_crate(&repository, &query, &Time(300), &Control::default()),
        Err(CatalogInspectError::SnapshotMismatch)
    );
    query.name = "../bad".into();
    assert_eq!(
        inspect_crate(&repository, &query, &Time(300), &Control::default()),
        Err(CatalogInspectError::Catalog(CatalogError::InvalidInput))
    );
    let control = Control {
        cancel_at: 0,
        ..Default::default()
    };
    assert_eq!(
        inspect_crate(
            &repository,
            &request(InspectSection::Overview),
            &Time(300),
            &control
        ),
        Err(CatalogInspectError::Project(ProjectError::Cancelled))
    );
    assert_eq!(repository.queries.get(), 0);
    Ok(())
}
#[test]
fn every_checkpoint_cancellation_including_final_denies_publication() -> TestResult {
    let baseline = Control::default();
    inspect_crate(
        &Repository::new(InspectSection::Features)?,
        &request(InspectSection::Features),
        &Time(300),
        &baseline,
    )?;
    let checkpoints = baseline.checks.load(Ordering::SeqCst);
    assert!(checkpoints >= 5);
    for cancel_at in 0..checkpoints {
        let control = Control {
            cancel_at,
            ..Default::default()
        };
        assert_eq!(
            inspect_crate(
                &Repository::new(InspectSection::Features)?,
                &request(InspectSection::Features),
                &Time(300),
                &control
            ),
            Err(CatalogInspectError::Project(ProjectError::Cancelled))
        );
    }
    let mut repository = Repository::new(InspectSection::Overview)?;
    repository.lookup = Err(CatalogError::Budget);
    let control = Control {
        cancel_at: 2,
        ..Default::default()
    };
    assert_eq!(
        inspect_crate(
            &repository,
            &request(InspectSection::Overview),
            &Time(300),
            &control
        ),
        Err(CatalogInspectError::Project(ProjectError::Cancelled))
    );
    Ok(())
}
#[test]
fn missing_crate_missing_version_and_empty_collection_remain_distinct() -> TestResult {
    let mut repository = Repository::new(InspectSection::Overview)?;
    repository.lookup = Ok(InspectLookup::CrateNotFound);
    let missing = inspect_crate(
        &repository,
        &request(InspectSection::Overview),
        &Time(300),
        &Control::default(),
    )?;
    assert_eq!(missing.lookup, InspectLookup::CrateNotFound);
    repository.lookup = Ok(InspectLookup::VersionNotFound);
    assert_eq!(
        inspect_crate(
            &repository,
            &request(InspectSection::Overview),
            &Time(300),
            &Control::default()
        ),
        Err(CatalogInspectError::Catalog(CatalogError::InvalidSnapshot))
    );
    let mut query = request(InspectSection::Overview);
    query.version = Some("9.0.0".into());
    assert_eq!(
        inspect_crate(&repository, &query, &Time(300), &Control::default())?.lookup,
        InspectLookup::VersionNotFound
    );
    let mut empty = page(InspectSection::Advisories);
    if let InspectPageData::Advisories { version, items } = &mut empty.data {
        version.advisory_count = 0;
        items.clear();
    }
    empty.pagination = InspectPagination {
        offset: 0,
        total: 0,
        returned: 0,
        next_offset: None,
        omitted_by_output: 0,
    };
    repository.lookup = Ok(InspectLookup::Found {
        page: Box::new(empty),
    });
    assert!(matches!(
        inspect_crate(
            &repository,
            &request(InspectSection::Advisories),
            &Time(300),
            &Control::default()
        )?
        .lookup,
        InspectLookup::Found { .. }
    ));
    Ok(())
}
#[test]
fn offset_at_total_is_valid_but_underfilled_or_nonprogressing_pages_fail() -> TestResult {
    let mut repository = Repository::new(InspectSection::Features)?;
    let mut query = request(InspectSection::Features);
    query.offset = 3;
    query.snapshot_fingerprint = Some(fingerprint(1)?);
    let mut final_page = page(InspectSection::Features);
    if let InspectPageData::Features { items, .. } = &mut final_page.data {
        items.clear();
    }
    final_page.pagination = InspectPagination {
        offset: 3,
        total: 3,
        returned: 0,
        next_offset: None,
        omitted_by_output: 0,
    };
    repository.lookup = Ok(InspectLookup::Found {
        page: Box::new(final_page),
    });
    inspect_crate(&repository, &query, &Time(300), &Control::default())?;
    for case in 0..12 {
        let mut bad = page(InspectSection::Features);
        match case {
            0 => bad.overview.name = "other".into(),
            1 => bad.pagination.offset = 1,
            2 => bad.pagination.returned = 1,
            3 => bad.pagination.total = 4,
            4 => bad.pagination.next_offset = Some(0),
            5 => bad.pagination.next_offset = Some(3),
            6 => bad.pagination.omitted_by_output = 1,
            7 => bad.overview.version_count = 65,
            8 => bad.overview.updated_at = Some(u64::MAX),
            9 => bad.overview.description = "a".repeat(4097),
            10 => {
                if let InspectPageData::Features { version, .. } = &mut bad.data {
                    version.version = "2.0.0".into();
                }
            }
            _ => {
                if let InspectPageData::Features { items, .. } = &mut bad.data {
                    items[1] = items[0].clone();
                }
            }
        }
        repository.lookup = Ok(InspectLookup::Found {
            page: Box::new(bad),
        });
        assert_eq!(
            inspect_crate(
                &repository,
                &request(InspectSection::Features),
                &Time(300),
                &Control::default()
            ),
            Err(CatalogInspectError::Catalog(CatalogError::InvalidSnapshot)),
            "case {case}"
        );
    }
    Ok(())
}
#[test]
fn bad_section_dependency_order_and_duplicate_versions_are_rejected() -> TestResult {
    let mut repository = Repository::new(InspectSection::Overview)?;
    assert_eq!(
        inspect_crate(
            &repository,
            &request(InspectSection::Versions),
            &Time(300),
            &Control::default()
        ),
        Err(CatalogInspectError::Catalog(CatalogError::InvalidSnapshot))
    );
    let mut bad = page(InspectSection::Dependencies);
    if let InspectPageData::Dependencies { items, .. } = &mut bad.data {
        items.reverse();
    }
    repository.lookup = Ok(InspectLookup::Found {
        page: Box::new(bad),
    });
    assert_eq!(
        inspect_crate(
            &repository,
            &request(InspectSection::Dependencies),
            &Time(300),
            &Control::default()
        ),
        Err(CatalogInspectError::Catalog(CatalogError::InvalidSnapshot))
    );
    let mut bad = page(InspectSection::Versions);
    if let InspectPageData::Versions { items } = &mut bad.data {
        items[1] = items[0].clone();
    }
    repository.lookup = Ok(InspectLookup::Found {
        page: Box::new(bad),
    });
    assert_eq!(
        inspect_crate(
            &repository,
            &request(InspectSection::Versions),
            &Time(300),
            &Control::default()
        ),
        Err(CatalogInspectError::Catalog(CatalogError::InvalidSnapshot))
    );
    Ok(())
}
#[test]
fn freshness_is_reassessed_and_metadata_changes_fail_closed() -> TestResult {
    let mut repository = Repository::new(InspectSection::Overview)?;
    let query = request(InspectSection::Overview);
    let fresh = inspect_crate(&repository, &query, &Time(86_500), &Control::default())?;
    let aging = inspect_crate(&repository, &query, &Time(86_501), &Control::default())?;
    assert_eq!(fresh.evidence.freshness().state(), FreshnessState::Fresh);
    assert_eq!(aging.evidence.freshness().state(), FreshnessState::Aging);
    assert_eq!(fresh.evidence.provenance(), aging.evidence.provenance());
    repository.mutate = true;
    assert_eq!(
        inspect_crate(&repository, &query, &Time(300), &Control::default()),
        Err(CatalogInspectError::Catalog(CatalogError::InvalidSnapshot))
    );
    Ok(())
}
#[test]
fn unverified_metadata_and_port_errors_are_not_notfound() -> TestResult {
    let mut repository = Repository::new(InspectSection::Overview)?;
    repository.metadata.provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "fixture".parse()?,
        None,
        None,
        IntegrityStatus::Unverified,
        false,
    )?;
    assert_eq!(
        inspect_crate(
            &repository,
            &request(InspectSection::Overview),
            &Time(300),
            &Control::default()
        ),
        Err(CatalogInspectError::Catalog(CatalogError::InvalidSnapshot))
    );
    assert_eq!(repository.queries.get(), 0);
    repository = Repository::new(InspectSection::Overview)?;
    repository.lookup = Err(CatalogError::Budget);
    assert_eq!(
        inspect_crate(
            &repository,
            &request(InspectSection::Overview),
            &Time(300),
            &Control::default()
        ),
        Err(CatalogInspectError::Catalog(CatalogError::Budget))
    );
    Ok(())
}
