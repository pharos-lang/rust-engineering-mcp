use rust_engineering_application::ManifestEditor;
use rust_engineering_domain::{LintLevel, LintScope, LintTool, ManifestEdit, ManifestEditError};
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value};

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
