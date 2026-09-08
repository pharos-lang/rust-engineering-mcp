//! Bounded model for the collapsed ("folded") stacks the guest profiling
//! helper writes (ADR-074 §5, ADR-076 §5).
//!
//! The helper emits one line per distinct stack, `root;next;...;leaf <count>`,
//! LF terminated, UTF-8, already sorted by the stack part. This module is the
//! adapter-side contract check for that artifact: every structural dimension
//! (input size, line count, frame count, frame length, alphabet, ordering) has
//! an independent bound, and every violation is a closed [`FoldedError`]
//! variant. There is no partial parse and no repair.
//!
//! Two invariants deserve to be spelled out because they are easy to get
//! backwards:
//!
//! * **Zero samples is a result, not a failure.** ADR-076 §5 declares "cero
//!   muestras es un resultado válido y declarado". A helper run that collected
//!   nothing writes an empty document, and [`parse_folded`] answers `Ok` with
//!   an empty profile. [`FoldedError::Empty`] is reserved for the strictly
//!   different case of *no artifact at all* — see [`parse_folded_artifact`].
//! * **An unsanitized frame is refused, never repaired.** ADR-074 §5 puts the
//!   frame-name sanitization in the helper ("los nombres de frame se sanean a
//!   un alfabeto cerrado"). A frame outside [`is_allowed_frame_byte`]'s
//!   alphabet is therefore a contract violation upstream, and silently
//!   rewriting it here would hide it. It is [`FoldedError::MalformedLine`].
// The M5 profiling gateway that consumes this model is a separate deliverable
// (ADR-076 §5); the model and its renderer are complete and independently
// tested, so the crate has no non-test caller yet.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Maximum number of stack lines accepted from one helper run.
pub(crate) const MAX_STACKS: usize = 65_536;
/// Maximum frames kept per stack. A deeper stack is truncated, never rejected.
pub(crate) const MAX_FRAMES_PER_STACK: usize = 256;
/// Maximum bytes of one frame name.
pub(crate) const MAX_FRAME_BYTES: usize = 200;
/// Artifact ceiling for collapsed stacks (ADR-076 §7: muestras ≤ 32 MiB).
pub(crate) const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
/// Sentinel the helper writes for a frame it could not symbolize (ADR-074 §5).
pub(crate) const UNKNOWN_FRAME: &str = "[unknown]";
/// Sentinel this parser appends to a stack truncated at
/// [`MAX_FRAMES_PER_STACK`]. It is inside the frame alphabet on purpose, so a
/// truncated stack stays renderable by the same rules as any other.
pub(crate) const TRUNCATED_FRAME: &str = "[truncated]";

/// One distinct stack, root-most frame first, with its sample count.
///
/// `count` is always non-zero: a zero count is [`FoldedError::InvalidCount`],
/// because a stack that was never sampled is not a stack the helper observed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FoldedStack {
    pub frames: Vec<String>,
    pub count: u64,
}

/// The whole parsed artifact plus the declarations ADR-074 §5 requires
/// ("una pérdida de muestras o de símbolos se declara; no se rellena").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FoldedProfile {
    /// Distinct stacks in input order, which is ascending byte order of the
    /// stack part. Identical consecutive stacks are merged into one entry.
    pub stacks: Vec<FoldedStack>,
    /// Sum of every accepted count, computed with checked arithmetic.
    pub total_samples: u64,
    /// `stacks.len()`, carried explicitly because the response DTO declares it.
    pub distinct_stacks: usize,
    /// Deepest accepted frame vector, sentinel included.
    pub deepest: usize,
    /// Blank lines skipped. Every other malformation is an error, so this only
    /// ever counts padding, never dropped evidence.
    pub rejected_lines: usize,
    /// Stacks truncated at [`MAX_FRAMES_PER_STACK`]; each carries exactly one
    /// [`TRUNCATED_FRAME`] sentinel.
    pub truncated_frames: usize,
}

