use super::*;
use rust_engineering_application::mutation_test::MutationArtifactStreams;
use rust_engineering_domain::mutation_test::{MutationCounts, MutationMutantRow};
use rust_engineering_domain::{ExecutionFingerprint, ExecutionTermination, RuntimeIdentity};
use sha2::{Digest, Sha256};

fn observation(
    counts: MutationCounts,
    baseline: MutationBaseline,
) -> Result<MutationTestObservation, Box<dyn std::error::Error>> {
    let execution_fingerprint =
        format!("sha256:{}", "3".repeat(64)).parse::<ExecutionFingerprint>()?;
    Ok(MutationTestObservation {
        options: MutationTestCommandOptions::try_from(MutationTestSelection::default())?,
        completeness: MutationCompleteness::Complete,
        validation_complete: true,
        baseline,
        counts,
        mutants: Vec::new(),
        mutants_omitted: 0,
        cap_exceeded: false,
        mutants_version: "27.1.0".to_owned(),
        guest_identity: MutationGuestIdentity::Guest,
        termination: ExecutionTermination::Exited,
        exit_code: Some(0),
        runtime: RuntimeIdentity {
            platform: "linux-aarch64".to_owned(),
            image_id: format!("sha256:{}", "1".repeat(64)),
            configuration_fingerprint: format!("sha256:{}", "2".repeat(64)).parse()?,
            execution_fingerprint: execution_fingerprint.clone(),
            rust_version: "rustc 1.98.1".to_owned(),
            cargo_version: "cargo 1.98.1".to_owned(),
            declared_toolchain: None,
        },
        execution_fingerprint,
        artifacts: MutationArtifactStreams::default(),
    })
}

fn encode(
    tool: &MutationTestTool,
    observation: MutationTestObservation,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let project: ProjectRef = "prj_00000000000000000000000000000001".parse()?;
    let result = MutationTestTaskResult::new(observation, Vec::new(), 1)?;
    Ok(serde_json::to_value(
        tool.encode_task_result(&project, result)?,
    )?)
}

fn caught(count: u32) -> MutationCounts {
    MutationCounts {
        generated: count,
        tested: count,
        caught: count,
        ..Default::default()
    }
}

#[test]
fn schema_is_closed_and_stable() -> Result<(), Box<dyn std::error::Error>> {
    let tool = MutationTestTool::new()?;
    let definition = serde_json::to_value(&tool.definition)?;
    assert_eq!(definition["name"], NAME);
    assert_eq!(definition["inputSchema"]["additionalProperties"], false);
    assert_eq!(definition["outputSchema"]["unevaluatedProperties"], false);
    assert_eq!(
        definition["inputSchema"]["$defs"]["ExecutionModeDto"]["enum"],
        serde_json::json!(["auto", "task", "synchronous"])
    );
    assert_eq!(
        definition["inputSchema"]["properties"]["max_mutants"]["default"],
        100
    );
    assert_eq!(
        definition["inputSchema"]["properties"]["max_mutants"]["maximum"],
        100
    );
    assert_eq!(
        definition["inputSchema"]["properties"]["mutant_timeout_seconds"]["default"],
        60
    );
    assert_eq!(
        definition["inputSchema"]["properties"]["mutant_timeout_seconds"]["maximum"],
        60
    );
    assert_eq!(
        definition,
        serde_json::from_str::<serde_json::Value>(include_str!(
            "../../../tests/snapshots/mutation-test-tool.json"
        ))?
    );
    let digest: [u8; 32] = Sha256::digest(serde_json::to_vec(&definition)?).into();
    assert_eq!(
        hex(&digest),
        "52d7a54b99df02cf1de9436fb7f1f370091d6593d0f1a37836d415128f0b35d3"
    );
    Ok(())
}

#[test]
fn semantic_validation_rejects_contradictory_or_open_arguments()
-> Result<(), Box<dyn std::error::Error>> {
    let tool = MutationTestTool::new()?;
    for value in [
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","all_features":true,"features":["std"]}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","max_mutants":0}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","max_mutants":101}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","mutant_timeout_seconds":0}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","mutant_timeout_seconds":61}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","target":"x86_64-unknown-linux-gnu"}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","shard":"1/4"}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","in_place":true}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","baseline":"skip"}),
        serde_json::json!({"project_ref":"prj_00000000000000000000000000000001","execution_mode":"background"}),
    ] {
        let arguments = serde_json::from_value(value.clone())?;
        if let Ok(input) = tool.contract.decode(Some(arguments)) {
            assert!(input.options().is_err(), "{value}");
        }
    }
    let arguments = serde_json::from_value(serde_json::json!({
        "project_ref":"prj_00000000000000000000000000000001",
        "package":"member",
        "no_default_features":true,
        "max_mutants":10,
        "mutant_timeout_seconds":30,
        "execution_mode":"task"
    }))?;
    let input = tool.contract.decode(Some(arguments))?;
    let options = input.options()?;
    assert_eq!(options.package(), Some("member"));
    assert_eq!(options.max_mutants(), 10);
    assert_eq!(options.mutant_timeout_seconds(), 30);
    assert_eq!(input.mode(), ExecutionMode::Task);
    Ok(())
}

