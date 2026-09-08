use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use proc_macro2::Span;
use serde::de::{SeqAccess, Visitor as SerdeVisitor};
use serde::{Deserialize, Serialize};
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Attribute, Expr, ForeignItem, GenericParam, ImplItem, Item, Lit, Meta, Pat, Safety, Stmt,
    TraitItem, Type, TypeParamBound,
};

pub const MANIFEST_PATH: &str = "/security/scan.json";
pub const HELPER_PATH: &str = "/opt/security/bin/rust-mcp-unsafe-helper";
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;
pub const MAX_FILES: usize = 4096;
pub const MAX_FINDINGS: usize = 128;
pub const MAX_BUDGET_MS: u64 = 118_000;
const MAX_CHILD_STDOUT_BYTES: usize = 512 * 1024;
const CHILD_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(5);
const FIXED_PATH: &str = "/usr/bin:/bin";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanManifest {
    pub schema_version: u32,
    pub budget_ms: u64,
    #[serde(deserialize_with = "deserialize_manifest_files")]
    pub files: Vec<ManifestFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestFile {
    pub index: u32,
    pub path: String,
}

fn deserialize_manifest_files<'de, D>(deserializer: D) -> Result<Vec<ManifestFile>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ManifestFilesVisitor;

    impl<'de> SerdeVisitor<'de> for ManifestFilesVisitor {
        type Value = Vec<ManifestFile>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("at most 4096 manifest file entries")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut files = Vec::with_capacity(sequence.size_hint().unwrap_or(0).min(MAX_FILES));
            while let Some(file) = sequence.next_element()? {
                if files.len() == MAX_FILES {
                    return Err(serde::de::Error::custom("too many manifest files"));
                }
                files.push(file);
            }
            Ok(files)
        }
    }

    deserializer.deserialize_seq(ManifestFilesVisitor)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Parsed,
    ParseError,
    Crashed,
    TimedOut,
    Unavailable,
    InvalidUtf8,
    TooLarge,
    BudgetExhausted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingKind {
    UnsafeAttribute,
    UnsafeBlock,
    UnsafeExternBlock,
    UnsafeFn,
    UnsafeImpl,
    UnsafeMod,
    UnsafeStatic,
    UnsafeTrait,
    ExternBlock,
    ExternCrate,
    ExternFn,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub file_index: u32,
    pub kind: FindingKind,
    pub byte_start: usize,
    pub byte_end: usize,
    pub line: usize,
    pub column: usize,
    pub conditional: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerOutput {
    pub schema_version: u32,
    pub file_index: u32,
    pub status: FileStatus,
    pub findings: Vec<Finding>,
    pub total_findings: u64,
    pub omitted_findings: u64,
    #[serde(rename = "macro_omitted")]
    pub macro_boundaries_omitted: u64,
    pub opaque_syntax_omitted: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FileSummary {
    #[serde(rename = "i")]
    pub index: u32,
    #[serde(rename = "s")]
    pub status: FileStatus,
    #[serde(rename = "total")]
    pub total_findings: u64,
    #[serde(rename = "omitted")]
    pub omitted_findings: u64,
    #[serde(rename = "macros")]
    pub macro_boundaries_omitted: u64,
    #[serde(rename = "opaque")]
    pub opaque_syntax_omitted: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SupervisorOutput {
    pub schema_version: u32,
    pub files: Vec<FileSummary>,
    pub findings: Vec<Finding>,
    pub total_findings: u64,
    pub omitted_findings: u64,
    pub cfg_evaluated: bool,
    pub macros_expanded: bool,
    pub generated_sources_scanned: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FatalErrorCode {
    InvalidArguments,
    ManifestUnavailable,
    ManifestTooLarge,
    ManifestInvalid,
    OutputTooLarge,
}

#[derive(Debug, Serialize)]
pub struct FatalOutput {
    pub schema_version: u32,
    pub error: FatalErrorCode,
}

impl FatalOutput {
    #[must_use]
    pub const fn new(error: FatalErrorCode) -> Self {
        Self {
            schema_version: 2,
            error,
        }
    }
}

pub fn load_manifest(path: impl AsRef<Path>) -> Result<ScanManifest, FatalErrorCode> {
    let bytes = read_bounded(path.as_ref(), MAX_MANIFEST_BYTES).map_err(|error| match error {
        ReadBoundedError::Unavailable => FatalErrorCode::ManifestUnavailable,
        ReadBoundedError::TooLarge => FatalErrorCode::ManifestTooLarge,
    })?;
    let manifest: ScanManifest =
        serde_json::from_slice(&bytes).map_err(|_| FatalErrorCode::ManifestInvalid)?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

pub fn validate_manifest(manifest: &ScanManifest) -> Result<(), FatalErrorCode> {
    if manifest.schema_version != 2
        || !(1..=MAX_BUDGET_MS).contains(&manifest.budget_ms)
        || manifest.files.len() > MAX_FILES
    {
        return Err(FatalErrorCode::ManifestInvalid);
    }

    let mut paths = std::collections::BTreeSet::new();
    for (position, file) in manifest.files.iter().enumerate() {
        let expected_index =
            u32::try_from(position).map_err(|_| FatalErrorCode::ManifestInvalid)?;
        if file.index != expected_index
            || !valid_source_path(&file.path)
            || !paths.insert(file.path.as_str())
        {
            return Err(FatalErrorCode::ManifestInvalid);
        }
    }
    Ok(())
}

pub fn run_file_worker(
    manifest_path: impl AsRef<Path>,
    file_index: u32,
) -> Result<WorkerOutput, FatalErrorCode> {
    let manifest = load_manifest(manifest_path)?;
    let position = usize::try_from(file_index).map_err(|_| FatalErrorCode::InvalidArguments)?;
    let file = manifest
        .files
        .get(position)
        .filter(|entry| entry.index == file_index)
        .ok_or(FatalErrorCode::InvalidArguments)?;

    Ok(worker_output_from_bytes(
        file_index,
        read_bounded(Path::new(&file.path), MAX_SOURCE_BYTES),
    ))
}

fn worker_output_from_bytes(
    file_index: u32,
    bytes: Result<Vec<u8>, ReadBoundedError>,
) -> WorkerOutput {
    let bytes = match bytes {
        Ok(bytes) => bytes,
        Err(ReadBoundedError::Unavailable) => {
            return empty_worker_output(file_index, FileStatus::Unavailable);
        }
        Err(ReadBoundedError::TooLarge) => {
            return empty_worker_output(file_index, FileStatus::TooLarge);
        }
    };
    let source = match std::str::from_utf8(&bytes) {
        Ok(source) => source,
        Err(_) => return empty_worker_output(file_index, FileStatus::InvalidUtf8),
    };
    scan_source(file_index, source)
}

#[must_use]
pub fn scan_source(file_index: u32, source: &str) -> WorkerOutput {
    let syntax = match syn::parse_file(source) {
        Ok(syntax) => syntax,
        Err(_) => return empty_worker_output(file_index, FileStatus::ParseError),
    };

    let mut scanner = UnsafeScanner::new(file_index, source);
    scanner.visit_file(&syntax);
    if scanner.invalid_span {
        std::mem::forget(syntax);
        return empty_worker_output(file_index, FileStatus::Crashed);
    }
    scanner.findings.sort_by_key(|finding| {
        (
            finding.byte_start,
            finding.byte_end,
            finding.kind,
            finding.conditional,
        )
    });

    let total_findings = u64::try_from(scanner.findings.len()).unwrap_or(u64::MAX);
    scanner.findings.truncate(MAX_FINDINGS);
    let retained = u64::try_from(scanner.findings.len()).unwrap_or(u64::MAX);
    let output = WorkerOutput {
        schema_version: 2,
        file_index,
        status: FileStatus::Parsed,
        findings: scanner.findings,
        total_findings,
        omitted_findings: total_findings.saturating_sub(retained),
        macro_boundaries_omitted: scanner.macro_boundaries_omitted,
        opaque_syntax_omitted: scanner.opaque_syntax_omitted,
    };

    // A recursive hostile AST can also overflow while being dropped. This short-lived
    // file worker deliberately retains it until process termination; the guest bounds
    // the memory and lifetime of the process.
    std::mem::forget(syntax);
    output
}

pub fn run_supervisor(manifest_path: impl AsRef<Path>) -> Result<SupervisorOutput, FatalErrorCode> {
    let manifest = load_manifest(manifest_path)?;
    let started = Instant::now();
    let mut output = aggregate_manifest(&manifest, || started.elapsed(), supervise_file);
    if encoded_size(&output)? > MAX_OUTPUT_BYTES {
        // Preserve every per-file status and count if future encoding changes
        // consume the measured margin around the retained finding rows.
        output.findings.clear();
        output.omitted_findings = output.total_findings;
        for summary in &mut output.files {
            summary.omitted_findings = summary.total_findings;
        }
    }
    if encoded_size(&output)? > MAX_OUTPUT_BYTES {
        return Err(FatalErrorCode::OutputTooLarge);
    }
    Ok(output)
}

fn encoded_size(output: &SupervisorOutput) -> Result<usize, FatalErrorCode> {
    serde_json::to_vec(output)
        .map(|bytes| bytes.len())
        .map_err(|_| FatalErrorCode::OutputTooLarge)
}

#[derive(Debug)]
struct Supervised {
    output: WorkerOutput,
    drain_confirmed: bool,
}

fn aggregate_manifest(
    manifest: &ScanManifest,
    mut elapsed: impl FnMut() -> Duration,
    mut supervise: impl FnMut(u32, Duration) -> Supervised,
) -> SupervisorOutput {
    let mut files = Vec::with_capacity(manifest.files.len());
    let mut retained_findings = Vec::with_capacity(MAX_FINDINGS);
    let mut total_findings = 0_u64;
    let budget = Duration::from_millis(manifest.budget_ms);

    for (position, file) in manifest.files.iter().enumerate() {
        let remaining = budget.saturating_sub(elapsed());
        if remaining.is_zero() {
            append_budget_exhausted(&manifest.files[position..], &mut files);
            break;
        }
        let supervised = supervise(file.index, CHILD_TIMEOUT.min(remaining));
        let outcome = supervised.output;
        total_findings = total_findings.saturating_add(outcome.total_findings);

        let remaining = MAX_FINDINGS.saturating_sub(retained_findings.len());
        let emitted = outcome.findings.len().min(remaining);
        retained_findings.extend(outcome.findings.into_iter().take(emitted));
        let emitted = u64::try_from(emitted).unwrap_or(u64::MAX);
        files.push(FileSummary {
            index: file.index,
            status: outcome.status,
            total_findings: outcome.total_findings,
            omitted_findings: outcome.total_findings.saturating_sub(emitted),
            macro_boundaries_omitted: outcome.macro_boundaries_omitted,
            opaque_syntax_omitted: outcome.opaque_syntax_omitted,
        });
        if !supervised.drain_confirmed {
            append_budget_exhausted(&manifest.files[position + 1..], &mut files);
            break;
        }
    }

    let retained = u64::try_from(retained_findings.len()).unwrap_or(u64::MAX);
    SupervisorOutput {
        schema_version: 2,
        files,
        findings: retained_findings,
        total_findings,
        omitted_findings: total_findings.saturating_sub(retained),
        cfg_evaluated: false,
        macros_expanded: false,
        generated_sources_scanned: false,
    }
}

fn append_budget_exhausted(entries: &[ManifestFile], files: &mut Vec<FileSummary>) {
    files.extend(entries.iter().map(|file| FileSummary {
        index: file.index,
        status: FileStatus::BudgetExhausted,
        total_findings: 0,
        omitted_findings: 0,
        macro_boundaries_omitted: 0,
        opaque_syntax_omitted: 0,
    }));
}

fn supervise_file(index: u32, allowance: Duration) -> Supervised {
    let started = Instant::now();
    let deadline = started.checked_add(allowance).unwrap_or(started);
    let child = Command::new(HELPER_PATH)
        .arg("--file-index")
        .arg(index.to_string())
        .env_clear()
        .env("PATH", FIXED_PATH)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(_) => {
            return Supervised {
                output: empty_worker_output(index, FileStatus::Crashed),
                drain_confirmed: true,
            };
        }
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_and_reap(&mut child);
            return Supervised {
                output: empty_worker_output(index, FileStatus::Crashed),
                drain_confirmed: true,
            };
        }
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    let _reader = thread::spawn(move || {
        let _ = sender.send(read_stream_bounded(stdout, MAX_CHILD_STDOUT_BYTES));
    });
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(Some(status)),
            Ok(None) => {
                let now = Instant::now();
                if now >= deadline {
                    terminate_and_reap(&mut child);
                    break Ok(None);
                }
                thread::sleep(CHILD_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
            }
            Err(_) => {
                terminate_and_reap(&mut child);
                break Err(());
            }
        }
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    let drained = if remaining.is_zero() {
        match receiver.try_recv() {
            Ok(value) => Ok(value),
            Err(TryRecvError::Empty) => Err(false),
            Err(TryRecvError::Disconnected) => Err(true),
        }
    } else {
        match receiver.recv_timeout(remaining) {
            Ok(value) => Ok(value),
            Err(RecvTimeoutError::Timeout) => Err(false),
            Err(RecvTimeoutError::Disconnected) => Err(true),
        }
    };
    let drain_confirmed = !matches!(drained, Err(false));
    let captured = match drained {
        Ok(Ok(bytes)) => Some(bytes),
        _ => None,
    };
    let output = match (status, captured) {
        (Ok(None), _) => empty_worker_output(index, FileStatus::TimedOut),
        (Ok(Some(status)), Some(bytes)) if status.success() => decode_worker_output(index, &bytes),
        _ => empty_worker_output(index, FileStatus::Crashed),
    };
    Supervised {
        output,
        drain_confirmed,
    }
}

fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn decode_worker_output(index: u32, bytes: &[u8]) -> WorkerOutput {
    let output: WorkerOutput = match serde_json::from_slice(bytes) {
        Ok(output) => output,
        Err(_) => return empty_worker_output(index, FileStatus::Crashed),
    };
    if !valid_worker_output(index, &output) {
        return empty_worker_output(index, FileStatus::Crashed);
    }
    output
}

fn valid_worker_output(index: u32, output: &WorkerOutput) -> bool {
    let retained = u64::try_from(output.findings.len()).unwrap_or(u64::MAX);
    output.schema_version == 2
        && output.file_index == index
        && matches!(
            output.status,
            FileStatus::Parsed
                | FileStatus::ParseError
                | FileStatus::Crashed
                | FileStatus::Unavailable
                | FileStatus::InvalidUtf8
                | FileStatus::TooLarge
        )
        && output.findings.len() <= MAX_FINDINGS
        && output.total_findings <= MAX_SOURCE_BYTES as u64
        && output.omitted_findings <= MAX_SOURCE_BYTES as u64
        && output.macro_boundaries_omitted <= MAX_SOURCE_BYTES as u64
        && output.opaque_syntax_omitted <= MAX_SOURCE_BYTES as u64
        && output
            .findings
            .iter()
            .all(|finding| valid_finding(index, finding))
        && output.total_findings == retained.saturating_add(output.omitted_findings)
        && output
            .findings
            .windows(2)
            .all(|pair| finding_key(&pair[0]) < finding_key(&pair[1]))
        && (output.status == FileStatus::Parsed
            || (output.findings.is_empty()
                && output.total_findings == 0
                && output.omitted_findings == 0
                && output.macro_boundaries_omitted == 0
                && output.opaque_syntax_omitted == 0))
}

fn finding_key(finding: &Finding) -> (usize, usize, FindingKind, bool) {
    (
        finding.byte_start,
        finding.byte_end,
        finding.kind,
        finding.conditional,
    )
}

fn valid_finding(index: u32, finding: &Finding) -> bool {
    finding.file_index == index
        && finding.byte_start < finding.byte_end
        && finding.byte_end <= MAX_SOURCE_BYTES
        && (1..=MAX_SOURCE_BYTES).contains(&finding.line)
        && (1..=MAX_SOURCE_BYTES).contains(&finding.column)
}

fn empty_worker_output(file_index: u32, status: FileStatus) -> WorkerOutput {
    WorkerOutput {
        schema_version: 2,
        file_index,
        status,
        findings: Vec::new(),
        total_findings: 0,
        omitted_findings: 0,
        macro_boundaries_omitted: 0,
        opaque_syntax_omitted: 0,
    }
}

fn valid_source_path(path: &str) -> bool {
    let relative = path
        .strip_prefix("/source/")
        .or_else(|| path.strip_prefix("/rust-mcp-vendor/"));
    let Some(relative) = relative else {
        return false;
    };
    if !relative.ends_with(".rs") || relative.is_empty() || relative.contains('\\') {
        return false;
    }
    relative
        .split('/')
        .all(|component| !component.is_empty() && component != "." && component != "..")
}

#[derive(Clone, Copy, Debug)]
enum ReadBoundedError {
    Unavailable,
    TooLarge,
}

fn read_bounded(path: &Path, maximum: usize) -> Result<Vec<u8>, ReadBoundedError> {
    let file = File::open(path).map_err(|_| ReadBoundedError::Unavailable)?;
    let limit = u64::try_from(maximum)
        .map_err(|_| ReadBoundedError::TooLarge)?
        .saturating_add(1);
    let mut bytes = Vec::with_capacity(maximum.min(64 * 1024));
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(|_| ReadBoundedError::Unavailable)?;
    if bytes.len() > maximum {
        return Err(ReadBoundedError::TooLarge);
    }
    Ok(bytes)
}

fn read_stream_bounded(mut stream: impl Read, maximum: usize) -> Result<Vec<u8>, ReadBoundedError> {
    let mut retained = Vec::with_capacity(maximum.min(64 * 1024));
    let mut buffer = [0_u8; 8192];
    let mut overflow = false;
    loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|_| ReadBoundedError::Unavailable)?;
        if count == 0 {
            break;
        }
        if retained.len().saturating_add(count) <= maximum {
            retained.extend_from_slice(&buffer[..count]);
        } else {
            overflow = true;
        }
    }
    if overflow {
        Err(ReadBoundedError::TooLarge)
    } else {
        Ok(retained)
    }
}

struct UnsafeScanner<'source> {
    file_index: u32,
    source: &'source str,
    conditional: bool,
    findings: Vec<Finding>,
    macro_boundaries_omitted: u64,
    opaque_syntax_omitted: u64,
    invalid_span: bool,
}

impl<'source> UnsafeScanner<'source> {
    fn new(file_index: u32, source: &'source str) -> Self {
        Self {
            file_index,
            source,
            conditional: false,
            findings: Vec::new(),
            macro_boundaries_omitted: 0,
            opaque_syntax_omitted: 0,
            invalid_span: false,
        }
    }

    fn record(&mut self, kind: FindingKind, span: Span, keyword: &str) {
        let range = span.byte_range();
        if range.start >= range.end
            || range.end > self.source.len()
            || self.source.get(range.clone()) != Some(keyword)
        {
            self.invalid_span = true;
            return;
        }
        let start = span.start();
        self.findings.push(Finding {
            file_index: self.file_index,
            kind,
            byte_start: range.start,
            byte_end: range.end,
            line: start.line,
            column: start.column.saturating_add(1),
            conditional: self.conditional,
        });
    }

    fn enter_attributes(&mut self, attributes: &[Attribute]) -> bool {
        let previous = self.conditional;
        self.conditional |= attributes.iter().any(is_cfg_attribute);
        previous
    }

    fn opaque(&mut self) {
        self.opaque_syntax_omitted = self.opaque_syntax_omitted.saturating_add(1);
    }

    fn inspect_cfg_attr(&mut self, meta: &Meta) {
        let Meta::List(list) = meta else {
            self.opaque();
            return;
        };
        let nested = list.parse_args_with(Punctuated::<Meta, syn::Token![,]>::parse_terminated);
        let Ok(nested) = nested else {
            self.opaque();
            return;
        };
        if nested.len() < 2 {
            self.opaque();
            return;
        }
        for meta in nested.iter().skip(1) {
            if meta.path().is_ident("unsafe") {
                if let Some(segment) = meta.path().segments.first() {
                    let previous = self.conditional;
                    self.conditional = true;
                    self.record(FindingKind::UnsafeAttribute, segment.ident.span(), "unsafe");
                    self.conditional = previous;
                }
            } else if meta.path().is_ident("cfg_attr") {
                self.inspect_cfg_attr(meta);
            }
        }
    }
}

impl<'ast> Visit<'ast> for UnsafeScanner<'_> {
    fn visit_file(&mut self, node: &'ast syn::File) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_file(self, node);
        self.conditional = previous;
    }

    fn visit_item(&mut self, node: &'ast Item) {
        if matches!(node, Item::Verbatim(_)) {
            self.opaque();
            return;
        }
        let previous = self.enter_attributes(item_attributes(node));
        visit::visit_item(self, node);
        self.conditional = previous;
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        if matches!(node, Expr::Verbatim(_)) {
            self.opaque();
            return;
        }
        let previous = self.enter_attributes(expr_attributes(node));
        visit::visit_expr(self, node);
        self.conditional = previous;
    }

    fn visit_type(&mut self, node: &'ast Type) {
        if matches!(node, Type::Verbatim(_)) {
            self.opaque();
            return;
        }
        let previous = self.enter_attributes(type_attributes(node));
        visit::visit_type(self, node);
        self.conditional = previous;
    }

    fn visit_generic_param(&mut self, node: &'ast GenericParam) {
        let attributes = match node {
            GenericParam::Lifetime(parameter) => parameter.attrs.as_slice(),
            GenericParam::Type(parameter) => parameter.attrs.as_slice(),
            GenericParam::Const(parameter) => parameter.attrs.as_slice(),
        };
        let previous = self.enter_attributes(attributes);
        visit::visit_generic_param(self, node);
        self.conditional = previous;
    }

    fn visit_impl_item(&mut self, node: &'ast ImplItem) {
        if matches!(node, ImplItem::Verbatim(_)) {
            self.opaque();
            return;
        }
        let previous = self.enter_attributes(impl_item_attributes(node));
        visit::visit_impl_item(self, node);
        self.conditional = previous;
    }

    fn visit_trait_item(&mut self, node: &'ast TraitItem) {
        if matches!(node, TraitItem::Verbatim(_)) {
            self.opaque();
            return;
        }
        let previous = self.enter_attributes(trait_item_attributes(node));
        visit::visit_trait_item(self, node);
        self.conditional = previous;
    }

    fn visit_foreign_item(&mut self, node: &'ast ForeignItem) {
        if matches!(node, ForeignItem::Verbatim(_)) {
            self.opaque();
            return;
        }
        let previous = self.enter_attributes(foreign_item_attributes(node));
        visit::visit_foreign_item(self, node);
        self.conditional = previous;
    }

    fn visit_stmt(&mut self, node: &'ast Stmt) {
        let attributes = match node {
            Stmt::Local(local) => local.attrs.as_slice(),
            Stmt::Macro(mac) => mac.attrs.as_slice(),
            Stmt::Item(_) | Stmt::Expr(_, _) => &[],
        };
        let previous = self.enter_attributes(attributes);
        visit::visit_stmt(self, node);
        self.conditional = previous;
    }

    fn visit_arm(&mut self, node: &'ast syn::Arm) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_arm(self, node);
        self.conditional = previous;
    }

    fn visit_field(&mut self, node: &'ast syn::Field) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_field(self, node);
        self.conditional = previous;
    }

    fn visit_field_value(&mut self, node: &'ast syn::FieldValue) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_field_value(self, node);
        self.conditional = previous;
    }

    fn visit_variant(&mut self, node: &'ast syn::Variant) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_variant(self, node);
        self.conditional = previous;
    }

    fn visit_pat(&mut self, node: &'ast Pat) {
        if matches!(node, Pat::Verbatim(_)) {
            self.opaque();
            return;
        }
        let previous = self.enter_attributes(pat_attributes(node));
        visit::visit_pat(self, node);
        self.conditional = previous;
    }

    fn visit_field_pat(&mut self, node: &'ast syn::FieldPat) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_field_pat(self, node);
        self.conditional = previous;
    }

    fn visit_receiver(&mut self, node: &'ast syn::Receiver) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_receiver(self, node);
        self.conditional = previous;
    }

    fn visit_fn_arg(&mut self, node: &'ast syn::FnArg) {
        let attributes = match node {
            syn::FnArg::Receiver(receiver) => receiver.attrs.as_slice(),
            syn::FnArg::Typed(pattern) => pattern.attrs.as_slice(),
        };
        let previous = self.enter_attributes(attributes);
        visit::visit_fn_arg(self, node);
        self.conditional = previous;
    }

    fn visit_variadic(&mut self, node: &'ast syn::Variadic) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_variadic(self, node);
        self.conditional = previous;
    }

    fn visit_named_arg(&mut self, node: &'ast syn::NamedArg) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_named_arg(self, node);
        self.conditional = previous;
    }

    fn visit_fn_ptr_variadic(&mut self, node: &'ast syn::FnPtrVariadic) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_fn_ptr_variadic(self, node);
        self.conditional = previous;
    }

    fn visit_type_param_bound(&mut self, node: &'ast TypeParamBound) {
        if matches!(node, TypeParamBound::Verbatim(_)) {
            self.opaque();
            return;
        }
        visit::visit_type_param_bound(self, node);
    }

    fn visit_lit(&mut self, node: &'ast Lit) {
        if matches!(node, Lit::Verbatim(_)) {
            self.opaque();
            return;
        }
        visit::visit_lit(self, node);
    }

    fn visit_attribute(&mut self, node: &'ast Attribute) {
        if node.path().is_ident("unsafe")
            && let Some(segment) = node.path().segments.first()
        {
            self.record(FindingKind::UnsafeAttribute, segment.ident.span(), "unsafe");
        } else if node.path().is_ident("cfg_attr") {
            self.inspect_cfg_attr(&node.meta);
        }
        // Attribute token streams are not expanded or recursively interpreted.
    }

    fn visit_macro(&mut self, _node: &'ast syn::Macro) {
        self.macro_boundaries_omitted = self.macro_boundaries_omitted.saturating_add(1);
    }

    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.record(FindingKind::UnsafeBlock, node.unsafe_token.span, "unsafe");
        visit::visit_expr_unsafe(self, node);
    }

    fn visit_signature(&mut self, node: &'ast syn::Signature) {
        if let Safety::Unsafe(token) = &node.safety {
            self.record(FindingKind::UnsafeFn, token.span, "unsafe");
        }
        if let Some(abi) = &node.abi {
            self.record(FindingKind::ExternFn, abi.extern_token.span, "extern");
        }
        visit::visit_signature(self, node);
    }

    fn visit_type_fn_ptr(&mut self, node: &'ast syn::TypeFnPtr) {
        if let Some(token) = &node.unsafety {
            self.record(FindingKind::UnsafeFn, token.span, "unsafe");
        }
        if let Some(abi) = &node.abi {
            self.record(FindingKind::ExternFn, abi.extern_token.span, "extern");
        }
        visit::visit_type_fn_ptr(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if let Some(token) = &node.unsafety {
            self.record(FindingKind::UnsafeImpl, token.span, "unsafe");
        }
        visit::visit_item_impl(self, node);
    }

    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        if let Some(token) = &node.unsafety {
            self.record(FindingKind::UnsafeExternBlock, token.span, "unsafe");
        }
        self.record(
            FindingKind::ExternBlock,
            node.abi.extern_token.span,
            "extern",
        );
        visit::visit_item_foreign_mod(self, node);
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        self.record(FindingKind::ExternCrate, node.extern_token.span, "extern");
        visit::visit_item_extern_crate(self, node);
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if let Some(token) = &node.unsafety {
            self.record(FindingKind::UnsafeMod, token.span, "unsafe");
        }
        visit::visit_item_mod(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if let Some(token) = &node.unsafety {
            self.record(FindingKind::UnsafeTrait, token.span, "unsafe");
        }
        visit::visit_item_trait(self, node);
    }

    fn visit_foreign_item_static(&mut self, node: &'ast syn::ForeignItemStatic) {
        if let Safety::Unsafe(token) = &node.safety {
            self.record(FindingKind::UnsafeStatic, token.span, "unsafe");
        }
        visit::visit_foreign_item_static(self, node);
    }
}

