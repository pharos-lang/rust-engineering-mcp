//! Bounded, incremental LSP base-protocol codec and the typed DTO subset M6
//! uses. Pure functions over bytes: no process, no I/O, no async. The
//! session lifecycle (spawning `rust-analyzer`, deadlines, kill/cleanup)
//! belongs to a later package; this module only turns bytes into typed
//! messages and typed messages into bytes, and converts the wire DTOs into
//! [`rust_engineering_domain::analyzer`] values.

use std::collections::BTreeMap;

use rust_engineering_domain as domain;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------

/// Every way the codec or correlator rejects a hostile or malformed peer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodecError {
    MalformedHeader,
    /// The `Content-Length` was well formed and above the frame bound. Distinct
    /// from [`Self::MalformedHeader`] because the bound is a decision of
    /// ADR-084 §8 and a caller that asserts it must be able to observe it.
    FrameLimit,
    MalformedMessage,
    BatchRejected,
    MessageLimit,
    ByteLimit,
    UnknownResponseId,
    DuplicateResponse,
    LateResponse,
}

impl CodecError {
    /// `false` only for `DuplicateResponse` and `LateResponse`: D25 §1.6
    /// treats late/duplicate responses as signals to discard and count, not
    /// evidence the peer's framing or JSON-RPC contract broke. Every other
    /// kind means the session must be killed.
    pub fn is_fatal(self) -> bool {
        !matches!(self, Self::DuplicateResponse | Self::LateResponse)
    }
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MalformedHeader => "malformed LSP base-protocol header",
            Self::FrameLimit => "declared frame length above the frame bound",
            Self::MalformedMessage => "malformed or non-conforming JSON-RPC message",
            Self::BatchRejected => "batched (array) JSON-RPC payload rejected",
            Self::MessageLimit => "message-per-job limit exceeded",
            Self::ByteLimit => "total body byte limit exceeded",
            Self::UnknownResponseId => "response to an id that was never sent",
            Self::DuplicateResponse => "response to an id that already responded",
            Self::LateResponse => "response to an id that already timed out",
        })
    }
}

impl std::error::Error for CodecError {}

// ---------------------------------------------------------------------
// Core wire types
// ---------------------------------------------------------------------

/// A JSON-RPC id: a string or an integer, never a float (rejected as
/// `CodecError::MalformedMessage`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(untagged)]
pub enum RequestId {
    Number(i64),
    Text(String),
}

/// A JSON-RPC error object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ResponseError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ResponseError {
    pub const REQUEST_CANCELLED: i64 = -32800;
    pub const CONTENT_MODIFIED: i64 = -32801;
    pub const METHOD_NOT_FOUND: i64 = -32601;

    /// The only response this adapter ever sends to a server-initiated
    /// request: every server→client request is refused, never granted.
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: Self::METHOD_NOT_FOUND,
            message: format!("method not found: {method}"),
            data: None,
        }
    }
}

/// One fully framed and JSON-RPC-validated inbound message.
#[derive(Clone, Debug, PartialEq)]
pub enum RawMessage {
    Request {
        id: RequestId,
        method: String,
        params: Option<serde_json::Value>,
    },
    Response {
        id: RequestId,
        outcome: Result<serde_json::Value, ResponseError>,
    },
    Notification {
        method: String,
        params: Option<serde_json::Value>,
    },
}

// ---------------------------------------------------------------------
// Decoder: base-protocol framing
// ---------------------------------------------------------------------

const MAX_HEADER_LINES: usize = 8;
const MAX_HEADER_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug)]
pub struct DecoderLimits {
    pub max_frame_bytes: usize,
    pub max_messages: usize,
    pub max_total_bytes: usize,
}

impl Default for DecoderLimits {
    fn default() -> Self {
        Self {
            max_frame_bytes: domain::MAX_FRAME_BYTES,
            max_messages: domain::MAX_MESSAGES_PER_JOB,
            max_total_bytes: domain::MAX_STDOUT_BYTES,
        }
    }
}

/// The longest needle the header scan looks for (`\r\n\r\n`) minus one: the
/// number of trailing bytes an incremental scan must re-examine so a
/// terminator straddling two `feed` calls is still found.
const HEADER_NEEDLE_OVERLAP: usize = 3;

/// A header that has already been parsed while its body is still arriving.
#[derive(Clone, Copy, Debug)]
struct PendingBody {
    body_start: usize,
    content_length: usize,
}

/// Accumulates bytes across calls and yields complete, validated frames.
///
/// Contract: a `Decoder` belongs to exactly one peer session. It is
/// incremental in both senses — bytes may be split arbitrarily across
/// [`Self::feed`] calls, and the work done per call is proportional to the
/// bytes of that call, never to the bytes already buffered. Once a fatal
/// error is returned the decoder is poisoned and every later call returns the
/// same error (see [`Self::feed`]).
pub struct Decoder {
    limits: DecoderLimits,
    buffer: Vec<u8>,
    total_messages: usize,
    total_bytes: usize,
    /// Bytes of `buffer` already searched for a header terminator, minus the
    /// [`HEADER_NEEDLE_OVERLAP`] tail. Reset to `0` whenever a frame is taken.
    scan_from: usize,
    /// `Some` once the current frame's header is parsed: the body is being
    /// awaited and the header must never be scanned or parsed again.
    pending: Option<PendingBody>,
    /// The `Content-Length` of the frame that broke the bound, kept so the
    /// caller can publish the number the peer declared rather than only the
    /// fact that something was refused.
    declared_frame_bytes: Option<u64>,
    poisoned: Option<CodecError>,
    #[cfg(test)]
    header_scans: usize,
}

impl Decoder {
    pub fn new(limits: DecoderLimits) -> Self {
        Self {
            limits,
            buffer: Vec::new(),
            total_messages: 0,
            total_bytes: 0,
            scan_from: 0,
            pending: None,
            declared_frame_bytes: None,
            poisoned: None,
            #[cfg(test)]
            header_scans: 0,
        }
    }

    /// Feeds newly received bytes and returns every complete frame they
    /// finished. Bytes that do not yet complete a frame are buffered for the
    /// next call; a violated limit or a malformed frame aborts immediately
    /// with the messages already produced by this call discarded (the
    /// session is being killed either way, per [`CodecError::is_fatal`]).
    ///
    /// Postcondition after a fatal error: the decoder is poisoned. Every
    /// subsequent call returns that same error without touching the buffer or
    /// the counters, so a caller that fails to kill the session cannot
    /// double-count messages or re-parse the offending frame.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<RawMessage>, CodecError> {
        if let Some(error) = self.poisoned {
            return Err(error);
        }
        match self.feed_inner(bytes) {
            Ok(messages) => Ok(messages),
            Err(error) => {
                if error.is_fatal() {
                    self.poisoned = Some(error);
                }
                Err(error)
            }
        }
    }

    fn feed_inner(&mut self, bytes: &[u8]) -> Result<Vec<RawMessage>, CodecError> {
        self.buffer.extend_from_slice(bytes);
        let mut messages = Vec::new();
        while let Some(message) = self.try_take_frame()? {
            messages.push(message);
        }
        Ok(messages)
    }

    /// How many times the header terminator was searched for. Test-only
    /// evidence that a trickled body never re-scans the buffered prefix.
    #[cfg(test)]
    fn header_scans(&self) -> usize {
        self.header_scans
    }

    /// The `Content-Length` that exceeded the frame bound, if one did. Only
    /// ever set together with [`CodecError::FrameLimit`].
    pub fn declared_frame_bytes(&self) -> Option<u64> {
        self.declared_frame_bytes
    }

    fn try_take_frame(&mut self) -> Result<Option<RawMessage>, CodecError> {
        let pending = match self.pending {
            Some(pending) => pending,
            None => match self.parse_header()? {
                Some(pending) => {
                    self.pending = Some(pending);
                    pending
                }
                None => return Ok(None),
            },
        };
        let needed = pending.body_start + pending.content_length;
        if self.buffer.len() < needed {
            return Ok(None);
        }
        self.total_messages += 1;
        if self.total_messages > self.limits.max_messages {
            return Err(CodecError::MessageLimit);
        }
        self.total_bytes += pending.content_length;
        if self.total_bytes > self.limits.max_total_bytes {
            return Err(CodecError::ByteLimit);
        }
        let body = self.buffer[pending.body_start..needed].to_vec();
        let message = parse_body(&body)?;
        self.buffer.drain(..needed);
        self.pending = None;
        self.scan_from = 0;
        Ok(Some(message))
    }

    /// Searches only the bytes not yet searched (plus the straddle overlap)
    /// for the header terminator and, on success, parses the header exactly
    /// once. Returns `None` while the terminator has not arrived.
    fn parse_header(&mut self) -> Result<Option<PendingBody>, CodecError> {
        #[cfg(test)]
        {
            self.header_scans += 1;
        }
        let from = self.scan_from;
        let Some(header_end) = find(&self.buffer[from..], b"\r\n\r\n").map(|offset| offset + from)
        else {
            if self.buffer.len() > MAX_HEADER_BYTES {
                return Err(CodecError::MalformedHeader);
            }
            // No terminator yet, so everything buffered is still header: a
            // bare `\n\n` in it already proves the peer is not using `\r\n`.
            if find(&self.buffer[from..], b"\n\n").is_some() {
                return Err(CodecError::MalformedHeader);
            }
            self.scan_from = self.buffer.len().saturating_sub(HEADER_NEEDLE_OVERLAP);
            return Ok(None);
        };
        if header_end + 4 > MAX_HEADER_BYTES {
            return Err(CodecError::MalformedHeader);
        }
        // Only a `\n\n` *before* the terminator is a framing violation; past
        // it the bytes are body, where `\n\n` is ordinary content.
        if find(&self.buffer[from..header_end], b"\n\n").is_some() {
            return Err(CodecError::MalformedHeader);
        }
        let lines = split_header_lines(&self.buffer[..header_end])?;
        if lines.is_empty() || lines.len() > MAX_HEADER_LINES {
            return Err(CodecError::MalformedHeader);
        }
        let declared = parse_content_length(&lines)?;
        // The bound is checked here, where the number can be kept: a peer that
        // announces a frame this side will not read is refused before a byte of
        // that body is buffered, and the announcement itself is the evidence.
        let Ok(content_length) = usize::try_from(declared).map_err(|_| ()) else {
            self.declared_frame_bytes = Some(declared);
            return Err(CodecError::FrameLimit);
        };
        if content_length > self.limits.max_frame_bytes {
            self.declared_frame_bytes = Some(declared);
            return Err(CodecError::FrameLimit);
        }
        Ok(Some(PendingBody {
            body_start: header_end + 4,
            content_length,
        }))
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Splits a header block (already stripped of the final blank-line
/// terminator) on exact `\r\n` pairs. Any stray `\r` or `\n` left inside a
/// resulting line means the peer used something other than `\r\n`
/// separators throughout, which is malformed.
fn split_header_lines(block: &[u8]) -> Result<Vec<&[u8]>, CodecError> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i + 1 < block.len() {
        if block[i] == b'\r' && block[i + 1] == b'\n' {
            lines.push(&block[start..i]);
            i += 2;
            start = i;
        } else {
            i += 1;
        }
    }
    if start < block.len() {
        lines.push(&block[start..]);
    }
    for line in &lines {
        if line.is_empty() || line.contains(&b'\r') || line.contains(&b'\n') {
            return Err(CodecError::MalformedHeader);
        }
    }
    Ok(lines)
}

/// Parses a `Content-Length` value in its one canonical spelling.
///
/// Preconditions: `value` is the header field value with at most the single
/// conventional space after the colon already removed.
///
/// Postconditions: `Some(n)` only for a run of ASCII digits with no sign, no
/// embedded or surrounding whitespace, no radix prefix and no redundant
/// leading zero (`0` alone is the sole value starting with `0`), and only
/// when the run fits a `u64`. Every other spelling is `None`, so two peers
/// can never disagree on how many body bytes a frame declares.
fn parse_canonical_length(value: &str) -> Option<u64> {
    let digits = value.as_bytes();
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if digits.len() > 1 && digits[0] == b'0' {
        return None;
    }
    value.parse().ok()
}

/// The declared `Content-Length`, unbounded: the frame bound belongs to
/// [`Decoder::parse_header`], which keeps the offending number as evidence.
fn parse_content_length(lines: &[&[u8]]) -> Result<u64, CodecError> {
    let mut content_length = None;
    for line in lines {
        let text = std::str::from_utf8(line).map_err(|_| CodecError::MalformedHeader)?;
        let (name, value) = text.split_once(':').ok_or(CodecError::MalformedHeader)?;
        if name.is_empty() || !name.is_ascii() {
            return Err(CodecError::MalformedHeader);
        }
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                if content_length.is_some() {
                    return Err(CodecError::MalformedHeader);
                }
                // Exactly one optional separating space, then digits only.
                let digits = value.strip_prefix(' ').unwrap_or(value);
                content_length =
                    Some(parse_canonical_length(digits).ok_or(CodecError::MalformedHeader)?);
            }
            "content-type" => {}
            _ => return Err(CodecError::MalformedHeader),
        }
    }
    content_length.ok_or(CodecError::MalformedHeader)
}

