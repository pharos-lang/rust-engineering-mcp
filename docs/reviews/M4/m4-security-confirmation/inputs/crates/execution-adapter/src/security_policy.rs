//! Parse the exact protected policy bytes and render only the fixed cargo-deny subset.
use rust_engineering_domain::security::{
    SECURITY_MAX_POLICY_BYTES, SecurityPolicy, SecurityPolicyDocument, SecurityRules,
};
use rust_engineering_domain::{SourceBundle, SourceFingerprint};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecurityPolicyError {
    Integrity,
    Invalid,
    Budget,
    ProjectExceptions,
}

/// The caller must acquire `bytes` with the host snapshot no-follow adapter.
/// Expected digest and parsed policy refer to this same buffer throughout.
pub fn parse_security_policy(
    bytes: &[u8],
    expected: &SourceFingerprint,
    now: u64,
) -> Result<SecurityPolicy, SecurityPolicyError> {
    if bytes.len() > SECURITY_MAX_POLICY_BYTES {
        return Err(SecurityPolicyError::Budget);
    }
    if crate::digest(bytes) != expected.to_string() {
        return Err(SecurityPolicyError::Integrity);
    }
    let document: SecurityPolicyDocument =
        serde_json::from_slice(bytes).map_err(|_| SecurityPolicyError::Invalid)?;
    #[derive(Serialize)]
    struct CanonicalRules<'a> {
        schema: &'static str,
        rules: &'a SecurityRules,
    }
    let canonical = serde_json::to_vec(&CanonicalRules {
        schema: "security-rules-v1",
        rules: &document.rules,
    })
    .map_err(|_| SecurityPolicyError::Invalid)?;
    let rules_digest = crate::digest(&canonical)
        .parse()
        .map_err(|_| SecurityPolicyError::Invalid)?;
    SecurityPolicy::new(document, expected.clone(), rules_digest, now)
        .map_err(|_| SecurityPolicyError::Invalid)
}

/// cargo-deny auto-loads these paths independently of --config. Reject them
/// before any guest process can read project-controlled exceptions.
pub(super) fn reject_project_exceptions(source: &SourceBundle) -> Result<(), SecurityPolicyError> {
    if rust_engineering_domain::security::security_source_has_exceptions(source) {
        return Err(SecurityPolicyError::ProjectExceptions);
    }
    Ok(())
}

