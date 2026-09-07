use rust_engineering_domain::RustCommand;
use rust_engineering_domain::nextest::{
    NextestCommandOptions, NextestExit, NextestOutcomeCounts, NextestSelection, NextestTestOutcome,
    NextestTestRow,
};

#[test]
fn test_nextest_variant_is_distinguishable_from_every_other_rust_command()
-> Result<(), Box<dyn std::error::Error>> {
    let options = NextestCommandOptions::try_from(NextestSelection::default())?;
    let encoded = serde_json::to_string(&RustCommand::TestNextest(options))?;
    assert!(encoded.starts_with(r#"{"test_nextest":"#));
    assert_ne!(encoded, serde_json::to_string(&RustCommand::Check)?);
    Ok(())
}

#[test]
fn nextest_selection_serde_rejects_unknown_fields_and_wrong_types() {
    for json in [
        r#"{"args":["--ignored"]}"#,
        r#"{"config_file":"/tmp/x"}"#,
        r#"{"profile":"other"}"#,
        r#"{"retries":3}"#,
        r#"{"retries":-1}"#,
    ] {
        assert!(
            serde_json::from_str::<NextestCommandOptions>(json).is_err(),
            "{json}"
        );
    }
}

#[test]
fn outcome_counts_and_bounded_rows_round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let counts = NextestOutcomeCounts {
        passed: 3,
        failed: 0,
        skipped: 1,
        retried: 0,
        flaky: 0,
        leaky: 0,
        timed_out: 0,
    };
    assert_eq!(counts.total(), 4);
    let row = NextestTestRow::new(
        "pkg::case".into(),
        "pkg::suite".into(),
        NextestTestOutcome::Passed,
        7,
    )?;
    assert_eq!(row.time_ms(), 7);
    assert!(
        NextestTestRow::new(String::new(), String::new(), NextestTestOutcome::Passed, 0).is_err()
    );
    Ok(())
}

#[test]
fn exit_classification_is_pinned_to_the_approved_runtime() {
    assert_eq!(NextestExit::classify(0), NextestExit::Success);
    assert_eq!(NextestExit::classify(4), NextestExit::NoTests);
    assert_eq!(NextestExit::classify(100), NextestExit::TestFailure);
    assert_eq!(NextestExit::classify(101), NextestExit::RunnerFailure);
    assert_eq!(NextestExit::classify(104), NextestExit::Uncalibrated);
    assert_eq!(NextestExit::classify(7), NextestExit::Uncalibrated);
    const { assert!(NextestExit::CALIBRATED) };
}
