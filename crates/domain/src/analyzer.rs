//! Analyzer wire values: validated positions, symbols and bounded edits.
//!
//! Everything here is serde-only and byte-in/byte-out. No I/O, no process
//! handling and no untyped JSON values (forbidden for domain by
//! `scripts/check-architecture.py`); the execution adapter owns the loosely
//! typed LSP wire DTOs and converts into these validated values.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::{NonEmptyText, Position, SourceFingerprint};

/// Closed failure set for every analyzer domain constructor and conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyzerError {
    NotUtf8,
    OffsetInsideCodePoint,
    OutOfRange,
    InvalidRange,
    OverlappingRanges,
    UnknownSymbolKind,
    UnknownSeverity,
    Invalid,
    LimitExceeded,
}

impl fmt::Display for AnalyzerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotUtf8 => "source bytes are not valid UTF-8",
            Self::OffsetInsideCodePoint => "offset falls inside a code point or surrogate pair",
            Self::OutOfRange => "position or offset is outside the file",
            Self::InvalidRange => "range end precedes range start",
            Self::OverlappingRanges => "edits overlap",
            Self::UnknownSymbolKind => "LSP SymbolKind is outside 1..=26",
            Self::UnknownSeverity => "LSP DiagnosticSeverity is outside 1..=4",
            Self::Invalid => "value fails a domain invariant",
            Self::LimitExceeded => "value exceeds a closed analyzer limit",
        })
    }
}

impl std::error::Error for AnalyzerError {}

/// Bounded budgets shared by the analyzer domain, codec and later packages.
pub const MAX_VISIBLE_RESULTS: usize = 512;
/// The deepest nesting level a [`DocumentSymbol`] may carry.
///
/// Contract: this is the single source of truth for the depth bound. A
/// converter walking a hierarchical server result must stop descending once
/// the next level would exceed it and count the skipped entries as omissions;
/// [`DocumentSymbol::new`] rejects any value above it, so exceeding the bound
/// can never produce a value, only an error or a counted omission.
pub const MAX_SYMBOL_DEPTH: u8 = 32;
pub const MAX_ACTIONS: usize = 32;
pub const MAX_EDITS: usize = 128;
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
pub const MAX_MESSAGES_PER_JOB: usize = 4096;
pub const MAX_STDOUT_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_STDERR_BYTES: usize = 1024 * 1024;
pub const INITIALIZE_TIMEOUT_SECONDS: u64 = 60;
pub const QUERY_TIMEOUT_SECONDS: u64 = 30;
pub const TOTAL_TIMEOUT_SECONDS: u64 = 180;
pub const MAX_RESULT_BYTES: usize = 512 * 1024;

/// A relative POSIX path inside the capture, ending in `.rs`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AnalyzerFile(String);

