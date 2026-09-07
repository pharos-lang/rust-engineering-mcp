use rust_engineering_application::ManifestEditor;
use rust_engineering_domain::{
    BuiltinProfile, DependencyKind, DependencySpec, LintLevel, LintScope, LintTool, ManifestEdit,
    ManifestEditError, ProfileDebugInfo, ProfileLto, ProfileOptLevel, ProfilePanic,
    ProfileSettingEdit, ProfileSettingKey, ProfileStrip,
};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value};

const MAX_MANIFEST_BYTES: usize = 256 * 1024;

/// Format-preserving transformation of captured manifest bytes.
///
/// This adapter does not read files, confer write authority, or validate Cargo
/// semantics. Callers remain responsible for the Cargo oracle and transaction.
#[derive(Clone, Copy, Debug, Default)]
pub struct TomlManifestEditor;

impl ManifestEditor for TomlManifestEditor {
    fn apply(&self, before: &[u8], edit: &ManifestEdit) -> Result<Vec<u8>, ManifestEditError> {
        if before.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestEditError::LimitExceeded);
        }
        let input = std::str::from_utf8(before).map_err(|_| ManifestEditError::InvalidManifest)?;
        let newline_style = newline_style(input);
        let mut document = input
            .parse::<DocumentMut>()
            .map_err(|_| ManifestEditError::InvalidManifest)?;
        let original_roundtrip = document.to_string();
        let preserves_original = match newline_style {
            NewlineStyle::Crlf => restore_crlf(&original_roundtrip) == before,
            NewlineStyle::Lf | NewlineStyle::None => original_roundtrip == input,
            NewlineStyle::Mixed => false,
        };

        let changed = match edit {
            ManifestEdit::LintSet {
                scope,
                tool,
                name,
                level,
                priority,
            } => set_lint(
                &mut document,
                *scope,
                *tool,
                name.as_str(),
                *level,
                *priority,
            )?,
            ManifestEdit::LintRemove { scope, tool, name } => {
                remove_lint(&mut document, *scope, *tool, name.as_str())?
            }
            ManifestEdit::FeatureSet { name, values } => {
                set_feature(&mut document, name.as_str(), values)?
            }
            ManifestEdit::FeatureRemove { name } => remove_feature(&mut document, name.as_str())?,
            ManifestEdit::ProfileSet { profile, setting } => {
                set_profile(&mut document, *profile, *setting)?
            }
            ManifestEdit::ProfileRemove { profile, setting } => {
                remove_profile(&mut document, *profile, *setting)?
            }
            ManifestEdit::WorkspaceDependencySet { name, spec } => {
                set_workspace_dependency(&mut document, name.as_str(), spec)?
            }
            ManifestEdit::WorkspaceDependencyRemove { name } => {
                remove_workspace_dependency(&mut document, name.as_str())?
            }
            ManifestEdit::DependencyAdd {
                kind,
                target,
                name,
                spec,
            } => add_dependency(
                &mut document,
                *kind,
                target.as_ref().map(|value| value.as_str()),
                name.as_str(),
                spec,
            )?,
            ManifestEdit::DependencyRemove { kind, target, name } => remove_dependency(
                &mut document,
                *kind,
                target.as_ref().map(|value| value.as_str()),
                name.as_str(),
            )?,
        };
        if !changed {
            return Ok(before.to_vec());
        }
        if !preserves_original {
            return Err(ManifestEditError::UnsupportedLayout);
        }

        let serialized = document.to_string();
        let after = match newline_style {
            NewlineStyle::Crlf => restore_crlf(&serialized),
            NewlineStyle::Mixed => return Err(ManifestEditError::UnsupportedLayout),
            NewlineStyle::Lf | NewlineStyle::None => serialized.into_bytes(),
        };
        if after.len() > MAX_MANIFEST_BYTES {
            return Err(ManifestEditError::LimitExceeded);
        }
        std::str::from_utf8(&after)
            .map_err(|_| ManifestEditError::InvalidManifest)?
            .parse::<DocumentMut>()
            .map_err(|_| ManifestEditError::InvalidManifest)?;
        Ok(after)
    }
}