/// The complete top-level key vocabulary of a JSON-RPC 2.0 message, minus
/// `jsonrpc` which is removed before the check. Anything else is malformed:
/// silently dropping an unrecognized key would let a peer smuggle meaning
/// past this adapter's closed contract.
const ALLOWED_MESSAGE_KEYS: [&str; 5] = ["id", "method", "params", "result", "error"];

fn parse_body(body: &[u8]) -> Result<RawMessage, CodecError> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| CodecError::MalformedMessage)?;
    let mut object = match value {
        serde_json::Value::Object(object) => object,
        serde_json::Value::Array(_) => return Err(CodecError::BatchRejected),
        _ => return Err(CodecError::MalformedMessage),
    };
    match object.remove("jsonrpc") {
        Some(serde_json::Value::String(version)) if version == "2.0" => {}
        _ => return Err(CodecError::MalformedMessage),
    }
    if object
        .keys()
        .any(|key| !ALLOWED_MESSAGE_KEYS.contains(&key.as_str()))
    {
        return Err(CodecError::MalformedMessage);
    }
    let method = object.remove("method");
    let has_id = object.contains_key("id");
    let result = object.remove("result");
    let error = object.remove("error");
    if let Some(method_value) = method {
        // A request or notification carrying a response payload is neither:
        // it is a peer contradicting itself, not something to interpret.
        if result.is_some() || error.is_some() {
            return Err(CodecError::MalformedMessage);
        }
        let serde_json::Value::String(method) = method_value else {
            return Err(CodecError::MalformedMessage);
        };
        let params = object.remove("params");
        if has_id {
            let id = parse_id(object.remove("id"))?;
            Ok(RawMessage::Request { id, method, params })
        } else {
            Ok(RawMessage::Notification { method, params })
        }
    } else {
        if result.is_some() && error.is_some() {
            return Err(CodecError::MalformedMessage);
        }
        let id = parse_id(object.remove("id"))?;
        let outcome = match (result, error) {
            (Some(value), None) => Ok(value),
            (None, Some(error_value)) => {
                let error: ResponseError = serde_json::from_value(error_value)
                    .map_err(|_| CodecError::MalformedMessage)?;
                Err(error)
            }
            _ => return Err(CodecError::MalformedMessage),
        };
        Ok(RawMessage::Response { id, outcome })
    }
}

fn parse_id(value: Option<serde_json::Value>) -> Result<RequestId, CodecError> {
    match value {
        Some(serde_json::Value::String(text)) => Ok(RequestId::Text(text)),
        Some(serde_json::Value::Number(number)) => number
            .as_i64()
            .map(RequestId::Number)
            .ok_or(CodecError::MalformedMessage),
        _ => Err(CodecError::MalformedMessage),
    }
}

// ---------------------------------------------------------------------
// Encoder
// ---------------------------------------------------------------------

/// An outgoing message. `Response` only ever carries an error: this
/// adapter never answers a server→client request with a result, so it can
/// never grant a capability (D25 §1.6).
#[derive(Clone, Debug)]
pub enum OutgoingMessage {
    Request {
        id: RequestId,
        method: String,
        params: Option<serde_json::Value>,
    },
    Notification {
        method: String,
        params: Option<serde_json::Value>,
    },
    Response {
        id: RequestId,
        error: ResponseError,
    },
}

/// Frames one outgoing message.
///
/// Postconditions: the returned bytes are a complete LSP frame whose
/// `Content-Length` equals the body length. Serialization is infallible for
/// every value this enum can hold, but a failure is still reported as
/// [`domain::AnalyzerError::Invalid`] rather than silently framed as an empty
/// or truncated body, which the peer would read as a malformed message with
/// no local trace of why.
pub fn encode(message: &OutgoingMessage) -> Result<Vec<u8>, domain::AnalyzerError> {
    let unrepresentable = |_| domain::AnalyzerError::Invalid;
    let mut object = serde_json::Map::new();
    object.insert("jsonrpc".into(), serde_json::Value::String("2.0".into()));
    match message {
        OutgoingMessage::Request { id, method, params } => {
            object.insert(
                "id".into(),
                serde_json::to_value(id).map_err(unrepresentable)?,
            );
            object.insert("method".into(), serde_json::Value::String(method.clone()));
            if let Some(params) = params {
                object.insert("params".into(), params.clone());
            }
        }
        OutgoingMessage::Notification { method, params } => {
            object.insert("method".into(), serde_json::Value::String(method.clone()));
            if let Some(params) = params {
                object.insert("params".into(), params.clone());
            }
        }
        OutgoingMessage::Response { id, error } => {
            object.insert(
                "id".into(),
                serde_json::to_value(id).map_err(unrepresentable)?,
            );
            object.insert(
                "error".into(),
                serde_json::to_value(error).map_err(unrepresentable)?,
            );
        }
    }
    let body = serde_json::to_vec(&serde_json::Value::Object(object)).map_err(unrepresentable)?;
    let mut framed = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    framed.extend_from_slice(&body);
    Ok(framed)
}

// ---------------------------------------------------------------------
// Correlator
// ---------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub struct Matched {
    pub id: RequestId,
    pub result: Result<serde_json::Value, ResponseError>,
}

/// Tracks our own outstanding requests, rejecting responses to ids we never
/// sent, sent twice, or that already timed out.
#[derive(Debug, Default)]
pub struct Correlator {
    next_id: i64,
    outstanding: std::collections::BTreeSet<RequestId>,
    responded: std::collections::BTreeSet<RequestId>,
    timed_out: std::collections::BTreeSet<RequestId>,
}

impl Correlator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates the next monotonically increasing integer id. Does not by
    /// itself register the id as outstanding; call [`Self::expect`] once the
    /// request has actually been sent.
    pub fn allocate(&mut self) -> RequestId {
        self.next_id += 1;
        RequestId::Number(self.next_id)
    }

    pub fn expect(&mut self, id: RequestId) {
        self.outstanding.insert(id);
    }

    pub fn timeout(&mut self, id: &RequestId) {
        if self.outstanding.remove(id) {
            self.timed_out.insert(id.clone());
        }
    }

    pub fn accept(&mut self, response: RawMessage) -> Result<Matched, CodecError> {
        let RawMessage::Response { id, outcome } = response else {
            return Err(CodecError::MalformedMessage);
        };
        if self.responded.contains(&id) {
            return Err(CodecError::DuplicateResponse);
        }
        if self.timed_out.contains(&id) {
            return Err(CodecError::LateResponse);
        }
        if !self.outstanding.remove(&id) {
            return Err(CodecError::UnknownResponseId);
        }
        self.responded.insert(id.clone());
        Ok(Matched {
            id,
            result: outcome,
        })
    }
}

// ---------------------------------------------------------------------
// Typed DTOs: positions and locations
// ---------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LspRange {
    pub start: LspPosition,
    pub end: LspPosition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub uri: String,
    pub range: LspRange,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

// ---------------------------------------------------------------------
// Typed DTOs: initialize
// ---------------------------------------------------------------------

/// Builds exactly the D25/D26 §4.3 client capabilities and §4.5
/// `initializationOptions`; nothing is negotiable at call sites.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub process_id: Option<u32>,
    pub root_uri: String,
    /// The one folder this session ever has. Added by the session package:
    /// `rootUri` alone is deprecated in LSP 3.17 and a conforming server may
    /// read only `workspaceFolders`, so naming the same single folder in both
    /// places leaves nothing to a server's choice of which field to honour. It
    /// widens nothing: `linkedProjects` already pins the one manifest
    /// rust-analyzer may load (ADR-084 §3).
    pub workspace_folders: Vec<WorkspaceFolder>,
    pub capabilities: serde_json::Value,
    pub initialization_options: serde_json::Value,
}

/// One LSP workspace folder.
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceFolder {
    pub uri: String,
    pub name: String,
}

impl InitializeParams {
    pub fn new() -> Self {
        Self {
            process_id: None,
            root_uri: "file:///source".to_owned(),
            workspace_folders: vec![WorkspaceFolder {
                uri: "file:///source".to_owned(),
                name: "source".to_owned(),
            }],
            capabilities: client_capabilities(),
            initialization_options: initialization_options(),
        }
    }
}

impl Default for InitializeParams {
    fn default() -> Self {
        Self::new()
    }
}

/// The fixed client capabilities (ADR-084 §4, D25/D26 §4.3).
///
/// Contract: nothing here is negotiable at a call site and nothing here
/// grants a server→client effect. `textDocument.codeAction` advertises
/// literal support because without it a spec-compliant server MAY answer
/// `textDocument/codeAction` with bare `Command` objects only, which
/// [`resolve_action`] rejects unconditionally — the feature would then be
/// silently empty rather than explicitly unsupported. `resolveSupport` stays
/// absent on purpose: edits must travel inline, since this lifecycle has no
/// `codeAction/resolve` round trip.
fn client_capabilities() -> serde_json::Value {
    serde_json::json!({
        "general": { "positionEncodings": ["utf-8"] },
        "window": { "workDoneProgress": false },
        "workspace": {
            "configuration": false,
            "didChangeWatchedFiles": { "dynamicRegistration": false },
            "workspaceEdit": { "documentChanges": true },
        },
        "textDocument": {
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
            "diagnostic": {},
            "codeAction": {
                "codeActionLiteralSupport": {
                    "codeActionKind": {
                        "valueSet": [
                            "quickfix",
                            "refactor",
                            "refactor.extract",
                            "refactor.inline",
                            "refactor.rewrite",
                            "source",
                            "source.organizeImports",
                        ],
                    },
                },
                "isPreferredSupport": true,
                "dataSupport": false,
                "disabledSupport": false,
            },
        },
        "experimental": { "serverStatusNotification": true },
    })
}

