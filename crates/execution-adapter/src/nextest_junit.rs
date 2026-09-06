//! Bounded, hand-rolled streaming JUnit reader for cargo-nextest reports.
//!
//! This is intentionally not a general XML parser. It rejects every `<!...>`
//! construct (DOCTYPE, ENTITY, comments, CDATA) outright, before any content
//! is read past it, so no DTD/entity/external-reference expansion is ever
//! attempted. Only the five predefined XML entities and numeric character
//! references are decoded; both are fixed-size, non-recursive substitutions.
//! Every structural dimension (depth, attribute count/length, text length,
//! input size) has an independent bound. Exceeding any of them, or any
//! malformed structure, yields [`JunitReport::Incomplete`] — never a partial
//! pass. Only the testcase-count dimension degrades gracefully (`rows`
//! stop being itemized once bounded, but `counts` stay exact), matching the
//! existing `diagnostics_omitted` convention.
use rust_engineering_domain::nextest::{NextestOutcomeCounts, NextestTestOutcome, NextestTestRow};

const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_DEPTH: usize = 12;
const MAX_ATTRIBUTES: usize = 16;
const MAX_ATTRIBUTE_LEN: usize = 4096;
const MAX_TEXT_LEN: usize = 65536;
const MAX_TAG_NAME_LEN: usize = 64;
const MAX_TESTCASE_ROWS: usize = 4096;
const MAX_TIME_MS: u64 = 24 * 60 * 60 * 1000;
/// Observed cargo-nextest failure-message substring for a leaked test whose
/// `leak-timeout.result` is configured as `"fail"`. This is a best-effort
/// heuristic pending a real leaky-test JUnit fixture; it is not a stable
/// upstream contract.
const LEAK_FAILURE_MARKER: &str = "leaked handles";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JunitReport {
    Parsed {
        counts: NextestOutcomeCounts,
        rows: Vec<NextestTestRow>,
        rows_omitted: u32,
    },
    /// Malformed document, a hostile/unsupported construct, or any bound
    /// exceeded. Never distinguishes the reason to the caller: a parse
    /// failure is uniformly `Incomplete`, never a partial pass.
    Incomplete,
}

#[derive(Default)]
struct TestcaseMarkers {
    active: bool,
    name: String,
    classname: String,
    time_ms: u64,
    skipped: bool,
    flaky: bool,
    rerun_failed: bool,
    retry_markers: u32,
    failed: bool,
    failure_text_has_leak_marker: bool,
    failure_text_has_timeout_marker: bool,
}

struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Scanner<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }
    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }
    fn advance(&mut self) {
        self.pos += 1;
    }
    fn starts_with(&self, needle: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(needle)
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.advance();
        }
    }
    /// Bounded search for a terminator; returns `false` (parse failure) if
    /// exhausted without finding it within the remaining input.
    fn skip_until(&mut self, terminator: &[u8]) -> bool {
        while self.pos < self.bytes.len() {
            if self.starts_with(terminator) {
                self.pos += terminator.len();
                return true;
            }
            self.advance();
        }
        false
    }
}

fn is_name_byte(b: u8, first: bool) -> bool {
    if first {
        b.is_ascii_alphabetic() || b == b'_'
    } else {
        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')
    }
}

fn read_name(scanner: &mut Scanner<'_>) -> Option<String> {
    let start = scanner.pos;
    if !scanner.peek().is_some_and(|b| is_name_byte(b, true)) {
        return None;
    }
    scanner.advance();
    while scanner.peek().is_some_and(|b| is_name_byte(b, false)) {
        scanner.advance();
        if scanner.pos - start > MAX_TAG_NAME_LEN {
            return None;
        }
    }
    String::from_utf8(scanner.bytes[start..scanner.pos].to_vec()).ok()
}

