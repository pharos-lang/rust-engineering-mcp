//! Captured Cargo.lock and manifest facts for the bounded supply-chain slice.
//!
//! Source locators are classified and hashed from their exact bytes, but never
//! copied into the returned graph. Checksum verification additionally requires
//! a deny observation bound to the same source, lock and vendor snapshot.

use rust_engineering_application::InspectionControl;
use rust_engineering_application::security::{DenyObservation, SecurityError};
use rust_engineering_domain::security::{SecurityPackage, SecuritySource};
use rust_engineering_domain::supply_chain::{SupplyGraph, SupplyPackage, SupplySource, YankedFact};
use rust_engineering_domain::{CargoVendorSnapshot, SourceBundle, SourceFingerprint};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

const MAX_PACKAGES: usize = 4_096;
const MAX_NAME_BYTES: usize = 64;
const MAX_VERSION_BYTES: usize = 128;
const MAX_SOURCE_BYTES: usize = 4_096;
const MAX_FEATURES: usize = 256;
const MAX_FEATURE_BYTES: usize = 128;

#[derive(Deserialize)]
struct LockFile {
    version: u32,
    #[serde(default)]
    package: Vec<LockedPackage>,
}

#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

struct LockPackage {
    name: String,
    version: String,
    source: SupplySource,
    source_fingerprint: Option<SourceFingerprint>,
    declared_checksum: Option<SourceFingerprint>,
    source_literal: Option<String>,
}

type Identity = (String, String, u8);

fn invalid() -> SecurityError {
    SecurityError::InvalidMetadata
}

fn fingerprint(bytes: &[u8]) -> Result<SourceFingerprint, SecurityError> {
    crate::digest(bytes).parse().map_err(|_| invalid())
}

fn file<'a>(source: &'a SourceBundle, path: &str) -> Option<&'a [u8]> {
    source
        .files()
        .binary_search_by(|entry| entry.path().cmp(path))
        .ok()
        .map(|index| source.files()[index].bytes())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_feature(feature: &str) -> bool {
    !feature.is_empty()
        && feature.len() <= MAX_FEATURE_BYTES
        && !feature.chars().any(char::is_control)
}

fn checksum(value: &str) -> Result<SourceFingerprint, SecurityError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid());
    }
    format!("sha256:{value}").parse().map_err(|_| invalid())
}

fn source_kind(source: Option<&str>) -> SupplySource {
    match source {
        None => SupplySource::Workspace,
        Some(
            "registry+https://github.com/rust-lang/crates.io-index"
            | "registry+https://index.crates.io/",
        ) => SupplySource::CratesIo,
        Some(value) if value.starts_with("registry+") => SupplySource::Registry,
        Some(value) if value.starts_with("git+") => SupplySource::Git,
        Some(_) => SupplySource::Unverified,
    }
}

fn source_rank(source: SupplySource) -> u8 {
    match source {
        SupplySource::Workspace => 0,
        SupplySource::CratesIo => 1,
        SupplySource::Registry => 2,
        SupplySource::Git => 3,
        SupplySource::Unverified => 4,
    }
}

fn parse_lock(bytes: &[u8]) -> Result<Vec<LockPackage>, SecurityError> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let lock: LockFile = toml::from_str(text).map_err(|_| invalid())?;
    if !(3..=4).contains(&lock.version) {
        return Err(invalid());
    }
    if lock.package.len() > MAX_PACKAGES {
        return Err(SecurityError::OutputLimit);
    }

    let mut exact = BTreeSet::new();
    let mut packages = Vec::with_capacity(lock.package.len());
    for package in lock.package {
        if !valid_name(&package.name)
            || package.version.len() > MAX_VERSION_BYTES
            || semver::Version::parse(&package.version).is_err()
        {
            return Err(invalid());
        }
        if package.source.as_ref().is_some_and(|source| {
            source.is_empty()
                || source.len() > MAX_SOURCE_BYTES
                || source.chars().any(char::is_control)
        }) {
            return Err(invalid());
        }
        if !exact.insert((
            package.name.clone(),
            package.version.clone(),
            package.source.clone(),
        )) {
            return Err(invalid());
        }
        let kind = source_kind(package.source.as_deref());
        let declared_checksum = package.checksum.as_deref().map(checksum).transpose()?;
        if kind == SupplySource::Workspace && declared_checksum.is_some() {
            return Err(invalid());
        }
        let source_fingerprint = package
            .source
            .as_deref()
            .map(str::as_bytes)
            .map(fingerprint)
            .transpose()?;
        packages.push(LockPackage {
            name: package.name,
            version: package.version,
            source: kind,
            source_fingerprint,
            declared_checksum,
            source_literal: package.source,
        });
    }
    packages.sort_by(|left, right| {
        (
            &left.name,
            &left.version,
            source_rank(left.source),
            &left.source_literal,
        )
            .cmp(&(
                &right.name,
                &right.version,
                source_rank(right.source),
                &right.source_literal,
            ))
    });
    Ok(packages)
}