/// The fixed D25/D26 §4.5 `initializationOptions`, nested exactly per key:
/// seventeen keys after the 2026-09-12 amendment to ADR-084 §3.
///
/// Contract: every key and value below is binding and unconditional. Because
/// rust-analyzer ignores unknown configuration keys in silence, correctness of
/// this map is not established here — the native calibration dumps
/// `--print-config-schema` from the real binary of the M6 image and fails if
/// any key below is absent from it. That dump, archived with its hash in the
/// receipt, is the oracle; this comment is not.
///
/// Two keys the brief listed are deliberately absent, both on the evidence of
/// that calibration (W04, [F1/F2](../../../docs/validation/M6/01.md)):
/// `cargo.sysrootQueryMetadata` does not exist in the real binary's schema, so
/// setting it configured nothing while still entering the `config_digest`; and
/// `cargo.autoreload=false` made the server publish `health: warning` for the
/// whole session ("auto-reloading is disabled and the workspace has changed"),
/// which degraded every M6 answer to `incomplete`. The default `autoreload=true`
/// is now in force: this lifecycle sends no `didChange`, mounts `/source`
/// read-only and lives for one query, so there is no reload for it to avoid.
pub fn initialization_options() -> serde_json::Value {
    serde_json::json!({
        "cargo": {
            "buildScripts": { "enable": false },
            "noDeps": true,
            "sysroot": "discover",
            "targetDir": null,
        },
        "procMacro": { "enable": false },
        "checkOnSave": false,
        "files": { "watcher": "client" },
        "cachePriming": { "enable": false },
        "numThreads": 1,
        "lru": { "capacity": 64 },
        "linkedProjects": ["/source/Cargo.toml"],
        "diagnostics": { "experimental": { "enable": false } },
        "workspace": {
            "symbol": {
                "search": { "scope": "workspace", "kind": "all_symbols", "limit": 512 },
            },
        },
        "references": { "excludeImports": false, "excludeTests": false },
    })
}

fn sha256_fingerprint(bytes: &[u8]) -> Result<domain::SourceFingerprint, domain::AnalyzerError> {
    let digest = Sha256::digest(bytes);
    let mut text = String::from("sha256:");
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text.parse().map_err(|_| domain::AnalyzerError::Invalid)
}

/// The sha256 of the canonical (`serde_json`'s default `Map` is a
/// `BTreeMap`, so keys are already sorted; `to_vec` is already compact) JSON
/// of [`initialization_options`].
///
/// Postcondition: the digest covers the exact bytes that would be sent. A
/// serialization failure is [`domain::AnalyzerError::Invalid`], never an
/// empty input — digesting nothing would produce a stable, plausible-looking
/// fingerprint of a configuration that was never sent, defeating the rollback
/// detection the digest exists for (D26 §2.6).
pub fn config_digest() -> Result<domain::SourceFingerprint, domain::AnalyzerError> {
    let bytes = serde_json::to_vec(&initialization_options())
        .map_err(|_| domain::AnalyzerError::Invalid)?;
    sha256_fingerprint(&bytes)
}

#[derive(Clone, Debug, Deserialize)]
pub struct InitializeResult {
    pub capabilities: InitializeCapabilities,
}

