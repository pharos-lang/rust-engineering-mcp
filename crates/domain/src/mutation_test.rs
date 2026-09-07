//! Closed `cargo-mutants` selection, budgets and outcome vocabulary.
//!
//! Every value here is either fixed by the product or comes from an explicitly
//! validated caller selection. Nothing in this module is derived from guest
//! stdout/stderr text: the adapter's bounded `mutants.out` parser is the only
//! producer of counts and rows, and it constructs them through the validating
//! constructors below.
//!
//! Exit-code classification is a documented HYPOTHESIS taken from the official
//! cargo-mutants documentation captured for M3-05. Unlike `nextest::NextestExit`
//! it is NOT yet confirmed against the pinned guest binary, so
//! [`MutationExit::CALIBRATED`] is `true`, pinned by the Q01 Docker receipt; no
//! exit code alone may promote a
//! result to clean.
use crate::{CheckOptions, CheckSelection, InvalidCheckOptions};
use serde::{Deserialize, Serialize};

/// Hard ceiling on mutants per job (roadmap M3-05: "máximo propuesto 100
/// mutantes/job, sin sharding inicial"). A generated set larger than the
/// caller's cap is refused before any mutant is built, never silently sharded.
pub const MUTATION_MAX_MUTANTS: u32 = 100;
pub const MUTATION_DEFAULT_MAX_MUTANTS: u32 = 100;
/// Per-mutant `--timeout` bound. The whole job additionally lives inside the
/// ADR-060 execute budget enforced by the application/gateway layers.
pub const MUTATION_MAX_MUTANT_TIMEOUT_SECONDS: u64 = 60;
pub const MUTATION_DEFAULT_MUTANT_TIMEOUT_SECONDS: u64 = 60;
/// `--build-timeout` is never caller supplied; it is a fixed function of the
/// validated per-mutant timeout, clamped into this closed range.
pub const MUTATION_MIN_BUILD_TIMEOUT_SECONDS: u64 = 60;
pub const MUTATION_MAX_BUILD_TIMEOUT_SECONDS: u64 = 300;
/// Bound on itemized mutant rows carried out of the adapter.
pub const MUTATION_MAX_ROWS: usize = 128;
/// Bound on one mutant description taken from a `mutants.out` list file.
pub const MUTATION_MAX_ROW_NAME: usize = 256;
/// Bound on the reported `cargo-mutants` version text.
pub const MUTATION_MAX_VERSION: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MutationTestSelection {
    pub package: Option<String>,
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
    pub target: Option<String>,
    pub max_mutants: u32,
    pub mutant_timeout_seconds: u64,
}

impl Default for MutationTestSelection {
    fn default() -> Self {
        Self {
            package: None,
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            target: None,
            max_mutants: MUTATION_DEFAULT_MAX_MUTANTS,
            mutant_timeout_seconds: MUTATION_DEFAULT_MUTANT_TIMEOUT_SECONDS,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "MutationTestSelection")]
pub struct MutationTestCommandOptions {
    package: Option<String>,
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    target: Option<String>,
    max_mutants: u32,
    mutant_timeout_seconds: u64,
}

impl TryFrom<MutationTestSelection> for MutationTestCommandOptions {
    type Error = InvalidCheckOptions;

    fn try_from(value: MutationTestSelection) -> Result<Self, Self::Error> {
        if !(1..=MUTATION_MAX_MUTANTS).contains(&value.max_mutants)
            || !(1..=MUTATION_MAX_MUTANT_TIMEOUT_SECONDS).contains(&value.mutant_timeout_seconds)
        {
            return Err(InvalidCheckOptions);
        }
        // Reuse the single validated Cargo selection grammar: package identifier,
        // bounded features without an `--all-features` contradiction, and only
        // the one installed target triple.
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
            max_mutants: value.max_mutants,
            mutant_timeout_seconds: value.mutant_timeout_seconds,
        })
    }
}