fn restore_crlf(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut restored = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte == b'\n' && (index == 0 || bytes[index - 1] != b'\r') {
            restored.push(b'\r');
        }
        restored.push(byte);
    }
    restored
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NewlineStyle {
    None,
    Lf,
    Crlf,
    Mixed,
}

fn newline_style(input: &str) -> NewlineStyle {
    let bytes = input.as_bytes();
    let mut index = 0;
    let mut saw_lf = false;
    let mut saw_crlf = false;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => {
                saw_crlf = true;
                index += 2;
            }
            b'\n' => {
                saw_lf = true;
                index += 1;
            }
            b'\r' => return NewlineStyle::Mixed,
            _ => index += 1,
        }
    }
    match (saw_lf, saw_crlf) {
        (false, false) => NewlineStyle::None,
        (true, false) => NewlineStyle::Lf,
        (false, true) => NewlineStyle::Crlf,
        (true, true) => NewlineStyle::Mixed,
    }
}

fn tool_name(tool: LintTool) -> &'static str {
    match tool {
        LintTool::Rust => "rust",
        LintTool::Clippy => "clippy",
    }
}

fn level_name(level: LintLevel) -> &'static str {
    match level {
        LintLevel::Allow => "allow",
        LintLevel::Warn => "warn",
        LintLevel::Deny => "deny",
        LintLevel::Forbid => "forbid",
    }
}

fn standard_table(item: &Item) -> Result<&Table, ManifestEditError> {
    let table = item
        .as_table()
        .ok_or(ManifestEditError::UnsupportedLayout)?;
    if table.is_dotted() {
        return Err(ManifestEditError::UnsupportedLayout);
    }
    Ok(table)
}

fn standard_table_mut(item: &mut Item) -> Result<&mut Table, ManifestEditError> {
    let table = item
        .as_table_mut()
        .ok_or(ManifestEditError::UnsupportedLayout)?;
    if table.is_dotted() {
        return Err(ManifestEditError::UnsupportedLayout);
    }
    Ok(table)
}

fn package_lints(root: &Table) -> Result<Option<&Table>, ManifestEditError> {
    if root.contains_key("workspace") && !root.contains_key("package") {
        return Err(ManifestEditError::InvalidOperation);
    }
    let Some(item) = root.get("lints") else {
        return Ok(None);
    };
    let lints = standard_table(item)?;
    if let Some(inherit) = lints.get("workspace") {
        if inherit.as_bool() == Some(true) {
            return Err(ManifestEditError::InheritedLints);
        }
        return Err(ManifestEditError::InvalidManifest);
    }
    Ok(Some(lints))
}

fn workspace_lints(root: &Table) -> Result<Option<&Table>, ManifestEditError> {
    let workspace = root
        .get("workspace")
        .ok_or(ManifestEditError::InvalidOperation)
        .and_then(standard_table)?;
    workspace.get("lints").map(standard_table).transpose()
}

fn lint_table(
    document: &DocumentMut,
    scope: LintScope,
    tool: LintTool,
) -> Result<Option<&Table>, ManifestEditError> {
    let lints = match scope {
        LintScope::Package => package_lints(document.as_table())?,
        LintScope::Workspace => workspace_lints(document.as_table())?,
    };
    lints
        .and_then(|table| table.get(tool_name(tool)))
        .map(standard_table)
        .transpose()
}

fn insert_standard_table(parent: &mut Table, name: &str) {
    parent.insert(name, Item::Table(Table::new()));
}

fn insert_implicit_table(parent: &mut Table, name: &str) {
    let mut table = Table::new();
    table.set_implicit(true);
    parent.insert(name, Item::Table(table));
}

