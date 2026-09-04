//! Validated Cargo selection and evidence for one captured check operation.
use crate::{
    Diagnostic, ExecutionTermination, InspectionSemantics, ProjectIdentityFingerprint, ProjectRef,
    RuntimeIdentity, SnapshotEvidence, SourceFingerprint,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CheckSelection {
    pub package: Option<String>,
    pub workspace: bool,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub all_targets: bool,
    pub target: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CheckSelection")]
pub struct CheckOptions {
    package: Option<String>,
    workspace: bool,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    all_targets: bool,
    target: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidCheckOptions;
impl std::fmt::Display for InvalidCheckOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid Cargo selection")
    }
}
impl std::error::Error for InvalidCheckOptions {}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && !value.starts_with('-')
}
impl TryFrom<CheckSelection> for CheckOptions {
    type Error = InvalidCheckOptions;
    fn try_from(mut value: CheckSelection) -> Result<Self, Self::Error> {
        if value.package.as_deref().is_some_and(|v| !identifier(v))
            || value.package.is_some() && value.workspace
            || value.all_features && !value.features.is_empty()
            || value.features.len() > 32
            || value.features.iter().any(|f| {
                f.len() > 128 || !f.split('/').all(identifier) || f.matches('/').count() > 1
            })
            || value
                .target
                .as_deref()
                .is_some_and(|v| v != "aarch64-unknown-linux-gnu")
        {
            return Err(InvalidCheckOptions);
        }
        value.features.sort();
        if value.features.windows(2).any(|v| v[0] == v[1]) {
            return Err(InvalidCheckOptions);
        }
        Ok(Self {
            package: value.package,
            workspace: value.workspace,
            features: value.features,
            all_features: value.all_features,
            no_default_features: value.no_default_features,
            all_targets: value.all_targets,
            target: value.target,
        })
    }
}
impl CheckOptions {
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
    pub fn workspace(&self) -> bool {
        self.workspace
    }
    pub fn features(&self) -> &[String] {
        &self.features
    }
    pub fn all_features(&self) -> bool {
        self.all_features
    }
    pub fn no_default_features(&self) -> bool {
        self.no_default_features
    }
    pub fn all_targets(&self) -> bool {
        self.all_targets
    }
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    Passed,
    Failed,
    Incomplete,
    LockfileUpdateRequired,
}
#[derive(Clone, Debug)]
pub struct CheckObservation {
    pub outcome: CheckOutcome,
    pub termination: ExecutionTermination,
    pub exit_code: Option<i32>,
    pub validation_complete: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub diagnostics_omitted: u64,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub runtime: RuntimeIdentity,
    pub source_fingerprint: SourceFingerprint,
}
#[derive(Clone, Debug)]
pub struct ProjectCheck {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub semantics: InspectionSemantics,
    pub options: CheckOptions,
    pub observation: CheckObservation,
    pub evidence: SnapshotEvidence,
    pub log: Option<crate::ArtifactMetadata>,
    pub retention_remaining_seconds: Option<u64>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_contradictions_duplicates_options_and_unsupported_targets() {
        assert!(CheckOptions::try_from(CheckSelection::default()).is_ok());
        let mut bad = vec![
            CheckSelection {
                package: Some("--manifest-path=/tmp/x".into()),
                ..Default::default()
            },
            CheckSelection {
                package: Some("member".into()),
                workspace: true,
                ..Default::default()
            },
            CheckSelection {
                features: vec!["a".into()],
                all_features: true,
                ..Default::default()
            },
            CheckSelection {
                features: vec!["a".into(), "a".into()],
                ..Default::default()
            },
            CheckSelection {
                features: vec!["a".into(); 33],
                ..Default::default()
            },
            CheckSelection {
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        ];
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
            bad.push(CheckSelection {
                features: vec![name.into()],
                ..Default::default()
            });
        }
        for value in bad {
            assert_eq!(CheckOptions::try_from(value), Err(InvalidCheckOptions));
        }
        let valid = CheckOptions::try_from(CheckSelection {
            features: vec!["serde/derive".into(), "std".into()],
            ..Default::default()
        });
        assert!(valid.is_ok());
    }
}
