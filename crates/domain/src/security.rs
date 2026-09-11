//! Security policy and findings. No protocol, persistence, command or filesystem API.
use crate::SourceFingerprint;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SECURITY_MAX_FINDINGS: usize = 128;
pub const SECURITY_MAX_POLICY_BYTES: usize = 64 * 1024;
pub const DENY_DEFAULT_TIMEOUT_SECONDS: u64 = 120;
pub const DENY_MAX_TIMEOUT_SECONDS: u64 = 120;

/// The pinned plugin auto-loads these names even with an explicit config path.
/// This pure check can run before inspection starts any guest process.
pub fn security_source_has_exceptions(source: &crate::SourceBundle) -> bool {
    source
        .files()
        .iter()
        .map(|file| file.path())
        .chain(source.directories().iter().map(String::as_str))
        .any(|path| {
            matches!(
                path.rsplit('/').next(),
                Some("deny.exceptions.toml" | ".deny.exceptions.toml")
            )
        })
}

/// A project-supplied Cargo configuration file anywhere in the captured source.
///
/// G2 forbids project wrappers, linkers and runners, and Cargo's config is
/// exactly where a project would install one. It can also redirect
/// `source.crates-io` at a directory the project itself controls, which would
/// let the project substitute the dependency bytes an analysis or a measurement
/// is about to describe. The M5 flows refuse such a source rather than trying to
/// decide which settings are harmless: the environment they run in is closed and
/// belongs to the server.
///
/// This predicate is deliberately name-based and conservative. It proves nothing
/// about a project that ships no config; it only refuses the ones that do.
pub fn source_has_cargo_configuration(source: &crate::SourceBundle) -> bool {
    source.files().iter().map(|file| file.path()).any(|path| {
        let mut segments = path.rsplit('/');
        let name = segments.next();
        let parent = segments.next();
        parent == Some(".cargo") && matches!(name, Some("config.toml" | "config"))
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DenySelection")]
pub struct DenyOptions {
    timeout_seconds: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DenySelection {
    pub timeout_seconds: u64,
}
impl Default for DenySelection {
    fn default() -> Self {
        Self {
            timeout_seconds: DENY_DEFAULT_TIMEOUT_SECONDS,
        }
    }
}
impl TryFrom<DenySelection> for DenyOptions {
    type Error = crate::InvalidCheckOptions;
    fn try_from(selection: DenySelection) -> Result<Self, Self::Error> {
        if !(1..=DENY_MAX_TIMEOUT_SECONDS).contains(&selection.timeout_seconds) {
            return Err(crate::InvalidCheckOptions);
        }
        Ok(Self {
            timeout_seconds: selection.timeout_seconds,
        })
    }
}
impl DenyOptions {
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityEngine {
    Rustsec,
    Licenses,
    Bans,
    Sources,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecuritySource {
    Workspace,
    CratesIo,
    Unverified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityPackage {
    pub name: String,
    pub version: String,
    pub source: SecuritySource,
    pub source_fingerprint: Option<SourceFingerprint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityLint {
    Warn,
    Deny,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityBan {
    pub name: String,
    pub version_requirement: String,
}

/// Canonical field order is part of security-rules-v1. Adapters hash the serialized
/// owned value, not an independently reopened policy file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityRules {
    pub allowed_licenses: Vec<String>,
    pub banned_packages: Vec<SecurityBan>,
    pub multiple_versions: SecurityLint,
    pub wildcards: SecurityLint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecuritySuppression {
    pub id: String,
    pub engine: SecurityEngine,
    pub rule: String,
    pub package: String,
    pub package_source: SecuritySource,
    pub version_requirement: String,
    pub reason: String,
    pub owner: String,
    /// UTC Unix seconds. Equality with the observation time means expired.
    pub expires_at: u64,
    pub rules_digest: SourceFingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityPolicyDocument {
    pub schema_version: u32,
    pub rules: SecurityRules,
    pub suppressions: Vec<SecuritySuppression>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSecurityPolicy;
impl std::fmt::Display for InvalidSecurityPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid or expired security policy")
    }
}
impl std::error::Error for InvalidSecurityPolicy {}

#[derive(Clone, Debug)]
pub struct SecurityPolicy {
    document: SecurityPolicyDocument,
    fingerprint: SourceFingerprint,
    rules_digest: SourceFingerprint,
    requirements: Vec<semver::VersionReq>,
}

fn identifier(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

fn prose(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn requirement(
    value: &str,
    allow_universal: bool,
) -> Result<semver::VersionReq, InvalidSecurityPolicy> {
    if value.is_empty() || value.len() > 128 || value.bytes().any(|b| b.is_ascii_control()) {
        return Err(InvalidSecurityPolicy);
    }
    let parsed = semver::VersionReq::parse(value).map_err(|_| InvalidSecurityPolicy)?;
    if !allow_universal
        && (parsed.comparators.is_empty()
            || parsed.comparators.iter().all(|c| {
                c.op == semver::Op::GreaterEq
                    && c.major == 0
                    && c.minor.unwrap_or(0) == 0
                    && c.patch.unwrap_or(0) == 0
                    && c.pre.is_empty()
            }))
    {
        return Err(InvalidSecurityPolicy);
    }
    Ok(parsed)
}

impl SecurityPolicy {
    /// Integrity of bytes and rules_digest is established by the protected
    /// policy adapter. This constructor enforces the domain constraints before
    /// any execution. No caller can deserialize directly into a validated policy.
    pub fn new(
        document: SecurityPolicyDocument,
        fingerprint: SourceFingerprint,
        rules_digest: SourceFingerprint,
        now: u64,
    ) -> Result<Self, InvalidSecurityPolicy> {
        if document.schema_version != 1
            || document.rules.allowed_licenses.len() > 128
            || document.rules.banned_packages.len() > 128
            || document.suppressions.len() > 128
        {
            return Err(InvalidSecurityPolicy);
        }
        let mut licenses = BTreeSet::new();
        for license in &document.rules.allowed_licenses {
            // SPDX identifiers only; expressions, LicenseRef and arbitrary TOML
            // are not policy escape hatches. The pinned plugin validates IDs.
            if !identifier(license, 128)
                || license.starts_with("LicenseRef-")
                || !licenses.insert(license)
            {
                return Err(InvalidSecurityPolicy);
            }
        }
        let mut bans = BTreeSet::new();
        for ban in &document.rules.banned_packages {
            if !identifier(&ban.name, 64) || !bans.insert((&ban.name, &ban.version_requirement)) {
                return Err(InvalidSecurityPolicy);
            }
            requirement(&ban.version_requirement, true)?;
        }
        let mut ids = BTreeSet::new();
        let mut targets = BTreeSet::new();
        let mut requirements = Vec::new();
        for suppression in &document.suppressions {
            if !identifier(&suppression.id, 64)
                || !ids.insert(&suppression.id)
                || !identifier(&suppression.rule, 96)
                || !identifier(&suppression.package, 64)
                || suppression.package_source == SecuritySource::Unverified
                || !prose(&suppression.reason, 512)
                || !prose(&suppression.owner, 128)
                || suppression.expires_at <= now
                || suppression.rules_digest != rules_digest
                || !targets.insert((
                    suppression.engine,
                    &suppression.rule,
                    &suppression.package,
                    suppression.package_source,
                    &suppression.version_requirement,
                ))
            {
                return Err(InvalidSecurityPolicy);
            }
            if suppression.engine == SecurityEngine::Rustsec {
                let bytes = suppression.rule.as_bytes();
                if bytes.len() != 17
                    || !bytes.starts_with(b"RUSTSEC-")
                    || bytes[12] != b'-'
                    || !bytes[8..12]
                        .iter()
                        .chain(&bytes[13..])
                        .all(u8::is_ascii_digit)
                {
                    return Err(InvalidSecurityPolicy);
                }
            }
            requirements.push(requirement(&suppression.version_requirement, false)?);
        }
        Ok(Self {
            document,
            fingerprint,
            rules_digest,
            requirements,
        })
    }

    pub fn rules(&self) -> &SecurityRules {
        &self.document.rules
    }
    pub fn fingerprint(&self) -> &SourceFingerprint {
        &self.fingerprint
    }
    pub fn rules_digest(&self) -> &SourceFingerprint {
        &self.rules_digest
    }

    /// Recheck after execution as well: a policy which expires while a job runs
    /// cannot authorize suppression at publication time.
    pub fn validate_at(&self, now: u64) -> Result<(), InvalidSecurityPolicy> {
        if self
            .document
            .suppressions
            .iter()
            .any(|s| s.expires_at <= now)
        {
            return Err(InvalidSecurityPolicy);
        }
        Ok(())
    }

    pub fn suppression_for(
        &self,
        engine: SecurityEngine,
        rule: &str,
        package: &SecurityPackage,
        now: u64,
    ) -> Option<&SecuritySuppression> {
        if self.validate_at(now).is_err() {
            return None;
        }
        let version = semver::Version::parse(&package.version).ok()?;
        self.document
            .suppressions
            .iter()
            .zip(&self.requirements)
            .find(|(s, req)| {
                s.engine == engine
                    && s.rule == rule
                    && s.package == package.name
                    && s.package_source == package.source
                    && req.matches(&version)
            })
            .map(|(s, _)| s)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecuritySeverity {
    Error,
    Warning,
    Note,
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "suppression", rename_all = "snake_case")]
pub enum FindingDisposition {
    Active,
    Suppressed(SecuritySuppression),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SecurityFinding {
    pub engine: SecurityEngine,
    pub rule: String,
    /// None is explicit ambiguous/missing identity, never suppression-eligible.
    pub package: Option<SecurityPackage>,
    pub severity: SecuritySeverity,
    pub message: String,
    pub disposition: FindingDisposition,
}

impl SecurityFinding {
    pub fn apply_policy(&mut self, policy: &SecurityPolicy, now: u64) {
        self.disposition = self
            .package
            .as_ref()
            .and_then(|p| policy.suppression_for(self.engine, &self.rule, p, now))
            .map_or(FindingDisposition::Active, |s| {
                FindingDisposition::Suppressed(s.clone())
            });
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityCounts {
    pub errors: u32,
    pub warnings: u32,
    pub notes: u32,
    pub helps: u32,
}
impl SecurityCounts {
    pub fn total(self) -> u64 {
        u64::from(self.errors)
            + u64::from(self.warnings)
            + u64::from(self.notes)
            + u64::from(self.helps)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityCompleteness {
    Complete,
    Partial,
    Invalid,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecurityPolicyState {
    Satisfied,
    SatisfiedWithSuppressions,
    Violated,
    Undetermined,
}

/// Passing policy requires complete evidence; suppression cannot repair missing
/// data or a stale advisory snapshot. Retained findings never erase originals.
pub fn policy_state(
    completeness: SecurityCompleteness,
    findings: &[SecurityFinding],
) -> SecurityPolicyState {
    if findings.iter().any(|f| {
        f.severity == SecuritySeverity::Error && f.disposition == FindingDisposition::Active
    }) {
        return SecurityPolicyState::Violated;
    }
    if completeness != SecurityCompleteness::Complete {
        return SecurityPolicyState::Undetermined;
    }
    if findings
        .iter()
        .any(|f| matches!(f.disposition, FindingDisposition::Suppressed(_)))
    {
        SecurityPolicyState::SatisfiedWithSuppressions
    } else {
        SecurityPolicyState::Satisfied
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Malformed fixed fixtures must fail the test immediately.
mod tests {
    use super::*;

    fn digest() -> SourceFingerprint {
        // Literal fixture digest has the domain's exact SHA-256 grammar.
        format!("sha256:{}", "a".repeat(64)).parse().unwrap()
    }
    fn document() -> SecurityPolicyDocument {
        SecurityPolicyDocument {
            schema_version: 1,
            rules: SecurityRules {
                allowed_licenses: vec!["MIT".into(), "Apache-2.0".into()],
                banned_packages: vec![],
                multiple_versions: SecurityLint::Deny,
                wildcards: SecurityLint::Deny,
            },
            suppressions: vec![SecuritySuppression {
                id: "approved-temporary-exception".into(),
                engine: SecurityEngine::Rustsec,
                rule: "RUSTSEC-2026-0001".into(),
                package: "affected-crate".into(),
                package_source: SecuritySource::CratesIo,
                version_requirement: ">=1.2.0, <1.3.0".into(),
                reason: "Pending migration with tracked owner".into(),
                owner: "security-team".into(),
                expires_at: 200,
                rules_digest: digest(),
            }],
        }
    }
    fn policy() -> SecurityPolicy {
        SecurityPolicy::new(document(), digest(), digest(), 100).unwrap()
    }
    fn package() -> SecurityPackage {
        SecurityPackage {
            name: "affected-crate".into(),
            version: "1.2.5".into(),
            source: SecuritySource::CratesIo,
            source_fingerprint: None,
        }
    }
    fn finding() -> SecurityFinding {
        SecurityFinding {
            engine: SecurityEngine::Rustsec,
            rule: "RUSTSEC-2026-0001".into(),
            package: Some(package()),
            severity: SecuritySeverity::Error,
            message: "Original advisory remains visible".into(),
            disposition: FindingDisposition::Active,
        }
    }

    #[test]
    fn banning_every_version_is_restrictive_but_universal_suppression_is_rejected() {
        let mut doc = document();
        doc.rules.banned_packages.push(SecurityBan {
            name: "affected-crate".into(),
            version_requirement: "*".into(),
        });
        assert!(SecurityPolicy::new(doc.clone(), digest(), digest(), 100).is_ok());
        doc.suppressions[0].version_requirement = "*".into();
        assert!(SecurityPolicy::new(doc, digest(), digest(), 100).is_err());
    }
    #[test]
    fn suppression_matches_every_identity_dimension_and_expires_at_boundary() {
        let policy = policy();
        assert!(
            policy
                .suppression_for(
                    SecurityEngine::Rustsec,
                    "RUSTSEC-2026-0001",
                    &package(),
                    199
                )
                .is_some()
        );
        for now in [200, 201, u64::MAX] {
            assert!(policy.validate_at(now).is_err());
            assert!(
                policy
                    .suppression_for(
                        SecurityEngine::Rustsec,
                        "RUSTSEC-2026-0001",
                        &package(),
                        now
                    )
                    .is_none()
            );
        }
        for changed in [
            SecurityPackage {
                name: "different".into(),
                ..package()
            },
            SecurityPackage {
                version: "1.3.0".into(),
                ..package()
            },
            SecurityPackage {
                version: "1.2.5-alpha.1".into(),
                ..package()
            },
            SecurityPackage {
                version: "invalid".into(),
                ..package()
            },
            SecurityPackage {
                source: SecuritySource::Workspace,
                ..package()
            },
            SecurityPackage {
                source: SecuritySource::Unverified,
                ..package()
            },
        ] {
            assert!(
                policy
                    .suppression_for(SecurityEngine::Rustsec, "RUSTSEC-2026-0001", &changed, 100)
                    .is_none()
            );
        }
        assert!(
            policy
                .suppression_for(SecurityEngine::Bans, "RUSTSEC-2026-0001", &package(), 100)
                .is_none()
        );
        assert!(
            policy
                .suppression_for(
                    SecurityEngine::Rustsec,
                    "RUSTSEC-2026-0002",
                    &package(),
                    100
                )
                .is_none()
        );
    }

    #[test]
    fn malformed_global_duplicate_and_expired_policies_are_rejected_before_execution() {
        let mutations: &[fn(&mut SecurityPolicyDocument)] = &[
            |d| d.schema_version = 2,
            |d| d.rules.allowed_licenses.push("MIT".into()),
            |d| d.rules.allowed_licenses = vec!["MIT OR Apache-2.0".into()],
            |d| d.rules.allowed_licenses = vec!["LicenseRef-custom".into()],
            |d| d.suppressions[0].rule = "*".into(),
            |d| d.suppressions[0].rule = "RUSTSEC-XXXX-0001".into(),
            |d| d.suppressions[0].package = "*".into(),
            |d| d.suppressions[0].version_requirement = "*".into(),
            |d| d.suppressions[0].version_requirement = ">=0.0.0".into(),
            |d| d.suppressions[0].version_requirement = "invalid".into(),
            |d| d.suppressions[0].owner = "  ".into(),
            |d| d.suppressions[0].reason = "bad\nreason".into(),
            |d| d.suppressions[0].expires_at = 100,
            |d| d.suppressions[0].package_source = SecuritySource::Unverified,
            |d| {
                d.suppressions[0].rules_digest =
                    format!("sha256:{}", "b".repeat(64)).parse().unwrap()
            },
            |d| d.suppressions.push(d.suppressions[0].clone()),
            |d| {
                let mut duplicate = d.suppressions[0].clone();
                duplicate.id = "other-id".into();
                d.suppressions.push(duplicate);
            },
        ];
        for (index, mutate) in mutations.iter().enumerate() {
            let mut doc = document();
            mutate(&mut doc);
            assert!(
                SecurityPolicy::new(doc, digest(), digest(), 100).is_err(),
                "case {index}"
            );
        }
        let mut strict = document();
        strict.rules.allowed_licenses.clear();
        strict.suppressions.clear();
        assert!(SecurityPolicy::new(strict, digest(), digest(), 100).is_ok());
    }

    #[test]
    fn suppressed_findings_preserve_original_and_cannot_repair_incomplete_evidence() {
        let policy = policy();
        let mut row = finding();
        let original = row.clone();
        row.apply_policy(&policy, 100);
        assert!(matches!(row.disposition, FindingDisposition::Suppressed(_)));
        assert_eq!(row.message, original.message);
        assert_eq!(row.severity, original.severity);
        assert_eq!(row.package, original.package);
        assert_eq!(
            policy_state(SecurityCompleteness::Complete, &[row.clone()]),
            SecurityPolicyState::SatisfiedWithSuppressions
        );
        for partial in [
            SecurityCompleteness::Partial,
            SecurityCompleteness::Invalid,
            SecurityCompleteness::Unavailable,
        ] {
            assert_eq!(
                policy_state(partial, &[row.clone()]),
                SecurityPolicyState::Undetermined
            );
            assert_eq!(
                policy_state(partial, std::slice::from_ref(&original)),
                SecurityPolicyState::Violated
            );
        }
        row.apply_policy(&policy, 200);
        assert_eq!(row.disposition, FindingDisposition::Active);
        let mut ambiguous = finding();
        ambiguous.package = None;
        ambiguous.apply_policy(&policy, 100);
        assert_eq!(ambiguous.disposition, FindingDisposition::Active);
    }

    #[test]
    fn policy_document_is_closed_and_cannot_accept_arbitrary_commands() {
        let mut json = serde_json::to_value(document()).unwrap();
        json["command"] = serde_json::json!("cargo deny");
        assert!(serde_json::from_value::<SecurityPolicyDocument>(json).is_err());
        let mut json = serde_json::to_value(document()).unwrap();
        json["rules"]["exceptions"] = serde_json::json!([]);
        assert!(serde_json::from_value::<SecurityPolicyDocument>(json).is_err());
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake.
mod cargo_configuration_tests {
    use crate::{SourceBundle, SourceFile, security::source_has_cargo_configuration};

    fn bundle(paths: &[&str]) -> SourceBundle {
        let files = paths
            .iter()
            .map(|path| SourceFile::new((*path).into(), b"x".to_vec()).expect("file"))
            .collect::<Vec<_>>();
        SourceBundle::new(files).expect("bundle")
    }

    #[test]
    fn a_project_cargo_configuration_is_detected_wherever_it_sits() {
        for path in [
            ".cargo/config.toml",
            ".cargo/config",
            "crates/inner/.cargo/config.toml",
            "a/b/c/.cargo/config",
        ] {
            assert!(
                source_has_cargo_configuration(&bundle(&[path])),
                "missed {path}"
            );
        }
    }

    #[test]
    fn unrelated_files_are_not_mistaken_for_one() {
        for path in [
            "Cargo.toml",
            "config.toml",
            "src/config.toml",
            "cargo/config.toml",
            "docs/.cargo-config.toml",
            ".cargo/audit.toml",
            ".cargo/config.toml.bak",
        ] {
            assert!(
                !source_has_cargo_configuration(&bundle(&[path])),
                "false positive on {path}"
            );
        }
    }
}