fn manifest_features(
    bytes: &[u8],
    inherited_version: Option<&str>,
) -> Result<Option<(String, String, Vec<String>)>, SecurityError> {
    let manifest: toml::Value = toml::from_str(std::str::from_utf8(bytes).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let root = manifest.as_table().ok_or_else(invalid)?;
    let Some(package) = root.get("package") else {
        return Ok(None);
    };
    let package = package.as_table().ok_or_else(invalid)?;
    let name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .filter(|name| valid_name(name))
        .ok_or_else(invalid)?;
    let version = match package.get("version") {
        Some(toml::Value::String(version)) => version.as_str(),
        Some(toml::Value::Table(inherited))
            if inherited.len() == 1
                && inherited.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
        {
            inherited_version.ok_or_else(invalid)?
        }
        _ => return Err(invalid()),
    };
    if version.len() > MAX_VERSION_BYTES || semver::Version::parse(version).is_err() {
        return Err(invalid());
    }
    let mut features = Vec::new();
    if let Some(table) = root.get("features") {
        let table = table.as_table().ok_or_else(invalid)?;
        if table.len() > MAX_FEATURES {
            return Err(SecurityError::OutputLimit);
        }
        for (feature, members) in table {
            if !valid_feature(feature) {
                return Err(invalid());
            }
            let members = members.as_array().ok_or_else(invalid)?;
            if members.len() > MAX_FEATURES
                || members.iter().any(|member| {
                    member.as_str().is_none_or(|member| {
                        member.is_empty()
                            || member.len() > MAX_FEATURE_BYTES
                            || member.chars().any(char::is_control)
                    })
                })
            {
                return Err(invalid());
            }
            features.push(feature.clone());
        }
    }
    Ok(Some((name.to_owned(), version.to_owned(), features)))
}

fn workspace_inherited_version(source: &SourceBundle) -> Result<Option<String>, SecurityError> {
    let Some(bytes) = file(source, "Cargo.toml") else {
        return Ok(None);
    };
    let manifest: toml::Value = toml::from_str(std::str::from_utf8(bytes).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let version = manifest
        .get("workspace")
        .and_then(toml::Value::as_table)
        .and_then(|workspace| workspace.get("package"))
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str);
    if version.is_some_and(|version| {
        version.len() > MAX_VERSION_BYTES || semver::Version::parse(version).is_err()
    }) {
        return Err(invalid());
    }
    Ok(version.map(str::to_owned))
}

fn workspace_features(
    source: &SourceBundle,
    lock: &[LockPackage],
    control: &dyn InspectionControl,
) -> Result<BTreeMap<(String, String), Vec<String>>, SecurityError> {
    let identities = lock
        .iter()
        .filter(|package| package.source == SupplySource::Workspace)
        .map(|package| (package.name.clone(), package.version.clone()))
        .collect::<BTreeSet<_>>();
    let inherited = workspace_inherited_version(source)?;
    let mut found = BTreeMap::new();
    for (index, source_file) in source.files().iter().enumerate() {
        if index % 64 == 0 {
            control.check()?;
        }
        if !matches!(source_file.path().rsplit('/').next(), Some("Cargo.toml")) {
            continue;
        }
        let Some((name, version, features)) =
            manifest_features(source_file.bytes(), inherited.as_deref())?
        else {
            continue;
        };
        if identities.contains(&(name.clone(), version.clone()))
            && found.insert((name, version), features).is_some()
        {
            return Err(invalid());
        }
    }
    Ok(found)
}

struct VendorFacts {
    checksums: BTreeMap<(String, String), SourceFingerprint>,
    features: BTreeMap<(String, String), Vec<String>>,
    archive_fingerprint: SourceFingerprint,
}

fn vendor_facts(
    vendor: &CargoVendorSnapshot,
    control: &dyn InspectionControl,
) -> Result<VendorFacts, SecurityError> {
    if vendor.packages.len() > MAX_PACKAGES {
        return Err(SecurityError::OutputLimit);
    }
    let mut checksums = BTreeMap::new();
    let mut features = BTreeMap::new();
    for (index, package) in vendor.packages.iter().enumerate() {
        if index % 64 == 0 {
            control.check()?;
        }
        if !valid_name(&package.name)
            || package.version.len() > MAX_VERSION_BYTES
            || semver::Version::parse(&package.version).is_err()
            || checksums
                .insert(
                    (package.name.clone(), package.version.clone()),
                    package.package_checksum.clone(),
                )
                .is_some()
        {
            return Err(invalid());
        }
        let path = format!("{}-{}/Cargo.toml", package.name, package.version);
        let manifest = file(&vendor.source, &path).ok_or_else(invalid)?;
        let Some((name, version, declared)) = manifest_features(manifest, None)? else {
            return Err(invalid());
        };
        if name != package.name || version != package.version {
            return Err(invalid());
        }
        features.insert((name, version), declared);
    }
    let archive = crate::source_archive::encode(&vendor.source).map_err(SecurityError::from)?;
    Ok(VendorFacts {
        checksums,
        features,
        archive_fingerprint: fingerprint(&archive)?,
    })
}

fn security_source(source: SupplySource) -> Option<SecuritySource> {
    match source {
        SupplySource::Workspace => Some(SecuritySource::Workspace),
        SupplySource::CratesIo => Some(SecuritySource::CratesIo),
        SupplySource::Registry | SupplySource::Git | SupplySource::Unverified => None,
    }
}

fn deny_identity(package: &SecurityPackage) -> Identity {
    let rank = match package.source {
        SecuritySource::Workspace => 0,
        SecuritySource::CratesIo => 1,
        SecuritySource::Unverified => 4,
    };
    (package.name.clone(), package.version.clone(), rank)
}

fn active_features(
    deny: &DenyObservation,
    vendor: &CargoVendorSnapshot,
    vendor_facts: &VendorFacts,
    lock: &[LockPackage],
    source_fingerprint: &SourceFingerprint,
    lock_fingerprint: &SourceFingerprint,
) -> Result<BTreeMap<Identity, Vec<String>>, SecurityError> {
    deny.validate()?;
    if &deny.source_fingerprint != source_fingerprint
        || &deny.lock_fingerprint != lock_fingerprint
        || deny.vendor_fingerprint != vendor.tree_fingerprint
        || deny.vendor_archive_fingerprint != vendor_facts.archive_fingerprint
        || deny.packages.len() != lock.len()
    {
        return Err(invalid());
    }

    let lock_identities = lock
        .iter()
        .map(|package| {
            security_source(package.source)
                .map(|_| {
                    (
                        package.name.clone(),
                        package.version.clone(),
                        source_rank(package.source),
                    )
                })
                .ok_or_else(invalid)
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let mut active = BTreeMap::new();
    for (index, package) in deny.packages.iter().enumerate() {
        if !valid_name(&package.name)
            || semver::Version::parse(&package.version).is_err()
            || package.source == SecuritySource::Unverified
        {
            return Err(invalid());
        }
        let expected_fingerprint = match package.source {
            SecuritySource::Workspace => source_fingerprint,
            SecuritySource::CratesIo => &vendor.tree_fingerprint,
            SecuritySource::Unverified => return Err(invalid()),
        };
        if package.source_fingerprint.as_ref() != Some(expected_fingerprint) {
            return Err(invalid());
        }
        let features = deny.enabled_features.get(index).ok_or_else(invalid)?;
        if features.len() > MAX_FEATURES
            || features.iter().any(|feature| !valid_feature(feature))
            || features.iter().collect::<BTreeSet<_>>().len() != features.len()
            || active
                .insert(deny_identity(package), features.clone())
                .is_some()
        {
            return Err(invalid());
        }
    }
    if active.keys().cloned().collect::<BTreeSet<_>>() != lock_identities {
        return Err(invalid());
    }
    Ok(active)
}

pub(super) fn facts(
    source: &SourceBundle,
    vendor: Option<&CargoVendorSnapshot>,
    deny: Option<&DenyObservation>,
    control: &dyn InspectionControl,
) -> Result<SupplyGraph, SecurityError> {
    control.check()?;
    let lock_bytes = file(source, "Cargo.lock").ok_or_else(invalid)?;
    let lock_fingerprint = fingerprint(lock_bytes)?;
    let source_archive = crate::source_archive::encode(source).map_err(SecurityError::from)?;
    let source_fingerprint = fingerprint(&source_archive)?;
    let lock = parse_lock(lock_bytes)?;
    control.check()?;

    let workspace_features = workspace_features(source, &lock, control)?;
    let vendor_facts = vendor
        .map(|vendor| vendor_facts(vendor, control))
        .transpose()?;
    let active = match (deny, vendor, vendor_facts.as_ref()) {
        (Some(deny), Some(vendor), Some(vendor_facts)) => Some(active_features(
            deny,
            vendor,
            vendor_facts,
            &lock,
            &source_fingerprint,
            &lock_fingerprint,
        )?),
        (Some(_), _, _) => return Err(invalid()),
        (None, _, _) => None,
    };

    let mut names = BTreeMap::<&str, usize>::new();
    for package in &lock {
        *names.entry(&package.name).or_default() += 1;
    }
    let mut packages = Vec::with_capacity(lock.len());
    for (index, package) in lock.iter().enumerate() {
        if index % 64 == 0 {
            control.check()?;
        }
        let key = (package.name.clone(), package.version.clone());
        let vendor_matches_checksum = package.source == SupplySource::CratesIo
            && package.declared_checksum.as_ref().is_some_and(|checksum| {
                vendor_facts
                    .as_ref()
                    .and_then(|facts| facts.checksums.get(&key))
                    == Some(checksum)
            });
        let declared_features = match package.source {
            SupplySource::Workspace => workspace_features.get(&key).cloned(),
            SupplySource::CratesIo if vendor_matches_checksum => vendor_facts
                .as_ref()
                .and_then(|facts| facts.features.get(&key))
                .cloned(),
            SupplySource::CratesIo
            | SupplySource::Registry
            | SupplySource::Git
            | SupplySource::Unverified => None,
        };
        let identity = (
            package.name.clone(),
            package.version.clone(),
            source_rank(package.source),
        );
        packages.push(SupplyPackage {
            name: package.name.clone(),
            version: package.version.clone(),
            source: package.source,
            source_fingerprint: package.source_fingerprint.clone(),
            declared_checksum: package.declared_checksum.clone(),
            checksum_verified: vendor_matches_checksum
                && active
                    .as_ref()
                    .is_some_and(|active| active.contains_key(&identity)),
            duplicate_name: names.get(package.name.as_str()).copied().unwrap_or(0) > 1,
            declared_features,
            active_features: active
                .as_ref()
                .and_then(|active| active.get(&identity))
                .cloned(),
            yanked: if package.source == SupplySource::CratesIo {
                YankedFact::NotConsulted
            } else {
                YankedFact::NotApplicable
            },
        });
    }
    control.check()?;
    Ok(SupplyGraph {
        source_fingerprint,
        lock_fingerprint,
        packages,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed fixtures should fail immediately if their setup is invalid.
mod tests {
    use super::*;
    use rust_engineering_application::security::SecurityArtifactStreams;
    use rust_engineering_application::{ExecutionCancellation, OperationControl, ProjectError};
    use rust_engineering_domain::security::SecurityCounts;
    use rust_engineering_domain::{
        CargoVendorPackage, ExecutionFingerprint, ExecutionTermination, RuntimeIdentity, SourceFile,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Control {
        calls: AtomicUsize,
        cancel_at: usize,
    }
    impl OperationControl for Control {
        fn check(&self) -> Result<(), ProjectError> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            if call >= self.cancel_at {
                Err(ProjectError::Cancelled)
            } else {
                Ok(())
            }
        }
    }
    impl ExecutionCancellation for Control {
        fn is_cancelled(&self) -> bool {
            self.calls.load(Ordering::SeqCst) >= self.cancel_at
        }
    }

    fn control() -> Control {
        Control {
            calls: AtomicUsize::new(0),
            cancel_at: usize::MAX,
        }
    }

    fn fp(byte: char) -> SourceFingerprint {
        format!("sha256:{}", byte.to_string().repeat(64))
            .parse()
            .unwrap()
    }

    fn execution_fp(byte: char) -> ExecutionFingerprint {
        format!("sha256:{}", byte.to_string().repeat(64))
            .parse()
            .unwrap()
    }

    fn source(lock: &str) -> SourceBundle {
        SourceBundle::new(vec![
            SourceFile::new(
                "Cargo.toml".into(),
                b"[package]\nname='app'\nversion='0.1.0'\n[features]\ndefault=[]\nfast=[]\n"
                    .to_vec(),
            )
            .unwrap(),
            SourceFile::new("Cargo.lock".into(), lock.as_bytes().to_vec()).unwrap(),
        ])
        .unwrap()
    }

    fn checksum_hex() -> String {
        "a".repeat(64)
    }

    fn basic_lock() -> String {
        format!(
            "version=4\n[[package]]\nname='app'\nversion='0.1.0'\n\n[[package]]\nname='dep'\nversion='1.2.3'\nsource='registry+https://github.com/rust-lang/crates.io-index'\nchecksum='{}'\n",
            checksum_hex()
        )
    }

    fn vendor() -> CargoVendorSnapshot {
        CargoVendorSnapshot {
            source: SourceBundle::new(vec![
                SourceFile::new(
                    "dep-1.2.3/Cargo.toml".into(),
                    b"[package]\nname='dep'\nversion='1.2.3'\n[features]\ndefault=[]\nsimd=[]\n"
                        .to_vec(),
                )
                .unwrap(),
            ])
            .unwrap(),
            tree_fingerprint: fp('b'),
            packages: vec![CargoVendorPackage {
                name: "dep".into(),
                version: "1.2.3".into(),
                package_checksum: format!("sha256:{}", checksum_hex()).parse().unwrap(),
            }],
        }
    }

    fn deny(source: &SourceBundle, vendor: &CargoVendorSnapshot) -> DenyObservation {
        let source_archive = crate::source_archive::encode(source).unwrap();
        let vendor_archive = crate::source_archive::encode(&vendor.source).unwrap();
        let source_fingerprint = fingerprint(&source_archive).unwrap();
        let execution = execution_fp('c');
        DenyObservation {
            source_fingerprint: source_fingerprint.clone(),
            vendor_fingerprint: vendor.tree_fingerprint.clone(),
            vendor_archive_fingerprint: fingerprint(&vendor_archive).unwrap(),
            policy_fingerprint: fp('d'),
            deny_config_fingerprint: fp('e'),
            cargo_config_fingerprint: fp('f'),
            metadata_original_fingerprint: fp('1'),
            metadata_derived_fingerprint: fp('2'),
            lock_fingerprint: fingerprint(file(source, "Cargo.lock").unwrap()).unwrap(),
            runtime: RuntimeIdentity {
                platform: "linux/arm64".into(),
                image_id: format!("sha256:{}", "3".repeat(64)),
                configuration_fingerprint: execution_fp('4'),
                execution_fingerprint: execution.clone(),
                rust_version: "rustc 1.98.1".into(),
                cargo_version: "cargo 1.98.1".into(),
                declared_toolchain: None,
            },
            execution_fingerprint: execution,
            packages: vec![
                SecurityPackage {
                    name: "app".into(),
                    version: "0.1.0".into(),
                    source: SecuritySource::Workspace,
                    source_fingerprint: Some(source_fingerprint),
                },
                SecurityPackage {
                    name: "dep".into(),
                    version: "1.2.3".into(),
                    source: SecuritySource::CratesIo,
                    source_fingerprint: Some(vendor.tree_fingerprint.clone()),
                },
            ],
            declared_licenses: vec![None, None],
            license_files: vec![vec![], vec![]],
            enabled_features: vec![vec!["fast".into()], vec!["simd".into()]],
            dependency_indices: vec![vec![1], vec![]],
            workspace_members: vec![0],
            findings: vec![],
            findings_omitted: 0,
            licenses: SecurityCounts::default(),
            bans: SecurityCounts::default(),
            sources: SecurityCounts::default(),
            parse_complete: true,
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            artifacts: SecurityArtifactStreams::default(),
        }
    }

    #[test]
    fn source_urls_never_escape_and_checksum_needs_bound_deny() {
        let source = source(&basic_lock());
        let vendor = vendor();
        let without_deny = facts(&source, Some(&vendor), None, &control()).unwrap();
        let dep = without_deny
            .packages
            .iter()
            .find(|package| package.name == "dep")
            .unwrap();
        assert_eq!(dep.source, SupplySource::CratesIo);
        assert!(dep.declared_checksum.is_some());
        assert!(!dep.checksum_verified);
        assert_eq!(
            dep.declared_features.as_deref(),
            Some(&["default".into(), "simd".into()][..])
        );
        assert!(dep.active_features.is_none());

        let serialized = serde_json::to_string(&without_deny).unwrap();
        assert!(!serialized.contains("github.com"));
        assert!(!serialized.contains("registry+"));
        let bound = facts(
            &source,
            Some(&vendor),
            Some(&deny(&source, &vendor)),
            &control(),
        )
        .unwrap();
        let dep = bound
            .packages
            .iter()
            .find(|package| package.name == "dep")
            .unwrap();
        assert!(dep.checksum_verified);
        assert_eq!(dep.active_features.as_deref(), Some(&["simd".into()][..]));
    }

    #[test]
    fn registry_git_credentials_are_hashed_and_duplicate_names_include_other_sources() {
        let lock = "version=4\n[[package]]\nname='same'\nversion='1.0.0'\nsource='registry+https://user:secret@private.invalid/index?token=hidden#frag'\nchecksum='bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'\n[[package]]\nname='same'\nversion='2.0.0'\nsource='git+https://oauth:credential@git.invalid/repo?token=hidden#abcdef'\n";
        let source = source(lock);
        let graph = facts(&source, None, None, &control()).unwrap();
        assert_eq!(graph.packages.len(), 2);
        assert!(graph.packages.iter().all(|package| package.duplicate_name));
        assert_eq!(graph.packages[0].source, SupplySource::Registry);
        assert_eq!(graph.packages[1].source, SupplySource::Git);
        assert!(
            graph
                .packages
                .iter()
                .all(|package| package.source_fingerprint.is_some())
        );
        assert!(
            graph
                .packages
                .iter()
                .all(|package| package.yanked == YankedFact::NotApplicable)
        );
        let serialized = serde_json::to_string(&graph).unwrap();
        for secret in [
            "private.invalid",
            "git.invalid",
            "secret",
            "credential",
            "hidden",
        ] {
            assert!(!serialized.contains(secret));
        }
    }

    #[test]
    fn exact_duplicate_malformed_identity_and_checksum_fail_closed() {
        for lock in [
            "version=4\n[[package]]\nname='x'\nversion='1.0.0'\n[[package]]\nname='x'\nversion='1.0.0'\n",
            "version=4\n[[package]]\nname='bad name'\nversion='1.0.0'\n",
            "version=4\n[[package]]\nname='x'\nversion='not-semver'\n",
            "version=4\n[[package]]\nname='x'\nversion='1.0.0'\nsource='registry+https://index.crates.io/'\nchecksum='ABC'\n",
            "version=4\nversion=4\n",
        ] {
            assert!(matches!(
                facts(&source(lock), None, None, &control()),
                Err(SecurityError::InvalidMetadata)
            ));
        }
    }

    #[test]
    fn deny_binding_and_vendor_identity_are_revalidated() {
        let source = source(&basic_lock());
        let vendor = vendor();
        let mut observation = deny(&source, &vendor);
        observation.lock_fingerprint = fp('9');
        assert!(matches!(
            facts(&source, Some(&vendor), Some(&observation), &control()),
            Err(SecurityError::InvalidMetadata)
        ));

        let mut mismatched = vendor.clone();
        mismatched.packages[0].version = "9.9.9".into();
        assert!(matches!(
            facts(&source, Some(&mismatched), None, &control()),
            Err(SecurityError::InvalidMetadata)
        ));
    }

    #[test]
    fn package_budget_and_cooperative_cancellation_are_enforced() {
        let mut lock = String::from("version=4\n");
        for index in 0..=MAX_PACKAGES {
            lock.push_str(&format!("[[package]]\nname='p{index}'\nversion='1.0.0'\n"));
        }
        assert!(matches!(
            facts(&source(&lock), None, None, &control()),
            Err(SecurityError::OutputLimit)
        ));

        let cancelled = Control {
            calls: AtomicUsize::new(0),
            cancel_at: 0,
        };
        assert!(matches!(
            facts(&source(&basic_lock()), None, None, &cancelled),
            Err(SecurityError::Inspection(
                rust_engineering_application::InspectionError::Project(ProjectError::Cancelled)
            ))
        ));
    }
}
