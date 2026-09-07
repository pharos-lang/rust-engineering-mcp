//! Closed cargo-semver-checks selection, outcome taxonomy and bounded,
//! best-effort findings. Exit-code classification and the per-finding
//! extraction mechanism are documented HYPOTHESES pending Docker calibration
//! against the pinned 0.50.0 binary (ADR-062 §9/§11; see
//! docs/validation/M3-04-semver-calibration.md for the recorded evidence).
use crate::{CheckOptions, CheckSelection, InvalidCheckOptions};
use serde::{Deserialize, Serialize};

/// Verified once during explicit M3 provisioning (ADR-063); never inferred
/// from an installed-file heuristic. A mismatch is `Unavailable`, checked
/// before any coverage/semver command runs.
pub const APPROVED_SEMVER_CHECKS_VERSION: &str = "0.50.0";

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SemverProjectSelection {
    pub package: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub target: Option<String>,
}

/// Closed selection applied identically to both the baseline and candidate
/// sides of one `cargo semver-checks check-release` invocation: the tool
/// itself only accepts one shared `--features`/`--all-features`/etc. set
/// (`--baseline-features`/`--current-features` are asymmetric per-side
/// escape hatches this product deliberately never exposes), so there is
/// exactly one validated selection here, never two independently supplied
/// ones that could silently diverge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SemverProjectSelection")]
pub struct SemverCommandOptions {
    package: Option<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    target: Option<String>,
}

impl TryFrom<SemverProjectSelection> for SemverCommandOptions {
    type Error = InvalidCheckOptions;
    fn try_from(value: SemverProjectSelection) -> Result<Self, Self::Error> {
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
            features: checked.features().to_vec(),
            all_features: checked.all_features(),
            no_default_features: checked.no_default_features(),
            target: checked.target().map(str::to_owned),
        })
    }
}

impl SemverCommandOptions {
    pub fn package(&self) -> Option<&str> {
        self.package.as_deref()
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
}

/// Classification of cargo-semver-checks 0.50.0 exit codes observed against
/// the approved M3 image. `Uncalibrated` is the fail-closed catch-all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemverExit {
    /// Observed: 0, no deny-level violation (warn-level findings may
    /// still be present and must still be surfaced).
    NoBreak,
    /// Observed: 100, one or more deny-level violations found.
    Breaking,
    /// Observed: 101, could not complete: rustdoc/build failure or a
    /// connectivity problem.
    Incomplete,
    /// Any other code; calibration has not pinned a meaning for it.
    Uncalibrated,
}

impl SemverExit {
    /// Pinned by the Docker cases recorded in the M3-04 calibration receipt.
    pub const CALIBRATED: bool = true;

