//! Pure conversion of bounded Cargo metadata v1 and captured root declarations.
//! Cargo IDs stay opaque; only response-local indexes and relative paths escape.
use std::collections::{BTreeMap, BTreeSet};

use rust_engineering_application::InspectionError;
use rust_engineering_domain::*;
use serde::{Deserialize, Deserializer, de};

const METADATA_LIMIT: usize = 256 * 1024;
const STRUCTURE_LIMIT: usize = 128 * 1024;
const PACKAGE_LIMIT: usize = 128;
const TARGET_LIMIT: usize = 512;
const DEPENDENCY_LIMIT: usize = 512;
const FEATURE_LIMIT: usize = 256;
const PROFILE_LIMIT: usize = 64;

fn invalid() -> InspectionError {
    InspectionError::InvalidMetadata
}
fn bounded(value: &str, max: usize) -> Result<(), InspectionError> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(invalid());
    }
    Ok(())
}
fn limit(count: usize, max: usize) -> Result<(), InspectionError> {
    if count > max {
        Err(InspectionError::OutputLimit)
    } else {
        Ok(())
    }
}
fn name(value: &str) -> Result<(), InspectionError> {
    bounded(value, 128)?;
    if !value
        .chars()
        .all(|character| character.is_alphanumeric() || matches!(character, '_' | '-'))
    {
        return Err(invalid());
    }
    Ok(())
}
// Required nullable fields must not silently default when Cargo omits a fact.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}
#[derive(Deserialize)]
struct Metadata {
    version: u32,
    resolve: (),
    workspace_root: String,
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    workspace_default_members: Vec<String>,
}
#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
    version: String,
    // --no-deps only includes local workspace packages.
    source: (),
    manifest_path: String,
    edition: String,
    #[serde(deserialize_with = "nullable")]
    rust_version: Option<String>,
    targets: Vec<Target>,
    features: UniqueMap<Vec<String>>,
    dependencies: Vec<Dependency>,
}
#[derive(Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
    crate_types: Vec<String>,
    src_path: String,
    edition: String,
    #[serde(default, rename = "required-features")]
    required_features: Vec<String>,
    test: bool,
    doctest: bool,
}
#[derive(Deserialize)]
struct Dependency {
    name: String,
    #[serde(deserialize_with = "nullable")]
    rename: Option<String>,
    req: String,
    #[serde(deserialize_with = "nullable")]
    kind: Option<String>,
    optional: bool,
    uses_default_features: bool,
    features: Vec<String>,
    #[serde(deserialize_with = "nullable")]
    target: Option<String>,
    #[serde(deserialize_with = "nullable")]
    source: Option<String>,
    #[serde(default)]
    path: Option<String>,
    #[serde(deserialize_with = "nullable")]
    registry: Option<String>,
}

/// Serde's ordinary maps overwrite duplicate keys. Features are facts, so reject.
struct UniqueMap<T>(BTreeMap<String, T>);
impl<'de, T: Deserialize<'de>> Deserialize<'de> for UniqueMap<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> de::Visitor<'de> for Visitor<T> {
            type Value = UniqueMap<T>;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("unique string-keyed object")
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut input: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((key, value)) = input.next_entry::<String, T>()? {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate feature"));
                    }
                }
                Ok(UniqueMap(values))
            }
        }
        deserializer.deserialize_map(Visitor(std::marker::PhantomData))
    }
}

