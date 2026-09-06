//! Validation and protected capture of an explicit Cargo directory source.
use rust_engineering_application::{OperationControl, ProjectError};
use rust_engineering_domain::{CargoVendorSnapshot, OperationalErrorCode, SourceFingerprint};
use std::path::Path;

// The capture path below only compiles on macOS, so the parsing and digest
// machinery it owns is imported with it.
#[cfg(target_os = "macos")]
use rust_engineering_domain::{CargoVendorPackage, SourceBundle, validate_source_path};
#[cfg(target_os = "macos")]
use serde::de::{MapAccess, Visitor};
#[cfg(target_os = "macos")]
use serde::{Deserialize, Deserializer};
#[cfg(target_os = "macos")]
use sha2::{Digest, Sha256};
#[cfg(target_os = "macos")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "macos")]
use std::fmt;

fn invalid() -> ProjectError {
    ProjectError::Rejected(OperationalErrorCode::InvalidProject)
}

// Reachable only through the macOS capture path in `inspect_cargo_vendor`.
#[cfg(target_os = "macos")]
fn fingerprint(
    bytes: impl IntoIterator<Item = impl AsRef<[u8]>>,
) -> Result<SourceFingerprint, ProjectError> {
    let mut hash = Sha256::new();
    for value in bytes {
        hash.update(value.as_ref());
    }
    finish_fingerprint(hash)
}

// Reachable only through the macOS capture path in `inspect_cargo_vendor`.
#[cfg(target_os = "macos")]
fn finish_fingerprint(hash: Sha256) -> Result<SourceFingerprint, ProjectError> {
    let mut encoded = String::from("sha256:");
    for byte in hash.finalize() {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").map_err(|_| ProjectError::Internal)?;
    }
    encoded.parse().map_err(|_| ProjectError::Internal)
}

// Reachable only through the macOS capture path in `inspect_cargo_vendor`.
#[cfg(target_os = "macos")]
fn tree_fingerprint(source: &SourceBundle) -> Result<SourceFingerprint, ProjectError> {
    let mut hash = Sha256::new();
    for file in source.files() {
        hash.update((file.path().len() as u64).to_le_bytes());
        hash.update(file.path().as_bytes());
        hash.update((file.bytes().len() as u64).to_le_bytes());
        hash.update(file.bytes());
    }
    finish_fingerprint(hash)
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct UniqueFiles(BTreeMap<String, String>);

#[cfg(target_os = "macos")]
impl<'de> Deserialize<'de> for UniqueFiles {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct UniqueFilesVisitor;
        impl<'de> Visitor<'de> for UniqueFilesVisitor {
            type Value = UniqueFiles;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a map of unique Cargo checksum paths")
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut files = BTreeMap::new();
                while let Some((path, digest)) = map.next_entry::<String, String>()? {
                    if files.insert(path, digest).is_some() {
                        return Err(serde::de::Error::custom("duplicate checksum path"));
                    }
                }
                Ok(UniqueFiles(files))
            }
        }
        deserializer.deserialize_map(UniqueFilesVisitor)
    }
}

#[cfg(target_os = "macos")]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CargoChecksum {
    files: UniqueFiles,
    package: Option<String>,
    #[serde(rename = "$comment")]
    comment: Option<String>,
}

// Reachable only through the macOS capture path in `inspect_cargo_vendor`.
#[cfg(target_os = "macos")]
fn checksum(value: &str) -> Result<SourceFingerprint, ProjectError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(invalid());
    }
    format!("sha256:{value}").parse().map_err(|_| invalid())
}

