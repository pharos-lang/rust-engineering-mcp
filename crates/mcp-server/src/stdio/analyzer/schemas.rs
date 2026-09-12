//! Closed wire mirrors for `rust.analyzer.symbols`. Domain does not derive
//! `JsonSchema` (architecture boundary), so every value here is rebuilt from
//! `rust_engineering_domain::analyzer` values rather than reusing them.
use schemars::JsonSchema;
use serde::Serialize;
use std::num::NonZeroU32;

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub line: NonZeroU32,
    pub column: NonZeroU32,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

/// The closed LSP 3.17 `SymbolKind` set, mirroring
/// `rust_engineering_domain::SymbolKind` 1:1.
#[derive(Serialize, JsonSchema)]
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

impl From<rust_engineering_domain::SymbolKind> for SymbolKind {
    fn from(value: rust_engineering_domain::SymbolKind) -> Self {
        use rust_engineering_domain::SymbolKind as Domain;
        match value {
            Domain::File => Self::File,
            Domain::Module => Self::Module,
            Domain::Namespace => Self::Namespace,
            Domain::Package => Self::Package,
            Domain::Class => Self::Class,
            Domain::Method => Self::Method,
            Domain::Property => Self::Property,
            Domain::Field => Self::Field,
            Domain::Constructor => Self::Constructor,
            Domain::Enum => Self::Enum,
            Domain::Interface => Self::Interface,
            Domain::Function => Self::Function,
            Domain::Variable => Self::Variable,
            Domain::Constant => Self::Constant,
            Domain::String => Self::String,
            Domain::Number => Self::Number,
            Domain::Boolean => Self::Boolean,
            Domain::Array => Self::Array,
            Domain::Object => Self::Object,
            Domain::Key => Self::Key,
            Domain::Null => Self::Null,
            Domain::EnumMember => Self::EnumMember,
            Domain::Struct => Self::Struct,
            Domain::Event => Self::Event,
            Domain::Operator => Self::Operator,
            Domain::TypeParameter => Self::TypeParameter,
        }
    }
}

/// A flattened `textDocument/documentSymbol` entry (ADR-083 §4).
///
/// `name`'s bound (256 Unicode scalars, no control character) mirrors
/// `lsp_codec::MAX_PEER_NAME_CHARS` (V05 P2), a different crate this schema
/// module may not depend on; the pattern below excludes only the C0 control
/// range and DEL (JSON Schema `pattern` need not support the Unicode
/// property classes `char::is_control` covers), so it is a best-effort mirror
/// — the actual bound is enforced in Rust by `lsp_codec`, not by this schema.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DocumentSymbol {
    pub depth: u8,
    #[schemars(length(min = 1, max = 256), regex(pattern = "^[^\\x00-\\x1f\\x7f]*$"))]
    pub name: String,
    pub kind: SymbolKind,
    #[schemars(length(min = 1, max = 1024))]
    pub detail: Option<String>,
    /// `true` when `detail` is a prefix of what the peer actually sent (V05
    /// P2): always `false` when `detail` is absent.
    pub detail_truncated: bool,
    pub deprecated: bool,
    pub range: Range,
    pub selection_range: Range,
}

/// A `workspace/symbol` entry, always under `/source` (ADR-083 §4). `name`
/// and `container` share [`DocumentSymbol::name`]'s peer-text bound.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSymbol {
    #[schemars(length(min = 1, max = 256), regex(pattern = "^[^\\x00-\\x1f\\x7f]*$"))]
    pub name: String,
    pub kind: SymbolKind,
    #[schemars(length(min = 1, max = 256), regex(pattern = "^[^\\x00-\\x1f\\x7f]*$"))]
    pub container: Option<String>,
    #[schemars(length(min = 1, max = 100))]
    pub file: String,
    pub range: Range,
}

/// The shape of `data.symbols` depends on the requested scope: a document
/// scope answers with a flattened symbol tree, a workspace scope with a flat
/// search result. Never both, matching the single query the session ran.
#[derive(Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Symbols {
    Document(Vec<DocumentSymbol>),
    Workspace(Vec<WorkspaceSymbol>),
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Semantics {
    LatestKnown,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub source_fingerprint: String,
    pub files: u32,
    pub semantics: Semantics,
    pub atomic: bool,
}

#[derive(Serialize, JsonSchema)]
pub enum PositionEncoding {
    #[serde(rename = "utf-8")]
    Utf8,
    #[serde(rename = "utf-16")]
    Utf16,
}

