//! Bounded parser for the integrity-qualified Miri/nextest result subset.
//!
//! This parser deliberately accepts a closed JUnit grammar. It does not resolve
//! DTDs or custom entities, and it does not retain diagnostic prose or source
//! snippets. Runtime classification requires the JUnit runner exit together
//! with structured rustc/Miri diagnostics captured for that same testcase.

use rust_engineering_domain::miri::{MiriCategory, MiriCounts, MiriFinding, MiriReport};
use serde_json::Value;

const MAX_JUNIT_BYTES: usize = 512 * 1024;
const MAX_STREAM_BYTES: usize = 1024 * 1024;
const MAX_DEPTH: usize = 5;
const MAX_NODES: usize = 16_384;
const MAX_ATTRIBUTES: usize = 12;
const MAX_ATTRIBUTE_BYTES: usize = 4_096;
const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_NAME_BYTES: usize = 64;
const MAX_IDENTITY_BYTES: usize = 512;
const MAX_TESTCASES: usize = 4_096;
const MAX_JSON_LINES: usize = 512;
const MAX_JSON_LINE_BYTES: usize = 256 * 1024;
const MAX_JSON_DEPTH: usize = 64;
const MAX_JSON_NODES: usize = 16_384;
const MAX_DIAGNOSTIC_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_FINDINGS: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MiriParseError {
    InputLimit,
    InvalidJunit,
}

pub(super) fn parse(
    junit: Option<&[u8]>,
    stdout: &[u8],
    stderr: &[u8],
    exit_code: i32,
) -> Result<MiriReport, MiriParseError> {
    if stdout.len() > MAX_STREAM_BYTES || stderr.len() > MAX_STREAM_BYTES {
        return Err(MiriParseError::InputLimit);
    }
    let Some(junit) = junit else {
        return Ok(classify_without_junit(stdout, stderr, exit_code));
    };
    if junit.len() > MAX_JUNIT_BYTES {
        return Err(MiriParseError::InputLimit);
    }

    let parsed = parse_junit(junit)?;
    Ok(classify_junit(parsed, stdout, stderr, exit_code))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct OutcomeCounts {
    tests: u32,
    skipped: u32,
    failures: u32,
    errors: u32,
}

impl OutcomeCounts {
    fn add(&mut self, other: Self) -> Result<(), MiriParseError> {
        self.tests = self
            .tests
            .checked_add(other.tests)
            .ok_or(MiriParseError::InvalidJunit)?;
        self.skipped = self
            .skipped
            .checked_add(other.skipped)
            .ok_or(MiriParseError::InvalidJunit)?;
        self.failures = self
            .failures
            .checked_add(other.failures)
            .ok_or(MiriParseError::InvalidJunit)?;
        self.errors = self
            .errors
            .checked_add(other.errors)
            .ok_or(MiriParseError::InvalidJunit)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Terminal {
    Failure(Option<i32>),
    Error(Option<i32>),
    Timeout,
    Skipped,
}

#[derive(Debug)]
struct Testcase {
    name: String,
    binary: String,
    terminal: Option<Terminal>,
    system_err: String,
    system_out_nonempty: bool,
    saw_system_err: bool,
    saw_system_out: bool,
}

#[derive(Debug)]
struct ParsedJunit {
    cases: Vec<Testcase>,
    counts: OutcomeCounts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tag {
    Testsuites,
    Testsuite,
    Testcase,
    Failure,
    Error,
    Skipped,
    SystemOut,
    SystemErr,
}

impl Tag {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "testsuites" => Some(Self::Testsuites),
            "testsuite" => Some(Self::Testsuite),
            "testcase" => Some(Self::Testcase),
            "failure" => Some(Self::Failure),
            "error" => Some(Self::Error),
            "skipped" => Some(Self::Skipped),
            "system-out" => Some(Self::SystemOut),
            "system-err" => Some(Self::SystemErr),
            _ => None,
        }
    }
}

#[derive(Debug)]
enum Token {
    Declaration,
    Start {
        name: String,
        attributes: Vec<(String, String)>,
        self_closing: bool,
    },
    End(String),
    Text(String),
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

    fn starts_with(&self, needle: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(needle)
    }

    fn advance(&mut self) {
        self.pos += 1;
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.advance();
        }
    }
}

fn is_name_byte(byte: u8, first: bool) -> bool {
    if first {
        byte.is_ascii_alphabetic() || byte == b'_'
    } else {
        byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':')
    }
}

fn read_name(scanner: &mut Scanner<'_>) -> Result<String, MiriParseError> {
    let start = scanner.pos;
    if !scanner.peek().is_some_and(|byte| is_name_byte(byte, true)) {
        return Err(MiriParseError::InvalidJunit);
    }
    scanner.advance();
    while scanner.peek().is_some_and(|byte| is_name_byte(byte, false)) {
        scanner.advance();
        if scanner.pos - start > MAX_NAME_BYTES {
            return Err(MiriParseError::InputLimit);
        }
    }
    String::from_utf8(scanner.bytes[start..scanner.pos].to_vec())
        .map_err(|_| MiriParseError::InvalidJunit)
}

fn valid_xml_char(character: char) -> bool {
    matches!(character, '\u{9}' | '\u{a}' | '\u{d}')
        || ('\u{20}'..='\u{d7ff}').contains(&character)
        || ('\u{e000}'..='\u{fffd}').contains(&character)
        || ('\u{10000}'..='\u{10ffff}').contains(&character)
}

fn decode_xml(raw: &[u8], max_bytes: usize) -> Result<String, MiriParseError> {
    let mut output = String::new();
    let mut index = 0;
    while index < raw.len() {
        if raw[index] != b'&' {
            let start = index;
            while index < raw.len() && raw[index] != b'&' {
                index += 1;
            }
            let plain = std::str::from_utf8(&raw[start..index])
                .map_err(|_| MiriParseError::InvalidJunit)?;
            if plain.chars().any(|character| !valid_xml_char(character)) {
                return Err(MiriParseError::InvalidJunit);
            }
            output.push_str(plain);
        } else {
            let relative_end = raw[index..]
                .iter()
                .position(|byte| *byte == b';')
                .ok_or(MiriParseError::InvalidJunit)?;
            let end = index + relative_end;
            if end - index > 12 {
                return Err(MiriParseError::InvalidJunit);
            }
            let entity = &raw[index + 1..end];
            let character = match entity {
                b"amp" => '&',
                b"lt" => '<',
                b"gt" => '>',
                b"apos" => '\'',
                b"quot" => '"',
                _ => {
                    let (digits, radix) = entity
                        .strip_prefix(b"#x")
                        .or_else(|| entity.strip_prefix(b"#X"))
                        .map(|digits| (digits, 16))
                        .or_else(|| entity.strip_prefix(b"#").map(|digits| (digits, 10)))
                        .ok_or(MiriParseError::InvalidJunit)?;
                    let text =
                        std::str::from_utf8(digits).map_err(|_| MiriParseError::InvalidJunit)?;
                    let scalar = u32::from_str_radix(text, radix)
                        .map_err(|_| MiriParseError::InvalidJunit)?;
                    char::from_u32(scalar).ok_or(MiriParseError::InvalidJunit)?
                }
            };
            if !valid_xml_char(character) {
                return Err(MiriParseError::InvalidJunit);
            }
            output.push(character);
            index = end + 1;
        }
        if output.len() > max_bytes {
            return Err(MiriParseError::InputLimit);
        }
    }
    Ok(output)
}

