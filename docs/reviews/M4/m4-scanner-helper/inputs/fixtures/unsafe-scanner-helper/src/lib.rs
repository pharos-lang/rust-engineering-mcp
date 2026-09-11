use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use proc_macro2::Span;
use serde::de::{SeqAccess, Visitor as SerdeVisitor};
use serde::{Deserialize, Serialize};
use syn::visit::{self, Visit};
use syn::{
    Attribute, Expr, ForeignItem, GenericParam, ImplItem, Item, Safety, Stmt, TraitItem, Type,
};

pub const MANIFEST_PATH: &str = "/security/scan.json";
pub const HELPER_PATH: &str = "/opt/security/bin/rust-mcp-unsafe-helper";
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_OUTPUT_BYTES: usize = 512 * 1024;
pub const MAX_FILES: usize = 4096;
pub const MAX_FINDINGS: usize = 128;
const MAX_CHILD_STDOUT_BYTES: usize = 512 * 1024;
const CHILD_TIMEOUT: Duration = Duration::from_secs(2);
const CHILD_POLL_INTERVAL: Duration = Duration::from_millis(5);
const FIXED_PATH: &str = "/usr/bin:/bin";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScanManifest {
    pub schema_version: u32,
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

#[derive(Clone, Debug, Deserialize, Serialize)]
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct FileSummary {
    pub index: u32,
    pub status: FileStatus,
    pub total_findings: u64,
    pub omitted_findings: u64,
    #[serde(rename = "macro_omitted")]
    pub macro_boundaries_omitted: u64,
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
            schema_version: 1,
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
    if manifest.schema_version != 1 || manifest.files.len() > MAX_FILES {
        return Err(FatalErrorCode::ManifestInvalid);
    }

    for (position, file) in manifest.files.iter().enumerate() {
        let expected_index =
            u32::try_from(position).map_err(|_| FatalErrorCode::ManifestInvalid)?;
        if file.index != expected_index || !valid_source_path(&file.path) {
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

    let bytes = match read_bounded(Path::new(&file.path), MAX_SOURCE_BYTES) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(empty_worker_output(file_index, FileStatus::Unavailable)),
    };
    let source = match std::str::from_utf8(&bytes) {
        Ok(source) => source,
        Err(_) => return Ok(empty_worker_output(file_index, FileStatus::Unavailable)),
    };
    Ok(scan_source(file_index, source))
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
        schema_version: 1,
        file_index,
        status: FileStatus::Parsed,
        findings: scanner.findings,
        total_findings,
        omitted_findings: total_findings.saturating_sub(retained),
        macro_boundaries_omitted: scanner.macro_boundaries_omitted,
    };

    // A recursive hostile AST can also overflow while being dropped. This short-lived
    // file worker deliberately retains it until process termination; the guest bounds
    // the memory and lifetime of the process.
    std::mem::forget(syntax);
    output
}

pub fn run_supervisor(manifest_path: impl AsRef<Path>) -> Result<SupervisorOutput, FatalErrorCode> {
    let manifest = load_manifest(manifest_path)?;
    let mut files = Vec::with_capacity(manifest.files.len());
    let mut retained_findings = Vec::with_capacity(MAX_FINDINGS);
    let mut total_findings = 0_u64;

    for file in &manifest.files {
        let outcome = supervise_file(file.index);
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
        });
    }

    let retained = u64::try_from(retained_findings.len()).unwrap_or(u64::MAX);
    let output = SupervisorOutput {
        schema_version: 1,
        files,
        findings: retained_findings,
        total_findings,
        omitted_findings: total_findings.saturating_sub(retained),
        cfg_evaluated: false,
        macros_expanded: false,
        generated_sources_scanned: false,
    };
    let encoded = serde_json::to_vec(&output).map_err(|_| FatalErrorCode::OutputTooLarge)?;
    if encoded.len() > MAX_OUTPUT_BYTES {
        return Err(FatalErrorCode::OutputTooLarge);
    }
    Ok(output)
}

