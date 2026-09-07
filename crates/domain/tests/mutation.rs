use rust_engineering_domain::{
    IdempotencyKey, LintName, ManifestEditError, MutationError, MutationId,
};

#[test]
fn mutation_ids_are_opaque_canonical_128_bit_hex_values() {
    let valid = format!("mut_{}", "0123456789abcdef".repeat(2));
    assert_eq!(
        MutationId::new(valid.clone()).map(|id| id.as_str().to_owned()),
        Ok(valid)
    );

    for invalid in [
        "".to_owned(),
        "mut_0123".to_owned(),
        format!("mut_{}", "A".repeat(32)),
        format!("prj_{}", "0".repeat(32)),
        format!("mut_{}", "0".repeat(33)),
        format!("mut_{}", "g".repeat(32)),
        format!("mut_{}\n", "0".repeat(32)),
    ] {
        assert_eq!(MutationId::new(invalid), Err(MutationError::Invalid));
    }
}

#[test]
fn idempotency_keys_accept_only_bounded_opaque_ascii_tokens() {
    for valid in ["a", "Request_01-retry", &"x".repeat(64)] {
        assert_eq!(
            IdempotencyKey::new(valid.to_owned()).map(|key| key.as_str().to_owned()),
            Ok(valid.to_owned())
        );
    }

    for invalid in [
        "".to_owned(),
        "x".repeat(65),
        "has space".to_owned(),
        "path/name".to_owned(),
        "café".to_owned(),
        "line\nbreak".to_owned(),
    ] {
        assert_eq!(IdempotencyKey::new(invalid), Err(MutationError::Invalid));
    }
}

#[test]
fn lint_names_are_bounded_ascii_identifiers_not_toml_paths() {
    for valid in ["a", "unsafe_code", "ClippyLint123", &"x".repeat(128)] {
        assert_eq!(
            LintName::new(valid.to_owned()).map(|name| name.as_str().to_owned()),
            Ok(valid.to_owned())
        );
    }

    for invalid in [
        "".to_owned(),
        "x".repeat(129),
        "clippy::pedantic".to_owned(),
        "workspace.lints".to_owned(),
        "unsafe-code".to_owned(),
        "café".to_owned(),
        "lint\nname".to_owned(),
    ] {
        assert_eq!(
            LintName::new(invalid),
            Err(ManifestEditError::InvalidOperation)
        );
    }
}
