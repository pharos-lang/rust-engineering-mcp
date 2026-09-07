//! Closed cargo-nextest selection, outcome counts and bounded per-test rows.
//! Exit-code classification is a documented HYPOTHESIS pending Docker calibration.
use crate::{CheckOptions, CheckSelection, InvalidCheckOptions};
use serde::{Deserialize, Serialize};

/// Fixed profile name written into the product-owned nextest configuration.
/// Never derived from caller input.
pub const NEXTEST_PROFILE: &str = "rust-mcp";

/// Matches the application layer's `NEXTEST_MAX_TIMEOUT_SECONDS` (ADR-060
/// job work budgets), not the smaller legacy 60s M1 `TestOptions` bound.
pub const NEXTEST_MAX_TIMEOUT_SECONDS: u64 = 3_600;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NextestSelection {
    pub package: Option<String>,
    pub test_filter: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub target: Option<String>,
    pub timeout: u64,
    pub retries: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "NextestSelection")]
pub struct NextestCommandOptions {
    package: Option<String>,
    test_filter: Option<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    target: Option<String>,
    timeout: u64,
    retries: u8,
}

impl Default for NextestSelection {
    fn default() -> Self {
        Self {
            package: None,
            test_filter: None,
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            target: None,
            timeout: 30,
            retries: 0,
        }
    }
}

impl TryFrom<NextestSelection> for NextestCommandOptions {
    type Error = InvalidCheckOptions;
    fn try_from(value: NextestSelection) -> Result<Self, Self::Error> {
        if !(1..=NEXTEST_MAX_TIMEOUT_SECONDS).contains(&value.timeout)
            || !(0..=2).contains(&value.retries)
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
            no_default_features: value.no_default_features,
            target: value.target,
            ..Default::default()
        })?;
        Ok(Self {
            package: checked.package().map(str::to_owned),
            test_filter: value.test_filter,
            features: checked.features().to_vec(),
            all_features: checked.all_features(),
            no_default_features: checked.no_default_features(),
            target: checked.target().map(str::to_owned),
            timeout: value.timeout,
            retries: value.retries,
        })
    }
}

impl NextestCommandOptions {
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
    pub fn no_default_features(&self) -> bool {
        self.no_default_features
    }
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }
    /// Seconds; also drives the fixed `slow-timeout` period in the generated
    /// product-owned nextest configuration.
    pub fn timeout(&self) -> u64 {
        self.timeout
    }
    pub fn retries(&self) -> u8 {
        self.retries
    }
}

/// Aggregate counts from a parsed JUnit report. Comes only from the parser,
/// never from human-readable stdout/stderr text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct NextestOutcomeCounts {
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    /// Additional attempts represented by JUnit rerun/flaky elements.
    pub retried: u32,
    pub flaky: u32,
    pub leaky: u32,
    pub timed_out: u32,
}
impl NextestOutcomeCounts {
    pub fn total(&self) -> u64 {
        u64::from(self.passed)
            + u64::from(self.failed)
            + u64::from(self.skipped)
            + u64::from(self.flaky)
            + u64::from(self.leaky)
            + u64::from(self.timed_out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextestTestOutcome {
    Passed,
    Failed,
    Skipped,
    Flaky,
    Leaky,
    TimedOut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidNextestRow;
impl std::fmt::Display for InvalidNextestRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("nextest test row exceeds bounded name/classname length")
    }
}
impl std::error::Error for InvalidNextestRow {}

const MAX_NEXTEST_NAME: usize = 512;

/// One bounded testcase row extracted from a JUnit report. The parser is the
/// only producer; construction validates length so nothing downstream can
/// exceed the bound regardless of guest output size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NextestTestRow {
    name: String,
    classname: String,
    outcome: NextestTestOutcome,
    attempts: u16,
    time_ms: u64,
}
impl NextestTestRow {
    pub fn new(
        name: String,
        classname: String,
        outcome: NextestTestOutcome,
        time_ms: u64,
    ) -> Result<Self, InvalidNextestRow> {
        Self::new_with_attempts(name, classname, outcome, 1, time_ms)
    }

