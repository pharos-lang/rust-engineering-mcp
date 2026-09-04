//! Bounded display-only parsing of the pinned rustfmt human check output.
//! Grammar: rustfmt src/emitter/diff.rs and src/rustfmt_diff.rs; real runtime
//! fixtures remain the compatibility oracle. This output never authorizes edits.
use rust_engineering_domain::SourceBundle;
use std::collections::BTreeSet;

const MAX_INPUT: usize = 256 * 1024;
const MAX_DIFF: usize = 32 * 1024;
const MAX_FILES: usize = 128;

pub(super) struct ParsedFormat {
    pub affected_files: Vec<String>,
    pub affected_files_omitted: u64,
    pub diff: Option<String>,
    pub diff_omitted: bool,
    pub complete: bool,
}

struct Chunk {
    file: usize,
    changed: bool,
    valid: bool,
}

/// Parse only captured file names. Never normalize an untrusted path into one.
pub(super) fn parse(stdout: &str, source: &SourceBundle, stream_complete: bool) -> ParsedFormat {
    let mut end = stdout.len().min(MAX_INPUT);
    while !stdout.is_char_boundary(end) {
        end -= 1;
    }
    let input = &stdout[..end];
    let mut complete = stream_complete && end == stdout.len();
    let mut affected = BTreeSet::new();
    let mut normalized = String::new();
    let mut too_large = false;
    let mut chunk: Option<Chunk> = None;
    // Calculate each bound once: repeated headers must not rescan source bytes.
    let positions: Vec<_> = source
        .files()
        .iter()
        .map(|file| file.bytes().iter().filter(|byte| **byte == b'\n').count() + 1)
        .collect();
    for raw_line in input.split_inclusive('\n') {
        let Some(line) = raw_line.strip_suffix('\n') else {
            complete = false;
            break;
        };
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.starts_with("Diff in ") || line.starts_with("Incorrect newline style in ") {
            finish_chunk(chunk.take(), &mut affected, &mut complete);
            if let Some(path) = line.strip_prefix("Incorrect newline style in ") {
                if let Some(file) = captured_file(path, source) {
                    affected.insert(file);
                    append(
                        &mut normalized,
                        &mut too_large,
                        "Incorrect newline style in ",
                    );
                    append(&mut normalized, &mut too_large, source.files()[file].path());
                    append(&mut normalized, &mut too_large, "\n");
                } else {
                    complete = false;
                }
                continue;
            }
            if let Some((file, position)) = diff_header(line, source, &positions) {
                chunk = Some(Chunk {
                    file,
                    changed: false,
                    valid: true,
                });
                append(
                    &mut normalized,
                    &mut too_large,
                    &format!("Diff in {}:{position}:\n", source.files()[file].path()),
                );
            } else {
                complete = false;
            }
        } else if let Some(current) = &mut chunk {
            // Inspect only the prefix; source text resembling a header stays body.
            if matches!(line.as_bytes().first(), Some(b' ' | b'+' | b'-'))
                && !line.chars().any(|c| c.is_control() && c != '\t')
            {
                current.changed |= line.starts_with(['+', '-']);
                append(&mut normalized, &mut too_large, line);
                append(&mut normalized, &mut too_large, "\n");
            } else {
                current.valid = false;
                complete = false;
            }
        } else {
            complete = false;
        }
    }
    finish_chunk(chunk, &mut affected, &mut complete);
    let affected_files_omitted = affected.len().saturating_sub(MAX_FILES) as u64;
    let affected_files = affected
        .into_iter()
        .take(MAX_FILES)
        .map(|index| source.files()[index].path().to_owned())
        .collect();
    let diff_omitted = !stdout.is_empty() && (!complete || too_large);
    let diff = if !normalized.is_empty() && !diff_omitted {
        Some(normalized)
    } else {
        None
    };
    ParsedFormat {
        affected_files,
        affected_files_omitted,
        diff,
        diff_omitted,
        complete,
    }
}