#[derive(Clone, Debug, Deserialize)]
pub struct InitializeCapabilities {
    #[serde(default, rename = "positionEncoding")]
    pub position_encoding: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ServerStatusParams {
    pub health: String,
    pub quiescent: bool,
    #[serde(default)]
    pub message: Option<String>,
}

// ---------------------------------------------------------------------
// Typed DTOs: documents, symbols, references, diagnostics
// ---------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentItem {
    pub uri: String,
    pub language_id: String,
    pub version: i64,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DidOpenTextDocumentParams {
    pub text_document: TextDocumentItem,
}

impl DidOpenTextDocumentParams {
    /// Every M6 session opens exactly one document at `version: 1` (D26
    /// §2.2): there is no `didChange`, so a stale version is impossible.
    pub fn new(uri: String, text: String) -> Self {
        Self {
            text_document: TextDocumentItem {
                uri,
                language_id: "rust".to_owned(),
                version: 1,
                text,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentSymbolParams {
    pub text_document: TextDocumentIdentifier,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDocumentSymbol {
    pub name: String,
    #[serde(default)]
    pub detail: Option<String>,
    pub kind: u32,
    #[serde(default)]
    pub deprecated: Option<bool>,
    pub range: LspRange,
    pub selection_range: LspRange,
    #[serde(default)]
    pub children: Option<Vec<LspDocumentSymbol>>,
}

/// The flat `workspace/symbol` result entry. LSP 3.17 also defines a
/// `WorkspaceSymbol` type distinguished from `SymbolInformation` only when
/// `workspace.symbol.resolveSupport` is advertised (a partial, uri-only
/// location); this client never advertises it, so `location` is always the
/// full `{uri, range}` shape and one struct covers both wire names.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolInformation {
    pub name: String,
    pub kind: u32,
    #[serde(default)]
    pub container_name: Option<String>,
    pub location: Location,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSymbolParams {
    pub query: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceContext {
    pub include_declaration: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceParams {
    pub text_document: TextDocumentIdentifier,
    pub position: LspPosition,
    pub context: ReferenceContext,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDiagnosticParams {
    pub text_document: TextDocumentIdentifier,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRelatedInformation {
    pub location: Location,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspDiagnostic {
    pub range: LspRange,
    #[serde(default)]
    pub severity: Option<u32>,
    /// LSP allows a string or a number here; both are coerced to `String` by
    /// [`diagnostics_to_domain`].
    #[serde(default)]
    pub code: Option<serde_json::Value>,
    pub message: String,
    #[serde(default)]
    pub related_information: Option<Vec<DiagnosticRelatedInformation>>,
}

/// Only the `full` kind is meaningful here: M6 never sends a
/// `previousResultId`, so an `unchanged` report from a hostile or confused
/// peer has nothing to be unchanged relative to.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DocumentDiagnosticReport {
    Full {
        #[serde(default)]
        items: Vec<LspDiagnostic>,
    },
    Unchanged {
        #[serde(rename = "resultId")]
        result_id: String,
    },
}

impl DocumentDiagnosticReport {
    pub fn into_full(self) -> Result<Vec<LspDiagnostic>, CodecError> {
        match self {
            Self::Full { items } => Ok(items),
            Self::Unchanged { .. } => Err(CodecError::MalformedMessage),
        }
    }
}

// ---------------------------------------------------------------------
// Typed DTOs: code actions and workspace edits
// ---------------------------------------------------------------------

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionContext {
    pub diagnostics: Vec<serde_json::Value>,
    /// The closed `only` filter, in its dotted LSP spelling. Added by the
    /// session package: without it the `only` input of ADR-083 §1.2 could not
    /// be expressed at all, and filtering after the fact would spend the
    /// server's work on kinds the caller excluded. Absent — not an empty array
    /// — when the caller asked for no filter, because an empty `only` means
    /// "no kind is acceptable" to a conforming server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only: Option<Vec<String>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeActionParams {
    pub text_document: TextDocumentIdentifier,
    pub range: LspRange,
    pub context: CodeActionContext,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CommandObject {
    pub title: String,
    pub command: String,
    #[serde(default)]
    pub arguments: Option<Vec<serde_json::Value>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeAction {
    pub title: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub is_preferred: Option<bool>,
    #[serde(default)]
    pub edit: Option<WorkspaceEdit>,
    #[serde(default)]
    pub command: Option<CommandObject>,
}

/// A bare `Command` object is distinguished from a `CodeAction` by
/// `command`'s type: a string identifier on `Command`, a nested object on
/// `CodeAction`. Untagged deserialization tries `CodeAction` first and falls
/// through to `Command` exactly when that type mismatch occurs.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum CodeActionOrCommand {
    CodeAction(CodeAction),
    Command(CommandObject),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LspTextEdit {
    pub range: LspRange,
    pub new_text: String,
    #[serde(default, rename = "insertTextFormat")]
    pub insert_text_format: Option<i32>,
    #[serde(default, rename = "annotationId")]
    pub annotation_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OptionalVersionedTextDocumentIdentifier {
    pub uri: String,
    #[serde(default)]
    pub version: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDocumentEdit {
    pub text_document: OptionalVersionedTextDocumentIdentifier,
    pub edits: Vec<LspTextEdit>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ResourceOperation {
    Create {
        uri: String,
    },
    Rename {
        #[serde(rename = "oldUri")]
        old_uri: String,
        #[serde(rename = "newUri")]
        new_uri: String,
    },
    Delete {
        uri: String,
    },
}

/// `TextDocumentEdit` has no `kind` field and requires `textDocument`/
/// `edits`, so it never accidentally matches a resource operation object
/// (which is missing both).
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum DocumentChangeOperation {
    Edit(TextDocumentEdit),
    ResourceOperation(ResourceOperation),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceEdit {
    #[serde(default)]
    pub changes: Option<BTreeMap<String, Vec<LspTextEdit>>>,
    #[serde(default)]
    pub document_changes: Option<Vec<DocumentChangeOperation>>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ShutdownParams;

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ExitParams;

// ---------------------------------------------------------------------
// Conversion helpers: wire -> domain
// ---------------------------------------------------------------------

/// Every LSP URI this adapter accepts as in-scope. Sysroot and dependency
/// paths (e.g. `/opt/rust/...`) are syntactically valid `file://` URIs but
/// fall outside `/source`, so the caller counts them as an omission rather
/// than treating them as an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UriRejection {
    External,
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let hex_text = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex_text, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Accepts only `file:///source/<rel>`; `<rel>` is percent-decoded and then
/// validated by [`domain::AnalyzerFile::new`], which alone already rejects
/// `..`, empty path components and non-`.rs` files.
pub fn lsp_uri_to_file(uri: &str) -> Result<domain::AnalyzerFile, UriRejection> {
    let rest = uri
        .strip_prefix("file:///source/")
        .ok_or(UriRejection::External)?;
    let decoded = percent_decode(rest).ok_or(UriRejection::External)?;
    domain::AnalyzerFile::new(decoded).map_err(|_| UriRejection::External)
}

pub fn lsp_range_to_text_range(
    range: &LspRange,
    index: &domain::LineIndex,
    encoding: domain::PositionEncoding,
) -> Result<domain::TextRange, domain::AnalyzerError> {
    let (start, end) = match encoding {
        domain::PositionEncoding::Utf8 => (
            index.position_from_utf8(range.start.line, range.start.character)?,
            index.position_from_utf8(range.end.line, range.end.character)?,
        ),
        domain::PositionEncoding::Utf16 => (
            index.position_from_utf16(range.start.line, range.start.character)?,
            index.position_from_utf16(range.end.line, range.end.character)?,
        ),
    };
    domain::TextRange::new(start, end)
}

/// The peer-text bounds of V05 P2: a `name` or `container` this long (in
/// Unicode scalars), or containing any control character, is refused rather
/// than let unbounded or control-laden peer text reach a validated domain
/// value. Checked against the raw wire string, before any domain constructor
/// sees it.
const MAX_PEER_NAME_CHARS: usize = 256;
/// `detail` above this many Unicode scalars is truncated, not refused: it is
/// descriptive text a caller reads, never an identifier matched on.
const MAX_PEER_DETAIL_CHARS: usize = 1024;

/// `true` when `value` is short enough and free of control characters to
/// cross the codec boundary as a `name` or `container` (V05 P2).
fn peer_text_in_bounds(value: &str) -> bool {
    value.chars().count() <= MAX_PEER_NAME_CHARS && !value.chars().any(char::is_control)
}

/// Flattens a hierarchical `documentSymbol` result depth-first, assigning
/// each entry its nesting `depth`.
///
/// Postconditions: at most [`domain::MAX_VISIBLE_RESULTS`] entries are
/// returned and no entry has a `depth` above [`domain::MAX_SYMBOL_DEPTH`].
/// Every entry's `detail` is at most [`MAX_PEER_DETAIL_CHARS`] Unicode
/// scalars, truncated from what the peer sent when longer
/// ([`domain::DocumentSymbol::detail_truncated`] records it). The second
/// element counts every entry the peer sent that is not in the first: those
/// past the visible cap, those below the depth cap, and those whose `name`
/// is above [`MAX_PEER_NAME_CHARS`] or contains a control character. Native
/// recursion is bounded by [`domain::MAX_SYMBOL_DEPTH`] regardless of the
/// nesting the peer sends, so an adversarial tree is a counted omission, not
/// a stack overflow.
pub fn document_symbols_to_domain(
    symbols: Vec<LspDocumentSymbol>,
    index: &domain::LineIndex,
    encoding: domain::PositionEncoding,
) -> Result<(Vec<domain::DocumentSymbol>, usize), domain::AnalyzerError> {
    let mut out = Vec::new();
    let mut truncated = 0usize;
    for symbol in symbols {
        walk_document_symbol(symbol, 0, index, encoding, &mut out, &mut truncated)?;
    }
    Ok((out, truncated))
}

/// Counts every node of `children` and its descendants without recursing.
///
/// Both the walk and the drop of the discarded subtree are iterative: derived
/// drop glue for a nested `Vec` is itself recursive, so simply dropping a
/// peer-controlled 10 000-deep tree would overflow the stack just as surely
/// as walking it.
fn count_and_discard(children: Vec<LspDocumentSymbol>) -> usize {
    let mut stack = children;
    let mut counted = 0usize;
    while let Some(mut symbol) = stack.pop() {
        counted += 1;
        if let Some(grandchildren) = symbol.children.take() {
            stack.extend(grandchildren);
        }
    }
    counted
}

fn walk_document_symbol(
    symbol: LspDocumentSymbol,
    depth: u8,
    index: &domain::LineIndex,
    encoding: domain::PositionEncoding,
    out: &mut Vec<domain::DocumentSymbol>,
    truncated: &mut usize,
) -> Result<(), domain::AnalyzerError> {
    let range = lsp_range_to_text_range(&symbol.range, index, encoding)?;
    let selection_range = lsp_range_to_text_range(&symbol.selection_range, index, encoding)?;
    let kind = domain::SymbolKind::try_from(symbol.kind)?;
    let name = peer_text_in_bounds(&symbol.name)
        .then(|| domain::NonEmptyText::try_from(symbol.name))
        .transpose()
        .map_err(|_| domain::AnalyzerError::Invalid)?;
    let children = symbol.children.unwrap_or_default();
    match name {
        Some(name) if out.len() < domain::MAX_VISIBLE_RESULTS => {
            let entry = domain::DocumentSymbol::new(
                name,
                kind,
                symbol.detail,
                symbol.deprecated.unwrap_or(false),
                range,
                selection_range,
                depth,
            )?
            .truncate_detail(MAX_PEER_DETAIL_CHARS);
            out.push(entry);
        }
        _ => *truncated += 1,
    }
    // Checked before descending and on every path, including the one past the
    // visible cap: the next level must still be a representable depth.
    let next_depth = match depth.checked_add(1) {
        Some(next) if next <= domain::MAX_SYMBOL_DEPTH => next,
        _ => {
            *truncated += count_and_discard(children);
            return Ok(());
        }
    };
    for child in children {
        walk_document_symbol(child, next_depth, index, encoding, out, truncated)?;
    }
    Ok(())
}

/// Converts one file's diagnostics. Related information pointing outside
/// `file` is omitted: M6-01 opens a single document per query (D26 §2.2), so
/// only `file`'s own [`domain::LineIndex`] is available here to translate
/// byte offsets against.
pub fn diagnostics_to_domain(
    file: &domain::AnalyzerFile,
    diagnostics: Vec<LspDiagnostic>,
    index: &domain::LineIndex,
    encoding: domain::PositionEncoding,
) -> Result<(Vec<domain::AnalyzerDiagnostic>, usize), domain::AnalyzerError> {
    let mut out = Vec::new();
    let mut truncated = 0usize;
    for diagnostic in diagnostics {
        if out.len() >= domain::MAX_VISIBLE_RESULTS {
            truncated += 1;
            continue;
        }
        let range = lsp_range_to_text_range(&diagnostic.range, index, encoding)?;
        let severity = domain::DiagnosticSeverity::try_from(diagnostic.severity.unwrap_or(1))?;
        let message = domain::NonEmptyText::try_from(diagnostic.message)
            .map_err(|_| domain::AnalyzerError::Invalid)?;
        let code = diagnostic.code.and_then(|value| match value {
            serde_json::Value::String(text) => Some(text),
            serde_json::Value::Number(number) => Some(number.to_string()),
            _ => None,
        });
        let related = diagnostic
            .related_information
            .unwrap_or_default()
            .into_iter()
            .filter_map(|info| {
                let related_file = lsp_uri_to_file(&info.location.uri).ok()?;
                if related_file != *file {
                    return None;
                }
                let related_range =
                    lsp_range_to_text_range(&info.location.range, index, encoding).ok()?;
                let related_message = domain::NonEmptyText::try_from(info.message).ok()?;
                Some(domain::RelatedInformation {
                    file: related_file,
                    range: related_range,
                    message: related_message,
                })
            })
            .take(32)
            .collect();
        out.push(domain::AnalyzerDiagnostic::new(
            file.clone(),
            range,
            severity,
            code,
            message,
            related,
        )?);
    }
    Ok((out, truncated))
}

/// Converts `textDocument/references` locations, which may span multiple
/// files, using one [`domain::LineIndex`] per file. A location outside
/// `/source` or whose file has no known index is omitted and counted.
/// `declarations` are the locations the caller already knows are the
/// definition site (the bare LSP response carries no such flag itself).
pub fn references_to_domain(
    locations: Vec<Location>,
    declarations: &[Location],
    indices: &BTreeMap<domain::AnalyzerFile, domain::LineIndex>,
    encoding: domain::PositionEncoding,
) -> Result<(Vec<domain::Reference>, usize), domain::AnalyzerError> {
    let mut out = Vec::new();
    let mut omitted = 0usize;
    for location in locations {
        if out.len() >= domain::MAX_VISIBLE_RESULTS {
            omitted += 1;
            continue;
        }
        let Ok(file) = lsp_uri_to_file(&location.uri) else {
            omitted += 1;
            continue;
        };
        let Some(index) = indices.get(&file) else {
            omitted += 1;
            continue;
        };
        let range = lsp_range_to_text_range(&location.range, index, encoding)?;
        let is_declaration = declarations.iter().any(|declaration| {
            declaration.uri == location.uri && declaration.range == location.range
        });
        out.push(domain::Reference {
            file,
            range,
            is_declaration,
        });
    }
    Ok((out, omitted))
}

/// Converts a `workspace/symbol` result.
///
/// Preconditions: `indices` holds a [`domain::LineIndex`] for every captured
/// `.rs` file the query could legitimately match; `encoding` is the encoding
/// the live server negotiated.
///
/// Postconditions: the result is sorted by `(file, range start, name)` and
/// then capped at [`domain::MAX_VISIBLE_RESULTS`], so the visible set depends
/// only on the symbols themselves and never on the order the server happened
/// to emit them. The second element counts every symbol not in the first:
/// those outside `/source` or in a file absent from `indices`, those whose
/// position does not resolve against the captured bytes, those whose `name`
/// or `container` is above [`MAX_PEER_NAME_CHARS`] or contains a control
/// character (V05 P2), and those dropped by the cap.
pub fn workspace_symbols_to_domain(
    symbols: Vec<SymbolInformation>,
    indices: &BTreeMap<domain::AnalyzerFile, domain::LineIndex>,
    encoding: domain::PositionEncoding,
) -> Result<(Vec<domain::WorkspaceSymbol>, usize), domain::AnalyzerError> {
    let mut out = Vec::new();
    let mut omitted = 0usize;
    for symbol in symbols {
        let Ok(file) = lsp_uri_to_file(&symbol.location.uri) else {
            omitted += 1;
            continue;
        };
        let Some(index) = indices.get(&file) else {
            omitted += 1;
            continue;
        };
        let Ok(range) = lsp_range_to_text_range(&symbol.location.range, index, encoding) else {
            omitted += 1;
            continue;
        };
        if !peer_text_in_bounds(&symbol.name)
            || symbol
                .container_name
                .as_deref()
                .is_some_and(|container| !peer_text_in_bounds(container))
        {
            omitted += 1;
            continue;
        }
        let name = domain::NonEmptyText::try_from(symbol.name)
            .map_err(|_| domain::AnalyzerError::Invalid)?;
        out.push(domain::WorkspaceSymbol {
            name,
            kind: domain::SymbolKind::try_from(symbol.kind)?,
            container: symbol.container_name,
            file,
            range,
        });
    }
    out.sort_by(|left, right| {
        (left.file.as_str(), left.range.start(), left.name.as_str()).cmp(&(
            right.file.as_str(),
            right.range.start(),
            right.name.as_str(),
        ))
    });
    if out.len() > domain::MAX_VISIBLE_RESULTS {
        omitted += out.len() - domain::MAX_VISIBLE_RESULTS;
        out.truncate(domain::MAX_VISIBLE_RESULTS);
    }
    Ok((out, omitted))
}

/// Builds the `file -> LineIndex` map the code-action resolution requires.
///
/// Preconditions: `files` yields every captured `.rs` file a code action may
/// legitimately touch, paired with that file's exact captured bytes — the
/// same bytes `didOpen` sent, since the server's ranges are computed against
/// them.
///
/// Postconditions: [`domain::ActionRejection::NotUtf8`] is returned if and
/// only if some file's bytes genuinely fail UTF-8 decoding. A file simply
/// missing from `files` is not an error here; it surfaces later as
/// [`domain::ActionRejection::FileNotInSnapshot`] on the action that needs it.
pub fn snapshot_indices<'a, I>(
    files: I,
) -> Result<BTreeMap<domain::AnalyzerFile, domain::LineIndex>, domain::ActionRejection>
where
    I: IntoIterator<Item = (domain::AnalyzerFile, &'a [u8])>,
{
    files
        .into_iter()
        .map(|(file, bytes)| {
            let index =
                domain::LineIndex::new(bytes).map_err(|_| domain::ActionRejection::NotUtf8)?;
            Ok((file, index))
        })
        .collect()
}

/// One resolved, applicable code action: edits already converted to domain
/// values, grouped per file with the document version they were computed
/// against (`None` when the server omitted it).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedAction {
    pub title: String,
    pub kind: Option<domain::CodeActionKind>,
    pub is_preferred: bool,
    pub edits: Vec<domain::TextEdit>,
    pub versions: BTreeMap<domain::AnalyzerFile, Option<i64>>,
}

/// Resolves every element of a `textDocument/codeAction` result array.
///
/// Preconditions: `actions` are the raw, still-untyped elements of that
/// array. They are deliberately *not* pre-deserialized into a
/// `Vec<CodeActionOrCommand>`: serde's `Vec<T>` is all-or-nothing, so one
/// element that matches neither shape would fail the whole batch and destroy
/// the per-item isolation promised below. `indices` must satisfy the contract
/// of [`snapshot_indices`].
///
/// Postconditions: the result has exactly one entry per input element, in
/// input order. An element that fails to deserialize is
/// [`domain::ActionRejection::UnresolvedEdit`] for that element alone; every
/// other element still resolves or is rejected on its own merits.
pub fn code_actions_to_candidates(
    actions: Vec<serde_json::Value>,
    indices: &BTreeMap<domain::AnalyzerFile, domain::LineIndex>,
    encoding: domain::PositionEncoding,
) -> Vec<Result<ResolvedAction, domain::ActionRejection>> {
    actions
        .into_iter()
        .map(|value| {
            let action: CodeActionOrCommand = serde_json::from_value(value)
                .map_err(|_| domain::ActionRejection::UnresolvedEdit)?;
            resolve_action(action, indices, encoding)
        })
        .collect()
}

/// Resolves one code action against the captured snapshot.
///
/// Preconditions: `indices` contains a [`domain::LineIndex`] for every
/// captured `.rs` file, not only the one document the session opened —
/// multi-file edits (a rename touching several modules, for example) are
/// legitimate results here, and a file absent from `indices` is rejected as
/// [`domain::ActionRejection::FileNotInSnapshot`] rather than resolved
/// against the wrong bytes.
///
/// Postconditions: on `Ok`, edits are grouped per file, sorted on
/// `(start, end)` and pairwise non-overlapping with no two edits in a file
/// sharing a start; on `Err`, the variant names the single reason this action
/// was refused.
fn resolve_action(
    action: CodeActionOrCommand,
    indices: &BTreeMap<domain::AnalyzerFile, domain::LineIndex>,
    encoding: domain::PositionEncoding,
) -> Result<ResolvedAction, domain::ActionRejection> {
    let action = match action {
        CodeActionOrCommand::Command(_) => return Err(domain::ActionRejection::Command),
        CodeActionOrCommand::CodeAction(action) => action,
    };
    if action.command.is_some() {
        return Err(domain::ActionRejection::Command);
    }
    let workspace_edit = action.edit.ok_or(domain::ActionRejection::UnresolvedEdit)?;

    let operations = match workspace_edit.document_changes {
        Some(operations) => operations,
        None => workspace_edit
            .changes
            .unwrap_or_default()
            .into_iter()
            .map(|(uri, edits)| {
                DocumentChangeOperation::Edit(TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                    edits,
                })
            })
            .collect(),
    };

    let mut versions = BTreeMap::new();
    let mut by_file: BTreeMap<domain::AnalyzerFile, Vec<(domain::TextRange, String)>> =
        BTreeMap::new();
    let mut total_edits = 0usize;
    let mut total_bytes = 0usize;

    for operation in operations {
        let text_document_edit = match operation {
            DocumentChangeOperation::Edit(edit) => edit,
            DocumentChangeOperation::ResourceOperation(_) => {
                return Err(domain::ActionRejection::ResourceOperation);
            }
        };
        let file = lsp_uri_to_file(&text_document_edit.text_document.uri)
            .map_err(|_| domain::ActionRejection::ExternalUri)?;
        if let Some(version) = text_document_edit.text_document.version
            && version != 1
        {
            return Err(domain::ActionRejection::VersionMismatch);
        }
        versions.insert(file.clone(), text_document_edit.text_document.version);
        let index = indices
            .get(&file)
            .ok_or(domain::ActionRejection::FileNotInSnapshot)?;
        for text_edit in text_document_edit.edits {
            if text_edit.insert_text_format == Some(2) {
                return Err(domain::ActionRejection::Snippet);
            }
            let range = lsp_range_to_text_range(&text_edit.range, index, encoding)
                .map_err(|_| domain::ActionRejection::UnresolvedEdit)?;
            total_edits += 1;
            total_bytes += text_edit.new_text.len();
            by_file
                .entry(file.clone())
                .or_default()
                .push((range, text_edit.new_text));
        }
    }

    if total_edits > domain::MAX_EDITS {
        return Err(domain::ActionRejection::EditLimit);
    }
    if total_bytes > domain::MAX_RESULT_BYTES {
        return Err(domain::ActionRejection::BytesLimit);
    }

    let mut edits = Vec::with_capacity(total_edits);
    for (file, mut file_edits) in by_file {
        file_edits.sort_by_key(|(range, _)| (range.start(), range.end()));
        for pair in file_edits.windows(2) {
            // Two edits sharing a start — including two zero-width insertions
            // at the same position — have no canonical order, so applying
            // them would make the text depend on the server's array order.
            if pair[0].0.start() == pair[1].0.start() || pair[0].0.end() > pair[1].0.start() {
                return Err(domain::ActionRejection::OverlappingRanges);
            }
        }
        for (range, new_text) in file_edits {
            edits.push(domain::TextEdit {
                file: file.clone(),
                range,
                new_text,
            });
        }
    }

    Ok(ResolvedAction {
        title: action.title,
        kind: action
            .kind
            .as_deref()
            .and_then(domain::CodeActionKind::from_lsp),
        is_preferred: action.is_preferred.unwrap_or(false),
        edits,
        versions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(content_length: usize) -> String {
        format!("Content-Length: {content_length}\r\n\r\n")
    }

    fn frame(body: &str) -> Vec<u8> {
        let mut bytes = header(body.len()).into_bytes();
        bytes.extend_from_slice(body.as_bytes());
        bytes
    }

    // ---- hostile-peer: header framing ----

    #[test]
    fn content_length_larger_than_max_frame_is_a_frame_limit_with_its_number() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        let declared = domain::MAX_FRAME_BYTES + 1;
        let bytes = header(declared);
        assert_eq!(decoder.feed(bytes.as_bytes()), Err(CodecError::FrameLimit));
        assert_eq!(
            decoder.declared_frame_bytes(),
            Some(declared as u64),
            "the refused length is evidence, not just the refusal"
        );
    }

    /// A length no `usize` can hold is still the frame bound being broken, and
    /// the declared number survives the refusal on a 32-bit host too.
    #[test]
    fn a_content_length_beyond_usize_is_a_frame_limit() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        assert_eq!(
            decoder.feed(format!("Content-Length: {}\r\n\r\n", u64::MAX).as_bytes()),
            Err(CodecError::FrameLimit)
        );
        assert_eq!(decoder.declared_frame_bytes(), Some(u64::MAX));
    }

    #[test]
    fn content_length_negative_non_numeric_and_overflow_are_malformed_headers() {
        for header_line in [
            "Content-Length: -5\r\n\r\n",
            "Content-Length: abc\r\n\r\n",
            "Content-Length: 99999999999999999999999999\r\n\r\n",
        ] {
            let mut decoder = Decoder::new(DecoderLimits::default());
            assert_eq!(
                decoder.feed(header_line.as_bytes()),
                Err(CodecError::MalformedHeader),
                "{header_line}"
            );
        }
    }

    #[test]
    fn content_length_zero_is_a_valid_frame_with_an_invalid_empty_body() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        // The header itself is well-formed (0 is a valid decimal length);
        // the frame completes immediately, but an empty body is not valid
        // JSON, so this fails at message parsing, not framing.
        assert_eq!(
            decoder.feed(b"Content-Length: 0\r\n\r\n"),
            Err(CodecError::MalformedMessage)
        );
    }

    #[test]
    fn content_length_accepts_only_the_canonical_decimal_spelling() {
        for header_line in [
            "Content-Length: +10\r\n\r\n",  // explicit sign
            "Content-Length:  10\r\n\r\n",  // whitespace inside the value
            "Content-Length: 1 0\r\n\r\n",  // whitespace between digits
            "Content-Length: 10 \r\n\r\n",  // trailing whitespace
            "Content-Length: 010\r\n\r\n",  // redundant leading zero
            "Content-Length: 0x10\r\n\r\n", // radix prefix
            "Content-Length: ١٠\r\n\r\n",   // non-ASCII digits
        ] {
            let mut decoder = Decoder::new(DecoderLimits::default());
            assert_eq!(
                decoder.feed(header_line.as_bytes()),
                Err(CodecError::MalformedHeader),
                "{header_line:?}"
            );
        }
    }

    #[test]
    fn a_trickled_frame_never_rescans_the_buffered_prefix() -> Result<(), CodecError> {
        // One maximal frame delivered one byte at a time. The header scan must
        // happen once per byte only until the terminator arrives, and never
        // again while the 1 MiB body accumulates: rescanning from byte 0 on
        // every feed would be ~10^12 byte comparisons here.
        let prefix = "{\"jsonrpc\":\"2.0\",\"method\":\"m\",\"params\":{\"padding\":\"";
        let suffix = "\"}}";
        let padding = domain::MAX_FRAME_BYTES - prefix.len() - suffix.len();
        let body = format!("{prefix}{}{suffix}", "x".repeat(padding));
        assert_eq!(body.len(), domain::MAX_FRAME_BYTES);
        let framed = frame(&body);
        let mut decoder = Decoder::new(DecoderLimits::default());
        let mut produced = 0usize;
        for byte in &framed {
            produced += decoder.feed(std::slice::from_ref(byte))?.len();
        }
        assert_eq!(produced, 1);
        assert!(
            decoder.header_scans() <= 64,
            "{} header scans for {} feed calls",
            decoder.header_scans(),
            framed.len()
        );
        Ok(())
    }

    #[test]
    fn a_fatal_error_poisons_the_decoder() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        assert_eq!(decoder.feed(&frame("[]")), Err(CodecError::BatchRejected));
        let valid = frame("{\"jsonrpc\":\"2.0\",\"method\":\"m\"}");
        for _ in 0..3 {
            assert_eq!(
                decoder.feed(&valid),
                Err(CodecError::BatchRejected),
                "a poisoned decoder must keep returning its first fatal error"
            );
        }
        assert_eq!(
            decoder.feed(&[]),
            Err(CodecError::BatchRejected),
            "even an empty feed must not clear the poison"
        );
    }

    #[test]
    fn missing_blank_line_waits_without_erroring() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        assert_eq!(
            decoder.feed(b"Content-Length: 5\r\n"),
            Ok(Vec::new()),
            "no terminator yet: wait, do not error"
        );
    }

    #[test]
    fn lf_only_separators_are_malformed() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        assert_eq!(
            decoder.feed(b"Content-Length: 5\n\n{\"a\":1}"),
            Err(CodecError::MalformedHeader)
        );
    }

    /// `n` legal `Content-Type` lines plus one `Content-Length` line: every
    /// line uses the codec's closed header vocabulary, so this isolates the
    /// "at most 8 header lines" rule from the "known header names only" one.
    fn header_block_with_lines(n: usize, body: &str) -> Vec<u8> {
        let mut bytes = String::new();
        for _ in 0..n {
            bytes.push_str("Content-Type: application/vscode-jsonrpc; charset=utf-8\r\n");
        }
        bytes.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        bytes.push_str(body);
        bytes.into_bytes()
    }

    #[test]
    fn eight_header_lines_is_allowed() -> Result<(), CodecError> {
        let mut decoder = Decoder::new(DecoderLimits::default());
        let body = "{\"jsonrpc\":\"2.0\",\"method\":\"m\"}";
        let bytes = header_block_with_lines(7, body);
        let messages = decoder.feed(&bytes)?;
        assert_eq!(messages.len(), 1);
        Ok(())
    }

    #[test]
    fn nine_header_lines_exceed_the_cap() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        let bytes = header_block_with_lines(8, "{}");
        assert_eq!(
            decoder.feed(&bytes),
            Err(CodecError::MalformedHeader),
            "9 header lines exceed the cap of 8"
        );
    }

    #[test]
    fn two_kib_header_exceeds_the_byte_cap() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        let mut bytes = format!("Content-Type: {}\r\n", "x".repeat(2000));
        bytes.push_str(&header(2));
        bytes.push_str("{}");
        assert_eq!(
            decoder.feed(bytes.as_bytes()),
            Err(CodecError::MalformedHeader)
        );
    }

    #[test]
    fn body_shorter_than_declared_then_eof_waits_then_completes() -> Result<(), CodecError> {
        let mut decoder = Decoder::new(DecoderLimits::default());
        let full = frame("{\"jsonrpc\":\"2.0\",\"method\":\"m\"}");
        let (first, rest) = full.split_at(full.len() - 5);
        assert_eq!(decoder.feed(first), Ok(Vec::new()), "short body: wait");
        let messages = decoder.feed(rest)?;
        assert_eq!(messages.len(), 1);
        Ok(())
    }

    #[test]
    fn body_with_invalid_utf8_is_malformed_message() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        let mut bytes = header(2).into_bytes();
        bytes.extend_from_slice(&[0xff, 0xfe]);
        assert_eq!(decoder.feed(&bytes), Err(CodecError::MalformedMessage));
    }

    #[test]
    fn json_array_batch_is_rejected() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        assert_eq!(decoder.feed(&frame("[]")), Err(CodecError::BatchRejected));
    }

    #[test]
    fn deeply_nested_json_is_malformed_not_a_panic() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        let depth = 200;
        let mut body = String::from("{\"jsonrpc\":\"2.0\",\"method\":\"m\",\"params\":");
        body.push_str(&"[".repeat(depth));
        body.push_str(&"]".repeat(depth));
        body.push('}');
        assert_eq!(
            decoder.feed(&frame(&body)),
            Err(CodecError::MalformedMessage)
        );
    }

    #[test]
    fn notification_flood_past_the_message_cap_is_rejected() {
        let limits = DecoderLimits {
            max_messages: 4,
            ..DecoderLimits::default()
        };
        let mut decoder = Decoder::new(limits);
        let one = frame("{\"jsonrpc\":\"2.0\",\"method\":\"m\"}");
        let mut all = Vec::new();
        for _ in 0..4 {
            all.extend_from_slice(&one);
        }
        assert_eq!(decoder.feed(&all).map(|m| m.len()), Ok(4));
        assert_eq!(decoder.feed(&one), Err(CodecError::MessageLimit));
    }

    #[test]
    fn byte_total_cap_is_enforced() {
        let limits = DecoderLimits {
            max_total_bytes: 4,
            ..DecoderLimits::default()
        };
        let mut decoder = Decoder::new(limits);
        assert_eq!(
            decoder.feed(&frame("{\"jsonrpc\":\"2.0\",\"method\":\"m\"}")),
            Err(CodecError::ByteLimit)
        );
    }

    #[test]
    fn both_result_and_error_or_missing_jsonrpc_is_malformed() {
        let mut decoder = Decoder::new(DecoderLimits::default());
        assert_eq!(
            decoder.feed(&frame(
                "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":1,\"error\":{\"code\":-1,\"message\":\"x\"}}"
            )),
            Err(CodecError::MalformedMessage)
        );
        let mut decoder = Decoder::new(DecoderLimits::default());
        assert_eq!(
            decoder.feed(&frame("{\"id\":1,\"method\":\"m\"}")),
            Err(CodecError::MalformedMessage)
        );
        let mut decoder = Decoder::new(DecoderLimits::default());
        assert_eq!(
            decoder.feed(&frame("{\"jsonrpc\":\"2.0\",\"id\":1.5,\"method\":\"m\"}")),
            Err(CodecError::MalformedMessage)
        );
    }

    #[test]
    fn unknown_top_level_keys_and_mixed_request_response_shapes_are_malformed() {
        for body in [
            // A key outside the JSON-RPC vocabulary is never silently dropped.
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"m\",\"extra\":1}",
            "{\"jsonrpc\":\"2.0\",\"method\":\"m\",\"Method\":\"m\"}",
            // A message that is both a request and a response is neither.
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"m\",\"result\":1}",
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"m\",\"error\":{\"code\":-1,\"message\":\"x\"}}",
            "{\"jsonrpc\":\"2.0\",\"method\":\"m\",\"result\":null}",
        ] {
            let mut decoder = Decoder::new(DecoderLimits::default());
            assert_eq!(
                decoder.feed(&frame(body)),
                Err(CodecError::MalformedMessage),
                "{body}"
            );
        }
    }

    // ---- hostile-peer: correlator ----

    #[test]
    fn correlator_rejects_unknown_duplicate_and_late_responses() -> Result<(), CodecError> {
        let mut correlator = Correlator::new();
        let id = correlator.allocate();
        correlator.expect(id.clone());
        let response_for = |id: RequestId| RawMessage::Response {
            id,
            outcome: Ok(serde_json::json!(null)),
        };
        assert_eq!(
            correlator.accept(response_for(RequestId::Number(9999))),
            Err(CodecError::UnknownResponseId)
        );
        let matched = correlator.accept(response_for(id.clone()))?;
        assert_eq!(matched.id, id);
        assert_eq!(
            correlator.accept(response_for(id.clone())),
            Err(CodecError::DuplicateResponse)
        );

        let timed_out_id = correlator.allocate();
        correlator.expect(timed_out_id.clone());
        correlator.timeout(&timed_out_id);
        assert_eq!(
            correlator.accept(response_for(timed_out_id)),
            Err(CodecError::LateResponse)
        );
        Ok(())
    }

    #[test]
    fn is_fatal_is_false_only_for_duplicate_and_late() {
        for error in [
            CodecError::MalformedHeader,
            CodecError::FrameLimit,
            CodecError::MalformedMessage,
            CodecError::BatchRejected,
            CodecError::MessageLimit,
            CodecError::ByteLimit,
            CodecError::UnknownResponseId,
        ] {
            assert!(error.is_fatal(), "{error:?}");
        }
        for error in [CodecError::DuplicateResponse, CodecError::LateResponse] {
            assert!(!error.is_fatal(), "{error:?}");
        }
    }

    // ---- hostile-peer: server-initiated request answered with -32601 ----

    #[test]
    fn server_request_is_answered_with_method_not_found_never_a_result()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut decoder = Decoder::new(DecoderLimits::default());
        let messages = decoder.feed(&frame(
            "{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"window/workDoneProgress/create\",\"params\":{}}",
        ))?;
        let RawMessage::Request { id, method, .. } = &messages[0] else {
            unreachable!("expected a request");
        };
        assert_eq!(method, "window/workDoneProgress/create");
        let response = OutgoingMessage::Response {
            id: id.clone(),
            error: ResponseError::method_not_found(method),
        };
        let encoded = encode(&response)?;
        let text = String::from_utf8(encoded)?;
        assert!(text.contains("-32601"));
        assert!(!text.contains("\"result\""));
        Ok(())
    }

    // ---- encode/decode round trip ----

    #[test]
    fn encode_then_decode_round_trips_a_request() -> Result<(), Box<dyn std::error::Error>> {
        let message = OutgoingMessage::Request {
            id: RequestId::Number(3),
            method: "textDocument/documentSymbol".to_owned(),
            params: Some(serde_json::json!({"textDocument": {"uri": "file:///source/a.rs"}})),
        };
        let bytes = encode(&message)?;
        let mut decoder = Decoder::new(DecoderLimits::default());
        let messages = decoder.feed(&bytes)?;
        assert_eq!(messages.len(), 1);
        let RawMessage::Request { id, method, params } = &messages[0] else {
            unreachable!("expected a request");
        };
        assert_eq!(*id, RequestId::Number(3));
        assert_eq!(method, "textDocument/documentSymbol");
        assert!(params.is_some());
        Ok(())
    }

    // ---- initialize params / config digest ----

    #[test]
    fn config_digest_is_stable_and_a_valid_fingerprint() -> Result<(), domain::AnalyzerError> {
        let first = config_digest()?;
        let second = config_digest()?;
        assert_eq!(first, second);
        assert!(first.as_str().starts_with("sha256:"));
        Ok(())
    }

    #[test]
    fn initialize_params_serializes_the_fixed_capabilities_and_options()
    -> Result<(), Box<dyn std::error::Error>> {
        let params = InitializeParams::new();
        let value = serde_json::to_value(&params)?;
        assert_eq!(
            value["capabilities"]["general"]["positionEncodings"],
            serde_json::json!(["utf-8"])
        );
        assert_eq!(value["initializationOptions"]["cargo"]["noDeps"], true);
        assert_eq!(
            value["initializationOptions"]["linkedProjects"],
            serde_json::json!(["/source/Cargo.toml"])
        );
        Ok(())
    }

    #[test]
    fn code_action_capability_declares_literal_support_and_nothing_else()
    -> Result<(), Box<dyn std::error::Error>> {
        let value = serde_json::to_value(InitializeParams::new())?;
        let code_action = &value["capabilities"]["textDocument"]["codeAction"];
        let object = code_action
            .as_object()
            .ok_or("textDocument.codeAction must be an object")?;
        let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "codeActionLiteralSupport",
                "dataSupport",
                "disabledSupport",
                "isPreferredSupport",
            ],
            "exactly these keys: an extra one would advertise an effect this \
             client never grants, a missing one lets the server fall back to \
             bare Command objects"
        );
        assert_eq!(code_action["isPreferredSupport"], serde_json::json!(true));
        assert_eq!(code_action["dataSupport"], serde_json::json!(false));
        assert_eq!(code_action["disabledSupport"], serde_json::json!(false));
        let literal = code_action["codeActionLiteralSupport"]
            .as_object()
            .ok_or("codeActionLiteralSupport must be an object")?;
        assert_eq!(
            literal.keys().map(String::as_str).collect::<Vec<_>>(),
            ["codeActionKind"]
        );
        let kind = code_action["codeActionLiteralSupport"]["codeActionKind"]
            .as_object()
            .ok_or("codeActionKind must be an object")?;
        assert_eq!(
            kind.keys().map(String::as_str).collect::<Vec<_>>(),
            ["valueSet"]
        );
        assert_eq!(
            code_action["codeActionLiteralSupport"]["codeActionKind"]["valueSet"],
            serde_json::json!([
                "quickfix",
                "refactor",
                "refactor.extract",
                "refactor.inline",
                "refactor.rewrite",
                "source",
                "source.organizeImports",
            ]),
            "the value set must be exactly the kinds domain::CodeActionKind resolves"
        );
        assert!(
            object.get("resolveSupport").is_none(),
            "no codeAction/resolve round trip exists in this lifecycle"
        );
        Ok(())
    }

    // ---- lsp_uri_to_file ----

    #[test]
    fn uri_to_file_accepts_only_source_scoped_rs_paths() {
        assert!(lsp_uri_to_file("file:///source/src/lib.rs").is_ok());
        for external in [
            "file:///opt/rust/lib/foo.rs",
            "file:///source/../etc/passwd",
            "file:///source/src/lib.txt",
            "http://example.com/a.rs",
            "file:///source/%2e%2e/etc/passwd.rs",
        ] {
            assert_eq!(
                lsp_uri_to_file(external),
                Err(UriRejection::External),
                "{external}"
            );
        }
    }

    // ---- document symbols / diagnostics / references conversions ----

    fn analyzer_file(path: &str) -> Result<domain::AnalyzerFile, domain::AnalyzerError> {
        domain::AnalyzerFile::new(path.to_owned())
    }

    fn lsp_range(sl: u32, sc: u32, el: u32, ec: u32) -> LspRange {
        LspRange {
            start: LspPosition {
                line: sl,
                character: sc,
            },
            end: LspPosition {
                line: el,
                character: ec,
            },
        }
    }

    #[test]
    fn document_symbols_flatten_depth_first_with_depth_and_cap() -> Result<(), domain::AnalyzerError>
    {
        let index = domain::LineIndex::new(b"fn outer() {\n    fn inner() {}\n}\n")?;
        let child = LspDocumentSymbol {
            name: "inner".into(),
            detail: None,
            kind: 12,
            deprecated: None,
            range: lsp_range(1, 0, 1, 17),
            selection_range: lsp_range(1, 7, 1, 12),
            children: None,
        };
        let parent = LspDocumentSymbol {
            name: "outer".into(),
            detail: None,
            kind: 12,
            deprecated: None,
            range: lsp_range(0, 0, 2, 1),
            selection_range: lsp_range(0, 3, 0, 8),
            children: Some(vec![child]),
        };
        let (flat, truncated) =
            document_symbols_to_domain(vec![parent], &index, domain::PositionEncoding::Utf8)?;
        assert_eq!(truncated, 0);
        assert_eq!(flat.len(), 2);
        assert_eq!(flat[0].name().as_str(), "outer");
        assert_eq!(flat[0].depth(), 0);
        assert_eq!(flat[1].name().as_str(), "inner");
        assert_eq!(flat[1].depth(), 1);
        Ok(())
    }

    /// A leaf symbol whose ranges resolve against `SYMBOL_SOURCE`.
    fn symbol_node(children: Option<Vec<LspDocumentSymbol>>) -> LspDocumentSymbol {
        LspDocumentSymbol {
            name: "s".into(),
            detail: None,
            kind: 12,
            deprecated: None,
            range: lsp_range(0, 0, 0, 9),
            selection_range: lsp_range(0, 3, 0, 4),
            children,
        }
    }

    const SYMBOL_SOURCE: &[u8] = b"fn a() {}\n";

    /// A `depth`-deep chain of single-child symbols, built iteratively so the
    /// test itself never recurses.
    fn symbol_chain(depth: usize) -> LspDocumentSymbol {
        let mut node = symbol_node(None);
        for _ in 0..depth {
            node = symbol_node(Some(vec![node]));
        }
        node
    }

    #[test]
    fn document_symbols_stop_descending_at_the_depth_cap() -> Result<(), Box<dyn std::error::Error>>
    {
        const CHAIN_DEPTH: usize = 10_000;
        let index = domain::LineIndex::new(SYMBOL_SOURCE)?;
        let visible = usize::from(domain::MAX_SYMBOL_DEPTH) + 1;

        // A chain far deeper than the cap, on its own: the walk stops at the
        // cap and every node below it is counted, not visited.
        let (flat, omitted) = document_symbols_to_domain(
            vec![symbol_chain(CHAIN_DEPTH)],
            &index,
            domain::PositionEncoding::Utf8,
        )?;
        assert_eq!(flat.len(), visible);
        assert_eq!(flat[visible - 1].depth(), domain::MAX_SYMBOL_DEPTH);
        assert_eq!(omitted, CHAIN_DEPTH + 1 - visible);

        // The same chain behind a full visible cap: the branch that only
        // counts must enforce the depth bound too.
        let mut symbols: Vec<LspDocumentSymbol> = (0..domain::MAX_VISIBLE_RESULTS)
            .map(|_| symbol_node(None))
            .collect();
        symbols.push(symbol_chain(CHAIN_DEPTH));
        let (flat, omitted) =
            document_symbols_to_domain(symbols, &index, domain::PositionEncoding::Utf8)?;
        assert_eq!(flat.len(), domain::MAX_VISIBLE_RESULTS);
        assert_eq!(
            omitted,
            CHAIN_DEPTH + 1,
            "every node of the over-deep chain is a counted omission"
        );
        Ok(())
    }

    #[test]
    fn document_symbols_omit_oversized_or_control_char_names_and_truncate_detail()
    -> Result<(), Box<dyn std::error::Error>> {
        let index = domain::LineIndex::new(SYMBOL_SOURCE)?;
        let mut oversized_name = symbol_node(None);
        oversized_name.name = "n".repeat(257);
        let mut control_char_name = symbol_node(None);
        control_char_name.name = "bad\u{0007}name".into();
        let mut long_detail = symbol_node(None);
        long_detail.detail = Some("d".repeat(1_025));
        let short_detail = symbol_node(None);
        let (flat, omitted) = document_symbols_to_domain(
            vec![oversized_name, control_char_name, long_detail, short_detail],
            &index,
            domain::PositionEncoding::Utf8,
        )?;
        assert_eq!(
            omitted, 2,
            "both oversized and control-char names are omitted"
        );
        assert_eq!(flat.len(), 2);
        assert_eq!(
            flat[0].detail().map(str::chars).map(Iterator::count),
            Some(1_024)
        );
        assert!(flat[0].detail_truncated());
        assert!(!flat[1].detail_truncated());
        Ok(())
    }

    #[test]
    fn diagnostics_convert_and_omit_cross_file_related_information()
    -> Result<(), Box<dyn std::error::Error>> {
        let file = analyzer_file("a.rs")?;
        let index = domain::LineIndex::new(b"let x = 1;\n")?;
        let diagnostic = LspDiagnostic {
            range: lsp_range(0, 4, 0, 5),
            severity: Some(3),
            code: Some(serde_json::json!("E0308")),
            message: "mismatched types".into(),
            related_information: Some(vec![
                DiagnosticRelatedInformation {
                    location: Location {
                        uri: "file:///source/a.rs".into(),
                        range: lsp_range(0, 0, 0, 3),
                    },
                    message: "let binding".into(),
                },
                DiagnosticRelatedInformation {
                    location: Location {
                        uri: "file:///source/b.rs".into(),
                        range: lsp_range(0, 0, 0, 3),
                    },
                    message: "elsewhere".into(),
                },
            ]),
        };
        let (converted, truncated) = diagnostics_to_domain(
            &file,
            vec![diagnostic],
            &index,
            domain::PositionEncoding::Utf8,
        )?;
        assert_eq!(truncated, 0);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].code(), Some("E0308"));
        assert_eq!(
            converted[0].related().len(),
            1,
            "the cross-file related entry is omitted"
        );
        Ok(())
    }