impl AnalyzerFile {
    pub fn new(value: String) -> Result<Self, AnalyzerError> {
        crate::validate_source_path(&value).map_err(|_| AnalyzerError::Invalid)?;
        if !value.ends_with(".rs") {
            return Err(AnalyzerError::Invalid);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for AnalyzerFile {
    type Error = AnalyzerError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<AnalyzerFile> for String {
    fn from(value: AnalyzerFile) -> Self {
        value.0
    }
}

/// A half-open-by-value range: `start <= end`, both 1-based Unicode scalar positions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TextRangeWire")]
pub struct TextRange {
    start: Position,
    end: Position,
}

impl TextRange {
    pub fn new(start: Position, end: Position) -> Result<Self, AnalyzerError> {
        if start > end {
            return Err(AnalyzerError::InvalidRange);
        }
        Ok(Self { start, end })
    }

    pub fn start(&self) -> Position {
        self.start
    }

    pub fn end(&self) -> Position {
        self.end
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextRangeWire {
    start: Position,
    end: Position,
}

impl TryFrom<TextRangeWire> for TextRange {
    type Error = AnalyzerError;

    fn try_from(value: TextRangeWire) -> Result<Self, Self::Error> {
        Self::new(value.start, value.end)
    }
}

/// Byte offsets and LSP line/character coordinates against one file's exact
/// captured bytes. Newline is `\n` only: `\r` stays inside the line, exactly
/// like rust-analyzer's own `LineIndex`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    text: String,
    /// Byte offset of the start of each line; `line_starts[0] == 0`.
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(bytes: &[u8]) -> Result<Self, AnalyzerError> {
        let text = String::from_utf8(bytes.to_vec()).map_err(|_| AnalyzerError::NotUtf8)?;
        let mut line_starts = vec![0usize];
        for (index, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }
        Ok(Self { text, line_starts })
    }

    fn line_of_offset(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            // `line_starts[0] == 0 <= offset`, so `Err` never returns 0.
            Err(index) => index - 1,
        }
    }

    /// The line's byte range, excluding a trailing `\n` if present.
    fn line_bounds(&self, line0: u32) -> Result<(usize, usize), AnalyzerError> {
        let index = line0 as usize;
        let start = *self
            .line_starts
            .get(index)
            .ok_or(AnalyzerError::OutOfRange)?;
        let raw_end = self
            .line_starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.text.len());
        let end = if raw_end > start && self.text.as_bytes()[raw_end - 1] == b'\n' {
            raw_end - 1
        } else {
            raw_end
        };
        Ok((start, end))
    }

    pub fn position_from_byte_offset(&self, offset: usize) -> Result<Position, AnalyzerError> {
        if offset > self.text.len() {
            return Err(AnalyzerError::OutOfRange);
        }
        if !self.text.is_char_boundary(offset) {
            return Err(AnalyzerError::OffsetInsideCodePoint);
        }
        let line0 = self.line_of_offset(offset);
        let start = self.line_starts[line0];
        let column = self.text[start..offset].chars().count() as u32 + 1;
        Position::new(line0 as u32 + 1, column).map_err(|_| AnalyzerError::OutOfRange)
    }

    pub fn byte_offset_from_position(&self, position: Position) -> Result<usize, AnalyzerError> {
        let line0 = position.line.get() - 1;
        let (start, end) = self.line_bounds(line0)?;
        let target_column = position.column.get() as usize - 1;
        if target_column == 0 {
            return Ok(start);
        }
        let mut offset = start;
        let mut count = 0usize;
        for ch in self.text[start..end].chars() {
            offset += ch.len_utf8();
            count += 1;
            if count == target_column {
                return Ok(offset);
            }
        }
        Err(AnalyzerError::OutOfRange)
    }

    /// `line0`/`col16` are LSP's zero-based line and UTF-16 code unit offset.
    pub fn position_from_utf16(&self, line0: u32, col16: u32) -> Result<Position, AnalyzerError> {
        let (start, end) = self.line_bounds(line0)?;
        let target = col16 as usize;
        let mut units = 0usize;
        let mut scalars = 0u32;
        for ch in self.text[start..end].chars() {
            if units == target {
                return Position::new(line0 + 1, scalars + 1)
                    .map_err(|_| AnalyzerError::OutOfRange);
            }
            let width = ch.len_utf16();
            if target < units + width {
                return Err(AnalyzerError::OffsetInsideCodePoint);
            }
            units += width;
            scalars += 1;
        }
        if units == target {
            Position::new(line0 + 1, scalars + 1).map_err(|_| AnalyzerError::OutOfRange)
        } else {
            Err(AnalyzerError::OutOfRange)
        }
    }

    /// `line0`/`col8` are LSP's zero-based line and byte offset within the
    /// line (the `positionEncoding: "utf-8"` variant, where "character"
    /// means a UTF-8 code unit, i.e. a byte).
    pub fn position_from_utf8(&self, line0: u32, col8: u32) -> Result<Position, AnalyzerError> {
        let (start, end) = self.line_bounds(line0)?;
        let offset = start
            .checked_add(col8 as usize)
            .ok_or(AnalyzerError::OutOfRange)?;
        if offset > end {
            return Err(AnalyzerError::OutOfRange);
        }
        if !self.text.is_char_boundary(offset) {
            return Err(AnalyzerError::OffsetInsideCodePoint);
        }
        let scalars = self.text[start..offset].chars().count() as u32;
        Position::new(line0 + 1, scalars + 1).map_err(|_| AnalyzerError::OutOfRange)
    }

    /// The inverse of [`Self::position_from_utf8`].
    pub fn utf8_from_position(&self, position: Position) -> Result<(u32, u32), AnalyzerError> {
        let line0 = position.line.get() - 1;
        let (start, _) = self.line_bounds(line0)?;
        let offset = self.byte_offset_from_position(position)?;
        Ok((line0, (offset - start) as u32))
    }
}

/// The closed LSP 3.17 `SymbolKind` set (values `1..=26`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    File,
    Module,
    Namespace,
    Package,
    Class,
    Method,
    Property,
    Field,
    Constructor,
    Enum,
    Interface,
    Function,
    Variable,
    Constant,
    String,
    Number,
    Boolean,
    Array,
    Object,
    Key,
    Null,
    EnumMember,
    Struct,
    Event,
    Operator,
    TypeParameter,
}

impl TryFrom<u32> for SymbolKind {
    type Error = AnalyzerError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Ok(match value {
            1 => Self::File,
            2 => Self::Module,
            3 => Self::Namespace,
            4 => Self::Package,
            5 => Self::Class,
            6 => Self::Method,
            7 => Self::Property,
            8 => Self::Field,
            9 => Self::Constructor,
            10 => Self::Enum,
            11 => Self::Interface,
            12 => Self::Function,
            13 => Self::Variable,
            14 => Self::Constant,
            15 => Self::String,
            16 => Self::Number,
            17 => Self::Boolean,
            18 => Self::Array,
            19 => Self::Object,
            20 => Self::Key,
            21 => Self::Null,
            22 => Self::EnumMember,
            23 => Self::Struct,
            24 => Self::Event,
            25 => Self::Operator,
            26 => Self::TypeParameter,
            _ => return Err(AnalyzerError::UnknownSymbolKind),
        })
    }
}

/// One entry of a hierarchical `textDocument/documentSymbol` result, already
/// flattened depth-first by the codec layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DocumentSymbolWire")]
pub struct DocumentSymbol {
    name: NonEmptyText,
    kind: SymbolKind,
    detail: Option<String>,
    deprecated: bool,
    range: TextRange,
    selection_range: TextRange,
    depth: u8,
}

impl DocumentSymbol {
    pub fn new(
        name: NonEmptyText,
        kind: SymbolKind,
        detail: Option<String>,
        deprecated: bool,
        range: TextRange,
        selection_range: TextRange,
        depth: u8,
    ) -> Result<Self, AnalyzerError> {
        if depth > MAX_SYMBOL_DEPTH {
            return Err(AnalyzerError::LimitExceeded);
        }
        if selection_range.start() < range.start() || selection_range.end() > range.end() {
            return Err(AnalyzerError::InvalidRange);
        }
        Ok(Self {
            name,
            kind,
            detail,
            deprecated,
            range,
            selection_range,
            depth,
        })
    }

