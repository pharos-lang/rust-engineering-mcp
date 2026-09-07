use rust_engineering_domain::RustCommand;
use rust_engineering_domain::mutation_test::{
    MUTATION_MAX_MUTANT_TIMEOUT_SECONDS, MUTATION_MAX_MUTANTS, MutationCounts, MutationExit,
    MutationMutantRow, MutationOutcomeClass, MutationTestCommandOptions, MutationTestSelection,
};

#[test]
fn mutation_commands_are_distinguishable_from_every_other_rust_command()
-> Result<(), Box<dyn std::error::Error>> {
    let options = MutationTestCommandOptions::try_from(MutationTestSelection::default())?;
    let encoded = serde_json::to_string(&RustCommand::MutationTest(options))?;
    assert!(encoded.starts_with(r#"{"mutation_test":"#));
    assert_ne!(encoded, serde_json::to_string(&RustCommand::Check)?);
    assert_ne!(
        serde_json::to_string(&RustCommand::MutantsVersion)?,
        serde_json::to_string(&RustCommand::CargoVersion)?
    );
    // A different selection is a different command identity, so a cached
    // fingerprint can never be reused across selections.
    let narrower = MutationTestCommandOptions::try_from(MutationTestSelection {
        max_mutants: 5,
        ..Default::default()
    })?;
    assert_ne!(
        encoded,
        serde_json::to_string(&RustCommand::MutationTest(narrower))?
    );
    Ok(())
}

#[test]
fn selection_serde_rejects_unknown_fields_wrong_types_and_out_of_range_budgets() {
    for json in [
        r#"{"shard":"1/4"}"#,
        r#"{"in_place":true}"#,
        r#"{"baseline":"skip"}"#,
        r#"{"output":"/tmp/x"}"#,
        r#"{"cargo_args":["--offline"]}"#,
        r#"{"max_mutants":"10"}"#,
        r#"{"max_mutants":-1}"#,
        r#"{"max_mutants":0}"#,
        r#"{"max_mutants":101}"#,
        r#"{"mutant_timeout_seconds":0}"#,
        r#"{"mutant_timeout_seconds":61}"#,
        r#"{"target":"x86_64-unknown-linux-gnu"}"#,
        r#"{"features":["std"],"all_features":true}"#,
    ] {
        assert!(
            serde_json::from_str::<MutationTestCommandOptions>(json).is_err(),
            "{json}"
        );
    }
}

#[test]
fn budgets_stay_inside_the_roadmap_bounds() -> Result<(), Box<dyn std::error::Error>> {
    let options = MutationTestCommandOptions::try_from(MutationTestSelection {
        max_mutants: MUTATION_MAX_MUTANTS,
        mutant_timeout_seconds: MUTATION_MAX_MUTANT_TIMEOUT_SECONDS,
        ..Default::default()
    })?;
    assert_eq!(options.max_mutants(), 100);
    assert_eq!(options.mutant_timeout_seconds(), 60);
    assert!(options.build_timeout_seconds() <= 300);
    Ok(())
}

#[test]
fn counts_and_rows_carry_denominators_and_bounds() -> Result<(), Box<dyn std::error::Error>> {
    let counts = MutationCounts {
        generated: 5,
        tested: 5,
        caught: 3,
        missed: 1,
        unviable: 1,
        ..Default::default()
    };
    assert!(counts.consistent());
    assert_eq!(counts.viable(), 4);
    assert!(!counts.clean());
    let row = MutationMutantRow::new(
        "src/lib.rs:9:5: replace unchecked_value -> u8 with 0".into(),
        MutationOutcomeClass::Missed,
    )?;
    assert_eq!(row.class(), MutationOutcomeClass::Missed);
    assert!(!row.class().credits_clean());
    assert!(MutationMutantRow::new("a\nb".into(), MutationOutcomeClass::Caught).is_err());
    Ok(())
}

#[test]
fn exit_classification_matches_the_calibrated_guest_binary() {
    assert_eq!(MutationExit::classify(0), MutationExit::Success);
    assert_eq!(MutationExit::classify(2), MutationExit::Missed);
    assert_eq!(MutationExit::classify(3), MutationExit::Timeout);
    assert_eq!(MutationExit::classify(4), MutationExit::BaselineFailed);
    assert_eq!(MutationExit::classify(70), MutationExit::Internal);
    assert_eq!(MutationExit::classify(104), MutationExit::Uncalibrated);
    // M3-05 still derives every verdict from mutants.out rather than trusting
    // an exit code alone; the numeric map is now pinned by Docker evidence.
    const { assert!(MutationExit::CALIBRATED) };
}
