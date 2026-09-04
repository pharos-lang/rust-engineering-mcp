//! Bounded Cargo JSON-lines normalization; no source or suggestion authorizes I/O.
//! Format references: Cargo's external-tools JSON messages and rustc's JSON
//! diagnostics, verified against the pinned Cargo/rustc 1.98.1 interface.
use rust_engineering_application::InspectionError;
use rust_engineering_domain::{
    Applicability, ByteRange, Diagnostic, DiagnosticSource, NonEmptyText, Position, Replacement,
    Severity, SourceBundle, SourceSpan, Suggestion, validate_source_path,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

const MAX_INPUT: usize = 256 * 1024;
const MAX_DIAGNOSTICS: usize = 128;
const MAX_OUTPUT: usize = 128 * 1024;
const MAX_SPANS: usize = 32;
const MAX_EDITS: usize = 16;
const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 256;
const MAX_TEXT: usize = 4096;

pub(super) struct ParsedCheck {
    pub diagnostics: Vec<Diagnostic>,
    pub diagnostics_omitted: u64,
    pub build_finished: Option<bool>,
    pub complete: bool,
}

#[derive(Deserialize)]
#[serde(tag = "reason")]
enum Event {
    #[serde(rename = "compiler-message")]
    Message { message: RawDiagnostic },
    #[serde(rename = "compiler-artifact")]
    Artifact {},
    #[serde(rename = "build-script-executed")]
    BuildScript {},
    #[serde(rename = "build-finished")]
    Finished { success: bool },
}
#[derive(Deserialize)]
struct RawCode {
    code: String,
    // Explanation and every unmodeled external field are deliberately ignored.
}
#[derive(Deserialize)]
struct RawDiagnostic {
    message: String,
    code: Option<RawCode>,
    level: String,
    spans: Vec<RawSpan>,
    children: Vec<RawDiagnostic>,
    // rendered duplicates evidence and can contain unchecked absolute paths.
}
#[derive(Deserialize)]
struct RawSpan {
    file_name: String,
    byte_start: u64,
    byte_end: u64,
    line_start: u32,
    line_end: u32,
    column_start: u32,
    column_end: u32,
    is_primary: bool,
    label: Option<String>,
    suggested_replacement: Option<String>,
    suggestion_applicability: Option<String>,
    // text and expansion are not published, recursively followed or interpreted.
}
struct Node<'a> {
    raw: &'a RawDiagnostic,
    source: DiagnosticSource,
    children_truncated: bool,
}

fn captured_file(path: &str, source: &SourceBundle) -> Option<usize> {
    let relative = path.strip_prefix("/source/").unwrap_or(path);
    validate_source_path(relative).ok()?;
    source
        .files()
        .binary_search_by(|file| file.path().cmp(relative))
        .ok()
}

