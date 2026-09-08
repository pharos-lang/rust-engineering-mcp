//! Strict, bounded parser for cargo-deny 0.19.7 JSON-lines output.
//!
//! The accepted shapes and rule codes are pinned to upstream commit
//! `759a4946dcfe93a56fb42d464e193c4c448af4e3`. Guest messages, labels, notes,
//! timestamps and paths are validated structurally but never copied into a
//! finding. This keeps unredacted guest text out of the domain result; the
//! security-log normalizer hashes the retained streams and publishes no raw
//! guest output.

use rust_engineering_domain::security::{
    FindingDisposition, SECURITY_MAX_FINDINGS, SecurityCounts, SecurityEngine, SecurityFinding,
    SecurityPackage, SecuritySeverity,
};
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use std::{collections::BTreeMap, fmt};

const MAX_STDERR_BYTES: usize = 4 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 128 * 1024;
const MAX_ROWS: usize = 4096;
const MAX_JSON_DEPTH: u32 = 128;
const MAX_JSON_NODES: usize = 65_536;
const MAX_CONTAINER_ELEMENTS: usize = 4096;
const MAX_STRING_BYTES: usize = 8192;
const MAX_GRAPH_DEPTH: usize = 32;
const MAX_GRAPH_NODES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParsedDeny {
    pub(crate) findings: Vec<SecurityFinding>,
    pub(crate) findings_omitted: u64,
    pub(crate) bans: SecurityCounts,
    pub(crate) licenses: SecurityCounts,
    pub(crate) sources: SecurityCounts,
    pub(crate) parse_complete: bool,
    /// A JSON log at ERROR level invalidates positive evidence even when the
    /// plugin subsequently emits a self-consistent summary.
    pub(crate) error_log_seen: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DenyParseError {
    TooLarge,
    InvalidUtf8,
    InvalidJson,
    InvalidShape,
    DuplicateKey,
    LimitExceeded,
    MissingSummary,
    MultipleSummary,
    SummaryNotFinal,
    CountMismatch,
    ExitMismatch,
    UnexpectedStdout,
}

impl fmt::Display for DenyParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::TooLarge => "cargo-deny JSON exceeded its byte limit",
            Self::InvalidUtf8 => "cargo-deny JSON was not UTF-8",
            Self::InvalidJson => "cargo-deny emitted malformed JSON",
            Self::InvalidShape => "cargo-deny emitted an unsupported JSON shape",
            Self::DuplicateKey => "cargo-deny JSON contained a duplicate key",
            Self::LimitExceeded => "cargo-deny JSON exceeded a structural limit",
            Self::MissingSummary => "cargo-deny did not emit a summary",
            Self::MultipleSummary => "cargo-deny emitted more than one summary",
            Self::SummaryNotFinal => "cargo-deny summary was not the final record",
            Self::CountMismatch => "cargo-deny summary did not match its diagnostics",
            Self::ExitMismatch => "cargo-deny exit code did not match its summary",
            Self::UnexpectedStdout => "cargo-deny emitted unexpected stdout",
        })
    }
}

impl std::error::Error for DenyParseError {}

#[derive(Debug)]
enum JsonNode {
    Null,
    Bool(bool),
    Unsigned(u64),
    Signed(i64),
    Floating(f64),
    String(String),
    Array(Vec<JsonNode>),
    Object(BTreeMap<String, JsonNode>),
}

#[derive(Default)]
struct JsonBudget {
    nodes: usize,
}

impl JsonBudget {
    fn add_node<E: de::Error>(&mut self) -> Result<(), E> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| E::custom("node limit"))?;
        if self.nodes > MAX_JSON_NODES {
            return Err(E::custom("node limit"));
        }
        Ok(())
    }
}

struct NodeSeed<'a> {
    depth: u32,
    budget: &'a mut JsonBudget,
}

impl<'de> DeserializeSeed<'de> for NodeSeed<'_> {
    type Value = JsonNode;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.budget.add_node()?;
        deserializer.deserialize_any(NodeVisitor {
            depth: self.depth,
            budget: self.budget,
        })
    }
}

struct NodeVisitor<'a> {
    depth: u32,
    budget: &'a mut JsonBudget,
}