// Reachable only through the macOS capture path in `inspect_cargo_vendor`.
#[cfg(target_os = "macos")]
fn package_name(name: &str) -> bool {
    name.len() <= 64
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

// Reachable only through the macOS capture path in `inspect_cargo_vendor`.
#[cfg(target_os = "macos")]
fn package_identity(manifest: &[u8]) -> Result<(String, String), ProjectError> {
    let parsed: toml::Value = toml::from_str(std::str::from_utf8(manifest).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let package = parsed
        .as_table()
        .and_then(|root| root.get("package"))
        .and_then(toml::Value::as_table)
        .ok_or_else(invalid)?;
    let name = package
        .get("name")
        .and_then(toml::Value::as_str)
        .ok_or_else(invalid)?;
    let version = package
        .get("version")
        .and_then(toml::Value::as_str)
        .ok_or_else(invalid)?;
    if !package_name(name) || semver::Version::parse(version).is_err() {
        return Err(invalid());
    }
    Ok((name.to_owned(), version.to_owned()))
}

// Reachable only through the macOS capture path in `inspect_cargo_vendor`.
#[cfg(target_os = "macos")]
pub(crate) fn validate(source: SourceBundle) -> Result<CargoVendorSnapshot, ProjectError> {
    // File paths determine every authorized directory. Reject explicit empty
    // directories so the host's file-tree digest also binds the whole topology.
    let mut implied_directories = BTreeSet::new();
    for file in source.files() {
        for (index, _) in file.path().match_indices('/') {
            implied_directories.insert(&file.path()[..index]);
        }
    }
    if source
        .directories()
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != implied_directories
    {
        return Err(invalid());
    }
    let tree_fingerprint = tree_fingerprint(&source)?;
    let files = source
        .files()
        .iter()
        .map(|file| (file.path(), file.bytes()))
        .collect::<BTreeMap<_, _>>();
    let roots = source
        .directories()
        .iter()
        .filter(|path| !path.contains('/'))
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if roots.is_empty()
        || source.files().iter().any(|file| !file.path().contains('/'))
        || source.directories().iter().any(|path| {
            path.split('/')
                .next()
                .is_none_or(|root| !roots.contains(root))
        })
    {
        return Err(invalid());
    }

    let mut packages = Vec::with_capacity(roots.len());
    let mut identities = BTreeSet::new();
    for root in roots {
        let manifest_path = format!("{root}/Cargo.toml");
        let checksum_path = format!("{root}/.cargo-checksum.json");
        let (name, version) =
            package_identity(files.get(manifest_path.as_str()).ok_or_else(invalid)?)?;
        if root != format!("{name}-{version}")
            || !identities.insert((name.clone(), version.clone()))
        {
            return Err(invalid());
        }
        let checksum_doc: CargoChecksum =
            serde_json::from_slice(files.get(checksum_path.as_str()).ok_or_else(invalid)?)
                .map_err(|_| invalid())?;
        if checksum_doc.comment.as_deref().is_some_and(str::is_empty) {
            return Err(invalid());
        }
        let package_checksum = checksum(checksum_doc.package.as_deref().ok_or_else(invalid)?)?;

        let actual = files
            .keys()
            .filter_map(|path| path.strip_prefix(&format!("{root}/")))
            .filter(|path| *path != ".cargo-checksum.json")
            .collect::<BTreeSet<_>>();
        let declared = checksum_doc
            .files
            .0
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if actual != declared {
            return Err(invalid());
        }
        for (relative, expected) in checksum_doc.files.0 {
            validate_source_path(&relative).map_err(|_| invalid())?;
            let bytes = files
                .get(format!("{root}/{relative}").as_str())
                .ok_or_else(invalid)?;
            let observed = fingerprint([*bytes])?;
            if observed != checksum(&expected)? {
                return Err(invalid());
            }
        }
        packages.push(CargoVendorPackage {
            name,
            version,
            package_checksum,
        });
    }
    packages.sort_by(|left, right| (&left.name, &left.version).cmp(&(&right.name, &right.version)));
    Ok(CargoVendorSnapshot {
        source,
        tree_fingerprint,
        packages,
    })
}

/// Capture and validate an explicitly configured Cargo directory source.
pub fn inspect_cargo_vendor(
    path: &Path,
    control: &dyn OperationControl,
) -> Result<CargoVendorSnapshot, ProjectError> {
    #[cfg(target_os = "macos")]
    {
        validate(crate::filesystem::capture_cargo_vendor(path, control)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, control);
        Err(ProjectError::Rejected(
            OperationalErrorCode::UnsupportedPlatform,
        ))
    }
}

/// Capture the same tree and fail closed if it is not the host-approved digest.
pub fn capture_with_expected(
    path: &Path,
    expected: &SourceFingerprint,
    control: &dyn OperationControl,
) -> Result<CargoVendorSnapshot, ProjectError> {
    let snapshot = inspect_cargo_vendor(path, control)?;
    if &snapshot.tree_fingerprint != expected {
        return Err(invalid());
    }
    Ok(snapshot)
}
