//! Closed semantic edits; parsing and serialization remain in the adapter.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintScope {
    Package,
    Workspace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintTool {
    Rust,
    Clippy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LintLevel {
    Allow,
    Warn,
    Deny,
    Forbid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintName(String);

impl LintName {
    pub fn new(value: String) -> Result<Self, ManifestEditError> {
        if value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_')
        {
            return Err(ManifestEditError::InvalidOperation);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestEdit {
    LintSet {
        scope: LintScope,
        tool: LintTool,
        name: LintName,
        level: LintLevel,
        priority: Option<i64>,
    },
    LintRemove {
        scope: LintScope,
        tool: LintTool,
        name: LintName,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestEditError {
    InvalidOperation,
    InvalidManifest,
    UnsupportedLayout,
    InheritedLints,
    LimitExceeded,
}
