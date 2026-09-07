//! Semantic allowlist repeated at the publication boundary.

use rust_engineering_domain::{
    DependencyName, DependencyTarget, FeatureName, FeatureValue, MutationError,
};
use std::collections::BTreeSet;
use toml::{Table, Value};

const MAX_NAME_BYTES: usize = 128;
const MAX_TARGET_BYTES: usize = 256;
const MAX_FEATURES: usize = 128;

pub(crate) fn validate_manifest_patch(before: &[u8], after: &[u8]) -> Result<(), MutationError> {
    let mut before = parse(before)?;
    let mut after = parse(after)?;

    let before_local_lints = take_root(&mut before, "lints")?;
    let after_local_lints = take_root(&mut after, "lints")?;
    let before_features = take_root(&mut before, "features")?;
    let after_features = take_root(&mut after, "features")?;
    if before_local_lints != after_local_lints || before_features != after_features {
        require_package(&before)?;
        require_package(&after)?;
    }
    validate_lints_delta(before_local_lints, after_local_lints)?;
    validate_features_delta(before_features, after_features)?;
    validate_profiles_delta(
        take_root(&mut before, "profile")?,
        take_root(&mut after, "profile")?,
    )?;
    validate_workspace_dependencies_delta(
        take_workspace(&mut before, "dependencies"),
        take_workspace(&mut after, "dependencies"),
    )?;
    validate_lints_delta(
        take_workspace(&mut before, "lints"),
        take_workspace(&mut after, "lints"),
    )?;

    if before != after {
        return Err(MutationError::Invalid);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DependencyDelta {
    Add,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DependencyLocation {
    target: Option<String>,
    table: &'static str,
}

pub(crate) fn validate_dependency_delta(
    before: &[u8],
    after: &[u8],
    expected: DependencyDelta,
) -> Result<(), MutationError> {
    let before = parse(before)?;
    let after = parse(after)?;
    require_package(&before)?;
    require_package(&after)?;

    let locations = dependency_locations(&before, &after);
    let mut delta: Option<(DependencyLocation, String, Value)> = None;
    for location in locations {
        let before_table = dependency_table(&before, &location);
        let after_table = dependency_table(&after, &location);
        let names: BTreeSet<_> = before_table
            .into_iter()
            .flat_map(Table::keys)
            .chain(after_table.into_iter().flat_map(Table::keys))
            .map(String::as_str)
            .collect();
        for name in names {
            let before_value = before_table.and_then(|table| table.get(name));
            let after_value = after_table.and_then(|table| table.get(name));
            if before_value == after_value {
                continue;
            }
            let value = match (expected, before_value, after_value) {
                (DependencyDelta::Add, None, Some(value)) => value.clone(),
                (DependencyDelta::Remove, Some(value), None) => value.clone(),
                _ => return Err(MutationError::Invalid),
            };
            if delta
                .replace((location.clone(), name.to_owned(), value))
                .is_some()
            {
                return Err(MutationError::Invalid);
            }
        }
    }

    let (location, name, value) = delta.ok_or(MutationError::Invalid)?;
    DependencyName::new(name.clone()).map_err(|_| MutationError::Invalid)?;
    if let Some(target) = &location.target {
        if target.len() > MAX_TARGET_BYTES {
            return Err(MutationError::Invalid);
        }
        DependencyTarget::new(target.clone()).map_err(|_| MutationError::Invalid)?;
    }
    if expected == DependencyDelta::Add {
        validate_registry_dependency(&value, false)?;
    }

    let mut expected_after = before;
    match expected {
        DependencyDelta::Add => {
            dependency_table_mut(&mut expected_after, &location, true)?.insert(name, value);
        }
        DependencyDelta::Remove => {
            dependency_table_mut(&mut expected_after, &location, false)?
                .remove(&name)
                .ok_or(MutationError::Invalid)?;
        }
    }
    if expected_after != after {
        return Err(MutationError::Invalid);
    }
    Ok(())
}

fn parse(bytes: &[u8]) -> Result<Value, MutationError> {
    if bytes.len() > 256 * 1024 {
        return Err(MutationError::LimitExceeded);
    }
    toml::from_str(std::str::from_utf8(bytes).map_err(|_| MutationError::Invalid)?)
        .map_err(|_| MutationError::Invalid)
}

fn root(value: &Value) -> Result<&Table, MutationError> {
    value.as_table().ok_or(MutationError::Invalid)
}

fn root_mut(value: &mut Value) -> Result<&mut Table, MutationError> {
    value.as_table_mut().ok_or(MutationError::Invalid)
}

fn require_package(value: &Value) -> Result<(), MutationError> {
    root(value)?
        .get("package")
        .and_then(Value::as_table)
        .ok_or(MutationError::Invalid)
        .map(|_| ())
}

fn take_root(value: &mut Value, name: &str) -> Result<Option<Value>, MutationError> {
    Ok(root_mut(value)?.remove(name))
}

fn take_workspace(value: &mut Value, name: &str) -> Option<Value> {
    value
        .as_table_mut()
        .and_then(|root| root.get_mut("workspace"))
        .and_then(Value::as_table_mut)
        .and_then(|workspace| workspace.remove(name))
}

fn changed_keys<'a>(before: &'a Table, after: &'a Table) -> BTreeSet<&'a str> {
    before
        .keys()
        .chain(after.keys())
        .map(String::as_str)
        .filter(|key| before.get(*key) != after.get(*key))
        .collect()
}

fn tables_or_empty<'a>(
    before: Option<&'a Value>,
    after: Option<&'a Value>,
) -> Result<(&'a Table, &'a Table), MutationError> {
    static EMPTY: std::sync::LazyLock<Table> = std::sync::LazyLock::new(Table::new);
    let before = before
        .map(|value| value.as_table().ok_or(MutationError::Invalid))
        .transpose()?
        .unwrap_or(&EMPTY);
    let after = after
        .map(|value| value.as_table().ok_or(MutationError::Invalid))
        .transpose()?
        .unwrap_or(&EMPTY);
    Ok((before, after))
}

fn validate_lints_delta(before: Option<Value>, after: Option<Value>) -> Result<(), MutationError> {
    if before == after {
        return Ok(());
    }
    let (before, after) = tables_or_empty(before.as_ref(), after.as_ref())?;
    for namespace in changed_keys(before, after) {
        if !matches!(namespace, "rust" | "clippy") {
            return Err(MutationError::Invalid);
        }
        let (before_lints, after_lints) =
            tables_or_empty(before.get(namespace), after.get(namespace))?;
        for name in changed_keys(before_lints, after_lints) {
            valid_lint_name(name)?;
            if let Some(setting) = after_lints.get(name) {
                validate_lint_setting(setting)?;
            }
        }
    }
    Ok(())
}

fn valid_lint_name(name: &str) -> Result<(), MutationError> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(MutationError::Invalid);
    }
    Ok(())
}

