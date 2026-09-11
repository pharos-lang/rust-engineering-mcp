//! Bind full frozen metadata to exact captured source/vendor bytes before deny.
use crate::supervisor::{Capture, Stop};
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::security::{SecurityPackage, SecuritySource};
use rust_engineering_domain::{CargoVendorSnapshot, SourceBundle, SourceFingerprint};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug)]
pub(super) struct PreparedSecurityMetadata {
    pub derived: Vec<u8>,
    pub packages: Vec<SecurityPackage>,
    /// Captured bundle-relative package directories, including a trailing slash except at root.
    pub package_roots: Vec<String>,
    pub original_fingerprint: SourceFingerprint,
    pub derived_fingerprint: SourceFingerprint,
    pub declared_licenses: Vec<Option<String>>,
    pub license_files: Vec<Vec<(String, SourceFingerprint)>>,
    pub enabled_features: Vec<Vec<String>>,
    pub dependency_indices: Vec<Vec<usize>>,
    pub workspace_members: Vec<usize>,
    pub lock_fingerprint: SourceFingerprint,
}

fn invalid() -> SecurityError {
    SecurityError::InvalidMetadata
}
fn fingerprint(bytes: &[u8]) -> Result<SourceFingerprint, SecurityError> {
    crate::digest(bytes).parse().map_err(|_| invalid())
}
fn file<'a>(source: &'a SourceBundle, path: &str) -> Option<&'a [u8]> {
    source
        .files()
        .binary_search_by(|f| f.path().cmp(path))
        .ok()
        .map(|i| source.files()[i].bytes())
}
fn text<'a>(object: &'a Value, key: &str) -> Result<&'a str, SecurityError> {
    object.get(key).and_then(Value::as_str).ok_or_else(invalid)
}
fn array<'a>(object: &'a Value, key: &str) -> Result<&'a Vec<Value>, SecurityError> {
    object
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(invalid)
}
fn bounded(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

#[derive(Deserialize)]
struct Lock {
    version: u32,
    package: Vec<LockedPackage>,
}
#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

pub(super) fn prepare(
    raw: &[u8],
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
) -> Result<PreparedSecurityMetadata, SecurityError> {
    if raw.is_empty() || raw.len() > 1024 * 1024 {
        return Err(SecurityError::OutputLimit);
    }
    // Shared strict visitor rejects duplicate fields before the frozen graph's
    // existing binding validator consumes the same bytes.
    let mut value = crate::deny_json::strict_value(raw).map_err(|_| invalid())?;
    let capture = Capture {
        code: Some(0),
        stdout: raw.to_vec(),
        stderr: vec![],
        stdout_truncated: false,
        stderr_truncated: false,
        stop: Stop::Exited,
        duration_ms: 0,
    };
    crate::resolution_gateway::metadata_graph(&capture, source, vendor).map_err(|_| invalid())?;
    let lock_bytes = file(source, "Cargo.lock").ok_or_else(invalid)?;
    let lock: Lock = toml::from_str(std::str::from_utf8(lock_bytes).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    if !(3..=4).contains(&lock.version) || lock.package.len() > 4096 {
        return Err(invalid());
    }
    let mut lock_keys = BTreeSet::new();
    for package in &lock.package {
        if !lock_keys.insert((&package.name, &package.version, &package.source)) {
            return Err(invalid());
        }
    }
    let raw_packages = array(&value, "packages")?;
    if raw_packages.len() > 4096 {
        return Err(SecurityError::OutputLimit);
    }
    let mut packages = Vec::new();
    let mut package_roots = Vec::new();
    let mut declared_licenses = Vec::new();
    let mut license_files = Vec::new();
    let mut indices = BTreeMap::new();
    let source_fingerprint = fingerprint(&crate::source_archive::encode(source)?)?;
    for (index, package) in raw_packages.iter().enumerate() {
        let id = text(package, "id")?;
        let name = text(package, "name")?;
        let version = text(package, "version")?;
        if !bounded(name, 64)
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
            || version.len() > 128
            || semver::Version::parse(version).is_err()
            || !bounded(id, 1024)
        {
            return Err(invalid());
        }
        indices.insert(id.to_owned(), index);
        let manifest = text(package, "manifest_path")?;
        let (root, bundle, kind) = if manifest.starts_with("/source/") {
            if !package.get("source").is_some_and(Value::is_null) {
                return Err(invalid());
            }
            ("/source/", source, SecuritySource::Workspace)
        } else {
            if package.get("source").and_then(Value::as_str)
                != Some("registry+https://github.com/rust-lang/crates.io-index")
            {
                return Err(invalid());
            }
            (
                "/rust-mcp-vendor/",
                &vendor.source,
                SecuritySource::CratesIo,
            )
        };
        let relative = manifest.strip_prefix(root).ok_or_else(invalid)?;
        let base = relative.strip_suffix("Cargo.toml").ok_or_else(invalid)?;
        package_roots.push(base.to_owned());
        let mut matches = lock.package.iter().filter(|p| {
            p.name == name
                && p.version == version
                && match kind {
                    SecuritySource::Workspace => p.source.is_none(),
                    SecuritySource::CratesIo => {
                        p.source.as_deref()
                            == Some("registry+https://github.com/rust-lang/crates.io-index")
                    }
                    SecuritySource::Unverified => false,
                }
        });
        let locked = matches.next().ok_or_else(invalid)?;
        if matches.next().is_some() {
            return Err(invalid());
        }
        if kind == SecuritySource::CratesIo {
            let expected = vendor
                .packages
                .iter()
                .find(|p| p.name == name && p.version == version)
                .ok_or_else(invalid)?;
            if locked.checksum.as_ref().map(|s| format!("sha256:{s}"))
                != Some(expected.package_checksum.to_string())
            {
                return Err(invalid());
            }
        } else if locked.checksum.is_some() {
            return Err(invalid());
        }
        let declared = match package.get("license") {
            Some(Value::Null) => None,
            Some(Value::String(s)) if bounded(s, 512) => Some(s.clone()),
            _ => return Err(invalid()),
        };
        declared_licenses.push(declared);
        let mut evidence = BTreeMap::new();
        if let Some(license) = package.get("license_file").filter(|v| !v.is_null()) {
            let license = license
                .as_str()
                .filter(|s| bounded(s, 256))
                .ok_or_else(invalid)?;
            let path = if license.starts_with('/') {
                license.strip_prefix(root).ok_or_else(invalid)?.to_owned()
            } else {
                format!("{base}{license}")
            };
            rust_engineering_domain::validate_source_path(&path).map_err(|_| invalid())?;
            if !path.starts_with(base) {
                return Err(invalid());
            }
            let bytes = file(bundle, &path).ok_or_else(invalid)?;
            evidence.insert(format!("{root}{path}"), fingerprint(bytes)?);
        }
        // The pinned gatherer searches immediate package files. Bind all likely
        // license/copyright inputs; no source text is accepted on metadata alone.
        for candidate in bundle.files() {
            if let Some(local) = candidate.path().strip_prefix(base)
                && !local.contains('/')
                && ["LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "UNLICENSE"]
                    .iter()
                    .any(|prefix| local.to_ascii_uppercase().starts_with(prefix))
            {
                evidence.insert(
                    format!("{root}{}", candidate.path()),
                    fingerprint(candidate.bytes())?,
                );
            }
        }
        license_files.push(evidence.into_iter().collect());
        for target in array(package, "targets")? {
            let path = text(target, "src_path")?;
            let relative = path.strip_prefix(root).ok_or_else(invalid)?;
            rust_engineering_domain::validate_source_path(relative).map_err(|_| invalid())?;
            if file(bundle, relative).is_none() {
                return Err(invalid());
            }
        }
        packages.push(SecurityPackage {
            name: name.into(),
            version: version.into(),
            source: kind,
            source_fingerprint: Some(if kind == SecuritySource::Workspace {
                source_fingerprint.clone()
            } else {
                vendor.tree_fingerprint.clone()
            }),
        });
    }
    let resolve = value.get("resolve").ok_or_else(invalid)?;
    let mut enabled_features = vec![Vec::new(); packages.len()];
    let mut dependency_indices = vec![Vec::new(); packages.len()];
    for node in array(resolve, "nodes")? {
        let index = *indices.get(text(node, "id")?).ok_or_else(invalid)?;
        let features = array(node, "features")?;
        if features.len() > 256 {
            return Err(SecurityError::OutputLimit);
        }
        let mut unique = BTreeSet::new();
        for feature in features {
            let f = feature
                .as_str()
                .filter(|s| bounded(s, 128))
                .ok_or_else(invalid)?;
            if !unique.insert(f.to_owned()) {
                return Err(invalid());
            }
        }
        enabled_features[index] = unique.into_iter().collect();
        let deps = array(node, "dependencies")?;
        let mut unique = BTreeSet::new();
        for dep in deps {
            let i = *indices
                .get(dep.as_str().ok_or_else(invalid)?)
                .ok_or_else(invalid)?;
            if !unique.insert(i) {
                return Err(invalid());
            }
        }
        let detailed = array(node, "deps")?
            .iter()
            .map(|dep| indices.get(text(dep, "pkg")?).copied().ok_or_else(invalid))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if detailed != unique {
            return Err(invalid());
        }
        dependency_indices[index] = unique.into_iter().collect();
    }
    let workspace_members = array(&value, "workspace_members")?
        .iter()
        .map(|v| {
            indices
                .get(v.as_str().ok_or_else(invalid)?)
                .copied()
                .ok_or_else(invalid)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if workspace_members
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .len()
        != workspace_members.len()
        || workspace_members
            .iter()
            .any(|&i| packages[i].source != SecuritySource::Workspace)
    {
        return Err(invalid());
    }
    for package in value
        .get_mut("packages")
        .and_then(Value::as_array_mut)
        .ok_or_else(invalid)?
    {
        package["license"] = Value::Null;
    }
    let derived = serde_json::to_vec(&value).map_err(|_| invalid())?;
    Ok(PreparedSecurityMetadata {
        original_fingerprint: fingerprint(raw)?,
        derived_fingerprint: fingerprint(&derived)?,
        derived,
        packages,
        package_roots,
        declared_licenses,
        license_files,
        enabled_features,
        dependency_indices,
        workspace_members,
        lock_fingerprint: fingerprint(lock_bytes)?,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed hostile-input fixtures use assertions and fatal setup validation.
mod tests {
    use super::*;
    use rust_engineering_domain::SourceFile;
    fn source(license: bool) -> SourceBundle {
        let mut files = vec![
            SourceFile::new(
                "Cargo.toml".into(),
                b"[package]\nname='probe'\nversion='0.1.0'\nlicense='MIT'\n".to_vec(),
            )
            .unwrap(),
            SourceFile::new(
                "Cargo.lock".into(),
                b"version=4\n[[package]]\nname='probe'\nversion='0.1.0'\n".to_vec(),
            )
            .unwrap(),
            SourceFile::new("src/lib.rs".into(), b"pub fn f() {}".to_vec()).unwrap(),
        ];
        if license {
            files.push(
                SourceFile::new("LICENSE".into(), b"license fixture bytes".to_vec()).unwrap(),
            );
        }
        SourceBundle::new(files).unwrap()
    }
    fn vendor() -> CargoVendorSnapshot {
        CargoVendorSnapshot {
            source: SourceBundle::new(vec![]).unwrap(),
            tree_fingerprint: fingerprint(b"empty vendor fixture").unwrap(),
            packages: vec![],
        }
    }
    fn metadata() -> Value {
        serde_json::json!({"version":1,"workspace_root":"/source","workspace_members":["probe"],
            "packages":[{"id":"probe","name":"probe","version":"0.1.0","manifest_path":"/source/Cargo.toml",
                "source":null,"license":"MIT","license_file":null,"dependencies":[],"features":{},
                "targets":[{"src_path":"/source/src/lib.rs"}]}],
            "resolve":{"root":"probe","nodes":[{"id":"probe","dependencies":[],"deps":[],"features":["default"]}]}})
    }
    fn run(
        value: &Value,
        source: &SourceBundle,
    ) -> Result<PreparedSecurityMetadata, SecurityError> {
        prepare(&serde_json::to_vec(value).unwrap(), source, &vendor())
    }
    #[test]
    fn declared_license_cannot_replace_captured_text_and_original_source_is_unchanged() {
        for license in [false, true] {
            let source = source(license);
            let before = source.clone();
            let result = run(&metadata(), &source).unwrap();
            let derived: Value = serde_json::from_slice(&result.derived).unwrap();
            assert!(derived["packages"][0]["license"].is_null());
            assert_eq!(result.declared_licenses, vec![Some("MIT".into())]);
            assert_eq!(result.license_files[0].len(), usize::from(license));
            assert_ne!(result.original_fingerprint, result.derived_fingerprint);
            assert_eq!(result.enabled_features, vec![vec!["default"]]);
            assert_eq!(result.workspace_members, vec![0]);
            assert_eq!(source, before);
        }
    }
    #[test]
    fn escapes_missing_files_and_forged_graphs_are_rejected_before_deny() {
        let mutations: &[fn(&mut Value)] = &[
            |v| v["packages"][0]["license_file"] = "../LICENSE".into(),
            |v| v["packages"][0]["license_file"] = "/opt/secret".into(),
            |v| v["packages"][0]["license_file"] = "missing".into(),
            |v| v["packages"][0]["targets"][0]["src_path"] = "/source/../secret".into(),
            |v| v["packages"][0]["targets"][0]["src_path"] = "/source/missing".into(),
            |v| v["packages"][0]["source"] = "registry+https://evil.example".into(),
            |v| v["packages"][0]["name"] = "forged".into(),
            |v| v["packages"][0]["version"] = "9.0.0".into(),
            |v| v["resolve"]["nodes"][0]["dependencies"] = serde_json::json!(["probe"]),
            |v| v["workspace_members"] = serde_json::json!(["probe", "probe"]),
        ];
        for (i, mutate) in mutations.iter().enumerate() {
            let mut value = metadata();
            mutate(&mut value);
            assert!(run(&value, &source(true)).is_err(), "case {i}");
        }
        let mut valid = metadata();
        valid["packages"][0]["license_file"] = "LICENSE".into();
        assert!(run(&valid, &source(true)).is_ok());
        valid["packages"][0]["license_file"] = "/source/LICENSE".into();
        assert!(run(&valid, &source(true)).is_ok());
    }
    #[test]
    fn duplicate_json_missing_lock_and_changed_license_bytes_have_distinct_oracles() {
        let raw = serde_json::to_string(&metadata()).unwrap().replace(
            "\"license\":\"MIT\"",
            "\"license\":null,\"license\":\"MIT\"",
        );
        assert!(prepare(raw.as_bytes(), &source(true), &vendor()).is_err());
        let no_lock = SourceBundle::new(
            source(true)
                .files()
                .iter()
                .filter(|f| f.path() != "Cargo.lock")
                .cloned()
                .collect(),
        )
        .unwrap();
        assert!(run(&metadata(), &no_lock).is_err());
        let mut files = source(true).files().to_vec();
        files.retain(|f| f.path() != "LICENSE");
        files.push(SourceFile::new("LICENSE".into(), b"different terms".to_vec()).unwrap());
        let changed = SourceBundle::new(files).unwrap();
        assert_ne!(
            run(&metadata(), &source(true)).unwrap().license_files,
            run(&metadata(), &changed).unwrap().license_files
        );
    }
}