impl MutationTestCommandOptions {
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
    pub fn max_mutants(&self) -> u32 {
        self.max_mutants
    }
    /// Seconds passed to `--timeout`; also the `--minimum-test-timeout` floor so
    /// the tool never derives a longer timeout from a slow baseline.
    pub fn mutant_timeout_seconds(&self) -> u64 {
        self.mutant_timeout_seconds
    }
    /// `--build-timeout`, a fixed function of the validated per-mutant timeout.
    /// Builds are slower than tests, so the multiplier is deliberately larger
    /// than one, but the closed clamp keeps the argv bounded and predictable.
    pub fn build_timeout_seconds(&self) -> u64 {
        self.mutant_timeout_seconds.saturating_mul(5).clamp(
            MUTATION_MIN_BUILD_TIMEOUT_SECONDS,
            MUTATION_MAX_BUILD_TIMEOUT_SECONDS,
        )
    }
}

/// `outcomes.json` scenario discriminator. The mandatory baseline is a separate
/// scenario from every mutant and is never counted as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationScenario {
    Baseline,
    Mutant,
}

/// The closed set of `outcomes.json` summary values this product understands.
/// An unknown summary is not mapped to anything: the parser reports invalid
/// evidence instead, so a future vocabulary change cannot silently pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationOutcomeClass {
    /// A test failed with the mutant applied: the mutant was detected.
    Caught,
    /// Tests still passed with the mutant applied: an inadequately tested path.
    Missed,
    /// The mutant made a test hang past the bounded per-mutant timeout.
    Timeout,
    /// The mutated tree failed to build; it proves nothing about test quality.
    Unviable,
    /// Tests passed for this scenario. Only meaningful for the baseline.
    Success,
    /// Tests failed for this scenario. Only meaningful for the baseline.
    Failure,
}

impl MutationOutcomeClass {
    /// Exact `cargo-mutants` summary spellings. Matching is case sensitive so a
    /// forged lowercase copy of the vocabulary in project output cannot alias a
    /// real machine-written summary.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "CaughtMutant" => Some(Self::Caught),
            "MissedMutant" => Some(Self::Missed),
            "Timeout" => Some(Self::Timeout),
            "Unviable" => Some(Self::Unviable),
            "Success" => Some(Self::Success),
            "Failure" => Some(Self::Failure),
            _ => None,
        }
    }

    /// Only a caught mutant contributes to a clean result. Timeout, unviable and
    /// every non-mutant class explicitly do not.
    pub fn credits_clean(self) -> bool {
        matches!(self, Self::Caught)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidMutationRow;
impl std::fmt::Display for InvalidMutationRow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("mutant row is empty, oversized or not printable ASCII")
    }
}
impl std::error::Error for InvalidMutationRow {}

/// One bounded mutant description taken from a `mutants.out` list file
/// (`caught.txt`, `missed.txt`, `timeout.txt`, `unviable.txt`). The bytes are
/// guest-controlled, so construction enforces printable ASCII and the row bound
/// before anything downstream can see them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MutationMutantRow {
    name: String,
    class: MutationOutcomeClass,
}

impl MutationMutantRow {
    pub fn new(name: String, class: MutationOutcomeClass) -> Result<Self, InvalidMutationRow> {
        if name.is_empty()
            || name.len() > MUTATION_MAX_ROW_NAME
            || !name
                .bytes()
                .all(|byte| (0x20..=0x7e).contains(&byte) && byte != b'\x7f')
        {
            return Err(InvalidMutationRow);
        }
        Ok(Self { name, class })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn class(&self) -> MutationOutcomeClass {
        self.class
    }
}

/// Every count carries its denominator explicitly. `generated` comes from the
/// separate listing pass, `tested` and the per-class counts come from
/// `outcomes.json`, and `viable` is derived, never reported by the tool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MutationCounts {
    /// Mutants the tool generated for this selection (listing pass denominator).
    pub generated: u32,
    /// Mutant scenarios with an outcome row (excludes the baseline scenario).
    pub tested: u32,
    pub caught: u32,
    pub missed: u32,
    pub timeout: u32,
    pub unviable: u32,
    /// Mutant scenarios whose summary was `Success`/`Failure`. These are not a
    /// mutation verdict and never credit a clean result.
    pub other: u32,
}