    #[test]
    fn references_span_multiple_files_and_omit_external_ones()
    -> Result<(), Box<dyn std::error::Error>> {
        let a = analyzer_file("a.rs")?;
        let b = analyzer_file("b.rs")?;
        let mut indices = BTreeMap::new();
        indices.insert(a.clone(), domain::LineIndex::new(b"fn a() {}\n")?);
        indices.insert(b.clone(), domain::LineIndex::new(b"fn b() {}\n")?);
        let declaration = Location {
            uri: "file:///source/a.rs".into(),
            range: lsp_range(0, 3, 0, 4),
        };
        let locations = vec![
            declaration.clone(),
            Location {
                uri: "file:///source/b.rs".into(),
                range: lsp_range(0, 3, 0, 4),
            },
            Location {
                uri: "file:///opt/rust/sysroot.rs".into(),
                range: lsp_range(0, 0, 0, 1),
            },
        ];
        let (references, omitted) = references_to_domain(
            locations,
            std::slice::from_ref(&declaration),
            &indices,
            domain::PositionEncoding::Utf8,
        )?;
        assert_eq!(references.len(), 2);
        assert_eq!(omitted, 1);
        assert!(references[0].is_declaration);
        assert!(!references[1].is_declaration);
        Ok(())
    }

    // ---- workspace symbols ----