/// Closed failure vocabulary. No variant carries guest-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FoldedError {
    /// No artifact at all: the caller asked to parse nothing. This is *not*
    /// the zero-sample case, which is a valid empty profile.
    Empty,
    /// Larger than [`MAX_INPUT_BYTES`]; refused before decoding or allocating.
    TooLarge,
    /// More than [`MAX_STACKS`] stack lines.
    TooManyStacks,
    /// Empty frame, no frame at all, oversized frame, or a frame outside the
    /// sanitized alphabet.
    MalformedLine,
    /// Missing, non-numeric, zero, or overflowing count; also an overflowing
    /// total or merge.
    InvalidCount,
    /// A stack part smaller than its predecessor. The artifact's determinism
    /// depends on the helper's ordering, so a violation is refused rather than
    /// re-sorted.
    NotSorted,
    /// Not UTF-8.
    InvalidUtf8,
}

impl fmt::Display for FoldedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid collapsed stack artifact")
    }
}
impl std::error::Error for FoldedError {}

/// The closed alphabet ADR-074 §5 requires of a sanitized frame name:
/// `[0-9A-Za-z_.:$<>,*&\[\]+-]`.
///
/// It admits Rust symbol syntax (`core::ptr::drop_in_place<&mut T>`) and the
/// bracketed sentinels, and admits no path separator, whitespace, quote,
/// parenthesis, equals sign or non-ASCII byte. A filesystem path or a module
/// path therefore cannot reach an artifact through this parser.
fn is_allowed_frame_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'_' | b'.'
                | b':'
                | b'$'
                | b'<'
                | b'>'
                | b','
                | b'*'
                | b'&'
                | b'['
                | b']'
                | b'+'
                | b'-'
        )
}

/// Splits one line at its LAST ASCII space into the stack part and a base-10
/// count. Rejects anything that is not a run of ASCII digits so that a signed
/// or space-padded count cannot be silently accepted.
fn split_line(line: &str) -> Result<(&str, u64), FoldedError> {
    let (stack, digits) = line.rsplit_once(' ').ok_or(FoldedError::InvalidCount)?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FoldedError::InvalidCount);
    }
    let count: u64 = digits.parse().map_err(|_| FoldedError::InvalidCount)?;
    if count == 0 {
        return Err(FoldedError::InvalidCount);
    }
    if stack.is_empty() {
        return Err(FoldedError::MalformedLine);
    }
    Ok((stack, count))
}

/// Validates every frame of one stack, including the frames past
/// [`MAX_FRAMES_PER_STACK`]: truncation drops frames from the artifact, it
/// must not drop the contract check on them.
fn parse_frames(stack: &str, truncated: &mut usize) -> Result<Vec<String>, FoldedError> {
    let mut frames: Vec<String> = Vec::new();
    let mut dropped = false;
    for frame in stack.split(';') {
        if frame.is_empty() || frame.len() > MAX_FRAME_BYTES {
            return Err(FoldedError::MalformedLine);
        }
        if !frame.bytes().all(is_allowed_frame_byte) {
            return Err(FoldedError::MalformedLine);
        }
        if frames.len() >= MAX_FRAMES_PER_STACK {
            dropped = true;
            continue;
        }
        frames.push(frame.to_owned());
    }
    if frames.is_empty() {
        return Err(FoldedError::MalformedLine);
    }
    if dropped {
        frames.push(TRUNCATED_FRAME.to_owned());
        *truncated = truncated.saturating_add(1);
    }
    Ok(frames)
}