fn supervise_file(index: u32) -> WorkerOutput {
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
        Err(_) => return empty_worker_output(index, FileStatus::Crashed),
    };

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_and_reap(&mut child);
            return empty_worker_output(index, FileStatus::Crashed);
        }
    };
    let reader = thread::spawn(move || read_stream_bounded(stdout, MAX_CHILD_STDOUT_BYTES));
    let deadline = Instant::now() + CHILD_TIMEOUT;

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(Some(status)),
            Ok(None) if Instant::now() < deadline => thread::sleep(CHILD_POLL_INTERVAL),
            Ok(None) => {
                terminate_and_reap(&mut child);
                break Ok(None);
            }
            Err(_) => {
                terminate_and_reap(&mut child);
                break Err(());
            }
        }
    };
    let captured = reader.join().ok().and_then(Result::ok);

    match (status, captured) {
        (Ok(None), _) => empty_worker_output(index, FileStatus::TimedOut),
        (Ok(Some(status)), Some(bytes)) if status.success() => decode_worker_output(index, &bytes),
        _ => empty_worker_output(index, FileStatus::Crashed),
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
    output.schema_version == 1
        && output.file_index == index
        && matches!(
            output.status,
            FileStatus::Parsed | FileStatus::ParseError | FileStatus::Unavailable
        )
        && output.findings.len() <= MAX_FINDINGS
        && output
            .findings
            .iter()
            .all(|finding| valid_finding(index, finding))
        && output.total_findings
            == u64::try_from(output.findings.len())
                .unwrap_or(u64::MAX)
                .saturating_add(output.omitted_findings)
        && (output.status == FileStatus::Parsed
            || (output.findings.is_empty()
                && output.total_findings == 0
                && output.omitted_findings == 0
                && output.macro_boundaries_omitted == 0))
}

fn valid_finding(index: u32, finding: &Finding) -> bool {
    finding.file_index == index
        && finding.byte_start < finding.byte_end
        && finding.byte_end <= MAX_SOURCE_BYTES
        && finding.line > 0
        && finding.column > 0
}

