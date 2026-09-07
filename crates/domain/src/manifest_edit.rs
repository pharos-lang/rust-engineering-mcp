//! Closed semantic edits; parsing and serialization remain in the adapter.

use crate::DependencyKind;

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

fn valid_cargo_name(value: &str, extra: &[u8]) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || extra.contains(&byte))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureName(String);

impl FeatureName {
    pub fn new(value: String) -> Result<Self, ManifestEditError> {
        if !valid_cargo_name(&value, b"-+.") {
            return Err(ManifestEditError::InvalidOperation);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyName(String);

impl DependencyName {
    pub fn new(value: String) -> Result<Self, ManifestEditError> {
        if !valid_cargo_name(&value, b"-") {
            return Err(ManifestEditError::InvalidOperation);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureValue(String);

impl FeatureValue {
    pub fn new(value: String) -> Result<Self, ManifestEditError> {
        if value.is_empty() || value.len() > 128 || !value.is_ascii() {
            return Err(ManifestEditError::InvalidOperation);
        }
        let valid = if let Some(dependency) = value.strip_prefix("dep:") {
            DependencyName::new(dependency.to_owned()).is_ok()
        } else if let Some((dependency, feature)) = value.split_once('/') {
            !feature.contains('/')
                && DependencyName::new(
                    dependency
                        .strip_suffix('?')
                        .unwrap_or(dependency)
                        .to_owned(),
                )
                .is_ok()
                && FeatureName::new(feature.to_owned()).is_ok()
        } else {
            FeatureName::new(value.clone()).is_ok()
        };
        if !valid {
            return Err(ManifestEditError::InvalidOperation);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyTarget(String);

impl DependencyTarget {
    pub fn new(value: String) -> Result<Self, ManifestEditError> {
        let printable = !value.is_empty()
            && value.len() <= 256
            && value.bytes().all(|byte| (b' '..=b'~').contains(&byte));
        let triple = value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
            && !value.contains("..");
        let cfg = value
            .strip_prefix("cfg(")
            .and_then(|inner| inner.strip_suffix(')'))
            .is_some_and(|inner| !inner.trim().is_empty());
        if !printable || !(triple || cfg) {
            return Err(ManifestEditError::InvalidOperation);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencySpec {
    pub requirement: String,
    pub package: Option<DependencyName>,
    pub features: Vec<FeatureName>,
    pub optional: bool,
    pub default_features: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinProfile {
    Dev,
    Release,
    Test,
    Bench,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileOptLevel {
    Zero,
    One,
    Two,
    Three,
    Size,
    SizeMin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileDebugInfo {
    None,
    Limited,
    Full,
    LineTablesOnly,
    LineDirectivesOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileStrip {
    None,
    Debuginfo,
    Symbols,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileLto {
    False,
    True,
    Off,
    Thin,
    Fat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfilePanic {
    Unwind,
    Abort,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileCodegenUnits(u32);

impl ProfileCodegenUnits {
    pub fn new(value: u32) -> Result<Self, ManifestEditError> {
        if value == 0 {
            return Err(ManifestEditError::InvalidOperation);
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSettingKey {
    OptLevel,
    Debug,
    Strip,
    DebugAssertions,
    OverflowChecks,
    Lto,
    Panic,
    Incremental,
    CodegenUnits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileSettingEdit {
    OptLevel(ProfileOptLevel),
    Debug(ProfileDebugInfo),
    Strip(ProfileStrip),
    DebugAssertions(bool),
    OverflowChecks(bool),
    Lto(ProfileLto),
    Panic(ProfilePanic),
    Incremental(bool),
    CodegenUnits(ProfileCodegenUnits),
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
    FeatureSet {
        name: FeatureName,
        values: Vec<FeatureValue>,
    },
    FeatureRemove {
        name: FeatureName,
    },
    ProfileSet {
        profile: BuiltinProfile,
        setting: ProfileSettingEdit,
    },
    ProfileRemove {
        profile: BuiltinProfile,
        setting: ProfileSettingKey,
    },
    WorkspaceDependencySet {
        name: DependencyName,
        spec: DependencySpec,
    },
    WorkspaceDependencyRemove {
        name: DependencyName,
    },
    DependencyAdd {
        kind: DependencyKind,
        target: Option<DependencyTarget>,
        name: DependencyName,
        spec: DependencySpec,
    },
    DependencyRemove {
        kind: DependencyKind,
        target: Option<DependencyTarget>,
        name: DependencyName,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManifestEditError {
    InvalidOperation,
    InvalidManifest,
    UnsupportedLayout,
    InheritedLints,
    Conflict,
    LimitExceeded,
}
