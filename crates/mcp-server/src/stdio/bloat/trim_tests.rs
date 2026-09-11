use super::tests::{TestResult, fixtures};
use super::*;
use rust_engineering_domain::bloat::{BloatAttribution, BloatCrate, BloatFunction, BloatProfile};

/// The budget the tool actually enforces, not a copy of it: a divergence
/// between the two would make every assertion below test the wrong limit.
const MAX_RESULT_BYTES: usize = super::super::security_tool::MAX_RESULT_BYTES;
const _: () = assert!(MAX_RESULT_BYTES == RESPONSE_BUDGET_BYTES as usize);

/// Rows whose names sit near the DTO's 512-byte ceiling, so a few hundred of
/// them push the complete response past the wire budget while every value in
/// it stays valid against the published schema.
fn wide_functions(count: usize) -> Vec<BloatFunction> {
    let filler = "k".repeat(500);
    (0..count)
        .map(|index| BloatFunction {
            crate_name: format!("{filler}{index:05}"),
            name: format!("{filler}{index:05}"),
            size_bytes: 1_000_000 + index as u64,
        })
        .collect()
}

fn wide_crates(count: usize) -> Vec<BloatCrate> {
    let filler = "k".repeat(500);
    (0..count)
        .map(|index| BloatCrate {
            name: format!("{filler}{index:05}"),
            size_bytes: 1_000_000 + index as u64,
        })
        .collect()
}

#[test]
fn a_small_complete_report_stays_passed_and_untrimmed() -> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let observed = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(fixtures::attribution(Some(4_096))),
    )?;
    let published = fixtures::published(observed, ArtifactCompleteness::Complete, true)?;
    let encoded = tool.encode_result(&reference, published, 7)?;
    assert!(serde_json::to_vec(&encoded)?.len() <= MAX_RESULT_BYTES);
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "passed");
    assert_eq!(value["data"]["observation"]["analysis_validated"], true);
    let attribution = &value["data"]["observation"]["attribution"];
    assert_eq!(attribution["ranking_cap"]["functions_omitted"], 0);
    assert_eq!(attribution["ranking_cap"]["crates_omitted"], 0);
    assert_eq!(attribution["response_trim"]["functions_omitted"], 0);
    assert_eq!(attribution["response_trim"]["crates_omitted"], 0);
    Ok(())
}

#[test]
fn encode_result_trims_the_lowest_ranked_function_rows_before_touching_crates() -> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let attribution = BloatAttribution {
        estimated: true,
        reported_file_size_bytes: Some(4_096),
        text_section_size_bytes: Some(2_048),
        functions: wide_functions(700),
        crates: wide_crates(5),
        functions_omitted_by_row_cap: 0,
        crates_omitted_by_row_cap: 0,
    };
    let observed = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(attribution),
    )?;
    let published = fixtures::published(observed, ArtifactCompleteness::Complete, true)?;

    let artifacts = published
        .artifacts
        .iter()
        .map(|descriptor| artifact(&reference, descriptor))
        .collect::<Result<Vec<_>, _>>()?;
    let untrimmed = tool.contract.encode(Output {
        outcome: Outcome::Passed {
            error_code: (),
            error_message: (),
            data: Box::new(Data {
                project_ref: reference.to_string(),
                semantics: "measured_file_size_is_exact_attribution_rankings_are_estimated",
                observation: observation(&published.observation, true),
                artifacts,
            }),
        },
        summary: "Exact measured file size and cargo-bloat's estimated attribution",
        duration_ms: 7,
    })?;
    assert!(serde_json::to_vec(&untrimmed)?.len() > MAX_RESULT_BYTES);

    let encoded = tool.encode_result(&reference, published, 7)?;
    let wire = serde_json::to_vec(&encoded)?;
    assert!(wire.len() <= MAX_RESULT_BYTES, "{}", wire.len());
    let value = encoded.structured_content.ok_or("structured content")?;
    // ADR-079 §1: a response that had to shed rows to fit its budget is still a
    // validated measurement. It was `blocked`/`EVIDENCE_INCOMPLETE` before.
    assert_eq!(value["status"], "passed", "{}", value["error_code"]);
    assert_eq!(value["data"]["observation"]["analysis_validated"], true);

    let attribution_value = &value["data"]["observation"]["attribution"];
    // The budget acted; the product's ranking cap did not. A reader can tell
    // which limit cost them which rows, which is the whole point of §1.
    assert_eq!(attribution_value["ranking_cap"]["functions_omitted"], 0);
    assert_eq!(attribution_value["ranking_cap"]["crates_omitted"], 0);
    assert_eq!(
        attribution_value["response_trim"]["budget_bytes"],
        MAX_RESULT_BYTES as u64
    );
    let functions_omitted = attribution_value["response_trim"]["functions_omitted"]
        .as_u64()
        .ok_or("functions_omitted")?;
    let crates_omitted = attribution_value["response_trim"]["crates_omitted"]
        .as_u64()
        .ok_or("crates_omitted")?;
    assert!(functions_omitted > 0);
    assert_eq!(
        crates_omitted, 0,
        "crates must not be touched while a function row remains"
    );
    let kept_functions = attribution_value["functions"]
        .as_array()
        .map(Vec::len)
        .ok_or("functions")?;
    assert_eq!(kept_functions as u64 + functions_omitted, 700);
    let kept_crates = attribution_value["crates"]
        .as_array()
        .map(Vec::len)
        .ok_or("crates")?;
    assert_eq!(kept_crates, 5, "crates are untouched by this trim");

    // The survivors are the highest-ranked rows: trimming drops from the tail.
    let sizes: Vec<u64> = attribution_value["functions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["size_bytes"].as_u64())
        .collect();
    assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]));
    // `measured`, `completeness` and `analyzer_version` are never trimmed.
    assert_eq!(
        value["data"]["observation"]["measured"]["size_bytes"],
        4_096
    );
    assert_eq!(value["data"]["observation"]["completeness"], "complete");
    assert!(
        value["data"]["observation"]["analyzer_version"]
            .as_str()
            .is_some_and(|version| !version.is_empty())
    );
    Ok(())
}