fn lint_table_mut(
    document: &mut DocumentMut,
    scope: LintScope,
    tool: LintTool,
) -> Result<&mut Table, ManifestEditError> {
    let root = document.as_table_mut();
    let lints = match scope {
        LintScope::Package => {
            if root.get("lints").is_none() {
                insert_implicit_table(root, "lints");
            }
            root.get_mut("lints")
                .ok_or(ManifestEditError::InvalidManifest)
                .and_then(standard_table_mut)?
        }
        LintScope::Workspace => {
            let workspace = root
                .get_mut("workspace")
                .ok_or(ManifestEditError::InvalidOperation)
                .and_then(standard_table_mut)?;
            if workspace.get("lints").is_none() {
                insert_implicit_table(workspace, "lints");
            }
            workspace
                .get_mut("lints")
                .ok_or(ManifestEditError::InvalidManifest)
                .and_then(standard_table_mut)?
        }
    };
    let tool = tool_name(tool);
    if lints.get(tool).is_none() {
        insert_standard_table(lints, tool);
    }
    lints
        .get_mut(tool)
        .ok_or(ManifestEditError::InvalidManifest)
        .and_then(standard_table_mut)
}

fn parsed_level(value: &str) -> Option<LintLevel> {
    match value {
        "allow" => Some(LintLevel::Allow),
        "warn" => Some(LintLevel::Warn),
        "deny" => Some(LintLevel::Deny),
        "forbid" => Some(LintLevel::Forbid),
        _ => None,
    }
}

fn lint_setting(value: &Value) -> Result<(LintLevel, Option<i64>), ManifestEditError> {
    if let Some(level) = value.as_str().and_then(parsed_level) {
        return Ok((level, None));
    }
    let detail = value
        .as_inline_table()
        .ok_or(ManifestEditError::InvalidManifest)?;
    if !(detail.len() == 1 || detail.len() == 2)
        || detail
            .iter()
            .any(|(key, _)| !matches!(key, "level" | "priority"))
    {
        return Err(ManifestEditError::InvalidManifest);
    }
    let level = detail
        .get("level")
        .and_then(Value::as_str)
        .and_then(parsed_level)
        .ok_or(ManifestEditError::InvalidManifest)?;
    let priority = detail
        .get("priority")
        .map(|value| value.as_integer().ok_or(ManifestEditError::InvalidManifest))
        .transpose()?;
    Ok((level, priority))
}

fn lint_value(level: LintLevel, priority: Option<i64>) -> Value {
    match priority {
        None => Value::from(level_name(level)),
        Some(priority) => {
            let mut detail = InlineTable::new();
            detail.insert("level", Value::from(level_name(level)));
            detail.insert("priority", Value::from(priority));
            detail.fmt();
            Value::InlineTable(detail)
        }
    }
}

fn set_lint(
    document: &mut DocumentMut,
    scope: LintScope,
    tool: LintTool,
    name: &str,
    level: LintLevel,
    priority: Option<i64>,
) -> Result<bool, ManifestEditError> {
    if let Some(existing) = lint_table(document, scope, tool)?.and_then(|table| table.get(name)) {
        let existing = existing
            .as_value()
            .ok_or(ManifestEditError::UnsupportedLayout)?;
        let (existing_level, existing_priority) = lint_setting(existing)?;
        if existing_level == level && existing_priority.unwrap_or(0) == priority.unwrap_or(0) {
            return Ok(false);
        }
    }

    let table = lint_table_mut(document, scope, tool)?;
    let mut replacement = lint_value(level, priority);
    if let Some(existing) = table.get_mut(name) {
        let existing_value = existing
            .as_value()
            .ok_or(ManifestEditError::UnsupportedLayout)?;
        *replacement.decor_mut() = existing_value.decor().clone();
        *existing = Item::Value(replacement);
    } else {
        table.insert(name, Item::Value(replacement));
    }
    Ok(true)
}

fn remove_lint(
    document: &mut DocumentMut,
    scope: LintScope,
    tool: LintTool,
    name: &str,
) -> Result<bool, ManifestEditError> {
    let Some(existing) = lint_table(document, scope, tool)?.and_then(|table| table.get(name))
    else {
        return Ok(false);
    };
    if existing.as_value().is_none() {
        return Err(ManifestEditError::UnsupportedLayout);
    }
    let removed = lint_table_mut(document, scope, tool)?.remove(name);
    if removed.is_none() {
        return Err(ManifestEditError::InvalidManifest);
    }
    Ok(true)
}

