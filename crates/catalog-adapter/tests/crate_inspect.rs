use rust_engineering_application::{CatalogInspectRepository, CatalogRepository};
use rust_engineering_catalog::SqliteCatalogRepository;
use rust_engineering_domain::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn version(raw: &str) -> VersionRecord {
    VersionRecord {
        version: raw.into(),
        yanked: false,
        rust_version: Some("1.70".into()),
        license: Some("MIT".into()),
        published_at: Some(42),
        features: vec![],
        dependencies: vec![],
        advisories: vec![],
    }
}
fn repository(
    versions: Vec<VersionRecord>,
) -> Result<SqliteCatalogRepository, Box<dyn std::error::Error>> {
    let provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "inspect-fixture".parse()?,
        Some(UnixSeconds(100)),
        Some(UnixSeconds(100)),
        IntegrityStatus::Verified,
        false,
    )?;
    let snapshot = SqliteCatalogRepository::build(
        1,
        provenance,
        &[CrateRecord {
            name: "example".into(),
            description: "declared facts".into(),
            repository: Some("declared repository text".into()),
            updated_at: Some(100),
            versions,
        }],
    )?;
    Ok(SqliteCatalogRepository::open(
        &snapshot.bytes,
        &snapshot.manifest,
    )?)
}
fn fixture() -> Result<SqliteCatalogRepository, Box<dyn std::error::Error>> {
    let mut old = version("1.0.0");
    old.features = vec!["zeta".into(), "alpha".into(), "middle".into()];
    old.advisories = vec![
        "RUSTSEC-2024-0003".into(),
        "RUSTSEC-2024-0001".into(),
        "RUSTSEC-2024-0002".into(),
    ];
    old.dependencies = vec![
        DependencyRecord {
            name: "same".into(),
            requirement: "^1".into(),
            kind: DependencyKind::Normal,
            optional: false,
        },
        DependencyRecord {
            name: "same".into(),
            requirement: "^2".into(),
            kind: DependencyKind::Build,
            optional: true,
        },
        DependencyRecord {
            name: "aaa".into(),
            requirement: "*".into(),
            kind: DependencyKind::Dev,
            optional: false,
        },
    ];
    let mut stable = version("2.0.0");
    stable.yanked = true;
    repository(vec![old, stable, version("10.0.0-alpha")])
}
fn request(section: InspectSection) -> CrateInspectRequest {
    CrateInspectRequest {
        name: "example".into(),
        section,
        version: None,
        limit: 20,
        offset: 0,
        snapshot_fingerprint: None,
    }
}
fn found(
    repo: &SqliteCatalogRepository,
    request: &CrateInspectRequest,
) -> Result<Box<InspectPage>, Box<dyn std::error::Error>> {
    match repo.inspect_page(request)? {
        InspectLookup::Found { page } => Ok(page),
        _ => Err("expected page".into()),
    }
}
#[test]
fn overview_preserves_stable_yanked_and_explicit_unknown_fields() -> TestResult {
    let repo = fixture()?;
    let page = found(&repo, &request(InspectSection::Overview))?;
    let stable = page.overview.latest_known_stable.ok_or("stable")?;
    assert_eq!(stable.version, "2.0.0");
    assert!(stable.yanked);
    assert_eq!(page.overview.version_count, 3);
    assert_eq!(
        page.overview.repository.as_deref(),
        Some("declared repository text")
    );
    assert_eq!(page.overview.documentation, InspectUnknown::default());
    assert_eq!(page.overview.source, InspectUnknown::default());
    assert_eq!(
        page.pagination,
        InspectPagination {
            offset: 0,
            total: 1,
            returned: 1,
            next_offset: None,
            omitted_by_output: 0
        }
    );
    assert!(matches!(
        page.data,
        InspectPageData::Overview {
            selected_version: None
        }
    ));
    let mut query = request(InspectSection::Overview);
    query.version = Some("1.0.0".into());
    let InspectPageData::Overview {
        selected_version: Some(selected),
    } = found(&repo, &query)?.data
    else {
        return Err("selected".into());
    };
    assert_eq!(
        (
            selected.feature_count,
            selected.dependency_count,
            selected.advisory_count
        ),
        (3, 3, 3)
    );
    assert_eq!(selected.published_at, Some(42));
    Ok(())
}
#[test]
fn all_collection_pages_reconstruct_exact_sorted_facts() -> TestResult {
    let repo = fixture()?;
    for section in [
        InspectSection::Versions,
        InspectSection::Features,
        InspectSection::Dependencies,
        InspectSection::Advisories,
    ] {
        let mut query = request(section);
        query.limit = 1;
        if section != InspectSection::Versions {
            query.version = Some("1.0.0".into());
        }
        let mut values = Vec::new();
        loop {
            let page = found(&repo, &query)?;
            assert_eq!(page.pagination.total, 3);
            assert_eq!(page.pagination.returned, 1);
            match page.data {
                InspectPageData::Versions { items } => {
                    values.extend(items.into_iter().map(|v| v.version))
                }
                InspectPageData::Features { version, items }
                | InspectPageData::Advisories { version, items } => {
                    assert_eq!(version.version, "1.0.0");
                    values.extend(items);
                }
                InspectPageData::Dependencies { version, items } => {
                    assert_eq!(version.version, "1.0.0");
                    values.extend(items.into_iter().map(|v| {
                        format!("{}:{:?}:{}:{}", v.name, v.kind, v.requirement, v.optional)
                    }));
                }
                _ => return Err("section".into()),
            }
            let Some(next) = page.pagination.next_offset else {
                break;
            };
            query.offset = next;
            query.snapshot_fingerprint = Some(repo.metadata().fingerprint.clone());
        }
        let expected = match section {
            InspectSection::Versions => vec!["10.0.0-alpha", "2.0.0", "1.0.0"],
            InspectSection::Features => vec!["alpha", "middle", "zeta"],
            InspectSection::Dependencies => vec![
                "aaa:Dev:*:false",
                "same:Build:^2:true",
                "same:Normal:^1:false",
            ],
            InspectSection::Advisories => vec![
                "RUSTSEC-2024-0001",
                "RUSTSEC-2024-0002",
                "RUSTSEC-2024-0003",
            ],
            _ => unreachable!(),
        };
        assert_eq!(values, expected);
        query.offset = 3;
        query.snapshot_fingerprint = Some(repo.metadata().fingerprint.clone());
        let end = found(&repo, &query)?;
        assert_eq!(end.pagination.returned, 0);
        assert_eq!(end.pagination.next_offset, None);
        query.offset = 4;
        assert_eq!(repo.inspect_page(&query), Err(CatalogError::InvalidInput));
    }
    Ok(())
}
#[test]
fn absence_empty_collection_and_no_stable_are_distinct() -> TestResult {
    let repo = repository(vec![version("1.0.0-alpha")])?;
    assert!(
        found(&repo, &request(InspectSection::Overview))?
            .overview
            .latest_known_stable
            .is_none()
    );
    let mut query = request(InspectSection::Overview);
    query.name = "missing".into();
    assert_eq!(repo.inspect_page(&query)?, InspectLookup::CrateNotFound);
    query.name = "example".into();
    query.version = Some("1.0.0".into());
    assert_eq!(repo.inspect_page(&query)?, InspectLookup::VersionNotFound);
    query.section = InspectSection::Advisories;
    query.version = Some("1.0.0-alpha".into());
    let page = found(&repo, &query)?;
    assert_eq!(page.pagination.total, 0);
    assert_eq!(page.pagination.returned, 0);
    assert!(matches!(page.data,InspectPageData::Advisories{items,..} if items.is_empty()));
    Ok(())
}
#[test]
fn syntax_and_shape_are_rejected_before_absence_lookup() -> TestResult {
    let repo = fixture()?;
    let mut queries = vec![];
    for raw in ["1.0", "*", "1.0.0' OR 1=1", "01.0.0"] {
        let mut q = request(InspectSection::Overview);
        q.name = "missing".into();
        q.version = Some(raw.into());
        queries.push(q);
    }
    let mut q = request(InspectSection::Overview);
    q.name = "x' OR 1=1".into();
    queries.push(q);
    for limit in [0, 51] {
        let mut q = request(InspectSection::Versions);
        q.limit = limit;
        queries.push(q);
    }
    let mut q = request(InspectSection::Versions);
    q.offset = 1;
    queries.push(q);
    queries.push(request(InspectSection::Features));
    let mut q = request(InspectSection::Versions);
    q.version = Some("1.0.0".into());
    queries.push(q);
    for q in queries {
        assert_eq!(repo.inspect_page(&q), Err(CatalogError::InvalidInput));
    }
    Ok(())
}
#[test]
fn maximum_schema_collections_and_versions_are_pageable() -> TestResult {
    let mut versions = (0..64)
        .map(|minor| version(&format!("1.{minor}.0")))
        .collect::<Vec<_>>();
    versions[0].features = (0..128).map(|i| format!("f{i:03}")).collect();
    let repo = repository(versions)?;
    let mut q = request(InspectSection::Versions);
    q.offset = 50;
    q.snapshot_fingerprint = Some(repo.metadata().fingerprint.clone());
    q.limit = 50;
    let page = found(&repo, &q)?;
    assert_eq!(page.pagination.total, 64);
    assert_eq!(page.pagination.returned, 14);
    q.section = InspectSection::Features;
    q.version = Some("1.0.0".into());
    q.offset = 100;
    let page = found(&repo, &q)?;
    assert_eq!(page.pagination.total, 128);
    assert_eq!(page.pagination.returned, 28);
    Ok(())
}