fn is_cfg_attribute(attribute: &Attribute) -> bool {
    attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
}

fn item_attributes(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(item) => &item.attrs,
        Item::Enum(item) => &item.attrs,
        Item::ExternCrate(item) => &item.attrs,
        Item::Fn(item) => &item.attrs,
        Item::ForeignMod(item) => &item.attrs,
        Item::Impl(item) => &item.attrs,
        Item::Macro(item) => &item.attrs,
        Item::Mod(item) => &item.attrs,
        Item::Static(item) => &item.attrs,
        Item::Struct(item) => &item.attrs,
        Item::Trait(item) => &item.attrs,
        Item::TraitAlias(item) => &item.attrs,
        Item::Type(item) => &item.attrs,
        Item::Union(item) => &item.attrs,
        Item::Use(item) => &item.attrs,
        Item::Verbatim(_) => &[],
        _ => &[],
    }
}

fn pat_attributes(pat: &Pat) -> &[Attribute] {
    match pat {
        Pat::Const(node) => &node.attrs,
        Pat::Guard(node) => &node.attrs,
        Pat::Ident(node) => &node.attrs,
        Pat::Lit(node) => &node.attrs,
        Pat::Macro(node) => &node.attrs,
        Pat::Or(node) => &node.attrs,
        Pat::Paren(node) => &node.attrs,
        Pat::Path(node) => &node.attrs,
        Pat::Range(node) => &node.attrs,
        Pat::Reference(node) => &node.attrs,
        Pat::Rest(node) => &node.attrs,
        Pat::Slice(node) => &node.attrs,
        Pat::Struct(node) => &node.attrs,
        Pat::Tuple(node) => &node.attrs,
        Pat::TupleStruct(node) => &node.attrs,
        Pat::Type(node) => &node.attrs,
        Pat::Wild(node) => &node.attrs,
        Pat::Verbatim(_) => &[],
        _ => &[],
    }
}