fn package_manifest(root: &Table) -> Result<(), ManifestEditError> {
    let package = root
        .get("package")
        .ok_or(ManifestEditError::InvalidOperation)?;
    standard_table(package)?;
    Ok(())
}

fn child_table<'a>(parent: &'a Table, name: &str) -> Result<Option<&'a Table>, ManifestEditError> {
    parent.get(name).map(standard_table).transpose()
}

fn child_table_mut<'a>(
    parent: &'a mut Table,
    name: &str,
    implicit_when_created: bool,
) -> Result<&'a mut Table, ManifestEditError> {
    if parent.get(name).is_none() {
        if implicit_when_created {
            insert_implicit_table(parent, name);
        } else {
            insert_standard_table(parent, name);
        }
    }
    parent
        .get_mut(name)
        .ok_or(ManifestEditError::InvalidManifest)
        .and_then(standard_table_mut)
}

fn replace_value(
    table: &mut Table,
    name: &str,
    mut replacement: Value,
) -> Result<(), ManifestEditError> {
    if let Some(existing) = table.get_mut(name) {
        let existing = existing
            .as_value()
            .ok_or(ManifestEditError::UnsupportedLayout)?;
        *replacement.decor_mut() = existing.decor().clone();
        *table
            .get_mut(name)
            .ok_or(ManifestEditError::InvalidManifest)? = Item::Value(replacement);
    } else {
        table.insert(name, Item::Value(replacement));
    }
    Ok(())
}

fn validate_feature_values(
    values: &[rust_engineering_domain::FeatureValue],
) -> Result<(), ManifestEditError> {
    if values.len() > 128 {
        return Err(ManifestEditError::InvalidOperation);
    }
    let mut names = std::collections::BTreeSet::new();
    if values.iter().any(|value| !names.insert(value.as_str())) {
        return Err(ManifestEditError::InvalidOperation);
    }
    Ok(())
}

fn feature_values(item: &Item) -> Result<Vec<&str>, ManifestEditError> {
    item.as_value()
        .ok_or(ManifestEditError::UnsupportedLayout)?
        .as_array()
        .ok_or(ManifestEditError::InvalidManifest)?
        .iter()
        .map(|value| value.as_str().ok_or(ManifestEditError::InvalidManifest))
        .collect()
}

fn feature_array(values: &[rust_engineering_domain::FeatureValue]) -> Value {
    let mut array = Array::new();
    for value in values {
        array.push(value.as_str());
    }
    Value::Array(array)
}

fn features_table(document: &DocumentMut) -> Result<Option<&Table>, ManifestEditError> {
    package_manifest(document.as_table())?;
    child_table(document.as_table(), "features")
}

fn set_feature(
    document: &mut DocumentMut,
    name: &str,
    values: &[rust_engineering_domain::FeatureValue],
) -> Result<bool, ManifestEditError> {
    validate_feature_values(values)?;
    if let Some(existing) = features_table(document)?.and_then(|table| table.get(name)) {
        let existing = feature_values(existing)?;
        if existing
            .iter()
            .copied()
            .eq(values.iter().map(|value| value.as_str()))
        {
            return Ok(false);
        }
    }
    let table = child_table_mut(document.as_table_mut(), "features", false)?;
    replace_value(table, name, feature_array(values))?;
    Ok(true)
}

fn remove_feature(document: &mut DocumentMut, name: &str) -> Result<bool, ManifestEditError> {
    let Some(existing) = features_table(document)?.and_then(|table| table.get(name)) else {
        return Ok(false);
    };
    feature_values(existing)?;
    let table = child_table_mut(document.as_table_mut(), "features", false)?;
    table
        .remove(name)
        .ok_or(ManifestEditError::InvalidManifest)?;
    Ok(true)
}

fn profile_name(profile: BuiltinProfile) -> &'static str {
    match profile {
        BuiltinProfile::Dev => "dev",
        BuiltinProfile::Release => "release",
        BuiltinProfile::Test => "test",
        BuiltinProfile::Bench => "bench",
    }
}