    pub fn classify(code: i32) -> Self {
        match code {
            0 => Self::NoBreak,
            100 => Self::Breaking,
            101 => Self::Incomplete,
            _ => Self::Uncalibrated,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemverFindingLevel {
    Deny,
    Warn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemverRequiredUpdate {
    Major,
    Minor,
    Patch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSemverFinding;
impl std::fmt::Display for InvalidSemverFinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("semver finding field exceeds its bounded length")
    }
}
impl std::error::Error for InvalidSemverFinding {}

const MAX_SEMVER_FIELD: usize = 512;

/// One best-effort finding extracted by the bounded text parser
/// (`semver_output.rs`). Per ADR-062 §11 this is never promoted past
/// `SemverFindingCompleteness::Partial`: the pinned binary exposes no
/// machine-readable findings flag, so every field here comes from scraping
/// non-colored terminal text and may be missing or approximate. Failure to
/// recognize the report makes the whole operation `Incomplete`; it can never
/// be interpreted as a clean comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SemverFinding {
    item: String,
    lint: String,
    level: SemverFindingLevel,
    required_update: Option<SemverRequiredUpdate>,
    span: Option<String>,
}

impl SemverFinding {
    pub fn new(
        item: String,
        lint: String,
        level: SemverFindingLevel,
        required_update: Option<SemverRequiredUpdate>,
        span: Option<String>,
    ) -> Result<Self, InvalidSemverFinding> {
        if item.is_empty()
            || item.len() > MAX_SEMVER_FIELD
            || lint.is_empty()
            || lint.len() > MAX_SEMVER_FIELD
            || span
                .as_ref()
                .is_some_and(|value| value.len() > MAX_SEMVER_FIELD)
        {
            return Err(InvalidSemverFinding);
        }
        Ok(Self {
            item,
            lint,
            level,
            required_update,
            span,
        })
    }
    pub fn item(&self) -> &str {
        &self.item
    }
    pub fn lint(&self) -> &str {
        &self.lint
    }
    pub fn level(&self) -> SemverFindingLevel {
        self.level
    }
    pub fn required_update(&self) -> Option<SemverRequiredUpdate> {
        self.required_update
    }
    pub fn span(&self) -> Option<&str> {
        self.span.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SemverFindingCounts {
    pub deny: u32,
    pub warn: u32,
}
impl SemverFindingCounts {
    pub fn total(&self) -> u64 {
        u64::from(self.deny) + u64::from(self.warn)
    }
}

/// Per ADR-062 §11: the finding *list* is never `Complete` for this pinned
/// binary (no machine-readable output exists), only `Partial` (some
/// best-effort rows extracted) or `Incomplete` (the parser recognized
/// nothing at all within its bounds). This is independent of the coarse
/// `SemverExit`-derived coarse outcome; application policy degrades a parser
/// failure to `Incomplete` so it cannot create a false "no break" result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemverFindingCompleteness {
    Partial,
    Incomplete,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_rejects_the_same_contradictions_as_check_options()
    -> Result<(), InvalidCheckOptions> {
        for selection in [
            SemverProjectSelection {
                package: Some("--manifest-path=/tmp/x".into()),
                ..Default::default()
            },
            SemverProjectSelection {
                features: vec!["a".into()],
                all_features: true,
                ..Default::default()
            },
            SemverProjectSelection {
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        ] {
            assert!(SemverCommandOptions::try_from(selection).is_err());
        }
        let options = SemverCommandOptions::try_from(SemverProjectSelection {
            package: Some("member".into()),
            features: vec!["std".into(), "member/extra".into()],
            target: Some("aarch64-unknown-linux-gnu".into()),
            ..Default::default()
        })?;
        assert_eq!(options.package(), Some("member"));
        assert_eq!(options.features(), &["member/extra", "std"]);
        assert_eq!(options.target(), Some("aarch64-unknown-linux-gnu"));
        Ok(())
    }

    #[test]
    fn serde_cannot_add_baseline_or_current_only_feature_escape_hatches()
    -> Result<(), serde_json::Error> {
        for json in [
            r#"{"baseline_features":["x"]}"#,
            r#"{"current_features":["x"]}"#,
            r#"{"workspace":true}"#,
            r#"{"release_type":"major"}"#,
        ] {
            assert!(
                serde_json::from_str::<SemverCommandOptions>(json).is_err(),
                "{json}"
            );
        }
        Ok(())
    }

    #[test]
    fn exit_classification_matches_calibrated_guest_observations() {
        assert_eq!(SemverExit::classify(0), SemverExit::NoBreak);
        assert_eq!(SemverExit::classify(100), SemverExit::Breaking);
        assert_eq!(SemverExit::classify(101), SemverExit::Incomplete);
        for other in [1, 2, 99, 102, -1, i32::MAX, i32::MIN] {
            assert_eq!(
                SemverExit::classify(other),
                SemverExit::Uncalibrated,
                "{other}"
            );
        }
        const { assert!(SemverExit::CALIBRATED) };
    }

    #[test]
    fn finding_construction_bounds_every_text_field() -> Result<(), InvalidSemverFinding> {
        assert!(
            SemverFinding::new(
                String::new(),
                "lint".into(),
                SemverFindingLevel::Deny,
                None,
                None
            )
            .is_err()
        );
        assert!(
            SemverFinding::new(
                "item".into(),
                "a".repeat(MAX_SEMVER_FIELD + 1),
                SemverFindingLevel::Deny,
                None,
                None
            )
            .is_err()
        );
        assert!(
            SemverFinding::new(
                "item".into(),
                "lint".into(),
                SemverFindingLevel::Deny,
                None,
                Some("a".repeat(MAX_SEMVER_FIELD + 1))
            )
            .is_err()
        );
        let finding = SemverFinding::new(
            "pub fn answer".into(),
            "function_missing".into(),
            SemverFindingLevel::Deny,
            Some(SemverRequiredUpdate::Major),
            Some("src/lib.rs:1".into()),
        )?;
        assert_eq!(finding.item(), "pub fn answer");
        assert_eq!(finding.lint(), "function_missing");
        assert_eq!(finding.level(), SemverFindingLevel::Deny);
        assert_eq!(finding.required_update(), Some(SemverRequiredUpdate::Major));
        assert_eq!(finding.span(), Some("src/lib.rs:1"));
        Ok(())
    }

    #[test]
    fn finding_counts_total_is_the_sum_of_deny_and_warn() {
        let counts = SemverFindingCounts { deny: 2, warn: 3 };
        assert_eq!(counts.total(), 5);
    }
}
