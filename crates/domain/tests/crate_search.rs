use rust_engineering_domain::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn msrv_is_canonical_bounded_and_numeric() -> TestResult {
    let short = MsrvVersion::parse("1.90")?;
    assert_eq!(short.components(), (1, 90, 0));
    assert_eq!(short, MsrvVersion::parse("1.90.0")?);
    assert_eq!(serde_json::to_string(&short)?, "\"1.90.0\"");
    assert!(MsrvVersion::parse("1.9")? < MsrvVersion::parse("1.10")?);
    assert_eq!(
        MsrvVersion::parse("18446744073709551615.0")?.components(),
        (u64::MAX, 0, 0)
    );
    for bad in [
        "",
        "1",
        "1.2.3.4",
        "01.2",
        "1.02",
        "1.2.00",
        "1.2-rc",
        "1.2.3+build",
        "1.2.",
        " 1.2",
        "1.2\n",
        "+1.2",
        "١.2",
        "18446744073709551616.0",
        "18446744073709551615.18446744073709551615",
    ] {
        assert_eq!(
            MsrvVersion::parse(bad),
            Err(CatalogError::InvalidInput),
            "{bad:?}"
        );
        assert!(serde_json::from_str::<MsrvVersion>(&serde_json::to_string(bad)?).is_err());
    }
    Ok(())
}
#[test]
fn filters_and_modes_use_closed_serialized_contracts() -> TestResult {
    let filters: CrateSearchFilters = serde_json::from_str("{}")?;
    assert_eq!(filters, CrateSearchFilters::default());
    assert_eq!(CrateSearchMode::default(), CrateSearchMode::Hybrid);
    for (raw, mode) in [
        ("lexical", CrateSearchMode::Lexical),
        ("semantic", CrateSearchMode::Semantic),
        ("hybrid", CrateSearchMode::Hybrid),
    ] {
        assert_eq!(
            serde_json::from_str::<CrateSearchMode>(&format!("\"{raw}\""))?,
            mode
        );
    }
    assert!(serde_json::from_str::<CrateSearchFilters>("{\"network\":true}").is_err());
    assert!(serde_json::from_str::<CrateSearchFilters>("{\"msrv_lte\":\"1.2-beta\"}").is_err());
    Ok(())
}