fn edition(value: &str) -> Result<RustEdition, InspectionError> {
    match value {
        "2015" => Ok(RustEdition::E2015),
        "2018" => Ok(RustEdition::E2018),
        "2021" => Ok(RustEdition::E2021),
        "2024" => Ok(RustEdition::E2024),
        _ => Err(invalid()),
    }
}
fn msrv(value: &str) -> Result<(), InspectionError> {
    bounded(value, 32)?;
    let parts: Vec<_> = value.split('.').collect();
    if !(2..=3).contains(&parts.len())
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
                || part.len() > 1 && part.starts_with('0')
        })
    {
        return Err(invalid());
    }
    let normalized = if parts.len() == 2 {
        format!("{value}.0")
    } else {
        value.to_owned()
    };
    semver::Version::parse(&normalized).map_err(|_| invalid())?;
    Ok(())
}
fn source_file<'a>(
    path: &str,
    source: &'a SourceBundle,
) -> Result<&'a SourceFile, InspectionError> {
    source
        .files()
        .binary_search_by(|file| file.path().cmp(path))
        .ok()
        .map(|index| &source.files()[index])
        .ok_or_else(invalid)
}
fn relative_file(path: &str, source: &SourceBundle) -> Result<String, InspectionError> {
    let relative = path.strip_prefix("/source/").ok_or_else(invalid)?;
    validate_source_path(relative).map_err(|_| invalid())?;
    source_file(relative, source)?;
    Ok(relative.to_owned())
}
fn relative_directory(path: &str, source: &SourceBundle) -> Result<String, InspectionError> {
    if path == "/source" {
        return Ok(".".into());
    }
    let relative = path.strip_prefix("/source/").ok_or_else(invalid)?;
    validate_source_path(relative).map_err(|_| invalid())?;
    if source
        .directories()
        .binary_search_by(|directory| directory.as_str().cmp(relative))
        .is_err()
    {
        return Err(invalid());
    }
    Ok(relative.to_owned())
}
fn feature_strings(mut values: Vec<String>) -> Result<Vec<String>, InspectionError> {
    limit(values.len(), FEATURE_LIMIT)?;
    for value in &values {
        bounded(value, 256)?;
    }
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid());
    }
    Ok(values)
}
fn kinds(mut values: Vec<String>, crate_types: bool) -> Result<Vec<TargetKind>, InspectionError> {
    if values.is_empty() {
        return Err(invalid());
    }
    limit(values.len(), 11)?;
    values.sort();
    if values.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(invalid());
    }
    values
        .iter()
        .map(|value| match value.as_str() {
            "lib" => Ok(TargetKind::Lib),
            "bin" => Ok(TargetKind::Bin),
            "rlib" => Ok(TargetKind::Rlib),
            "dylib" => Ok(TargetKind::Dylib),
            "cdylib" => Ok(TargetKind::Cdylib),
            "staticlib" => Ok(TargetKind::Staticlib),
            "proc-macro" => Ok(TargetKind::ProcMacro),
            "example" if !crate_types => Ok(TargetKind::Example),
            "test" if !crate_types => Ok(TargetKind::Test),
            "bench" if !crate_types => Ok(TargetKind::Bench),
            "custom-build" if !crate_types => Ok(TargetKind::CustomBuild),
            _ => Err(invalid()),
        })
        .collect()
}
fn target(
    target: Target,
    source: &SourceBundle,
    features: &BTreeSet<String>,
) -> Result<ProjectTarget, InspectionError> {
    name(&target.name)?;
    let required_features = feature_strings(target.required_features)?;
    if required_features
        .iter()
        .any(|feature| !features.contains(feature))
    {
        return Err(invalid());
    }
    Ok(ProjectTarget {
        name: target.name,
        kinds: kinds(target.kind, false)?,
        crate_types: kinds(target.crate_types, true)?,
        source_path: relative_file(&target.src_path, source)?,
        edition: edition(&target.edition)?,
        required_features,
        test: target.test,
        doctest: target.doctest,
    })
}
fn dependency(
    dependency: Dependency,
    source: &SourceBundle,
) -> Result<DirectDependency, InspectionError> {
    name(&dependency.name)?;
    if let Some(rename) = &dependency.rename {
        name(rename)?;
    }
    bounded(&dependency.req, 256)?;
    semver::VersionReq::parse(&dependency.req).map_err(|_| invalid())?;
    if let Some(condition) = &dependency.target {
        bounded(condition, 1024)?;
    }
    if let Some(registry) = &dependency.registry {
        bounded(registry, 4096)?;
    }
    if let Some(origin) = &dependency.source {
        bounded(origin, 4096)?;
    }
    let kind = match dependency.kind.as_deref() {
        None => DeclaredDependencyKind::Normal,
        Some("build") => DeclaredDependencyKind::Build,
        Some("dev") => DeclaredDependencyKind::Dev,
        _ => return Err(invalid()),
    };
    let (source_kind, relative_path) =
        match (dependency.source.as_deref(), dependency.path.as_deref()) {
            (None, Some(path)) if dependency.registry.is_none() => {
                let relative = relative_directory(path, source)?;
                let manifest = if relative == "." {
                    "Cargo.toml".into()
                } else {
                    format!("{relative}/Cargo.toml")
                };
                source_file(&manifest, source)?;
                (DependencySourceKind::Path, Some(relative))
            }
            (Some(origin), None)
                if origin.starts_with("registry+") && origin.len() > 9
                    || origin.starts_with("sparse+") && origin.len() > 7 =>
            {
                (DependencySourceKind::Registry, None)
            }
            (Some(origin), None)
                if origin.starts_with("git+")
                    && origin.len() > 4
                    && dependency.registry.is_none() =>
            {
                (DependencySourceKind::Git, None)
            }
            _ => return Err(invalid()),
        };
    // Hash every origin field, preserving credentials only inside the one-way
    // digest. No ID/URL/path outside the captured source is returned or logged.
    let mut encoded = b"rust-engineering-mcp/dependency-origin/v1\0".to_vec();
    encoded.extend(
        serde_json::to_vec(&(
            source_kind,
            &dependency.source,
            &dependency.registry,
            &dependency.path,
        ))
        .map_err(|_| InspectionError::Internal)?,
    );
    let identity = super::digest(&encoded)
        .parse()
        .map_err(|_| InspectionError::Internal)?;
    Ok(DirectDependency {
        name: dependency.name,
        rename: dependency.rename,
        version_requirement: dependency.req,
        kind,
        optional: dependency.optional,
        uses_default_features: dependency.uses_default_features,
        features: feature_strings(dependency.features)?,
        target_condition: dependency.target,
        origin: DependencyOrigin {
            kind: source_kind,
            identity,
            relative_path,
        },
    })
}