#[test]
fn encode_result_trims_crate_rows_once_every_function_row_is_gone() -> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let attribution = BloatAttribution {
        estimated: true,
        reported_file_size_bytes: Some(4_096),
        text_section_size_bytes: Some(2_048),
        functions: wide_functions(20),
        crates: wide_crates(1_200),
        functions_omitted_by_row_cap: 0,
        crates_omitted_by_row_cap: 0,
    };
    let observed = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(attribution),
    )?;
    let published = fixtures::published(observed, ArtifactCompleteness::Complete, true)?;
    let encoded = tool.encode_result(&reference, published, 7)?;
    let wire = serde_json::to_vec(&encoded)?;
    assert!(wire.len() <= MAX_RESULT_BYTES, "{}", wire.len());
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "passed", "{}", value["error_code"]);
    assert_eq!(value["data"]["observation"]["analysis_validated"], true);

    let attribution_value = &value["data"]["observation"]["attribution"];
    assert_eq!(attribution_value["ranking_cap"]["functions_omitted"], 0);
    assert_eq!(attribution_value["ranking_cap"]["crates_omitted"], 0);
    let functions_omitted = attribution_value["response_trim"]["functions_omitted"]
        .as_u64()
        .ok_or("functions_omitted")?;
    let crates_omitted = attribution_value["response_trim"]["crates_omitted"]
        .as_u64()
        .ok_or("crates_omitted")?;
    assert_eq!(
        functions_omitted, 20,
        "every function row must be gone before a crate row is dropped"
    );
    assert!(
        attribution_value["functions"]
            .as_array()
            .ok_or("functions")?
            .is_empty()
    );
    assert!(crates_omitted > 0);
    let kept_crates = attribution_value["crates"]
        .as_array()
        .map(Vec::len)
        .ok_or("crates")?;
    assert_eq!(kept_crates as u64 + crates_omitted, 1_200);
    let sizes: Vec<u64> = attribution_value["crates"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| row["size_bytes"].as_u64())
        .collect();
    assert!(sizes.windows(2).all(|pair| pair[0] >= pair[1]));
    Ok(())
}

/// The discriminating case for ADR-079 §1's third concept: both limits act on
/// the same response, and the payload keeps them apart. The product's own cap
/// dropped 378 function rows from the ranking before the DTO ever saw it, and
/// then the 512 KiB budget dropped more from what was left. A reader must be
/// able to say which limit cost which rows without inferring anything, and the
/// result must still be `passed` — neither limit is a validity failure.
///
/// A regression that merged the two counters back into one, or that let either
/// of them decide the status, fails here.
#[test]
fn the_products_cap_and_the_response_budget_are_counted_apart_when_both_act() -> TestResult {
    let tool = BloatTool::new()?;
    let reference = super::super::security_tool::test_fixtures::project_ref()?;
    let attribution = BloatAttribution {
        estimated: true,
        reported_file_size_bytes: Some(4_096),
        text_section_size_bytes: Some(2_048),
        functions: wide_functions(700),
        crates: wide_crates(5),
        // What the product's own row cap had already dropped upstream.
        functions_omitted_by_row_cap: 378,
        crates_omitted_by_row_cap: 3,
    };
    let observed = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(attribution),
    )?;
    let published = fixtures::published(observed, ArtifactCompleteness::Complete, true)?;
    let encoded = tool.encode_result(&reference, published, 7)?;
    let wire = serde_json::to_vec(&encoded)?;
    assert!(wire.len() <= MAX_RESULT_BYTES, "{}", wire.len());
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "passed", "{}", value["error_code"]);
    assert_eq!(value["data"]["observation"]["analysis_validated"], true);

    let attribution_value = &value["data"]["observation"]["attribution"];
    let cap = &attribution_value["ranking_cap"];
    let trim = &attribution_value["response_trim"];
    // The cap's counts are exactly what the analysis reported, untouched by
    // trimming; the trim's counts are exactly what trimming removed.
    assert_eq!(cap["max_rows"], BLOAT_MAX_ROWS as u64);
    assert_eq!(cap["functions_omitted"], 378);
    assert_eq!(cap["crates_omitted"], 3);
    let trimmed = trim["functions_omitted"].as_u64().ok_or("trimmed")?;
    assert!(trimmed > 0);
    assert_eq!(trim["crates_omitted"], 0);
    assert_eq!(trim["budget_bytes"], MAX_RESULT_BYTES as u64);
    // Every row that left the response is accounted to exactly one limit.
    let kept = attribution_value["functions"]
        .as_array()
        .map(Vec::len)
        .ok_or("functions")?;
    assert_eq!(kept as u64 + trimmed, 700);
    // And the measurement the whole result rests on is untouched by either.
    assert_eq!(
        value["data"]["observation"]["measured"]["size_bytes"],
        4_096
    );
    assert_eq!(value["data"]["observation"]["completeness"], "complete");
    Ok(())
}