fn expr_attributes(expr: &Expr) -> &[Attribute] {
    match expr {
        Expr::Array(expr) => &expr.attrs,
        Expr::Assign(expr) => &expr.attrs,
        Expr::Async(expr) => &expr.attrs,
        Expr::Await(expr) => &expr.attrs,
        Expr::Binary(expr) => &expr.attrs,
        Expr::Block(expr) => &expr.attrs,
        Expr::Break(expr) => &expr.attrs,
        Expr::Call(expr) => &expr.attrs,
        Expr::Cast(expr) => &expr.attrs,
        Expr::Closure(expr) => &expr.attrs,
        Expr::Const(expr) => &expr.attrs,
        Expr::Continue(expr) => &expr.attrs,
        Expr::Field(expr) => &expr.attrs,
        Expr::ForLoop(expr) => &expr.attrs,
        Expr::Group(expr) => &expr.attrs,
        Expr::If(expr) => &expr.attrs,
        Expr::Index(expr) => &expr.attrs,
        Expr::Infer(expr) => &expr.attrs,
        Expr::Let(expr) => &expr.attrs,
        Expr::Lit(expr) => &expr.attrs,
        Expr::Loop(expr) => &expr.attrs,
        Expr::Macro(expr) => &expr.attrs,
        Expr::Match(expr) => &expr.attrs,
        Expr::MethodCall(expr) => &expr.attrs,
        Expr::Paren(expr) => &expr.attrs,
        Expr::Path(expr) => &expr.attrs,
        Expr::Range(expr) => &expr.attrs,
        Expr::RawAddr(expr) => &expr.attrs,
        Expr::Reference(expr) => &expr.attrs,
        Expr::Repeat(expr) => &expr.attrs,
        Expr::Return(expr) => &expr.attrs,
        Expr::Struct(expr) => &expr.attrs,
        Expr::Try(expr) => &expr.attrs,
        Expr::TryBlock(expr) => &expr.attrs,
        Expr::Tuple(expr) => &expr.attrs,
        Expr::Unary(expr) => &expr.attrs,
        Expr::Unsafe(expr) => &expr.attrs,
        Expr::Verbatim(_) => &[],
        Expr::While(expr) => &expr.attrs,
        Expr::Yield(expr) => &expr.attrs,
        _ => &[],
    }
}