pub(super) fn parse(
    bytes: &[u8],
    source: &SourceBundle,
    mut runtime: RuntimeIdentity,
) -> Result<ProjectStructure, InspectionError> {
    limit(bytes.len(), METADATA_LIMIT)?;
    let mut metadata: Metadata = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    // Reading these unit fields documents their required-null invariant.
    let () = metadata.resolve;
    if metadata.version != 1 || metadata.workspace_root != "/source" || metadata.packages.is_empty()
    {
        return Err(invalid());
    }
    limit(metadata.packages.len(), PACKAGE_LIMIT)?;
    limit(metadata.workspace_members.len(), PACKAGE_LIMIT)?;
    limit(metadata.workspace_default_members.len(), PACKAGE_LIMIT)?;
    metadata
        .packages
        .sort_by(|a, b| a.manifest_path.cmp(&b.manifest_path));
    let mut ids = BTreeMap::new();
    let mut manifests = BTreeSet::new();
    let mut names = BTreeSet::new();
    let mut packages = Vec::new();
    for (index, mut package) in metadata.packages.into_iter().enumerate() {
        let () = package.source;
        bounded(&package.id, 4096)?;
        name(&package.name)?;
        bounded(&package.version, 128)?;
        semver::Version::parse(&package.version).map_err(|_| invalid())?;
        if let Some(version) = &package.rust_version {
            msrv(version)?;
        }
        let manifest_path = relative_file(&package.manifest_path, source)?;
        if !manifest_path.ends_with("/Cargo.toml") && manifest_path != "Cargo.toml" {
            return Err(invalid());
        }
        if ids.insert(package.id, index as u32).is_some()
            || !manifests.insert(manifest_path.clone())
            || !names.insert(package.name.clone())
        {
            return Err(invalid());
        }
        limit(package.targets.len(), TARGET_LIMIT)?;
        limit(package.dependencies.len(), DEPENDENCY_LIMIT)?;
        limit(package.features.0.len(), FEATURE_LIMIT)?;
        if package.targets.is_empty() {
            return Err(invalid());
        }
        let feature_names = package.features.0.keys().cloned().collect::<BTreeSet<_>>();
        let mut features = Vec::new();
        for (name, activations) in package.features.0 {
            bounded(&name, 128)?;
            features.push(DeclaredFeature {
                name,
                activations: feature_strings(activations)?,
            });
        }
        package
            .targets
            .sort_by(|a, b| (&a.name, &a.kind, &a.src_path).cmp(&(&b.name, &b.kind, &b.src_path)));
        let mut target_keys = BTreeSet::new();
        let mut targets = Vec::new();
        for item in package.targets {
            let mut key_kinds = item.kind.clone();
            key_kinds.sort();
            if !target_keys.insert((item.name.clone(), key_kinds)) {
                return Err(invalid());
            }
            targets.push(target(item, source, &feature_names)?);
        }
        package.dependencies.sort_by(|a, b| {
            (&a.name, &a.rename, &a.kind, &a.target).cmp(&(&b.name, &b.rename, &b.kind, &b.target))
        });
        let mut dependency_keys = BTreeSet::new();
        let mut direct_dependencies = Vec::new();
        for item in package.dependencies {
            if !dependency_keys.insert((
                item.rename.as_ref().unwrap_or(&item.name).clone(),
                item.kind.clone(),
                item.target.clone(),
            )) {
                return Err(invalid());
            }
            direct_dependencies.push(dependency(item, source)?);
        }
        packages.push(ProjectPackage {
            package_index: index as u32,
            name: package.name,
            version: package.version,
            manifest_path,
            edition: edition(&package.edition)?,
            rust_version: package.rust_version,
            targets,
            features,
            direct_dependencies,
        });
    }
    let members = member_indexes(metadata.workspace_members, &ids)?;
    if members.len() != packages.len() {
        return Err(invalid());
    }
    let defaults = member_indexes(metadata.workspace_default_members, &ids)?;
    if defaults.iter().any(|index| !members.contains(index)) {
        return Err(invalid());
    }
    let profiles = profiles(source)?;
    runtime.declared_toolchain = declared_toolchain(source)?;
    for text in [
        &runtime.platform,
        &runtime.image_id,
        &runtime.rust_version,
        &runtime.cargo_version,
    ] {
        bounded(text, 256)?;
    }
    runtime
        .image_id
        .parse::<SourceFingerprint>()
        .map_err(|_| invalid())?;
    let archive = super::source_archive::encode(source).map_err(InspectionError::Execution)?;
    let structure = ProjectStructure {
        workspace_members: members,
        workspace_default_members: defaults,
        packages,
        profiles,
        cargo_configuration: CargoConfiguration {
            project_config_policy: ProjectConfigPolicy::Rejected,
            frozen: true,
            offline: true,
            incremental: false,
            target_directory_ephemeral: true,
        },
        runtime,
        source_fingerprint: super::digest(&archive)
            .parse()
            .map_err(|_| InspectionError::Internal)?,
    };
    limit(
        serde_json::to_vec(&structure)
            .map_err(|_| InspectionError::Internal)?
            .len(),
        STRUCTURE_LIMIT,
    )?;
    Ok(structure)
}
fn member_indexes(
    values: Vec<String>,
    ids: &BTreeMap<String, u32>,
) -> Result<Vec<u32>, InspectionError> {
    let mut indexes = BTreeSet::new();
    for value in values {
        bounded(&value, 4096)?;
        if !indexes.insert(*ids.get(&value).ok_or_else(invalid)?) {
            return Err(invalid());
        }
    }
    Ok(indexes.into_iter().collect())
}