/// `encode_bounded` falls back to its `exhausted` output exactly when the
/// trim closure reports `false`. Neither of the two scenarios that produce
/// `false` — an absent `attribution`, or one with nothing left in either row
/// — can be manufactured through realistic evidence that also exceeds the
/// 512 KiB budget: this DTO's non-row fields are all bounded to a few
/// kilobytes at most (see `bloat::schemas`), so a response with zero rows
/// always fits. What is tested here, and what the exhausted fallback
/// actually depends on, is that [`trim_lowest_ranked_row`] correctly reports
/// "nothing left" at exactly the right moment.
#[test]
fn trim_lowest_ranked_row_signals_exhaustion_once_nothing_remains_to_drop() -> TestResult {
    let reference = super::super::security_tool::test_fixtures::project_ref()?;

    // No attribution at all: nothing was ever estimated.
    let observed = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Unavailable,
        None,
        None,
    )?;
    let mut data = Data {
        project_ref: reference.to_string(),
        semantics: "measured_file_size_is_exact_attribution_rankings_are_estimated",
        observation: observation(&observed, true),
        artifacts: Vec::new(),
    };
    assert!(!trim_lowest_ranked_row(&mut data));

    // An attribution whose rows are already empty offers nothing either.
    let mut empty = fixtures::attribution(Some(4_096));
    empty.functions.clear();
    empty.crates.clear();
    let observed = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(empty),
    )?;
    let mut data = Data {
        project_ref: reference.to_string(),
        semantics: "measured_file_size_is_exact_attribution_rankings_are_estimated",
        observation: observation(&observed, true),
        artifacts: Vec::new(),
    };
    assert!(!trim_lowest_ranked_row(&mut data));

    // Popping the very last row still succeeds; only the next call, once
    // nothing remains, reports exhaustion.
    let mut single_function = fixtures::attribution(Some(4_096));
    single_function.crates.clear();
    assert_eq!(single_function.functions.len(), 1);
    let observed = fixtures::observation(
        fixtures::options(BloatProfile::Release)?,
        BloatExit::Passed,
        Some(0),
        BloatCompleteness::Complete,
        Some(fixtures::measured(4_096)),
        Some(single_function),
    )?;
    let mut data = Data {
        project_ref: reference.to_string(),
        semantics: "measured_file_size_is_exact_attribution_rankings_are_estimated",
        observation: observation(&observed, true),
        artifacts: Vec::new(),
    };
    assert!(trim_lowest_ranked_row(&mut data));
    assert!(
        data.observation
            .attribution
            .as_ref()
            .ok_or("attribution")?
            .functions
            .is_empty()
    );
    assert!(!trim_lowest_ranked_row(&mut data));
    Ok(())
}

/// The exact shape `encode_result` falls back to once `trim_lowest_ranked_row`
/// reports exhaustion: a declared `blocked` result, `OUTPUT_LIMIT_EXCEEDED`,
/// and no partial `data` — mirroring every other M4/M5 tool's exhausted
/// fallback through the same `encode_bounded` helper.
#[test]
fn the_exhausted_output_is_a_declared_blocked_result_without_data() -> TestResult {
    let tool = BloatTool::new()?;
    let encoded = tool.contract.encode(Output {
        outcome: Outcome::Blocked {
            error_code: Code::OutputLimitExceeded,
            error_message: "Bloat analysis response exceeds its fixed budget",
            data: None,
        },
        summary: "Bloat analysis response exceeds its fixed budget",
        duration_ms: 7,
    })?;
    let value = encoded.structured_content.ok_or("structured content")?;
    assert_eq!(value["status"], "blocked");
    assert_eq!(value["error_code"], "OUTPUT_LIMIT_EXCEEDED");
    assert!(value["data"].is_null());
    Ok(())
}
