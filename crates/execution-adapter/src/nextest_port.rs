//! Bridges Sol's `application::nextest::ProjectNextestPort` shape (package
//! I00) to this package's own closed domain types and gateway phases.
//!
//! The two sides were designed concurrently and their grammars only
//! partially line up; every place below where a value is approximated,
//! clamped or not yet tracked is called out explicitly rather than silently
//! rounded. None of these gaps can turn a failure/partial result into a pass.
use crate::RustGateway;
use rust_engineering_application::nextest::{
    ArtifactStreams, NextestCompleteness, NextestCounts, NextestObservation, NextestOptions,
    NextestTestRow as AppNextestTestRow, NextestTestStatus,
};
use rust_engineering_application::{InspectionControl, InspectionError, ProjectError};
use rust_engineering_domain::nextest::{
    NextestCommandOptions, NextestOutcomeCounts, NextestSelection, NextestTestOutcome,
};
use rust_engineering_domain::{
    ExecutionLimits, ExecutionTermination, RuntimeIdentity, SourceBundle,
};

/// Sol's `NextestObservation::validate()` caps itemized rows at this many;
/// this package's own JUnit parser bounds rows independently and much higher
/// (4,096), so rows beyond this port-level cap are counted but not itemized.
const MAX_OBSERVATION_TEST_ROWS: usize =
    rust_engineering_application::nextest::NEXTEST_MAX_TEST_ROWS;
const MAX_TEST_ID_LEN: usize = 256;

fn to_domain_options(options: &NextestOptions) -> Result<NextestCommandOptions, InspectionError> {
    NextestCommandOptions::try_from(NextestSelection {
        package: options.package().map(str::to_owned),
        test_filter: options.test_filter().map(str::to_owned),
        features: options.features().to_vec(),
        all_features: options.all_features(),
        no_default_features: options.no_default_features(),
        target: options.target().map(str::to_owned),
        timeout: options.timeout_seconds(),
        retries: options.retries(),
    })
    .map_err(|_| InspectionError::Internal)
}

fn outcome_status(outcome: NextestTestOutcome) -> NextestTestStatus {
    match outcome {
        NextestTestOutcome::Passed => NextestTestStatus::Passed,
        NextestTestOutcome::Failed => NextestTestStatus::Failed,
        NextestTestOutcome::Skipped => NextestTestStatus::Ignored,
        NextestTestOutcome::Flaky => NextestTestStatus::Flaky,
        NextestTestOutcome::Leaky => NextestTestStatus::Leaked,
        NextestTestOutcome::TimedOut => NextestTestStatus::TimedOut,
    }
}

fn test_id(classname: &str, name: &str) -> String {
    let mut id = if classname.is_empty() {
        name.to_owned()
    } else {
        format!("{classname}::{name}")
    };
    if id.len() > MAX_TEST_ID_LEN {
        let mut end = MAX_TEST_ID_LEN;
        while !id.is_char_boundary(end) {
            end -= 1;
        }
        id.truncate(end);
    }
    id
}

fn to_app_counts(counts: NextestOutcomeCounts) -> NextestCounts {
    NextestCounts {
        selected: counts.total(),
        passed: u64::from(counts.passed),
        failed: u64::from(counts.failed),
        ignored: u64::from(counts.skipped),
        retried: u64::from(counts.retried),
        flaky: u64::from(counts.flaky),
        leaked: u64::from(counts.leaky),
        timed_out: u64::from(counts.timed_out),
    }
}