    pub fn name(&self) -> &NonEmptyText {
        &self.name
    }

    pub fn kind(&self) -> SymbolKind {
        self.kind
    }

    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    pub fn deprecated(&self) -> bool {
        self.deprecated
    }

    pub fn range(&self) -> TextRange {
        self.range
    }

    pub fn selection_range(&self) -> TextRange {
        self.selection_range
    }

    pub fn depth(&self) -> u8 {
        self.depth
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentSymbolWire {
    name: NonEmptyText,
    kind: SymbolKind,
    detail: Option<String>,
    deprecated: bool,
    range: TextRange,
    selection_range: TextRange,
    depth: u8,
}

impl TryFrom<DocumentSymbolWire> for DocumentSymbol {
    type Error = AnalyzerError;

    fn try_from(value: DocumentSymbolWire) -> Result<Self, Self::Error> {
        Self::new(
            value.name,
            value.kind,
            value.detail,
            value.deprecated,
            value.range,
            value.selection_range,
            value.depth,
        )
    }
}

/// A `workspace/symbol` result entry; always under `/source`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSymbol {
    pub name: NonEmptyText,
    pub kind: SymbolKind,
    pub container: Option<String>,
    pub file: AnalyzerFile,
    pub range: TextRange,
}

/// A `textDocument/references` result entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub file: AnalyzerFile,
    pub range: TextRange,
    pub is_declaration: bool,
}

/// The closed LSP `DiagnosticSeverity` set (values `1..=4`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Information,
    Hint,
}

impl TryFrom<u32> for DiagnosticSeverity {
    type Error = AnalyzerError;

    fn try_from(value: u32) -> Result<Self, AnalyzerError> {
        Ok(match value {
            1 => Self::Error,
            2 => Self::Warning,
            3 => Self::Information,
            4 => Self::Hint,
            _ => return Err(AnalyzerError::UnknownSeverity),
        })
    }
}

/// A related span attached to a diagnostic (e.g. "previous definition here").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelatedInformation {
    pub file: AnalyzerFile,
    pub range: TextRange,
    pub message: NonEmptyText,
}

const MAX_DIAGNOSTIC_CODE_CHARS: usize = 128;
const MAX_DIAGNOSTIC_MESSAGE_CHARS: usize = 4096;
const MAX_RELATED_INFORMATION: usize = 32;

/// A native rust-analyzer diagnostic; `cargo check` diagnostics use
/// [`crate::Diagnostic`] instead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "AnalyzerDiagnosticWire")]
pub struct AnalyzerDiagnostic {
    file: AnalyzerFile,
    range: TextRange,
    severity: DiagnosticSeverity,
    code: Option<String>,
    message: NonEmptyText,
    related: Vec<RelatedInformation>,
}

impl AnalyzerDiagnostic {
    pub fn new(
        file: AnalyzerFile,
        range: TextRange,
        severity: DiagnosticSeverity,
        code: Option<String>,
        message: NonEmptyText,
        related: Vec<RelatedInformation>,
    ) -> Result<Self, AnalyzerError> {
        if code
            .as_ref()
            .is_some_and(|value| value.chars().count() > MAX_DIAGNOSTIC_CODE_CHARS)
        {
            return Err(AnalyzerError::LimitExceeded);
        }
        if message.as_str().chars().count() > MAX_DIAGNOSTIC_MESSAGE_CHARS {
            return Err(AnalyzerError::LimitExceeded);
        }
        if related.len() > MAX_RELATED_INFORMATION {
            return Err(AnalyzerError::LimitExceeded);
        }
        Ok(Self {
            file,
            range,
            severity,
            code,
            message,
            related,
        })
    }

    pub fn file(&self) -> &AnalyzerFile {
        &self.file
    }

    pub fn range(&self) -> TextRange {
        self.range
    }

    pub fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    pub fn code(&self) -> Option<&str> {
        self.code.as_deref()
    }

    pub fn message(&self) -> &NonEmptyText {
        &self.message
    }

