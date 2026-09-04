//! Real SQLite/FTS5 selection boundaries; no synthetic repository implementation.
use rust_engineering_application::CatalogSearchRepository;
use rust_engineering_catalog::SqliteCatalogRepository;
use rust_engineering_domain::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn version(text: &str, msrv: Option<&str>) -> VersionRecord {
    VersionRecord {
        version: text.into(),
        yanked: false,
        rust_version: msrv.map(str::to_owned),
        license: Some("MIT".into()),
        published_at: Some(100),
        features: vec![],
        dependencies: vec![],
        advisories: vec![],
    }
}
fn krate(name: &str, description: &str, versions: Vec<VersionRecord>) -> CrateRecord {
    CrateRecord {
        name: name.into(),
        description: description.into(),
        repository: Some(format!("https://example.invalid/{name}")),
        updated_at: Some(100),
        versions,
    }
}
fn repository(
    crates: &[CrateRecord],
) -> Result<SqliteCatalogRepository, Box<dyn std::error::Error>> {
    let provenance = Provenance::new(
        SourceKind::RegistrySnapshot,
        "search-fixture".parse()?,
        Some(UnixSeconds(100)),
        Some(UnixSeconds(100)),
        IntegrityStatus::Verified,
        false,
    )?;
    let snapshot = SqliteCatalogRepository::build(1, provenance, crates)?;
    assert_eq!(snapshot.manifest.format_version, 1);
    Ok(SqliteCatalogRepository::open(
        &snapshot.bytes,
        &snapshot.manifest,
    )?)
}
fn eligible(value: CrateSelection) -> Result<Box<SearchCrateFacts>, &'static str> {
    match value {
        CrateSelection::Eligible(value) => Ok(value),
        _ => Err("expected eligible"),
    }
}
fn msrv(text: &str) -> Result<CrateSearchFilters, CatalogError> {
    Ok(CrateSearchFilters {
        msrv_lte: Some(MsrvVersion::parse(text)?),
        ..Default::default()
    })
}

#[test]
fn selects_highest_eligible_across_all_versions_and_preserves_stable_yanked_fact() -> TestResult {
    let mut yanked = version("4.0.0", Some("1.70"));
    yanked.yanked = true;
    let repo = repository(&[krate(
        "choices",
        "compiler library",
        vec![
            version("1.0.0", Some("1.60")),
            version("2.0.0", Some("1.70")),
            version("3.0.0", Some("1.90")),
            yanked,
            version("5.0.0-beta.1", Some("1.70")),
        ],
    )])?;
    let result = eligible(repo.select("choices", &msrv("1.80")?)?)?;
    assert_eq!(result.selected_version.version, "2.0.0");
    assert_eq!(result.version_count, 5);
    assert_eq!(
        result.repository.as_deref(),
        Some("https://example.invalid/choices")
    );
    let stable = result.latest_known_stable.ok_or("stable")?;
    assert_eq!(stable.version, "4.0.0");
    assert!(stable.yanked);
    assert_eq!(
        eligible(repo.select("choices", &CrateSearchFilters::default())?)?
            .selected_version
            .version,
        "3.0.0"
    );
    let mut filters = msrv("1.80")?;
    filters.allow_yanked = true;
    assert_eq!(
        eligible(repo.select("choices", &filters)?)?
            .selected_version
            .version,
        "4.0.0"
    );
    filters.include_prerelease = true;
    assert_eq!(
        eligible(repo.select("choices", &filters)?)?
            .selected_version
            .version,
        "5.0.0-beta.1"
    );
    Ok(())
}

#[test]
fn msrv_unknown_or_unstable_is_excluded_only_when_compatibility_requested() -> TestResult {
    let repo = repository(&[
        krate("unknown", "unknown MSRV", vec![version("1.0.0", None)]),
        krate(
            "unstable",
            "unstable MSRV",
            vec![version("1.0.0", Some("1.70.0-nightly"))],
        ),
        krate(
            "buildmeta",
            "build MSRV",
            vec![version("1.0.0", Some("1.70.0+local"))],
        ),
    ])?;
    for (name, raw) in [
        ("unknown", None),
        ("unstable", Some("1.70.0-nightly")),
        ("buildmeta", Some("1.70.0+local")),
    ] {
        assert_eq!(
            eligible(repo.select(name, &CrateSearchFilters::default())?)?
                .selected_version
                .rust_version
                .as_deref(),
            raw
        );
        assert!(matches!(
            repo.select(name, &msrv("1.99")?)?,
            CrateSelection::FilteredOut
        ));
    }
    Ok(())
}

#[test]
fn msrv_numeric_comparison_handles_partial_patch_and_u64_maximum() -> TestResult {
    let repo = repository(&[
        krate(
            "minor",
            "version",
            vec![
                version("1.0.0", Some("1.9")),
                version("2.0.0", Some("1.10")),
            ],
        ),
        krate(
            "patch",
            "version",
            vec![
                version("1.0.0", Some("1.70")),
                version("2.0.0", Some("1.70.1")),
            ],
        ),
        krate(
            "maximum",
            "version",
            vec![version("1.0.0", Some("18446744073709551615.0"))],
        ),
    ])?;
    assert_eq!(
        eligible(repo.select("minor", &msrv("1.9")?)?)?
            .selected_version
            .version,
        "1.0.0"
    );
    assert_eq!(
        eligible(repo.select("patch", &msrv("1.70.0")?)?)?
            .selected_version
            .version,
        "1.0.0"
    );
    assert_eq!(
        eligible(repo.select("patch", &msrv("1.70.1")?)?)?
            .selected_version
            .version,
        "2.0.0"
    );
    assert!(matches!(
        repo.select("maximum", &msrv("18446744073709551614.99")?)?,
        CrateSelection::FilteredOut
    ));
    assert_eq!(
        eligible(repo.select("maximum", &msrv("18446744073709551615.0")?)?)?
            .selected_version
            .version,
        "1.0.0"
    );
    Ok(())
}