fn impl_item_attributes(item: &ImplItem) -> &[Attribute] {
    match item {
        ImplItem::Const(item) => &item.attrs,
        ImplItem::Fn(item) => &item.attrs,
        ImplItem::Macro(item) => &item.attrs,
        ImplItem::Type(item) => &item.attrs,
        ImplItem::Verbatim(_) => &[],
        _ => &[],
    }
}

fn type_attributes(node: &Type) -> &[Attribute] {
    match node {
        Type::Array(node) => &node.attrs,
        Type::FnPtr(node) => &node.attrs,
        Type::Group(node) => &node.attrs,
        Type::ImplTrait(node) => &node.attrs,
        Type::Infer(node) => &node.attrs,
        Type::Macro(node) => &node.attrs,
        Type::Never(node) => &node.attrs,
        Type::Paren(node) => &node.attrs,
        Type::Path(node) => &node.attrs,
        Type::Ptr(node) => &node.attrs,
        Type::Reference(node) => &node.attrs,
        Type::Slice(node) => &node.attrs,
        Type::TraitObject(node) => &node.attrs,
        Type::Tuple(node) => &node.attrs,
        Type::Verbatim(_) => &[],
        _ => &[],
    }
}

fn trait_item_attributes(item: &TraitItem) -> &[Attribute] {
    match item {
        TraitItem::Const(item) => &item.attrs,
        TraitItem::Fn(item) => &item.attrs,
        TraitItem::Macro(item) => &item.attrs,
        TraitItem::Type(item) => &item.attrs,
        TraitItem::Verbatim(_) => &[],
        _ => &[],
    }
}