/// Decodes only the five predefined XML entities and numeric character
/// references. Any other `&...;` sequence, or an unterminated `&`, is a
/// parse failure. There is no custom entity table, so no recursive or
/// exponential expansion is representable.
fn decode_text(raw: &[u8]) -> Option<String> {
    let mut out = String::new();
    let mut i = 0;
    while i < raw.len() {
        if raw[i] != b'&' {
            let start = i;
            while i < raw.len() && raw[i] != b'&' {
                i += 1;
            }
            out.push_str(std::str::from_utf8(&raw[start..i]).ok()?);
            continue;
        }
        let end = raw[i..].iter().position(|&b| b == b';').map(|p| i + p)?;
        if end - i > 12 {
            return None;
        }
        let entity = &raw[i + 1..end];
        let decoded = match entity {
            b"amp" => '&',
            b"lt" => '<',
            b"gt" => '>',
            b"apos" => '\'',
            b"quot" => '"',
            _ => {
                let numeric = entity
                    .strip_prefix(b"#x")
                    .or_else(|| entity.strip_prefix(b"#X"))
                    .map(|hex| (hex, 16))
                    .or_else(|| entity.strip_prefix(b"#").map(|dec| (dec, 10)));
                let (digits, radix) = numeric?;
                let text = std::str::from_utf8(digits).ok()?;
                let code = u32::from_str_radix(text, radix).ok()?;
                char::from_u32(code)?
            }
        };
        out.push(decoded);
        if out.len() > MAX_TEXT_LEN {
            return None;
        }
        i = end + 1;
    }
    if out.len() > MAX_TEXT_LEN {
        return None;
    }
    Some(out)
}

enum Token {
    Start { name: String, self_closing: bool },
    End { name: String },
    Text(String),
}

/// Reads exactly one structural token starting at the scanner's current
/// position. Returns `None` on any malformed or hostile construct. Loops
/// internally (never recurses) so an unbounded run of skippable processing
/// instructions cannot exhaust the call stack.
fn next_token(scanner: &mut Scanner<'_>) -> Option<Option<Token>> {
    loop {
        if scanner.pos >= scanner.bytes.len() {
            return Some(None);
        }
        if scanner.peek() != Some(b'<') {
            let start = scanner.pos;
            while scanner.pos < scanner.bytes.len() && scanner.peek() != Some(b'<') {
                scanner.advance();
                if scanner.pos - start > MAX_TEXT_LEN {
                    return None;
                }
            }
            let text = decode_text(&scanner.bytes[start..scanner.pos])?;
            return Some(Some(Token::Text(text)));
        }
        // Reject every markup-declaration/comment/CDATA/DOCTYPE/ENTITY form
        // before reading past it: no DTD or entity expansion is ever attempted.
        if scanner.peek_at(1) == Some(b'!') {
            return None;
        }
        if scanner.peek_at(1) == Some(b'?') {
            scanner.pos += 2;
            if !scanner.skip_until(b"?>") {
                return None;
            }
            continue;
        }
        break;
    }
    if scanner.peek_at(1) == Some(b'/') {
        scanner.pos += 2;
        let name = read_name(scanner)?;
        scanner.skip_ws();
        if scanner.peek() != Some(b'>') {
            return None;
        }
        scanner.advance();
        return Some(Some(Token::End { name }));
    }
    scanner.advance();
    let name = read_name(scanner)?;
    let mut attribute_count = 0usize;
    loop {
        scanner.skip_ws();
        match scanner.peek() {
            Some(b'/') => {
                scanner.advance();
                if scanner.peek() != Some(b'>') {
                    return None;
                }
                scanner.advance();
                return Some(Some(Token::Start {
                    name,
                    self_closing: true,
                }));
            }
            Some(b'>') => {
                scanner.advance();
                return Some(Some(Token::Start {
                    name,
                    self_closing: false,
                }));
            }
            Some(_) => {
                attribute_count += 1;
                if attribute_count > MAX_ATTRIBUTES {
                    return None;
                }
                let _ = read_name(scanner)?;
                scanner.skip_ws();
                if scanner.peek() != Some(b'=') {
                    return None;
                }
                scanner.advance();
                scanner.skip_ws();
                let quote = scanner.peek()?;
                if quote != b'"' && quote != b'\'' {
                    return None;
                }
                scanner.advance();
                let start = scanner.pos;
                while scanner.peek().is_some_and(|b| b != quote) {
                    scanner.advance();
                    if scanner.pos - start > MAX_ATTRIBUTE_LEN {
                        return None;
                    }
                }
                if scanner.peek() != Some(quote) {
                    return None;
                }
                let _value = decode_text(&scanner.bytes[start..scanner.pos])?;
                scanner.advance();
            }
            None => return None,
        }
    }
}