#[test]
fn no_mutation_selection_is_synchronously_qualified() -> Result<(), Box<dyn std::error::Error>> {
    // The smallest possible job still needs one build plus one bounded run,
    // which the ADR-060 derivation floors at the 300-second default.
    let smallest = MutationTestCommandOptions::try_from(MutationTestSelection {
        max_mutants: 1,
        mutant_timeout_seconds: 1,
        ..Default::default()
    })?;
    assert_eq!(total_budget_seconds(&smallest), 300);
    assert!(!synchronous_qualified(&smallest));
    let largest = MutationTestCommandOptions::try_from(MutationTestSelection::default())?;
    assert_eq!(total_budget_seconds(&largest), 3_600);
    assert!(!synchronous_qualified(&largest));
    // Auto therefore always yields the structured remediation, and explicit
    // task mode is rejected while Tasks advertisement is off.
    assert_eq!(
        select_execution_mode(ExecutionMode::Auto, false, synchronous_qualified(&largest)),
        Ok(ExecutionSelection::TasksRequired)
    );
    assert!(
        select_execution_mode(
            ExecutionMode::Synchronous,
            false,
            synchronous_qualified(&largest)
        )
        .is_err()
    );
    assert!(select_execution_mode(ExecutionMode::Task, false, false).is_err());
    let tool = MutationTestTool::new()?;
    let blocked = serde_json::to_value(tool.tasks_required()?)?;
    // A blocked remediation is an error result, not a silent success.
    assert_eq!(blocked["isError"], true);
    assert_eq!(blocked["structuredContent"]["status"], "blocked");
    assert_eq!(blocked["structuredContent"]["error_code"], "TASKS_REQUIRED");
    assert!(blocked["structuredContent"]["data"].is_null());
    Ok(())
}

#[test]
fn a_missed_mutant_fails_and_names_the_surviving_function() -> Result<(), Box<dyn std::error::Error>>
{
    let tool = MutationTestTool::new()?;
    let mut observation = observation(
        MutationCounts {
            generated: 4,
            tested: 4,
            caught: 3,
            missed: 1,
            ..Default::default()
        },
        MutationBaseline::Passed,
    )?;
    observation.mutants = vec![MutationMutantRow::new(
        "src/lib.rs:7:5: replace unchecked_value -> u8 with 0".into(),
        MutationOutcomeClass::Missed,
    )?];
    let failed = encode(&tool, observation)?;
    // A tool-level failure is a valid result, not a protocol error.
    assert_eq!(failed["isError"], false);
    assert_eq!(failed["structuredContent"]["status"], "failed");
    assert_eq!(failed["structuredContent"]["data"]["counts"]["missed"], 1);
    assert_eq!(failed["structuredContent"]["data"]["counts"]["viable"], 4);
    assert_eq!(
        failed["structuredContent"]["data"]["mutants"][0]["name"],
        "src/lib.rs:7:5: replace unchecked_value -> u8 with 0"
    );
    assert_eq!(
        failed["structuredContent"]["data"]["mutants"][0]["outcome"],
        "missed"
    );
    Ok(())
}

#[test]
fn a_failing_baseline_is_a_failed_outcome_with_baseline_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let tool = MutationTestTool::new()?;
    let mut observation = observation(MutationCounts::default(), MutationBaseline::Failed)?;
    observation.exit_code = Some(4);
    observation.validation_complete = false;
    let failed = encode(&tool, observation)?;
    assert_eq!(failed["isError"], false);
    assert_eq!(failed["structuredContent"]["status"], "failed");
    assert_eq!(failed["structuredContent"]["data"]["baseline"], "failed");
    assert_eq!(failed["structuredContent"]["data"]["counts"]["tested"], 0);
    assert_eq!(failed["structuredContent"]["data"]["exit_code"], 4);
    Ok(())
}