impl<'de> Visitor<'de> for NodeVisitor<'_> {
    type Value = JsonNode;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("bounded cargo-deny JSON")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(JsonNode::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(JsonNode::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
        Ok(JsonNode::Bool(value))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        Ok(JsonNode::Unsigned(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(JsonNode::Signed(value))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        if !value.is_finite() {
            return Err(E::custom("non-finite floating point value"));
        }
        Ok(JsonNode::Floating(value))
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        if value.len() > MAX_STRING_BYTES {
            return Err(E::custom("string limit"));
        }
        Ok(JsonNode::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        if value.len() > MAX_STRING_BYTES {
            return Err(E::custom("string limit"));
        }
        Ok(JsonNode::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let depth = self
            .depth
            .checked_sub(1)
            .ok_or_else(|| de::Error::custom("depth limit"))?;
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(NodeSeed {
            depth,
            budget: &mut *self.budget,
        })? {
            values.push(value);
            if values.len() > MAX_CONTAINER_ELEMENTS {
                return Err(de::Error::custom("element limit"));
            }
        }
        Ok(JsonNode::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let depth = self
            .depth
            .checked_sub(1)
            .ok_or_else(|| de::Error::custom("depth limit"))?;
        let mut values = BTreeMap::new();
        while let Some(key) = map.next_key_seed(KeySeed)? {
            let value = map.next_value_seed(NodeSeed {
                depth,
                budget: &mut *self.budget,
            })?;
            if values.insert(key, value).is_some() {
                return Err(de::Error::custom("duplicate key"));
            }
            if values.len() > MAX_CONTAINER_ELEMENTS {
                return Err(de::Error::custom("element limit"));
            }
        }
        Ok(JsonNode::Object(values))
    }
}

struct KeySeed;

impl<'de> DeserializeSeed<'de> for KeySeed {
    type Value = String;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_str(KeyVisitor)
    }
}

struct KeyVisitor;

impl Visitor<'_> for KeyVisitor {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded JSON key")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        if value.len() > MAX_STRING_BYTES {
            return Err(E::custom("string limit"));
        }
        Ok(value.to_owned())
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
        if value.len() > MAX_STRING_BYTES {
            return Err(E::custom("string limit"));
        }
        Ok(value)
    }
}

fn parse_json(line: &str) -> Result<JsonNode, DenyParseError> {
    let mut budget = JsonBudget::default();
    let mut deserializer = serde_json::Deserializer::from_str(line);
    let value = NodeSeed {
        depth: MAX_JSON_DEPTH,
        budget: &mut budget,
    }
    .deserialize(&mut deserializer)
    .map_err(|error| {
        let message = error.to_string();
        if message.contains("duplicate key") {
            DenyParseError::DuplicateKey
        } else if message.contains(" limit") || message.contains("recursion limit") {
            DenyParseError::LimitExceeded
        } else {
            DenyParseError::InvalidJson
        }
    })?;
    deserializer
        .end()
        .map_err(|_| DenyParseError::InvalidJson)?;
    Ok(value)
}

fn into_serde_value(value: JsonNode) -> Result<serde_json::Value, DenyParseError> {
    Ok(match value {
        JsonNode::Null => serde_json::Value::Null,
        JsonNode::Bool(value) => serde_json::Value::Bool(value),
        JsonNode::Unsigned(value) => serde_json::Value::Number(value.into()),
        JsonNode::Signed(value) => serde_json::Value::Number(value.into()),
        JsonNode::Floating(value) => serde_json::Value::Number(
            serde_json::Number::from_f64(value).ok_or(DenyParseError::InvalidJson)?,
        ),
        JsonNode::String(value) => serde_json::Value::String(value),
        JsonNode::Array(values) => serde_json::Value::Array(
            values
                .into_iter()
                .map(into_serde_value)
                .collect::<Result<_, _>>()?,
        ),
        JsonNode::Object(values) => serde_json::Value::Object(
            values
                .into_iter()
                .map(|(key, value)| Ok((key, into_serde_value(value)?)))
                .collect::<Result<_, DenyParseError>>()?,
        ),
    })
}

/// Shared adapter-boundary decoder for one bounded JSON document. It rejects
/// duplicate keys at every depth before producing a `Value`; callers remain
/// responsible for validating their own closed schema.
pub(super) fn strict_value(bytes: &[u8]) -> Result<serde_json::Value, DenyParseError> {
    if bytes.len() > MAX_STDERR_BYTES {
        return Err(DenyParseError::TooLarge);
    }
    let input = std::str::from_utf8(bytes).map_err(|_| DenyParseError::InvalidUtf8)?;
    into_serde_value(parse_json(input)?)
}

fn object(value: JsonNode) -> Result<BTreeMap<String, JsonNode>, DenyParseError> {
    match value {
        JsonNode::Object(value) => Ok(value),
        _ => Err(DenyParseError::InvalidShape),
    }
}

fn array(value: JsonNode) -> Result<Vec<JsonNode>, DenyParseError> {
    match value {
        JsonNode::Array(value) => Ok(value),
        _ => Err(DenyParseError::InvalidShape),
    }
}

fn string(value: JsonNode) -> Result<String, DenyParseError> {
    match value {
        JsonNode::String(value) => Ok(value),
        _ => Err(DenyParseError::InvalidShape),
    }
}

fn unsigned(value: JsonNode) -> Result<u64, DenyParseError> {
    match value {
        JsonNode::Unsigned(value) => Ok(value),
        JsonNode::Signed(value) if value >= 0 => {
            u64::try_from(value).map_err(|_| DenyParseError::InvalidShape)
        }
        _ => Err(DenyParseError::InvalidShape),
    }
}

fn take(object: &mut BTreeMap<String, JsonNode>, key: &str) -> Result<JsonNode, DenyParseError> {
    object.remove(key).ok_or(DenyParseError::InvalidShape)
}

fn ensure_empty(object: &BTreeMap<String, JsonNode>) -> Result<(), DenyParseError> {
    if object.is_empty() {
        Ok(())
    } else {
        Err(DenyParseError::InvalidShape)
    }
}

enum ParsedSeverity {
    Finding(SecuritySeverity),
    Bug,
}

fn severity(value: String) -> Result<ParsedSeverity, DenyParseError> {
    match value.as_str() {
        "error" => Ok(ParsedSeverity::Finding(SecuritySeverity::Error)),
        "warning" => Ok(ParsedSeverity::Finding(SecuritySeverity::Warning)),
        "note" => Ok(ParsedSeverity::Finding(SecuritySeverity::Note)),
        "help" => Ok(ParsedSeverity::Finding(SecuritySeverity::Help)),
        "bug" => Ok(ParsedSeverity::Bug),
        _ => Err(DenyParseError::InvalidShape),
    }
}

// `src/bans/diags.rs`, `src/licenses/diags.rs` and `src/sources/diags.rs`
// at the pinned upstream commit. These complete enum projections are the only
// rule-to-engine authority; text matching is deliberately absent.
const BANS_CODES: &[&str] = &[
    "banned",
    "allowed",
    "not-allowed",
    "duplicate",
    "skipped",
    "wildcard",
    "unmatched-skip",
    "unnecessary-skip",
    "allowed-by-wrapper",
    "unmatched-wrapper",
    "skipped-by-root",
    "unmatched-skip-root",
    "build-script-not-allowed",
    "exact-features-mismatch",
    "feature-not-explicitly-allowed",
    "feature-banned",
    "unknown-feature",
    "default-feature-enabled",
    "path-bypassed",
    "path-bypassed-by-glob",
    "checksum-match",
    "checksum-mismatch",
    "denied-by-extension",
    "detected-executable",
    "detected-executable-script",
    "unable-to-check-path",
    "features-enabled",
    "unmatched-bypass",
    "unmatched-path-bypass",
    "unmatched-glob",
    "unused-wrapper",
    "workspace-duplicate",
    "unresolved-workspace-dependency",
    "unused-workspace-dependency",
    "non-utf8-path",
    "non-root-path",
];
const LICENSE_CODES: &[&str] = &[
    "accepted",
    "rejected",
    "unlicensed",
    "skipped-private-workspace-crate",
    "license-not-encountered",
    "license-exception-not-encountered",
    "missing-clarification-file",
    "parse-error",
    "empty-license-field",
    "no-license-field",
    "gather-failure",
];
const SOURCE_CODES: &[&str] = &[
    "git-source-underspecified",
    "allowed-source",
    "allowed-by-organization",
    "source-not-allowed",
    "unmatched-source",
    "unmatched-organization",
];

fn rule_engine(code: &str) -> Option<SecurityEngine> {
    if BANS_CODES.contains(&code) {
        Some(SecurityEngine::Bans)
    } else if LICENSE_CODES.contains(&code) {
        Some(SecurityEngine::Licenses)
    } else if SOURCE_CODES.contains(&code) {
        Some(SecurityEngine::Sources)
    } else {
        None
    }
}

fn validate_label(value: JsonNode) -> Result<(), DenyParseError> {
    let mut label = object(value)?;
    let _ = string(take(&mut label, "message")?)?;
    let _ = string(take(&mut label, "span")?)?;
    let line = unsigned(take(&mut label, "line")?)?;
    let column = unsigned(take(&mut label, "column")?)?;
    if line == 0 || column == 0 || line > u64::from(u32::MAX) || column > u64::from(u32::MAX) {
        return Err(DenyParseError::InvalidShape);
    }
    ensure_empty(&label)
}

fn validate_graph_node(
    value: JsonNode,
    depth: usize,
    nodes: &mut usize,
) -> Result<Option<(String, String)>, DenyParseError> {
    if depth > MAX_GRAPH_DEPTH {
        return Err(DenyParseError::LimitExceeded);
    }
    *nodes = nodes.checked_add(1).ok_or(DenyParseError::LimitExceeded)?;
    if *nodes > MAX_GRAPH_NODES {
        return Err(DenyParseError::LimitExceeded);
    }

    let mut graph = object(value)?;
    let identity = match (graph.remove("Krate"), graph.remove("Feature")) {
        (Some(value), None) => {
            let mut krate = object(value)?;
            let name = string(take(&mut krate, "name")?)?;
            let version = string(take(&mut krate, "version")?)?;
            if name.is_empty() || semver::Version::parse(&version).is_err() {
                return Err(DenyParseError::InvalidShape);
            }
            if let Some(kind) = krate.remove("kind")
                && !matches!(string(kind)?.as_str(), "dev" | "build")
            {
                return Err(DenyParseError::InvalidShape);
            }
            ensure_empty(&krate)?;
            Some((name, version))
        }
        (None, Some(value)) => {
            let mut feature = object(value)?;
            if string(take(&mut feature, "crate_name")?)?.is_empty()
                || string(take(&mut feature, "name")?)?.is_empty()
            {
                return Err(DenyParseError::InvalidShape);
            }
            ensure_empty(&feature)?;
            None
        }
        _ => return Err(DenyParseError::InvalidShape),
    };

    if let Some(repeat) = graph.remove("repeat")
        && !matches!(repeat, JsonNode::Bool(true))
    {
        return Err(DenyParseError::InvalidShape);
    }
    if let Some(parents) = graph.remove("parents") {
        let parents = array(parents)?;
        if parents.is_empty() {
            return Err(DenyParseError::InvalidShape);
        }
        for parent in parents {
            let _ = validate_graph_node(parent, depth + 1, nodes)?;
        }
    }
    ensure_empty(&graph)?;
    Ok(identity)
}

fn package_from_graphs(
    value: Option<JsonNode>,
    packages: &[SecurityPackage],
) -> Result<Option<SecurityPackage>, DenyParseError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let graphs = array(value)?;
    let mut nodes = 0usize;
    let mut roots = Vec::new();
    for graph in graphs {
        // The pinned grapher wraps a feature-rooted graph in its crate root.
        roots.push(validate_graph_node(graph, 0, &mut nodes)?.ok_or(DenyParseError::InvalidShape)?);
    }
    if roots.len() != 1 {
        return Ok(None);
    }
    let (name, version) = &roots[0];
    let mut matches = packages
        .iter()
        .filter(|package| package.name == *name && package.version == *version);
    let first = matches.next();
    if first.is_some() && matches.next().is_none() {
        Ok(first.cloned())
    } else {
        Ok(None)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct AllCounts {
    bans: SecurityCounts,
    licenses: SecurityCounts,
    sources: SecurityCounts,
}

fn counts_mut(
    counts: &mut AllCounts,
    engine: SecurityEngine,
) -> Result<&mut SecurityCounts, DenyParseError> {
    Ok(match engine {
        SecurityEngine::Bans => &mut counts.bans,
        SecurityEngine::Licenses => &mut counts.licenses,
        SecurityEngine::Sources => &mut counts.sources,
        SecurityEngine::Rustsec => return Err(DenyParseError::InvalidShape),
    })
}

fn increment(counts: &mut SecurityCounts, severity: SecuritySeverity) {
    match severity {
        SecuritySeverity::Error => counts.errors += 1,
        SecuritySeverity::Warning => counts.warnings += 1,
        SecuritySeverity::Note => counts.notes += 1,
        SecuritySeverity::Help => counts.helps += 1,
    }
}

enum ParsedDiagnostic {
    Finding(Box<SecurityFinding>),
    UnresolvedWorkspaceDependency(SecurityPackage),
}

fn parse_diagnostic(
    fields: JsonNode,
    packages: &[SecurityPackage],
) -> Result<ParsedDiagnostic, DenyParseError> {
    let mut fields = object(fields)?;
    let severity = severity(string(take(&mut fields, "severity")?)?)?;
    // Validate but deliberately discard all guest prose.
    let _ = string(take(&mut fields, "message")?)?;
    let rule = string(take(&mut fields, "code")?)?;
    let engine = rule_engine(&rule).ok_or(DenyParseError::InvalidShape)?;

    let labels = if let Some(labels) = fields.remove("labels") {
        let labels = array(labels)?;
        let count = labels.len();
        for label in labels {
            validate_label(label)?;
        }
        Some(count)
    } else {
        None
    };
    let has_notes = if let Some(notes) = fields.remove("notes") {
        for note in array(notes)? {
            let _ = string(note)?;
        }
        true
    } else {
        false
    };
    let package = package_from_graphs(fields.remove("graphs"), packages)?;
    ensure_empty(&fields)?;

    match severity {
        ParsedSeverity::Finding(severity) => {
            Ok(ParsedDiagnostic::Finding(Box::new(SecurityFinding {
                engine,
                message: format!("cargo-deny reported rule '{rule}'"),
                rule,
                package,
                severity,
                disposition: FindingDisposition::Active,
            })))
        }
        // cargo-deny 0.19.7 emits this exact diagnostic while resolving an
        // unversioned inherited workspace dependency. Its own stats explicitly
        // exclude Severity::Bug, and the paired wildcard diagnostic carries the
        // policy result. No other bug-level record is accepted.
        ParsedSeverity::Bug
            if rule == "unresolved-workspace-dependency" && labels == Some(2) && !has_notes =>
        {
            package
                .map(ParsedDiagnostic::UnresolvedWorkspaceDependency)
                .ok_or(DenyParseError::InvalidShape)
        }
        ParsedSeverity::Bug => Err(DenyParseError::InvalidShape),
    }
}

fn parse_counts(value: JsonNode) -> Result<SecurityCounts, DenyParseError> {
    let mut counts = object(value)?;
    let errors = u32::try_from(unsigned(take(&mut counts, "errors")?)?)
        .map_err(|_| DenyParseError::InvalidShape)?;
    let warnings = u32::try_from(unsigned(take(&mut counts, "warnings")?)?)
        .map_err(|_| DenyParseError::InvalidShape)?;
    let notes = u32::try_from(unsigned(take(&mut counts, "notes")?)?)
        .map_err(|_| DenyParseError::InvalidShape)?;
    let helps = u32::try_from(unsigned(take(&mut counts, "helps")?)?)
        .map_err(|_| DenyParseError::InvalidShape)?;
    ensure_empty(&counts)?;
    Ok(SecurityCounts {
        errors,
        warnings,
        notes,
        helps,
    })
}

fn parse_summary(fields: JsonNode) -> Result<AllCounts, DenyParseError> {
    let mut fields = object(fields)?;
    let bans = parse_counts(take(&mut fields, "bans")?)?;
    let licenses = parse_counts(take(&mut fields, "licenses")?)?;
    let sources = parse_counts(take(&mut fields, "sources")?)?;
    // In particular, an `advisories` member is rejected.
    ensure_empty(&fields)?;
    Ok(AllCounts {
        bans,
        licenses,
        sources,
    })
}

fn parse_log(fields: JsonNode) -> Result<bool, DenyParseError> {
    let mut fields = object(fields)?;
    let timestamp = string(take(&mut fields, "timestamp")?)?;
    let level = string(take(&mut fields, "level")?)?;
    let _ = string(take(&mut fields, "message")?)?;
    ensure_empty(&fields)?;
    if timestamp.is_empty() {
        return Err(DenyParseError::InvalidShape);
    }
    match level.as_str() {
        "ERROR" => Ok(true),
        "WARN" | "INFO" | "DEBUG" | "TRACE" => Ok(false),
        _ => Err(DenyParseError::InvalidShape),
    }
}

enum Record {
    Diagnostic(ParsedDiagnostic),
    Log { error: bool },
    Summary(AllCounts),
}

fn parse_record(line: &str, packages: &[SecurityPackage]) -> Result<Record, DenyParseError> {
    let mut record = object(parse_json(line)?)?;
    let kind = string(take(&mut record, "type")?)?;
    let fields = take(&mut record, "fields")?;
    ensure_empty(&record)?;
    match kind.as_str() {
        "diagnostic" => Ok(Record::Diagnostic(parse_diagnostic(fields, packages)?)),
        "log" => Ok(Record::Log {
            error: parse_log(fields)?,
        }),
        "summary" => Ok(Record::Summary(parse_summary(fields)?)),
        _ => Err(DenyParseError::InvalidShape),
    }
}

/// Parses the pinned cargo-deny JSON-lines stream. A successful parse is not by
/// itself passing evidence: callers must also require `parse_complete`.
pub(crate) fn parse(
    stderr: &[u8],
    stdout: &[u8],
    exit_code: i32,
    truncated: bool,
    packages: &[SecurityPackage],
) -> Result<ParsedDeny, DenyParseError> {
    if truncated {
        return Err(DenyParseError::LimitExceeded);
    }
    if !stdout.is_empty() {
        return Err(DenyParseError::UnexpectedStdout);
    }
    if stderr.len() > MAX_STDERR_BYTES {
        return Err(DenyParseError::TooLarge);
    }
    let stderr = std::str::from_utf8(stderr).map_err(|_| DenyParseError::InvalidUtf8)?;
    if !stderr.ends_with('\n') {
        return Err(DenyParseError::InvalidJson);
    }

    let mut observed = AllCounts::default();
    let mut summary = None;
    let mut findings = Vec::new();
    let mut findings_omitted = 0u64;
    let mut workspace_resolution_bugs = Vec::new();
    let mut wildcard_packages = Vec::new();
    let mut error_log_seen = false;
    let mut rows = 0usize;

    for line in stderr.split_inclusive('\n') {
        rows += 1;
        if rows > MAX_ROWS || line.len() > MAX_LINE_BYTES {
            return Err(DenyParseError::LimitExceeded);
        }
        if line.trim().is_empty() {
            return Err(DenyParseError::InvalidJson);
        }
        let record = parse_record(line, packages)?;
        if summary.is_some() {
            return Err(match record {
                Record::Summary(_) => DenyParseError::MultipleSummary,
                _ => DenyParseError::SummaryNotFinal,
            });
        }
        match record {
            Record::Diagnostic(diagnostic) => match diagnostic {
                ParsedDiagnostic::Finding(finding) => {
                    let finding = *finding;
                    increment(counts_mut(&mut observed, finding.engine)?, finding.severity);
                    if finding.engine == SecurityEngine::Bans && finding.rule == "wildcard" {
                        let package = finding
                            .package
                            .as_ref()
                            .ok_or(DenyParseError::InvalidShape)?;
                        wildcard_packages.push(package.clone());
                    }
                    if findings.len() < SECURITY_MAX_FINDINGS {
                        findings.push(finding);
                    } else {
                        findings_omitted = findings_omitted.saturating_add(1);
                    }
                }
                ParsedDiagnostic::UnresolvedWorkspaceDependency(package) => {
                    workspace_resolution_bugs.push(package);
                }
            },
            Record::Log { error } => error_log_seen |= error,
            Record::Summary(counts) => summary = Some(counts),
        }
    }

    let summary = summary.ok_or(DenyParseError::MissingSummary)?;
    if summary != observed {
        return Err(DenyParseError::CountMismatch);
    }
    if workspace_resolution_bugs
        .iter()
        .any(|package| !wildcard_packages.contains(package))
    {
        return Err(DenyParseError::InvalidShape);
    }
    let expected_exit = (i32::from(summary.bans.errors > 0) * 2)
        | (i32::from(summary.licenses.errors > 0) * 4)
        | (i32::from(summary.sources.errors > 0) * 8);
    if exit_code != expected_exit {
        return Err(DenyParseError::ExitMismatch);
    }

    Ok(ParsedDeny {
        findings,
        findings_omitted,
        bans: summary.bans,
        licenses: summary.licenses,
        sources: summary.sources,
        parse_complete: !error_log_seen,
        error_log_seen,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::security::SecuritySource;
    use serde_json::json;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn package(name: &str, version: &str) -> SecurityPackage {
        SecurityPackage {
            name: name.to_owned(),
            version: version.to_owned(),
            source: SecuritySource::Workspace,
            source_fingerprint: None,
        }
    }

    fn summary(bans: SecurityCounts, licenses: SecurityCounts, sources: SecurityCounts) -> String {
        format!(
            "{}\n",
            json!({"type":"summary","fields":{
                "bans":{"errors":bans.errors,"warnings":bans.warnings,"notes":bans.notes,"helps":bans.helps},
                "licenses":{"errors":licenses.errors,"warnings":licenses.warnings,"notes":licenses.notes,"helps":licenses.helps},
                "sources":{"errors":sources.errors,"warnings":sources.warnings,"notes":sources.notes,"helps":sources.helps}
            }})
        )
    }

    fn diagnostic(code: &str, severity: &str, name: &str, version: &str) -> String {
        format!(
            "{}\n",
            json!({"type":"diagnostic","fields":{
                "severity":severity,
                "message":"hostile /Users/alice/.ssh/id_ed25519 secret-token",
                "code":code,
                "graphs":[{"Krate":{"name":name,"version":version}}]
            }})
        )
    }

    fn unresolved_workspace(name: &str, version: &str) -> String {
        format!(
            "{}\n",
            json!({"type":"diagnostic","fields":{
                "severity":"bug",
                "message":"failed to resolve a workspace dependency",
                "code":"unresolved-workspace-dependency",
                "labels":[
                    {"message":"usage of workspace dependency","span":"true","line":8,"column":19},
                    {"message":"","span":"true","line":8,"column":19}
                ],
                "graphs":[{"Krate":{"name":name,"version":version}}]
            }})
        )
    }

    #[test]
    fn parses_clean_and_native_0197_oracles() -> TestResult {
        let clean = parse(
            include_bytes!("../../../fixtures/m4-deny-parser/clean.stderr.jsonl"),
            b"",
            0,
            false,
            &[],
        )?;
        assert!(clean.parse_complete);
        assert!(clean.findings.is_empty());

        let native = parse(
            include_bytes!("../../../fixtures/m4-deny-parser/native-license-missing.stderr.jsonl"),
            b"",
            4,
            false,
            &[package("m4-probe", "0.0.0")],
        )?;
        assert!(native.parse_complete);
        assert_eq!(native.findings.len(), 3);
        assert_eq!(native.licenses.errors, 1);
        assert_eq!(native.licenses.warnings, 2);
        assert!(native.findings.iter().all(|finding| {
            finding
                .package
                .as_ref()
                .is_some_and(|package| package.name == "m4-probe")
        }));
        Ok(())
    }

    #[test]
    fn parses_four_gateway_native_goldens_with_exact_counts_rules_and_packages() -> TestResult {
        struct NativeCase {
            stderr: &'static [u8],
            stdout: &'static [u8],
            exit_code: i32,
            bans: SecurityCounts,
            licenses: SecurityCounts,
            rules: &'static [&'static str],
        }

        let cases = [
            NativeCase {
                stderr: include_bytes!(
                    "../../../fixtures/m4-deny-parser/native-clean.stderr.jsonl"
                ),
                stdout: include_bytes!("../../../fixtures/m4-deny-parser/native-clean.stdout"),
                exit_code: 0,
                bans: SecurityCounts::default(),
                licenses: SecurityCounts {
                    warnings: 1,
                    helps: 1,
                    ..SecurityCounts::default()
                },
                rules: &["no-license-field", "accepted"],
            },
            NativeCase {
                stderr: include_bytes!(
                    "../../../fixtures/m4-deny-parser/native-manifest-without-text.stderr.jsonl"
                ),
                stdout: include_bytes!(
                    "../../../fixtures/m4-deny-parser/native-manifest-without-text.stdout"
                ),
                exit_code: 4,
                bans: SecurityCounts::default(),
                licenses: SecurityCounts {
                    errors: 1,
                    warnings: 3,
                    ..SecurityCounts::default()
                },
                rules: &[
                    "no-license-field",
                    "unlicensed",
                    "unlicensed",
                    "license-not-encountered",
                ],
            },
            NativeCase {
                stderr: include_bytes!(
                    "../../../fixtures/m4-deny-parser/native-text-not-allowed.stderr.jsonl"
                ),
                stdout: include_bytes!(
                    "../../../fixtures/m4-deny-parser/native-text-not-allowed.stdout"
                ),
                exit_code: 4,
                bans: SecurityCounts::default(),
                licenses: SecurityCounts {
                    errors: 1,
                    warnings: 2,
                    ..SecurityCounts::default()
                },
                rules: &["no-license-field", "rejected", "license-not-encountered"],
            },
            NativeCase {
                stderr: include_bytes!(
                    "../../../fixtures/m4-deny-parser/native-banned-package.stderr.jsonl"
                ),
                stdout: include_bytes!(
                    "../../../fixtures/m4-deny-parser/native-banned-package.stdout"
                ),
                exit_code: 2,
                bans: SecurityCounts {
                    errors: 1,
                    ..SecurityCounts::default()
                },
                licenses: SecurityCounts {
                    warnings: 1,
                    helps: 1,
                    ..SecurityCounts::default()
                },
                rules: &["banned", "no-license-field", "accepted"],
            },
        ];

        let packages = [package("m4-deny-probe", "0.1.0")];
        for case in cases {
            assert!(case.stdout.is_empty());
            let parsed = parse(case.stderr, case.stdout, case.exit_code, false, &packages)?;
            assert!(parsed.parse_complete);
            assert!(!parsed.error_log_seen);
            assert_eq!(parsed.findings_omitted, 0);
            assert_eq!(parsed.bans, case.bans);
            assert_eq!(parsed.licenses, case.licenses);
            assert_eq!(parsed.sources, SecurityCounts::default());
            assert_eq!(
                parsed
                    .findings
                    .iter()
                    .map(|finding| finding.rule.as_str())
                    .collect::<Vec<_>>(),
                case.rules
            );
            assert!(parsed.findings.iter().all(|finding| {
                !finding.message.contains("/source")
                    && !finding.message.contains("LICENSE")
                    && if finding.rule == "license-not-encountered" {
                        finding.package.is_none()
                    } else {
                        finding.package.as_ref().is_some_and(|package| {
                            package.name == "m4-deny-probe"
                                && package.version == "0.1.0"
                                && package.source == SecuritySource::Workspace
                        })
                    }
            }));
        }
        Ok(())
    }

    #[test]
    fn parses_workspace_parent_graph_goldens_and_excludes_paired_bug_diagnostics() -> TestResult {
        let packages = [package("app", "0.1.0"), package("helper", "0.1.0")];
        let wildcard = parse(
            include_bytes!(
                "../../../fixtures/m4-deny-parser/native-workspace-wildcard.stderr.jsonl"
            ),
            include_bytes!("../../../fixtures/m4-deny-parser/native-workspace-wildcard.stdout"),
            2,
            false,
            &packages,
        )?;
        assert!(wildcard.parse_complete);
        assert_eq!(
            wildcard.bans,
            SecurityCounts {
                errors: 1,
                ..SecurityCounts::default()
            }
        );
        assert_eq!(
            wildcard.licenses,
            SecurityCounts {
                warnings: 2,
                helps: 2,
                ..SecurityCounts::default()
            }
        );
        assert_eq!(wildcard.findings.len(), 5);
        assert_eq!(wildcard.findings_omitted, 0);
        assert_eq!(wildcard.findings[0].rule, "wildcard");
        assert_eq!(
            wildcard.findings[0]
                .package
                .as_ref()
                .map(|p| p.name.as_str()),
            Some("app")
        );
        assert!(
            wildcard.findings[1..3].iter().all(|finding| {
                finding.package.as_ref().map(|p| p.name.as_str()) == Some("app")
            })
        );
        assert!(wildcard.findings[3..].iter().all(|finding| {
            finding.package.as_ref().map(|p| p.name.as_str()) == Some("helper")
        }));

        let exact = parse(
            include_bytes!("../../../fixtures/m4-deny-parser/native-workspace-exact.stderr.jsonl"),
            include_bytes!("../../../fixtures/m4-deny-parser/native-workspace-exact.stdout"),
            0,
            false,
            &packages,
        )?;
        assert!(exact.parse_complete);
        assert_eq!(exact.bans, SecurityCounts::default());
        assert_eq!(exact.licenses, wildcard.licenses);
        assert_eq!(exact.findings.len(), 4);
        assert!(
            exact.findings[..2].iter().all(|finding| {
                finding.package.as_ref().map(|p| p.name.as_str()) == Some("app")
            })
        );
        assert!(exact.findings[2..].iter().all(|finding| {
            finding.package.as_ref().map(|p| p.name.as_str()) == Some("helper")
        }));
        Ok(())
    }

    #[test]
    fn bug_severity_is_closed_to_workspace_resolution_paired_with_wildcard() {
        let packages = [package("app", "0.1.0"), package("other", "0.1.0")];
        let mut unpaired = unresolved_workspace("app", "0.1.0");
        unpaired.push_str(&summary(
            SecurityCounts::default(),
            SecurityCounts::default(),
            SecurityCounts::default(),
        ));
        assert_eq!(
            parse(unpaired.as_bytes(), b"", 0, false, &packages),
            Err(DenyParseError::InvalidShape)
        );

        let mut wrong_code = diagnostic("wildcard", "bug", "app", "0.1.0");
        wrong_code.push_str(&summary(
            SecurityCounts::default(),
            SecurityCounts::default(),
            SecurityCounts::default(),
        ));
        assert_eq!(
            parse(wrong_code.as_bytes(), b"", 0, false, &packages),
            Err(DenyParseError::InvalidShape)
        );

        let mut wrong_package = unresolved_workspace("other", "0.1.0");
        wrong_package.push_str(&diagnostic("wildcard", "error", "app", "0.1.0"));
        wrong_package.push_str(&summary(
            SecurityCounts {
                errors: 1,
                ..SecurityCounts::default()
            },
            SecurityCounts::default(),
            SecurityCounts::default(),
        ));
        assert_eq!(
            parse(wrong_package.as_bytes(), b"", 2, false, &packages),
            Err(DenyParseError::InvalidShape)
        );
    }

    #[test]
    fn maps_only_pinned_codes_and_never_retains_guest_prose() -> TestResult {
        let all_codes: std::collections::BTreeSet<_> = BANS_CODES
            .iter()
            .chain(LICENSE_CODES)
            .chain(SOURCE_CODES)
            .copied()
            .collect();
        assert_eq!(BANS_CODES.len(), 36);
        assert_eq!(LICENSE_CODES.len(), 11);
        assert_eq!(SOURCE_CODES.len(), 6);
        assert_eq!(all_codes.len(), 53);
        assert!(
            BANS_CODES
                .iter()
                .all(|code| rule_engine(code) == Some(SecurityEngine::Bans))
        );
        assert!(
            LICENSE_CODES
                .iter()
                .all(|code| rule_engine(code) == Some(SecurityEngine::Licenses))
        );
        assert!(
            SOURCE_CODES
                .iter()
                .all(|code| rule_engine(code) == Some(SecurityEngine::Sources))
        );

        let cases = [
            ("banned", SecurityEngine::Bans),
            ("non-root-path", SecurityEngine::Bans),
            ("accepted", SecurityEngine::Licenses),
            ("gather-failure", SecurityEngine::Licenses),
            ("git-source-underspecified", SecurityEngine::Sources),
            ("unmatched-organization", SecurityEngine::Sources),
        ];
        for (code, engine) in cases {
            let mut input = diagnostic(code, "error", "pkg", "1.2.3");
            let mut bans = SecurityCounts::default();
            let mut licenses = SecurityCounts::default();
            let mut sources = SecurityCounts::default();
            match engine {
                SecurityEngine::Bans => bans.errors = 1,
                SecurityEngine::Licenses => licenses.errors = 1,
                SecurityEngine::Sources => sources.errors = 1,
                SecurityEngine::Rustsec => return Err("invalid test engine".into()),
            }
            input.push_str(&summary(bans, licenses, sources));
            let parsed = parse(
                input.as_bytes(),
                b"",
                match engine {
                    SecurityEngine::Bans => 2,
                    SecurityEngine::Licenses => 4,
                    SecurityEngine::Sources => 8,
                    SecurityEngine::Rustsec => 0,
                },
                false,
                &[package("pkg", "1.2.3")],
            )?;
            assert_eq!(parsed.findings[0].engine, engine);
            assert_eq!(parsed.findings[0].rule, code);
            assert_eq!(
                parsed.findings[0].message,
                format!("cargo-deny reported rule '{code}'")
            );
            assert!(!parsed.findings[0].message.contains("Users"));
            assert_eq!(parsed.findings[0].disposition, FindingDisposition::Active);
        }

        let mut unknown = diagnostic("future-rule", "warning", "pkg", "1.2.3");
        unknown.push_str(&summary(
            SecurityCounts::default(),
            SecurityCounts::default(),
            SecurityCounts::default(),
        ));
        assert_eq!(
            parse(unknown.as_bytes(), b"", 0, false, &[]),
            Err(DenyParseError::InvalidShape)
        );
        Ok(())
    }

    #[test]
    fn package_association_requires_one_graph_root_and_one_exact_package() -> TestResult {
        let mut input = diagnostic("banned", "warning", "pkg", "1.2.3");
        input.push_str(&summary(
            SecurityCounts {
                warnings: 1,
                ..SecurityCounts::default()
            },
            SecurityCounts::default(),
            SecurityCounts::default(),
        ));
        let exact = parse(input.as_bytes(), b"", 0, false, &[package("pkg", "1.2.3")])?;
        assert_eq!(
            exact.findings[0].package.as_ref().map(|p| p.name.as_str()),
            Some("pkg")
        );

        for packages in [
            vec![],
            vec![package("pkg", "1.2.4")],
            vec![package("pkg", "1.2.3"), package("pkg", "1.2.3")],
        ] {
            assert!(
                parse(input.as_bytes(), b"", 0, false, &packages)?.findings[0]
                    .package
                    .is_none()
            );
        }

        let ambiguous = input.replace(
            r#"[{"Krate":{"name":"pkg","version":"1.2.3"}}]"#,
            r#"[{"Krate":{"name":"pkg","version":"1.2.3"}},{"Krate":{"name":"pkg","version":"1.2.3"}}]"#,
        );
        assert!(
            parse(
                ambiguous.as_bytes(),
                b"",
                0,
                false,
                &[package("pkg", "1.2.3")]
            )?
            .findings[0]
                .package
                .is_none()
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_unknown_malformed_and_non_final_records() {
        let clean = include_str!("../../../fixtures/m4-deny-parser/clean.stderr.jsonl");
        let duplicate = clean.replacen(r#""errors":0"#, r#""errors":0,"errors":0"#, 1);
        assert_eq!(
            parse(duplicate.as_bytes(), b"", 0, false, &[]),
            Err(DenyParseError::DuplicateKey)
        );
        assert_eq!(
            parse(b"{}\n", b"", 0, false, &[]),
            Err(DenyParseError::InvalidShape)
        );
        assert_eq!(
            parse(b"{\n", b"", 0, false, &[]),
            Err(DenyParseError::InvalidJson)
        );
        let two = format!("{clean}{clean}");
        assert_eq!(
            parse(two.as_bytes(), b"", 0, false, &[]),
            Err(DenyParseError::MultipleSummary)
        );
        let after = format!(
            "{clean}{}",
            json!({"type":"log","fields":{"timestamp":"now","level":"INFO","message":"x"}})
        ) + "\n";
        assert_eq!(
            parse(after.as_bytes(), b"", 0, false, &[]),
            Err(DenyParseError::SummaryNotFinal)
        );
    }

    #[test]
    fn rejects_summary_count_exit_stdout_and_capture_mismatches() {
        let clean = include_bytes!("../../../fixtures/m4-deny-parser/clean.stderr.jsonl");
        assert_eq!(
            parse(clean, b"unexpected", 0, false, &[]),
            Err(DenyParseError::UnexpectedStdout)
        );
        assert_eq!(
            parse(clean, b"", 0, true, &[]),
            Err(DenyParseError::LimitExceeded)
        );
        assert_eq!(
            parse(clean, b"", 2, false, &[]),
            Err(DenyParseError::ExitMismatch)
        );
        let mut missing = diagnostic("banned", "error", "pkg", "1.0.0");
        missing.push_str(&summary(
            SecurityCounts::default(),
            SecurityCounts::default(),
            SecurityCounts::default(),
        ));
        assert_eq!(
            parse(missing.as_bytes(), b"", 0, false, &[]),
            Err(DenyParseError::CountMismatch)
        );
        assert_eq!(
            parse(b"", b"", 0, false, &[]),
            Err(DenyParseError::InvalidJson)
        );

        let advisories = String::from_utf8_lossy(clean).replace(
            r#""bans":{"#,
            r#""advisories":{"errors":0,"helps":0,"notes":0,"warnings":0},"bans":{"#,
        );
        assert_eq!(
            parse(advisories.as_bytes(), b"", 0, false, &[]),
            Err(DenyParseError::InvalidShape)
        );
    }

    #[test]
    fn counts_every_supported_severity_before_applying_the_visible_cap() -> TestResult {
        let mut input = String::new();
        for severity in ["error", "warning", "note", "help"] {
            input.push_str(&diagnostic("source-not-allowed", severity, "pkg", "1.0.0"));
        }
        input.push_str(&summary(
            SecurityCounts::default(),
            SecurityCounts::default(),
            SecurityCounts {
                errors: 1,
                warnings: 1,
                notes: 1,
                helps: 1,
            },
        ));
        let parsed = parse(input.as_bytes(), b"", 8, false, &[])?;
        assert_eq!(parsed.sources.total(), 4);
        assert_eq!(parsed.findings.len(), 4);
        assert!(parsed.parse_complete);
        Ok(())
    }

    #[test]
    fn error_logs_and_unknown_levels_cannot_be_positive_evidence() -> TestResult {
        let log = |level: &str| {
            format!(
                "{}\n",
                json!({"type":"log","fields":{"timestamp":"2026-09-07T00:00:00Z","level":level,"message":"/host/secret"}})
            )
        };
        let mut error = log("ERROR");
        error.push_str(include_str!(
            "../../../fixtures/m4-deny-parser/clean.stderr.jsonl"
        ));
        let parsed = parse(error.as_bytes(), b"", 0, false, &[])?;
        assert!(parsed.error_log_seen);
        assert!(!parsed.parse_complete);

        let mut unknown = log("NOTICE");
        unknown.push_str(include_str!(
            "../../../fixtures/m4-deny-parser/clean.stderr.jsonl"
        ));
        assert_eq!(
            parse(unknown.as_bytes(), b"", 0, false, &[]),
            Err(DenyParseError::InvalidShape)
        );
        Ok(())
    }

    #[test]
    fn caps_visible_findings_without_losing_total_counts() -> TestResult {
        let total = SECURITY_MAX_FINDINGS + 5;
        let mut input = String::new();
        for _ in 0..total {
            input.push_str(&diagnostic("banned", "warning", "pkg", "1.0.0"));
        }
        input.push_str(&summary(
            SecurityCounts {
                warnings: u32::try_from(total)?,
                ..SecurityCounts::default()
            },
            SecurityCounts::default(),
            SecurityCounts::default(),
        ));
        let parsed = parse(input.as_bytes(), b"", 0, false, &[])?;
        assert_eq!(parsed.findings.len(), SECURITY_MAX_FINDINGS);
        assert_eq!(parsed.findings_omitted, 5);
        assert_eq!(parsed.bans.warnings, u32::try_from(total)?);
        assert!(parsed.parse_complete);
        Ok(())
    }

    #[test]
    fn structural_limits_fail_closed() {
        let mut oversized = vec![b'x'; MAX_STDERR_BYTES + 1];
        oversized.push(b'\n');
        assert_eq!(
            parse(&oversized, b"", 0, false, &[]),
            Err(DenyParseError::TooLarge)
        );

        let deep = format!(
            "{{\"type\":\"log\",\"fields\":{{\"timestamp\":{},\"level\":\"INFO\",\"message\":\"x\"}}}}\n",
            "[".repeat(MAX_JSON_DEPTH as usize + 1) + &"]".repeat(MAX_JSON_DEPTH as usize + 1)
        );
        assert!(matches!(
            parse(deep.as_bytes(), b"", 0, false, &[]),
            Err(DenyParseError::LimitExceeded | DenyParseError::InvalidJson)
        ));
    }

    #[test]
    fn shared_strict_decoder_preserves_finite_metadata_floats_and_rejects_duplicates() {
        assert_eq!(
            strict_value(br#"{"package":{"metadata":{"threshold":0.95}}}"#)
                .ok()
                .and_then(|value| value.pointer("/package/metadata/threshold").cloned()),
            Some(json!(0.95))
        );
        assert_eq!(
            strict_value(br#"{"package":{"name":"a","name":"b"}}"#),
            Err(DenyParseError::DuplicateKey)
        );
    }
}
