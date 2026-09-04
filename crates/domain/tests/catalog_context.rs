use rust_engineering_domain::{CatalogComponentUnavailable, Component};

#[test]
fn catalog_component_serialization_is_explicit_and_disjoint() -> Result<(), serde_json::Error> {
    assert_eq!(
        serde_json::to_value(Component::Available { value: 3u32 })?,
        serde_json::json!({"status":"available","value":3})
    );
    for (reason, name) in [
        (CatalogComponentUnavailable::NotConfigured, "not_configured"),
        (CatalogComponentUnavailable::Missing, "missing"),
        (CatalogComponentUnavailable::Invalid, "invalid"),
        (
            CatalogComponentUnavailable::IdentityMismatch,
            "identity_mismatch",
        ),
        (
            CatalogComponentUnavailable::UnsupportedPlatform,
            "unsupported_platform",
        ),
        (
            CatalogComponentUnavailable::FeatureDisabled,
            "feature_disabled",
        ),
        (CatalogComponentUnavailable::Denied, "denied"),
        (CatalogComponentUnavailable::IoUnavailable, "io_unavailable"),
        (CatalogComponentUnavailable::Budget, "budget"),
        (
            CatalogComponentUnavailable::DependencyUnavailable,
            "dependency_unavailable",
        ),
    ] {
        assert_eq!(
            serde_json::to_value(Component::<u32>::Unavailable { reason })?,
            serde_json::json!({"status":"unavailable","reason":name})
        );
    }
    Ok(())
}
