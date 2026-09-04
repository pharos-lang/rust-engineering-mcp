use rust_engineering_domain::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn request() -> CrateInspectRequest {
    CrateInspectRequest {
        name: "serde".into(),
        section: InspectSection::Overview,
        version: None,
        limit: 20,
        offset: 0,
        snapshot_fingerprint: None,
    }
}
#[test]
fn request_shape_bounds_and_section_rules_are_enforced() -> TestResult {
    request().validate()?;
    for section in [
        InspectSection::Features,
        InspectSection::Dependencies,
        InspectSection::Advisories,
    ] {
        let mut value = request();
        value.section = section;
        assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
        value.version = Some("1.0.0".into());
        value.validate()?;
        value.offset = 1;
        assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
        value.snapshot_fingerprint = Some(format!("sha256:{:064x}", 1).parse()?);
        value.offset = 128;
        value.limit = 50;
        value.validate()?;
        value.offset = 129;
        assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
    }
    for bad in ["", "a/b", "a.b", "ü", "a b", "a\0b"] {
        let mut value = request();
        value.name = bad.into();
        assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
    }
    let mut value = request();
    value.name = "a".repeat(64);
    value.validate()?;
    value.name.push('a');
    assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
    for limit in [0, 51, u32::MAX] {
        let mut value = request();
        value.limit = limit;
        assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
    }
    for bad in [String::new(), "a".repeat(129), "1.0.0\n".into()] {
        let mut value = request();
        value.version = Some(bad);
        assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
    }
    let mut value = request();
    value.version = Some("not-semver".into());
    value.validate()?; // Exact grammar intentionally remains at the existing adapter boundary.
    value.section = InspectSection::Versions;
    assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
    value = request();
    value.offset = 1;
    value.snapshot_fingerprint = Some(format!("sha256:{:064x}", 1).parse()?);
    assert_eq!(value.validate(), Err(CatalogError::InvalidInput));
    Ok(())
}
#[test]
fn unknown_coverage_and_missing_outcomes_have_disjoint_wire_shapes() -> TestResult {
    assert_eq!(InspectSection::default(), InspectSection::Overview);
    assert_eq!(
        serde_json::from_str::<InspectSection>("\"advisories\"")?,
        InspectSection::Advisories
    );
    assert!(serde_json::from_str::<InspectSection>("\"all\"").is_err());
    assert_eq!(
        serde_json::to_value(InspectUnknown::default())?,
        serde_json::json!({"status":"unknown","reason":"not_recorded_in_snapshot"})
    );
    assert_eq!(
        serde_json::to_value(InspectLookup::CrateNotFound)?,
        serde_json::json!({"kind":"crate_not_found"})
    );
    assert_eq!(
        serde_json::to_value(InspectLookup::VersionNotFound)?,
        serde_json::json!({"kind":"version_not_found"})
    );
    assert_eq!(
        serde_json::to_value(InspectPageData::Overview {
            selected_version: None
        })?,
        serde_json::json!({"section":"overview","selected_version":null})
    );
    Ok(())
}
