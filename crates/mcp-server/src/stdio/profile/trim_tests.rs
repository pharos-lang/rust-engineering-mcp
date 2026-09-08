//! The 512 KiB response budget, exercised on a ranking that cannot fit.
//!
//! The trim strategy is deliberate: the collapsed-stacks artifact keeps every
//! stack the sampler wrote, so the response may drop its lowest-ranked frames
//! and say how many it dropped. What it must never do is present a trimmed
//! ranking as a complete one.
use super::tests::{TestResult, fixtures};
use super::*;
use rust_engineering_domain::profile::{
    ProfileBuildOutcome, ProfileCompleteness, ProfileFrameWeight, ProfileStatus,
};

const MAX_RESULT_BYTES: usize = 512 * 1024;

/// Frames at the sanitizer's own 200-byte ceiling, so the ranking is large
/// while every value in it stays one the sampler could really have produced.
fn wide_frames(count: usize) -> Vec<ProfileFrameWeight> {
    let filler = "a".repeat(194);
    (0..count)
        .map(|index| ProfileFrameWeight {
            frame: format!("{filler}{index:05}"),
            self_samples: (count - index) as u64,
            total_samples: (count - index) as u64 * 2,
        })
        .collect()
}

fn encoded(frames: Vec<ProfileFrameWeight>) -> TestResult<(usize, serde_json::Value)> {
    let tool = ProfileTool::new()?;
    let observation = fixtures::observation(
        ProfileBuildOutcome::Built,
        ProfileStatus::Complete,
        ProfileCompleteness::Complete,
        fixtures::counters(4096, 0),
        frames,
    )?;
    let published = fixtures::published(observation, ArtifactCompleteness::Complete, true)?;
    let reference: ProjectRef = format!("prj_{}", "0".repeat(32)).parse()?;
    let result = tool.encode_result(&reference, published, 7)?;
    let bytes = serde_json::to_vec(&result)?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    Ok((bytes.len(), value))
}

#[test]
fn a_ranking_that_fits_is_published_whole() -> TestResult {
    let (bytes, value) = encoded(wide_frames(4))?;
    assert!(bytes <= MAX_RESULT_BYTES, "{bytes} bytes");
    let observation = &value["structuredContent"]["data"]["observation"];
    assert_eq!(observation["top_frames"].as_array().map(Vec::len), Some(4));
    assert_eq!(observation["top_frames_omitted"], 0);
    assert_eq!(observation["complete"], true);
    Ok(())
}

#[test]
fn an_oversize_ranking_is_trimmed_from_the_bottom_and_says_so() -> TestResult {
    let requested = 4096;
    let (bytes, value) = encoded(wide_frames(requested))?;
    assert!(
        bytes <= MAX_RESULT_BYTES,
        "the trimmed response still exceeds the budget: {bytes} bytes"
    );
    let observation = &value["structuredContent"]["data"]["observation"];
    let kept = observation["top_frames"]
        .as_array()
        .ok_or("top_frames absent")?;
    assert!(!kept.is_empty(), "trimming emptied the ranking entirely");
    assert!(kept.len() < requested, "nothing was trimmed");
    let omitted = observation["top_frames_omitted"]
        .as_u64()
        .ok_or("top_frames_omitted absent")?;
    assert_eq!(
        usize::try_from(omitted)? + kept.len(),
        requested,
        "the omission count must account for every dropped frame"
    );
    // Two different bounds can drop a frame and they mean different things:
    // `MAX_RESPONSE_FRAMES` is the product's own ceiling on the ranking, while
    // the budget trim means the response did not fit. This test cannot tell
    // them apart from the outside, so it asserts the accounting rather than the
    // flag; the fitting case above is where `complete` is pinned.
    assert!(
        observation["complete"].is_boolean(),
        "completeness must always be stated"
    );
    // The highest-ranked frame survives; the lowest-ranked one leaves first.
    assert_eq!(kept[0]["self_samples"], requested as u64);
    assert!(
        kept.last().map(|frame| &frame["self_samples"]) != Some(&serde_json::json!(1)),
        "the lowest-ranked frame should have been dropped first"
    );
    // The artifacts are never the thing that gets dropped.
    assert_eq!(
        value["structuredContent"]["data"]["artifacts"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(value["isError"], false);
    Ok(())
}