impl MutationCounts {
    /// Mutants that actually built and ran: the denominator for caught/missed.
    pub fn viable(&self) -> u32 {
        self.tested
            .saturating_sub(self.unviable)
            .saturating_sub(self.timeout)
            .saturating_sub(self.other)
    }

    /// Structural invariants that must hold before any verdict is derived.
    pub fn consistent(&self) -> bool {
        let accounted = u64::from(self.caught)
            + u64::from(self.missed)
            + u64::from(self.timeout)
            + u64::from(self.unviable)
            + u64::from(self.other);
        accounted == u64::from(self.tested) && self.tested <= self.generated
    }

    /// A clean mutation result requires a complete, consistent set in which
    /// every viable mutant was caught and nothing was left untested.
    pub fn clean(&self) -> bool {
        self.consistent()
            && self.generated > 0
            && self.tested == self.generated
            && self.missed == 0
            && self.timeout == 0
            && self.unviable == 0
            && self.other == 0
            && self.viable() > 0
            && self.caught == self.viable()
    }
}

/// Outcome of the mandatory `--baseline run` scenario. A failing baseline is a
/// valid tool outcome that carries baseline evidence and no mutation verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationBaseline {
    Passed,
    Failed,
    /// No baseline scenario appeared in `outcomes.json`. Never treated as pass.
    Missing,
}

/// Guest-side identity recorded in `mutants.out/lock.json`. The values are
/// asserted to be the sandbox's own; anything host shaped is redacted before it
/// can reach a response or an artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationGuestIdentity {
    /// Username and hostname were the expected guest values.
    Guest,
    /// Something host shaped appeared; the values were dropped, not reported.
    Redacted,
    /// `lock.json` was absent or unparsable.
    Unavailable,
}

/// Classification of cargo-mutants 27.1.0 exit codes observed against the
/// approved M3 image. `Uncalibrated` is the fail-closed catch-all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationExit {
    /// 0: every viable mutant that was tested was caught.
    Success,
    /// 1: usage error (bad arguments). Always a product defect, never a verdict.
    Usage,
    /// 2: some mutants were not covered by tests.
    Missed,
    /// 3: some tests timed out.
    Timeout,
    /// 4: the baseline tests already fail or hang; no mutations were tested.
    BaselineFailed,
    /// 5/6: `--in-diff` errors. This product never passes `--in-diff`.
    Diff,
    /// 70: internal cargo-mutants error.
    Internal,
    /// Any other code.
    Uncalibrated,
}

impl MutationExit {
    /// Pinned by the Docker cases recorded in the M3-05 calibration receipt.
    /// Structured `mutants.out` evidence remains the authoritative oracle.
    pub const CALIBRATED: bool = true;