/// Reads the `name`/`classname`/`time` attributes of a start tag by
/// re-scanning its raw span. Attribute order is not significant; unknown
/// attributes are ignored (still bounded by [`next_token`]'s own caps).
fn testcase_attributes(scanner: &Scanner<'_>, tag_start: usize) -> Option<(String, String, u64)> {
    let mut inner = Scanner::new(scanner.bytes);
    inner.pos = tag_start + 1;
    let _ = read_name(&mut inner)?;
    let mut name = String::new();
    let mut classname = String::new();
    let mut time_ms = 0u64;
    loop {
        inner.skip_ws();
        match inner.peek() {
            Some(b'/') | Some(b'>') => break,
            Some(_) => {
                let key = read_name(&mut inner)?;
                inner.skip_ws();
                if inner.peek() != Some(b'=') {
                    return None;
                }
                inner.advance();
                inner.skip_ws();
                let quote = inner.peek()?;
                inner.advance();
                let start = inner.pos;
                while inner.peek().is_some_and(|b| b != quote) {
                    inner.advance();
                    if inner.pos - start > MAX_ATTRIBUTE_LEN {
                        return None;
                    }
                }
                let value = decode_text(&inner.bytes[start..inner.pos])?;
                inner.advance();
                match key.as_str() {
                    "name" => name = value,
                    "classname" => classname = value,
                    "time" => time_ms = parse_time_ms(&value)?,
                    _ => (),
                }
            }
            None => return None,
        }
    }
    Some((name, classname, time_ms))
}

/// Reads bounded `type`/`message` evidence from an immediate JUnit outcome
/// element. cargo-nextest 0.9.143 represents a leak-fail as a self-closing
/// `<error type="test exited with code 0, but leaked handles ..."/>`, so there
/// is no text node from which to derive the classification.
fn outcome_attribute_markers(scanner: &Scanner<'_>, tag_start: usize) -> Option<(bool, bool)> {
    let mut inner = Scanner::new(scanner.bytes);
    inner.pos = tag_start + 1;
    let _ = read_name(&mut inner)?;
    let mut leak = false;
    let mut timeout = false;
    loop {
        inner.skip_ws();
        match inner.peek() {
            Some(b'/') | Some(b'>') => break,
            Some(_) => {
                let key = read_name(&mut inner)?;
                inner.skip_ws();
                if inner.peek() != Some(b'=') {
                    return None;
                }
                inner.advance();
                inner.skip_ws();
                let quote = inner.peek()?;
                if quote != b'"' && quote != b'\'' {
                    return None;
                }
                inner.advance();
                let start = inner.pos;
                while inner.peek().is_some_and(|byte| byte != quote) {
                    inner.advance();
                    if inner.pos - start > MAX_ATTRIBUTE_LEN {
                        return None;
                    }
                }
                if inner.peek() != Some(quote) {
                    return None;
                }
                let value = decode_text(&inner.bytes[start..inner.pos])?;
                inner.advance();
                if key == "type" || key == "message" {
                    leak |= value.contains(LEAK_FAILURE_MARKER);
                    timeout |= value.to_ascii_lowercase().contains("timed out");
                }
            }
            None => return None,
        }
    }
    Some((leak, timeout))
}

fn parse_time_ms(value: &str) -> Option<u64> {
    let seconds: f64 = value.parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let ms = seconds * 1000.0;
    if ms > MAX_TIME_MS as f64 {
        return None;
    }
    Some(ms.round() as u64)
}