    pub fn related(&self) -> &[RelatedInformation] {
        &self.related
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AnalyzerDiagnosticWire {
    file: AnalyzerFile,
    range: TextRange,
    severity: DiagnosticSeverity,
    code: Option<String>,
    message: NonEmptyText,
    related: Vec<RelatedInformation>,
}

impl TryFrom<AnalyzerDiagnosticWire> for AnalyzerDiagnostic {
    type Error = AnalyzerError;

    fn try_from(value: AnalyzerDiagnosticWire) -> Result<Self, Self::Error> {
        Self::new(
            value.file,
            value.range,
            value.severity,
            value.code,
            value.message,
            value.related,
        )
    }
}

/// The closed code action kinds M6 resolves. Wire spelling matches the MCP
/// `only` filter vocabulary, not the dotted LSP strings ([`Self::from_lsp`]
/// and [`Self::to_lsp`] bridge to those).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodeActionKind {
    #[serde(rename = "quickfix")]
    QuickFix,
    #[serde(rename = "refactor")]
    Refactor,
    #[serde(rename = "refactor_extract")]
    RefactorExtract,
    #[serde(rename = "refactor_inline")]
    RefactorInline,
    #[serde(rename = "refactor_rewrite")]
    RefactorRewrite,
    #[serde(rename = "source")]
    Source,
    #[serde(rename = "source_organize_imports")]
    SourceOrganizeImports,
}

impl CodeActionKind {
    pub fn from_lsp(value: &str) -> Option<Self> {
        Some(match value {
            "quickfix" => Self::QuickFix,
            "refactor" => Self::Refactor,
            "refactor.extract" => Self::RefactorExtract,
            "refactor.inline" => Self::RefactorInline,
            "refactor.rewrite" => Self::RefactorRewrite,
            "source" => Self::Source,
            "source.organizeImports" => Self::SourceOrganizeImports,
            _ => return None,
        })
    }

    pub fn to_lsp(self) -> &'static str {
        match self {
            Self::QuickFix => "quickfix",
            Self::Refactor => "refactor",
            Self::RefactorExtract => "refactor.extract",
            Self::RefactorInline => "refactor.inline",
            Self::RefactorRewrite => "refactor.rewrite",
            Self::Source => "source",
            Self::SourceOrganizeImports => "source.organizeImports",
        }
    }
}

/// Every reason M6 rejects a `CodeAction` instead of resolving it.
///
/// Contract: each variant means exactly what it says and nothing else — a
/// caller may show the reason to a user, so no variant is a catch-all. In
/// particular [`Self::NotUtf8`] means some file's captured bytes genuinely
/// failed UTF-8 decoding, while [`Self::FileNotInSnapshot`] means the edit
/// targets an in-scope `/source` path the caller never supplied bytes for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionRejection {
    Command,
    Snippet,
    ResourceOperation,
    ExternalUri,
    VersionMismatch,
    OverlappingRanges,
    EditLimit,
    BytesLimit,
    NotUtf8,
    /// The edit targets a valid `/source/**.rs` path that is absent from the
    /// snapshot the resolution ran against, so no line index exists to
    /// translate its positions. Never a statement about the file's encoding.
    FileNotInSnapshot,
    UnresolvedEdit,
}

/// One replacement of `range` with `new_text` inside `file`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextEdit {
    pub file: AnalyzerFile,
    pub range: TextRange,
    pub new_text: String,
}

/// Applies non-overlapping edits to `before`, returning the resulting bytes.
///
/// Preconditions: `before` is the exact byte sequence the edits' ranges were
/// computed against; every range resolves inside it. `edits` may arrive in
/// any order — the result depends only on the ranges, never on input order.
///
/// Postconditions: edits are applied left to right after sorting on
/// `(start, end)`. Touching edits (one's end equals the next's start) are
/// accepted; any two edits sharing a start are rejected with
/// [`AnalyzerError::OverlappingRanges`], including two zero-width insertions
/// at the same position, whose relative order would otherwise be decided by
/// the submission order rather than by the ranges themselves.
pub fn apply_edits(before: &[u8], edits: &[(TextRange, &str)]) -> Result<Vec<u8>, AnalyzerError> {
    let index = LineIndex::new(before)?;
    let mut spans: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|(range, text)| {
            let start = index.byte_offset_from_position(range.start())?;
            let end = index.byte_offset_from_position(range.end())?;
            Ok((start, end, *text))
        })
        .collect::<Result<_, AnalyzerError>>()?;
    spans.sort_by_key(|(start, end, _)| (*start, *end));
    for pair in spans.windows(2) {
        if pair[0].0 == pair[1].0 || pair[0].1 > pair[1].0 {
            return Err(AnalyzerError::OverlappingRanges);
        }
    }
    let source = index.text.as_bytes();
    let mut result = Vec::with_capacity(source.len());
    let mut cursor = 0usize;
    for (start, end, new_text) in spans {
        result.extend_from_slice(&source[cursor..start]);
        result.extend_from_slice(new_text.as_bytes());
        cursor = end;
    }
    result.extend_from_slice(&source[cursor..]);
    Ok(result)
}

/// Why a result set omits an entry rather than reporting it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OmissionKind {
    ExternalUri,
    SysrootLocation,
    DependencyLocation,
    LimitVisible,
    NotUtf8File,
    UnresolvablePosition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Omission {
    pub kind: OmissionKind,
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessState {
    Complete,
    Incomplete,
}

/// Why a result is `incomplete`. Deliberately closed and small: only the
/// causes the M6-01 codec/domain layer itself can observe. Later packages
/// (application/tool layer) extend the wider envelope `reason` enum, which is
/// not this type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncompleteReason {
    AnalyzerNotReady,
    LimitVisible,
    SysrootWarning,
    Timeout,
}

/// Whether a result is exhaustive, and if not, why and what was left out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "CompletenessWire")]
pub struct Completeness {
    state: CompletenessState,
    omissions: Vec<Omission>,
    reasons: Vec<IncompleteReason>,
}

impl Completeness {
    pub fn complete() -> Self {
        Self {
            state: CompletenessState::Complete,
            omissions: Vec::new(),
            reasons: Vec::new(),
        }
    }