    pub fn classify(code: i32) -> Self {
        match code {
            0 => Self::Success,
            1 => Self::Usage,
            2 => Self::Missed,
            3 => Self::Timeout,
            4 => Self::BaselineFailed,
            5 | 6 => Self::Diff,
            70 => Self::Internal,
            _ => Self::Uncalibrated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_and_selection_reject_out_of_range_and_open_arguments() {
        for max_mutants in [0, MUTATION_MAX_MUTANTS + 1, u32::MAX] {
            assert!(
                MutationTestCommandOptions::try_from(MutationTestSelection {
                    max_mutants,
                    ..Default::default()
                })
                .is_err(),
                "{max_mutants}"
            );
        }
        for mutant_timeout_seconds in [0, MUTATION_MAX_MUTANT_TIMEOUT_SECONDS + 1, u64::MAX] {
            assert!(
                MutationTestCommandOptions::try_from(MutationTestSelection {
                    mutant_timeout_seconds,
                    ..Default::default()
                })
                .is_err(),
                "{mutant_timeout_seconds}"
            );
        }
        for selection in [
            MutationTestSelection {
                package: Some("--workspace".into()),
                ..Default::default()
            },
            MutationTestSelection {
                features: vec!["std".into()],
                all_features: true,
                ..Default::default()
            },
            MutationTestSelection {
                target: Some("x86_64-unknown-linux-gnu".into()),
                ..Default::default()
            },
        ] {
            assert!(MutationTestCommandOptions::try_from(selection).is_err());
        }
    }

    #[test]
    fn defaults_and_derived_build_timeout_are_closed() -> Result<(), InvalidCheckOptions> {
        let options = MutationTestCommandOptions::try_from(MutationTestSelection::default())?;
        assert_eq!(options.max_mutants(), MUTATION_DEFAULT_MAX_MUTANTS);
        assert_eq!(
            options.mutant_timeout_seconds(),
            MUTATION_DEFAULT_MUTANT_TIMEOUT_SECONDS
        );
        assert_eq!(options.build_timeout_seconds(), 300);
        for (timeout, expected) in [(1, 60), (12, 60), (13, 65), (60, 300)] {
            let options = MutationTestCommandOptions::try_from(MutationTestSelection {
                mutant_timeout_seconds: timeout,
                ..Default::default()
            })?;
            assert_eq!(options.build_timeout_seconds(), expected, "{timeout}");
            assert!(
                (MUTATION_MIN_BUILD_TIMEOUT_SECONDS..=MUTATION_MAX_BUILD_TIMEOUT_SECONDS)
                    .contains(&options.build_timeout_seconds())
            );
        }
        let selected = MutationTestCommandOptions::try_from(MutationTestSelection {
            package: Some("member".into()),
            features: vec!["std".into(), "member/extra".into()],
            all_features: false,
            no_default_features: true,
            target: Some("aarch64-unknown-linux-gnu".into()),
            max_mutants: 7,
            mutant_timeout_seconds: 5,
        })?;
        assert_eq!(selected.package(), Some("member"));
        assert_eq!(selected.features(), &["member/extra", "std"]);
        assert!(selected.no_default_features());
        assert_eq!(selected.target(), Some("aarch64-unknown-linux-gnu"));
        assert_eq!(selected.max_mutants(), 7);
        Ok(())
    }

    #[test]
    fn serde_cannot_bypass_validation_or_add_free_flags() -> Result<(), serde_json::Error> {
        for json in [
            r#"{"max_mutants":0}"#,
            r#"{"max_mutants":101}"#,
            r#"{"mutant_timeout_seconds":0}"#,
            r#"{"mutant_timeout_seconds":61}"#,
            r#"{"shard":"1/4"}"#,
            r#"{"in_place":true}"#,
            r#"{"baseline":"skip"}"#,
            r#"{"cargo_args":["--offline"]}"#,
            r#"{"output":"/tmp/x"}"#,
        ] {
            assert!(
                serde_json::from_str::<MutationTestCommandOptions>(json).is_err(),
                "{json}"
            );
        }
        let options: MutationTestCommandOptions =
            serde_json::from_str(r#"{"max_mutants":10,"mutant_timeout_seconds":30}"#)?;
        let encoded = serde_json::to_string(&options)?;
        assert_eq!(
            serde_json::from_str::<MutationTestCommandOptions>(&encoded)?,
            options
        );
        Ok(())
    }

    #[test]
    fn outcome_vocabulary_is_closed_and_only_caught_credits_clean() {
        for (text, class) in [
            ("CaughtMutant", MutationOutcomeClass::Caught),
            ("MissedMutant", MutationOutcomeClass::Missed),
            ("Timeout", MutationOutcomeClass::Timeout),
            ("Unviable", MutationOutcomeClass::Unviable),
            ("Success", MutationOutcomeClass::Success),
            ("Failure", MutationOutcomeClass::Failure),
        ] {
            assert_eq!(MutationOutcomeClass::parse(text), Some(class));
            assert_eq!(class.credits_clean(), class == MutationOutcomeClass::Caught);
        }
        for text in [
            "caught",
            "caughtmutant",
            "CAUGHTMUTANT",
            "Caught",
            "CaughtMutant ",
            "",
            "Killed",
        ] {
            assert_eq!(MutationOutcomeClass::parse(text), None, "{text}");
        }
    }

    #[test]
    fn counts_expose_denominators_and_never_call_an_incomplete_set_clean() {
        let clean = MutationCounts {
            generated: 4,
            tested: 4,
            caught: 4,
            ..Default::default()
        };
        assert!(clean.consistent());
        assert_eq!(clean.viable(), 4);
        assert!(clean.clean());
        for counts in [
            // A missed mutant.
            MutationCounts {
                generated: 4,
                tested: 4,
                caught: 3,
                missed: 1,
                ..Default::default()
            },
            // A timeout.
            MutationCounts {
                generated: 4,
                tested: 4,
                caught: 3,
                timeout: 1,
                ..Default::default()
            },
            // An unviable mutant.
            MutationCounts {
                generated: 4,
                tested: 4,
                caught: 3,
                unviable: 1,
                ..Default::default()
            },
            // An unclassified mutant scenario.
            MutationCounts {
                generated: 4,
                tested: 4,
                caught: 3,
                other: 1,
                ..Default::default()
            },
            // Fewer mutants tested than generated: an incomplete run.
            MutationCounts {
                generated: 4,
                tested: 3,
                caught: 3,
                ..Default::default()
            },
            // Nothing generated at all.
            MutationCounts::default(),
            // Every mutant unviable: nothing was actually exercised.
            MutationCounts {
                generated: 2,
                tested: 2,
                unviable: 2,
                ..Default::default()
            },
        ] {
            assert!(!counts.clean(), "{counts:?}");
        }
        // Class counts that do not add up to the tested denominator.
        let inconsistent = MutationCounts {
            generated: 4,
            tested: 4,
            caught: 2,
            ..Default::default()
        };
        assert!(!inconsistent.consistent());
        assert!(!inconsistent.clean());
        // More scenarios than were generated cannot be trusted either.
        let overrun = MutationCounts {
            generated: 1,
            tested: 2,
            caught: 2,
            ..Default::default()
        };
        assert!(!overrun.consistent());
    }

    #[test]
    fn mutant_rows_are_bounded_printable_ascii() {
        assert!(
            MutationMutantRow::new(
                "src/lib.rs:12:5: replace unchecked_value -> u8 with 0".into(),
                MutationOutcomeClass::Missed,
            )
            .is_ok()
        );
        for name in [
            String::new(),
            "a".repeat(MUTATION_MAX_ROW_NAME + 1),
            "line\nbreak".into(),
            "bell\u{7}".into(),
            "delete\u{7f}".into(),
            "unicode é".into(),
        ] {
            assert!(
                MutationMutantRow::new(name.clone(), MutationOutcomeClass::Caught).is_err(),
                "{name:?}"
            );
        }
    }

    #[test]
    fn exit_classification_matches_calibrated_guest_observations() {
        assert_eq!(MutationExit::classify(0), MutationExit::Success);
        assert_eq!(MutationExit::classify(1), MutationExit::Usage);
        assert_eq!(MutationExit::classify(2), MutationExit::Missed);
        assert_eq!(MutationExit::classify(3), MutationExit::Timeout);
        assert_eq!(MutationExit::classify(4), MutationExit::BaselineFailed);
        assert_eq!(MutationExit::classify(5), MutationExit::Diff);
        assert_eq!(MutationExit::classify(6), MutationExit::Diff);
        assert_eq!(MutationExit::classify(70), MutationExit::Internal);
        for other in [7, 69, 71, 100, 101, -1, i32::MAX, i32::MIN] {
            assert_eq!(
                MutationExit::classify(other),
                MutationExit::Uncalibrated,
                "{other}"
            );
        }
        const { assert!(MutationExit::CALIBRATED) };
    }
}