pub fn parse_junit(input: &[u8]) -> JunitReport {
    if input.len() > MAX_INPUT_BYTES {
        return JunitReport::Incomplete;
    }
    let mut scanner = Scanner::new(input);
    let mut stack: Vec<String> = Vec::new();
    let mut counts = NextestOutcomeCounts::default();
    let mut rows = Vec::new();
    let mut rows_omitted = 0u32;
    let mut current = TestcaseMarkers::default();
    // Non-testcase failure/error/skipped context depth: only markers on the
    // immediate children of `<testcase>` are meaningful signals; anything
    // recorded while nested deeper (e.g. text inside a `<system-out>`) must
    // not itself flip a marker.
    let mut marker_depth: Option<usize> = None;

    loop {
        if stack.len() > MAX_DEPTH {
            return JunitReport::Incomplete;
        }
        let tag_start = scanner.pos;
        let token = match next_token(&mut scanner) {
            Some(token) => token,
            None => return JunitReport::Incomplete,
        };
        let Some(token) = token else {
            break;
        };
        match token {
            Token::Text(text) => {
                if current.active && marker_depth == Some(stack.len()) {
                    current.failure_text_has_leak_marker |= text.contains(LEAK_FAILURE_MARKER);
                    current.failure_text_has_timeout_marker |=
                        text.to_ascii_lowercase().contains("timed out");
                }
            }
            Token::Start { name, self_closing } => {
                if name == "testcase" {
                    if current.active {
                        return JunitReport::Incomplete;
                    }
                    let (test_name, classname, time_ms) =
                        match testcase_attributes(&scanner, tag_start) {
                            Some(v) => v,
                            None => return JunitReport::Incomplete,
                        };
                    current = TestcaseMarkers {
                        active: true,
                        name: test_name,
                        classname,
                        time_ms,
                        ..Default::default()
                    };
                    if self_closing {
                        finish_testcase(&mut current, &mut counts, &mut rows, &mut rows_omitted);
                        continue;
                    }
                } else if current.active
                    && marker_depth.is_none()
                    && stack.last().map(String::as_str) == Some("testcase")
                {
                    // Record the stack depth children of this marker element
                    // will see (i.e. one past its own, not-yet-pushed depth).
                    marker_depth = Some(stack.len() + 1);
                    match name.as_str() {
                        "skipped" => current.skipped = true,
                        "flakyFailure" | "flakyError" => {
                            current.flaky = true;
                            current.retry_markers += 1;
                        }
                        "rerunFailure" | "rerunError" => {
                            current.rerun_failed = true;
                            current.retry_markers += 1;
                        }
                        "failure" | "error" => {
                            current.failed = true;
                            let Some((leak, timeout)) =
                                outcome_attribute_markers(&scanner, tag_start)
                            else {
                                return JunitReport::Incomplete;
                            };
                            current.failure_text_has_leak_marker |= leak;
                            current.failure_text_has_timeout_marker |= timeout;
                        }
                        _ => marker_depth = None,
                    }
                }
                if !self_closing {
                    stack.push(name);
                    if stack.len() > MAX_DEPTH {
                        return JunitReport::Incomplete;
                    }
                }
            }
            Token::End { name } => {
                let Some(top) = stack.pop() else {
                    return JunitReport::Incomplete;
                };
                if top != name {
                    return JunitReport::Incomplete;
                }
                if marker_depth == Some(stack.len() + 1) {
                    marker_depth = None;
                }
                if name == "testcase" && current.active {
                    finish_testcase(&mut current, &mut counts, &mut rows, &mut rows_omitted);
                }
            }
        }
    }
    if !stack.is_empty() || current.active {
        return JunitReport::Incomplete;
    }
    JunitReport::Parsed {
        counts,
        rows,
        rows_omitted,
    }
}