    pub fn incomplete(reasons: Vec<IncompleteReason>) -> Self {
        Self {
            state: CompletenessState::Incomplete,
            omissions: Vec::new(),
            reasons,
        }
    }

    /// Attaches omissions, enforcing that a `Complete` result never carries a
    /// `LimitVisible` omission (a visible-result cap always means
    /// `Incomplete`).
    pub fn with_omissions(mut self, omissions: Vec<Omission>) -> Result<Self, AnalyzerError> {
        if self.state == CompletenessState::Complete
            && omissions
                .iter()
                .any(|o| o.kind == OmissionKind::LimitVisible)
        {
            return Err(AnalyzerError::Invalid);
        }
        self.omissions = omissions;
        Ok(self)
    }

    pub fn state(&self) -> CompletenessState {
        self.state
    }

    pub fn omissions(&self) -> &[Omission] {
        &self.omissions
    }

    pub fn reasons(&self) -> &[IncompleteReason] {
        &self.reasons
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompletenessWire {
    state: CompletenessState,
    #[serde(default)]
    omissions: Vec<Omission>,
    #[serde(default)]
    reasons: Vec<IncompleteReason>,
}

impl TryFrom<CompletenessWire> for Completeness {
    type Error = AnalyzerError;

    fn try_from(value: CompletenessWire) -> Result<Self, Self::Error> {
        if value.state == CompletenessState::Complete
            && value
                .omissions
                .iter()
                .any(|o| o.kind == OmissionKind::LimitVisible)
        {
            return Err(AnalyzerError::Invalid);
        }
        Ok(Self {
            state: value.state,
            omissions: value.omissions,
            reasons: value.reasons,
        })
    }
}

/// Which LSP position encoding the live server negotiated.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PositionEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-16")]
    Utf16,
}

/// The exact runtime a result was produced against, published on every M6
/// tool response so a caller can detect a rollback (D26 §2.6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerIdentity {
    pub version: NonEmptyText,
    pub binary_sha256: SourceFingerprint,
    pub image_id: NonEmptyText,
    pub config_digest: SourceFingerprint,
    pub position_encoding: PositionEncoding,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContractError;

    fn pos(line: u32, column: u32) -> Result<Position, ContractError> {
        Position::new(line, column)
    }

    fn text(value: &str) -> Result<NonEmptyText, ContractError> {
        NonEmptyText::try_from(value.to_owned())
    }

    // ---- AnalyzerFile ----

    #[test]
    fn analyzer_file_requires_relative_rs_path() {
        assert!(AnalyzerFile::new("src/lib.rs".into()).is_ok());
        for bad in ["src/lib.txt", "/abs.rs", "../lib.rs", "", "a//b.rs"] {
            assert_eq!(AnalyzerFile::new(bad.into()), Err(AnalyzerError::Invalid));
        }
    }

    // ---- TextRange ----

    #[test]
    fn text_range_rejects_inverted_bounds() -> Result<(), Box<dyn std::error::Error>> {
        assert!(TextRange::new(pos(1, 1)?, pos(1, 5)?).is_ok());
        assert_eq!(
            TextRange::new(pos(2, 1)?, pos(1, 1)?),
            Err(AnalyzerError::InvalidRange)
        );
        Ok(())
    }

    // ---- LineIndex: byte offset <-> Position ----

    #[test]
    fn ascii_offsets_round_trip() -> Result<(), Box<dyn std::error::Error>> {
        let index = LineIndex::new(b"ab\ncd")?;
        assert_eq!(index.position_from_byte_offset(0)?, pos(1, 1)?);
        assert_eq!(index.position_from_byte_offset(2)?, pos(1, 3)?);
        assert_eq!(index.position_from_byte_offset(3)?, pos(2, 1)?);
        assert_eq!(index.position_from_byte_offset(5)?, pos(2, 3)?);
        assert_eq!(index.byte_offset_from_position(pos(2, 1)?)?, 3);
        assert_eq!(index.byte_offset_from_position(pos(1, 3)?)?, 2);
        Ok(())
    }

    #[test]
    fn offset_past_end_is_out_of_range() -> Result<(), Box<dyn std::error::Error>> {
        let index = LineIndex::new(b"ab")?;
        assert_eq!(
            index.position_from_byte_offset(3),
            Err(AnalyzerError::OutOfRange)
        );
        assert_eq!(
            index.position_from_byte_offset(2),
            Ok(pos(1, 3)?),
            "offset == len is a valid end position"
        );
        Ok(())
    }

