//! Bounded parser for `cargo-bloat 0.12.1 --message-format json` (ADR-076 §6).
//!
//! Two views exist and this module keeps them apart on purpose:
//!
//! ```text
//! functions: {"file-size":465736,"text-section-size":214808,
//!             "functions":[{"crate":"std","name":"…","size":9364}]}
//! crates:    {"file-size":465736,"text-section-size":214808,
//!             "crates":[{"name":"std","size":221512},{"name":"[Unknown]","size":2872}]}
//! ```
//!
//! The keys are hyphenated, and `[Unknown]` is a real crate name the tool emits
//! for the bytes it could not attribute — it is the analyzer's own bucket, not a
//! parse failure, and it is preserved verbatim.
//!
//! Everything here is an ESTIMATE. ADR-076 §6 separates the exact file size this
//! product measures itself from the attribution `cargo-bloat` reports, so this
//! module never merges the two: it reports what the analyzer said, refuses a
//! report that cannot describe any real file (a row bigger than the file, a text
//! section bigger than the file), and leaves the exact/estimated distinction to
//! [`rust_engineering_domain::bloat::BloatAttribution`].
//!
//! A symbol name is untrusted text produced by compiling the project. It is data
//! for a report, never a path and never a command, so a control character — a
//! NUL, a newline, a carriage return, any C0/C1 — refuses the report rather than
//! being escaped into something that renders.
// The M5 bloat gateway that consumes this parser is a separate, concurrent
// deliverable (ADR-076 §6); the parser is complete and independently tested, so
// the crate has no non-test caller for it yet.
#![allow(dead_code)]

use rust_engineering_domain::bloat::{BLOAT_MAX_ROWS, BloatCrate, BloatFunction};
use serde::Deserialize;

/// Artifact ceiling for the bloat report (ADR-076 §7: bloat ≤ 4 MiB).
pub(crate) const MAX_BLOAT_JSON: usize = 4 * 1024 * 1024;

/// Byte ceiling on one `crate`/`name` string.
///
/// This CAPS, it does not reject: a monomorphized Rust symbol legitimately runs
/// past 512 bytes, and discarding a whole binary's attribution because one
/// symbol is long would throw away real evidence over a display concern. The
/// name is only ever display data in a ranking, so the tail is dropped at a
/// UTF-8 boundary. A control character is a different matter and is refused.
pub(crate) const MAX_BLOAT_NAME_BYTES: usize = 512;

/// Defensive ceiling on the rows one payload may declare, independent of
/// [`BLOAT_MAX_ROWS`]: the report cap keeps the largest rows and declares the
/// rest omitted, while this bound refuses a payload that is not a plausible
/// `cargo-bloat` run at all. The 4 MiB input ceiling already bounds the work;
/// this makes the bound explicit instead of implied.
pub(crate) const MAX_BLOAT_INPUT_ROWS: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BloatFunctionsReport {
    pub file_size_bytes: u64,
    pub text_section_size_bytes: u64,
    pub functions: Vec<BloatFunction>,
    pub omitted: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BloatCratesReport {
    pub file_size_bytes: u64,
    pub text_section_size_bytes: u64,
    pub crates: Vec<BloatCrate>,
    pub omitted: u32,
}

/// Closed failure vocabulary. No variant carries project-controlled text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BloatParseError {
    /// No output at all (nothing, or only whitespace).
    Empty,
    /// Above [`MAX_BLOAT_JSON`]. Refused before parsing.
    TooLarge,
    /// Not UTF-8, not JSON, or a document that cannot describe a real file: a
    /// negative or non-integral size, a row larger than the file, a text
    /// section larger than the file, or a control character in a name.
    Malformed,
    /// A valid document of the OTHER view. `parse_functions` never falls back
    /// to `crates` and `parse_crates` never falls back to `functions`.
    UnexpectedShape,
    /// More rows than [`MAX_BLOAT_INPUT_ROWS`].
    TooManyRows,
}

