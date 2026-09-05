//! Closed MCP DTOs for typed manifest and dependency mutations.

use super::{Action, Input, MutationInput};
use rmcp::model::ErrorData;
use rust_engineering_domain::{
    BuiltinProfile, DependencyKind, DependencyName, DependencySpec, DependencyTarget, FeatureName,
    FeatureValue, LintLevel, LintName, LintScope, LintTool, ManifestEdit, MutationError,
    MutationKind, ProfileCodegenUnits, ProfileDebugInfo, ProfileLto, ProfileOptLevel, ProfilePanic,
    ProfileSettingEdit, ProfileSettingKey, ProfileStrip, ProjectIdentityFingerprint, ProjectRef,
    SourceFingerprint, validate_source_path,
};
use schemars::JsonSchema;
use serde::Deserialize;

pub(super) const DEPENDENCY_ADD_NAME: &str = "rust.dependency.add";
pub(super) const DEPENDENCY_REMOVE_NAME: &str = "rust.dependency.remove";

#[derive(Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct FeatureNameInput(
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_.+-]*$")
    )]
    String,
);

#[derive(Deserialize, JsonSchema)]
#[serde(transparent)]
pub(super) struct FeatureValueInput(
    #[schemars(length(min = 1, max = 128), regex(pattern = "^[ -~]+$"))] String,
);

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Scope {
    Package,
    Workspace,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Namespace {
    Rust,
    Clippy,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Level {
    Allow,
    Warn,
    Deny,
    Forbid,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(super) enum Profile {
    Dev,
    Release,
    Test,
    Bench,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(super) enum ProfileSettingKeyInput {
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

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum OptLevelInput {
    Integer(#[schemars(range(min = 0, max = 3))] u8),
    Text(SizeOptLevelInput),
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum SizeOptLevelInput {
    S,
    Z,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(super) enum DebugInfoInput {
    None,
    Limited,
    Full,
    LineTablesOnly,
    LineDirectivesOnly,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum StripInput {
    None,
    Debuginfo,
    Symbols,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum LtoTextInput {
    Off,
    Thin,
    Fat,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(super) enum LtoInput {
    Boolean(bool),
    Text(LtoTextInput),
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub(super) enum PanicInput {
    Unwind,
    Abort,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(
    tag = "name",
    content = "value",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub(super) enum ProfileSettingInput {
    OptLevel(OptLevelInput),
    Debug(DebugInfoInput),
    Strip(StripInput),
    DebugAssertions(bool),
    OverflowChecks(bool),
    Lto(LtoInput),
    Panic(PanicInput),
    Incremental(bool),
    CodegenUnits(#[schemars(range(min = 1))] u32),
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DependencySpecInput {
    #[schemars(length(min = 1, max = 128), regex(pattern = "^[ -~]+$"))]
    requirement: String,
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
    )]
    package: Option<String>,
    #[serde(default)]
    #[schemars(length(max = 128))]
    features: Vec<FeatureNameInput>,
    #[serde(default)]
    optional: bool,
    #[serde(default = "default_true")]
    default_features: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Edit {
    LintSet {
        scope: Scope,
        tool: Namespace,
        #[schemars(length(min = 1, max = 128), regex(pattern = "^[A-Za-z0-9_]+$"))]
        name: String,
        level: Level,
        priority: Option<i64>,
    },
    LintRemove {
        scope: Scope,
        tool: Namespace,
        #[schemars(length(min = 1, max = 128), regex(pattern = "^[A-Za-z0-9_]+$"))]
        name: String,
    },
    FeatureSet {
        #[schemars(
            length(min = 1, max = 128),
            regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_.+-]*$")
        )]
        name: String,
        #[schemars(length(max = 128))]
        values: Vec<FeatureValueInput>,
    },
    FeatureRemove {
        #[schemars(
            length(min = 1, max = 128),
            regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_.+-]*$")
        )]
        name: String,
    },
    ProfileSet {
        profile: Profile,
        setting: ProfileSettingInput,
    },
    ProfileRemove {
        profile: Profile,
        setting: ProfileSettingKeyInput,
    },
    WorkspaceDependencySet {
        #[schemars(
            length(min = 1, max = 128),
            regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
        )]
        name: String,
        spec: DependencySpecInput,
    },
    WorkspaceDependencyRemove {
        #[schemars(
            length(min = 1, max = 128),
            regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
        )]
        name: String,
    },
}

impl Edit {
    pub(super) fn into_domain(self) -> Result<ManifestEdit, MutationError> {
        let scope = |value| match value {
            Scope::Package => LintScope::Package,
            Scope::Workspace => LintScope::Workspace,
        };
        let tool = |value| match value {
            Namespace::Rust => LintTool::Rust,
            Namespace::Clippy => LintTool::Clippy,
        };
        let profile = |value| match value {
            Profile::Dev => BuiltinProfile::Dev,
            Profile::Release => BuiltinProfile::Release,
            Profile::Test => BuiltinProfile::Test,
            Profile::Bench => BuiltinProfile::Bench,
        };
        Ok(match self {
            Self::LintSet {
                scope: selected_scope,
                tool: selected_tool,
                name,
                level,
                priority,
            } => ManifestEdit::LintSet {
                scope: scope(selected_scope),
                tool: tool(selected_tool),
                name: lint_name(name)?,
                priority,
                level: match level {
                    Level::Allow => LintLevel::Allow,
                    Level::Warn => LintLevel::Warn,
                    Level::Deny => LintLevel::Deny,
                    Level::Forbid => LintLevel::Forbid,
                },
            },
            Self::LintRemove {
                scope: selected_scope,
                tool: selected_tool,
                name,
            } => ManifestEdit::LintRemove {
                scope: scope(selected_scope),
                tool: tool(selected_tool),
                name: lint_name(name)?,
            },
            Self::FeatureSet { name, values } => ManifestEdit::FeatureSet {
                name: feature_name(name)?,
                values: values
                    .into_iter()
                    .map(|value| FeatureValue::new(value.0).map_err(|_| MutationError::Invalid))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            Self::FeatureRemove { name } => ManifestEdit::FeatureRemove {
                name: feature_name(name)?,
            },
            Self::ProfileSet {
                profile: selected,
                setting,
            } => ManifestEdit::ProfileSet {
                profile: profile(selected),
                setting: setting.into_domain()?,
            },
            Self::ProfileRemove {
                profile: selected,
                setting,
            } => ManifestEdit::ProfileRemove {
                profile: profile(selected),
                setting: setting.into_domain(),
            },
            Self::WorkspaceDependencySet { name, spec } => {
                if spec.optional {
                    return Err(MutationError::Invalid);
                }
                ManifestEdit::WorkspaceDependencySet {
                    name: dependency_name(name)?,
                    spec: spec.into_domain()?,
                }
            }
            Self::WorkspaceDependencyRemove { name } => ManifestEdit::WorkspaceDependencyRemove {
                name: dependency_name(name)?,
            },
        })
    }
}

impl ProfileSettingInput {
    fn into_domain(self) -> Result<ProfileSettingEdit, MutationError> {
        Ok(match self {
            Self::OptLevel(value) => ProfileSettingEdit::OptLevel(match value {
                OptLevelInput::Integer(0) => ProfileOptLevel::Zero,
                OptLevelInput::Integer(1) => ProfileOptLevel::One,
                OptLevelInput::Integer(2) => ProfileOptLevel::Two,
                OptLevelInput::Integer(3) => ProfileOptLevel::Three,
                OptLevelInput::Integer(_) => return Err(MutationError::Invalid),
                OptLevelInput::Text(SizeOptLevelInput::S) => ProfileOptLevel::Size,
                OptLevelInput::Text(SizeOptLevelInput::Z) => ProfileOptLevel::SizeMin,
            }),
            Self::Debug(value) => ProfileSettingEdit::Debug(match value {
                DebugInfoInput::None => ProfileDebugInfo::None,
                DebugInfoInput::Limited => ProfileDebugInfo::Limited,
                DebugInfoInput::Full => ProfileDebugInfo::Full,
                DebugInfoInput::LineTablesOnly => ProfileDebugInfo::LineTablesOnly,
                DebugInfoInput::LineDirectivesOnly => ProfileDebugInfo::LineDirectivesOnly,
            }),
            Self::Strip(value) => ProfileSettingEdit::Strip(match value {
                StripInput::None => ProfileStrip::None,
                StripInput::Debuginfo => ProfileStrip::Debuginfo,
                StripInput::Symbols => ProfileStrip::Symbols,
            }),
            Self::DebugAssertions(value) => ProfileSettingEdit::DebugAssertions(value),
            Self::OverflowChecks(value) => ProfileSettingEdit::OverflowChecks(value),
            Self::Lto(value) => ProfileSettingEdit::Lto(match value {
                LtoInput::Boolean(false) => ProfileLto::False,
                LtoInput::Boolean(true) => ProfileLto::True,
                LtoInput::Text(LtoTextInput::Off) => ProfileLto::Off,
                LtoInput::Text(LtoTextInput::Thin) => ProfileLto::Thin,
                LtoInput::Text(LtoTextInput::Fat) => ProfileLto::Fat,
            }),
            Self::Panic(value) => ProfileSettingEdit::Panic(match value {
                PanicInput::Unwind => ProfilePanic::Unwind,
                PanicInput::Abort => ProfilePanic::Abort,
            }),
            Self::Incremental(value) => ProfileSettingEdit::Incremental(value),
            Self::CodegenUnits(value) => ProfileSettingEdit::CodegenUnits(
                ProfileCodegenUnits::new(value).map_err(|_| MutationError::Invalid)?,
            ),
        })
    }
}

impl ProfileSettingKeyInput {
    fn into_domain(self) -> ProfileSettingKey {
        match self {
            Self::OptLevel => ProfileSettingKey::OptLevel,
            Self::Debug => ProfileSettingKey::Debug,
            Self::Strip => ProfileSettingKey::Strip,
            Self::DebugAssertions => ProfileSettingKey::DebugAssertions,
            Self::OverflowChecks => ProfileSettingKey::OverflowChecks,
            Self::Lto => ProfileSettingKey::Lto,
            Self::Panic => ProfileSettingKey::Panic,
            Self::Incremental => ProfileSettingKey::Incremental,
            Self::CodegenUnits => ProfileSettingKey::CodegenUnits,
        }
    }
}

impl DependencySpecInput {
    fn into_domain(self) -> Result<DependencySpec, MutationError> {
        if !bounded_ascii(&self.requirement, 128) || self.features.len() > 128 {
            return Err(MutationError::Invalid);
        }
        Ok(DependencySpec {
            requirement: self.requirement,
            package: self.package.map(dependency_name).transpose()?,
            features: self
                .features
                .into_iter()
                .map(|feature| feature_name(feature.0))
                .collect::<Result<Vec<_>, _>>()?,
            optional: self.optional,
            default_features: self.default_features,
        })
    }
}

fn lint_name(value: String) -> Result<LintName, MutationError> {
    LintName::new(value).map_err(|_| MutationError::Invalid)
}

fn feature_name(value: String) -> Result<FeatureName, MutationError> {
    FeatureName::new(value).map_err(|_| MutationError::Invalid)
}

fn dependency_name(value: String) -> Result<DependencyName, MutationError> {
    DependencyName::new(value).map_err(|_| MutationError::Invalid)
}

fn bounded_ascii(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value.bytes().all(|byte| (b' '..=b'~').contains(&byte))
}

#[derive(Clone, Copy, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub(super) enum DependencyKindInput {
    #[default]
    Normal,
    Dev,
    Build,
}

impl DependencyKindInput {
    fn into_domain(self) -> DependencyKind {
        match self {
            Self::Normal => DependencyKind::Normal,
            Self::Dev => DependencyKind::Dev,
            Self::Build => DependencyKind::Build,
        }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::stdio) struct DependencyAddInput {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    action: DependencyAddAction,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum DependencyAddAction {
    Preview {
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        expected_project_fingerprint: ProjectIdentityFingerprint,
        #[serde(default = "default_manifest_path")]
        #[schemars(
            length(min = 10, max = 100),
            regex(
                pattern = "^(([A-Za-z0-9_.-]*[A-Za-z0-9_-][A-Za-z0-9_.-]*|\\.{3,})/){0,31}Cargo\\.toml$"
            )
        )]
        manifest_path: String,
        #[serde(default)]
        dependency_kind: DependencyKindInput,
        #[schemars(length(min = 1, max = 256), regex(pattern = "^[ -~]+$"))]
        target: Option<String>,
        #[schemars(
            length(min = 1, max = 128),
            regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
        )]
        name: String,
        #[schemars(length(min = 1, max = 128), regex(pattern = "^[ -~]+$"))]
        requirement: String,
        #[schemars(
            length(min = 1, max = 128),
            regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
        )]
        package: Option<String>,
        #[serde(default)]
        #[schemars(length(max = 128))]
        features: Vec<FeatureNameInput>,
        #[serde(default)]
        optional: bool,
        #[serde(default = "default_true")]
        default_features: bool,
    },
    Commit {
        #[schemars(regex(pattern = "^mut_[0-9a-f]{32}$"))]
        plan_id: String,
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        plan_digest: SourceFingerprint,
        #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]+$"))]
        idempotency_key: String,
    },
    Receipt {
        #[schemars(regex(pattern = "^mut_[0-9a-f]{32}$"))]
        operation_id: String,
        recover: bool,
    },
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(in crate::stdio) struct DependencyRemoveInput {
    #[schemars(with = "String", regex(pattern = "^prj_[0-9a-f]{32}$"))]
    project_ref: ProjectRef,
    action: DependencyRemoveAction,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum DependencyRemoveAction {
    Preview {
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        expected_project_fingerprint: ProjectIdentityFingerprint,
        #[serde(default = "default_manifest_path")]
        #[schemars(
            length(min = 10, max = 100),
            regex(
                pattern = "^(([A-Za-z0-9_.-]*[A-Za-z0-9_-][A-Za-z0-9_.-]*|\\.{3,})/){0,31}Cargo\\.toml$"
            )
        )]
        manifest_path: String,
        #[serde(default)]
        dependency_kind: DependencyKindInput,
        #[schemars(length(min = 1, max = 256), regex(pattern = "^[ -~]+$"))]
        target: Option<String>,
        #[schemars(
            length(min = 1, max = 128),
            regex(pattern = "^[A-Za-z0-9_][A-Za-z0-9_-]*$")
        )]
        name: String,
    },
    Commit {
        #[schemars(regex(pattern = "^mut_[0-9a-f]{32}$"))]
        plan_id: String,
        #[schemars(with = "String", regex(pattern = "^sha256:[0-9a-f]{64}$"))]
        plan_digest: SourceFingerprint,
        #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]+$"))]
        idempotency_key: String,
    },
    Receipt {
        #[schemars(regex(pattern = "^mut_[0-9a-f]{32}$"))]
        operation_id: String,
        recover: bool,
    },
}