fn profile_setting_key(setting: ProfileSettingEdit) -> ProfileSettingKey {
    match setting {
        ProfileSettingEdit::OptLevel(_) => ProfileSettingKey::OptLevel,
        ProfileSettingEdit::Debug(_) => ProfileSettingKey::Debug,
        ProfileSettingEdit::Strip(_) => ProfileSettingKey::Strip,
        ProfileSettingEdit::DebugAssertions(_) => ProfileSettingKey::DebugAssertions,
        ProfileSettingEdit::OverflowChecks(_) => ProfileSettingKey::OverflowChecks,
        ProfileSettingEdit::Lto(_) => ProfileSettingKey::Lto,
        ProfileSettingEdit::Panic(_) => ProfileSettingKey::Panic,
        ProfileSettingEdit::Incremental(_) => ProfileSettingKey::Incremental,
        ProfileSettingEdit::CodegenUnits(_) => ProfileSettingKey::CodegenUnits,
    }
}

fn profile_key_name(setting: ProfileSettingKey) -> &'static str {
    match setting {
        ProfileSettingKey::OptLevel => "opt-level",
        ProfileSettingKey::Debug => "debug",
        ProfileSettingKey::Strip => "strip",
        ProfileSettingKey::DebugAssertions => "debug-assertions",
        ProfileSettingKey::OverflowChecks => "overflow-checks",
        ProfileSettingKey::Lto => "lto",
        ProfileSettingKey::Panic => "panic",
        ProfileSettingKey::Incremental => "incremental",
        ProfileSettingKey::CodegenUnits => "codegen-units",
    }
}

fn profile_table(
    document: &DocumentMut,
    profile: BuiltinProfile,
) -> Result<Option<&Table>, ManifestEditError> {
    let Some(profiles) = child_table(document.as_table(), "profile")? else {
        return Ok(None);
    };
    child_table(profiles, profile_name(profile))
}

fn profile_table_mut(
    document: &mut DocumentMut,
    profile: BuiltinProfile,
) -> Result<&mut Table, ManifestEditError> {
    let profiles = child_table_mut(document.as_table_mut(), "profile", true)?;
    child_table_mut(profiles, profile_name(profile), false)
}

fn profile_value(setting: ProfileSettingEdit) -> Value {
    match setting {
        ProfileSettingEdit::OptLevel(level) => match level {
            ProfileOptLevel::Zero => Value::from(0),
            ProfileOptLevel::One => Value::from(1),
            ProfileOptLevel::Two => Value::from(2),
            ProfileOptLevel::Three => Value::from(3),
            ProfileOptLevel::Size => Value::from("s"),
            ProfileOptLevel::SizeMin => Value::from("z"),
        },
        ProfileSettingEdit::Debug(debug) => Value::from(match debug {
            ProfileDebugInfo::None => "none",
            ProfileDebugInfo::Limited => "limited",
            ProfileDebugInfo::Full => "full",
            ProfileDebugInfo::LineTablesOnly => "line-tables-only",
            ProfileDebugInfo::LineDirectivesOnly => "line-directives-only",
        }),
        ProfileSettingEdit::Strip(strip) => Value::from(match strip {
            ProfileStrip::None => "none",
            ProfileStrip::Debuginfo => "debuginfo",
            ProfileStrip::Symbols => "symbols",
        }),
        ProfileSettingEdit::DebugAssertions(value)
        | ProfileSettingEdit::OverflowChecks(value)
        | ProfileSettingEdit::Incremental(value) => Value::from(value),
        ProfileSettingEdit::Lto(lto) => match lto {
            ProfileLto::False => Value::from(false),
            ProfileLto::True => Value::from(true),
            ProfileLto::Off => Value::from("off"),
            ProfileLto::Thin => Value::from("thin"),
            ProfileLto::Fat => Value::from("fat"),
        },
        ProfileSettingEdit::Panic(panic) => Value::from(match panic {
            ProfilePanic::Unwind => "unwind",
            ProfilePanic::Abort => "abort",
        }),
        ProfileSettingEdit::CodegenUnits(value) => Value::from(i64::from(value.get())),
    }
}