/// One document, covering both views.
///
/// Not `deny_unknown_fields`: the analyzer version is pinned by provisioning
/// and a patch release adding a field must not turn a usable report into a
/// refusal. Every field read is typed explicitly instead, and no
/// `serde_json::Value` is ever built from project-controlled bytes.
///
/// Sizes are typed `u64`, which is itself the rejection of a negative or
/// non-integral size: serde refuses `-1` and `9364.5` before this module sees
/// them, and a size that is not a whole number of bytes cannot describe a file.
#[derive(Deserialize)]
struct Document {
    #[serde(rename = "file-size")]
    file_size: u64,
    #[serde(rename = "text-section-size")]
    text_section_size: u64,
    functions: Option<Vec<FunctionRow>>,
    crates: Option<Vec<CrateRow>>,
}

#[derive(Deserialize)]
struct FunctionRow {
    // `crate` is a keyword; the wire key is not.
    #[serde(rename = "crate")]
    crate_name: String,
    name: String,
    size: u64,
}

#[derive(Deserialize)]
struct CrateRow {
    name: String,
    size: u64,
}

/// Shared entry checks for both views.
fn document(bytes: &[u8]) -> Result<Document, BloatParseError> {
    if bytes.len() > MAX_BLOAT_JSON {
        return Err(BloatParseError::TooLarge);
    }
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(BloatParseError::Empty);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| BloatParseError::Malformed)?;
    let document: Document = serde_json::from_str(text).map_err(|_| BloatParseError::Malformed)?;
    if document.text_section_size > document.file_size {
        return Err(BloatParseError::Malformed);
    }
    Ok(document)
}

/// A name as it may appear in a report: control-character free, capped at
/// [`MAX_BLOAT_NAME_BYTES`] on a UTF-8 boundary.
fn report_name(value: &str) -> Result<String, BloatParseError> {
    // `char::is_control` covers NUL, LF, CR and every other C0/C1 control.
    if value.chars().any(char::is_control) {
        return Err(BloatParseError::Malformed);
    }
    if value.len() <= MAX_BLOAT_NAME_BYTES {
        return Ok(value.to_owned());
    }
    let mut end = MAX_BLOAT_NAME_BYTES;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    Ok(value[..end].to_owned())
}

/// A size the analyzer attributed. It can never exceed the file it describes.
fn attributed_size(size: u64, file_size: u64) -> Result<u64, BloatParseError> {
    if size > file_size {
        return Err(BloatParseError::Malformed);
    }
    Ok(size)
}

/// Rows dropped by [`BLOAT_MAX_ROWS`], saturating at `u32::MAX`.
fn omitted(total: usize) -> u32 {
    u32::try_from(total.saturating_sub(BLOAT_MAX_ROWS)).unwrap_or(u32::MAX)
}

fn checked_rows<T>(rows: &[T]) -> Result<(), BloatParseError> {
    if rows.len() > MAX_BLOAT_INPUT_ROWS {
        return Err(BloatParseError::TooManyRows);
    }
    Ok(())
}

/// Parses the functions view. A crates-view payload is
/// [`BloatParseError::UnexpectedShape`]: the two views answer different
/// questions and one is never read as the other. A payload carrying BOTH keys
/// is refused for the same reason — the pinned analyzer emits exactly one.
pub(crate) fn parse_functions(bytes: &[u8]) -> Result<BloatFunctionsReport, BloatParseError> {
    let document = document(bytes)?;
    let (Some(rows), None) = (document.functions, document.crates) else {
        return Err(BloatParseError::UnexpectedShape);
    };
    checked_rows(&rows)?;
    let mut functions = rows
        .into_iter()
        .map(|row| {
            Ok(BloatFunction {
                crate_name: report_name(&row.crate_name)?,
                name: report_name(&row.name)?,
                size_bytes: attributed_size(row.size, document.file_size)?,
            })
        })
        .collect::<Result<Vec<_>, BloatParseError>>()?;
    // The analyzer already sorts descending; sorting here makes the kept set a
    // function of the content and never of the arrival order.
    functions.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.crate_name.cmp(&right.crate_name))
    });
    let dropped = omitted(functions.len());
    functions.truncate(BLOAT_MAX_ROWS);
    Ok(BloatFunctionsReport {
        file_size_bytes: document.file_size,
        text_section_size_bytes: document.text_section_size,
        functions,
        omitted: dropped,
    })
}