fn validate_lint_setting(setting: &Value) -> Result<(), MutationError> {
    if setting
        .as_str()
        .is_some_and(|level| matches!(level, "allow" | "warn" | "deny" | "forbid"))
    {
        return Ok(());
    }
    let setting = setting.as_table().ok_or(MutationError::Invalid)?;
    if setting.is_empty()
        || setting.len() > 2
        || !setting
            .keys()
            .all(|key| matches!(key.as_str(), "level" | "priority"))
        || !setting
            .get("level")
            .and_then(Value::as_str)
            .is_some_and(|level| matches!(level, "allow" | "warn" | "deny" | "forbid"))
        || setting
            .get("priority")
            .is_some_and(|priority| priority.as_integer().is_none())
    {
        return Err(MutationError::Invalid);
    }
    Ok(())
}

fn validate_features_delta(
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), MutationError> {
    if before == after {
        return Ok(());
    }
    let (before, after) = tables_or_empty(before.as_ref(), after.as_ref())?;
    for name in changed_keys(before, after) {
        FeatureName::new(name.to_owned()).map_err(|_| MutationError::Invalid)?;
        let Some(value) = after.get(name) else {
            continue;
        };
        let values = value.as_array().ok_or(MutationError::Invalid)?;
        if values.len() > MAX_FEATURES {
            return Err(MutationError::Invalid);
        }
        let mut unique = BTreeSet::new();
        for value in values {
            let value = value.as_str().ok_or(MutationError::Invalid)?;
            FeatureValue::new(value.to_owned()).map_err(|_| MutationError::Invalid)?;
            if !unique.insert(value) {
                return Err(MutationError::Invalid);
            }
        }
    }
    Ok(())
}