fn default_manifest_path() -> String {
    "Cargo.toml".to_owned()
}

fn manifest_path(value: String) -> Result<String, ErrorData> {
    if validate_source_path(&value).is_err()
        || (value != "Cargo.toml" && !value.ends_with("/Cargo.toml"))
    {
        return Err(invalid_arguments());
    }
    Ok(value)
}

fn dependency_target(value: Option<String>) -> Result<Option<DependencyTarget>, ErrorData> {
    value
        .map(|target| DependencyTarget::new(target).map_err(|_| invalid_arguments()))
        .transpose()
}

fn dependency_spec(
    requirement: String,
    package: Option<String>,
    features: Vec<FeatureNameInput>,
    optional: bool,
    default_features: bool,
) -> Result<DependencySpec, ErrorData> {
    DependencySpecInput {
        requirement,
        package,
        features,
        optional,
        default_features,
    }
    .into_domain()
    .map_err(|_| invalid_arguments())
}

fn invalid_arguments() -> ErrorData {
    ErrorData::invalid_params("Invalid tool arguments", None)
}

impl MutationInput for DependencyAddInput {
    const NAME: &'static str = DEPENDENCY_ADD_NAME;
    const DESCRIPTION: &'static str = "Preview, commit or inspect a journaled dependency addition. Host --allow-dependency-add is required. Preview also requires approved offline Cargo vendor data; commit and receipt use the approved plan/journal without reloading the dataset. New effects require an unexpired approved plan. Exact ID/digest/key can replay an existing journal under current authority. Commit invalidates its input project_ref: call rust.project.open and use its newly returned data.project_ref for ALL later calls, including receipt/recovery; never reuse the precommit reference.";
    const KIND: MutationKind = MutationKind::DependencyAdd;