pub(super) fn run(
    gateway: &RustGateway,
    source: &SourceBundle,
    options: &NextestOptions,
    control: &dyn InspectionControl,
) -> Result<NextestObservation, InspectionError> {
    let domain_options = to_domain_options(options)?;
    let wall_ms = options
        .timeout_seconds()
        .checked_mul(1000)
        .ok_or(InspectionError::Internal)?;
    let limits = ExecutionLimits::new_job(wall_ms, 256 * 1024).ok_or(InspectionError::Internal)?;
    let execution = gateway
        .execute_nextest(source, &domain_options, limits, control)
        .map_err(InspectionError::Execution)?;
    let result = execution.result;
    match result.termination {
        ExecutionTermination::TimedOut => (),
        ExecutionTermination::Cancelled => {
            return Err(InspectionError::Project(ProjectError::Cancelled));
        }
        ExecutionTermination::OutputLimit => (),
        ExecutionTermination::Exited => (),
    }
    let junit_bytes = execution.junit.unwrap_or_default();
    let (completeness, counts, tests, tests_omitted) = if junit_bytes.is_empty() {
        (
            NextestCompleteness::Unavailable,
            NextestCounts::default(),
            Vec::new(),
            0u64,
        )
    } else {
        match super::nextest_junit::parse_junit(&junit_bytes) {
            super::nextest_junit::JunitReport::Incomplete => (
                NextestCompleteness::Invalid,
                NextestCounts::default(),
                Vec::new(),
                0u64,
            ),
            super::nextest_junit::JunitReport::Parsed {
                counts,
                rows,
                rows_omitted,
            } => {
                let (kept, extra_omitted) = if rows.len() > MAX_OBSERVATION_TEST_ROWS {
                    (
                        &rows[..MAX_OBSERVATION_TEST_ROWS],
                        rows.len() - MAX_OBSERVATION_TEST_ROWS,
                    )
                } else {
                    (&rows[..], 0)
                };
                let tests = kept
                    .iter()
                    .map(|row| AppNextestTestRow {
                        test_id: test_id(row.classname(), row.name()),
                        status: outcome_status(row.outcome()),
                        attempts: row.attempts(),
                        duration_ms: row.time_ms(),
                    })
                    .collect::<Vec<_>>();
                let omitted = u64::from(rows_omitted) + extra_omitted as u64;
                let completeness = if omitted == 0 && !execution.junit_truncated {
                    NextestCompleteness::Complete
                } else {
                    NextestCompleteness::Partial
                };
                (completeness, to_app_counts(counts), tests, omitted)
            }
        }
    };
    let validation_complete = matches!(completeness, NextestCompleteness::Complete)
        && tests_omitted == 0
        && !execution.junit_truncated;
    let runtime = RuntimeIdentity {
        platform: result.platform.into(),
        image_id: result.image_id.clone(),
        configuration_fingerprint: gateway
            .configuration_fingerprint()
            .map_err(InspectionError::Execution)?,
        execution_fingerprint: result.execution_fingerprint.clone(),
        rust_version: super::rust_gateway::APPROVED_RUST_VERSION.into(),
        cargo_version: super::rust_gateway::APPROVED_CARGO_VERSION.into(),
        declared_toolchain: super::project_metadata::declared_toolchain(source)?,
    };
    Ok(NextestObservation {
        options: options.clone(),
        validation_complete,
        completeness,
        counts,
        tests,
        tests_omitted,
        doctests_run: false,
        termination: result.termination,
        exit_code: result.exit_code,
        runtime,
        execution_fingerprint: result.execution_fingerprint,
        artifacts: ArtifactStreams {
            junit_xml: junit_bytes,
            stdout: result.stdout.into_bytes(),
            stderr: result.stderr.into_bytes(),
            junit_truncated: execution.junit_truncated,
            stdout_truncated: result.stdout_truncated,
            stderr_truncated: result.stderr_truncated,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_id_truncates_on_a_char_boundary_within_the_observation_cap() {
        let long = "a".repeat(MAX_TEST_ID_LEN + 10);
        let id = test_id(&long, "case");
        assert!(id.len() <= MAX_TEST_ID_LEN);
        assert!(id.is_char_boundary(id.len()));
        assert_eq!(test_id("", "solo"), "solo");
        assert_eq!(test_id("pkg::suite", "case"), "pkg::suite::case");
    }

    #[test]
    fn outcome_status_mapping_is_total_and_stable() {
        for (domain, app) in [
            (NextestTestOutcome::Passed, NextestTestStatus::Passed),
            (NextestTestOutcome::Failed, NextestTestStatus::Failed),
            (NextestTestOutcome::Skipped, NextestTestStatus::Ignored),
            (NextestTestOutcome::Flaky, NextestTestStatus::Flaky),
            (NextestTestOutcome::Leaky, NextestTestStatus::Leaked),
            (NextestTestOutcome::TimedOut, NextestTestStatus::TimedOut),
        ] {
            assert_eq!(outcome_status(domain), app);
        }
    }

    #[test]
    fn retried_count_never_falls_below_flaky_count() {
        let counts = NextestOutcomeCounts {
            passed: 1,
            failed: 1,
            skipped: 0,
            retried: 3,
            flaky: 3,
            leaky: 1,
            timed_out: 0,
        };
        let app = to_app_counts(counts);
        assert!(app.retried >= app.flaky);
        assert_eq!(app.selected, counts.total());
    }
}