#[test]
fn only_selected_version_supplies_sorted_advisory_ids_and_scalar_facts() -> TestResult {
    let mut old = version("1.0.0", Some("1.60"));
    old.advisories = vec!["RUSTSEC-2023-0002".into(), "RUSTSEC-2023-0001".into()];
    old.license = Some("BSD-3-Clause".into());
    old.published_at = Some(42);
    let mut newer = version("2.0.0", Some("1.99"));
    newer.advisories = vec!["RUSTSEC-2024-9999".into()];
    let repo = repository(&[krate("advisory", "facts", vec![old, newer])])?;
    let facts = eligible(repo.select("advisory", &msrv("1.70")?)?)?;
    assert_eq!(
        facts.selected_version.known_advisory_ids,
        vec!["RUSTSEC-2023-0001", "RUSTSEC-2023-0002"]
    );
    assert_eq!(
        facts.selected_version.license.as_deref(),
        Some("BSD-3-Clause")
    );
    assert_eq!(facts.selected_version.published_at, Some(42));
    assert_eq!(facts.description, "facts");
    Ok(())
}

#[test]
fn missing_filtered_invalid_and_prerelease_only_identities_are_distinct() -> TestResult {
    let repo = repository(&[krate(
        "preview",
        "only preview",
        vec![version("1.0.0-alpha", None)],
    )])?;
    assert!(matches!(
        repo.select("absent", &CrateSearchFilters::default())?,
        CrateSelection::Missing
    ));
    assert!(matches!(
        repo.select("preview", &CrateSearchFilters::default())?,
        CrateSelection::FilteredOut
    ));
    assert!(matches!(
        repo.select("preview' OR 1=1", &CrateSearchFilters::default()),
        Err(CatalogError::InvalidInput)
    ));
    let filters = CrateSearchFilters {
        include_prerelease: true,
        ..Default::default()
    };
    assert!(
        eligible(repo.select("preview", &filters)?)?
            .latest_known_stable
            .is_none()
    );
    Ok(())
}

#[test]
fn real_bm25_orders_scores_then_names_and_honors_limit() -> TestResult {
    let repo = repository(&[
        krate("beta", "needle common", vec![version("1.0.0", None)]),
        krate("alpha", "needle common", vec![version("1.0.0", None)]),
        krate(
            "dense",
            "needle needle needle needle common",
            vec![version("1.0.0", None)],
        ),
    ])?;
    let rows = repo.lexical_candidates(&CatalogQuery::new("needle".into(), 50)?)?;
    assert_eq!(
        rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        vec!["dense", "alpha", "beta"]
    );
    assert!(rows.iter().all(|r| r.bm25.is_finite()));
    assert!(rows[0].bm25 < rows[1].bm25);
    assert_eq!(rows[1].bm25, rows[2].bm25);
    assert_eq!(
        repo.lexical_candidates(&CatalogQuery::new("needle".into(), 1)?)?
            .len(),
        1
    );
    Ok(())
}

#[test]
fn fts_terms_are_literal_and_never_expand_or_or_prefix_syntax() -> TestResult {
    let repo = repository(&[
        krate("literal", "alpha OR beta", vec![version("1.0.0", None)]),
        krate("without", "alpha beta", vec![version("1.0.0", None)]),
        krate("prefix", "alphabet", vec![version("1.0.0", None)]),
    ])?;
    let names = |query: &str| -> Result<Vec<String>, CatalogError> {
        Ok(repo
            .lexical_candidates(&CatalogQuery::new(query.into(), 50)?)?
            .into_iter()
            .map(|r| r.name)
            .collect())
    };
    assert_eq!(names("alpha OR beta")?, vec!["literal"]);
    assert_eq!(names("alpha\" OR \"beta")?, vec!["literal"]);
    assert!(names("alph*")?.is_empty());
    assert!(names("\"")?.is_empty());
    assert_eq!(names("alpha beta")?.len(), 2);
    Ok(())
}

#[test]
fn sixty_four_scalar_versions_are_supported_without_schema_change() -> TestResult {
    let versions = (0..64)
        .map(|minor| version(&format!("1.{minor}.0"), Some("1.70")))
        .collect::<Vec<_>>();
    let repo = repository(&[krate("bounded", "versions", versions.clone())])?;
    let selected = eligible(repo.select("bounded", &CrateSearchFilters::default())?)?;
    assert_eq!(selected.version_count, 64);
    assert_eq!(selected.selected_version.version, "1.63.0");
    let mut excessive = versions;
    excessive.push(version("2.0.0", None));
    assert!(repository(&[krate("bounded", "versions", excessive)]).is_err());
    Ok(())
}