fn profile_value_matches(value: &Value, setting: ProfileSettingEdit) -> bool {
    match setting {
        ProfileSettingEdit::OptLevel(level) => match level {
            ProfileOptLevel::Zero => value.as_integer() == Some(0),
            ProfileOptLevel::One => value.as_integer() == Some(1),
            ProfileOptLevel::Two => value.as_integer() == Some(2),
            ProfileOptLevel::Three => value.as_integer() == Some(3),
            ProfileOptLevel::Size => value.as_str() == Some("s"),
            ProfileOptLevel::SizeMin => value.as_str() == Some("z"),
        },
        ProfileSettingEdit::Debug(debug) => match debug {
            ProfileDebugInfo::None => {
                value.as_bool() == Some(false)
                    || value.as_integer() == Some(0)
                    || value.as_str() == Some("none")
            }
            ProfileDebugInfo::Limited => {
                value.as_integer() == Some(1) || value.as_str() == Some("limited")
            }
            ProfileDebugInfo::Full => {
                value.as_bool() == Some(true)
                    || value.as_integer() == Some(2)
                    || value.as_str() == Some("full")
            }
            ProfileDebugInfo::LineTablesOnly => value.as_str() == Some("line-tables-only"),
            ProfileDebugInfo::LineDirectivesOnly => value.as_str() == Some("line-directives-only"),
        },
        ProfileSettingEdit::Strip(strip) => match strip {
            ProfileStrip::None => value.as_bool() == Some(false) || value.as_str() == Some("none"),
            ProfileStrip::Debuginfo => value.as_str() == Some("debuginfo"),
            ProfileStrip::Symbols => {
                value.as_bool() == Some(true) || value.as_str() == Some("symbols")
            }
        },
        ProfileSettingEdit::DebugAssertions(expected)
        | ProfileSettingEdit::OverflowChecks(expected)
        | ProfileSettingEdit::Incremental(expected) => value.as_bool() == Some(expected),
        ProfileSettingEdit::Lto(lto) => match lto {
            ProfileLto::False => value.as_bool() == Some(false),
            ProfileLto::True => value.as_bool() == Some(true),
            ProfileLto::Off => value.as_str() == Some("off"),
            ProfileLto::Thin => value.as_str() == Some("thin"),
            ProfileLto::Fat => value.as_str() == Some("fat"),
        },
        ProfileSettingEdit::Panic(panic) => {
            value.as_str()
                == Some(match panic {
                    ProfilePanic::Unwind => "unwind",
                    ProfilePanic::Abort => "abort",
                })
        }
        ProfileSettingEdit::CodegenUnits(expected) => {
            value.as_integer() == Some(i64::from(expected.get()))
        }
    }
}

fn set_profile(
    document: &mut DocumentMut,
    profile: BuiltinProfile,
    setting: ProfileSettingEdit,
) -> Result<bool, ManifestEditError> {
    let key = profile_key_name(profile_setting_key(setting));
    if let Some(existing) = profile_table(document, profile)?.and_then(|table| table.get(key)) {
        let existing = existing
            .as_value()
            .ok_or(ManifestEditError::UnsupportedLayout)?;
        if profile_value_matches(existing, setting) {
            return Ok(false);
        }
    }
    replace_value(
        profile_table_mut(document, profile)?,
        key,
        profile_value(setting),
    )?;
    Ok(true)
}

fn remove_profile(
    document: &mut DocumentMut,
    profile: BuiltinProfile,
    setting: ProfileSettingKey,
) -> Result<bool, ManifestEditError> {
    let key = profile_key_name(setting);
    let Some(existing) = profile_table(document, profile)?.and_then(|table| table.get(key)) else {
        return Ok(false);
    };
    if existing.as_value().is_none() {
        return Err(ManifestEditError::UnsupportedLayout);
    }
    profile_table_mut(document, profile)?
        .remove(key)
        .ok_or(ManifestEditError::InvalidManifest)?;
    Ok(true)
}