fn next_token(scanner: &mut Scanner<'_>) -> Result<Option<Token>, MiriParseError> {
    if scanner.pos >= scanner.bytes.len() {
        return Ok(None);
    }
    if scanner.peek() != Some(b'<') {
        let start = scanner.pos;
        while scanner.pos < scanner.bytes.len() && scanner.peek() != Some(b'<') {
            scanner.advance();
            if scanner.pos - start > MAX_TEXT_BYTES {
                return Err(MiriParseError::InputLimit);
            }
        }
        return decode_xml(&scanner.bytes[start..scanner.pos], MAX_TEXT_BYTES)
            .map(Token::Text)
            .map(Some);
    }
    if scanner.peek_at(1) == Some(b'!') {
        return Err(MiriParseError::InvalidJunit);
    }
    if scanner.peek_at(1) == Some(b'?') {
        const DECLARATION: &[u8] = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>";
        if scanner.pos != 0 || !scanner.starts_with(DECLARATION) {
            return Err(MiriParseError::InvalidJunit);
        }
        scanner.pos += DECLARATION.len();
        return Ok(Some(Token::Declaration));
    }
    if scanner.peek_at(1) == Some(b'/') {
        scanner.pos += 2;
        let name = read_name(scanner)?;
        scanner.skip_ws();
        if scanner.peek() != Some(b'>') {
            return Err(MiriParseError::InvalidJunit);
        }
        scanner.advance();
        return Ok(Some(Token::End(name)));
    }

    scanner.advance();
    let name = read_name(scanner)?;
    let mut attributes = Vec::new();
    loop {
        scanner.skip_ws();
        match scanner.peek() {
            Some(b'/') => {
                scanner.advance();
                if scanner.peek() != Some(b'>') {
                    return Err(MiriParseError::InvalidJunit);
                }
                scanner.advance();
                return Ok(Some(Token::Start {
                    name,
                    attributes,
                    self_closing: true,
                }));
            }
            Some(b'>') => {
                scanner.advance();
                return Ok(Some(Token::Start {
                    name,
                    attributes,
                    self_closing: false,
                }));
            }
            Some(_) => {
                if attributes.len() >= MAX_ATTRIBUTES {
                    return Err(MiriParseError::InputLimit);
                }
                let key = read_name(scanner)?;
                if attributes.iter().any(|(existing, _)| existing == &key) {
                    return Err(MiriParseError::InvalidJunit);
                }
                scanner.skip_ws();
                if scanner.peek() != Some(b'=') {
                    return Err(MiriParseError::InvalidJunit);
                }
                scanner.advance();
                scanner.skip_ws();
                let quote = scanner.peek().ok_or(MiriParseError::InvalidJunit)?;
                if !matches!(quote, b'\'' | b'"') {
                    return Err(MiriParseError::InvalidJunit);
                }
                scanner.advance();
                let start = scanner.pos;
                while scanner.peek().is_some_and(|byte| byte != quote) {
                    if scanner.peek() == Some(b'<') {
                        return Err(MiriParseError::InvalidJunit);
                    }
                    scanner.advance();
                    if scanner.pos - start > MAX_ATTRIBUTE_BYTES {
                        return Err(MiriParseError::InputLimit);
                    }
                }
                if scanner.peek() != Some(quote) {
                    return Err(MiriParseError::InvalidJunit);
                }
                let value = decode_xml(&scanner.bytes[start..scanner.pos], MAX_ATTRIBUTE_BYTES)?;
                scanner.advance();
                attributes.push((key, value));
            }
            None => return Err(MiriParseError::InvalidJunit),
        }
    }
}

fn attribute<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find_map(|(key, value)| (key == name).then_some(value.as_str()))
}

fn attributes_are_known(attributes: &[(String, String)], allowed: &[&str]) -> bool {
    attributes
        .iter()
        .all(|(key, _)| allowed.contains(&key.as_str()))
}

fn count_attribute(attributes: &[(String, String)], name: &str) -> Result<u32, MiriParseError> {
    let value = attribute(attributes, name).ok_or(MiriParseError::InvalidJunit)?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(MiriParseError::InvalidJunit);
    }
    value
        .parse::<u32>()
        .map_err(|_| MiriParseError::InvalidJunit)
}

fn parse_declared_counts(attributes: &[(String, String)]) -> Result<OutcomeCounts, MiriParseError> {
    Ok(OutcomeCounts {
        tests: count_attribute(attributes, "tests")?,
        skipped: count_attribute(attributes, "skipped")?,
        failures: count_attribute(attributes, "failures")?,
        errors: count_attribute(attributes, "errors")?,
    })
}

fn validate_optional_time(attributes: &[(String, String)]) -> Result<(), MiriParseError> {
    let Some(value) = attribute(attributes, "time") else {
        return Ok(());
    };
    let seconds = value
        .parse::<f64>()
        .map_err(|_| MiriParseError::InvalidJunit)?;
    if !seconds.is_finite() || seconds < 0.0 || seconds > 86_400.0 {
        return Err(MiriParseError::InvalidJunit);
    }
    Ok(())
}

fn validated_identity(value: Option<&str>) -> Result<String, MiriParseError> {
    let value = value.ok_or(MiriParseError::InvalidJunit)?;
    if value.len() > MAX_IDENTITY_BYTES {
        return Err(MiriParseError::InputLimit);
    }
    // Keep a conservative dependency-free subset of Rust/nextest identities:
    // Unicode letters/numbers and the fixed separators used by paths, generated
    // names and binary IDs. Whitespace, prose punctuation and bidi formatting
    // controls cannot reach the normalized finding.
    let mut saw_identifier = false;
    for character in value.chars() {
        if character == '_' || character.is_alphanumeric() {
            saw_identifier = true;
        } else if !matches!(character, ':' | '$' | '.' | '<' | '>' | '#' | '-') {
            return Err(MiriParseError::InvalidJunit);
        }
    }
    if !saw_identifier {
        return Err(MiriParseError::InvalidJunit);
    }
    Ok(value.to_owned())
}