    fn into_request(self) -> Result<Input, ErrorData> {
        let action = match self.action {
            DependencyAddAction::Preview {
                expected_project_fingerprint,
                manifest_path: selected_manifest,
                dependency_kind,
                target,
                name,
                requirement,
                package,
                features,
                optional,
                default_features,
            } => Action::SemanticPreview {
                expected_project_fingerprint,
                target_manifest: manifest_path(selected_manifest)?,
                edit: ManifestEdit::DependencyAdd {
                    kind: dependency_kind.into_domain(),
                    target: dependency_target(target)?,
                    name: dependency_name(name).map_err(|_| invalid_arguments())?,
                    spec: dependency_spec(
                        requirement,
                        package,
                        features,
                        optional,
                        default_features,
                    )?,
                },
            },
            DependencyAddAction::Commit {
                plan_id,
                plan_digest,
                idempotency_key,
            } => Action::Commit {
                plan_id,
                plan_digest,
                idempotency_key,
            },
            DependencyAddAction::Receipt {
                operation_id,
                recover,
            } => Action::Receipt {
                operation_id,
                recover,
            },
        };
        Ok(Input {
            project_ref: self.project_ref,
            action,
        })
    }
}

impl MutationInput for DependencyRemoveInput {
    const NAME: &'static str = DEPENDENCY_REMOVE_NAME;
    const DESCRIPTION: &'static str = "Preview, commit or inspect a journaled dependency removal. Host --allow-dependency-remove is required. Preview also requires approved offline Cargo vendor data; commit and receipt use the approved plan/journal without reloading the dataset. New effects require an unexpired approved plan. Exact ID/digest/key can replay an existing journal under current authority. Commit invalidates its input project_ref: call rust.project.open and use its newly returned data.project_ref for ALL later calls, including receipt/recovery; never reuse the precommit reference.";
    const KIND: MutationKind = MutationKind::DependencyRemove;