fn validate_dependency_spec(spec: &DependencySpec) -> Result<(), ManifestEditError> {
    if spec.requirement.is_empty()
        || spec.requirement.len() > 128
        || !spec
            .requirement
            .bytes()
            .all(|byte| (b' '..=b'~').contains(&byte))
        || semver::VersionReq::parse(&spec.requirement).is_err()
        || spec.features.len() > 128
    {
        return Err(ManifestEditError::InvalidOperation);
    }
    let mut names = std::collections::BTreeSet::new();
    if spec
        .features
        .iter()
        .any(|name| !names.insert(name.as_str()))
    {
        return Err(ManifestEditError::InvalidOperation);
    }
    Ok(())
}

fn version_requirements_equal(left: &str, right: &str) -> bool {
    if left.len() > 128 || !left.bytes().all(|byte| (b' '..=b'~').contains(&byte)) {
        return false;
    }
    match (
        semver::VersionReq::parse(left),
        semver::VersionReq::parse(right),
    ) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn feature_names_equal(existing: &Array, expected: &DependencySpec) -> bool {
    let mut existing = existing
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>();
    let mut expected = expected
        .features
        .iter()
        .map(|name| name.as_str())
        .collect::<Vec<_>>();
    let Some(existing) = existing.as_mut() else {
        return false;
    };
    existing.sort_unstable();
    expected.sort_unstable();
    *existing == expected
}

fn dependency_matches(value: &Value, expected: &DependencySpec) -> bool {
    if let Some(requirement) = value.as_str() {
        return expected.package.is_none()
            && expected.features.is_empty()
            && !expected.optional
            && expected.default_features
            && version_requirements_equal(requirement, &expected.requirement);
    }
    let Some(table) = value.as_inline_table() else {
        return false;
    };
    if table.iter().any(|(key, _)| {
        !matches!(
            key,
            "version" | "package" | "features" | "optional" | "default-features"
        )
    }) {
        return false;
    }
    let Some(requirement) = table.get("version").and_then(Value::as_str) else {
        return false;
    };
    let package = match table.get("package") {
        None => None,
        Some(value) => match value.as_str() {
            Some(package) => Some(package),
            None => return false,
        },
    };
    let features_match = match table.get("features") {
        None => expected.features.is_empty(),
        Some(value) => value
            .as_array()
            .is_some_and(|array| feature_names_equal(array, expected)),
    };
    version_requirements_equal(requirement, &expected.requirement)
        && package == expected.package.as_ref().map(|name| name.as_str())
        && features_match
        && table.get("optional").map_or(!expected.optional, |value| {
            value.as_bool() == Some(expected.optional)
        })
        && table
            .get("default-features")
            .map_or(expected.default_features, |value| {
                value.as_bool() == Some(expected.default_features)
            })
}

fn dependency_value(spec: &DependencySpec) -> Value {
    if spec.package.is_none() && spec.features.is_empty() && !spec.optional && spec.default_features
    {
        return Value::from(spec.requirement.as_str());
    }
    let mut detail = InlineTable::new();
    detail.insert("version", Value::from(spec.requirement.as_str()));
    if let Some(package) = &spec.package {
        detail.insert("package", Value::from(package.as_str()));
    }
    if !spec.features.is_empty() {
        let mut features = Array::new();
        for feature in &spec.features {
            features.push(feature.as_str());
        }
        detail.insert("features", Value::Array(features));
    }
    if spec.optional {
        detail.insert("optional", Value::from(true));
    }
    if !spec.default_features {
        detail.insert("default-features", Value::from(false));
    }
    detail.fmt();
    Value::InlineTable(detail)
}

fn workspace_dependencies(document: &DocumentMut) -> Result<Option<&Table>, ManifestEditError> {
    let workspace = document
        .as_table()
        .get("workspace")
        .ok_or(ManifestEditError::InvalidOperation)
        .and_then(standard_table)?;
    child_table(workspace, "dependencies")
}

fn workspace_dependencies_mut(document: &mut DocumentMut) -> Result<&mut Table, ManifestEditError> {
    let workspace = document
        .as_table_mut()
        .get_mut("workspace")
        .ok_or(ManifestEditError::InvalidOperation)
        .and_then(standard_table_mut)?;
    child_table_mut(workspace, "dependencies", false)
}

fn set_workspace_dependency(
    document: &mut DocumentMut,
    name: &str,
    spec: &DependencySpec,
) -> Result<bool, ManifestEditError> {
    validate_dependency_spec(spec)?;
    if spec.optional {
        return Err(ManifestEditError::InvalidOperation);
    }
    if let Some(existing) = workspace_dependencies(document)?.and_then(|table| table.get(name)) {
        let existing = existing
            .as_value()
            .ok_or(ManifestEditError::UnsupportedLayout)?;
        if dependency_matches(existing, spec) {
            return Ok(false);
        }
    }
    replace_value(
        workspace_dependencies_mut(document)?,
        name,
        dependency_value(spec),
    )?;
    Ok(true)
}

fn remove_workspace_dependency(
    document: &mut DocumentMut,
    name: &str,
) -> Result<bool, ManifestEditError> {
    let Some(existing) = workspace_dependencies(document)?.and_then(|table| table.get(name)) else {
        return Ok(false);
    };
    if existing.as_value().is_none() {
        return Err(ManifestEditError::UnsupportedLayout);
    }
    workspace_dependencies_mut(document)?
        .remove(name)
        .ok_or(ManifestEditError::InvalidManifest)?;
    Ok(true)
}

fn dependency_table_name(kind: DependencyKind) -> &'static str {
    match kind {
        DependencyKind::Normal => "dependencies",
        DependencyKind::Build => "build-dependencies",
        DependencyKind::Dev => "dev-dependencies",
    }
}