fn parse_runner_exit(value: Option<&str>) -> Option<i32> {
    value?
        .strip_prefix("test failure with exit code ")?
        .parse::<i32>()
        .ok()
}

#[derive(Debug)]
struct SuiteState {
    expected: OutcomeCounts,
    actual: OutcomeCounts,
}

#[derive(Debug)]
struct XmlState {
    stack: Vec<Tag>,
    declaration_seen: bool,
    root_seen: bool,
    root_closed: bool,
    root_expected: Option<OutcomeCounts>,
    root_actual: OutcomeCounts,
    suite: Option<SuiteState>,
    testcase: Option<Testcase>,
    cases: Vec<Testcase>,
}

impl XmlState {
    fn new() -> Self {
        Self {
            stack: Vec::new(),
            declaration_seen: false,
            root_seen: false,
            root_closed: false,
            root_expected: None,
            root_actual: OutcomeCounts::default(),
            suite: None,
            testcase: None,
            cases: Vec::new(),
        }
    }

    fn declaration(&mut self) -> Result<(), MiriParseError> {
        if self.declaration_seen || self.root_seen || !self.stack.is_empty() {
            return Err(MiriParseError::InvalidJunit);
        }
        self.declaration_seen = true;
        Ok(())
    }

    fn start(&mut self, tag: Tag, attributes: &[(String, String)]) -> Result<(), MiriParseError> {
        let parent = self.stack.last().copied();
        match tag {
            Tag::Testsuites => {
                if parent.is_some() || self.root_seen || self.root_closed {
                    return Err(MiriParseError::InvalidJunit);
                }
                if !attributes_are_known(
                    attributes,
                    &[
                        "name",
                        "tests",
                        "skipped",
                        "failures",
                        "errors",
                        "uuid",
                        "timestamp",
                        "time",
                    ],
                ) || attribute(attributes, "name") != Some("nextest-run")
                {
                    return Err(MiriParseError::InvalidJunit);
                }
                validate_optional_time(attributes)?;
                self.root_expected = Some(parse_declared_counts(attributes)?);
                self.root_seen = true;
            }
            Tag::Testsuite => {
                if parent != Some(Tag::Testsuites) || self.suite.is_some() {
                    return Err(MiriParseError::InvalidJunit);
                }
                if !attributes_are_known(
                    attributes,
                    &[
                        "name",
                        "tests",
                        "skipped",
                        "failures",
                        "errors",
                        "time",
                        "timestamp",
                    ],
                ) {
                    return Err(MiriParseError::InvalidJunit);
                }
                let _ = validated_identity(attribute(attributes, "name"))?;
                validate_optional_time(attributes)?;
                self.suite = Some(SuiteState {
                    expected: parse_declared_counts(attributes)?,
                    actual: OutcomeCounts::default(),
                });
            }
            Tag::Testcase => {
                if self.cases.len() >= MAX_TESTCASES {
                    return Err(MiriParseError::InputLimit);
                }
                if parent != Some(Tag::Testsuite) || self.testcase.is_some() {
                    return Err(MiriParseError::InvalidJunit);
                }
                if !attributes_are_known(attributes, &["name", "classname", "time", "timestamp"]) {
                    return Err(MiriParseError::InvalidJunit);
                }
                validate_optional_time(attributes)?;
                self.testcase = Some(Testcase {
                    name: validated_identity(attribute(attributes, "name"))?,
                    binary: validated_identity(attribute(attributes, "classname"))?,
                    terminal: None,
                    system_err: String::new(),
                    system_out_nonempty: false,
                    saw_system_err: false,
                    saw_system_out: false,
                });
            }
            Tag::Failure | Tag::Error => {
                if parent != Some(Tag::Testcase)
                    || !attributes_are_known(attributes, &["type", "message"])
                {
                    return Err(MiriParseError::InvalidJunit);
                }
                let testcase = self.testcase.as_mut().ok_or(MiriParseError::InvalidJunit)?;
                if testcase.terminal.is_some() {
                    return Err(MiriParseError::InvalidJunit);
                }
                let kind = if tag == Tag::Failure
                    && attribute(attributes, "type") == Some("test timeout")
                {
                    Terminal::Timeout
                } else if tag == Tag::Failure {
                    Terminal::Failure(parse_runner_exit(attribute(attributes, "type")))
                } else {
                    Terminal::Error(parse_runner_exit(attribute(attributes, "type")))
                };
                testcase.terminal = Some(kind);
            }
            Tag::Skipped => {
                if parent != Some(Tag::Testcase) || !attributes_are_known(attributes, &["message"])
                {
                    return Err(MiriParseError::InvalidJunit);
                }
                let testcase = self.testcase.as_mut().ok_or(MiriParseError::InvalidJunit)?;
                if testcase.terminal.replace(Terminal::Skipped).is_some() {
                    return Err(MiriParseError::InvalidJunit);
                }
            }
            Tag::SystemErr => {
                if parent != Some(Tag::Testcase) || !attributes.is_empty() {
                    return Err(MiriParseError::InvalidJunit);
                }
                let testcase = self.testcase.as_mut().ok_or(MiriParseError::InvalidJunit)?;
                if testcase.saw_system_err {
                    return Err(MiriParseError::InvalidJunit);
                }
                testcase.saw_system_err = true;
            }
            Tag::SystemOut => {
                if parent != Some(Tag::Testcase) || !attributes.is_empty() {
                    return Err(MiriParseError::InvalidJunit);
                }
                let testcase = self.testcase.as_mut().ok_or(MiriParseError::InvalidJunit)?;
                if testcase.saw_system_out {
                    return Err(MiriParseError::InvalidJunit);
                }
                testcase.saw_system_out = true;
            }
        }
        self.stack.push(tag);
        if self.stack.len() > MAX_DEPTH {
            return Err(MiriParseError::InputLimit);
        }
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<(), MiriParseError> {
        match self.stack.last().copied() {
            Some(Tag::SystemErr) => {
                let testcase = self.testcase.as_mut().ok_or(MiriParseError::InvalidJunit)?;
                if testcase.system_err.len().saturating_add(text.len()) > MAX_TEXT_BYTES {
                    return Err(MiriParseError::InputLimit);
                }
                testcase.system_err.push_str(text);
            }
            Some(Tag::SystemOut) => {
                if !text.trim().is_empty() {
                    self.testcase
                        .as_mut()
                        .ok_or(MiriParseError::InvalidJunit)?
                        .system_out_nonempty = true;
                }
            }
            Some(Tag::Failure | Tag::Error | Tag::Skipped) => {}
            _ if text.trim().is_empty() => {}
            _ => return Err(MiriParseError::InvalidJunit),
        }
        Ok(())
    }

    fn end(&mut self, tag: Tag) -> Result<(), MiriParseError> {
        if self.stack.pop() != Some(tag) {
            return Err(MiriParseError::InvalidJunit);
        }
        match tag {
            Tag::Testcase => {
                let testcase = self.testcase.take().ok_or(MiriParseError::InvalidJunit)?;
                let suite = self.suite.as_mut().ok_or(MiriParseError::InvalidJunit)?;
                suite.actual.tests = suite
                    .actual
                    .tests
                    .checked_add(1)
                    .ok_or(MiriParseError::InvalidJunit)?;
                match testcase.terminal {
                    Some(Terminal::Failure(_) | Terminal::Timeout) => suite.actual.failures += 1,
                    Some(Terminal::Error(_)) => suite.actual.errors += 1,
                    Some(Terminal::Skipped) => suite.actual.skipped += 1,
                    None => {}
                }
                self.cases.push(testcase);
            }
            Tag::Testsuite => {
                let suite = self.suite.take().ok_or(MiriParseError::InvalidJunit)?;
                if suite.expected != suite.actual {
                    return Err(MiriParseError::InvalidJunit);
                }
                self.root_actual.add(suite.actual)?;
            }
            Tag::Testsuites => {
                if self.suite.is_some() || self.testcase.is_some() {
                    return Err(MiriParseError::InvalidJunit);
                }
                if self.root_expected != Some(self.root_actual) {
                    return Err(MiriParseError::InvalidJunit);
                }
                self.root_closed = true;
            }
            Tag::Failure | Tag::Error | Tag::Skipped | Tag::SystemOut | Tag::SystemErr => {}
        }
        Ok(())
    }
}

fn parse_junit(input: &[u8]) -> Result<ParsedJunit, MiriParseError> {
    let mut scanner = Scanner::new(input);
    let mut state = XmlState::new();
    let mut nodes = 0usize;
    loop {
        let token = next_token(&mut scanner)?;
        let Some(token) = token else {
            break;
        };
        nodes += 1;
        if nodes > MAX_NODES {
            return Err(MiriParseError::InputLimit);
        }
        match token {
            Token::Declaration => state.declaration()?,
            Token::Text(text) => state.text(&text)?,
            Token::Start {
                name,
                attributes,
                self_closing,
            } => {
                let tag = Tag::from_name(&name).ok_or(MiriParseError::InvalidJunit)?;
                state.start(tag, &attributes)?;
                if self_closing {
                    state.end(tag)?;
                }
            }
            Token::End(name) => {
                let tag = Tag::from_name(&name).ok_or(MiriParseError::InvalidJunit)?;
                state.end(tag)?;
            }
        }
    }
    if !state.stack.is_empty()
        || !state.root_seen
        || !state.root_closed
        || state.suite.is_some()
        || state.testcase.is_some()
    {
        return Err(MiriParseError::InvalidJunit);
    }
    Ok(ParsedJunit {
        cases: state.cases,
        counts: state.root_actual,
    })
}

#[derive(Clone, Copy, Debug, Default)]
struct JsonEvidence {
    saw_json: bool,
    has_error: bool,
    undefined_behavior: bool,
    unsupported: bool,
    post_monomorphization: bool,
    rustc_compile: bool,
    unexpected: bool,
}

impl JsonEvidence {
    fn merge(&mut self, other: Self) {
        self.saw_json |= other.saw_json;
        self.has_error |= other.has_error;
        self.undefined_behavior |= other.undefined_behavior;
        self.unsupported |= other.unsupported;
        self.post_monomorphization |= other.post_monomorphization;
        self.rustc_compile |= other.rustc_compile;
        self.unexpected |= other.unexpected;
    }