/// Parses the crates view. A functions-view payload is
/// [`BloatParseError::UnexpectedShape`]; see [`parse_functions`].
pub(crate) fn parse_crates(bytes: &[u8]) -> Result<BloatCratesReport, BloatParseError> {
    let document = document(bytes)?;
    let (Some(rows), None) = (document.crates, document.functions) else {
        return Err(BloatParseError::UnexpectedShape);
    };
    checked_rows(&rows)?;
    let mut crates = rows
        .into_iter()
        .map(|row| {
            Ok(BloatCrate {
                // `[Unknown]` is the analyzer's own bucket for unattributed
                // bytes and survives verbatim.
                name: report_name(&row.name)?,
                size_bytes: attributed_size(row.size, document.file_size)?,
            })
        })
        .collect::<Result<Vec<_>, BloatParseError>>()?;
    crates.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.name.cmp(&right.name))
    });
    let dropped = omitted(crates.len());
    crates.truncate(BLOAT_MAX_ROWS);
    Ok(BloatCratesReport {
        file_size_bytes: document.file_size,
        text_section_size_bytes: document.text_section_size,
        crates,
        omitted: dropped,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;

    /// Captured verbatim from `cargo-bloat 0.12.1 --release --message-format json -n N`.
    const FUNCTIONS_PAYLOAD: &[u8] = br#"{"file-size":465736,"text-section-size":214808,"functions":[{"crate":"std","name":"std::backtrace_rs::symbolize::gimli::resolve","size":9364}]}"#;
    /// The same run with `--crates`.
    const CRATES_PAYLOAD: &[u8] = br#"{"file-size":465736,"text-section-size":214808,"crates":[{"name":"std","size":221512},{"name":"[Unknown]","size":2872}]}"#;

    /// A crates-view document. The text section is held at one byte so the
    /// fixture stays valid for every `file_size` a test needs.
    fn crates_document(rows: &[(&str, u64)], file_size: u64) -> Vec<u8> {
        let body = rows
            .iter()
            .map(|(name, size)| format!(r#"{{"name":"{name}","size":{size}}}"#))
            .collect::<Vec<_>>()
            .join(",");
        format!(r#"{{"file-size":{file_size},"text-section-size":1,"crates":[{body}]}}"#)
            .into_bytes()
    }

    #[test]
    fn the_real_functions_payload_parses_field_for_field() -> Result<(), BloatParseError> {
        let report = parse_functions(FUNCTIONS_PAYLOAD)?;
        assert_eq!(report.file_size_bytes, 465_736);
        assert_eq!(report.text_section_size_bytes, 214_808);
        assert_eq!(report.omitted, 0);
        assert_eq!(
            report.functions,
            vec![BloatFunction {
                crate_name: "std".to_owned(),
                name: "std::backtrace_rs::symbolize::gimli::resolve".to_owned(),
                size_bytes: 9_364,
            }]
        );
        Ok(())
    }

    #[test]
    fn the_real_crates_payload_parses_field_for_field_and_keeps_unknown()
    -> Result<(), BloatParseError> {
        let report = parse_crates(CRATES_PAYLOAD)?;
        assert_eq!(report.file_size_bytes, 465_736);
        assert_eq!(report.text_section_size_bytes, 214_808);
        assert_eq!(report.omitted, 0);
        assert_eq!(
            report.crates,
            vec![
                BloatCrate {
                    name: "std".to_owned(),
                    size_bytes: 221_512,
                },
                // The analyzer's own bucket, preserved verbatim.
                BloatCrate {
                    name: "[Unknown]".to_owned(),
                    size_bytes: 2_872,
                },
            ]
        );
        Ok(())
    }

    #[test]
    fn neither_view_is_ever_read_as_the_other() {
        assert_eq!(
            parse_functions(CRATES_PAYLOAD).err(),
            Some(BloatParseError::UnexpectedShape)
        );
        assert_eq!(
            parse_crates(FUNCTIONS_PAYLOAD).err(),
            Some(BloatParseError::UnexpectedShape)
        );
        // Neither key at all.
        let bare = br#"{"file-size":10,"text-section-size":5}"#;
        assert_eq!(
            parse_functions(bare).err(),
            Some(BloatParseError::UnexpectedShape)
        );
        assert_eq!(
            parse_crates(bare).err(),
            Some(BloatParseError::UnexpectedShape)
        );
        // Both keys: not the pinned analyzer's output.
        let both = br#"{"file-size":10,"text-section-size":5,"functions":[],"crates":[]}"#;
        assert_eq!(
            parse_functions(both).err(),
            Some(BloatParseError::UnexpectedShape)
        );
        assert_eq!(
            parse_crates(both).err(),
            Some(BloatParseError::UnexpectedShape)
        );
    }

    #[test]
    fn oversize_empty_and_non_utf8_inputs_are_refused() {
        assert_eq!(
            parse_functions(&vec![b' '; MAX_BLOAT_JSON + 1]).err(),
            Some(BloatParseError::TooLarge)
        );
        assert_eq!(
            parse_crates(&vec![b' '; MAX_BLOAT_JSON + 1]).err(),
            Some(BloatParseError::TooLarge)
        );
        for empty in [&b""[..], b"   \n\t"] {
            assert_eq!(parse_functions(empty).err(), Some(BloatParseError::Empty));
            assert_eq!(parse_crates(empty).err(), Some(BloatParseError::Empty));
        }
        let mut invalid = CRATES_PAYLOAD.to_vec();
        invalid.push(0xff);
        assert_eq!(
            parse_crates(&invalid).err(),
            Some(BloatParseError::Malformed)
        );
        assert_eq!(
            parse_functions(b"\xff\xfe not json").err(),
            Some(BloatParseError::Malformed)
        );
    }

    #[test]
    fn a_document_that_cannot_describe_a_real_file_is_malformed() {
        // A row bigger than the file it is attributed to.
        assert_eq!(
            parse_crates(&crates_document(&[("std", 4_097)], 4_096)).err(),
            Some(BloatParseError::Malformed)
        );
        assert_eq!(
            parse_functions(
                br#"{"file-size":4096,"text-section-size":10,"functions":[{"crate":"a","name":"b","size":4097}]}"#
            )
            .err(),
            Some(BloatParseError::Malformed)
        );
        // A text section bigger than the file that contains it.
        assert_eq!(
            parse_crates(
                br#"{"file-size":4096,"text-section-size":4097,"crates":[{"name":"a","size":1}]}"#
            )
            .err(),
            Some(BloatParseError::Malformed)
        );
        // Negative and non-integral sizes, in every position.
        for payload in [
            &br#"{"file-size":-1,"text-section-size":1,"crates":[{"name":"a","size":1}]}"#[..],
            br#"{"file-size":10,"text-section-size":-1,"crates":[{"name":"a","size":1}]}"#,
            br#"{"file-size":10,"text-section-size":1,"crates":[{"name":"a","size":-1}]}"#,
            br#"{"file-size":10.5,"text-section-size":1,"crates":[{"name":"a","size":1}]}"#,
            br#"{"file-size":10,"text-section-size":1,"crates":[{"name":"a","size":1.5}]}"#,
        ] {
            assert_eq!(
                parse_crates(payload).err(),
                Some(BloatParseError::Malformed)
            );
        }
        // Exactly the file size is attributable; a whole binary can be one row.
        assert!(parse_crates(&crates_document(&[("std", 4_096)], 4_096)).is_ok());
    }

    #[test]
    fn rows_cap_at_the_ceiling_keeping_the_largest() -> Result<(), BloatParseError> {
        let names = (0..300)
            .map(|index| format!("c{index:03}"))
            .collect::<Vec<_>>();
        let rows = names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.as_str(), 300 - index as u64))
            .collect::<Vec<_>>();
        let report = parse_crates(&crates_document(&rows, 100_000))?;
        assert_eq!(report.crates.len(), BLOAT_MAX_ROWS);
        assert_eq!(report.omitted, 44);
        // The largest survive, the smallest are the ones dropped.
        assert_eq!(report.crates.first().map(|row| row.size_bytes), Some(300));
        assert_eq!(report.crates.last().map(|row| row.size_bytes), Some(45));
        assert!(report.crates.iter().all(|row| row.size_bytes >= 45));
        Ok(())
    }

    #[test]
    fn more_rows_than_the_input_ceiling_are_refused() {
        let rows = (0..=MAX_BLOAT_INPUT_ROWS)
            .map(|_| r#"{"name":"a","size":1}"#)
            .collect::<Vec<_>>()
            .join(",");
        let payload =
            format!(r#"{{"file-size":10,"text-section-size":1,"crates":[{rows}]}}"#).into_bytes();
        assert!(payload.len() <= MAX_BLOAT_JSON);
        assert_eq!(
            parse_crates(&payload).err(),
            Some(BloatParseError::TooManyRows)
        );
    }

    #[test]
    fn hostile_names_are_refused_and_long_ones_are_capped() -> Result<(), BloatParseError> {
        // Written as JSON escapes so the source stays free of control
        // characters: a newline, a carriage return, a NUL, a C0 control and
        // a C1 control all arrive decoded as real control characters.
        for hostile in [r"a\nb", r"a\rb", r"a\u0000b", r"a\u0007b", r"a\u009bb"] {
            let payload = format!(
                r#"{{"file-size":10,"text-section-size":1,"crates":[{{"name":"{hostile}","size":1}}]}}"#
            );
            assert_eq!(
                parse_crates(payload.as_bytes()).err(),
                Some(BloatParseError::Malformed),
                "{hostile}"
            );
            let payload = format!(
                r#"{{"file-size":10,"text-section-size":1,"functions":[{{"crate":"{hostile}","name":"n","size":1}}]}}"#
            );
            assert_eq!(
                parse_functions(payload.as_bytes()).err(),
                Some(BloatParseError::Malformed),
                "{hostile}"
            );
        }
        // A long symbol is capped, not refused: the report keeps its evidence.
        let long = "x".repeat(MAX_BLOAT_NAME_BYTES + 40);
        let report = parse_crates(&crates_document(&[(long.as_str(), 1)], 10))?;
        assert_eq!(
            report.crates.first().map(|row| row.name.len()),
            Some(MAX_BLOAT_NAME_BYTES)
        );
        // Exactly at the cap nothing is dropped.
        let exact = "y".repeat(MAX_BLOAT_NAME_BYTES);
        let report = parse_crates(&crates_document(&[(exact.as_str(), 1)], 10))?;
        assert_eq!(
            report.crates.first().map(|row| row.name.as_str()),
            Some(exact.as_str())
        );
        // A multi-byte boundary is never split.
        let wide = "é".repeat(MAX_BLOAT_NAME_BYTES);
        let report = parse_crates(&crates_document(&[(wide.as_str(), 1)], 10))?;
        let kept = report
            .crates
            .first()
            .map(|row| row.name.clone())
            .unwrap_or_default();
        assert_eq!(kept.len(), MAX_BLOAT_NAME_BYTES);
        assert!(kept.chars().all(|value| value == 'é'));
        Ok(())
    }

    #[test]
    fn ordering_is_deterministic_across_equal_sizes() -> Result<(), BloatParseError> {
        let report = parse_crates(&crates_document(
            &[("zeta", 10), ("alpha", 10), ("mid", 20)],
            1_000,
        ))?;
        assert_eq!(
            report
                .crates
                .iter()
                .map(|row| row.name.as_str())
                .collect::<Vec<_>>(),
            ["mid", "alpha", "zeta"]
        );
        let report = parse_functions(
            br#"{"file-size":1000,"text-section-size":10,"functions":[{"crate":"z","name":"same","size":5},{"crate":"a","name":"same","size":5},{"crate":"m","name":"other","size":5}]}"#,
        )?;
        assert_eq!(
            report
                .functions
                .iter()
                .map(|row| (row.name.as_str(), row.crate_name.as_str()))
                .collect::<Vec<_>>(),
            [("other", "m"), ("same", "a"), ("same", "z")]
        );
        Ok(())
    }
}