fn root_toml(source: &SourceBundle) -> Result<toml::Value, InspectionError> {
    let bytes = source_file("Cargo.toml", source)?.bytes();
    limit(bytes.len(), METADATA_LIMIT)?;
    toml::from_str(std::str::from_utf8(bytes).map_err(|_| invalid())?).map_err(|_| invalid())
}
pub(super) fn declared_toolchain(source: &SourceBundle) -> Result<Option<String>, InspectionError> {
    let mut selection = None;
    for file in source.files() {
        if file.path() == "rust-toolchain" {
            if std::str::from_utf8(file.bytes())
                .map_err(|_| invalid())?
                .trim()
                != "1.98.1"
            {
                return Err(invalid());
            }
            selection = Some("1.98.1".into());
        } else if file.path() == "rust-toolchain.toml" {
            limit(file.bytes().len(), METADATA_LIMIT)?;
            let value: toml::Value =
                toml::from_str(std::str::from_utf8(file.bytes()).map_err(|_| invalid())?)
                    .map_err(|_| invalid())?;
            let table = value.as_table().ok_or_else(invalid)?;
            let toolchain = table
                .get("toolchain")
                .and_then(toml::Value::as_table)
                .ok_or_else(invalid)?;
            if table.len() != 1
                || toolchain.len() != 1
                || toolchain.get("channel").and_then(toml::Value::as_str) != Some("1.98.1")
            {
                return Err(invalid());
            }
            selection = Some("1.98.1".into());
        }
    }
    Ok(selection)
}
fn profiles(source: &SourceBundle) -> Result<Vec<DeclaredProfile>, InspectionError> {
    let root = root_toml(source)?;
    let root = root.as_table().ok_or_else(invalid)?;
    let Some(raw) = root.get("profile") else {
        return Ok(Vec::new());
    };
    let table = raw.as_table().ok_or_else(invalid)?;
    limit(table.len(), PROFILE_LIMIT)?;
    let builtins = ["dev", "release", "test", "bench"];
    let mut profiles = Vec::new();
    for (name, value) in table {
        profile_name(name)?;
        let values = value.as_table().ok_or_else(invalid)?;
        let inherits = values
            .get("inherits")
            .map(|v| v.as_str().map(str::to_owned).ok_or_else(invalid))
            .transpose()?;
        if let Some(parent) = &inherits {
            profile_name(parent)?;
            if parent == name || !builtins.contains(&parent.as_str()) && !table.contains_key(parent)
            {
                return Err(invalid());
            }
        } else if !builtins.contains(&name.as_str()) {
            return Err(invalid());
        }
        if matches!(name.as_str(), "dev" | "release") && inherits.is_some() {
            return Err(invalid());
        }
        let mut settings = Vec::new();
        let mut package_overrides = Vec::new();
        let mut build_override = Vec::new();
        for (key, value) in values {
            match key.as_str() {
                "inherits" => {}
                "package" => {
                    let packages = value.as_table().ok_or_else(invalid)?;
                    limit(packages.len(), PACKAGE_LIMIT)?;
                    for (package, values) in packages {
                        package_profile_name(package)?;
                        package_overrides.push(PackageProfile {
                            package: package.clone(),
                            settings: profile_settings(
                                values.as_table().ok_or_else(invalid)?,
                                true,
                            )?,
                        });
                    }
                }
                "build-override" => {
                    build_override = profile_settings(value.as_table().ok_or_else(invalid)?, true)?
                }
                _ => settings.push(profile_setting(key, value, false)?),
            }
        }
        profiles.push(DeclaredProfile {
            name: name.clone(),
            inherits,
            settings,
            package_overrides,
            build_override,
        });
    }
    // Bound and reject inheritance cycles without inventing effective settings.
    for profile in &profiles {
        let mut seen = BTreeSet::new();
        let mut current = Some(profile.name.as_str());
        while let Some(name) = current {
            if !seen.insert(name) {
                return Err(invalid());
            }
            current = profiles
                .iter()
                .find(|p| p.name == name)
                .and_then(|p| p.inherits.as_deref());
        }
    }
    Ok(profiles)
}
fn profile_name(value: &str) -> Result<(), InspectionError> {
    name(value)?;
    if ["debug", "build-override", "package", "doc"].contains(&value) {
        return Err(invalid());
    }
    Ok(())
}
fn package_profile_name(value: &str) -> Result<(), InspectionError> {
    if value == "*" {
        return Ok(());
    }
    // URL-bearing PackageIdSpec forms cannot safely fit this public string field.
    let (package, version) = value
        .split_once('@')
        .or_else(|| value.split_once(':'))
        .map_or((value, None), |(n, v)| (n, Some(v)));
    name(package)?;
    if let Some(version) = version {
        bounded(version, 128)?;
        semver::Version::parse(version).map_err(|_| invalid())?;
    }
    Ok(())
}
fn profile_settings(
    table: &toml::map::Map<String, toml::Value>,
    override_: bool,
) -> Result<Vec<ProfileSetting>, InspectionError> {
    table
        .iter()
        .map(|(key, value)| profile_setting(key, value, override_))
        .collect()
}
fn profile_setting(
    key: &str,
    value: &toml::Value,
    override_: bool,
) -> Result<ProfileSetting, InspectionError> {
    if override_ && matches!(key, "panic" | "lto" | "rpath") {
        return Err(invalid());
    }
    let name = match key {
        "opt-level" => ProfileSettingName::OptLevel,
        "debug" => ProfileSettingName::Debug,
        "split-debuginfo" => ProfileSettingName::SplitDebuginfo,
        "strip" => ProfileSettingName::Strip,
        "debug-assertions" => ProfileSettingName::DebugAssertions,
        "overflow-checks" => ProfileSettingName::OverflowChecks,
        "lto" => ProfileSettingName::Lto,
        "panic" => ProfileSettingName::Panic,
        "incremental" => ProfileSettingName::Incremental,
        "codegen-units" => ProfileSettingName::CodegenUnits,
        "rpath" => ProfileSettingName::Rpath,
        _ => return Err(invalid()),
    };
    let valid = match key {
        "opt-level" => {
            value.as_integer().is_some_and(|v| (0..=3).contains(&v))
                || value.as_str().is_some_and(|v| matches!(v, "s" | "z"))
        }
        "debug" => {
            value.is_bool()
                || value.as_integer().is_some_and(|v| (0..=2).contains(&v))
                || value.as_str().is_some_and(|v| {
                    matches!(
                        v,
                        "none" | "limited" | "full" | "line-directives-only" | "line-tables-only"
                    )
                })
        }
        "split-debuginfo" => value
            .as_str()
            .is_some_and(|v| matches!(v, "off" | "packed" | "unpacked")),
        "strip" => {
            value.is_bool()
                || value
                    .as_str()
                    .is_some_and(|v| matches!(v, "none" | "debuginfo" | "symbols"))
        }
        "lto" => {
            value.is_bool()
                || value
                    .as_str()
                    .is_some_and(|v| matches!(v, "off" | "thin" | "fat"))
        }
        "panic" => value
            .as_str()
            .is_some_and(|v| matches!(v, "unwind" | "abort")),
        "codegen-units" => value
            .as_integer()
            .is_some_and(|v| v > 0 && v <= i64::from(u32::MAX)),
        _ => value.is_bool(),
    };
    if !valid {
        return Err(invalid());
    }
    let value = match value {
        toml::Value::Boolean(v) => ProfileValue::Boolean(*v),
        toml::Value::Integer(v) => ProfileValue::Integer(u32::try_from(*v).map_err(|_| invalid())?),
        toml::Value::String(v) => ProfileValue::Text(v.clone()),
        _ => return Err(invalid()),
    };
    Ok(ProfileSetting { name, value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    fn runtime() -> Result<RuntimeIdentity, InspectionError> {
        Ok(RuntimeIdentity {
            platform: "linux-aarch64".into(),
            image_id: super::super::digest(b"runtime"),
            configuration_fingerprint: super::super::digest(b"configuration")
                .parse()
                .map_err(|_| invalid())?,
            execution_fingerprint: super::super::digest(b"execution")
                .parse()
                .map_err(|_| invalid())?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        })
    }
    fn source(
        manifest_tail: &str,
        extra: &[(&str, &str)],
    ) -> Result<SourceBundle, InspectionError> {
        let manifest =
            format!("[package]\nname='root'\nversion='1.2.3'\nedition='2024'\n{manifest_tail}");
        let mut files = vec![
            SourceFile::new("Cargo.toml".into(), manifest.into_bytes()).map_err(|_| invalid())?,
            SourceFile::new("src/lib.rs".into(), b"pub fn f() {}".to_vec())
                .map_err(|_| invalid())?,
        ];
        for (path, contents) in extra {
            files.push(
                SourceFile::new((*path).into(), contents.as_bytes().to_vec())
                    .map_err(|_| invalid())?,
            );
        }
        SourceBundle::new(files).map_err(|_| invalid())
    }
    fn package(id: &str, name: &str, directory: &str) -> Value {
        let prefix = if directory.is_empty() {
            "/source".into()
        } else {
            format!("/source/{directory}")
        };
        json!({"id":id,"name":name,"version":"1.2.3","source":null,"manifest_path":format!("{prefix}/Cargo.toml"),"edition":"2024","rust_version":null,
            "dependencies":[],"features":{"default":[],"feat":[]},
            "targets":[{"name":name,"kind":["lib"],"crate_types":["lib"],"src_path":format!("{prefix}/src/lib.rs"),"edition":"2024","required-features":[],"test":true,"doctest":true}]})
    }
    fn metadata() -> Value {
        json!({"version":1,"resolve":null,"workspace_root":"/source","packages":[package("opaque:never-parse-me","root","")],"workspace_members":["opaque:never-parse-me"],"workspace_default_members":["opaque:never-parse-me"]})
    }
    fn dependency_json(kind: Value, origin: Value, path: Option<&str>) -> Value {
        let mut value = json!({"name":"dep","rename":null,"req":"^1.0","kind":kind,"optional":false,"uses_default_features":true,"features":[],"target":null,"source":origin,"registry":null});
        if let Some(path) = path {
            value["path"] = json!(path);
        }
        value
    }
    fn parse_value(
        value: &Value,
        source: &SourceBundle,
    ) -> Result<ProjectStructure, InspectionError> {
        parse(
            &serde_json::to_vec(value).map_err(|_| InspectionError::Internal)?,
            source,
            runtime()?,
        )
    }
    fn is_invalid(result: Result<ProjectStructure, InspectionError>) {
        assert!(matches!(result, Err(InspectionError::InvalidMetadata)));
    }
    #[test]
    fn minimal_metadata_has_nullable_msrv_fixed_policy_and_captured_digest()
    -> Result<(), InspectionError> {
        let source = source("", &[])?;
        let result = parse_value(&metadata(), &source)?;
        assert_eq!(result.packages[0].rust_version, None);
        assert_eq!(result.packages[0].manifest_path, "Cargo.toml");
        assert_eq!(result.packages[0].targets[0].source_path, "src/lib.rs");
        assert_eq!(result.workspace_members, vec![0]);
        assert!(
            result.cargo_configuration.frozen
                && result.cargo_configuration.offline
                && result.cargo_configuration.target_directory_ephemeral
        );
        assert!(!result.cargo_configuration.incremental);
        assert!(result.profiles.is_empty());
        assert_eq!(result.runtime.declared_toolchain, None);
        assert_eq!(
            result.source_fingerprint.to_string(),
            super::super::digest(
                &super::super::source_archive::encode(&source)
                    .map_err(InspectionError::Execution)?
            )
        );
        Ok(())
    }
    #[test]
    fn maps_opaque_ids_stably_and_preserves_cargo_resolved_workspace_inheritance()
    -> Result<(), InspectionError> {
        let source = source(
            "",
            &[
                (
                    "a/Cargo.toml",
                    "[package]\nname='a'\nversion.workspace=true\nedition.workspace=true\nrust-version.workspace=true\n",
                ),
                ("a/src/lib.rs", ""),
            ],
        )?;
        let mut value = metadata();
        let mut member = package("http://token:SECRET@opaque#changed-format", "a", "a");
        member["rust_version"] = json!("1.85");
        member["version"] = json!("2.3.4");
        member["edition"] = json!("2021");
        value["packages"]
            .as_array_mut()
            .ok_or_else(invalid)?
            .push(member);
        value["workspace_members"] = json!([
            "http://token:SECRET@opaque#changed-format",
            "opaque:never-parse-me"
        ]);
        value["workspace_default_members"] = json!(["http://token:SECRET@opaque#changed-format"]);
        let first = parse_value(&value, &source)?;
        value["packages"]
            .as_array_mut()
            .ok_or_else(invalid)?
            .reverse();
        value["workspace_members"]
            .as_array_mut()
            .ok_or_else(invalid)?
            .reverse();
        let second = parse_value(&value, &source)?;
        assert_eq!(
            serde_json::to_vec(&first).map_err(|_| invalid())?,
            serde_json::to_vec(&second).map_err(|_| invalid())?
        );
        assert_eq!(first.packages[1].version, "2.3.4");
        assert_eq!(first.packages[1].edition, RustEdition::E2021);
        assert_eq!(first.packages[1].rust_version.as_deref(), Some("1.85"));
        assert_eq!(first.workspace_default_members, vec![1]);
        assert!(
            !serde_json::to_string(&first)
                .map_err(|_| invalid())?
                .contains("SECRET")
        );
        Ok(())
    }
    #[test]
    fn malformed_unknown_semantics_and_missing_facts_fail_but_new_fields_are_tolerated()
    -> Result<(), InspectionError> {
        let source = source("", &[])?;
        for bytes in [b"{".as_slice(), b"null", b"[]", b"{}", b"{}{}"] {
            is_invalid(parse(bytes, &source, runtime()?));
        }
        for (key, replacement) in [
            ("version", json!(2)),
            ("resolve", json!({})),
            ("workspace_root", json!("/source/")),
        ] {
            let mut value = metadata();
            value[key] = replacement;
            is_invalid(parse_value(&value, &source));
        }
        for key in [
            "version",
            "resolve",
            "workspace_root",
            "workspace_members",
            "workspace_default_members",
        ] {
            let mut value = metadata();
            value.as_object_mut().ok_or_else(invalid)?.remove(key);
            is_invalid(parse_value(&value, &source));
        }
        for key in ["source", "rust_version", "edition", "features"] {
            let mut value = metadata();
            value["packages"][0]
                .as_object_mut()
                .ok_or_else(invalid)?
                .remove(key);
            is_invalid(parse_value(&value, &source));
        }
        let mut value = metadata();
        value["new-field"] = json!({"anything":[1,2]});
        value["packages"][0]["new-package-field"] = json!(true);
        value["packages"][0]["targets"][0]["new-target-field"] = json!(null);
        parse_value(&value, &source)?;
        Ok(())
    }
    #[test]
    fn duplicate_ids_members_names_paths_and_feature_keys_are_rejected()
    -> Result<(), InspectionError> {
        let source = source("", &[])?;
        for key in ["workspace_members", "workspace_default_members"] {
            let mut value = metadata();
            value[key] = json!(["opaque:never-parse-me", "opaque:never-parse-me"]);
            is_invalid(parse_value(&value, &source));
            value[key] = json!(["missing"]);
            is_invalid(parse_value(&value, &source));
        }
        let mut value = metadata();
        value["workspace_members"] = json!([]);
        is_invalid(parse_value(&value, &source));
        let mut value = metadata();
        let duplicate = value["packages"][0].clone();
        value["packages"]
            .as_array_mut()
            .ok_or_else(invalid)?
            .push(duplicate);
        is_invalid(parse_value(&value, &source));
        let encoded = serde_json::to_string(&metadata())
            .map_err(|_| invalid())?
            .replace(
                "\"features\":{\"default\":[],\"feat\":[]}",
                "\"features\":{\"same\":[],\"same\":[]}",
            );
        assert!(encoded.contains("\"same\":[],\"same\":[]"));
        is_invalid(parse(encoded.as_bytes(), &source, runtime()?));
        let mut value = metadata();
        let duplicate = value["packages"][0]["targets"][0].clone();
        value["packages"][0]["targets"]
            .as_array_mut()
            .ok_or_else(invalid)?
            .push(duplicate);
        is_invalid(parse_value(&value, &source));
        Ok(())
    }
    #[test]
    fn duplicate_package_identity_fields_are_each_rejected_independently()
    -> Result<(), InspectionError> {
        let source = source(
            "",
            &[
                ("a/Cargo.toml", "[package]\nname='a'"),
                ("a/src/lib.rs", ""),
            ],
        )?;
        for (field, duplicate) in [
            ("id", "opaque:never-parse-me"),
            ("manifest_path", "/source/Cargo.toml"),
            ("name", "root"),
        ] {
            let mut value = metadata();
            let mut second = package("second-id", "a", "a");
            second[field] = json!(duplicate);
            value["packages"]
                .as_array_mut()
                .ok_or_else(invalid)?
                .push(second);
            value["workspace_members"] = json!(["opaque:never-parse-me", "second-id"]);
            is_invalid(parse_value(&value, &source));
        }
        // Cargo package names allow Unicode alphanumeric characters; crates.io's
        // narrower publishing rules must not become local metadata restrictions.
        let mut value = metadata();
        value["packages"][0]["name"] = json!("café");
        parse_value(&value, &source)?;
        Ok(())
    }
    #[test]
    fn guest_paths_must_name_exact_captured_files_or_path_dependency_manifests()
    -> Result<(), InspectionError> {
        let source = source("", &[("emptydir/placeholder", "")])?;
        for path in [
            "/source-other/src/lib.rs",
            "/source/../src/lib.rs",
            "/source//src/lib.rs",
            "/source/./src/lib.rs",
            "src/lib.rs",
            "/source/missing.rs",
            "/source/src\\lib.rs",
        ] {
            let mut value = metadata();
            value["packages"][0]["targets"][0]["src_path"] = json!(path);
            is_invalid(parse_value(&value, &source));
        }
        for path in [
            "/source-other",
            "/source/../outside",
            "/source/emptydir",
            "/source/missing",
            ".",
        ] {
            let mut value = metadata();
            value["packages"][0]["dependencies"] =
                json!([dependency_json(Value::Null, Value::Null, Some(path))]);
            is_invalid(parse_value(&value, &source));
        }
        let mut value = metadata();
        value["packages"][0]["manifest_path"] = json!("/source/src/lib.rs");
        is_invalid(parse_value(&value, &source));
        Ok(())
    }
    #[test]
    fn versions_editions_names_targets_and_declared_features_are_validated()
    -> Result<(), InspectionError> {
        let source = source("", &[])?;
        for (key, bad) in [
            ("version", "1.0"),
            ("version", "1.2.3\n"),
            ("rust_version", "1.85-beta"),
            ("rust_version", "01.85"),
            ("rust_version", "1"),
            ("edition", "2030"),
            ("name", "injected\nname"),
        ] {
            let mut value = metadata();
            value["packages"][0][key] = json!(bad);
            is_invalid(parse_value(&value, &source));
        }
        for (key, bad) in [
            ("kind", json!(["future-kind"])),
            ("crate_types", json!(["test"])),
            ("kind", json!(["lib", "lib"])),
            ("required-features", json!(["missing"])),
            ("required-features", json!(["feat", "feat"])),
        ] {
            let mut value = metadata();
            value["packages"][0]["targets"][0][key] = bad;
            is_invalid(parse_value(&value, &source));
        }
        Ok(())
    }
    #[test]
    fn declarations_keep_rename_cfg_features_and_hash_complete_secret_origins()
    -> Result<(), InspectionError> {
        let source = source(
            "",
            &[
                ("dep/Cargo.toml", "[package]\nname='dep'"),
                ("dep/src/lib.rs", ""),
            ],
        )?;
        let mut value = metadata();
        let mut registry = dependency_json(
            Value::Null,
            json!("registry+https://user:SECRET@registry.invalid/index"),
            None,
        );
        registry["rename"] = json!("alias");
        registry["optional"] = json!(true);
        registry["uses_default_features"] = json!(false);
        registry["features"] = json!(["z", "a"]);
        registry["target"] = json!("cfg(unix)");
        registry["registry"] = json!("https://user:SECRET@registry.invalid/index");
        let git = dependency_json(
            json!("build"),
            json!("git+https://user:SECRET@git.invalid/repo?rev=abc#fullrevision"),
            None,
        );
        let path = dependency_json(json!("dev"), Value::Null, Some("/source/dep"));
        value["packages"][0]["dependencies"] = json!([registry, git, path]);
        let result = parse_value(&value, &source)?;
        let dependencies = &result.packages[0].direct_dependencies;
        assert_eq!(dependencies.len(), 3);
        let alias = dependencies
            .iter()
            .find(|d| d.rename.is_some())
            .ok_or_else(invalid)?;
        assert_eq!(alias.features, vec!["a", "z"]);
        assert!(alias.optional);
        assert!(!alias.uses_default_features);
        assert_eq!(alias.target_condition.as_deref(), Some("cfg(unix)"));
        assert!(
            dependencies
                .iter()
                .any(|d| d.kind == DeclaredDependencyKind::Build
                    && d.origin.kind == DependencySourceKind::Git)
        );
        assert!(
            dependencies
                .iter()
                .any(|d| d.kind == DeclaredDependencyKind::Dev
                    && d.origin.relative_path.as_deref() == Some("dep"))
        );
        let serialized = serde_json::to_string(&result).map_err(|_| invalid())?;
        assert!(
            !serialized.contains("SECRET")
                && !serialized.contains("https://")
                && !serialized.contains("/source/")
        );
        let previous = alias.origin.identity.clone();
        value["packages"][0]["dependencies"][0]["registry"] =
            json!("https://different.invalid/index");
        let changed = parse_value(&value, &source)?;
        assert_ne!(
            changed.packages[0]
                .direct_dependencies
                .iter()
                .find(|d| d.rename.is_some())
                .ok_or_else(invalid)?
                .origin
                .identity,
            previous
        );
        Ok(())
    }
    #[test]
    fn inconsistent_dependency_origins_kinds_requirements_and_duplicates_fail()
    -> Result<(), InspectionError> {
        let source = source("", &[])?;
        for dependency in [
            dependency_json(json!("future"), json!("registry+x"), None),
            dependency_json(Value::Null, json!("future+x"), None),
            dependency_json(Value::Null, Value::Null, None),
            dependency_json(Value::Null, json!("git+x"), Some("/source")),
        ] {
            let mut value = metadata();
            value["packages"][0]["dependencies"] = json!([dependency]);
            is_invalid(parse_value(&value, &source));
        }
        let mut dep = dependency_json(Value::Null, json!("registry+x"), None);
        dep["req"] = json!("not-semver");
        let mut value = metadata();
        value["packages"][0]["dependencies"] = json!([dep]);
        is_invalid(parse_value(&value, &source));
        let dep = dependency_json(Value::Null, json!("registry+x"), None);
        value["packages"][0]["dependencies"] = json!([dep, dep]);
        is_invalid(parse_value(&value, &source));
        Ok(())
    }
    #[test]
    fn root_profiles_are_declared_settings_with_typed_values_and_overrides()
    -> Result<(), InspectionError> {
        let source = source(
            "[profile.dev]\nopt-level=1\ndebug='line-tables-only'\nstrip=true\nsplit-debuginfo='off'\ndebug-assertions=true\noverflow-checks=false\nlto='thin'\npanic='unwind'\nincremental=true\ncodegen-units=16\nrpath=false\n[profile.dev.package.'dep:1.2.3']\nopt-level='z'\n[profile.dev.build-override]\ndebug=false\n[profile.custom]\ninherits='dev'\nopt-level=3\n",
            &[("rust-toolchain.toml", "[toolchain]\nchannel='1.98.1'\n")],
        )?;
        let result = parse_value(&metadata(), &source)?;
        assert_eq!(result.runtime.declared_toolchain.as_deref(), Some("1.98.1"));
        assert_eq!(result.profiles.len(), 2);
        let profile = result
            .profiles
            .iter()
            .find(|p| p.name == "dev")
            .ok_or_else(invalid)?;
        assert_eq!(profile.settings.len(), 11);
        assert_eq!(profile.package_overrides[0].package, "dep:1.2.3");
        assert!(matches!(
            profile.build_override[0].value,
            ProfileValue::Boolean(false)
        ));
        assert!(
            profile
                .settings
                .iter()
                .any(|s| s.name == ProfileSettingName::Incremental
                    && matches!(s.value, ProfileValue::Boolean(true)))
        );
        assert!(!result.cargo_configuration.incremental); // declared vs effective policy
        Ok(())
    }
    #[test]
    fn invalid_unknown_profiles_cycles_overrides_and_toolchain_are_rejected()
    -> Result<(), InspectionError> {
        for tail in [
            "[profile.dev]\nopt-level=4",
            "[profile.dev]\ndebug=3",
            "[profile.dev]\ncodegen-units=0",
            "[profile.dev]\npanic='crash'",
            "[profile.dev]\nunknown=true",
            "[profile.dev.package.dep]\nlto=true",
            "[profile.dev.build-override]\nrpath=true",
            "[profile.custom]\nopt-level=1",
            "[profile.custom]\ninherits='missing'",
            "[profile.a]\ninherits='b'\n[profile.b]\ninherits='a'",
            "[profile.dev.package.'https://user:SECRET@host/repo#dep']\nopt-level=1",
        ] {
            is_invalid(parse_value(&metadata(), &source(tail, &[])?));
        }
        for (path, contents) in [
            ("rust-toolchain", "stable"),
            (
                "rust-toolchain.toml",
                "[toolchain]\nchannel='1.98.1'\ncomponents=[]",
            ),
        ] {
            is_invalid(parse_value(&metadata(), &source("", &[(path, contents)])?));
        }
        Ok(())
    }
    #[test]
    fn profiles_and_override_counts_are_bounded_without_partial_results()
    -> Result<(), InspectionError> {
        let at_limit = (0..PROFILE_LIMIT)
            .map(|n| format!("[profile.p{n}]\ninherits='dev'\n"))
            .collect::<String>();
        assert_eq!(
            parse_value(&metadata(), &source(&at_limit, &[])?)?
                .profiles
                .len(),
            PROFILE_LIMIT
        );
        let over = format!("{at_limit}[profile.extra]\ninherits='dev'\n");
        assert!(matches!(
            parse_value(&metadata(), &source(&over, &[])?),
            Err(InspectionError::OutputLimit)
        ));
        let overrides = (0..=PACKAGE_LIMIT)
            .map(|n| format!("[profile.dev.package.dep{n}]\nopt-level=1\n"))
            .collect::<String>();
        assert!(matches!(
            parse_value(&metadata(), &source(&overrides, &[])?),
            Err(InspectionError::OutputLimit)
        ));
        Ok(())
    }
    #[test]
    fn input_collection_and_aggregate_public_output_budgets_fail_entire_result()
    -> Result<(), InspectionError> {
        let source = source("", &[])?;
        assert!(matches!(
            parse(&vec![b' '; METADATA_LIMIT + 1], &source, runtime()?),
            Err(InspectionError::OutputLimit)
        ));
        let mut value = metadata();
        value["packages"] = json!(vec![package("id", "root", ""); PACKAGE_LIMIT + 1]);
        assert!(matches!(
            parse_value(&value, &source),
            Err(InspectionError::OutputLimit)
        ));
        let mut value = metadata();
        let item = value["packages"][0]["targets"][0].clone();
        value["packages"][0]["targets"] = json!(vec![item; TARGET_LIMIT + 1]);
        assert!(matches!(
            parse_value(&value, &source),
            Err(InspectionError::OutputLimit)
        ));
        let mut value = metadata();
        value["packages"][0]["dependencies"] = json!(
            (0..513)
                .map(|n| {
                    let mut d = dependency_json(Value::Null, json!("registry+x"), None);
                    d["rename"] = json!(format!("alias{n}"));
                    d
                })
                .collect::<Vec<_>>()
        );
        assert!(matches!(
            parse_value(&value, &source),
            Err(InspectionError::OutputLimit)
        ));
        let mut value = metadata();
        value["packages"][0]["features"] = json!(
            (0..257)
                .map(|n| (format!("f{n}"), Vec::<String>::new()))
                .collect::<BTreeMap<_, _>>()
        );
        assert!(matches!(
            parse_value(&value, &source),
            Err(InspectionError::OutputLimit)
        ));
        // Valid under per-package/input limits, but output expands every declaration
        // with an origin identity. Aggregate public result must still be rejected.
        let mut value = metadata();
        value["packages"][0]["dependencies"] = json!(
            (0..512)
                .map(|n| {
                    let mut d = dependency_json(Value::Null, json!("registry+x"), None);
                    d["rename"] = json!(format!("alias{n}"));
                    d
                })
                .collect::<Vec<_>>()
        );
        assert!(serde_json::to_vec(&value).map_err(|_| invalid())?.len() < METADATA_LIMIT);
        assert!(matches!(
            parse_value(&value, &source),
            Err(InspectionError::OutputLimit)
        ));
        Ok(())
    }
}