    fn workspace_symbol(name: &str, uri: &str, range: LspRange) -> SymbolInformation {
        SymbolInformation {
            name: name.to_owned(),
            kind: 12,
            container_name: None,
            location: Location {
                uri: uri.to_owned(),
                range,
            },
        }
    }

    #[test]
    fn workspace_symbols_sort_deterministically_and_count_omissions()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut indices = BTreeMap::new();
        indices.insert(analyzer_file("a.rs")?, domain::LineIndex::new(b"ab\ncd\n")?);
        indices.insert(analyzer_file("b.rs")?, domain::LineIndex::new(b"ab\ncd\n")?);
        // Deliberately out of order, and with two symbols sharing a position.
        let symbols = vec![
            workspace_symbol("zeta", "file:///source/b.rs", lsp_range(0, 0, 0, 1)),
            workspace_symbol("beta", "file:///source/a.rs", lsp_range(1, 0, 1, 1)),
            workspace_symbol("alpha", "file:///source/a.rs", lsp_range(1, 0, 1, 1)),
            workspace_symbol("gamma", "file:///source/a.rs", lsp_range(0, 0, 0, 1)),
            // Outside /source: an omission, not an error.
            workspace_symbol("extern", "file:///opt/rust/core.rs", lsp_range(0, 0, 0, 1)),
            // In scope but absent from the snapshot: no index to resolve it.
            workspace_symbol("missing", "file:///source/c.rs", lsp_range(0, 0, 0, 1)),
        ];
        let (converted, omitted) =
            workspace_symbols_to_domain(symbols, &indices, domain::PositionEncoding::Utf8)?;
        assert_eq!(omitted, 2);
        let order: Vec<(&str, &str)> = converted
            .iter()
            .map(|symbol| (symbol.file.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(
            order,
            [
                ("a.rs", "gamma"),
                ("a.rs", "alpha"),
                ("a.rs", "beta"),
                ("b.rs", "zeta"),
            ],
            "sorted by (file, range start, name), not by the server's order"
        );
        Ok(())
    }

    #[test]
    fn workspace_symbols_cap_the_visible_set_and_count_the_remainder()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut indices = BTreeMap::new();
        indices.insert(analyzer_file("a.rs")?, domain::LineIndex::new(b"ab\n")?);
        let over = 7usize;
        let symbols: Vec<SymbolInformation> = (0..domain::MAX_VISIBLE_RESULTS + over)
            .map(|n| {
                workspace_symbol(
                    &format!("s{n:04}"),
                    "file:///source/a.rs",
                    lsp_range(0, 0, 0, 1),
                )
            })
            .collect();
        let (converted, omitted) =
            workspace_symbols_to_domain(symbols, &indices, domain::PositionEncoding::Utf8)?;
        assert_eq!(converted.len(), domain::MAX_VISIBLE_RESULTS);
        assert_eq!(omitted, over);
        assert_eq!(
            converted[0].name.as_str(),
            "s0000",
            "the cap keeps the first entries of the sorted order, not of the wire order"
        );
        Ok(())
    }