/// Parses the collapsed-stacks artifact.
///
/// An empty document is an empty profile with `total_samples == 0`; it is the
/// declared zero-sample outcome, not an error. Every other deviation from the
/// helper contract is a [`FoldedError`].
pub(crate) fn parse_folded(input: &[u8]) -> Result<FoldedProfile, FoldedError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(FoldedError::TooLarge);
    }
    let text = std::str::from_utf8(input).map_err(|_| FoldedError::InvalidUtf8)?;
    let mut profile = FoldedProfile::default();
    // Exactly one trailing terminator is structural; anything after it would
    // be a blank line and is counted as such.
    let body = text.strip_suffix('\n').unwrap_or(text);
    if body.is_empty() {
        return Ok(profile);
    }
    let mut lines = 0usize;
    let mut previous: Option<&str> = None;
    for line in body.split('\n') {
        if line.bytes().all(|byte| byte == b' ' || byte == b'\t') {
            profile.rejected_lines = profile.rejected_lines.saturating_add(1);
            continue;
        }
        lines = lines.saturating_add(1);
        if lines > MAX_STACKS {
            return Err(FoldedError::TooManyStacks);
        }
        let (stack, count) = split_line(line)?;
        if previous.is_some_and(|prior| prior > stack) {
            return Err(FoldedError::NotSorted);
        }
        profile.total_samples = profile
            .total_samples
            .checked_add(count)
            .ok_or(FoldedError::InvalidCount)?;
        if previous == Some(stack) {
            // Sorted input puts every repetition of a stack next to the entry
            // it merges into, so the last pushed entry is always the target.
            let merged = profile
                .stacks
                .last_mut()
                .ok_or(FoldedError::MalformedLine)?;
            merged.count = merged
                .count
                .checked_add(count)
                .ok_or(FoldedError::InvalidCount)?;
            continue;
        }
        let frames = parse_frames(stack, &mut profile.truncated_frames)?;
        profile.deepest = profile.deepest.max(frames.len());
        profile.stacks.push(FoldedStack { frames, count });
        previous = Some(stack);
    }
    profile.distinct_stacks = profile.stacks.len();
    Ok(profile)
}

/// Parses an *optional* artifact.
///
/// `None` is the only way to obtain [`FoldedError::Empty`]: the helper
/// produced no collapsed-stacks file at all, so the caller asked to parse
/// nothing. `Some(b"")` is a helper run that legitimately collected zero
/// samples and yields an empty profile.
pub(crate) fn parse_folded_artifact(input: Option<&[u8]>) -> Result<FoldedProfile, FoldedError> {
    match input {
        None => Err(FoldedError::Empty),
        Some(bytes) => parse_folded(bytes),
    }
}

/// One row of the bounded frame ranking ADR-076 §5 requires in the response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FrameWeight {
    pub frame: String,
    /// Samples of the stacks whose LEAF is this frame.
    pub self_samples: u64,
    /// Samples of the stacks that CONTAIN this frame, counted once per stack.
    pub total_samples: u64,
}