impl From<rust_engineering_domain::PositionEncoding> for PositionEncoding {
    fn from(value: rust_engineering_domain::PositionEncoding) -> Self {
        match value {
            rust_engineering_domain::PositionEncoding::Utf8 => Self::Utf8,
            rust_engineering_domain::PositionEncoding::Utf16 => Self::Utf16,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Analyzer {
    #[schemars(length(min = 1))]
    pub version: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub binary_sha256: String,
    #[schemars(length(min = 1))]
    pub image_id: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub config_digest: String,
    /// `None` exactly when `initialize` never answered: no negotiation
    /// happened, so no encoding is published.
    pub position_encoding: Option<PositionEncoding>,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sysroot {
    Present,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Toolchain {
    pub rust_version: &'static str,
    pub sysroot: Sysroot,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    Quiescent,
    NotReady,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Ok,
    Warning,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Readiness {
    pub state: ReadinessState,
    /// `Some` only for `state: quiescent`: a never-ready session observed no
    /// server health at all, and inventing one would be a claim the adapter
    /// cannot support.
    pub health: Option<Health>,
    pub elapsed_ms: u64,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessState {
    Complete,
    Incomplete,
}

/// Mirrors `rust_engineering_domain::OmissionKind` 1:1; only `limit_visible`
/// and `not_utf8_file` are reachable from this tool's converter today, but the
/// full closed set is modelled so a future gateway change needs no schema
/// edit here. `oversized_entry` (V05 P2) is enforced by `lsp_codec` at the
/// peer boundary, but the M6-01 gateway still folds that count into
/// `limit_visible` pending a change to `analyzer_gateway.rs::record_omissions`
/// that reports it under this variant instead.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OmissionKind {
    ExternalUri,
    SysrootLocation,
    DependencyLocation,
    LimitVisible,
    NotUtf8File,
    UnresolvablePosition,
    OversizedEntry,
}

impl From<rust_engineering_domain::OmissionKind> for OmissionKind {
    fn from(value: rust_engineering_domain::OmissionKind) -> Self {
        use rust_engineering_domain::OmissionKind as Domain;
        match value {
            Domain::ExternalUri => Self::ExternalUri,
            Domain::SysrootLocation => Self::SysrootLocation,
            Domain::DependencyLocation => Self::DependencyLocation,
            Domain::LimitVisible => Self::LimitVisible,
            Domain::NotUtf8File => Self::NotUtf8File,
            Domain::UnresolvablePosition => Self::UnresolvablePosition,
            Domain::OversizedEntry => Self::OversizedEntry,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Omission {
    pub kind: OmissionKind,
    pub count: u32,
}

/// Mirrors `rust_engineering_domain::IncompleteReason`, plus this tool's own
/// `result_limit`: the 512 KiB output trim (D2) has no domain reason of its
/// own because it never reaches the analyzer session at all.
#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    AnalyzerNotReady,
    LimitVisible,
    AnalyzerWarning,
    NotUtf8File,
    Timeout,
    ResultLimit,
}

impl From<rust_engineering_domain::IncompleteReason> for Reason {
    fn from(value: rust_engineering_domain::IncompleteReason) -> Self {
        use rust_engineering_domain::IncompleteReason as Domain;
        match value {
            Domain::AnalyzerNotReady => Self::AnalyzerNotReady,
            Domain::LimitVisible => Self::LimitVisible,
            Domain::AnalyzerWarning => Self::AnalyzerWarning,
            Domain::NotUtf8File => Self::NotUtf8File,
            Domain::Timeout => Self::Timeout,
        }
    }
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Completeness {
    pub state: CompletenessState,
    #[schemars(length(max = 8))]
    pub omissions: Vec<Omission>,
    #[schemars(length(max = 8))]
    pub reasons: Vec<Reason>,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_visible: u32,
    pub initialize_timeout_seconds: u32,
    pub query_timeout_seconds: u32,
    pub total_timeout_seconds: u32,
    pub frame_bytes: u32,
    pub messages: u32,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Session {
    pub messages_in: u32,
    pub messages_out: u32,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub duration_ms: u64,
    pub stderr_bytes: u64,
    pub server_requests: u32,
}

#[derive(Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Termination {
    Exited,
    TimedOut,
    Cancelled,
    OutputLimit,
}

impl From<rust_engineering_domain::ExecutionTermination> for Termination {
    fn from(value: rust_engineering_domain::ExecutionTermination) -> Self {
        use rust_engineering_domain::ExecutionTermination as Domain;
        match value {
            Domain::Exited => Self::Exited,
            Domain::TimedOut => Self::TimedOut,
            Domain::Cancelled => Self::Cancelled,
            Domain::OutputLimit => Self::OutputLimit,
        }
    }
}