    fn into_request(self) -> Result<Input, ErrorData> {
        let action = match self.action {
            DependencyRemoveAction::Preview {
                expected_project_fingerprint,
                manifest_path: selected_manifest,
                dependency_kind,
                target,
                name,
            } => Action::SemanticPreview {
                expected_project_fingerprint,
                target_manifest: manifest_path(selected_manifest)?,
                edit: ManifestEdit::DependencyRemove {
                    kind: dependency_kind.into_domain(),
                    target: dependency_target(target)?,
                    name: dependency_name(name).map_err(|_| invalid_arguments())?,
                },
            },
            DependencyRemoveAction::Commit {
                plan_id,
                plan_digest,
                idempotency_key,
            } => Action::Commit {
                plan_id,
                plan_digest,
                idempotency_key,
            },
            DependencyRemoveAction::Receipt {
                operation_id,
                recover,
            } => Action::Receipt {
                operation_id,
                recover,
            },
        };
        Ok(Input {
            project_ref: self.project_ref,
            action,
        })
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Fixed DTO fixtures; production conversion remains fallible.
mod tests {
    use super::*;
    use serde_json::json;

    fn decode_edit(value: serde_json::Value) -> Result<ManifestEdit, MutationError> {
        serde_json::from_value::<Edit>(value)
            .map_err(|_| MutationError::Invalid)?
            .into_domain()
    }

    #[test]
    fn lint_json_shape_and_mapping_remain_unchanged() {
        let edit = decode_edit(json!({
            "operation": "lint_set",
            "scope": "workspace",
            "tool": "clippy",
            "name": "unwrap_used",
            "level": "deny",
            "priority": 2
        }))
        .expect("lint edit");
        assert!(matches!(
            edit,
            ManifestEdit::LintSet {
                scope: LintScope::Workspace,
                tool: LintTool::Clippy,
                level: LintLevel::Deny,
                priority: Some(2),
                ..
            }
        ));
    }

    #[test]
    fn feature_values_use_domain_validation() {
        let valid = decode_edit(json!({
            "operation": "feature_set",
            "name": "full",
            "values": ["dep:serde", "serde/derive", "tokio?/rt"]
        }));
        assert!(matches!(valid, Ok(ManifestEdit::FeatureSet { .. })));
        assert_eq!(
            decode_edit(json!({
                "operation": "feature_set",
                "name": "full",
                "values": ["dep:../escape"]
            })),
            Err(MutationError::Invalid)
        );
    }

    #[test]
    fn profile_values_are_typed_and_bounded() {
        for setting in [
            json!({"name":"opt-level", "value":3}),
            json!({"name":"opt-level", "value":"z"}),
            json!({"name":"debug", "value":"line-tables-only"}),
            json!({"name":"strip", "value":"symbols"}),
            json!({"name":"lto", "value":true}),
            json!({"name":"lto", "value":"thin"}),
            json!({"name":"panic", "value":"abort"}),
            json!({"name":"incremental", "value":false}),
            json!({"name":"codegen-units", "value":1}),
        ] {
            assert!(
                decode_edit(json!({
                    "operation": "profile_set",
                    "profile": "release",
                    "setting": setting
                }))
                .is_ok()
            );
        }
        assert!(
            serde_json::from_value::<Edit>(json!({
                "operation": "profile_set",
                "profile": "release",
                "setting": {"name":"opt-level", "value":4}
            }))
            .is_ok()
        );
        assert_eq!(
            decode_edit(json!({
                "operation": "profile_set",
                "profile": "release",
                "setting": {"name":"opt-level", "value":4}
            })),
            Err(MutationError::Invalid)
        );
        assert_eq!(
            decode_edit(json!({
                "operation": "profile_set",
                "profile": "release",
                "setting": {"name":"codegen-units", "value":0}
            })),
            Err(MutationError::Invalid)
        );
    }

    #[test]
    fn workspace_dependencies_forbid_optional_and_unknown_inheritance() {
        assert_eq!(
            decode_edit(json!({
                "operation": "workspace_dependency_set",
                "name": "serde",
                "spec": {"requirement":"1", "optional":true}
            })),
            Err(MutationError::Invalid)
        );
        assert!(
            serde_json::from_value::<Edit>(json!({
                "operation": "workspace_dependency_set",
                "name": "serde",
                "spec": {"requirement":"1", "workspace":true}
            }))
            .is_err()
        );
    }

    #[test]
    fn add_preview_defaults_and_maps_direct_fields() {
        let input: DependencyAddInput = serde_json::from_value(json!({
            "project_ref":"prj_00000000000000000000000000000001",
            "action": {
                "mode":"preview",
                "expected_project_fingerprint":"sha256:0000000000000000000000000000000000000000000000000000000000000001",
                "name":"serde",
                "requirement":"1",
                "features":["derive"]
            }
        }))
        .expect("add DTO");
        let request = input.into_request().expect("domain request");
        assert!(matches!(
            request.action,
            Action::SemanticPreview {
                target_manifest,
                edit: ManifestEdit::DependencyAdd {
                    kind: DependencyKind::Normal,
                    target: None,
                    spec: DependencySpec {
                        optional: false,
                        default_features: true,
                        ..
                    },
                    ..
                },
                ..
            } if target_manifest == "Cargo.toml"
        ));
    }

    #[test]
    fn dependency_preview_rejects_bad_paths_targets_and_unknown_fields() {
        let base = json!({
            "project_ref":"prj_00000000000000000000000000000001",
            "action": {
                "mode":"preview",
                "expected_project_fingerprint":"sha256:0000000000000000000000000000000000000000000000000000000000000001",
                "manifest_path":"../Cargo.toml",
                "target":"x86_64..unknown",
                "name":"serde",
                "requirement":"1",
                "workspace":true
            }
        });
        assert!(serde_json::from_value::<DependencyAddInput>(base).is_err());

        let input: DependencyRemoveInput = serde_json::from_value(json!({
            "project_ref":"prj_00000000000000000000000000000001",
            "action": {
                "mode":"preview",
                "expected_project_fingerprint":"sha256:0000000000000000000000000000000000000000000000000000000000000001",
                "manifest_path":"../Cargo.toml",
                "target":"x86_64..unknown",
                "name":"serde"
            }
        }))
        .expect("serde accepts values guarded by domain constructors");
        assert!(input.into_request().is_err());

        let empty_requirement: DependencyAddInput = serde_json::from_value(json!({
            "project_ref":"prj_00000000000000000000000000000001",
            "action": {
                "mode":"preview",
                "expected_project_fingerprint":"sha256:0000000000000000000000000000000000000000000000000000000000000001",
                "name":"serde",
                "requirement":""
            }
        }))
        .expect("schema constraints are also enforced during conversion");
        assert!(empty_requirement.into_request().is_err());
    }

    #[test]
    fn dependency_manifest_schema_rejects_traversal_and_matches_portable_components() {
        let schema = schemars::schema_for!(DependencyRemoveInput);
        let value = serde_json::to_value(schema).expect("schema JSON");
        let validator = jsonschema::validator_for(&value).expect("schema validator");
        for (path, accepted) in [
            ("Cargo.toml", true),
            ("a-b/Cargo.toml", true),
            (".hidden/Cargo.toml", true),
            (".../Cargo.toml", true),
            ("../Cargo.toml", false),
            ("a/./Cargo.toml", false),
            ("a/../Cargo.toml", false),
            ("/Cargo.toml", false),
            ("a//Cargo.toml", false),
        ] {
            let arguments = json!({"project_ref":"prj_00000000000000000000000000000001", "action": {
                "mode":"preview", "expected_project_fingerprint": "sha256:0000000000000000000000000000000000000000000000000000000000000001",
                "manifest_path":path, "name":"serde" }});
            assert_eq!(validator.is_valid(&arguments), accepted, "path {path}");
        }
    }

    #[test]
    fn empty_and_unknown_edit_fields_are_rejected() {
        assert_eq!(
            decode_edit(json!({
                "operation":"feature_remove",
                "name":""
            })),
            Err(MutationError::Invalid)
        );
        assert!(
            serde_json::from_value::<Edit>(json!({
                "operation":"lint_remove",
                "scope":"package",
                "tool":"rust",
                "name":"unsafe_code",
                "extra":true
            }))
            .is_err()
        );
    }

    #[test]
    fn schemas_keep_dependency_tools_closed() {
        let add =
            serde_json::to_value(schemars::schema_for!(DependencyAddInput)).expect("add schema");
        let remove = serde_json::to_value(schemars::schema_for!(DependencyRemoveInput))
            .expect("remove schema");
        let add_text = add.to_string();
        let remove_text = remove.to_string();
        assert!(add_text.contains("dependency_kind"));
        assert!(add_text.contains("default_features"));
        assert!(!remove_text.contains("requirement"));
        assert!(!remove_text.contains("features"));
    }

    #[test]
    fn dependency_lifecycle_actions_preserve_commit_and_receipt_contracts() {
        let commit: DependencyAddInput = serde_json::from_value(json!({
            "project_ref":"prj_00000000000000000000000000000001",
            "action": {
                "mode":"commit",
                "plan_id":"mut_00000000000000000000000000000001",
                "plan_digest":"sha256:0000000000000000000000000000000000000000000000000000000000000002",
                "idempotency_key":"request-1"
            }
        }))
        .expect("commit DTO");
        assert!(matches!(
            commit.into_request().expect("commit request").action,
            Action::Commit { idempotency_key, .. } if idempotency_key == "request-1"
        ));

        let receipt: DependencyRemoveInput = serde_json::from_value(json!({
            "project_ref":"prj_00000000000000000000000000000001",
            "action": {
                "mode":"receipt",
                "operation_id":"mut_00000000000000000000000000000001",
                "recover":true
            }
        }))
        .expect("receipt DTO");
        assert!(matches!(
            receipt.into_request().expect("receipt request").action,
            Action::Receipt { recover: true, .. }
        ));
    }
}