    fn known_miri_error(self) -> bool {
        self.undefined_behavior || self.unsupported || self.post_monomorphization
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn json_within_bounds(root: &Value) -> bool {
    let mut stack = vec![(root, 1usize)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if nodes > MAX_JSON_NODES || depth > MAX_JSON_DEPTH {
            return false;
        }
        match value {
            Value::Array(values) => {
                stack.extend(values.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(values) => {
                stack.extend(values.values().map(|value| (value, depth + 1)));
            }
            Value::String(value) if value.len() > MAX_TEXT_BYTES => return false,
            _ => {}
        }
    }
    true
}

fn rustc_error_code(value: &Value) -> bool {
    value
        .get("code")
        .and_then(Value::as_object)
        .and_then(|code| code.get("code"))
        .and_then(Value::as_str)
        .is_some_and(|code| {
            code.len() == 5
                && code.starts_with('E')
                && code.as_bytes()[1..].iter().all(u8::is_ascii_digit)
        })
}

fn is_abort_helper(message: &str) -> bool {
    let Some(rest) = message.strip_prefix("aborting due to ") else {
        return false;
    };
    let Some(number) = rest
        .strip_suffix(" previous error")
        .or_else(|| rest.strip_suffix(" previous errors"))
    else {
        return false;
    };
    !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
}

fn analyze_json_line(line: &[u8]) -> JsonEvidence {
    let mut evidence = JsonEvidence {
        saw_json: true,
        ..JsonEvidence::default()
    };
    if line.len() > MAX_JSON_LINE_BYTES {
        evidence.unexpected = true;
        return evidence;
    }
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        evidence.unexpected = true;
        return evidence;
    };
    if !json_within_bounds(&value)
        || value.get("$message_type").and_then(Value::as_str) != Some("diagnostic")
    {
        evidence.unexpected = true;
        return evidence;
    }
    let Some(level) = value.get("level").and_then(Value::as_str) else {
        evidence.unexpected = true;
        return evidence;
    };
    let Some(message) = value.get("message").and_then(Value::as_str) else {
        evidence.unexpected = true;
        return evidence;
    };
    if message.len() > MAX_DIAGNOSTIC_MESSAGE_BYTES {
        evidence.unexpected = true;
        return evidence;
    }
    if level != "error" {
        if message.starts_with("Undefined Behavior:")
            || message.starts_with("unsupported operation:")
            || message.starts_with("post-monomorphization error:")
        {
            evidence.unexpected = true;
        }
        return evidence;
    }

    evidence.has_error = true;
    if message.starts_with("Undefined Behavior:") {
        evidence.undefined_behavior = true;
    } else if message.starts_with("unsupported operation:") {
        evidence.unsupported = true;
    } else if message.starts_with("post-monomorphization error:") {
        evidence.post_monomorphization = true;
    } else if rustc_error_code(&value) {
        evidence.rustc_compile = true;
    } else if !is_abort_helper(message) {
        evidence.unexpected = true;
    }
    evidence
}

fn analyze_json_stream(bytes: &[u8], reject_non_json: bool) -> JsonEvidence {
    let mut evidence = JsonEvidence::default();
    let mut lines = 0usize;
    for raw_line in bytes.split(|byte| *byte == b'\n') {
        let line = trim_ascii(raw_line);
        if line.is_empty() {
            continue;
        }
        if line.first() == Some(&b'{') {
            lines += 1;
            if lines > MAX_JSON_LINES {
                evidence.saw_json = true;
                evidence.unexpected = true;
                continue;
            }
            evidence.merge(analyze_json_line(line));
        } else if reject_non_json {
            evidence.unexpected = true;
        }
    }
    evidence
}

struct ReportBuilder {
    counts: MiriCounts,
    findings: Vec<MiriFinding>,
    findings_omitted: u64,
    complete: bool,
}

impl ReportBuilder {
    fn new() -> Self {
        Self {
            counts: MiriCounts::default(),
            findings: Vec::new(),
            findings_omitted: 0,
            complete: true,
        }
    }

    fn finding(&mut self, category: MiriCategory, testcase: Option<&Testcase>) {
        match category {
            MiriCategory::UndefinedBehavior => self.counts.undefined_behavior += 1,
            MiriCategory::UnsupportedOperation => self.counts.unsupported_operation += 1,
            MiriCategory::TestFailure => self.counts.test_failures += 1,
            MiriCategory::CompileFailure => self.counts.compile_failures += 1,
            MiriCategory::Timeout => self.counts.timeouts += 1,
            MiriCategory::Unclassified => self.counts.unclassified += 1,
        }
        if self.findings.len() < MAX_FINDINGS {
            self.findings.push(MiriFinding {
                category,
                test_name: testcase.map(|testcase| testcase.name.clone()),
                test_binary: testcase.map(|testcase| testcase.binary.clone()),
            });
        } else {
            self.findings_omitted = self.findings_omitted.saturating_add(1);
            self.complete = false;
        }
    }

    fn unclassified(&mut self, testcase: Option<&Testcase>) {
        self.complete = false;
        self.finding(MiriCategory::Unclassified, testcase);
    }
}

fn classify_failed_case(
    builder: &mut ReportBuilder,
    testcase: &Testcase,
    runner_exit: Option<i32>,
) {
    let evidence = analyze_json_stream(testcase.system_err.as_bytes(), true);
    if testcase.system_out_nonempty {
        builder.unclassified(Some(testcase));
        return;
    }
    let known_categories = u8::from(evidence.undefined_behavior)
        + u8::from(evidence.unsupported)
        + u8::from(evidence.post_monomorphization);
    match runner_exit {
        Some(1)
            if !evidence.unexpected
                && !evidence.rustc_compile
                && evidence.known_miri_error()
                && known_categories == 1 =>
        {
            if evidence.undefined_behavior {
                builder.finding(MiriCategory::UndefinedBehavior, Some(testcase));
            }
            if evidence.unsupported {
                builder.finding(MiriCategory::UnsupportedOperation, Some(testcase));
            }
            if evidence.post_monomorphization {
                builder.finding(MiriCategory::CompileFailure, Some(testcase));
            }
        }
        Some(101) if !evidence.saw_json && !evidence.has_error && !evidence.unexpected => {
            builder.finding(MiriCategory::TestFailure, Some(testcase));
        }
        _ => builder.unclassified(Some(testcase)),
    }
}

fn classify_junit(
    parsed: ParsedJunit,
    _stdout: &[u8],
    _stderr: &[u8],
    exit_code: i32,
) -> MiriReport {
    let mut builder = ReportBuilder::new();
    builder.counts.tests = parsed.counts.tests;
    for testcase in &parsed.cases {
        match testcase.terminal {
            None => {
                builder.counts.passed += 1;
                if testcase.system_out_nonempty || !testcase.system_err.trim().is_empty() {
                    builder.unclassified(Some(testcase));
                }
            }
            Some(Terminal::Skipped) => {
                builder.counts.skipped += 1;
                builder.unclassified(Some(testcase));
            }
            Some(Terminal::Timeout) => {
                builder.counts.failed += 1;
                builder.complete = false;
                builder.finding(MiriCategory::Timeout, Some(testcase));
            }
            Some(Terminal::Failure(runner_exit) | Terminal::Error(runner_exit)) => {
                builder.counts.failed += 1;
                classify_failed_case(&mut builder, testcase, runner_exit);
            }
        }
    }

    let expected_exit = if parsed.counts.failures + parsed.counts.errors > 0 {
        100
    } else if parsed.counts.tests == parsed.counts.skipped {
        4
    } else {
        0
    };
    if exit_code != expected_exit {
        builder.unclassified(None);
    }
    if parsed.counts.tests == 0 {
        builder.unclassified(None);
    }

    let clean = builder.complete
        && exit_code == 0
        && builder.counts.tests > 0
        && builder.counts.passed == builder.counts.tests
        && builder.counts.failed == 0
        && builder.counts.skipped == 0
        && builder.findings_omitted == 0;
    MiriReport {
        counts: builder.counts,
        findings: builder.findings,
        findings_omitted: builder.findings_omitted,
        complete: builder.complete,
        clean,
        junit_present: true,
        exit_code: Some(exit_code),
    }
}

fn classify_without_junit(stdout: &[u8], stderr: &[u8], exit_code: i32) -> MiriReport {
    let stdout_evidence = analyze_json_stream(stdout, false);
    let stderr_evidence = analyze_json_stream(stderr, false);
    let corroborated_compile = exit_code == 104
        && !stdout_evidence.saw_json
        && !stderr_evidence.unexpected
        && !stderr_evidence.undefined_behavior
        && !stderr_evidence.unsupported
        && (stderr_evidence.rustc_compile || stderr_evidence.post_monomorphization);
    let mut builder = ReportBuilder::new();
    if corroborated_compile {
        builder.finding(MiriCategory::CompileFailure, None);
    } else {
        builder.unclassified(None);
    }
    MiriReport {
        counts: builder.counts,
        findings: builder.findings,
        findings_omitted: builder.findings_omitted,
        complete: builder.complete,
        clean: false,
        junit_present: false,
        exit_code: Some(exit_code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLEAN: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/clean.junit.xml"
    );
    const CLEAN_RECEIPT: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/clean.json"
    );
    const BENIGN_FORGED: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/benign-forged.junit.xml"
    );
    const BENIGN_FORGED_RECEIPT: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/benign-forged.json"
    );
    const UAF: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/uaf.junit.xml"
    );
    const UAF_RECEIPT: &[u8] =
        include_bytes!("../../../fixtures/m4-runtime-oracles/miri-classification/results/uaf.json");
    const UNINIT: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/uninit.junit.xml"
    );
    const UNINIT_RECEIPT: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/uninit.json"
    );
    const ALIAS: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/alias.junit.xml"
    );
    const ALIAS_RECEIPT: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/alias.json"
    );
    const RACE: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/race.junit.xml"
    );
    const RACE_RECEIPT: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/race.json"
    );
    const FFI: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/ffi.junit.xml"
    );
    const FFI_RECEIPT: &[u8] =
        include_bytes!("../../../fixtures/m4-runtime-oracles/miri-classification/results/ffi.json");
    const EMPTY: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/empty.junit.xml"
    );
    const EMPTY_RECEIPT: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/empty.json"
    );
    const IGNORED: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/ignored.junit.xml"
    );
    const IGNORED_RECEIPT: &[u8] = include_bytes!(
        "../../../fixtures/m4-runtime-oracles/miri-classification/results/ignored.json"
    );

    fn parsed(junit: &[u8], exit_code: i32) -> Result<MiriReport, &'static str> {
        parse(Some(junit), b"", b"", exit_code).map_err(|_| "Miri parse failed")
    }

    fn oracle(receipt: &[u8], junit: &[u8]) -> Result<MiriReport, &'static str> {
        let receipt: Value =
            serde_json::from_slice(receipt).map_err(|_| "invalid oracle receipt")?;
        let stdout = receipt
            .get("stdout")
            .and_then(Value::as_str)
            .ok_or("missing oracle stdout")?;
        let stderr = receipt
            .get("stderr")
            .and_then(Value::as_str)
            .ok_or("missing oracle stderr")?;
        let exit_code = receipt
            .get("exit_code")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or("missing oracle exit code")?;
        parse(Some(junit), stdout.as_bytes(), stderr.as_bytes(), exit_code)
            .map_err(|_| "oracle result did not parse")
    }

    fn oracle_without_junit(receipt: &[u8]) -> Result<MiriReport, &'static str> {
        let receipt: Value =
            serde_json::from_slice(receipt).map_err(|_| "invalid oracle receipt")?;
        let stdout = receipt
            .get("stdout")
            .and_then(Value::as_str)
            .ok_or("missing oracle stdout")?;
        let stderr = receipt
            .get("stderr")
            .and_then(Value::as_str)
            .ok_or("missing oracle stderr")?;
        let exit_code = receipt
            .get("exit_code")
            .and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or("missing oracle exit code")?;
        parse(None, stdout.as_bytes(), stderr.as_bytes(), exit_code)
            .map_err(|_| "oracle result did not parse")
    }

    fn category_count(report: &MiriReport, category: MiriCategory) -> usize {
        report
            .findings
            .iter()
            .filter(|finding| finding.category == category)
            .count()
    }

    #[test]
    fn nine_oracle_junit_goldens_have_exact_categories() -> Result<(), &'static str> {
        let clean = oracle(CLEAN_RECEIPT, CLEAN)?;
        assert!(clean.clean);
        assert!(clean.complete);
        assert!(clean.validate());
        assert_eq!(clean.counts.tests, 1);
        assert_eq!(clean.counts.passed, 1);
        assert!(clean.findings.is_empty());

        let forged = oracle(BENIGN_FORGED_RECEIPT, BENIGN_FORGED)?;
        assert!(!forged.clean);
        assert!(forged.complete);
        assert!(forged.validate());
        assert_eq!(forged.counts.tests, 2);
        assert_eq!(forged.counts.passed, 1);
        assert_eq!(forged.counts.failed, 1);
        assert_eq!(forged.counts.test_failures, 1);
        assert_eq!(category_count(&forged, MiriCategory::TestFailure), 1);

        for (receipt, golden) in [
            (UAF_RECEIPT, UAF),
            (UNINIT_RECEIPT, UNINIT),
            (ALIAS_RECEIPT, ALIAS),
            (RACE_RECEIPT, RACE),
        ] {
            let report = oracle(receipt, golden)?;
            assert!(report.complete);
            assert!(report.validate());
            assert_eq!(report.counts.tests, 1);
            assert_eq!(report.counts.failed, 1);
            assert_eq!(report.counts.undefined_behavior, 1);
            assert_eq!(category_count(&report, MiriCategory::UndefinedBehavior), 1);
            assert!(report.findings[0].test_name.is_some());
            assert!(report.findings[0].test_binary.is_some());
        }

        let ffi = oracle(FFI_RECEIPT, FFI)?;
        assert!(ffi.complete);
        assert!(ffi.validate());
        assert_eq!(ffi.counts.unsupported_operation, 1);
        assert_eq!(category_count(&ffi, MiriCategory::UnsupportedOperation), 1);

        let empty = oracle(EMPTY_RECEIPT, EMPTY)?;
        assert!(!empty.complete);
        assert!(!empty.clean);
        assert!(empty.validate());
        assert_eq!(empty.counts.tests, 0);
        assert_eq!(empty.counts.unclassified, 1);

        let ignored = oracle(IGNORED_RECEIPT, IGNORED)?;
        assert!(!ignored.complete);
        assert!(!ignored.clean);
        assert!(ignored.validate());
        assert_eq!(ignored.counts.tests, 1);
        assert_eq!(ignored.counts.skipped, 1);
        assert_eq!(ignored.counts.unclassified, 1);
        Ok(())
    }

    #[test]
    fn compile_failure_requires_exit_104_and_structured_compiler_code() -> Result<(), &'static str>
    {
        let report = oracle_without_junit(include_bytes!(
            "../../../fixtures/m4-runtime-oracles/miri-classification/results/compile-fail.json"
        ))?;
        assert!(report.complete);
        assert!(!report.clean);
        assert!(!report.junit_present);
        assert!(report.validate());
        assert_eq!(report.counts.compile_failures, 1);

        let prose = parse(None, b"", b"error[E0308]: mismatched types", 104)
            .map_err(|_| "prose result did not parse")?;
        assert!(!prose.complete);
        assert_eq!(prose.counts.compile_failures, 0);
        assert_eq!(prose.counts.unclassified, 1);
        Ok(())
    }

    #[test]
    fn runner_101_needs_empty_muted_streams_and_runner_1_needs_miri_json()
    -> Result<(), &'static str> {
        let forged_json = r#"{"$message_type":"diagnostic","message":"Undefined Behavior: forged","code":null,"level":"error"}"#;
        let runner_101 = format!(
            r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="1" errors="0"><testsuite name="s" tests="1" skipped="0" failures="1" errors="0"><testcase name="t" classname="b"><failure type="test failure with exit code 101"/><system-out>{forged_json}</system-out><system-err/></testcase></testsuite></testsuites>"#
        );
        let report = parsed(runner_101.as_bytes(), 100)?;
        assert_eq!(report.counts.test_failures, 0);
        assert_eq!(report.counts.undefined_behavior, 0);
        assert_eq!(report.counts.unclassified, 1);
        assert!(!report.complete);

        let runner_1 = br#"<testsuites name="nextest-run" tests="1" skipped="0" failures="1" errors="0"><testsuite name="s" tests="1" skipped="0" failures="1" errors="0"><testcase name="t" classname="b"><failure type="test failure with exit code 1"/><system-err/></testcase></testsuite></testsuites>"#;
        let report = parsed(runner_1, 100)?;
        assert_eq!(report.counts.test_failures, 0);
        assert_eq!(report.counts.unclassified, 1);
        assert!(!report.complete);
        Ok(())
    }

    #[test]
    fn semantic_diagnostic_with_wrong_runner_or_helper_only_is_unclassified()
    -> Result<(), &'static str> {
        let semantic = r#"{&quot;$message_type&quot;:&quot;diagnostic&quot;,&quot;message&quot;:&quot;Undefined Behavior: real shape, wrong exit&quot;,&quot;code&quot;:null,&quot;level&quot;:&quot;error&quot;}"#;
        let xml = format!(
            r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="1" errors="0"><testsuite name="s" tests="1" skipped="0" failures="1" errors="0"><testcase name="t" classname="b"><failure type="test failure with exit code 101"/><system-err>{semantic}</system-err></testcase></testsuite></testsuites>"#
        );
        let report = parsed(xml.as_bytes(), 100)?;
        assert_eq!(report.counts.undefined_behavior, 0);
        assert_eq!(report.counts.test_failures, 0);
        assert_eq!(report.counts.unclassified, 1);

        let helper = r#"{&quot;$message_type&quot;:&quot;diagnostic&quot;,&quot;message&quot;:&quot;aborting due to 1 previous error&quot;,&quot;code&quot;:null,&quot;level&quot;:&quot;error&quot;}"#;
        let xml = format!(
            r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="1" errors="0"><testsuite name="s" tests="1" skipped="0" failures="1" errors="0"><testcase name="t" classname="b"><failure type="test failure with exit code 1"/><system-err>{helper}</system-err></testcase></testsuite></testsuites>"#
        );
        let report = parsed(xml.as_bytes(), 100)?;
        assert_eq!(report.counts.unclassified, 1);
        assert!(!report.complete);
        Ok(())
    }

    #[test]
    fn malformed_and_excessively_nested_json_are_partial_not_semantic_findings()
    -> Result<(), &'static str> {
        let nested = format!(
            "{}0{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        );
        for diagnostic in [
            String::from("{not-json"),
            format!(
                r#"{{"$message_type":"diagnostic","message":"Undefined Behavior: nested","code":null,"level":"error","extra":{} }}"#,
                nested
            ),
        ] {
            let escaped = diagnostic.replace('&', "&amp;").replace('"', "&quot;");
            let xml = format!(
                r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="1" errors="0"><testsuite name="s" tests="1" skipped="0" failures="1" errors="0"><testcase name="t" classname="b"><failure type="test failure with exit code 1"/><system-err>{escaped}</system-err></testcase></testsuite></testsuites>"#
            );
            let report = parsed(xml.as_bytes(), 100)?;
            assert_eq!(report.counts.undefined_behavior, 0);
            assert_eq!(report.counts.unclassified, 1);
            assert!(!report.complete);
            assert!(report.validate());
        }
        Ok(())
    }

    #[test]
    fn post_monomorphization_is_a_typed_compile_failure() -> Result<(), &'static str> {
        let diagnostic = r#"{&quot;$message_type&quot;:&quot;diagnostic&quot;,&quot;message&quot;:&quot;post-monomorphization error: unsupported target feature&quot;,&quot;code&quot;:null,&quot;level&quot;:&quot;error&quot;}"#;
        let xml = format!(
            r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="1" errors="0"><testsuite name="s" tests="1" skipped="0" failures="1" errors="0"><testcase name="t" classname="b"><failure type="test failure with exit code 1"/><system-err>{diagnostic}</system-err></testcase></testsuite></testsuites>"#
        );
        let report = parsed(xml.as_bytes(), 100)?;
        assert!(report.complete);
        assert!(report.validate());
        assert_eq!(report.counts.compile_failures, 1);
        assert_eq!(report.counts.unclassified, 0);
        Ok(())
    }

    #[test]
    fn clean_requires_matching_exit_and_ignores_unauthenticated_top_streams()
    -> Result<(), &'static str> {
        let mismatch = parsed(CLEAN, 100)?;
        assert!(!mismatch.clean);
        assert!(!mismatch.complete);
        assert_eq!(mismatch.counts.unclassified, 1);

        let diagnostic = br#"{"$message_type":"diagnostic","message":"Undefined Behavior: outside testcase","code":null,"level":"error"}"#;
        let report = parse(Some(CLEAN), b"", diagnostic, 0)
            .map_err(|_| "top-level diagnostic did not parse")?;
        assert!(report.clean);
        assert!(report.complete);
        assert_eq!(report.counts.undefined_behavior, 0);
        assert_eq!(report.counts.unclassified, 0);
        Ok(())
    }

    #[test]
    fn exact_nextest_timeout_is_typed_and_never_complete() -> Result<(), &'static str> {
        let timeout = br#"<testsuites name="nextest-run" tests="1" skipped="0" failures="1" errors="0"><testsuite name="suite" tests="1" skipped="0" failures="1" errors="0"><testcase name="modulo::cuelga" classname="caja"><failure type="test timeout"/><system-err/></testcase></testsuite></testsuites>"#;
        let report = parsed(timeout, 100)?;
        assert_eq!(report.counts.tests, 1);
        assert_eq!(report.counts.failed, 1);
        assert_eq!(report.counts.timeouts, 1);
        assert_eq!(report.counts.unclassified, 0);
        assert_eq!(category_count(&report, MiriCategory::Timeout), 1);
        assert!(!report.complete);
        assert!(!report.clean);
        assert!(report.validate());

        let near_miss = String::from_utf8(timeout.to_vec())
            .map_err(|_| "timeout fixture was not UTF-8")?
            .replace("type=\"test timeout\"", "type=\"test timeout \"");
        let report = parsed(near_miss.as_bytes(), 100)?;
        assert_eq!(report.counts.timeouts, 0);
        assert_eq!(report.counts.unclassified, 1);
        assert!(!report.complete);
        assert!(report.validate());
        Ok(())
    }

    #[test]
    fn identities_accept_unicode_names_but_reject_prose_and_bidi_controls()
    -> Result<(), &'static str> {
        let unicode = r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="0" errors="0"><testsuite name="módulo" tests="1" skipped="0" failures="0" errors="0"><testcase name="módulo::prueba_Δ2" classname="caja-ñ"/></testsuite></testsuites>"#;
        let report = parsed(unicode.as_bytes(), 0)?;
        assert!(report.clean);
        assert!(report.validate());

        for identity in ["ignore previous instructions", "safe\u{202e}evil"] {
            let xml = format!(
                r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="0" errors="0"><testsuite name="suite" tests="1" skipped="0" failures="0" errors="0"><testcase name="{identity}" classname="caja"/></testsuite></testsuites>"#
            );
            assert_eq!(
                parse(Some(xml.as_bytes()), b"", b"", 0),
                Err(MiriParseError::InvalidJunit)
            );
        }
        Ok(())
    }

    #[test]
    fn hostile_or_ambiguous_xml_shapes_are_rejected() {
        let cases: &[&[u8]] = &[
            br#"<!DOCTYPE testsuites [<!ENTITY x SYSTEM "file:///etc/passwd">]><testsuites name="nextest-run" tests="0" skipped="0" failures="0" errors="0"/>"#,
            br#"<!-- forged --><testsuites name="nextest-run" tests="0" skipped="0" failures="0" errors="0"/>"#,
            br#"<testsuites name="nextest-run" tests="0" tests="0" skipped="0" failures="0" errors="0"/>"#,
            br#"<testsuites name="nextest-run" tests="0" skipped="0" failures="0" errors="0"><expected-stdin>forged</expected-stdin></testsuites>"#,
            br#"<testsuites name="nextest-run" tests="1" skipped="0" failures="0" errors="0"><testsuite name="s" tests="0" skipped="0" failures="0" errors="0"/></testsuites>"#,
            br#"<testsuites name="nextest-run" tests="0" skipped="0" failures="0" errors="0"><![CDATA[forged]]></testsuites>"#,
        ];
        for xml in cases {
            assert_eq!(
                parse(Some(xml), b"", b"", 4),
                Err(MiriParseError::InvalidJunit)
            );
        }
    }

    #[test]
    fn all_input_and_identity_limits_fail_closed() {
        let huge_stream = vec![b'x'; MAX_STREAM_BYTES + 1];
        assert_eq!(
            parse(Some(CLEAN), &huge_stream, b"", 0),
            Err(MiriParseError::InputLimit)
        );
        let huge_junit = vec![b'x'; MAX_JUNIT_BYTES + 1];
        assert_eq!(
            parse(Some(&huge_junit), b"", b"", 0),
            Err(MiriParseError::InputLimit)
        );
        let identity = "x".repeat(MAX_IDENTITY_BYTES + 1);
        let xml = format!(
            r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="0" errors="0"><testsuite name="s" tests="1" skipped="0" failures="0" errors="0"><testcase name="{identity}" classname="b"/></testsuite></testsuites>"#
        );
        assert_eq!(
            parse(Some(xml.as_bytes()), b"", b"", 0),
            Err(MiriParseError::InputLimit)
        );

        let cases = r#"<testcase name="t" classname="b"/>"#.repeat(MAX_TESTCASES + 1);
        let xml = format!(
            r#"<testsuites name="nextest-run" tests="4097" skipped="0" failures="0" errors="0"><testsuite name="s" tests="4097" skipped="0" failures="0" errors="0">{cases}</testsuite></testsuites>"#
        );
        assert!(xml.len() < MAX_JUNIT_BYTES);
        assert_eq!(
            parse(Some(xml.as_bytes()), b"", b"", 0),
            Err(MiriParseError::InputLimit)
        );

        let text = "x".repeat(MAX_TEXT_BYTES + 1);
        let xml = format!(
            r#"<testsuites name="nextest-run" tests="1" skipped="0" failures="1" errors="0"><testsuite name="s" tests="1" skipped="0" failures="1" errors="0"><testcase name="t" classname="b"><failure type="test failure with exit code 101"/><system-err>{text}</system-err></testcase></testsuite></testsuites>"#
        );
        assert!(xml.len() < MAX_JUNIT_BYTES);
        assert_eq!(
            parse(Some(xml.as_bytes()), b"", b"", 100),
            Err(MiriParseError::InputLimit)
        );

        let node_cases = r#"<testcase name="t" classname="b"><failure type="test failure with exit code 101"/><system-out/><system-err/></testcase>"#.repeat(3_300);
        let xml = format!(
            r#"<testsuites name="nextest-run" tests="3300" skipped="0" failures="3300" errors="0"><testsuite name="s" tests="3300" skipped="0" failures="3300" errors="0">{node_cases}</testsuite></testsuites>"#
        );
        assert!(xml.len() < MAX_JUNIT_BYTES, "{}", xml.len());
        assert_eq!(
            parse(Some(xml.as_bytes()), b"", b"", 100),
            Err(MiriParseError::InputLimit)
        );
    }

    #[test]
    fn findings_are_bounded_without_losing_exact_counts() -> Result<(), &'static str> {
        let mut cases = String::new();
        for index in 0..129 {
            cases.push_str(&format!(
                r#"<testcase name="t{index}" classname="b"><failure type="test failure with exit code 101"/><system-err/></testcase>"#
            ));
        }
        let xml = format!(
            r#"<testsuites name="nextest-run" tests="129" skipped="0" failures="129" errors="0"><testsuite name="s" tests="129" skipped="0" failures="129" errors="0">{cases}</testsuite></testsuites>"#
        );
        let report = parsed(xml.as_bytes(), 100)?;
        assert_eq!(report.counts.tests, 129);
        assert_eq!(report.counts.failed, 129);
        assert_eq!(report.counts.test_failures, 129);
        assert_eq!(report.findings.len(), MAX_FINDINGS);
        assert_eq!(report.findings_omitted, 1);
        assert!(!report.complete);
        assert!(report.validate());
        Ok(())
    }
}
