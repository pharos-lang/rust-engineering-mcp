//! Bounded selections for the configured Cargo test command, not a coverage claim.
use crate::{
    ArtifactMetadata, CheckObservation, CheckOptions, CheckSelection, InspectionSemantics,
    InvalidCheckOptions, ProjectIdentityFingerprint, ProjectRef, SnapshotEvidence,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TestSelection {
    pub package: Option<String>,
    pub test_filter: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub target: Option<String>,
    pub timeout: u64,
}
impl Default for TestSelection {
    fn default() -> Self {
        Self {
            package: None,
            test_filter: None,
            features: Vec::new(),
            all_features: false,
            target: None,
            timeout: 30,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TestSelection")]
pub struct TestOptions {
    package: Option<String>,
    test_filter: Option<String>,
    features: Vec<String>,
    all_features: bool,
    target: Option<String>,
    timeout: u64,
}
impl TryFrom<TestSelection> for TestOptions {
    type Error = InvalidCheckOptions;
    fn try_from(value: TestSelection) -> Result<Self, Self::Error> {
        if !(1..=60).contains(&value.timeout)
            || value.test_filter.as_ref().is_some_and(|filter| {
                filter.is_empty()
                    || filter.len() > 128
                    || !filter
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_:".contains(&b))
                    || !filter
                        .as_bytes()
                        .first()
                        .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            })
        {
            return Err(InvalidCheckOptions);
        }
        let checked = CheckOptions::try_from(CheckSelection {
            package: value.package,
            features: value.features,
            all_features: value.all_features,
            target: value.target,
            ..Default::default()
        })?;
        Ok(Self {
            package: checked.package().map(str::to_owned),
            test_filter: value.test_filter,
            features: checked.features().to_vec(),
            all_features: checked.all_features(),
            target: checked.target().map(str::to_owned),
            timeout: value.timeout,
        })
    }
}
impl TestOptions {
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
    }
    pub fn test_filter(&self) -> Option<&str> {
        self.test_filter.as_deref()
    }
    pub fn features(&self) -> &[String] {
        &self.features
    }
    pub fn all_features(&self) -> bool {
        self.all_features
    }
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
    pub fn timeout(&self) -> u64 {
        self.timeout
    }
}
#[derive(Clone, Debug)]
pub struct TestObservation {
    pub execution: CheckObservation,
    pub build_succeeded: Option<bool>,
}
#[derive(Clone, Debug)]
pub struct ProjectTest {
    pub project_ref: ProjectRef,
    pub project_identity_fingerprint: ProjectIdentityFingerprint,
    pub semantics: InspectionSemantics,
    pub options: TestOptions,
    pub observation: TestObservation,
    pub evidence: SnapshotEvidence,
    pub log: Option<ArtifactMetadata>,
    pub retention_remaining_seconds: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_filter_and_timeout_reject_injection_and_non_ascii() -> Result<(), InvalidCheckOptions>
    {
        for filter in [
            "",
            "--ignored",
            "a b",
            "a\nb",
            "$(id)",
            "a;b",
            "é",
            ":case",
            "a-b",
        ] {
            assert_eq!(
                TestOptions::try_from(TestSelection {
                    test_filter: Some(filter.into()),
                    ..Default::default()
                }),
                Err(InvalidCheckOptions),
                "{filter:?}"
            );
        }
        assert!(
            TestOptions::try_from(TestSelection {
                test_filter: Some("a".repeat(129)),
                ..Default::default()
            })
            .is_err()
        );
        for timeout in [0, 61, u64::MAX] {
            assert!(
                TestOptions::try_from(TestSelection {
                    timeout,
                    ..Default::default()
                })
                .is_err()
            );
        }
        for timeout in [1, 30, 60] {
            let options = TestOptions::try_from(TestSelection {
                test_filter: Some(format!("_module::{}", "a".repeat(119))),
                timeout,
                ..Default::default()
            })?;
            assert_eq!(options.timeout(), timeout);
            assert_eq!(options.test_filter().map(str::len), Some(128));
        }
        assert_eq!(
            TestOptions::try_from(TestSelection::default())?.timeout(),
            30
        );
        Ok(())
    }

    #[test]
    fn closed_selection_preserves_features_and_only_installed_target()
    -> Result<(), InvalidCheckOptions> {
        for selection in [
            TestSelection {
                features: vec!["std".into()],
                all_features: true,
                ..Default::default()
            },
            TestSelection {
                features: vec!["std".into(), "std".into()],
                ..Default::default()
            },
            TestSelection {
                package: Some("--workspace".into()),
                ..Default::default()
            },
            TestSelection {
                features: vec!["--all-features".into()],
                ..Default::default()
            },
            TestSelection {
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
            TestSelection {
                target: Some("./custom.json".into()),
                ..Default::default()
            },
        ] {
            assert!(TestOptions::try_from(selection).is_err());
        }
        let options = TestOptions::try_from(TestSelection {
            package: Some("member".into()),
            features: vec!["std".into(), "member/extra".into()],
            target: Some("aarch64-unknown-linux-gnu".into()),
            ..Default::default()
        })?;
        assert_eq!(options.package(), Some("member"));
        assert_eq!(options.features(), &["member/extra", "std"]);
        assert!(!options.all_features());
        assert_eq!(options.target(), Some("aarch64-unknown-linux-gnu"));
        assert!(
            TestOptions::try_from(TestSelection {
                all_features: true,
                ..Default::default()
            })?
            .all_features()
        );
        Ok(())
    }

    #[test]
    fn serde_cannot_bypass_selection_validation_or_add_harness_args()
    -> Result<(), serde_json::Error> {
        for json in [
            r#"{"timeout":0}"#,
            r#"{"timeout":61}"#,
            r#"{"timeout":-1}"#,
            r#"{"timeout":1.5}"#,
            r#"{"timeout":"30"}"#,
            r#"{"timeout":null}"#,
            r#"{"all_features":"true"}"#,
            r#"{"features":"std"}"#,
            r#"{"test_filter":5}"#,
            r#"{"test_filter":"--ignored"}"#,
            r#"{"args":["--ignored"]}"#,
            r#"{"workspace":true}"#,
            r#"{"all_targets":true}"#,
            r#"{"no_default_features":true}"#,
        ] {
            assert!(serde_json::from_str::<TestOptions>(json).is_err(), "{json}");
        }
        let options: TestOptions =
            serde_json::from_str(r#"{"test_filter":"module::case","timeout":60}"#)?;
        let encoded = serde_json::to_string(&options)?;
        assert_eq!(serde_json::from_str::<TestOptions>(&encoded)?, options);
        Ok(())
    }
}