pub(super) fn deny_config(policy: &SecurityPolicy) -> Result<Vec<u8>, SecurityPolicyError> {
    // Only validated ASCII identifiers and semver requirements are interpolated;
    // JSON string quoting is also valid TOML basic-string quoting for this set.
    let rules = policy.rules();
    let quoted =
        |value: &str| serde_json::to_string(value).map_err(|_| SecurityPolicyError::Invalid);
    let licenses =
        serde_json::to_string(&rules.allowed_licenses).map_err(|_| SecurityPolicyError::Invalid)?;
    let bans = rules
        .banned_packages
        .iter()
        .map(|b| {
            Ok(format!(
                "{{ name = {}, version = {} }}",
                quoted(&b.name)?,
                quoted(&b.version_requirement)?
            ))
        })
        .collect::<Result<Vec<_>, SecurityPolicyError>>()?
        .join(", ");
    let lint = |v| match v {
        rust_engineering_domain::security::SecurityLint::Warn => "warn",
        rust_engineering_domain::security::SecurityLint::Deny => "deny",
    };
    Ok(format!(
        r#"[graph]
all-features = false
no-default-features = false
exclude-dev = false
targets = []
[output]
feature-depth = 0
[licenses]
allow = {licenses}
confidence-threshold = 0.95
include-dev = true
include-build = true
[licenses.private]
ignore = false
ignore-sources = []
registries = []
[bans]
multiple-versions = "{}"
wildcards = "{}"
allow-workspace = false
deny = [{bans}]
[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
"#,
        lint(rules.multiple_versions),
        lint(rules.wildcards)
    )
    .into_bytes())
}

pub(super) const SECURITY_CARGO_CONFIG: &[u8] = br#"[net]
offline = true
[source.crates-io]
replace-with = "rust-mcp-vendor"
[source.rust-mcp-vendor]
directory = "/rust-mcp-vendor"
"#;

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed parser fixtures fail immediately when their construction is invalid.
mod tests {
    use super::*;
    use rust_engineering_domain::SourceFile;
    const POLICY: &[u8] = br#"{"schema_version":1,"rules":{"allowed_licenses":["MIT"],"banned_packages":[{"name":"bad-crate","version_requirement":"=1.2.3"}],"multiple_versions":"deny","wildcards":"deny"},"suppressions":[]}"#;

    #[test]
    fn exact_bytes_closed_json_and_duplicate_fields_cannot_bypass_integrity() {
        let expected = crate::digest(POLICY).parse().unwrap();
        assert!(parse_security_policy(POLICY, &expected, 100).is_ok());
        let changed = [POLICY, b" "].concat();
        assert!(matches!(
            parse_security_policy(&changed, &expected, 100),
            Err(SecurityPolicyError::Integrity)
        ));
        for bad in [
            String::from_utf8_lossy(POLICY).replace(
                "\"schema_version\":1",
                "\"schema_version\":1,\"schema_version\":1",
            ),
            String::from_utf8_lossy(POLICY).replace("\"rules\":", "\"command\":\"sh\",\"rules\":"),
            String::from_utf8_lossy(POLICY).replace("\"MIT\"", "\"MIT\\\"\\n[advisories]\""),
        ] {
            let hash = crate::digest(bad.as_bytes()).parse().unwrap();
            assert!(matches!(
                parse_security_policy(bad.as_bytes(), &hash, 100),
                Err(SecurityPolicyError::Invalid)
            ));
        }
        assert!(matches!(
            parse_security_policy(&vec![b' '; SECURITY_MAX_POLICY_BYTES + 1], &expected, 100),
            Err(SecurityPolicyError::Budget)
        ));
    }

    #[test]
    fn generated_config_cannot_ignore_dev_build_private_or_unknown_sources() {
        let policy =
            parse_security_policy(POLICY, &crate::digest(POLICY).parse().unwrap(), 100).unwrap();
        let config: toml::Value =
            toml::from_str(std::str::from_utf8(&deny_config(&policy).unwrap()).unwrap()).unwrap();
        assert_eq!(config["licenses"]["include-dev"].as_bool(), Some(true));
        assert_eq!(config["licenses"]["include-build"].as_bool(), Some(true));
        assert_eq!(
            config["licenses"]["private"]["ignore"].as_bool(),
            Some(false)
        );
        assert_eq!(
            config["licenses"]["confidence-threshold"].as_float(),
            Some(0.95)
        );
        assert_eq!(config["sources"]["unknown-git"].as_str(), Some("deny"));
        assert_eq!(config["sources"]["unknown-registry"].as_str(), Some("deny"));
        assert!(config.get("advisories").is_none());
        assert!(config["licenses"].get("exceptions").is_none());
        assert!(config["licenses"].get("clarify").is_none());
        assert_eq!(
            config["bans"]["deny"][0]["version"].as_str(),
            Some("=1.2.3")
        );
        let home: toml::Value =
            toml::from_str(std::str::from_utf8(SECURITY_CARGO_CONFIG).unwrap()).unwrap();
        assert_eq!(home["net"]["offline"].as_bool(), Some(true));
    }

    #[test]
    fn auto_loaded_exception_names_are_rejected_at_all_depths_before_execution() {
        for name in [
            "deny.exceptions.toml",
            ".deny.exceptions.toml",
            ".cargo/deny.exceptions.toml",
            "member/.cargo/deny.exceptions.toml",
        ] {
            let source = SourceBundle::new(vec![
                SourceFile::new(name.into(), b"[licenses]".to_vec()).unwrap(),
            ])
            .unwrap();
            assert_eq!(
                reject_project_exceptions(&source),
                Err(SecurityPolicyError::ProjectExceptions)
            );
        }
        let source = SourceBundle::new(vec![
            SourceFile::new("deny.toml".into(), b"ignored project policy".to_vec()).unwrap(),
        ])
        .unwrap();
        assert!(reject_project_exceptions(&source).is_ok());
    }
}