fn foreign_item_attributes(item: &ForeignItem) -> &[Attribute] {
    match item {
        ForeignItem::Fn(item) => &item.attrs,
        ForeignItem::Macro(item) => &item.attrs,
        ForeignItem::Static(item) => &item.attrs,
        ForeignItem::Type(item) => &item.attrs,
        ForeignItem::Verbatim(_) => &[],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::{Cursor, Error as IoError};

    fn parsed(source: &str) -> WorkerOutput {
        let output = scan_source(7, source);
        assert_eq!(output.status, FileStatus::Parsed);
        output
    }

    fn manifest(count: u32, budget_ms: u64) -> ScanManifest {
        ScanManifest {
            schema_version: 2,
            budget_ms,
            files: (0..count)
                .map(|index| ManifestFile {
                    index,
                    path: format!("/source/{index}.rs"),
                })
                .collect(),
        }
    }

    fn parsed_empty(index: u32) -> WorkerOutput {
        empty_worker_output(index, FileStatus::Parsed)
    }

    #[test]
    fn ignores_keywords_in_comments_strings_raw_strings_and_macros() {
        let source = r####"
            // unsafe extern
            const A: &str = "unsafe extern";
            const B: &str = r###"unsafe extern"###;
            macro_rules! hidden { () => { unsafe { extern "C" fn concealed() {} } } }
            fn visible() { hidden!(); unsafe {} }
        "####;
        let output = parsed(source);
        assert_eq!(output.total_findings, 1);
        assert_eq!(output.findings[0].kind, FindingKind::UnsafeBlock);
        assert_eq!(output.macro_boundaries_omitted, 2);
    }

    #[test]
    fn reports_v2_syntax_classes_and_nested_cfg_attr() {
        let source = r#"
            extern crate core;
            unsafe trait Contract {}
            unsafe impl Contract for Thing {}
            unsafe extern "C" {
                unsafe static VALUE: u8;
                safe fn okay();
                unsafe fn risky();
            }
            unsafe mod ffi;
            #[cfg_attr(feature = "one", cfg_attr(feature = "two", unsafe(no_mangle)))]
            extern "C" fn attributed() {}
            fn block(#[cfg(feature = "arg")] callback: unsafe extern "C" fn()) {
                unsafe {}
            }
        "#;
        let output = parsed(source);
        let kinds = output
            .findings
            .iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>();
        for expected in [
            FindingKind::ExternCrate,
            FindingKind::UnsafeTrait,
            FindingKind::UnsafeImpl,
            FindingKind::UnsafeExternBlock,
            FindingKind::UnsafeStatic,
            FindingKind::UnsafeFn,
            FindingKind::ExternBlock,
            FindingKind::UnsafeMod,
            FindingKind::UnsafeAttribute,
            FindingKind::ExternFn,
            FindingKind::UnsafeBlock,
        ] {
            assert!(kinds.contains(&expected), "missing {expected:?}");
        }
        let nested = output
            .findings
            .iter()
            .find(|finding| finding.kind == FindingKind::UnsafeAttribute)
            .unwrap_or_else(|| unreachable!());
        assert!(nested.conditional);
        let parameter = output
            .findings
            .iter()
            .find(|finding| {
                finding.kind == FindingKind::UnsafeFn
                    && &source[finding.byte_start..finding.byte_end] == "unsafe"
                    && finding.line > 10
            })
            .unwrap_or_else(|| unreachable!());
        assert!(parameter.conditional);
    }

    #[test]
    fn reports_unicode_crlf_positions_and_caps_findings() {
        let source = "fn café() {\r\n    let naïve = 1; unsafe {}\r\n}\r\n";
        let output = parsed(source);
        let finding = &output.findings[0];
        assert_eq!(&source[finding.byte_start..finding.byte_end], "unsafe");
        assert_eq!(finding.line, 2);
        assert_eq!(finding.column, 20);

        let many = format!("fn f() {{ {} }}", "unsafe {};".repeat(MAX_FINDINGS + 3));
        let output = parsed(&many);
        assert_eq!(output.findings.len(), MAX_FINDINGS);
        assert_eq!(output.total_findings, (MAX_FINDINGS + 3) as u64);
        assert_eq!(output.omitted_findings, 3);
    }

    #[test]
    fn verbatim_is_declared_opaque_and_parse_errors_copy_no_diagnostic() {
        let syntax = syn::File {
            shebang: None,
            frontmatter: None,
            attrs: Vec::new(),
            items: vec![Item::Verbatim("future syntax".parse().unwrap_or_default())],
        };
        let mut scanner = UnsafeScanner::new(0, "future syntax");
        scanner.visit_file(&syntax);
        assert_eq!(scanner.opaque_syntax_omitted, 1);

        let output = scan_source(3, "fn broken(");
        assert_eq!(output.status, FileStatus::ParseError);
        assert!(output.findings.is_empty());
        assert_eq!(output.opaque_syntax_omitted, 0);
    }

    #[test]
    fn manifest_v2_rejects_budget_paths_indices_and_duplicates() {
        assert!(validate_manifest(&manifest(2, 1)).is_ok());
        assert!(validate_manifest(&manifest(2, MAX_BUDGET_MS)).is_ok());
        assert!(validate_manifest(&manifest(1, 0)).is_err());
        assert!(validate_manifest(&manifest(1, MAX_BUDGET_MS + 1)).is_err());

        let mut duplicate = manifest(2, 100);
        duplicate.files[1].path = duplicate.files[0].path.clone();
        assert!(validate_manifest(&duplicate).is_err());
        let mut wrong_index = manifest(2, 100);
        wrong_index.files[1].index = 7;
        assert!(validate_manifest(&wrong_index).is_err());

        for path in [
            "/tmp/lib.rs",
            "/source/../security/scan.json",
            "/rust-mcp-vendor/pkg/./lib.rs",
            "/source/src\\lib.rs",
            "/source/src/lib.txt",
        ] {
            let candidate = ScanManifest {
                schema_version: 2,
                budget_ms: 100,
                files: vec![ManifestFile {
                    index: 0,
                    path: path.to_owned(),
                }],
            };
            assert!(validate_manifest(&candidate).is_err(), "accepted {path}");
        }
    }

    #[test]
    fn worker_statuses_and_decoder_are_closed_and_bounded() {
        assert_eq!(
            worker_output_from_bytes(0, Err(ReadBoundedError::Unavailable)).status,
            FileStatus::Unavailable
        );
        assert_eq!(
            worker_output_from_bytes(0, Err(ReadBoundedError::TooLarge)).status,
            FileStatus::TooLarge
        );
        assert_eq!(
            worker_output_from_bytes(0, Ok(vec![0xff])).status,
            FileStatus::InvalidUtf8
        );

        let valid = parsed_empty(4);
        assert!(valid_worker_output(4, &valid));
        let encoded = serde_json::to_vec(&valid).unwrap_or_default();
        assert_eq!(decode_worker_output(4, &encoded), valid);

        let mut wrong_index = valid.clone();
        wrong_index.file_index = 5;
        assert!(!valid_worker_output(4, &wrong_index));
        let mut inflated = valid.clone();
        inflated.total_findings = MAX_SOURCE_BYTES as u64 + 1;
        inflated.omitted_findings = MAX_SOURCE_BYTES as u64 + 1;
        assert!(!valid_worker_output(4, &inflated));
        let mut worker_only = valid;
        worker_only.status = FileStatus::BudgetExhausted;
        assert!(!valid_worker_output(4, &worker_only));

        let duplicate = br#"{"schema_version":2,"schema_version":2,"file_index":4,"status":"parsed","findings":[],"total_findings":0,"omitted_findings":0,"macro_omitted":0,"opaque_syntax_omitted":0}"#;
        assert_eq!(
            decode_worker_output(4, duplicate).status,
            FileStatus::Crashed
        );
    }

    struct FailsAfterChunk(bool);
    impl Read for FailsAfterChunk {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.0 {
                return Err(IoError::other("synthetic read failure"));
            }
            self.0 = true;
            buffer[..2].copy_from_slice(b"ok");
            Ok(2)
        }
    }

    #[test]
    fn bounded_read_retains_exact_limit_drains_overflow_and_rejects_midstream_error() {
        assert_eq!(
            read_stream_bounded(Cursor::new(vec![1; 32]), 32).unwrap_or_default(),
            vec![1; 32]
        );
        assert!(matches!(
            read_stream_bounded(Cursor::new(vec![1; 33]), 32),
            Err(ReadBoundedError::TooLarge)
        ));
        assert!(matches!(
            read_stream_bounded(FailsAfterChunk(false), 32),
            Err(ReadBoundedError::Unavailable)
        ));

        let path = std::env::temp_dir().join(format!(
            "rust-mcp-unsafe-read-bounded-{}",
            std::process::id()
        ));
        std::fs::write(&path, vec![2; 33]).unwrap_or_else(|_| unreachable!());
        assert!(matches!(
            read_bounded(&path, 32),
            Err(ReadBoundedError::TooLarge)
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn aggregation_preserves_prefix_and_marks_budget_or_drain_remainder() {
        let elapsed = Cell::new(Duration::ZERO);
        let allowances = Cell::new(Vec::new());
        let output = aggregate_manifest(
            &manifest(3, 50),
            || elapsed.get(),
            |index, allowance| {
                let mut recorded = allowances.take();
                recorded.push(allowance);
                allowances.set(recorded);
                elapsed.set(Duration::from_millis(50));
                Supervised {
                    output: parsed_empty(index),
                    drain_confirmed: true,
                }
            },
        );
        assert_eq!(allowances.take(), vec![Duration::from_millis(50)]);
        assert_eq!(output.files[0].status, FileStatus::Parsed);
        assert_eq!(output.files[1].status, FileStatus::BudgetExhausted);
        assert_eq!(output.files[2].status, FileStatus::BudgetExhausted);

        let output = aggregate_manifest(
            &manifest(3, 100),
            || Duration::ZERO,
            |index, _| Supervised {
                output: parsed_empty(index),
                drain_confirmed: false,
            },
        );
        assert_eq!(output.files[0].status, FileStatus::Parsed);
        assert!(
            output.files[1..]
                .iter()
                .all(|file| file.status == FileStatus::BudgetExhausted)
        );
    }

    #[test]
    fn compact_worst_case_summary_stays_within_output_limit() {
        let files = (0..MAX_FILES)
            .map(|index| FileSummary {
                index: index as u32,
                status: FileStatus::Parsed,
                total_findings: MAX_SOURCE_BYTES as u64,
                omitted_findings: MAX_SOURCE_BYTES as u64,
                macro_boundaries_omitted: MAX_SOURCE_BYTES as u64,
                opaque_syntax_omitted: MAX_SOURCE_BYTES as u64,
            })
            .collect();
        let findings = (0..MAX_FINDINGS)
            .map(|_| Finding {
                file_index: (MAX_FILES - 1) as u32,
                kind: FindingKind::UnsafeExternBlock,
                byte_start: MAX_SOURCE_BYTES - 6,
                byte_end: MAX_SOURCE_BYTES,
                line: MAX_SOURCE_BYTES,
                column: MAX_SOURCE_BYTES,
                conditional: true,
            })
            .collect();
        let per_file = MAX_SOURCE_BYTES as u64;
        let output = SupervisorOutput {
            schema_version: 2,
            files,
            findings,
            total_findings: per_file * MAX_FILES as u64,
            omitted_findings: per_file * MAX_FILES as u64,
            cfg_evaluated: false,
            macros_expanded: false,
            generated_sources_scanned: false,
        };
        let bytes = serde_json::to_vec(&output).unwrap_or_default();
        assert!(bytes.len() <= MAX_OUTPUT_BYTES, "{} bytes", bytes.len());
    }

    #[test]
    fn manifest_deserialization_stops_at_entry_limit() {
        let entries = (0..=MAX_FILES)
            .map(|index| format!(r#"{{"index":{index},"path":"/source/{index}.rs"}}"#))
            .collect::<Vec<_>>()
            .join(",");
        let manifest = format!(r#"{{"schema_version":2,"budget_ms":100,"files":[{entries}]}}"#);
        assert!(manifest.len() <= MAX_MANIFEST_BYTES);
        assert!(serde_json::from_str::<ScanManifest>(&manifest).is_err());
    }
}