#[test]
fn only_a_complete_all_caught_report_passes() -> Result<(), Box<dyn std::error::Error>> {
    let tool = MutationTestTool::new()?;
    let passed = encode(&tool, observation(caught(3), MutationBaseline::Passed)?)?;
    assert_eq!(passed["isError"], false);
    assert_eq!(passed["structuredContent"]["status"], "passed");
    assert_eq!(passed["structuredContent"]["data"]["counts"]["caught"], 3);
    assert_eq!(passed["structuredContent"]["data"]["counts"]["viable"], 3);
    assert_eq!(
        passed["structuredContent"]["data"]["counts"]["generated"],
        3
    );
    assert_eq!(passed["structuredContent"]["data"]["baseline"], "passed");
    assert_eq!(
        passed["structuredContent"]["data"]["guest_identity"],
        "guest"
    );
    Ok(())
}

#[test]
fn timeout_unviable_missing_baseline_and_partial_evidence_never_pass()
-> Result<(), Box<dyn std::error::Error>> {
    let tool = MutationTestTool::new()?;
    let timed_out_class = MutationCounts {
        generated: 3,
        tested: 3,
        caught: 2,
        timeout: 1,
        ..Default::default()
    };
    let unviable = MutationCounts {
        generated: 3,
        tested: 3,
        caught: 2,
        unviable: 1,
        ..Default::default()
    };
    let untested = MutationCounts {
        generated: 4,
        tested: 3,
        caught: 3,
        ..Default::default()
    };
    for counts in [timed_out_class, unviable, untested] {
        let value = encode(&tool, observation(counts, MutationBaseline::Passed)?)?;
        assert_ne!(value["structuredContent"]["status"], "passed", "{counts:?}");
        assert_eq!(value["structuredContent"]["status"], "blocked");
    }
    // A missing baseline scenario is never read as a passing one.
    let missing = encode(&tool, observation(caught(3), MutationBaseline::Missing)?)?;
    assert_eq!(missing["structuredContent"]["status"], "blocked");
    assert_eq!(missing["structuredContent"]["data"]["baseline"], "missing");
    // Partial evidence, a redacted guest identity and an
    // unpublished bundle each block a pass on their own.
    for mutate in [
        (|observation: &mut MutationTestObservation| {
            observation.completeness = MutationCompleteness::Partial;
            observation.validation_complete = false;
        }) as fn(&mut MutationTestObservation),
        |observation| {
            observation.guest_identity = MutationGuestIdentity::Redacted;
        },
        |observation| {
            observation.artifacts.bundle_unavailable = true;
            observation.validation_complete = false;
        },
        |observation| {
            observation.mutants_omitted = 1;
            observation.validation_complete = false;
        },
    ] {
        let mut value = observation(caught(3), MutationBaseline::Passed)?;
        mutate(&mut value);
        let encoded = encode(&tool, value)?;
        assert_ne!(encoded["structuredContent"]["status"], "passed");
    }
    Ok(())
}

#[test]
fn an_oversized_generated_set_is_refused_before_anything_is_built()
-> Result<(), Box<dyn std::error::Error>> {
    let tool = MutationTestTool::new()?;
    let mut value = observation(MutationCounts::default(), MutationBaseline::Missing)?;
    value.cap_exceeded = true;
    value.validation_complete = false;
    value.completeness = MutationCompleteness::Unavailable;
    let encoded = encode(&tool, value)?;
    assert_eq!(encoded["structuredContent"]["status"], "blocked");
    assert_eq!(
        encoded["structuredContent"]["error_code"],
        "MUTANT_LIMIT_EXCEEDED"
    );
    assert_eq!(
        encoded["structuredContent"]["data"]["omissions"]["mutant_limit_exceeded"],
        true
    );
    Ok(())
}

#[test]
fn a_timed_out_run_is_blocked_and_keeps_its_partial_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let tool = MutationTestTool::new()?;
    let mut value = observation(MutationCounts::default(), MutationBaseline::Missing)?;
    value.termination = ExecutionTermination::TimedOut;
    value.exit_code = None;
    value.completeness = MutationCompleteness::Unavailable;
    value.validation_complete = false;
    let encoded = encode(&tool, value)?;
    assert_eq!(encoded["structuredContent"]["status"], "blocked");
    assert_eq!(
        encoded["structuredContent"]["error_code"],
        "COMMAND_TIMEOUT"
    );
    assert_eq!(
        encoded["structuredContent"]["data"]["termination"],
        "timed_out"
    );
    Ok(())
}
