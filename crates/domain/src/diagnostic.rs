//! Normalized diagnostic evidence; paths and suggestions do not authorize I/O.

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::{ContractError, NonEmptyText};

/// One-based line and Unicode scalar column, not a byte or UTF-16 offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub line: NonZeroU32,
    pub column: NonZeroU32,
}

impl Position {
    pub fn new(line: u32, column: u32) -> Result<Self, ContractError> {
        Ok(Self {
            line: NonZeroU32::new(line).ok_or(ContractError::InvalidSpan)?,
            column: NonZeroU32::new(column).ok_or(ContractError::InvalidSpan)?,
        })
    }
}

/// Zero-based byte offsets with an exclusive end; equality denotes an insertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ByteRangeWire")]
pub struct ByteRange {
    start: u64,
    end: u64,
}

impl ByteRange {
    pub fn new(start: u64, end: u64) -> Result<Self, ContractError> {
        if start > end {
            return Err(ContractError::InvalidSpan);
        }
        Ok(Self { start, end })
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn end(&self) -> u64 {
        self.end
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ByteRangeWire {
    start: u64,
    end: u64,
}

impl TryFrom<ByteRangeWire> for ByteRange {
    type Error = ContractError;

    fn try_from(value: ByteRangeWire) -> Result<Self, Self::Error> {
        Self::new(value.start, value.end)
    }
}

/// Textual source evidence with an exclusive end. No source content is read to
/// cross-check byte offsets against Unicode scalar positions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SourceSpanWire")]
pub struct SourceSpan {
    file: NonEmptyText,
    start: Position,
    end: Position,
    bytes: Option<ByteRange>,
    is_primary: bool,
    label: Option<NonEmptyText>,
}

impl SourceSpan {
    pub fn new(
        file: NonEmptyText,
        start: Position,
        end: Position,
        bytes: Option<ByteRange>,
        is_primary: bool,
        label: Option<NonEmptyText>,
    ) -> Result<Self, ContractError> {
        if start > end {
            return Err(ContractError::InvalidSpan);
        }
        Ok(Self {
            file,
            start,
            end,
            bytes,
            is_primary,
            label,
        })
    }

    pub fn file(&self) -> &NonEmptyText {
        &self.file
    }

    pub fn start(&self) -> Position {
        self.start
    }

    pub fn end(&self) -> Position {
        self.end
    }

    pub fn bytes(&self) -> Option<ByteRange> {
        self.bytes
    }

    pub fn is_primary(&self) -> bool {
        self.is_primary
    }

    pub fn label(&self) -> Option<&NonEmptyText> {
        self.label.as_ref()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceSpanWire {
    file: NonEmptyText,
    start: Position,
    end: Position,
    bytes: Option<ByteRange>,
    is_primary: bool,
    label: Option<NonEmptyText>,
}

impl TryFrom<SourceSpanWire> for SourceSpan {
    type Error = ContractError;

    fn try_from(value: SourceSpanWire) -> Result<Self, Self::Error> {
        Self::new(
            value.file,
            value.start,
            value.end,
            value.bytes,
            value.is_primary,
            value.label,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSource {
    Rustc,
    Cargo,
    Clippy,
    Rustfmt,
    Rustsec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Note,
    Help,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    pub source: DiagnosticSource,
    pub severity: Severity,
    pub code: Option<NonEmptyText>,
    pub message: NonEmptyText,
    pub spans: Vec<SourceSpan>,
    pub rendered: Option<String>,
    pub suggestions: Vec<Suggestion>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applicability {
    MachineApplicable,
    MaybeIncorrect,
    HasPlaceholders,
    Unspecified,
}

/// An indivisible suggestion, which may require multiple edits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SuggestionWire")]
pub struct Suggestion {
    message: NonEmptyText,
    applicability: Applicability,
    edits: Vec<Replacement>,
}

impl Suggestion {
    pub fn new(
        message: NonEmptyText,
        applicability: Applicability,
        edits: Vec<Replacement>,
    ) -> Result<Self, ContractError> {
        if edits.is_empty() {
            return Err(ContractError::EmptySuggestion);
        }
        Ok(Self {
            message,
            applicability,
            edits,
        })
    }

    pub fn message(&self) -> &NonEmptyText {
        &self.message
    }

    pub fn applicability(&self) -> Applicability {
        self.applicability
    }

    pub fn edits(&self) -> &[Replacement] {
        &self.edits
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestionWire {
    message: NonEmptyText,
    applicability: Applicability,
    edits: Vec<Replacement>,
}

impl TryFrom<SuggestionWire> for Suggestion {
    type Error = ContractError;

    fn try_from(value: SuggestionWire) -> Result<Self, Self::Error> {
        Self::new(value.message, value.applicability, value.edits)
    }
}

/// An empty replacement is a deletion, not an absent edit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replacement {
    pub span: SourceSpan,
    pub replacement: String,
}