/// Only requested offsets are retained. Scan each referenced source file once,
/// avoiding a quadratic scan for many spans on one long Unicode source line.
fn positions(nodes: &[Node<'_>], source: &SourceBundle) -> BTreeMap<(usize, u64), Position> {
    let mut requested: BTreeMap<usize, BTreeSet<u64>> = BTreeMap::new();
    for node in nodes {
        for span in node.raw.spans.iter().take(MAX_SPANS) {
            if let Some(file) = captured_file(&span.file_name, source) {
                let offsets = requested.entry(file).or_default();
                offsets.insert(span.byte_start);
                offsets.insert(span.byte_end);
            }
        }
    }
    let mut positions = BTreeMap::new();
    for (file, offsets) in requested {
        let Ok(text) = std::str::from_utf8(source.files()[file].bytes()) else {
            continue;
        };
        let (mut line, mut column) = (1, 1);
        for (offset, character) in text.char_indices() {
            if offsets.contains(&(offset as u64))
                && let Ok(position) = Position::new(line, column)
            {
                positions.insert((file, offset as u64), position);
            }
            // SourceBundle caps each file at 1MiB: these counters cannot overflow.
            if character == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        if offsets.contains(&(text.len() as u64))
            && let Ok(position) = Position::new(line, column)
        {
            positions.insert((file, text.len() as u64), position);
        }
    }
    positions
}

fn text(value: &str, truncated: &mut bool) -> Option<NonEmptyText> {
    let mut end = value.len().min(MAX_TEXT);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    if end != value.len() {
        *truncated = true;
    }
    value[..end].parse().ok()
}

fn span(
    raw: &RawSpan,
    source: &SourceBundle,
    positions: &BTreeMap<(usize, u64), Position>,
    truncated: &mut bool,
) -> Option<SourceSpan> {
    let file = captured_file(&raw.file_name, source)?;
    let start = Position::new(raw.line_start, raw.column_start).ok()?;
    let end = Position::new(raw.line_end, raw.column_end).ok()?;
    if positions.get(&(file, raw.byte_start)) != Some(&start)
        || positions.get(&(file, raw.byte_end)) != Some(&end)
    {
        return None;
    }
    let bytes = ByteRange::new(raw.byte_start, raw.byte_end).ok()?;
    let label = raw.label.as_deref().and_then(|value| {
        // rustc uses an empty label for an unlabeled secondary span. This is
        // absence of text, not discarded evidence or malformed source position.
        if value.is_empty() {
            return None;
        }
        let label = text(value, truncated);
        if label.is_none() {
            *truncated = true;
        }
        label
    });
    SourceSpan::new(
        source.files()[file].path().parse().ok()?,
        start,
        end,
        Some(bytes),
        raw.is_primary,
        label,
    )
    .ok()
}

fn applicability(value: Option<&str>) -> Option<Applicability> {
    match value {
        Some("MachineApplicable") => Some(Applicability::MachineApplicable),
        Some("MaybeIncorrect") => Some(Applicability::MaybeIncorrect),
        Some("HasPlaceholders") => Some(Applicability::HasPlaceholders),
        Some("Unspecified") | None => Some(Applicability::Unspecified),
        Some(_) => None,
    }
}

fn overlapping(edits: &[Replacement]) -> bool {
    for (index, left) in edits.iter().enumerate() {
        for right in &edits[index + 1..] {
            if left.span.file() != right.span.file() {
                continue;
            }
            let (Some(left), Some(right)) = (left.span.bytes(), right.span.bytes()) else {
                return true;
            };
            // Duplicate insertions have ambiguous ordering. Otherwise touching
            // endpoints are allowed, but interior overlap is not one safe group.
            if (left.start() == right.start())
                || (left.start() < right.end() && right.start() < left.end())
            {
                return true;
            }
        }
    }
    false
}

fn normalize(
    node: &Node<'_>,
    source: &SourceBundle,
    positions: &BTreeMap<(usize, u64), Position>,
) -> Option<Diagnostic> {
    let raw = node.raw;
    let severity = match raw.level.as_str() {
        "error" | "error: internal compiler error" => Severity::Error,
        "warning" => Severity::Warning,
        "note" | "failure-note" => Severity::Note,
        "help" => Severity::Help,
        _ => return None,
    };
    let mut truncated = node.children_truncated || raw.spans.len() > MAX_SPANS;
    let message = text(&raw.message, &mut truncated)?;
    let code = raw.code.as_ref().and_then(|code| {
        if code.code.len() > 128 || code.code.chars().any(char::is_control) {
            truncated = true;
            None
        } else {
            let code = code.code.parse().ok();
            if code.is_none() {
                truncated = true;
            }
            code
        }
    });
    let mut spans = Vec::new();
    let mut edits = Vec::new();
    let mut group_applicability = None;
    let mut group_valid = raw.spans.len() <= MAX_SPANS;
    let mut has_suggestion = false;
    for raw_span in raw.spans.iter().take(MAX_SPANS) {
        let source_span = span(raw_span, source, positions, &mut truncated);
        if source_span.is_none() {
            truncated = true;
        }
        if let Some(replacement) = &raw_span.suggested_replacement {
            has_suggestion = true;
            let applicable = applicability(raw_span.suggestion_applicability.as_deref());
            match (source_span.as_ref(), applicable) {
                (Some(span), Some(applicable))
                    if replacement.len() <= MAX_TEXT && edits.len() < MAX_EDITS =>
                {
                    group_applicability = Some(match group_applicability {
                        None => applicable,
                        Some(previous) if previous == applicable => applicable,
                        _ => Applicability::Unspecified,
                    });
                    edits.push(Replacement {
                        span: span.clone(),
                        replacement: replacement.clone(),
                    });
                }
                _ => group_valid = false,
            }
        }
        if let Some(span) = source_span {
            spans.push(span);
        }
    }
    // A node's replacement spans form ONE multipart suggestion, never individual
    // independently applicable edits. Dropping any part discards the whole group.
    // Flattening nodes means each Diagnostic has at most one (therefore <=16)
    // suggestion; siblings' alternatives remain separate Diagnostic entries.
    let suggestions = if has_suggestion {
        if group_valid && !overlapping(&edits) {
            match Suggestion::new(
                message.clone(),
                group_applicability.unwrap_or(Applicability::Unspecified),
                edits,
            ) {
                Ok(suggestion) => vec![suggestion],
                Err(_) => {
                    truncated = true;
                    vec![]
                }
            }
        } else {
            truncated = true;
            vec![]
        }
    } else {
        vec![]
    };
    Some(Diagnostic {
        source: node.source,
        severity,
        code,
        message,
        spans,
        rendered: None,
        suggestions,
        truncated,
    })
}

pub(super) fn parse(
    stdout: &str,
    source: &SourceBundle,
    stream_complete: bool,
) -> Result<ParsedCheck, InspectionError> {
    if stdout.len() > MAX_INPUT {
        return Err(InspectionError::OutputLimit);
    }
    let mut result = ParsedCheck {
        diagnostics: vec![],
        diagnostics_omitted: 0,
        build_finished: None,
        complete: stream_complete,
    };
    let mut roots = Vec::new();
    for line in stdout.split_inclusive('\n') {
        // Preserve only complete records. Even syntactically valid JSON without
        // its final LF may have come from a truncated output stream.
        if !line.ends_with('\n') || result.build_finished.is_some() {
            result.complete = false;
            break;
        }
        match serde_json::from_str::<Event>(line) {
            Ok(Event::Message { message }) => roots.push(message),
            Ok(Event::Finished { success }) => result.build_finished = Some(success),
            Ok(Event::Artifact {} | Event::BuildScript {}) => (),
            Err(_) => {
                result.complete = false;
                break;
            }
        }
    }
    if result.build_finished.is_none() {
        result.complete = false;
    }
    let mut stack: Vec<_> = roots
        .iter()
        .rev()
        .map(|node| (node, 0usize, None::<usize>))
        .collect();
    let mut nodes: Vec<Node<'_>> = Vec::new();
    let mut visited = 0;
    while let Some((raw, depth, parent)) = stack.pop() {
        visited += 1;
        let next_parent = if visited > MAX_NODES || depth > MAX_DEPTH {
            result.diagnostics_omitted += 1;
            result.complete = false;
            if let Some(parent) = parent {
                nodes[parent].children_truncated = true;
            }
            parent
        } else {
            // Classify the named lint family, not authenticated producer identity.
            // Child notes/help retain their parent family even without a code.
            let source = parent.map_or_else(
                || {
                    if raw
                        .code
                        .as_ref()
                        .is_some_and(|code| code.code.starts_with("clippy::"))
                    {
                        DiagnosticSource::Clippy
                    } else {
                        DiagnosticSource::Rustc
                    }
                },
                |index| nodes[index].source,
            );
            nodes.push(Node {
                raw,
                source,
                children_truncated: false,
            });
            Some(nodes.len() - 1)
        };
        stack.extend(
            raw.children
                .iter()
                .rev()
                .map(|child| (child, depth + 1, next_parent)),
        );
    }
    let positions = positions(&nodes, source);
    let mut serialized_bytes = 2; // The surrounding diagnostics array brackets.
    for node in &nodes {
        if result.diagnostics.len() >= MAX_DIAGNOSTICS {
            result.diagnostics_omitted += 1;
            result.complete = false;
            continue;
        }
        let Some(diagnostic) = normalize(node, source, &positions) else {
            result.diagnostics_omitted += 1;
            result.complete = false;
            continue;
        };
        if diagnostic.truncated {
            result.complete = false;
        }
        let size = serde_json::to_vec(&diagnostic)
            .map_err(|_| InspectionError::Internal)?
            .len()
            + usize::from(!result.diagnostics.is_empty());
        if serialized_bytes + size > MAX_OUTPUT {
            result.diagnostics_omitted += 1;
            result.complete = false;
            continue;
        }
        serialized_bytes += size;
        result.diagnostics.push(diagnostic);
    }
    if result.diagnostics_omitted > 0
        && let Some(last) = result.diagnostics.last_mut()
    {
        last.truncated = true;
    }
    Ok(result)
}

/// Cargo JSON ends before stable/custom test harness output begins. Keep that
/// untrusted tail in the retained log. Cargo-looking tail events make the phase
/// boundary ambiguous; neither a forged prefix nor a harness can certify it.
pub(super) fn parse_test(
    stdout: &str,
    source: &SourceBundle,
    stream_complete: bool,
) -> Result<ParsedCheck, InspectionError> {
    if stdout.len() > MAX_INPUT {
        return Err(InspectionError::OutputLimit);
    }
    let mut offset = 0;
    for line in stdout.split_inclusive('\n') {
        offset += line.len();
        if !line.ends_with('\n') {
            break;
        }
        if let Ok(Event::Finished { success }) = serde_json::from_str::<Event>(line) {
            let mut parsed = parse(&stdout[..offset], source, stream_complete)?;
            // A failed compile cannot legitimately start a test harness.
            if !success && !stdout[offset..].is_empty() {
                parsed.complete = false;
            }
            if stdout[offset..].lines().any(|line| {
                line.contains("\"reason\"") || serde_json::from_str::<Event>(line).is_ok()
            }) {
                parsed.complete = false;
                parsed.build_finished = None;
            }
            return Ok(parsed);
        }
    }
    parse(stdout, source, stream_complete)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::SourceFile;
    use serde_json::{Value, json};
    type TestResult = Result<(), String>;
    fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> Result<T, String> {
        value.map_err(|error| format!("{error:?}"))
    }
    fn source() -> Result<SourceBundle, String> {
        checked(SourceBundle::new(vec![checked(SourceFile::new(
            "src/lib.rs".into(),
            "aé🦀z\nnext\n".as_bytes().to_vec(),
        ))?]))
    }
    fn span_json() -> Value {
        json!({"file_name":"src/lib.rs","byte_start":1,"byte_end":3,
            "line_start":1,"line_end":1,"column_start":2,"column_end":3,
            "is_primary":true,"label":null,"suggested_replacement":null,
            "suggestion_applicability":null,"text":[{"text":"unchecked source text"}],
            "expansion":{"span":{"file_name":"/secret/host"}}})
    }
    fn message() -> Value {
        json!({"message":"unused variable","code":{"code":"unused_variables","explanation":"never publish"},
            "level":"warning","spans":[span_json()],"children":[],
            "rendered":"unsafe rendered /private/secret"})
    }
    fn event(message: Value) -> Value {
        json!({"reason":"compiler-message","package_id":"opaque/package","message":message})
    }
    fn finished(success: bool) -> Value {
        json!({"reason":"build-finished","success":success})
    }
    fn lines(events: &[Value]) -> Result<String, String> {
        let mut text = String::new();
        for event in events {
            text.push_str(&checked(serde_json::to_string(event))?);
            text.push('\n');
        }
        Ok(text)
    }
    fn parse_events(events: &[Value]) -> Result<ParsedCheck, String> {
        checked(parse(&lines(events)?, &source()?, true))
    }

    #[test]
    fn test_harness_tail_is_not_reparsed_as_cargo_or_coverage() -> TestResult {
        let prefix = lines(&[event(message()), finished(true)])?;
        // Ordinary harness text is not interpreted as structured coverage.
        for tail in [
            "",
            "running 0 tests\n",
            "not JSON\n{\"user_data\":true}\n",
            "unclosed text",
        ] {
            let parsed = checked(parse_test(&(prefix.clone() + tail), &source()?, true))?;
            assert!(parsed.complete);
            assert_eq!(parsed.build_finished, Some(true));
            assert_eq!(parsed.diagnostics.len(), 1);
            let partial = checked(parse_test(&(prefix.clone() + tail), &source()?, false))?;
            assert!(!partial.complete);
            assert_eq!(partial.build_finished, Some(true));
        }
        assert!(!checked(parse(&(prefix + "running 0 tests\n"), &source()?, true))?.complete);
        Ok(())
    }
    #[test]
    fn test_forged_phase_or_cargo_looking_harness_tail_never_certifies_completion() -> TestResult {
        for tail in [
            lines(&[event(message()), finished(false)])?,
            lines(&[finished(true)])?,
            "{\"reason\":\"compiler-artifact\"}".into(),
            "garbage{\"reason\":\"build-finished\",\"success\":false}\n".into(),
            "{\"reason\" : \"unknown-future-event\"}\n".into(),
        ] {
            let text = lines(&[finished(true)])? + &tail;
            let parsed = checked(parse_test(&text, &source()?, true))?;
            assert!(!parsed.complete);
            assert_eq!(parsed.build_finished, None);
        }
        Ok(())
    }
    #[test]
    fn test_compile_failure_requires_ended_cargo_stream_and_preserves_partial_diagnostics()
    -> TestResult {
        let prefix = lines(&[event(message()), finished(false)])?;
        let parsed = checked(parse_test(&prefix, &source()?, true))?;
        assert!(parsed.complete);
        assert_eq!(parsed.build_finished, Some(false));
        assert!(
            !checked(parse_test(
                &(prefix + "unexpected tail\n"),
                &source()?,
                true
            ))?
            .complete
        );
        for tail in [
            "",
            "not JSON\n",
            "{\"reason\":\"build-finished\",\"success\":true}",
        ] {
            let prefix = lines(&[event(message())])?;
            let parsed = checked(parse_test(&(prefix + tail), &source()?, true))?;
            assert!(!parsed.complete);
            assert_eq!(parsed.diagnostics.len(), 1);
        }
        let bad_prefix = "bad\n".to_owned() + &lines(&[finished(true)])?;
        assert!(!checked(parse_test(&bad_prefix, &source()?, true))?.complete);
        assert!(matches!(
            parse_test(&"x".repeat(MAX_INPUT + 1), &source()?, true),
            Err(InspectionError::OutputLimit)
        ));
        Ok(())
    }
    #[test]
    fn clippy_lint_family_includes_code_less_child_suggestions() -> TestResult {
        let mut lint = message();
        lint["code"]["code"] = json!("clippy::needless_return");
        let mut help = message();
        help["level"] = json!("help");
        help["code"] = Value::Null;
        help["spans"][0]["suggested_replacement"] = json!("x");
        help["spans"][0]["suggestion_applicability"] = json!("MachineApplicable");
        lint["children"] = json!([help]);
        let mut code_less_root = message();
        code_less_root["code"] = Value::Null;
        let parsed = parse_events(&[
            event(lint),
            event(message()),
            event(code_less_root),
            finished(true),
        ])?;
        assert!(parsed.complete);
        assert_eq!(parsed.diagnostics.len(), 4);
        assert_eq!(parsed.diagnostics[0].source, DiagnosticSource::Clippy);
        assert_eq!(parsed.diagnostics[1].source, DiagnosticSource::Clippy);
        assert_eq!(parsed.diagnostics[1].suggestions.len(), 1);
        assert_eq!(parsed.diagnostics[2].source, DiagnosticSource::Rustc);
        assert_eq!(parsed.diagnostics[3].source, DiagnosticSource::Rustc);
        assert!(parsed.diagnostics[3].code.is_none());
        Ok(())
    }
    #[test]
    fn empty_secondary_label_is_absence_without_incomplete_evidence() -> TestResult {
        let mut raw = message();
        raw["spans"][0]["label"] = json!("");
        let parsed = parse_events(&[event(raw), finished(false)])?;
        assert!(parsed.complete);
        assert!(!parsed.diagnostics[0].truncated);
        assert!(parsed.diagnostics[0].spans[0].label().is_none());
        Ok(())
    }
    #[test]
    fn successful_and_failed_finished_events_retain_real_severities() -> TestResult {
        for success in [false, true] {
            let parsed = parse_events(&[event(message()), finished(success)])?;
            assert!(parsed.complete);
            assert_eq!(parsed.build_finished, Some(success));
            assert_eq!(parsed.diagnostics_omitted, 0);
            assert_eq!(parsed.diagnostics.len(), 1);
            let diagnostic = &parsed.diagnostics[0];
            assert_eq!(diagnostic.source, DiagnosticSource::Rustc);
            assert_eq!(diagnostic.severity, Severity::Warning);
            assert_eq!(
                diagnostic.code.as_ref().map(NonEmptyText::as_str),
                Some("unused_variables")
            );
            assert_eq!(diagnostic.rendered, None);
            let serialized = checked(serde_json::to_string(diagnostic))?;
            for excluded in [
                "/private/secret",
                "/secret/host",
                "never publish",
                "unchecked source text",
            ] {
                assert!(!serialized.contains(excluded));
            }
        }
        let clean = parse_events(&[
            json!({"reason":"compiler-artifact","future_field":[1,2]}),
            json!({"reason":"build-script-executed","env":[["IGNORED","secret"]]}),
            finished(true),
        ])?;
        assert!(clean.complete && clean.diagnostics.is_empty());
        for (level, severity) in [
            ("error", Severity::Error),
            ("warning", Severity::Warning),
            ("note", Severity::Note),
            ("help", Severity::Help),
            ("failure-note", Severity::Note),
            ("error: internal compiler error", Severity::Error),
        ] {
            let mut node = message();
            node["level"] = json!(level);
            let parsed = parse_events(&[event(node), finished(false)])?;
            assert_eq!(parsed.diagnostics[0].severity, severity);
        }
        Ok(())
    }

    #[test]
    fn unicode_columns_and_exclusive_byte_ranges_match_captured_source() -> TestResult {
        for path in ["src/lib.rs", "/source/src/lib.rs"] {
            let mut node = message();
            node["spans"][0]["file_name"] = json!(path);
            let parsed = parse_events(&[event(node), finished(true)])?;
            assert!(parsed.complete);
            let span = &parsed.diagnostics[0].spans[0];
            assert_eq!(span.file().as_str(), "src/lib.rs");
            assert_eq!(span.start(), checked(Position::new(1, 2))?);
            assert_eq!(span.end(), checked(Position::new(1, 3))?);
            assert_eq!(span.bytes(), Some(checked(ByteRange::new(1, 3))?));
        }
        let mut node = message();
        node["spans"][0] = json!({"file_name":"src/lib.rs","byte_start":7,"byte_end":13,
            "line_start":1,"line_end":2,"column_start":4,"column_end":5,"is_primary":false});
        assert!(parse_events(&[event(node), finished(true)])?.complete);
        let mut node = message();
        node["spans"][0] = json!({"file_name":"src/lib.rs","byte_start":14,"byte_end":14,
            "line_start":3,"line_end":3,"column_start":1,"column_end":1,"is_primary":true});
        assert!(parse_events(&[event(node), finished(true)])?.complete);
        Ok(())
    }

    #[test]
    fn malformed_unknown_duplicate_and_partial_records_preserve_only_valid_prefix() -> TestResult {
        let prefix = lines(&[event(message())])?;
        for bad in [
            "{\n",
            "plain proc macro output\n",
            "[]\n",
            "null\n",
            "{\"reason\":\"new-event\"}\n",
            "{\"reason\":\"build-finished\",\"success\":null}\n",
            "{\"reason\":\"compiler-message\",\"message\":{}}\n",
        ] {
            let input = format!("{prefix}{bad}{}", lines(&[finished(true)])?);
            let parsed = checked(parse(&input, &source()?, true))?;
            assert!(!parsed.complete, "{bad}");
            assert_eq!(parsed.diagnostics.len(), 1);
            assert_eq!(parsed.build_finished, None);
        }
        let mut no_lf = format!("{prefix}{}", lines(&[finished(true)])?);
        no_lf.pop();
        let parsed = checked(parse(&no_lf, &source()?, true))?;
        assert!(!parsed.complete && parsed.build_finished.is_none());
        assert_eq!(parsed.diagnostics.len(), 1);
        for tail in [finished(true), finished(false), event(message())] {
            let parsed = parse_events(&[event(message()), finished(true), tail])?;
            assert!(!parsed.complete);
            assert_eq!(parsed.build_finished, Some(true));
            assert_eq!(parsed.diagnostics.len(), 1);
        }
        let parsed = checked(parse(
            &lines(&[event(message()), finished(true)])?,
            &source()?,
            false,
        ))?;
        assert!(!parsed.complete && parsed.build_finished == Some(true));
        assert!(!checked(parse("", &source()?, true))?.complete);
        assert!(!parse_events(&[event(message())])?.complete);
        Ok(())
    }

    #[test]
    fn external_generated_escaped_and_unknown_paths_never_enter_spans() -> TestResult {
        for path in [
            "/etc/passwd",
            "/source/../outside",
            "/source//src/lib.rs",
            "./src/lib.rs",
            "../src/lib.rs",
            "src\\lib.rs",
            "src/./lib.rs",
            "src//lib.rs",
            "missing.rs",
            "",
            "/work/target/generated.rs",
            "/rustc/hash/library/core/src/lib.rs",
            "<macro expansion>",
        ] {
            let mut node = message();
            node["spans"][0]["file_name"] = json!(path);
            let parsed = parse_events(&[event(node), finished(true)])?;
            assert!(!parsed.complete, "{path}");
            assert!(parsed.diagnostics[0].truncated);
            assert!(parsed.diagnostics[0].spans.is_empty());
            assert_eq!(parsed.diagnostics[0].rendered, None);
        }
        Ok(())
    }

    #[test]
    fn invalid_and_misaligned_positions_are_omitted_without_losing_message() -> TestResult {
        for (field, value) in [
            ("line_start", 0),
            ("column_start", 0),
            ("line_end", 0),
            ("column_end", 1),
            ("byte_start", 2),
            ("byte_end", 2),
            ("byte_end", 9999),
            ("line_end", 3),
            ("column_end", 4),
            ("byte_start", 8),
        ] {
            let mut node = message();
            node["spans"][0][field] = json!(value);
            let parsed = parse_events(&[event(node), finished(false)])?;
            assert!(!parsed.complete, "{field}={value}");
            assert_eq!(parsed.diagnostics.len(), 1);
            assert!(parsed.diagnostics[0].spans.is_empty());
            assert_eq!(parsed.diagnostics[0].message.as_str(), "unused variable");
        }
        let binary = checked(SourceBundle::new(vec![checked(SourceFile::new(
            "src/lib.rs".into(),
            vec![0xff; 4],
        ))?]))?;
        let parsed = checked(parse(
            &lines(&[event(message()), finished(false)])?,
            &binary,
            true,
        ))?;
        assert!(!parsed.complete && parsed.diagnostics[0].spans.is_empty());
        Ok(())
    }

    fn multipart() -> Value {
        let mut node = message();
        node["message"] = json!("replace both fragments");
        node["level"] = json!("help");
        node["spans"][0]["suggested_replacement"] = json!("é_renamed");
        node["spans"][0]["suggestion_applicability"] = json!("MachineApplicable");
        let mut second = span_json();
        second["byte_start"] = json!(7);
        second["byte_end"] = json!(8);
        second["column_start"] = json!(4);
        second["column_end"] = json!(5);
        second["suggested_replacement"] = json!("");
        second["suggestion_applicability"] = json!("MachineApplicable");
        node["spans"] = json!([node["spans"][0].clone(), second]);
        node
    }

    #[test]
    fn multipart_suggestions_stay_grouped_per_flattened_child_node() -> TestResult {
        let mut root = message();
        root["children"] = json!([multipart(), multipart()]);
        let parsed = parse_events(&[event(root), finished(false)])?;
        assert!(parsed.complete);
        assert_eq!(parsed.diagnostics.len(), 3);
        assert!(parsed.diagnostics[0].suggestions.is_empty());
        for diagnostic in &parsed.diagnostics[1..] {
            assert_eq!(diagnostic.suggestions.len(), 1);
            let suggestion = &diagnostic.suggestions[0];
            assert_eq!(suggestion.applicability(), Applicability::MachineApplicable);
            assert_eq!(suggestion.edits().len(), 2);
            assert_eq!(suggestion.edits()[0].replacement, "é_renamed");
            assert_eq!(suggestion.edits()[1].replacement, "");
        }
        let mut node = multipart();
        node["spans"][1]["suggestion_applicability"] = json!("MaybeIncorrect");
        let parsed = parse_events(&[event(node), finished(false)])?;
        assert_eq!(
            parsed.diagnostics[0].suggestions[0].applicability(),
            Applicability::Unspecified
        );
        Ok(())
    }

    #[test]
    fn any_missing_invalid_overlapping_or_oversize_edit_discards_the_whole_group() -> TestResult {
        for (field, value) in [
            ("file_name", json!("/work/generated.rs")),
            ("suggested_replacement", json!("x".repeat(MAX_TEXT + 1))),
            ("suggestion_applicability", json!("FutureUnknownConfidence")),
            ("byte_end", json!(999)),
        ] {
            let mut node = multipart();
            node["spans"][1][field] = value;
            let parsed = parse_events(&[event(node), finished(false)])?;
            assert!(!parsed.complete);
            assert!(parsed.diagnostics[0].truncated);
            assert!(parsed.diagnostics[0].suggestions.is_empty());
        }
        let mut overlap = multipart();
        overlap["spans"][1] = overlap["spans"][0].clone();
        let parsed = parse_events(&[event(overlap), finished(false)])?;
        assert!(parsed.diagnostics[0].suggestions.is_empty());
        assert!(!parsed.complete);
        Ok(())
    }

    #[test]
    fn message_and_span_limits_are_explicit_and_utf8_safe() -> TestResult {
        let mut node = message();
        node["message"] = json!("🦀".repeat(1025));
        node["spans"] = json!(vec![span_json(); MAX_SPANS + 1]);
        let parsed = parse_events(&[event(node), finished(true)])?;
        assert!(!parsed.complete);
        assert_eq!(parsed.diagnostics[0].message.as_str().len(), MAX_TEXT);
        assert_eq!(parsed.diagnostics[0].spans.len(), MAX_SPANS);
        // Distinct adjacent edits prove the edit-count guard independently of
        // the overlap guard, and exactly sixteen parts remain one suggestion.
        let ascii = checked(SourceBundle::new(vec![checked(SourceFile::new(
            "src/lib.rs".into(),
            vec![b'x'; 20],
        ))?]))?;
        for count in [MAX_EDITS, MAX_EDITS + 1] {
            let mut node = message();
            node["spans"] = Value::Array((0..count).map(|index| json!({
                "file_name":"src/lib.rs", "byte_start":index, "byte_end":index + 1,
                "line_start":1, "line_end":1, "column_start":index + 1,
                "column_end":index + 2, "is_primary":false,
                "suggested_replacement":"y", "suggestion_applicability":"MachineApplicable"
            })).collect());
            let parsed = checked(parse(
                &lines(&[event(node), finished(false)])?,
                &ascii,
                true,
            ))?;
            if count == MAX_EDITS {
                assert!(parsed.complete);
                assert_eq!(
                    parsed.diagnostics[0].suggestions[0].edits().len(),
                    MAX_EDITS
                );
            } else {
                assert!(!parsed.complete && parsed.diagnostics[0].suggestions.is_empty());
            }
        }
        for field in ["message", "level"] {
            let mut node = message();
            node[field] = json!("");
            let parsed = parse_events(&[event(node), finished(false)])?;
            assert_eq!(parsed.diagnostics_omitted, 1);
            assert!(parsed.diagnostics.is_empty() && !parsed.complete);
        }
        Ok(())
    }

    #[test]
    fn diagnostic_count_node_depth_and_aggregate_json_budgets_never_claim_complete() -> TestResult {
        let mut short = message();
        short["spans"] = json!([]);
        let mut root = short.clone();
        root["children"] = json!(vec![short.clone(); MAX_NODES + 20]);
        let parsed = parse_events(&[event(root), finished(true)])?;
        assert_eq!(parsed.diagnostics.len(), MAX_DIAGNOSTICS);
        assert_eq!(
            parsed.diagnostics_omitted,
            (MAX_NODES + 21 - MAX_DIAGNOSTICS) as u64
        );
        assert!(!parsed.complete);
        // Invalid levels consume node work without filling output slots: this
        // distinguishes MAX_NODES from the separate 128-diagnostic output cap.
        let mut unsupported = short.clone();
        unsupported["level"] = json!("future-level");
        let mut children = vec![unsupported; MAX_NODES - 2];
        children.extend(vec![short.clone(); 20]);
        let mut root = short.clone();
        root["children"] = json!(children);
        let parsed = parse_events(&[event(root), finished(true)])?;
        assert_eq!(parsed.diagnostics.len(), 2);
        assert!(!parsed.complete);
        let mut deep = short.clone();
        for _ in 0..(MAX_DEPTH + 2) {
            let mut parent = short.clone();
            parent["children"] = json!([deep]);
            deep = parent;
        }
        let parsed = parse_events(&[event(deep), finished(true)])?;
        assert_eq!(parsed.diagnostics.len(), MAX_DEPTH + 1);
        assert_eq!(parsed.diagnostics_omitted, 2);
        assert!(!parsed.complete);
        let mut large = message();
        large["message"] = json!("m".repeat(MAX_TEXT));
        large["spans"][0]["suggested_replacement"] = json!("r".repeat(MAX_TEXT));
        large["spans"][0]["suggestion_applicability"] = json!("MachineApplicable");
        let mut events = vec![event(large); 28];
        events.push(finished(true));
        let input = lines(&events)?;
        assert!(input.len() < MAX_INPUT);
        let parsed = checked(parse(&input, &source()?, true))?;
        assert!(!parsed.complete && parsed.diagnostics_omitted > 0);
        assert!(checked(serde_json::to_vec(&parsed.diagnostics))?.len() <= MAX_OUTPUT);
        Ok(())
    }

    #[test]
    fn excessive_input_and_parser_recursion_fail_closed_without_dropping_prior_records()
    -> TestResult {
        assert!(matches!(
            parse(&" ".repeat(MAX_INPUT + 1), &source()?, true),
            Err(InspectionError::OutputLimit)
        ));
        let prefix = lines(&[event(message())])?;
        let too_deep = format!(
            "{prefix}{{\"reason\":\"compiler-artifact\",\"ignored\":{}0{}}}\n",
            "[".repeat(150),
            "]".repeat(150)
        );
        let parsed = checked(parse(&too_deep, &source()?, true))?;
        assert!(!parsed.complete);
        assert_eq!(parsed.diagnostics.len(), 1);
        Ok(())
    }
}