    #[test]
    fn workspace_symbols_omit_oversized_or_control_char_name_and_container()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut indices = BTreeMap::new();
        indices.insert(analyzer_file("a.rs")?, domain::LineIndex::new(b"ab\n")?);
        let mut oversized_name =
            workspace_symbol("s", "file:///source/a.rs", lsp_range(0, 0, 0, 1));
        oversized_name.name = "n".repeat(257);
        let mut control_char_container =
            workspace_symbol("ok", "file:///source/a.rs", lsp_range(0, 0, 0, 1));
        control_char_container.container_name = Some("bad\u{0007}container".into());
        let short = workspace_symbol("short", "file:///source/a.rs", lsp_range(0, 0, 0, 1));
        let (converted, omitted) = workspace_symbols_to_domain(
            vec![oversized_name, control_char_container, short],
            &indices,
            domain::PositionEncoding::Utf8,
        )?;
        assert_eq!(omitted, 2);
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].name.as_str(), "short");
        Ok(())
    }

    // ---- code actions / workspace edits ----

    fn code_action_value(
        value: serde_json::Value,
    ) -> Result<CodeActionOrCommand, serde_json::Error> {
        serde_json::from_value(value)
    }

    fn source_indices(
        files: &[(&str, &[u8])],
    ) -> Result<BTreeMap<domain::AnalyzerFile, domain::LineIndex>, domain::AnalyzerError> {
        files
            .iter()
            .map(|(path, bytes)| Ok((analyzer_file(path)?, domain::LineIndex::new(bytes)?)))
            .collect()
    }

    #[test]
    fn workspace_edit_with_create_file_is_rejected_as_resource_operation()
    -> Result<(), Box<dyn std::error::Error>> {
        let indices = source_indices(&[("a.rs", b"fn a() {}\n")])?;
        let action = code_action_value(serde_json::json!({
            "title": "create it",
            "edit": {
                "documentChanges": [
                    {"kind": "create", "uri": "file:///source/new.rs"}
                ]
            }
        }))?;
        let resolved = resolve_action(action, &indices, domain::PositionEncoding::Utf8);
        assert_eq!(resolved, Err(domain::ActionRejection::ResourceOperation));
        Ok(())
    }

    #[test]
    fn workspace_edit_with_snippet_edit_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let indices = source_indices(&[("a.rs", b"fn a() {}\n")])?;
        let action = code_action_value(serde_json::json!({
            "title": "snippet",
            "edit": {
                "documentChanges": [{
                    "textDocument": {"uri": "file:///source/a.rs", "version": 1},
                    "edits": [{
                        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
                        "newText": "${1:x}",
                        "insertTextFormat": 2
                    }]
                }]
            }
        }))?;
        let resolved = resolve_action(action, &indices, domain::PositionEncoding::Utf8);
        assert_eq!(resolved, Err(domain::ActionRejection::Snippet));
        Ok(())
    }

    #[test]
    fn code_action_with_a_command_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let indices = source_indices(&[])?;
        let bare_command = code_action_value(serde_json::json!({
            "title": "run",
            "command": "rust-analyzer.someCommand",
            "arguments": []
        }))?;
        assert_eq!(
            resolve_action(bare_command, &indices, domain::PositionEncoding::Utf8),
            Err(domain::ActionRejection::Command)
        );
        let action_with_command = code_action_value(serde_json::json!({
            "title": "apply then run",
            "edit": {"documentChanges": []},
            "command": {"title": "run", "command": "rust-analyzer.someCommand"}
        }))?;
        assert_eq!(
            resolve_action(
                action_with_command,
                &indices,
                domain::PositionEncoding::Utf8
            ),
            Err(domain::ActionRejection::Command)
        );
        Ok(())
    }

    #[test]
    fn workspace_edit_with_external_uri_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let indices = source_indices(&[])?;
        let action = code_action_value(serde_json::json!({
            "title": "external",
            "edit": {
                "documentChanges": [{
                    "textDocument": {"uri": "file:///opt/rust/sysroot.rs", "version": 1},
                    "edits": []
                }]
            }
        }))?;
        assert_eq!(
            resolve_action(action, &indices, domain::PositionEncoding::Utf8),
            Err(domain::ActionRejection::ExternalUri)
        );
        Ok(())
    }

    #[test]
    fn workspace_edit_with_overlapping_edits_is_rejected() -> Result<(), Box<dyn std::error::Error>>
    {
        let indices = source_indices(&[("a.rs", b"abcdef\n")])?;
        let action = code_action_value(serde_json::json!({
            "title": "overlap",
            "edit": {
                "documentChanges": [{
                    "textDocument": {"uri": "file:///source/a.rs", "version": 1},
                    "edits": [
                        {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}}, "newText": "X"},
                        {"range": {"start": {"line": 0, "character": 2}, "end": {"line": 0, "character": 4}}, "newText": "Y"}
                    ]
                }]
            }
        }))?;
        assert_eq!(
            resolve_action(action, &indices, domain::PositionEncoding::Utf8),
            Err(domain::ActionRejection::OverlappingRanges)
        );
        Ok(())
    }

    #[test]
    fn workspace_edit_with_version_mismatch_is_rejected() -> Result<(), Box<dyn std::error::Error>>
    {
        let indices = source_indices(&[("a.rs", b"abcdef\n")])?;
        let action = code_action_value(serde_json::json!({
            "title": "stale",
            "edit": {
                "documentChanges": [{
                    "textDocument": {"uri": "file:///source/a.rs", "version": 2},
                    "edits": [
                        {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "newText": "X"}
                    ]
                }]
            }
        }))?;
        assert_eq!(
            resolve_action(action, &indices, domain::PositionEncoding::Utf8),
            Err(domain::ActionRejection::VersionMismatch)
        );
        Ok(())
    }

    #[test]
    fn clean_multi_edit_action_converts_and_round_trips_through_apply_edits()
    -> Result<(), Box<dyn std::error::Error>> {
        let indices = source_indices(&[("a.rs", b"abcdef\n")])?;
        let action = code_action_value(serde_json::json!({
            "title": "tidy",
            "kind": "quickfix",
            "isPreferred": true,
            "edit": {
                "documentChanges": [{
                    "textDocument": {"uri": "file:///source/a.rs", "version": 1},
                    "edits": [
                        {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}, "newText": "X"},
                        {"range": {"start": {"line": 0, "character": 3}, "end": {"line": 0, "character": 4}}, "newText": ""}
                    ]
                }]
            }
        }))?;
        let resolved = resolve_action(action, &indices, domain::PositionEncoding::Utf8)
            .map_err(|rejection| format!("{rejection:?}"))?;
        assert_eq!(resolved.kind, Some(domain::CodeActionKind::QuickFix));
        assert!(resolved.is_preferred);
        assert_eq!(resolved.edits.len(), 2);
        let edits: Vec<(domain::TextRange, &str)> = resolved
            .edits
            .iter()
            .map(|edit| (edit.range, edit.new_text.as_str()))
            .collect();
        let after = domain::apply_edits(b"abcdef\n", &edits)?;
        assert_eq!(after, b"Xbcef\n");
        Ok(())
    }

    /// A `WorkspaceEdit` action over `a.rs` carrying `edits` verbatim.
    fn action_with_edits(edits: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "title": "edit",
            "edit": {
                "documentChanges": [{
                    "textDocument": {"uri": "file:///source/a.rs", "version": 1},
                    "edits": edits,
                }],
            },
        })
    }

    fn insertion(character: u32, new_text: &str) -> serde_json::Value {
        serde_json::json!({
            "range": {
                "start": {"line": 0, "character": character},
                "end": {"line": 0, "character": character},
            },
            "newText": new_text,
        })
    }

    #[test]
    fn an_edit_for_a_file_outside_the_snapshot_is_not_an_encoding_failure()
    -> Result<(), Box<dyn std::error::Error>> {
        // `b.rs` is a legitimate in-scope path (a multi-file rename would
        // touch one), but this resolution was given no bytes for it.
        let indices = source_indices(&[("a.rs", b"fn a() {}\n")])?;
        let action = code_action_value(serde_json::json!({
            "title": "rename across files",
            "edit": {
                "documentChanges": [{
                    "textDocument": {"uri": "file:///source/b.rs", "version": 1},
                    "edits": [],
                }],
            },
        }))?;
        assert_eq!(
            resolve_action(action, &indices, domain::PositionEncoding::Utf8),
            Err(domain::ActionRejection::FileNotInSnapshot)
        );
        Ok(())
    }

    #[test]
    fn only_genuinely_undecodable_bytes_are_not_utf8() -> Result<(), Box<dyn std::error::Error>> {
        let file = analyzer_file("a.rs")?;
        assert_eq!(
            snapshot_indices([(file.clone(), [0xffu8, 0xfe].as_slice())]).err(),
            Some(domain::ActionRejection::NotUtf8)
        );
        let indices = snapshot_indices([(file, "fn á() {}\n".as_bytes())])
            .map_err(|rejection| format!("{rejection:?}"))?;
        assert_eq!(indices.len(), 1);
        Ok(())
    }

    #[test]
    fn one_malformed_element_rejects_only_itself() -> Result<(), Box<dyn std::error::Error>> {
        let indices = source_indices(&[("a.rs", b"abcdef\n")])?;
        let candidates = code_actions_to_candidates(
            vec![
                // Matches neither `CodeAction` (no `title`) nor `Command`.
                serde_json::json!({"kind": "quickfix"}),
                action_with_edits(serde_json::json!([insertion(0, "X")])),
                serde_json::json!(42),
                serde_json::json!({
                    "title": "bare",
                    "command": "rust-analyzer.run",
                }),
            ],
            &indices,
            domain::PositionEncoding::Utf8,
        );
        assert_eq!(candidates.len(), 4);
        assert_eq!(
            candidates[0],
            Err(domain::ActionRejection::UnresolvedEdit),
            "a malformed element must not fail the batch"
        );
        let resolved = candidates[1]
            .as_ref()
            .map_err(|rejection| format!("{rejection:?}"))?;
        assert_eq!(resolved.edits.len(), 1);
        assert_eq!(candidates[2], Err(domain::ActionRejection::UnresolvedEdit));
        assert_eq!(
            candidates[3],
            Err(domain::ActionRejection::Command),
            "a well-formed bare Command keeps its own precise reason"
        );
        Ok(())
    }

    #[test]
    fn edits_are_ordered_by_range_and_coincident_starts_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let indices = source_indices(&[("a.rs", b"abcdef\n")])?;
        // Supplied out of order: the resolved order must come from the ranges.
        let ordered = code_action_value(action_with_edits(serde_json::json!([
            insertion(4, "C"),
            insertion(0, "A"),
            insertion(2, "B"),
        ])))?;
        let resolved = resolve_action(ordered, &indices, domain::PositionEncoding::Utf8)
            .map_err(|rejection| format!("{rejection:?}"))?;
        let texts: Vec<&str> = resolved
            .edits
            .iter()
            .map(|edit| edit.new_text.as_str())
            .collect();
        assert_eq!(texts, ["A", "B", "C"]);

        // Two zero-width insertions at the same position have no canonical
        // order, so neither ordering may be silently picked.
        let coincident = code_action_value(action_with_edits(serde_json::json!([
            insertion(1, "A"),
            insertion(1, "B"),
        ])))?;
        assert_eq!(
            resolve_action(coincident, &indices, domain::PositionEncoding::Utf8),
            Err(domain::ActionRejection::OverlappingRanges)
        );

        // An insertion coincident with the start of a replacement is the same
        // ambiguity, in either submission order.
        let replacement = serde_json::json!({
            "range": {
                "start": {"line": 0, "character": 1},
                "end": {"line": 0, "character": 3},
            },
            "newText": "R",
        });
        for edits in [
            serde_json::json!([insertion(1, "A"), replacement.clone()]),
            serde_json::json!([replacement, insertion(1, "A")]),
        ] {
            let action = code_action_value(action_with_edits(edits))?;
            assert_eq!(
                resolve_action(action, &indices, domain::PositionEncoding::Utf8),
                Err(domain::ActionRejection::OverlappingRanges)
            );
        }
        Ok(())
    }

    #[test]
    fn edit_and_byte_limits_are_enforced() -> Result<(), Box<dyn std::error::Error>> {
        let line = "a".repeat(600);
        let mut source = line.into_bytes();
        source.push(b'\n');
        let indices = source_indices(&[("a.rs", source.as_slice())])?;

        let at_limit: Vec<serde_json::Value> = (0..domain::MAX_EDITS)
            .map(|n| insertion(n as u32, "x"))
            .collect();
        let accepted = code_action_value(action_with_edits(serde_json::json!(at_limit)))?;
        assert!(
            resolve_action(accepted, &indices, domain::PositionEncoding::Utf8).is_ok(),
            "exactly MAX_EDITS edits are still applicable"
        );

        let over_limit: Vec<serde_json::Value> = (0..domain::MAX_EDITS + 1)
            .map(|n| insertion(n as u32, "x"))
            .collect();
        let rejected = code_action_value(action_with_edits(serde_json::json!(over_limit)))?;
        assert_eq!(
            resolve_action(rejected, &indices, domain::PositionEncoding::Utf8),
            Err(domain::ActionRejection::EditLimit)
        );

        // Few edits, but more replacement text than a result may carry.
        let half = domain::MAX_RESULT_BYTES / 2 + 1;
        let heavy = code_action_value(action_with_edits(serde_json::json!([
            insertion(0, &"x".repeat(half)),
            insertion(1, &"y".repeat(half)),
        ])))?;
        assert_eq!(
            resolve_action(heavy, &indices, domain::PositionEncoding::Utf8),
            Err(domain::ActionRejection::BytesLimit)
        );
        Ok(())
    }
}
