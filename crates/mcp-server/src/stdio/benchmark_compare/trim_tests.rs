use super::tests::{TestResult, comparison_row, input};
use super::*;

const MAX_RESULT_BYTES: usize = 512 * 1024;
/// Sized so the complete response is just past the wire budget: the trimming
/// strategy is exercised, and the test does not re-encode a megabyte hundreds
/// of times to prove it.
const WIDE_COMPARISONS: usize = 300;
const WIDE_NAMES: usize = 40;

/// A report whose keys are all at the dataset's 512-byte text ceiling, so the
/// complete response is larger than the wire budget while every value in it
/// stays inside the published contract.
fn wide_report() -> ComparisonReport {
    let filler = "k".repeat(508);
    let rows: Vec<BenchmarkComparison> = (0..WIDE_COMPARISONS)
        .map(|index| {
            comparison_row(
                &format!("{filler}{index:04}"),
                if index % 4 == 0 {
                    ComparisonVerdict::Regression
                } else {
                    ComparisonVerdict::Inconclusive
                },
                0.01 * (index % 17) as f64,
            )
        })
        .collect();
    ComparisonReport {
        method: ComparisonMethod::frozen(u32::try_from(rows.len()).unwrap_or(u32::MAX)),
        compared: rows.len(),
        comparisons: rows,
        baseline_only: (0..WIDE_NAMES)
            .map(|index| format!("b{filler}{index:03}"))
            .collect(),
        candidate_only: (0..WIDE_NAMES)
            .map(|index| format!("c{filler}{index:03}"))
            .collect(),
    }
}

#[test]
fn encode_result_trims_names_then_the_lowest_ranked_rows_into_the_wire_budget() -> TestResult {
    let tool = ComparisonTool::new()?;
    let input = input()?;
    let untrimmed = tool.contract.encode(Output {
        outcome: Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(Data {
                project_ref: input.project_ref.to_string(),
                semantics: "observed_difference_between_two_measurements_without_attribution",
                baseline_artifact_id: input.baseline_artifact_id.to_string(),
                candidate_artifact_id: input.candidate_artifact_id.to_string(),
                report: report(CompareOutcome::Report(Box::new(wide_report()))),
            }),
        },
        summary: "Observed difference between two measurements under the frozen method",
        duration_ms: 7,
    })?;
    assert!(serde_json::to_vec(&untrimmed)?.len() > MAX_RESULT_BYTES);

    let encoded =
        tool.encode_result(&input, CompareOutcome::Report(Box::new(wide_report())), 7)?;
    let wire = serde_json::to_vec(&encoded)?;
    assert!(wire.len() <= MAX_RESULT_BYTES, "{}", wire.len());
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["error_code"], "EVIDENCE_INCOMPLETE");
    let report = &value["data"]["report"];
    assert_eq!(report["complete"], false);
    // The comparison itself is still counted in full; the omission is declared.
    assert_eq!(report["compared"], WIDE_COMPARISONS as u64);
    let candidate_omitted = report["candidate_only_omitted"]
        .as_u64()
        .ok_or("candidate_only_omitted")?;
    assert!(candidate_omitted > 0);
    let kept_candidates = report["candidate_only"]
        .as_array()
        .map(Vec::len)
        .ok_or("candidate_only")? as u64;
    assert_eq!(kept_candidates + candidate_omitted, WIDE_NAMES as u64);
    // Names carry no measurement, so they leave before any comparison row.
    let kept_rows = report["comparisons"]
        .as_array()
        .map(Vec::len)
        .ok_or("comparisons")? as u64;
    let rows_omitted = report["comparisons_omitted"]
        .as_u64()
        .ok_or("comparisons_omitted")?;
    assert_eq!(kept_rows + rows_omitted, WIDE_COMPARISONS as u64);
    assert!(rows_omitted == 0 || kept_candidates == 0);
    // Any comparison row that survived is still ranked, material first.
    let verdicts: Vec<&str> = report["comparisons"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["verdict"].as_str())
        .collect();
    let first_inconclusive = verdicts
        .iter()
        .position(|verdict| *verdict == "inconclusive")
        .unwrap_or(verdicts.len());
    assert!(
        verdicts[..first_inconclusive]
            .iter()
            .all(|verdict| *verdict != "inconclusive")
    );
    assert_eq!(report["method"]["family_size"], WIDE_COMPARISONS as u64);
    Ok(())
}

#[test]
fn a_small_report_stays_passed_and_untrimmed() -> TestResult {
    let tool = ComparisonTool::new()?;
    let input = input()?;
    let report = super::tests::comparison_report(vec![comparison_row(
        "bench/one",
        ComparisonVerdict::NoMaterialChange,
        0.001,
    )]);
    let encoded = tool.encode_result(&input, CompareOutcome::Report(Box::new(report)), 7)?;
    assert!(serde_json::to_vec(&encoded)?.len() <= MAX_RESULT_BYTES);
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "passed");
    assert_eq!(value["data"]["report"]["complete"], true);
    assert_eq!(value["data"]["report"]["comparisons_omitted"], 0);
    assert_eq!(value["data"]["report"]["method"]["multiplicity"], "none");
    Ok(())
}