fn captured_file(path: &str, source: &SourceBundle) -> Option<usize> {
    let path = path.strip_prefix("/source/")?;
    source
        .files()
        .binary_search_by(|file| file.path().cmp(path))
        .ok()
}

fn diff_header(line: &str, source: &SourceBundle, positions: &[usize]) -> Option<(usize, usize)> {
    let text = line.strip_prefix("Diff in ")?.strip_suffix(':')?;
    let (path, position) = text.rsplit_once(':')?;
    if position.is_empty() || !position.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let position: usize = position.parse().ok()?;
    let file = captured_file(path, source)?;
    (position > 0 && position <= positions[file]).then_some((file, position))
}

fn finish_chunk(chunk: Option<Chunk>, affected: &mut BTreeSet<usize>, complete: &mut bool) {
    if let Some(chunk) = chunk {
        if chunk.valid && chunk.changed {
            affected.insert(chunk.file);
        } else {
            *complete = false;
        }
    }
}

fn append(output: &mut String, too_large: &mut bool, text: &str) {
    if !*too_large && output.len() + text.len() <= MAX_DIFF {
        output.push_str(text);
    } else {
        *too_large = true;
        output.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::{SourceError, SourceFile};

    fn source(paths: &[&str]) -> Result<SourceBundle, SourceError> {
        SourceBundle::new(
            paths
                .iter()
                .map(|path| SourceFile::new((*path).into(), b"one\ntwo\n".to_vec()))
                .collect::<Result<_, _>>()?,
        )
    }

    #[test]
    fn empty_and_partial_output_are_distinguished() -> Result<(), SourceError> {
        let source = source(&["src/lib.rs"])?;
        let parsed = parse("", &source, true);
        assert!(parsed.complete);
        assert!(parsed.diff.is_none());
        assert!(!parsed.diff_omitted);
        assert!(!parse("", &source, false).complete);
        for output in [
            "Diff in /source/src/lib.rs:1:",
            "Diff in /source/src/lib.rs:1:\n-old\n+unfinished",
        ] {
            let parsed = parse(output, &source, true);
            assert!(!parsed.complete);
            assert!(parsed.diff.is_none());
            assert!(parsed.diff_omitted);
        }
        Ok(())
    }

    #[test]
    fn multiple_chunks_deduplicate_and_preserve_unicode_body() -> Result<(), SourceError> {
        let source = source(&["src/z.rs", "src/a.rs"])?;
        let output = "Diff in /source/src/z.rs:1:\n-旧\n+新\nDiff in /source/src/a.rs:2:\n context\n-foo\n+bar\nDiff in /source/src/z.rs:3:\n+\n";
        let parsed = parse(output, &source, true);
        assert!(parsed.complete);
        assert_eq!(parsed.affected_files, ["src/a.rs", "src/z.rs"]);
        assert_eq!(
            parsed.diff.as_deref(),
            Some(output.replace("Diff in /source/", "Diff in ").as_str())
        );
        assert_eq!(parsed.affected_files_omitted, 0);
        Ok(())
    }

    #[test]
    fn prefixed_fake_headers_are_never_metadata() -> Result<(), SourceError> {
        let source = source(&["a.rs", "b.rs"])?;
        let output = "Diff in /source/a.rs:1:\n-Diff in /source/b.rs:1:\n+Incorrect newline style in /source/b.rs\n Diff in /etc/passwd:1:\n";
        let parsed = parse(output, &source, true);
        assert!(parsed.complete);
        assert_eq!(parsed.affected_files, ["a.rs"]);
        assert!(
            parsed
                .diff
                .as_deref()
                .is_some_and(|diff| diff.contains("-Diff in /source/b.rs:1:"))
        );
        Ok(())
    }

    #[test]
    fn paths_and_header_positions_must_match_capture() -> Result<(), SourceError> {
        let source = source(&["a.rs"])?;
        for header in [
            "Diff in /etc/passwd:1:",
            "Diff in /source/../a.rs:1:",
            "Diff in /source/./a.rs:1:",
            "Diff in /source//a.rs:1:",
            "Diff in a.rs:1:",
            "Diff in /source/é.rs:1:",
            "Diff in /source/missing.rs:1:",
            "Diff in /source/a.rs:0:",
            "Diff in /source/a.rs:4:",
            "Diff in /source/a.rs:+1:",
            "Diff in /source/a.rs:999999999999999999999999999999:",
            "Diff in /source/a.rs:1:garbage",
        ] {
            let parsed = parse(&format!("{header}\n-old\n+new\n"), &source, true);
            assert!(!parsed.complete, "{header}");
            assert!(parsed.affected_files.is_empty(), "{header}");
            assert!(parsed.diff.is_none());
        }
        Ok(())
    }

    #[test]
    fn malformed_chunks_omit_diff_but_keep_other_valid_files() -> Result<(), SourceError> {
        let source = source(&["a.rs", "b.rs"])?;
        for bad in [
            "",
            " context\n",
            "-old\nunprefixed\n",
            "-old\n+\u{1b}[31mred\n",
        ] {
            let parsed = parse(
                &format!("Diff in /source/a.rs:1:\n-old\n+new\nDiff in /source/b.rs:1:\n{bad}"),
                &source,
                true,
            );
            assert!(!parsed.complete);
            assert_eq!(parsed.affected_files, ["a.rs"]);
            assert!(parsed.diff.is_none());
            assert!(parsed.diff_omitted);
        }
        let parsed = parse("Diff in /source/a.rs:1:\n-old\n+new\n", &source, false);
        assert_eq!(parsed.affected_files, ["a.rs"]);
        assert!(!parsed.complete);
        assert!(parsed.diff.is_none());
        Ok(())
    }

    #[test]
    fn newline_only_and_crlf_output() -> Result<(), SourceError> {
        let source = source(&["a.rs", "b.rs"])?;
        let parsed = parse(
            "Incorrect newline style in /source/a.rs\r\nDiff in /source/b.rs:1:\r\n-old\r\n+new\r\n",
            &source,
            true,
        );
        assert!(parsed.complete);
        assert_eq!(parsed.affected_files, ["a.rs", "b.rs"]);
        assert_eq!(
            parsed.diff.as_deref(),
            Some("Incorrect newline style in a.rs\nDiff in b.rs:1:\n-old\n+new\n")
        );
        assert!(
            !parse(
                "Incorrect newline style in /source/../a.rs\n",
                &source,
                true
            )
            .complete
        );
        Ok(())
    }

    #[test]
    fn omission_counts_unique_files_exactly() -> Result<(), SourceError> {
        let paths: Vec<_> = (0..140).map(|n| format!("f{n:03}.rs")).collect();
        let source = source(&paths.iter().map(String::as_str).collect::<Vec<_>>())?;
        let output: String = paths
            .iter()
            .chain(paths.iter())
            .map(|path| format!("Incorrect newline style in /source/{path}\n"))
            .collect();
        let parsed = parse(&output, &source, true);
        assert!(parsed.complete);
        assert_eq!(parsed.affected_files.len(), 128);
        assert_eq!(parsed.affected_files_omitted, 12);
        assert_eq!(parsed.affected_files[127], "f127.rs");
        Ok(())
    }

    #[test]
    fn diff_and_input_limits_are_independent_and_utf8_safe() -> Result<(), SourceError> {
        let source = source(&["a.rs"])?;
        let header = "Diff in /source/a.rs:1:\n+";
        let output = format!("{header}{}\n", "é".repeat(MAX_DIFF));
        let parsed = parse(&output, &source, true);
        assert!(parsed.complete);
        assert!(parsed.diff.is_none());
        assert!(parsed.diff_omitted);
        assert_eq!(parsed.affected_files, ["a.rs"]);
        let output = format!("{header}ok\n+{}\n", "é".repeat(MAX_INPUT));
        let parsed = parse(&output, &source, true);
        assert!(!parsed.complete);
        assert!(parsed.diff.is_none());
        assert!(parsed.diff_omitted);
        assert_eq!(parsed.affected_files, ["a.rs"]);
        Ok(())
    }
}