    pub fn new_with_attempts(
        name: String,
        classname: String,
        outcome: NextestTestOutcome,
        attempts: u16,
        time_ms: u64,
    ) -> Result<Self, InvalidNextestRow> {
        if name.is_empty() || name.len() > MAX_NEXTEST_NAME || classname.len() > MAX_NEXTEST_NAME {
            return Err(InvalidNextestRow);
        }
        if attempts == 0 {
            return Err(InvalidNextestRow);
        }
        Ok(Self {
            name,
            classname,
            outcome,
            attempts,
            time_ms,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn classname(&self) -> &str {
        &self.classname
    }
    pub fn outcome(&self) -> NextestTestOutcome {
        self.outcome
    }
    pub fn attempts(&self) -> u16 {
        self.attempts
    }
    pub fn time_ms(&self) -> u64 {
        self.time_ms
    }
}

/// Classification of cargo-nextest 0.9.143 process exit codes observed against
/// the approved M3 image. `Uncalibrated` is the fail-closed catch-all for any
/// code not exercised by the Docker qualification.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NextestExit {
    /// Observed: 0, all selected tests passed.
    Success,
    /// Observed: 4, the product `no-tests = "fail"` policy found no tests.
    NoTests,
    /// Observed: 100, one or more tests failed, including a slow-test timeout.
    TestFailure,
    /// Observed: 101, the runner or build itself failed (not a test result).
    RunnerFailure,
    /// Any other code; calibration has not pinned a meaning for it.
    Uncalibrated,
}
impl NextestExit {
    /// The product-relevant exit paths below are pinned by Docker tests against
    /// cargo-nextest 0.9.143 in the approved image. Joined gateway cancellation
    /// intentionally has no process exit code and is classified by termination.
    pub const CALIBRATED: bool = true;
    pub fn classify(code: i32) -> Self {
        match code {
            0 => Self::Success,
            4 => Self::NoTests,
            100 => Self::TestFailure,
            101 => Self::RunnerFailure,
            _ => Self::Uncalibrated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_filter_timeout_and_retries_reject_injection_and_out_of_range()
    -> Result<(), InvalidCheckOptions> {
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
                NextestCommandOptions::try_from(NextestSelection {
                    test_filter: Some(filter.into()),
                    ..Default::default()
                }),
                Err(InvalidCheckOptions),
                "{filter:?}"
            );
        }
        for timeout in [0, NEXTEST_MAX_TIMEOUT_SECONDS + 1, u64::MAX] {
            assert!(
                NextestCommandOptions::try_from(NextestSelection {
                    timeout,
                    ..Default::default()
                })
                .is_err()
            );
        }
        for retries in [3, u8::MAX] {
            assert!(
                NextestCommandOptions::try_from(NextestSelection {
                    retries,
                    ..Default::default()
                })
                .is_err()
            );
        }
        for retries in [0, 1, 2] {
            let options = NextestCommandOptions::try_from(NextestSelection {
                retries,
                ..Default::default()
            })?;
            assert_eq!(options.retries(), retries);
        }
        assert_eq!(
            NextestCommandOptions::try_from(NextestSelection::default())?.timeout(),
            30
        );
        Ok(())
    }

    #[test]
    fn closed_selection_preserves_features_and_only_installed_target()
    -> Result<(), InvalidCheckOptions> {
        for selection in [
            NextestSelection {
                features: vec!["std".into()],
                all_features: true,
                ..Default::default()
            },
            NextestSelection {
                package: Some("--workspace".into()),
                ..Default::default()
            },
            NextestSelection {
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        ] {
            assert!(NextestCommandOptions::try_from(selection).is_err());
        }
        let options = NextestCommandOptions::try_from(NextestSelection {
            package: Some("member".into()),
            features: vec!["std".into(), "member/extra".into()],
            no_default_features: true,
            target: Some("aarch64-unknown-linux-gnu".into()),
            ..Default::default()
        })?;
        assert_eq!(options.package(), Some("member"));
        assert_eq!(options.features(), &["member/extra", "std"]);
        assert!(options.no_default_features());
        assert_eq!(options.target(), Some("aarch64-unknown-linux-gnu"));
        Ok(())
    }

    #[test]
    fn serde_cannot_bypass_selection_validation_or_add_harness_args()
    -> Result<(), serde_json::Error> {
        for json in [
            r#"{"timeout":0}"#,
            r#"{"retries":3}"#,
            r#"{"retries":-1}"#,
            r#"{"retries":"0"}"#,
            r#"{"args":["--ignored"]}"#,
            r#"{"profile":"other"}"#,
            r#"{"config_file":"/tmp/x"}"#,
        ] {
            assert!(
                serde_json::from_str::<NextestCommandOptions>(json).is_err(),
                "{json}"
            );
        }
        let options: NextestCommandOptions =
            serde_json::from_str(r#"{"test_filter":"module::case","timeout":60,"retries":2}"#)?;
        let encoded = serde_json::to_string(&options)?;
        assert_eq!(
            serde_json::from_str::<NextestCommandOptions>(&encoded)?,
            options
        );
        Ok(())
    }

    #[test]
    fn outcome_counts_total_and_bounded_row_construction() -> Result<(), InvalidNextestRow> {
        let counts = NextestOutcomeCounts {
            passed: 2,
            failed: 1,
            skipped: 1,
            retried: 4,
            flaky: 1,
            leaky: 1,
            timed_out: 1,
        };
        assert_eq!(counts.total(), 7);
        assert!(
            NextestTestRow::new(String::new(), String::new(), NextestTestOutcome::Passed, 0)
                .is_err()
        );
        assert!(
            NextestTestRow::new(
                "a".repeat(MAX_NEXTEST_NAME + 1),
                String::new(),
                NextestTestOutcome::Passed,
                0
            )
            .is_err()
        );
        let row = NextestTestRow::new(
            "case".into(),
            "pkg::mod".into(),
            NextestTestOutcome::Flaky,
            42,
        )?;
        assert_eq!(row.name(), "case");
        assert_eq!(row.classname(), "pkg::mod");
        assert_eq!(row.outcome(), NextestTestOutcome::Flaky);
        assert_eq!(row.time_ms(), 42);
        Ok(())
    }

    #[test]
    fn exit_classification_matches_docker_calibration() {
        assert_eq!(NextestExit::classify(0), NextestExit::Success);
        assert_eq!(NextestExit::classify(4), NextestExit::NoTests);
        assert_eq!(NextestExit::classify(100), NextestExit::TestFailure);
        assert_eq!(NextestExit::classify(101), NextestExit::RunnerFailure);
        for other in [1, 2, 99, 102, 103, 104, 105, -1, i32::MAX, i32::MIN] {
            assert_eq!(
                NextestExit::classify(other),
                NextestExit::Uncalibrated,
                "{other}"
            );
        }
        const { assert!(NextestExit::CALIBRATED) };
    }
}