fn validate_profiles_delta(
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), MutationError> {
    if before == after {
        return Ok(());
    }
    let (before, after) = tables_or_empty(before.as_ref(), after.as_ref())?;
    for profile in changed_keys(before, after) {
        if !matches!(profile, "dev" | "release" | "test" | "bench") {
            return Err(MutationError::Invalid);
        }
        let (before_settings, after_settings) =
            tables_or_empty(before.get(profile), after.get(profile))?;
        for setting in changed_keys(before_settings, after_settings) {
            if let Some(value) = after_settings.get(setting) {
                validate_profile_setting(setting, value)?;
            } else if !is_profile_setting(setting) {
                return Err(MutationError::Invalid);
            }
        }
    }
    Ok(())
}

fn is_profile_setting(name: &str) -> bool {
    matches!(
        name,
        "opt-level"
            | "debug"
            | "strip"
            | "debug-assertions"
            | "overflow-checks"
            | "lto"
            | "panic"
            | "incremental"
            | "codegen-units"
    )
}

fn validate_profile_setting(name: &str, value: &Value) -> Result<(), MutationError> {
    let valid = match name {
        "opt-level" => {
            value
                .as_integer()
                .is_some_and(|value| (0..=3).contains(&value))
                || value
                    .as_str()
                    .is_some_and(|value| matches!(value, "s" | "z"))
        }
        "debug" => {
            value.as_bool().is_some()
                || value
                    .as_integer()
                    .is_some_and(|value| (0..=2).contains(&value))
                || value.as_str().is_some_and(|value| {
                    matches!(
                        value,
                        "none" | "limited" | "full" | "line-tables-only" | "line-directives-only"
                    )
                })
        }
        "strip" => {
            value.as_bool().is_some()
                || value
                    .as_str()
                    .is_some_and(|value| matches!(value, "none" | "debuginfo" | "symbols"))
        }
        "debug-assertions" | "overflow-checks" | "incremental" => value.as_bool().is_some(),
        "lto" => {
            value.as_bool().is_some()
                || value
                    .as_str()
                    .is_some_and(|value| matches!(value, "off" | "thin" | "fat"))
        }
        "panic" => value
            .as_str()
            .is_some_and(|value| matches!(value, "unwind" | "abort")),
        "codegen-units" => value
            .as_integer()
            .is_some_and(|value| (1..=i64::from(u32::MAX)).contains(&value)),
        _ => false,
    };
    if !valid {
        return Err(MutationError::Invalid);
    }
    Ok(())
}

fn validate_workspace_dependencies_delta(
    before: Option<Value>,
    after: Option<Value>,
) -> Result<(), MutationError> {
    if before == after {
        return Ok(());
    }
    let (before, after) = tables_or_empty(before.as_ref(), after.as_ref())?;
    for name in changed_keys(before, after) {
        DependencyName::new(name.to_owned()).map_err(|_| MutationError::Invalid)?;
        if let Some(value) = after.get(name) {
            validate_registry_dependency(value, true)?;
        }
    }
    Ok(())
}