fn finish_testcase(
    current: &mut TestcaseMarkers,
    counts: &mut NextestOutcomeCounts,
    rows: &mut Vec<NextestTestRow>,
    rows_omitted: &mut u32,
) {
    let outcome = if current.skipped {
        counts.skipped += 1;
        NextestTestOutcome::Skipped
    } else if current.flaky {
        counts.flaky += 1;
        NextestTestOutcome::Flaky
    } else if current.failed || current.rerun_failed {
        if current.failure_text_has_leak_marker {
            counts.leaky += 1;
            NextestTestOutcome::Leaky
        } else if current.failure_text_has_timeout_marker {
            counts.timed_out += 1;
            NextestTestOutcome::TimedOut
        } else {
            counts.failed += 1;
            NextestTestOutcome::Failed
        }
    } else {
        counts.passed += 1;
        NextestTestOutcome::Passed
    };
    counts.retried += current.retry_markers;
    let mut finished = std::mem::take(current);
    finished.active = false;
    if rows.len() >= MAX_TESTCASE_ROWS {
        *rows_omitted += 1;
        return;
    }
    let row = u16::try_from(finished.retry_markers + 1)
        .ok()
        .and_then(|attempts| {
            NextestTestRow::new_with_attempts(
                finished.name,
                finished.classname,
                outcome,
                attempts,
                finished.time_ms,
            )
            .ok()
        });
    match row {
        Some(row) => rows.push(row),
        None => *rows_omitted += 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_by_outcome(rows: &[NextestTestRow], outcome: NextestTestOutcome) -> Vec<&str> {
        rows.iter()
            .filter(|r| r.outcome() == outcome)
            .map(NextestTestRow::name)
            .collect()
    }

    fn parsed(
        input: &[u8],
    ) -> Result<(NextestOutcomeCounts, Vec<NextestTestRow>, u32), &'static str> {
        match parse_junit(input) {
            JunitReport::Parsed {
                counts,
                rows,
                rows_omitted,
            } => Ok((counts, rows, rows_omitted)),
            JunitReport::Incomplete => Err("expected a parsed report"),
        }
    }

    #[test]
    fn official_docs_sample_parses_to_exact_counts_and_classifications() -> Result<(), &'static str>
    {
        let xml = include_str!("../tests/fixtures/nextest-junit-docs-sample.xml");
        let (counts, rows, rows_omitted) = parsed(xml.as_bytes())?;
        assert_eq!(rows_omitted, 0);
        assert_eq!(
            counts,
            NextestOutcomeCounts {
                passed: 1,
                failed: 1,
                skipped: 0,
                retried: 5,
                flaky: 1,
                leaky: 0,
                timed_out: 0,
            }
        );
        assert_eq!(
            rows_by_outcome(&rows, NextestTestOutcome::Passed),
            ["test_cwd"]
        );
        assert_eq!(
            rows_by_outcome(&rows, NextestTestOutcome::Failed),
            ["test_failure_assert"]
        );
        assert_eq!(
            rows_by_outcome(&rows, NextestTestOutcome::Flaky),
            ["test_flaky_mod_4"]
        );
        assert_eq!(rows[1].attempts(), 3);
        assert_eq!(rows[2].attempts(), 4);
        Ok(())
    }

    #[test]
    fn two_pass_one_fail_positive_control() -> Result<(), &'static str> {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites name="nextest-run" tests="3" failures="1" errors="0">
  <testsuite name="pkg::suite" tests="3" disabled="0" errors="0" failures="1">
    <testcase name="a" classname="pkg::suite" time="0.001"></testcase>
    <testcase name="b" classname="pkg::suite" time="0.002"></testcase>
    <testcase name="c" classname="pkg::suite" time="0.003">
      <failure type="test failure">boom</failure>
    </testcase>
  </testsuite>
</testsuites>"#;
        let (counts, rows, _) = parsed(xml.as_bytes())?;
        assert_eq!(counts.passed, 2);
        assert_eq!(counts.failed, 1);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].time_ms(), 1);
        assert_eq!(rows[2].outcome(), NextestTestOutcome::Failed);
        Ok(())
    }

    #[test]
    fn skipped_testcase_is_counted_as_skipped_not_passed() -> Result<(), &'static str> {
        let xml = r#"<testsuites><testsuite name="s" tests="1" errors="0" failures="0">
            <testcase name="ignored_case" classname="s"><skipped/></testcase>
        </testsuite></testsuites>"#;
        let (counts, rows, _) = parsed(xml.as_bytes())?;
        assert_eq!(counts.skipped, 1);
        assert_eq!(rows[0].outcome(), NextestTestOutcome::Skipped);
        Ok(())
    }

    #[test]
    fn leak_fail_message_marker_is_classified_leaky_not_plain_failure() -> Result<(), &'static str>
    {
        let xml = r#"<testsuites><testsuite name="s" tests="1" errors="0" failures="1">
            <testcase name="leaky_case" classname="s">
                <failure type="test failure">test failed: exited with code 0, but leaked handles</failure>
            </testcase>
        </testsuite></testsuites>"#;
        let (counts, rows, _) = parsed(xml.as_bytes())?;
        assert_eq!(counts.leaky, 1);
        assert_eq!(counts.failed, 0);
        assert_eq!(rows[0].outcome(), NextestTestOutcome::Leaky);
        Ok(())
    }

    #[test]
    fn observed_self_closing_leak_error_type_is_classified_from_junit() -> Result<(), &'static str>
    {
        let xml = r#"<testsuites><testsuite name="s" tests="1" errors="1" failures="0">
            <testcase name="leaky_case" classname="s" time="1.009">
                <error type="test exited with code 0, but leaked handles so was marked failed"/>
            </testcase>
        </testsuite></testsuites>"#;
        let (counts, rows, _) = parsed(xml.as_bytes())?;
        assert_eq!(counts.leaky, 1);
        assert_eq!(counts.failed, 0);
        assert_eq!(rows[0].outcome(), NextestTestOutcome::Leaky);
        Ok(())
    }

    #[test]
    fn timeout_failure_message_is_classified_and_attempts_are_derived_from_junit()
    -> Result<(), &'static str> {
        let xml = r#"<testsuites><testsuite name="s" tests="1" errors="0" failures="1">
            <testcase name="slow_case" classname="s">
                <failure type="test failure">test timed out after 1.000s</failure>
                <rerunFailure type="test failure">test timed out after 1.000s</rerunFailure>
            </testcase>
        </testsuite></testsuites>"#;
        let (counts, rows, _) = parsed(xml.as_bytes())?;
        assert_eq!(counts.timed_out, 1);
        assert_eq!(counts.retried, 1);
        assert_eq!(rows[0].outcome(), NextestTestOutcome::TimedOut);
        assert_eq!(rows[0].attempts(), 2);
        Ok(())
    }

    #[test]
    fn billion_laughs_doctype_is_rejected_before_any_expansion() {
        let mut xml =
            String::from("<?xml version=\"1.0\"?>\n<!DOCTYPE lolz [\n <!ENTITY lol \"lol\">\n");
        for n in 1..=9 {
            xml.push_str(&format!(
                " <!ENTITY lol{n} \"&lol{prev};&lol{prev};&lol{prev};&lol{prev};&lol{prev};&lol{prev};&lol{prev};&lol{prev};&lol{prev};&lol{prev};\">\n",
                prev = n - 1
            ));
        }
        xml.push_str("]>\n<testsuites>&lol9;</testsuites>");
        assert_eq!(parse_junit(xml.as_bytes()), JunitReport::Incomplete);
    }

    #[test]
    fn external_entity_reference_is_rejected() {
        let xml = r#"<?xml version="1.0"?>
<!DOCTYPE testsuites [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<testsuites>&xxe;</testsuites>"#;
        assert_eq!(parse_junit(xml.as_bytes()), JunitReport::Incomplete);
    }

    #[test]
    fn excessive_nesting_depth_is_rejected() {
        let mut xml = String::from("<testsuites>");
        for _ in 0..(MAX_DEPTH + 4) {
            xml.push_str("<testsuite>");
        }
        assert_eq!(parse_junit(xml.as_bytes()), JunitReport::Incomplete);
    }

    #[test]
    fn oversized_attribute_value_is_rejected() {
        let xml = format!(
            r#"<testsuites><testsuite name="{}" tests="0" errors="0" failures="0"></testsuite></testsuites>"#,
            "a".repeat(MAX_ATTRIBUTE_LEN + 1)
        );
        assert_eq!(parse_junit(xml.as_bytes()), JunitReport::Incomplete);
    }

    #[test]
    fn oversized_attribute_count_is_rejected() {
        let mut attrs = String::new();
        for i in 0..(MAX_ATTRIBUTES + 4) {
            attrs.push_str(&format!(" a{i}=\"1\""));
        }
        let xml = format!("<testsuites{attrs}></testsuites>");
        assert_eq!(parse_junit(xml.as_bytes()), JunitReport::Incomplete);
    }

    #[test]
    fn oversized_input_is_rejected_without_scanning() {
        let xml = vec![b'a'; MAX_INPUT_BYTES + 1];
        assert_eq!(parse_junit(&xml), JunitReport::Incomplete);
    }

    #[test]
    fn mismatched_end_tag_and_truncated_document_are_rejected() {
        assert_eq!(
            parse_junit(b"<testsuites><testsuite></testsuites></testsuite>"),
            JunitReport::Incomplete
        );
        assert_eq!(
            parse_junit(b"<testsuites><testsuite>"),
            JunitReport::Incomplete
        );
        assert_eq!(
            parse_junit(b""),
            JunitReport::Parsed {
                counts: NextestOutcomeCounts::default(),
                rows: Vec::new(),
                rows_omitted: 0,
            }
        );
    }

    #[test]
    fn forged_flaky_marker_outside_testcase_scope_does_not_flip_a_sibling()
    -> Result<(), &'static str> {
        let xml = r#"<testsuites><testsuite name="s" tests="1" errors="0" failures="0">
            <testcase name="a" classname="s"></testcase>
        </testsuite><flakyFailure>not a real testcase child</flakyFailure></testsuites>"#;
        let (counts, rows, _) = parsed(xml.as_bytes())?;
        assert_eq!(counts.passed, 1);
        assert_eq!(counts.flaky, 0);
        assert_eq!(rows[0].outcome(), NextestTestOutcome::Passed);
        Ok(())
    }

    #[test]
    fn testcase_count_beyond_the_bound_is_omitted_but_counts_stay_exact() -> Result<(), &'static str>
    {
        let mut xml = String::from("<testsuites><testsuite>");
        let total = MAX_TESTCASE_ROWS + 5;
        for i in 0..total {
            xml.push_str(&format!(
                "<testcase name=\"t{i}\" classname=\"s\"></testcase>"
            ));
        }
        xml.push_str("</testsuite></testsuites>");
        let (counts, rows, rows_omitted) = parsed(xml.as_bytes())?;
        assert_eq!(counts.passed as usize, total);
        assert_eq!(rows.len(), MAX_TESTCASE_ROWS);
        assert_eq!(rows_omitted as usize, total - MAX_TESTCASE_ROWS);
        Ok(())
    }

    #[test]
    fn unknown_entity_and_unterminated_ampersand_are_rejected() {
        assert_eq!(
            parse_junit(b"<testsuites>&unknown;</testsuites>"),
            JunitReport::Incomplete
        );
        assert_eq!(
            parse_junit(b"<testsuites>&amp</testsuites>"),
            JunitReport::Incomplete
        );
    }

    #[test]
    fn numeric_character_references_decode_without_recursion() {
        let xml = r#"<testsuites><testsuite name="&#65;&#x42;" tests="0" errors="0" failures="0"></testsuite></testsuites>"#;
        assert!(matches!(
            parse_junit(xml.as_bytes()),
            JunitReport::Parsed { .. }
        ));
    }
}