fn empty_worker_output(file_index: u32, status: FileStatus) -> WorkerOutput {
    WorkerOutput {
        schema_version: 1,
        file_index,
        status,
        findings: Vec::new(),
        total_findings: 0,
        omitted_findings: 0,
        macro_boundaries_omitted: 0,
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
}

impl<'ast> Visit<'ast> for UnsafeScanner<'_> {
    fn visit_file(&mut self, node: &'ast syn::File) {
        let previous = self.enter_attributes(&node.attrs);
        visit::visit_file(self, node);
        self.conditional = previous;
    }

    fn visit_item(&mut self, node: &'ast Item) {
        let previous = self.enter_attributes(item_attributes(node));
        visit::visit_item(self, node);
        self.conditional = previous;
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        let previous = self.enter_attributes(expr_attributes(node));
        visit::visit_expr(self, node);
        self.conditional = previous;
    }

    fn visit_type(&mut self, node: &'ast Type) {
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
        let previous = self.enter_attributes(impl_item_attributes(node));
        visit::visit_impl_item(self, node);
        self.conditional = previous;
    }

    fn visit_trait_item(&mut self, node: &'ast TraitItem) {
        let previous = self.enter_attributes(trait_item_attributes(node));
        visit::visit_trait_item(self, node);
        self.conditional = previous;
    }

    fn visit_foreign_item(&mut self, node: &'ast ForeignItem) {
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

    fn visit_attribute(&mut self, node: &'ast Attribute) {
        if node.path().is_ident("unsafe")
            && let Some(segment) = node.path().segments.first()
        {
            self.record(FindingKind::UnsafeAttribute, segment.ident.span(), "unsafe");
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

    fn parsed(source: &str) -> WorkerOutput {
        let output = scan_source(7, source);
        assert_eq!(output.status, FileStatus::Parsed);
        output
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
    fn reports_all_accepted_syntax_classes() {
        let source = r#"
            extern crate core;
            unsafe fn free() {}
            extern "C" fn exported() {}
            type Callback = unsafe extern "C" fn();
            unsafe impl Send for Thing {}
            unsafe extern "C" { safe fn okay(); unsafe fn risky(); }
            unsafe mod ffi;
            #[unsafe(no_mangle)]
            extern "C" fn attributed() {}
            fn block() { unsafe {} }
        "#;
        let output = parsed(source);
        let kinds = output
            .findings
            .iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>();
        assert!(kinds.contains(&FindingKind::ExternCrate));
        assert!(kinds.contains(&FindingKind::UnsafeFn));
        assert!(kinds.contains(&FindingKind::ExternFn));
        assert!(kinds.contains(&FindingKind::UnsafeImpl));
        assert!(kinds.contains(&FindingKind::UnsafeExternBlock));
        assert!(kinds.contains(&FindingKind::ExternBlock));
        assert!(kinds.contains(&FindingKind::UnsafeMod));
        assert!(kinds.contains(&FindingKind::UnsafeAttribute));
        assert!(kinds.contains(&FindingKind::UnsafeBlock));

        let unsafe_functions = kinds
            .iter()
            .filter(|kind| **kind == FindingKind::UnsafeFn)
            .count();
        assert_eq!(unsafe_functions, 3);
    }

    #[test]
    fn propagates_cfg_and_cfg_attr_without_evaluating_them() {
        let source = r#"
            #![cfg(feature = "crate_condition")]
            #[cfg(feature = "one")]
            mod conditional_module { fn f() { unsafe {} } }

            fn expression() {
                #[cfg(target_os = "none")]
                if true { unsafe {} }
            }

            #[cfg_attr(feature = "two", unsafe(no_mangle))]
            extern "C" fn conditional_attribute() {}
        "#;
        let output = parsed(source);
        assert_eq!(output.total_findings, 3);
        assert!(output.findings.iter().all(|finding| finding.conditional));
    }

    #[test]
    fn reports_unicode_and_crlf_positions_from_keyword_span() {
        let source = "fn café() {\r\n    let naïve = 1; unsafe {}\r\n}\r\n";
        let output = parsed(source);
        let finding = &output.findings[0];
        assert_eq!(&source[finding.byte_start..finding.byte_end], "unsafe");
        assert_eq!(
            finding.byte_start,
            source.find("unsafe").unwrap_or(usize::MAX)
        );
        assert_eq!(finding.line, 2);
        assert_eq!(finding.column, 20);
    }

    #[test]
    fn caps_findings_and_preserves_total_and_omitted_counts() {
        let source = format!("fn f() {{ {} }}", "unsafe {};".repeat(MAX_FINDINGS + 3));
        let output = parsed(&source);
        assert_eq!(output.findings.len(), MAX_FINDINGS);
        assert_eq!(output.total_findings, (MAX_FINDINGS + 3) as u64);
        assert_eq!(output.omitted_findings, 3);
    }

    #[test]
    fn returns_parse_error_without_copying_parser_diagnostics() {
        let output = scan_source(3, "fn broken(");
        assert_eq!(output.status, FileStatus::ParseError);
        assert!(output.findings.is_empty());
    }

    #[test]
    fn rejects_noncontiguous_indices_and_untrusted_paths() {
        let valid = ScanManifest {
            schema_version: 1,
            files: vec![ManifestFile {
                index: 0,
                path: "/source/src/lib.rs".to_owned(),
            }],
        };
        assert!(validate_manifest(&valid).is_ok());

        for path in [
            "/tmp/lib.rs",
            "/source/../security/scan.json",
            "/rust-mcp-vendor/pkg/./lib.rs",
            "/source/src\\lib.rs",
            "/source/src/lib.txt",
        ] {
            let manifest = ScanManifest {
                schema_version: 1,
                files: vec![ManifestFile {
                    index: 0,
                    path: path.to_owned(),
                }],
            };
            assert!(validate_manifest(&manifest).is_err(), "accepted {path}");
        }
    }

    #[test]
    fn serialized_worst_case_summary_stays_within_protocol_limit() {
        let files = (0..MAX_FILES)
            .map(|index| FileSummary {
                index: index as u32,
                status: FileStatus::Unavailable,
                total_findings: MAX_SOURCE_BYTES as u64,
                omitted_findings: MAX_SOURCE_BYTES as u64,
                macro_boundaries_omitted: MAX_SOURCE_BYTES as u64,
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
        let output = SupervisorOutput {
            schema_version: 1,
            files,
            findings,
            total_findings: u64::MAX,
            omitted_findings: u64::MAX,
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
        let manifest = format!(r#"{{"schema_version":1,"files":[{entries}]}}"#);
        assert!(manifest.len() <= MAX_MANIFEST_BYTES);
        assert!(serde_json::from_str::<ScanManifest>(&manifest).is_err());
    }
}