    #[test]
    fn offset_inside_bmp_code_point_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        // "é" (U+00E9) encodes as two UTF-8 bytes.
        let index = LineIndex::new("é".as_bytes())?;
        assert_eq!(index.position_from_byte_offset(0), Ok(pos(1, 1)?));
        assert_eq!(
            index.position_from_byte_offset(1),
            Err(AnalyzerError::OffsetInsideCodePoint)
        );
        assert_eq!(index.position_from_byte_offset(2), Ok(pos(1, 2)?));
        Ok(())
    }

    #[test]
    fn astral_and_emoji_are_one_scalar_each() -> Result<(), Box<dyn std::error::Error>> {
        // U+1D11E MUSICAL SYMBOL G CLEF, then an emoji: both astral, 4 UTF-8 bytes each.
        let text = "𝄞😀x";
        let index = LineIndex::new(text.as_bytes())?;
        assert_eq!(index.position_from_byte_offset(0)?, pos(1, 1)?);
        assert_eq!(index.position_from_byte_offset(4)?, pos(1, 2)?);
        assert_eq!(index.position_from_byte_offset(8)?, pos(1, 3)?);
        for inside in [1, 2, 3, 5, 6, 7] {
            assert_eq!(
                index.position_from_byte_offset(inside),
                Err(AnalyzerError::OffsetInsideCodePoint),
                "offset {inside}"
            );
        }
        assert_eq!(index.byte_offset_from_position(pos(1, 2)?)?, 4);
        assert_eq!(index.byte_offset_from_position(pos(1, 4)?)?, 9);
        Ok(())
    }

    #[test]
    fn combining_sequences_count_each_scalar() -> Result<(), Box<dyn std::error::Error>> {
        // "e" + COMBINING ACUTE ACCENT (U+0301): two scalars, one grapheme.
        let text = "e\u{0301}x";
        let index = LineIndex::new(text.as_bytes())?;
        assert_eq!(index.position_from_byte_offset(0)?, pos(1, 1)?);
        assert_eq!(index.position_from_byte_offset(1)?, pos(1, 2)?);
        assert_eq!(index.position_from_byte_offset(3)?, pos(1, 3)?);
        Ok(())
    }

    #[test]
    fn crlf_keeps_cr_inside_the_line() -> Result<(), Box<dyn std::error::Error>> {
        let index = LineIndex::new(b"ab\r\ncd")?;
        // The '\r' at offset 2 is the last scalar of line 1 (column 3); '\n'
        // at offset 3 starts line 2.
        assert_eq!(index.position_from_byte_offset(2)?, pos(1, 3)?);
        assert_eq!(index.position_from_byte_offset(4)?, pos(2, 1)?);
        assert_eq!(index.byte_offset_from_position(pos(1, 3)?)?, 2);
        Ok(())
    }

    #[test]
    fn bom_at_start_is_an_ordinary_scalar() -> Result<(), Box<dyn std::error::Error>> {
        let text = "\u{FEFF}ab";
        let index = LineIndex::new(text.as_bytes())?;
        assert_eq!(index.position_from_byte_offset(0)?, pos(1, 1)?);
        assert_eq!(index.position_from_byte_offset(3)?, pos(1, 2)?);
        Ok(())
    }

    #[test]
    fn empty_file_has_one_empty_line() -> Result<(), Box<dyn std::error::Error>> {
        let index = LineIndex::new(b"")?;
        assert_eq!(index.position_from_byte_offset(0)?, pos(1, 1)?);
        assert_eq!(index.byte_offset_from_position(pos(1, 1)?)?, 0);
        Ok(())
    }

    #[test]
    fn trailing_newline_adds_an_empty_final_line() -> Result<(), Box<dyn std::error::Error>> {
        let with_newline = LineIndex::new(b"a\n")?;
        assert_eq!(with_newline.position_from_byte_offset(2)?, pos(2, 1)?);
        let without_newline = LineIndex::new(b"a")?;
        assert_eq!(without_newline.position_from_byte_offset(1)?, pos(1, 2)?);
        Ok(())
    }

    // ---- LineIndex: UTF-16 and UTF-8 LSP encodings ----

    #[test]
    fn utf16_position_rejects_mid_surrogate_pair() -> Result<(), Box<dyn std::error::Error>> {
        // Astral scalars are surrogate pairs (2 UTF-16 units) in this encoding.
        let index = LineIndex::new("𝄞x".as_bytes())?;
        assert_eq!(index.position_from_utf16(0, 0)?, pos(1, 1)?);
        assert_eq!(
            index.position_from_utf16(0, 1),
            Err(AnalyzerError::OffsetInsideCodePoint)
        );
        assert_eq!(index.position_from_utf16(0, 2)?, pos(1, 2)?);
        Ok(())
    }

    #[test]
    fn utf16_position_out_of_range_past_line_length() -> Result<(), Box<dyn std::error::Error>> {
        let index = LineIndex::new(b"ab")?;
        assert_eq!(
            index.position_from_utf16(0, 5),
            Err(AnalyzerError::OutOfRange)
        );
        assert_eq!(index.position_from_utf16(0, 2)?, pos(1, 3)?);
        Ok(())
    }

    #[test]
    fn utf8_position_is_a_byte_offset_within_the_line_and_round_trips()
    -> Result<(), Box<dyn std::error::Error>> {
        let index = LineIndex::new("é\nx".as_bytes())?;
        assert_eq!(index.position_from_utf8(0, 0)?, pos(1, 1)?);
        assert_eq!(index.position_from_utf8(0, 2)?, pos(1, 2)?);
        assert_eq!(
            index.position_from_utf8(0, 1),
            Err(AnalyzerError::OffsetInsideCodePoint)
        );
        assert_eq!(index.utf8_from_position(pos(1, 2)?)?, (0, 2));
        assert_eq!(index.utf8_from_position(pos(2, 1)?)?, (1, 0));
        Ok(())
    }

    // ---- SymbolKind / DiagnosticSeverity ----

    #[test]
    fn symbol_kind_closed_range_is_1_to_26() {
        assert_eq!(SymbolKind::try_from(1), Ok(SymbolKind::File));
        assert_eq!(SymbolKind::try_from(26), Ok(SymbolKind::TypeParameter));
        assert_eq!(
            SymbolKind::try_from(0),
            Err(AnalyzerError::UnknownSymbolKind)
        );
        assert_eq!(
            SymbolKind::try_from(27),
            Err(AnalyzerError::UnknownSymbolKind)
        );
    }

    #[test]
    fn diagnostic_severity_closed_range_is_1_to_4() {
        assert_eq!(
            DiagnosticSeverity::try_from(1),
            Ok(DiagnosticSeverity::Error)
        );
        assert_eq!(
            DiagnosticSeverity::try_from(4),
            Ok(DiagnosticSeverity::Hint)
        );
        assert_eq!(
            DiagnosticSeverity::try_from(0),
            Err(AnalyzerError::UnknownSeverity)
        );
        assert_eq!(
            DiagnosticSeverity::try_from(5),
            Err(AnalyzerError::UnknownSeverity)
        );
    }

    // ---- DocumentSymbol ----

    #[test]
    fn document_symbol_requires_selection_inside_range_and_bounded_depth()
    -> Result<(), Box<dyn std::error::Error>> {
        let range = TextRange::new(pos(1, 1)?, pos(1, 10)?)?;
        let inside = TextRange::new(pos(1, 2)?, pos(1, 5)?)?;
        let outside = TextRange::new(pos(1, 2)?, pos(1, 20)?)?;
        let name = || text("f");
        assert!(
            DocumentSymbol::new(name()?, SymbolKind::Function, None, false, range, inside, 0)
                .is_ok()
        );
        assert_eq!(
            DocumentSymbol::new(
                name()?,
                SymbolKind::Function,
                None,
                false,
                range,
                outside,
                0
            ),
            Err(AnalyzerError::InvalidRange)
        );
        assert_eq!(
            DocumentSymbol::new(
                name()?,
                SymbolKind::Function,
                None,
                false,
                range,
                inside,
                33
            ),
            Err(AnalyzerError::LimitExceeded)
        );
        assert!(
            DocumentSymbol::new(
                name()?,
                SymbolKind::Function,
                None,
                false,
                range,
                inside,
                32
            )
            .is_ok()
        );
        Ok(())
    }

    // ---- AnalyzerDiagnostic ----

    #[test]
    fn analyzer_diagnostic_enforces_code_message_and_related_bounds()
    -> Result<(), Box<dyn std::error::Error>> {
        let range = TextRange::new(pos(1, 1)?, pos(1, 2)?)?;
        let file = AnalyzerFile::new("a.rs".into())?;
        let ok = AnalyzerDiagnostic::new(
            file.clone(),
            range,
            DiagnosticSeverity::Error,
            Some("E0308".into()),
            text("mismatched types")?,
            Vec::new(),
        );
        assert!(ok.is_ok());
        let long_code = AnalyzerDiagnostic::new(
            file.clone(),
            range,
            DiagnosticSeverity::Error,
            Some("x".repeat(129)),
            text("m")?,
            Vec::new(),
        );
        assert_eq!(long_code, Err(AnalyzerError::LimitExceeded));
        let long_message = AnalyzerDiagnostic::new(
            file.clone(),
            range,
            DiagnosticSeverity::Error,
            None,
            text(&"m".repeat(4097))?,
            Vec::new(),
        );
        assert_eq!(long_message, Err(AnalyzerError::LimitExceeded));
        let related = RelatedInformation {
            file: file.clone(),
            range,
            message: text("prior")?,
        };
        let too_many_related = AnalyzerDiagnostic::new(
            file,
            range,
            DiagnosticSeverity::Error,
            None,
            text("m")?,
            std::iter::repeat_n(related, 33).collect(),
        );
        assert_eq!(too_many_related, Err(AnalyzerError::LimitExceeded));
        Ok(())
    }

    // ---- CodeActionKind ----

    #[test]
    fn code_action_kind_lsp_strings_round_trip() {
        let cases = [
            ("quickfix", CodeActionKind::QuickFix),
            ("refactor", CodeActionKind::Refactor),
            ("refactor.extract", CodeActionKind::RefactorExtract),
            ("refactor.inline", CodeActionKind::RefactorInline),
            ("refactor.rewrite", CodeActionKind::RefactorRewrite),
            ("source", CodeActionKind::Source),
            (
                "source.organizeImports",
                CodeActionKind::SourceOrganizeImports,
            ),
        ];
        for (lsp, kind) in cases {
            assert_eq!(CodeActionKind::from_lsp(lsp), Some(kind));
            assert_eq!(kind.to_lsp(), lsp);
        }
        assert_eq!(CodeActionKind::from_lsp("bogus"), None);
    }

    #[test]
    fn code_action_kind_wire_spelling_matches_mcp_vocabulary()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(
            serde_json::to_string(&CodeActionKind::RefactorExtract)?,
            "\"refactor_extract\""
        );
        assert_eq!(
            serde_json::to_string(&CodeActionKind::QuickFix)?,
            "\"quickfix\""
        );
        Ok(())
    }

    // ---- apply_edits ----

    fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Result<TextRange, Box<dyn std::error::Error>> {
        Ok(TextRange::new(pos(sl, sc)?, pos(el, ec)?)?)
    }

    #[test]
    fn apply_edits_handles_several_edits_and_end_insertion()
    -> Result<(), Box<dyn std::error::Error>> {
        let before = b"abcdef";
        let edits = [
            (range(1, 1, 1, 2)?, "X"), // replace "a" -> "X"
            (range(1, 4, 1, 5)?, ""),  // delete "d"
            (range(1, 7, 1, 7)?, "!"), // insert at end
        ];
        let after = apply_edits(before, &edits)?;
        assert_eq!(after, b"Xbcef!");
        Ok(())
    }

    #[test]
    fn apply_edits_allows_touching_but_rejects_overlap() -> Result<(), Box<dyn std::error::Error>> {
        let before = b"abcdef";
        let touching = [(range(1, 1, 1, 3)?, "X"), (range(1, 3, 1, 5)?, "Y")];
        assert_eq!(apply_edits(before, &touching)?, b"XYef");
        let overlapping = [(range(1, 1, 1, 4)?, "X"), (range(1, 3, 1, 5)?, "Y")];
        assert_eq!(
            apply_edits(before, &overlapping),
            Err(AnalyzerError::OverlappingRanges)
        );
        Ok(())
    }

    #[test]
    fn apply_edits_result_does_not_depend_on_input_order() -> Result<(), Box<dyn std::error::Error>>
    {
        let before = b"abcdef";
        let sorted = [
            (range(1, 1, 1, 2)?, "X"),
            (range(1, 4, 1, 5)?, ""),
            (range(1, 7, 1, 7)?, "!"),
        ];
        let shuffled = [
            (range(1, 7, 1, 7)?, "!"),
            (range(1, 1, 1, 2)?, "X"),
            (range(1, 4, 1, 5)?, ""),
        ];
        assert_eq!(
            apply_edits(before, &shuffled)?,
            apply_edits(before, &sorted)?
        );
        assert_eq!(apply_edits(before, &shuffled)?, b"Xbcef!");
        Ok(())
    }

    #[test]
    fn apply_edits_rejects_edits_sharing_a_start() -> Result<(), Box<dyn std::error::Error>> {
        let before = b"abcdef";
        // A zero-width insertion coincident with the start of a replacement:
        // accepting it would make the output depend on submission order.
        let insertion_then_replacement = [(range(1, 2, 1, 2)?, "X"), (range(1, 2, 1, 4)?, "Y")];
        assert_eq!(
            apply_edits(before, &insertion_then_replacement),
            Err(AnalyzerError::OverlappingRanges)
        );
        let reversed = [(range(1, 2, 1, 4)?, "Y"), (range(1, 2, 1, 2)?, "X")];
        assert_eq!(
            apply_edits(before, &reversed),
            Err(AnalyzerError::OverlappingRanges),
            "the rejection must not depend on input order"
        );
        // Two zero-width insertions at the same position are equally ambiguous.
        let two_insertions = [(range(1, 3, 1, 3)?, "X"), (range(1, 3, 1, 3)?, "Y")];
        assert_eq!(
            apply_edits(before, &two_insertions),
            Err(AnalyzerError::OverlappingRanges)
        );
        Ok(())
    }

    #[test]
    fn apply_edits_rejects_non_utf8_source() {
        let before = [0xff, 0xfe];
        assert_eq!(apply_edits(&before, &[]), Err(AnalyzerError::NotUtf8));
    }

    // ---- Completeness ----

    #[test]
    fn completeness_rejects_limit_visible_omission_while_complete() {
        assert!(
            Completeness::complete()
                .with_omissions(vec![Omission {
                    kind: OmissionKind::ExternalUri,
                    count: 1,
                }])
                .is_ok()
        );
        assert_eq!(
            Completeness::complete().with_omissions(vec![Omission {
                kind: OmissionKind::LimitVisible,
                count: 1,
            }]),
            Err(AnalyzerError::Invalid)
        );
        assert!(
            Completeness::incomplete(vec![IncompleteReason::LimitVisible])
                .with_omissions(vec![Omission {
                    kind: OmissionKind::LimitVisible,
                    count: 1,
                }])
                .is_ok()
        );
    }

    #[test]
    fn completeness_wire_enforces_the_same_invariant() {
        let invalid = serde_json::json!({
            "state": "complete",
            "omissions": [{"kind": "limit_visible", "count": 1}],
            "reasons": [],
        });
        assert!(serde_json::from_value::<Completeness>(invalid).is_err());
        let valid = serde_json::json!({
            "state": "incomplete",
            "omissions": [{"kind": "limit_visible", "count": 1}],
            "reasons": ["limit_visible"],
        });
        assert!(serde_json::from_value::<Completeness>(valid).is_ok());
    }

    // ---- AnalyzerIdentity ----

    #[test]
    fn analyzer_identity_round_trips_through_json() -> Result<(), Box<dyn std::error::Error>> {
        let identity = AnalyzerIdentity {
            version: text("rust-analyzer 1.98.1")?,
            binary_sha256: format!("sha256:{}", "a".repeat(64)).parse()?,
            image_id: text("sha256:m6")?,
            config_digest: format!("sha256:{}", "b".repeat(64)).parse()?,
            position_encoding: PositionEncoding::Utf8,
        };
        let json = serde_json::to_string(&identity)?;
        assert!(json.contains("\"utf-8\""));
        let round_tripped: AnalyzerIdentity = serde_json::from_str(&json)?;
        assert_eq!(round_tripped, identity);
        Ok(())
    }
}