/// Ranks frames by inclusive then exclusive weight.
///
/// A stack contributes its count to `total_samples` once per *distinct* frame,
/// so a recursive stack such as `a;b;a` credits `a` a single time and the
/// ranking cannot exceed the profile's own total. Ordering is
/// `total_samples` descending, then `self_samples` descending, then the frame
/// name ascending, which is total and therefore deterministic.
pub(crate) fn frame_weights(profile: &FoldedProfile, limit: usize) -> Vec<FrameWeight> {
    let mut weights: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    for stack in &profile.stacks {
        if let Some(leaf) = stack.frames.last() {
            let entry = weights.entry(leaf.as_str()).or_default();
            entry.0 = entry.0.saturating_add(stack.count);
        }
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for frame in &stack.frames {
            if seen.insert(frame.as_str()) {
                let entry = weights.entry(frame.as_str()).or_default();
                entry.1 = entry.1.saturating_add(stack.count);
            }
        }
    }
    let mut ranked: Vec<FrameWeight> = weights
        .into_iter()
        .map(|(frame, (self_samples, total_samples))| FrameWeight {
            frame: frame.to_owned(),
            self_samples,
            total_samples,
        })
        .collect();
    ranked.sort_by(|left, right| {
        right
            .total_samples
            .cmp(&left.total_samples)
            .then(right.self_samples.cmp(&left.self_samples))
            .then(left.frame.cmp(&right.frame))
    });
    ranked.truncate(limit);
    ranked
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(profile: &FoldedProfile, index: usize) -> Result<&[String], FoldedError> {
        Ok(&profile
            .stacks
            .get(index)
            .ok_or(FoldedError::MalformedLine)?
            .frames)
    }

    #[test]
    fn parses_a_sorted_document_into_stacks_counts_and_declarations() -> Result<(), FoldedError> {
        let profile = parse_folded(b"main;alpha 3\nmain;beta;gamma 7\n")?;
        assert_eq!(profile.stacks.len(), 2);
        assert_eq!(profile.distinct_stacks, 2);
        assert_eq!(profile.total_samples, 10);
        assert_eq!(profile.deepest, 3);
        assert_eq!(profile.rejected_lines, 0);
        assert_eq!(profile.truncated_frames, 0);
        assert_eq!(frames(&profile, 0)?, ["main", "alpha"]);
        assert_eq!(profile.stacks.get(1).map(|stack| stack.count), Some(7));
        Ok(())
    }

    #[test]
    fn empty_document_is_a_declared_zero_sample_profile_not_an_error() -> Result<(), FoldedError> {
        for input in [b"".as_slice(), b"\n".as_slice()] {
            let profile = parse_folded(input)?;
            assert!(profile.stacks.is_empty());
            assert_eq!(profile.total_samples, 0);
            assert_eq!(profile.distinct_stacks, 0);
            assert_eq!(profile.deepest, 0);
        }
        // Same artifact, presented as present-but-empty.
        assert_eq!(
            parse_folded_artifact(Some(b"")),
            Ok(FoldedProfile::default())
        );
        Ok(())
    }

    #[test]
    fn absent_artifact_is_the_only_source_of_the_empty_error() {
        assert_eq!(parse_folded_artifact(None), Err(FoldedError::Empty));
    }

    #[test]
    fn input_over_the_ceiling_is_rejected_before_decoding() {
        assert_eq!(
            parse_folded(&vec![b'a'; MAX_INPUT_BYTES + 1]),
            Err(FoldedError::TooLarge)
        );
    }

    #[test]
    fn non_utf8_input_is_rejected() {
        assert_eq!(
            parse_folded(b"main;al\xffpha 1\n"),
            Err(FoldedError::InvalidUtf8)
        );
    }

    #[test]
    fn more_lines_than_the_stack_ceiling_are_rejected() -> Result<(), FoldedError> {
        let mut document = String::new();
        for index in 0..MAX_STACKS {
            document.push_str(&format!("f{index:06} 1\n"));
        }
        let accepted = parse_folded(document.as_bytes())?;
        assert_eq!(accepted.distinct_stacks, MAX_STACKS);
        document.push_str(&format!("f{MAX_STACKS:06} 1\n"));
        assert_eq!(
            parse_folded(document.as_bytes()),
            Err(FoldedError::TooManyStacks)
        );
        Ok(())
    }

    #[test]
    fn malformed_stacks_are_rejected() {
        // Empty frame between two separators.
        assert_eq!(
            parse_folded(b"main;;leaf 1\n"),
            Err(FoldedError::MalformedLine)
        );
        // Leading and trailing empty frames.
        assert_eq!(parse_folded(b";main 1\n"), Err(FoldedError::MalformedLine));
        assert_eq!(parse_folded(b"main; 1\n"), Err(FoldedError::MalformedLine));
        // No stack at all before the count.
        assert_eq!(parse_folded(b" 1\n"), Err(FoldedError::MalformedLine));
        // Oversized frame, one byte past the bound.
        let oversized = format!("{} 1\n", "a".repeat(MAX_FRAME_BYTES + 1));
        assert_eq!(
            parse_folded(oversized.as_bytes()),
            Err(FoldedError::MalformedLine)
        );
        // Exactly at the bound is accepted.
        let exact = format!("{} 1\n", "a".repeat(MAX_FRAME_BYTES));
        assert!(parse_folded(exact.as_bytes()).is_ok());
    }

    #[test]
    fn invalid_counts_are_rejected() {
        assert_eq!(parse_folded(b"main 0\n"), Err(FoldedError::InvalidCount));
        assert_eq!(parse_folded(b"main\n"), Err(FoldedError::InvalidCount));
        assert_eq!(parse_folded(b"main x\n"), Err(FoldedError::InvalidCount));
        assert_eq!(parse_folded(b"main -1\n"), Err(FoldedError::InvalidCount));
        assert_eq!(parse_folded(b"main +1\n"), Err(FoldedError::InvalidCount));
        assert_eq!(
            parse_folded(b"main 1_000\n"),
            Err(FoldedError::InvalidCount)
        );
        assert_eq!(
            parse_folded(b"main 18446744073709551616\n"),
            Err(FoldedError::InvalidCount)
        );
        // A carriage return keeps the count from being a pure digit run.
        assert_eq!(parse_folded(b"main 1\r\n"), Err(FoldedError::InvalidCount));
    }

    #[test]
    fn descending_stacks_are_rejected_instead_of_being_re_sorted() {
        assert_eq!(
            parse_folded(b"zeta 1\nalpha 1\n"),
            Err(FoldedError::NotSorted)
        );
    }

    #[test]
    fn identical_consecutive_stacks_merge_with_checked_arithmetic() -> Result<(), FoldedError> {
        let profile = parse_folded(b"main;leaf 2\nmain;leaf 3\nmain;other 1\n")?;
        assert_eq!(profile.stacks.len(), 2);
        assert_eq!(profile.distinct_stacks, 2);
        assert_eq!(profile.stacks.first().map(|stack| stack.count), Some(5));
        assert_eq!(profile.total_samples, 6);
        let overflowing = format!("main {max}\nmain {max}\n", max = u64::MAX);
        assert_eq!(
            parse_folded(overflowing.as_bytes()),
            Err(FoldedError::InvalidCount)
        );
        Ok(())
    }

    #[test]
    fn deeper_stacks_keep_the_root_most_frames_and_carry_the_sentinel() -> Result<(), FoldedError> {
        let mut stack = String::new();
        for index in 0..(MAX_FRAMES_PER_STACK + 4) {
            if index > 0 {
                stack.push(';');
            }
            stack.push_str(&format!("f{index:04}"));
        }
        let profile = parse_folded(format!("{stack} 9\n").as_bytes())?;
        let kept = frames(&profile, 0)?;
        assert_eq!(kept.len(), MAX_FRAMES_PER_STACK + 1);
        assert_eq!(kept.first().map(String::as_str), Some("f0000"));
        assert_eq!(
            kept.get(MAX_FRAMES_PER_STACK - 1).map(String::as_str),
            Some("f0255")
        );
        assert_eq!(kept.last().map(String::as_str), Some(TRUNCATED_FRAME));
        assert_eq!(profile.truncated_frames, 1);
        assert_eq!(profile.deepest, MAX_FRAMES_PER_STACK + 1);
        assert_eq!(profile.total_samples, 9);
        Ok(())
    }

    #[test]
    fn unsanitized_frames_are_refused_rather_than_repaired() {
        // A separator smuggled behind an escape: the backslash is not in the
        // alphabet, so the line never becomes a two-frame stack.
        assert_eq!(
            parse_folded(b"main\\;leaf 1\n"),
            Err(FoldedError::MalformedLine)
        );
        // A line terminator can never sit inside a frame: the carriage return
        // is refused outright, and a bare LF splits the document instead.
        assert_eq!(
            parse_folded(b"main\rleaf 1\n"),
            Err(FoldedError::MalformedLine)
        );
        assert!(!is_allowed_frame_byte(b'\n'));
        assert_eq!(parse_folded(b"ma\nin 1\n"), Err(FoldedError::InvalidCount));
        // A filesystem path: ADR-074 §5 forbids emitting one at all.
        assert_eq!(
            parse_folded(b"/path/like/this 1\n"),
            Err(FoldedError::MalformedLine)
        );
        // A non-ASCII byte that is still valid UTF-8.
        assert_eq!(
            parse_folded("café 1\n".as_bytes()),
            Err(FoldedError::MalformedLine)
        );
        // A script-looking frame: the slash, parentheses and quote are all
        // outside the alphabet.
        assert_eq!(
            parse_folded(b"<script>alert('x')</script> 1\n"),
            Err(FoldedError::MalformedLine)
        );
        // A space inside a frame is impossible by construction: the last space
        // is the count separator, so the remainder fails the alphabet.
        assert_eq!(
            parse_folded(b"<T as U>::f 1\n"),
            Err(FoldedError::MalformedLine)
        );
    }

    #[test]
    fn rust_symbol_syntax_and_the_sentinels_stay_inside_the_alphabet() -> Result<(), FoldedError> {
        // `[truncated]` sorts before `[unknown]`, and no frame carries the
        // space that a `<T as U>::f` rendering would need.
        let document = format!(
            "{TRUNCATED_FRAME};a$b.c,d*e+f-g[0] 1\n\
             {UNKNOWN_FRAME};core::ptr::drop_in_place<&alloc::vec::Vec<u8>> 2\n"
        );
        let profile = parse_folded(document.as_bytes())?;
        assert_eq!(profile.stacks.len(), 2);
        assert_eq!(
            frames(&profile, 1)?.first().map(String::as_str),
            Some(UNKNOWN_FRAME)
        );
        assert_eq!(profile.total_samples, 3);
        Ok(())
    }

    #[test]
    fn blank_padding_lines_are_counted_and_skipped() -> Result<(), FoldedError> {
        let profile = parse_folded(b"main 1\n\n   \nzeta 2\n")?;
        assert_eq!(profile.stacks.len(), 2);
        assert_eq!(profile.rejected_lines, 2);
        assert_eq!(profile.total_samples, 3);
        Ok(())
    }

    #[test]
    fn frame_weights_separate_self_from_total_and_never_double_count_recursion()
    -> Result<(), FoldedError> {
        // `a;b;a` recurses: `a` must be credited once for that stack.
        let profile = parse_folded(b"a;b;a 4\na;c 6\n")?;
        let ranked = frame_weights(&profile, 10);
        assert_eq!(
            ranked,
            vec![
                FrameWeight {
                    frame: "a".to_owned(),
                    self_samples: 4,
                    total_samples: 10,
                },
                FrameWeight {
                    frame: "c".to_owned(),
                    self_samples: 6,
                    total_samples: 6,
                },
                FrameWeight {
                    frame: "b".to_owned(),
                    self_samples: 0,
                    total_samples: 4,
                },
            ]
        );
        assert!(
            ranked
                .iter()
                .all(|weight| weight.total_samples <= profile.total_samples)
        );
        Ok(())
    }

    #[test]
    fn frame_weights_are_bounded_and_totally_ordered() -> Result<(), FoldedError> {
        // Two frames tie on total; the self weight then the name break it.
        let profile = parse_folded(b"m;x 5\nm;y 5\n")?;
        let ranked = frame_weights(&profile, 2);
        assert_eq!(ranked.len(), 2);
        assert_eq!(
            ranked
                .iter()
                .map(|weight| weight.frame.as_str())
                .collect::<Vec<_>>(),
            ["m", "x"]
        );
        assert!(frame_weights(&profile, 0).is_empty());
        assert!(frame_weights(&FoldedProfile::default(), 8).is_empty());
        Ok(())
    }
}