fn dependency_table<'a>(
    document: &'a DocumentMut,
    kind: DependencyKind,
    target: Option<&str>,
) -> Result<Option<&'a Table>, ManifestEditError> {
    package_manifest(document.as_table())?;
    let name = dependency_table_name(kind);
    let Some(target) = target else {
        return child_table(document.as_table(), name);
    };
    let Some(targets) = child_table(document.as_table(), "target")? else {
        return Ok(None);
    };
    let Some(target) = child_table(targets, target)? else {
        return Ok(None);
    };
    child_table(target, name)
}

fn dependency_table_mut<'a>(
    document: &'a mut DocumentMut,
    kind: DependencyKind,
    target: Option<&str>,
) -> Result<&'a mut Table, ManifestEditError> {
    let name = dependency_table_name(kind);
    let Some(target) = target else {
        return child_table_mut(document.as_table_mut(), name, false);
    };
    let targets = child_table_mut(document.as_table_mut(), "target", true)?;
    let target = child_table_mut(targets, target, true)?;
    child_table_mut(target, name, false)
}

fn add_dependency(
    document: &mut DocumentMut,
    kind: DependencyKind,
    target: Option<&str>,
    name: &str,
    spec: &DependencySpec,
) -> Result<bool, ManifestEditError> {
    validate_dependency_spec(spec)?;
    if let Some(existing) =
        dependency_table(document, kind, target)?.and_then(|table| table.get(name))
    {
        let existing = existing
            .as_value()
            .ok_or(ManifestEditError::UnsupportedLayout)?;
        return if dependency_matches(existing, spec) {
            Ok(false)
        } else {
            Err(ManifestEditError::Conflict)
        };
    }
    replace_value(
        dependency_table_mut(document, kind, target)?,
        name,
        dependency_value(spec),
    )?;
    Ok(true)
}

fn remove_dependency(
    document: &mut DocumentMut,
    kind: DependencyKind,
    target: Option<&str>,
    name: &str,
) -> Result<bool, ManifestEditError> {
    let Some(existing) =
        dependency_table(document, kind, target)?.and_then(|table| table.get(name))
    else {
        return Ok(false);
    };
    // Remove the complete entry; do not normalize or edit its nested layout.
    // ADR-057 permits a table/dotted dependency only for this whole-key removal.
    if existing.as_value().is_none() && existing.as_table().is_none() {
        return Err(ManifestEditError::UnsupportedLayout);
    }
    dependency_table_mut(document, kind, target)?
        .remove(name)
        .ok_or(ManifestEditError::InvalidManifest)?;
    Ok(true)
}