fn validate_registry_dependency(value: &Value, workspace: bool) -> Result<(), MutationError> {
    if let Some(requirement) = value.as_str() {
        return validate_requirement(requirement);
    }
    let table = value.as_table().ok_or(MutationError::Invalid)?;
    if table.keys().any(|key| {
        !matches!(
            key.as_str(),
            "version" | "package" | "features" | "optional" | "default-features"
        )
    }) || workspace && table.contains_key("optional")
    {
        return Err(MutationError::Invalid);
    }
    validate_requirement(
        table
            .get("version")
            .and_then(Value::as_str)
            .ok_or(MutationError::Invalid)?,
    )?;
    if let Some(package) = table.get("package") {
        DependencyName::new(package.as_str().ok_or(MutationError::Invalid)?.to_owned())
            .map_err(|_| MutationError::Invalid)?;
    }
    if let Some(features) = table.get("features") {
        let features = features.as_array().ok_or(MutationError::Invalid)?;
        if features.len() > MAX_FEATURES {
            return Err(MutationError::Invalid);
        }
        let mut unique = BTreeSet::new();
        for feature in features {
            let feature = feature.as_str().ok_or(MutationError::Invalid)?;
            FeatureName::new(feature.to_owned()).map_err(|_| MutationError::Invalid)?;
            if !unique.insert(feature) {
                return Err(MutationError::Invalid);
            }
        }
    }
    for key in ["optional", "default-features"] {
        if table
            .get(key)
            .is_some_and(|value| value.as_bool().is_none())
        {
            return Err(MutationError::Invalid);
        }
    }
    Ok(())
}

fn validate_requirement(requirement: &str) -> Result<(), MutationError> {
    if requirement.is_empty()
        || requirement.len() > MAX_NAME_BYTES
        || !requirement
            .bytes()
            .all(|byte| (b' '..=b'~').contains(&byte))
        || semver::VersionReq::parse(requirement).is_err()
    {
        return Err(MutationError::Invalid);
    }
    Ok(())
}

fn dependency_locations(before: &Value, after: &Value) -> BTreeSet<DependencyLocation> {
    let mut locations = BTreeSet::new();
    for table in dependency_table_names() {
        locations.insert(DependencyLocation {
            target: None,
            table,
        });
    }
    for document in [before, after] {
        let Some(targets) = document
            .as_table()
            .and_then(|root| root.get("target"))
            .and_then(Value::as_table)
        else {
            continue;
        };
        for target in targets.keys() {
            for table in dependency_table_names() {
                locations.insert(DependencyLocation {
                    target: Some(target.clone()),
                    table,
                });
            }
        }
    }
    locations
}

fn dependency_table_names() -> [&'static str; 3] {
    ["dependencies", "dev-dependencies", "build-dependencies"]
}

fn dependency_table<'a>(document: &'a Value, location: &DependencyLocation) -> Option<&'a Table> {
    let root = document.as_table()?;
    let parent = match &location.target {
        None => root,
        Some(target) => root.get("target")?.as_table()?.get(target)?.as_table()?,
    };
    parent.get(location.table)?.as_table()
}

