use rust_engineering_domain::{
    DependencyName, DependencyTarget, FeatureName, FeatureValue, ManifestEditError,
    ProfileCodegenUnits,
};

#[test]
fn cargo_names_are_ascii_bounded_and_nonempty() -> Result<(), ManifestEditError> {
    assert_eq!(
        FeatureName::new("fast-path.v2+simd".to_owned())?.as_str(),
        "fast-path.v2+simd"
    );
    assert_eq!(
        DependencyName::new("serde_json".to_owned())?.as_str(),
        "serde_json"
    );
    for invalid in ["", "-leading", "snowman-☃", &"x".repeat(129)] {
        assert_eq!(
            FeatureName::new(invalid.to_owned()),
            Err(ManifestEditError::InvalidOperation)
        );
    }
    for invalid in ["", ".leading", "crate+fork", "crate/name"] {
        assert_eq!(
            DependencyName::new(invalid.to_owned()),
            Err(ManifestEditError::InvalidOperation)
        );
    }
    Ok(())
}

#[test]
fn feature_values_cover_the_closed_cargo_forms() -> Result<(), ManifestEditError> {
    for valid in ["std", "dep:serde", "serde/derive", "serde?/derive"] {
        assert_eq!(FeatureValue::new(valid.to_owned())?.as_str(), valid);
    }
    for invalid in [
        "dep:",
        "dep:serde/derive",
        "serde/",
        "/derive",
        "serde??/derive",
        "serde/derive/extra",
    ] {
        assert_eq!(
            FeatureValue::new(invalid.to_owned()),
            Err(ManifestEditError::InvalidOperation),
            "{invalid}"
        );
    }
    Ok(())
}

#[test]
fn dependency_targets_are_data_not_paths_or_arguments() -> Result<(), ManifestEditError> {
    for valid in [
        "x86_64-unknown-linux-gnu",
        "cfg(unix)",
        "cfg(all(unix, target_arch = \"aarch64\"))",
    ] {
        assert_eq!(DependencyTarget::new(valid.to_owned())?.as_str(), valid);
    }
    for invalid in [
        "",
        "../Cargo.toml",
        "..",
        "-not-a-triple",
        "/absolute",
        "--target evil",
        "cfg()",
        "cfg(unix)\n[dependencies]",
    ] {
        assert_eq!(
            DependencyTarget::new(invalid.to_owned()),
            Err(ManifestEditError::InvalidOperation),
            "{invalid:?}"
        );
    }
    Ok(())
}

#[test]
fn codegen_units_are_nonzero_and_bounded_by_the_type() -> Result<(), ManifestEditError> {
    assert_eq!(ProfileCodegenUnits::new(1)?.get(), 1);
    assert_eq!(ProfileCodegenUnits::new(u32::MAX)?.get(), u32::MAX);
    assert_eq!(
        ProfileCodegenUnits::new(0),
        Err(ManifestEditError::InvalidOperation)
    );
    Ok(())
}
