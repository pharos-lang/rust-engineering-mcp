//! Closed Clippy selections; source lint policy can intentionally suppress lints.
use crate::{
    ArtifactMetadata, CheckObservation, CheckOptions, CheckSelection, InspectionSemantics,
    InvalidCheckOptions, ProjectIdentityFingerprint, ProjectRef, SnapshotEvidence,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintProfile {
    #[default]
    Default,
    Strict,
    Pedantic,
    Project,
}
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ClippySelection {
    pub package: Option<String>,
    pub workspace: bool,
    pub features: Vec<String>,
    pub all_targets: bool,
    pub lint_profile: LintProfile,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ClippySelection")]
pub struct ClippyOptions {
    package: Option<String>,
    workspace: bool,
    features: Vec<String>,
    all_targets: bool,
    lint_profile: LintProfile,
}
impl TryFrom<ClippySelection> for ClippyOptions {
    type Error = InvalidCheckOptions;
    fn try_from(value: ClippySelection) -> Result<Self, Self::Error> {
        let checked = CheckOptions::try_from(CheckSelection {
            package: value.package,
            workspace: value.workspace,
            features: value.features,
            all_targets: value.all_targets,
            ..Default::default()
        })?;
        Ok(Self {
            package: checked.package().map(str::to_owned),
            workspace: checked.workspace(),
            features: checked.features().to_vec(),
            all_targets: checked.all_targets(),
            lint_profile: value.lint_profile,
        })
    }
}
impl ClippyOptions {
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
    pub fn workspace(&self) -> bool {
        self.workspace
    }
    pub fn features(&self) -> &[String] {
        &self.features
    }
    pub fn all_targets(&self) -> bool {
        self.all_targets
    }
    pub fn lint_profile(&self) -> LintProfile {
        self.lint_profile
    }
}
#[derive(Clone, Debug)]
pub struct ProjectClippy {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub semantics: InspectionSemantics,
    pub options: ClippyOptions,
    pub observation: CheckObservation,
    pub evidence: SnapshotEvidence,
    pub log: Option<ArtifactMetadata>,
    pub retention_remaining_seconds: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_all_profiles_roundtrip_only_supported_keys()
    -> Result<(), Box<dyn std::error::Error>> {
        let options: ClippyOptions = serde_json::from_str("{}")?;
        assert_eq!(options.package(), None);
        assert!(!options.workspace() && !options.all_targets());
        assert!(options.features().is_empty());
        assert_eq!(options.lint_profile(), LintProfile::Default);
        assert_eq!(
            serde_json::to_value(&options)?,
            serde_json::json!({
                "package": null, "workspace": false, "features": [],
                "all_targets": false, "lint_profile": "default"
            })
        );
        for profile in ["default", "strict", "pedantic", "project"] {
            let options: ClippyOptions =
                serde_json::from_value(serde_json::json!({"lint_profile": profile}))?;
            assert_eq!(serde_json::to_value(&options)?["lint_profile"], profile);
        }
        Ok(())
    }

    #[test]
    fn deserialization_cannot_bypass_closed_validated_options() {
        for input in [
            serde_json::json!({"lint_profile": "all"}),
            serde_json::json!({"lint_profile": "Strict"}),
            serde_json::json!({"package": "member", "workspace": true}),
            serde_json::json!({"package": "--manifest-path=x"}),
            serde_json::json!({"features": ["std", "std"]}),
        ] {
            assert!(serde_json::from_value::<ClippyOptions>(input).is_err());
        }
        for key in [
            "target",
            "all_features",
            "no_default_features",
            "args",
            "config",
            "fix",
        ] {
            let input = serde_json::json!({key: false});
            assert!(serde_json::from_value::<ClippyOptions>(input).is_err());
        }
        for name in [
            "",
            "-a",
            "a,b",
            "a b",
            "a/../b",
            "a//b",
            "a\nb",
            "$(echo x)",
        ] {
            assert!(
                ClippyOptions::try_from(ClippySelection {
                    features: vec![name.into()],
                    ..Default::default()
                })
                .is_err()
            );
        }
        for features in [vec!["a".into(); 33], vec!["a".repeat(129)]] {
            assert!(
                ClippyOptions::try_from(ClippySelection {
                    features,
                    ..Default::default()
                })
                .is_err()
            );
        }
    }

    #[test]
    fn selection_preserves_package_targets_profile_and_sorts_features()
    -> Result<(), InvalidCheckOptions> {
        let options = ClippyOptions::try_from(ClippySelection {
            package: Some("member-name".into()),
            all_targets: true,
            features: vec!["std".into(), "serde/derive".into()],
            lint_profile: LintProfile::Pedantic,
            ..Default::default()
        })?;
        assert_eq!(options.package(), Some("member-name"));
        assert!(options.all_targets());
        assert_eq!(options.features(), ["serde/derive", "std"]);
        assert_eq!(options.lint_profile(), LintProfile::Pedantic);
        Ok(())
    }
}