fn dependency_table_mut<'a>(
    document: &'a mut Value,
    location: &DependencyLocation,
    create: bool,
) -> Result<&'a mut Table, MutationError> {
    let root = root_mut(document)?;
    let parent = match &location.target {
        None => root,
        Some(target) => {
            if create && !root.contains_key("target") {
                root.insert("target".to_owned(), Value::Table(Table::new()));
            }
            let targets = root
                .get_mut("target")
                .and_then(Value::as_table_mut)
                .ok_or(MutationError::Invalid)?;
            if create && !targets.contains_key(target) {
                targets.insert(target.clone(), Value::Table(Table::new()));
            }
            targets
                .get_mut(target)
                .and_then(Value::as_table_mut)
                .ok_or(MutationError::Invalid)?
        }
    };
    if create && !parent.contains_key(location.table) {
        parent.insert(location.table.to_owned(), Value::Table(Table::new()));
    }
    parent
        .get_mut(location.table)
        .and_then(Value::as_table_mut)
        .ok_or(MutationError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_families_preserve_unknown_neighbors() {
        let before = br#"
[package]
name = "demo"
[package.metadata.private]
value = "keep"
[features]
old = ["legacy?/x"]
[profile.release]
unknown = "keep"
[workspace.dependencies]
legacy = { path = "vendor/legacy" }
"#;
        let after = br#"
[package]
name = "demo"
[package.metadata.private]
value = "keep"
[features]
old = ["legacy?/x"]
new = ["dep:serde"]
[profile.release]
unknown = "keep"
lto = "thin"
[workspace.dependencies]
legacy = { path = "vendor/legacy" }
serde = { version = "1", features = ["derive"] }
"#;
        assert_eq!(validate_manifest_patch(before, after), Ok(()));
    }

    #[test]
    fn manifest_patch_rejects_changed_unknowns_and_forbidden_sources() {
        let before = b"[package]\nname = \"demo\"\n";
        for after in [
            b"[package]\nname = \"other\"\n".as_slice(),
            b"[package]\nname = \"demo\"\n[features]\nx = [\"bad value\"]\n".as_slice(),
            b"[package]\nname = \"demo\"\n[profile.custom]\nlto = true\n".as_slice(),
            b"[package]\nname = \"demo\"\n[workspace.dependencies]\nx = { git = \"https://example.invalid\" }\n".as_slice(),
            b"[package]\nname = \"demo\"\n[workspace.dependencies]\nx = { version = \"1\", optional = false }\n".as_slice(),
        ] {
            assert_eq!(validate_manifest_patch(before, after), Err(MutationError::Invalid));
        }
    }

    #[test]
    fn dependency_delta_is_exactly_one_add_or_remove() {
        let before = b"[package]\nname = \"demo\"\n[package.metadata]\nkeep = true\n";
        let after = b"[package]\nname = \"demo\"\n[package.metadata]\nkeep = true\n[target.'cfg(unix)'.build-dependencies]\nsys = { version = \"1\", package = \"libc\", optional = true, default-features = false }\n";
        assert_eq!(
            validate_dependency_delta(before, after, DependencyDelta::Add),
            Ok(())
        );
        let removed = b"[package]\nname = \"demo\"\n[package.metadata]\nkeep = true\n[target.'cfg(unix)'.build-dependencies]\n";
        assert_eq!(
            validate_dependency_delta(after, removed, DependencyDelta::Remove),
            Ok(())
        );
    }

    #[test]
    fn dependency_delta_rejects_replacement_second_key_and_unknown_change() {
        let before = b"[package]\nname = \"demo\"\n[dependencies]\na = \"1\"\n";
        for after in [
            b"[package]\nname = \"demo\"\n[dependencies]\na = \"2\"\n".as_slice(),
            b"[package]\nname = \"demo\"\n[dependencies]\na = \"1\"\nb = \"1\"\nc = \"1\"\n"
                .as_slice(),
            b"[package]\nname = \"changed\"\n[dependencies]\na = \"1\"\nb = \"1\"\n".as_slice(),
            b"[package]\nname = \"demo\"\n[dependencies]\na = \"1\"\nb = { path = \"../b\" }\n"
                .as_slice(),
            b"[package]\nname = \"demo\"\n[dependencies]\na = \"1\"\nb = { version = \"1\", registry = \"private\" }\n"
                .as_slice(),
            b"[package]\nname = \"demo\"\n[dependencies]\na = \"1\"\nb = { version = \"1\", registry-index = \"https://example.invalid\" }\n"
                .as_slice(),
        ] {
            assert_eq!(
                validate_dependency_delta(before, after, DependencyDelta::Add),
                Err(MutationError::Invalid)
            );
        }
    }

    #[test]
    fn profile_delta_rejects_overrides_inherits_and_changed_unknown_settings() {
        let before = b"[package]\nname = \"demo\"\n[profile.release]\nfuture = \"keep\"\n";
        for after in [
            b"[package]\nname = \"demo\"\n[profile.release]\nfuture = \"changed\"\nlto = \"thin\"\n".as_slice(),
            b"[package]\nname = \"demo\"\n[profile.release]\nfuture = \"keep\"\ninherits = \"dev\"\n".as_slice(),
            b"[package]\nname = \"demo\"\n[profile.release]\nfuture = \"keep\"\n[profile.release.package.demo]\nopt-level = 3\n".as_slice(),
        ] {
            assert_eq!(validate_manifest_patch(before, after), Err(MutationError::Invalid));
        }
    }

    #[test]
    fn removing_an_existing_non_registry_dependency_is_still_one_local_remove() {
        let before = b"[package]\nname = \"demo\"\n[dependencies]\nold = { path = \"vendor/old\" }\nkeep = \"1\"\n";
        let after = b"[package]\nname = \"demo\"\n[dependencies]\nkeep = \"1\"\n";
        assert_eq!(
            validate_dependency_delta(before, after, DependencyDelta::Remove),
            Ok(())
        );
    }
}
