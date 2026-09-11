//! ADR-078: a large, immutable, digest-addressed offline vendor capture.
//!
//! This is deliberately **not** [`crate::SourceBundle`] and deliberately does
//! not reuse its constants. `SourceBundle` is the qualified M2/M4 offline-data
//! contract — 4 096 entries, 16 MiB, 1 MiB per file, 100 bytes per path and a
//! closed `[A-Za-z0-9._/-]` alphabet — and every flow that carries host data to
//! the guest shares it. The criterion 0.8.2 closure breaks four of those bounds
//! at once, and thirteen of its paths break none of them because they contain
//! parentheses. No quota moves those thirteen, so this contract has to decide
//! on the path alphabet as well as on the counts and the sizes.
//!
//! What this module enforces:
//!
//! * every limit in the ADR-078 table, applied **during** the read rather than
//!   after it: an entry that would cross the per-file or the total ceiling is
//!   refused at the point that crosses it;
//! * the widened alphabet, and only as wide as the ADR says;
//! * one canonical order, so the same tree always produces the same digest and
//!   a directory always precedes its own subtree;
//! * an incremental digest on both the capture and the verify path. Neither
//!   [`VendorCaptureBuilder`] nor [`VendorCaptureVerifier`] holds the tree or
//!   the artifact: their whole state is one open entry, one previous path and a
//!   directory stack bounded by the depth limit.
//!
//! The hash function is not here. `domain` owns the framing fed to it — the
//! tags, the length prefixes and the trailer — and the adapter that already
//! depends on SHA-256 owns the algorithm, through [`CaptureHasher`].
use crate::SourceFingerprint;
use std::cmp::Ordering;

// -- the ADR-078 limits table -------------------------------------------------
//
// Every value below is fixed by ADR-078, "Los límites, ahora que las mediciones
// existen (2026-09-09)". None of them is derived from the criterion closure
// fitting: that closure would also fit with half the entries and a third of the
// total bytes. Do not widen, narrow, or add one here.

/// **Measured.** The read buffer is what fixes the peak resident size: 2,7 MB
/// over the interpreter floor at 64 KiB against 4,4 MB at 1 MiB, for no time
/// (0,3706 s against 0,3792 s, inside the noise).
pub const VENDOR_CAPTURE_READ_BUFFER_BYTES: usize = 64 * 1024;
/// **Policy, informed by measurement.** 156 MB is a 1,0 s cycle; 512 MiB
/// projects to ~3,5 s of wall clock and ~1,1 GB of host disk between the
/// capture and the ingested tree, which is still an interactive operation.
pub const VENDOR_CAPTURE_MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// **Policy.** The measured closure uses 6 793. The slack is deliberate and is
/// not justified by the measurement: it is justified by not wanting to come
/// back to this decision with the next harness.
pub const VENDOR_CAPTURE_MAX_ENTRIES: usize = 32_768;
/// **Argued, not chosen.** 96,5 % of the closure's bytes sit in files ≤ 1 MiB
/// and the largest is 1 670 630 B. A median-derived limit (5 344 B) and a
/// maximum-derived one differ by three orders of magnitude, so neither serves.
/// 8 MiB is ~5× the largest observed file.
pub const VENDOR_CAPTURE_MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// **Measured.** Twice the observed maximum of 112 bytes; the p99 is 104.
pub const VENDOR_CAPTURE_MAX_PATH_BYTES: usize = 200;
/// **Measured.** The observed maximum is 9.
pub const VENDOR_CAPTURE_MAX_DEPTH: usize = 16;

/// The bytes a capture path may contain **beyond** `SourceBundle`'s
/// `[A-Za-z0-9._/-]`: what a `.crate` published on crates.io can legitimately
/// carry in the file names of its tests.
///
/// The list is short on purpose, and the ADR's own sentence is the test this
/// module owes: *what is not admitted is still what matters*. Nothing here is a
/// control byte, a non-ASCII byte, a `\`, a `:`, a quote, a shell
/// metacharacter or a wildcard, so a path still cannot express something the
/// guest reads as something else.
const EXTRA_PATH_BYTES: &[u8] = b"()+,=@[]{}~ ";

/// Every reason a capture, or an artifact offered as one, is refused. Closed: a
/// new refusal is a contract change, never an opaque string.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VendorCaptureError {
    /// The grammar: the alphabet, an empty component, `.`, `..`, an absolute
    /// path, a duplicate, an entry out of canonical order, an entry whose
    /// parent directory was never declared, or malformed artifact framing.
    Invalid,
    /// One of the ADR-078 quotas, refused at the point of the read that crosses
    /// it and never after the fact.
    Limits,
    /// An entry that is neither a regular file nor a directory: a symlink, a
    /// hard link, a device, a socket or a fifo. ADR-078 §6 makes this a
    /// refusal, never a skip.
    Link,
    /// The tree, or the artifact, changed under the reader. A capture of a tree
    /// that moved is not published.
    Mutated,
    /// The recomputed digest is not the declared one.
    Digest,
}

impl std::fmt::Display for VendorCaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "vendor capture path or framing is outside the contract",
            Self::Limits => "vendor capture exceeds an ADR-078 quota",
            Self::Link => "vendor capture entry is not a regular file or directory",
            Self::Mutated => "vendor tree or artifact changed during the read",
            Self::Digest => "vendor capture digest is not the declared one",
        })
    }
}
impl std::error::Error for VendorCaptureError {}

/// Validate one relative capture path against ADR-078's grammar and bounds.
///
/// `SourceBundle`'s [`crate::validate_source_path`] is untouched: the qualified
/// M2/M4 flows keep their closed alphabet. This one admits `[A-Za-z0-9._/-]`
/// plus `()+,=@[]{}~` and space, and nothing else.
pub fn validate_capture_path(path: &str) -> Result<(), VendorCaptureError> {
    let admitted = |byte: &u8| {
        byte.is_ascii_alphanumeric() || b"._/-".contains(byte) || EXTRA_PATH_BYTES.contains(byte)
    };
    if path.is_empty()
        || !path.bytes().all(|byte| admitted(&byte))
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(VendorCaptureError::Invalid);
    }
    if path.len() > VENDOR_CAPTURE_MAX_PATH_BYTES
        || path.split('/').count() > VENDOR_CAPTURE_MAX_DEPTH
    {
        return Err(VendorCaptureError::Limits);
    }
    Ok(())
}

/// The canonical order of a capture: component-wise, not byte-wise.
///
/// Byte order would put `a-b` between `a` and `a/b`, because `-` sorts below
/// `/`, and a reader could then no longer keep a directory on a stack while its
/// subtree arrives. Component-wise order is what a depth-first walk that sorts
/// each directory's names already produces, and under it a directory is always
/// immediately followed by its own subtree.
fn canonical_order(left: &str, right: &str) -> Ordering {
    left.split('/').cmp(right.split('/'))
}

fn parent_of(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(parent, _)| parent)
}

fn is_directory_prefix(directory: &str, path: &str) -> bool {
    path.len() > directory.len()
        && path.as_bytes().get(directory.len()) == Some(&b'/')
        && path.starts_with(directory)
}

/// The hash a capture is addressed by.
///
/// `domain` feeds it a framing it owns entirely and never chooses the
/// algorithm; the adapter that already depends on SHA-256 provides it. Two
/// independent hashers are used per capture: one over the tree's canonical
/// framing and one over the artifact's exact bytes.
pub trait CaptureHasher {
    fn update(&mut self, bytes: &[u8]);
    /// `sha256:<64 lowercase hex digits>`.
    fn finish(self) -> Result<SourceFingerprint, VendorCaptureError>;
}

/// Domain separation for the tree digest. A digest of this contract's framing
/// must never collide with a digest of `SourceBundle`'s.
const TREE_DOMAIN: &[u8] = b"rust-engineering-mcp/vendor-capture/v1\0";

const BLOCK: usize = 512;
const USTAR_NAME: usize = 100;
const USTAR_PREFIX: usize = 155;
const DIRECTORY_MODE: u64 = 0o755;
const FILE_MODE: u64 = 0o444;

/// Up to three 512-byte blocks: a pax extended header, its single record, and
/// the entry header itself. Returned by value, so no allocation happens per
/// entry on the capture path.
#[derive(Clone, Copy)]
pub struct CaptureBlocks {
    bytes: [u8; 3 * BLOCK],
    len: usize,
}
impl CaptureBlocks {
    fn empty() -> Self {
        Self {
            bytes: [0; 3 * BLOCK],
            len: 0,
        }
    }
    fn push(&mut self, block: [u8; BLOCK]) {
        if self.len + BLOCK <= self.bytes.len() {
            self.bytes[self.len..self.len + BLOCK].copy_from_slice(&block);
            self.len += BLOCK;
        }
    }
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

fn octal(field: &mut [u8], value: u64) -> Result<(), VendorCaptureError> {
    let width = field
        .len()
        .checked_sub(1)
        .ok_or(VendorCaptureError::Invalid)?;
    let mut digits = [b'0'; 12];
    if width > digits.len() {
        return Err(VendorCaptureError::Invalid);
    }
    let mut remaining = value;
    for slot in digits[..width].iter_mut().rev() {
        *slot = b'0' + u8::try_from(remaining & 7).unwrap_or(0);
        remaining >>= 3;
    }
    if remaining != 0 {
        return Err(VendorCaptureError::Limits);
    }
    field[..width].copy_from_slice(&digits[..width]);
    field[width] = 0;
    Ok(())
}

/// The USTAR header checksum: every byte, with the checksum field itself read
/// as eight spaces.
fn header_checksum(header: &[u8; BLOCK]) -> u64 {
    header
        .iter()
        .enumerate()
        .map(|(index, byte)| {
            if (148..156).contains(&index) {
                32
            } else {
                u64::from(*byte)
            }
        })
        .sum()
}

fn ustar_header(
    name: &str,
    prefix: &str,
    size: u64,
    kind: u8,
    mode: u64,
) -> Result<[u8; BLOCK], VendorCaptureError> {
    if name.is_empty() || name.len() > USTAR_NAME || prefix.len() > USTAR_PREFIX {
        return Err(VendorCaptureError::Invalid);
    }
    let mut header = [0u8; BLOCK];
    header[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut header[100..108], mode)?;
    octal(&mut header[108..116], 0)?;
    octal(&mut header[116..124], 0)?;
    octal(&mut header[124..136], size)?;
    octal(&mut header[136..148], 0)?;
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    header[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
    let sum = header_checksum(&header);
    octal(&mut header[148..155], sum)?;
    header[155] = b' ';
    Ok(header)
}

/// The `prefix`/`name` split USTAR admits, or `None` when the path needs a pax
/// extended header because its own last component exceeds 100 bytes.
///
/// A 150-byte file name is inside ADR-078's 200-byte path limit, so refusing it
/// here would be adding a limit the ADR does not name. The pax record carries
/// it instead.
fn split_name(path: &str) -> Option<(&str, &str)> {
    if path.len() <= USTAR_NAME {
        return Some(("", path));
    }
    for (index, _) in path.match_indices('/') {
        let name = path.len() - index - 1;
        if name > 0 && name <= USTAR_NAME && index <= USTAR_PREFIX {
            return Some((&path[..index], &path[index + 1..]));
        }
    }
    None
}

/// The deterministic stand-in name a pax-framed entry carries in its own USTAR
/// header. It is intentionally *not* a truncation of the real path: an
/// extractor that ignored the pax record would then produce an obviously wrong
/// tree that fails the build loudly, rather than a plausible one that fails
/// quietly.
fn pax_placeholder(ordinal: usize) -> String {
    format!("PaxLongPath/{ordinal}")
}

/// One pax `path=` record, self-sized exactly as POSIX.1-2001 requires: the
/// decimal length includes the digits that state it.
fn pax_record(path: &str) -> Result<([u8; BLOCK], usize), VendorCaptureError> {
    let payload = " path=\n".len() + path.len();
    let mut length = payload + 1;
    for _ in 0..4 {
        let candidate = payload + length.to_string().len();
        if candidate == length {
            break;
        }
        length = candidate;
    }
    let text = format!("{length} path={path}\n");
    if text.len() != length || length > BLOCK {
        return Err(VendorCaptureError::Invalid);
    }
    let mut block = [0u8; BLOCK];
    block[..text.len()].copy_from_slice(text.as_bytes());
    Ok((block, text.len()))
}

/// The immutable identity of one capture.
///
/// It is a value, not a handle: it carries no path and no authority, and it has
/// no constructor other than a completed capture or a completed verification.
/// The tree digest is the identity ADR-078 §2 addresses the capture by, and the
/// one that travels in the provenance of any measurement that used it; the
/// artifact digest is the digest of the bytes at rest, so a store can also
/// detect a rewritten artifact that still decodes to the same tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VendorCapture {
    tree_digest: SourceFingerprint,
    artifact_digest: SourceFingerprint,
    artifact_bytes: u64,
    entries: usize,
    files: usize,
    directories: usize,
    total_bytes: u64,
}
impl VendorCapture {
    /// The identity. This is what a measurement's provenance carries.
    pub fn tree_digest(&self) -> &SourceFingerprint {
        &self.tree_digest
    }
    pub fn artifact_digest(&self) -> &SourceFingerprint {
        &self.artifact_digest
    }
    pub fn artifact_bytes(&self) -> u64 {
        self.artifact_bytes
    }
    pub fn entries(&self) -> usize {
        self.entries
    }
    pub fn files(&self) -> usize {
        self.files
    }
    pub fn directories(&self) -> usize {
        self.directories
    }
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
    /// ADR-078 §2: a capture whose digest is not the declared one is refused,
    /// never used.
    pub fn matches(&self, declared: &SourceFingerprint) -> bool {
        &self.tree_digest == declared
    }
}

/// The shared bookkeeping of the two paths: the grammar, the canonical order,
/// the topology and the quotas. Its memory is one path plus a directory stack,
/// both bounded by the depth and path limits.
struct Counters {
    stack: Vec<String>,
    previous: Option<String>,
    entries: usize,
    files: usize,
    directories: usize,
    total: u64,
}
impl Counters {
    fn new() -> Self {
        Self {
            stack: Vec::new(),
            previous: None,
            entries: 0,
            files: 0,
            directories: 0,
            total: 0,
        }
    }

    /// Grammar, order, topology and the entry quota — all of it before a single
    /// content byte is read.
    fn admit(&mut self, path: &str) -> Result<(), VendorCaptureError> {
        validate_capture_path(path)?;
        if let Some(previous) = &self.previous
            && canonical_order(previous, path) != Ordering::Less
        {
            return Err(VendorCaptureError::Invalid);
        }
        while self
            .stack
            .last()
            .is_some_and(|open| !is_directory_prefix(open, path))
        {
            self.stack.pop();
        }
        if self.stack.last().map(String::as_str) != parent_of(path) {
            return Err(VendorCaptureError::Invalid);
        }
        self.entries += 1;
        if self.entries > VENDOR_CAPTURE_MAX_ENTRIES {
            return Err(VendorCaptureError::Limits);
        }
        self.previous = Some(path.to_owned());
        Ok(())
    }

    fn opened_directory(&mut self, path: &str) {
        self.directories += 1;
        self.stack.push(path.to_owned());
    }

    /// The per-file and total quotas, charged before the first byte is read, so
    /// an oversize entry is never opened for reading at all.
    fn reserve(&mut self, declared: u64) -> Result<(), VendorCaptureError> {
        if declared > VENDOR_CAPTURE_MAX_FILE_BYTES {
            return Err(VendorCaptureError::Limits);
        }
        let total = self
            .total
            .checked_add(declared)
            .ok_or(VendorCaptureError::Limits)?;
        if total > VENDOR_CAPTURE_MAX_TOTAL_BYTES {
            return Err(VendorCaptureError::Limits);
        }
        self.files += 1;
        Ok(())
    }
}

struct OpenFile {
    declared: u64,
    written: u64,
}

/// Builds one capture incrementally.
///
/// The caller drives it with what it reads from the tree, and writes the bytes
/// this type hands back, in order:
///
/// ```text
/// directory(path)                -> write the returned blocks
/// begin_file(path, size)         -> write the returned blocks
///   chunk(bytes) .. chunk(bytes) -> write each `bytes` verbatim
/// end_file()                     -> write the returned padding
/// finish()                       -> write the returned trailer
/// ```
///
/// `chunk` is the only method that sees content, and it never keeps any.
pub struct VendorCaptureBuilder<H: CaptureHasher> {
    tree: H,
    artifact: H,
    counters: Counters,
    open: Option<OpenFile>,
    artifact_bytes: u64,
}

impl<H: CaptureHasher> VendorCaptureBuilder<H> {
    pub fn new(mut tree: H, artifact: H) -> Self {
        tree.update(TREE_DOMAIN);
        Self {
            tree,
            artifact,
            counters: Counters::new(),
            open: None,
            artifact_bytes: 0,
        }
    }

    fn emit(&mut self, blocks: CaptureBlocks) -> CaptureBlocks {
        self.artifact.update(blocks.as_slice());
        self.artifact_bytes += blocks.len as u64;
        blocks
    }

    fn header(
        &mut self,
        path: &str,
        size: u64,
        kind: u8,
        mode: u64,
    ) -> Result<CaptureBlocks, VendorCaptureError> {
        let mut blocks = CaptureBlocks::empty();
        match split_name(path) {
            Some((prefix, name)) => blocks.push(ustar_header(name, prefix, size, kind, mode)?),
            None => {
                let (record, length) = pax_record(path)?;
                let label = pax_placeholder(self.counters.entries);
                blocks.push(ustar_header(&label, "", length as u64, b'x', FILE_MODE)?);
                blocks.push(record);
                blocks.push(ustar_header(&label, "", size, kind, mode)?);
            }
        }
        Ok(blocks)
    }

    /// Declare one directory. Directories are entries and are charged against
    /// the entry quota exactly as files are.
    pub fn directory(&mut self, path: &str) -> Result<CaptureBlocks, VendorCaptureError> {
        if self.open.is_some() {
            return Err(VendorCaptureError::Invalid);
        }
        self.counters.admit(path)?;
        let blocks = self.header(path, 0, b'5', DIRECTORY_MODE)?;
        self.counters.opened_directory(path);
        self.tree.update(b"d");
        self.tree.update(&(path.len() as u64).to_le_bytes());
        self.tree.update(path.as_bytes());
        Ok(self.emit(blocks))
    }

    /// Open one regular file of exactly `size` bytes. The quota is charged
    /// here, so a file over the ceiling is refused before it is read.
    pub fn begin_file(
        &mut self,
        path: &str,
        size: u64,
    ) -> Result<CaptureBlocks, VendorCaptureError> {
        if self.open.is_some() {
            return Err(VendorCaptureError::Invalid);
        }
        self.counters.admit(path)?;
        self.counters.reserve(size)?;
        let blocks = self.header(path, size, b'0', FILE_MODE)?;
        self.tree.update(b"f");
        self.tree.update(&(path.len() as u64).to_le_bytes());
        self.tree.update(path.as_bytes());
        self.tree.update(&size.to_le_bytes());
        self.open = Some(OpenFile {
            declared: size,
            written: 0,
        });
        Ok(self.emit(blocks))
    }

    /// Feed one read buffer's worth of the open file.
    ///
    /// A file that grew past what it declared is refused here, at the chunk
    /// that crosses it: the quota is not re-checked at the end, and no oversize
    /// content is ever accumulated.
    pub fn chunk(&mut self, bytes: &[u8]) -> Result<(), VendorCaptureError> {
        let open = self.open.as_mut().ok_or(VendorCaptureError::Invalid)?;
        let written = open
            .written
            .checked_add(bytes.len() as u64)
            .ok_or(VendorCaptureError::Limits)?;
        if written > open.declared {
            return Err(VendorCaptureError::Mutated);
        }
        open.written = written;
        self.tree.update(bytes);
        self.artifact.update(bytes);
        self.artifact_bytes += bytes.len() as u64;
        Ok(())
    }

    /// Close the open file. A file that ended short of what it declared moved
    /// under the reader and is refused.
    pub fn end_file(&mut self) -> Result<CaptureBlocks, VendorCaptureError> {
        let open = self.open.take().ok_or(VendorCaptureError::Invalid)?;
        if open.written != open.declared {
            return Err(VendorCaptureError::Mutated);
        }
        self.counters.total += open.declared;
        let padding = (BLOCK - (open.declared as usize % BLOCK)) % BLOCK;
        let mut blocks = CaptureBlocks::empty();
        blocks.len = padding;
        Ok(self.emit(blocks))
    }

    /// Close the capture and produce its identity, plus the artifact trailer.
    pub fn finish(mut self) -> Result<(VendorCapture, CaptureBlocks), VendorCaptureError> {
        if self.open.is_some() {
            return Err(VendorCaptureError::Invalid);
        }
        let mut trailer = CaptureBlocks::empty();
        trailer.push([0u8; BLOCK]);
        trailer.push([0u8; BLOCK]);
        let trailer = self.emit(trailer);
        self.tree.update(b"e");
        self.tree
            .update(&(self.counters.entries as u64).to_le_bytes());
        self.tree.update(&self.counters.total.to_le_bytes());
        Ok((
            VendorCapture {
                tree_digest: self.tree.finish()?,
                artifact_digest: self.artifact.finish()?,
                artifact_bytes: self.artifact_bytes,
                entries: self.counters.entries,
                files: self.counters.files,
                directories: self.counters.directories,
                total_bytes: self.counters.total,
            },
            trailer,
        ))
    }
}

enum Mode {
    Block,
    Record { size: usize, padding: usize },
    Content { remaining: u64, padding: usize },
    Padding { remaining: usize },
    Trailer { blocks: usize },
}

/// Re-derives a capture's identity from its artifact, one buffer at a time.
///
/// It is the same contract read backwards, and deliberately not a weaker one:
/// the alphabet, the canonical order, the topology and every quota are applied
/// again as the bytes arrive, so an artifact cannot smuggle in a path or an
/// entry the capture path would have refused. Content is hashed straight out of
/// the caller's buffer and never retained.
pub struct VendorCaptureVerifier<H: CaptureHasher> {
    tree: H,
    artifact: H,
    counters: Counters,
    mode: Mode,
    block: [u8; BLOCK],
    filled: usize,
    pending_path: Option<String>,
    artifact_bytes: u64,
}

impl<H: CaptureHasher> VendorCaptureVerifier<H> {
    pub fn new(mut tree: H, artifact: H) -> Self {
        tree.update(TREE_DOMAIN);
        Self {
            tree,
            artifact,
            counters: Counters::new(),
            mode: Mode::Block,
            block: [0; BLOCK],
            filled: 0,
            pending_path: None,
            artifact_bytes: 0,
        }
    }

    pub fn chunk(&mut self, mut bytes: &[u8]) -> Result<(), VendorCaptureError> {
        self.artifact.update(bytes);
        self.artifact_bytes += bytes.len() as u64;
        while !bytes.is_empty() {
            match self.mode {
                Mode::Content { remaining, padding } => {
                    let take = usize::try_from(remaining.min(bytes.len() as u64))
                        .map_err(|_| VendorCaptureError::Invalid)?;
                    self.tree.update(&bytes[..take]);
                    bytes = &bytes[take..];
                    let left = remaining - take as u64;
                    self.mode = if left > 0 {
                        Mode::Content {
                            remaining: left,
                            padding,
                        }
                    } else if padding > 0 {
                        Mode::Padding { remaining: padding }
                    } else {
                        Mode::Block
                    };
                }
                Mode::Padding { remaining } => {
                    let take = remaining.min(bytes.len());
                    if bytes[..take].iter().any(|byte| *byte != 0) {
                        return Err(VendorCaptureError::Invalid);
                    }
                    bytes = &bytes[take..];
                    self.mode = if remaining > take {
                        Mode::Padding {
                            remaining: remaining - take,
                        }
                    } else {
                        Mode::Block
                    };
                }
                Mode::Record { size, padding } => {
                    let take = (size - self.filled).min(bytes.len());
                    self.block[self.filled..self.filled + take].copy_from_slice(&bytes[..take]);
                    self.filled += take;
                    bytes = &bytes[take..];
                    if self.filled == size {
                        let path = decode_record(&self.block[..size])?;
                        self.filled = 0;
                        self.pending_path = Some(path);
                        self.mode = if padding > 0 {
                            Mode::Padding { remaining: padding }
                        } else {
                            Mode::Block
                        };
                    }
                }
                Mode::Trailer { blocks } => {
                    // Nothing may follow the two zero blocks: an artifact with
                    // an appended tail is a different artifact wearing this
                    // one's framing.
                    if blocks >= 2 {
                        return Err(VendorCaptureError::Invalid);
                    }
                    let take = (BLOCK - self.filled).min(bytes.len());
                    if bytes[..take].iter().any(|byte| *byte != 0) {
                        return Err(VendorCaptureError::Invalid);
                    }
                    self.filled += take;
                    bytes = &bytes[take..];
                    if self.filled == BLOCK {
                        self.filled = 0;
                        self.mode = Mode::Trailer { blocks: blocks + 1 };
                    }
                }
                Mode::Block => {
                    let take = (BLOCK - self.filled).min(bytes.len());
                    self.block[self.filled..self.filled + take].copy_from_slice(&bytes[..take]);
                    self.filled += take;
                    bytes = &bytes[take..];
                    if self.filled == BLOCK {
                        self.filled = 0;
                        self.header()?;
                    }
                }
            }
        }
        Ok(())
    }

    fn header(&mut self) -> Result<(), VendorCaptureError> {
        if self.block.iter().all(|byte| *byte == 0) {
            if self.pending_path.is_some() {
                return Err(VendorCaptureError::Invalid);
            }
            self.mode = Mode::Trailer { blocks: 1 };
            return Ok(());
        }
        if &self.block[257..263] != b"ustar\0" || &self.block[263..265] != b"00" {
            return Err(VendorCaptureError::Invalid);
        }
        if field_octal(&self.block[148..156])? != header_checksum(&self.block) {
            return Err(VendorCaptureError::Invalid);
        }
        let size = field_octal(&self.block[124..136])?;
        let padding = (BLOCK - (size as usize % BLOCK)) % BLOCK;
        match self.block[156] {
            b'x' => {
                if self.pending_path.is_some() || size == 0 || size > BLOCK as u64 {
                    return Err(VendorCaptureError::Invalid);
                }
                self.mode = Mode::Record {
                    size: usize::try_from(size).map_err(|_| VendorCaptureError::Invalid)?,
                    padding,
                };
                Ok(())
            }
            kind @ (b'0' | b'5') => {
                let path = match self.pending_path.take() {
                    Some(path) => path,
                    None => decode_name(&self.block)?,
                };
                self.counters.admit(&path)?;
                if kind == b'5' {
                    if size != 0 {
                        return Err(VendorCaptureError::Invalid);
                    }
                    self.counters.opened_directory(&path);
                    self.tree.update(b"d");
                    self.tree.update(&(path.len() as u64).to_le_bytes());
                    self.tree.update(path.as_bytes());
                    self.mode = Mode::Block;
                } else {
                    self.counters.reserve(size)?;
                    self.counters.total += size;
                    self.tree.update(b"f");
                    self.tree.update(&(path.len() as u64).to_le_bytes());
                    self.tree.update(path.as_bytes());
                    self.tree.update(&size.to_le_bytes());
                    self.mode = if size > 0 {
                        Mode::Content {
                            remaining: size,
                            padding,
                        }
                    } else {
                        Mode::Block
                    };
                }
                Ok(())
            }
            // ADR-078 §6: a symlink, a hard link, a device, a fifo or a socket
            // is a refusal, not a skip.
            _ => Err(VendorCaptureError::Link),
        }
    }

    /// Close the verification and produce the identity the artifact carries. An
    /// artifact that ended mid-entry, or with only one trailer block, never
    /// reaches this: it is truncated, not complete.
    pub fn finish(mut self) -> Result<VendorCapture, VendorCaptureError> {
        if !matches!(self.mode, Mode::Trailer { blocks: 2 })
            || self.filled != 0
            || self.pending_path.is_some()
        {
            return Err(VendorCaptureError::Invalid);
        }
        self.tree.update(b"e");
        self.tree
            .update(&(self.counters.entries as u64).to_le_bytes());
        self.tree.update(&self.counters.total.to_le_bytes());
        Ok(VendorCapture {
            tree_digest: self.tree.finish()?,
            artifact_digest: self.artifact.finish()?,
            artifact_bytes: self.artifact_bytes,
            entries: self.counters.entries,
            files: self.counters.files,
            directories: self.counters.directories,
            total_bytes: self.counters.total,
        })
    }

    /// ADR-078 §2: refuse rather than use a capture whose digest is not the
    /// declared one.
    pub fn verified(
        self,
        declared: &SourceFingerprint,
    ) -> Result<VendorCapture, VendorCaptureError> {
        let capture = self.finish()?;
        if !capture.matches(declared) {
            return Err(VendorCaptureError::Digest);
        }
        Ok(capture)
    }
}

fn field_octal(field: &[u8]) -> Result<u64, VendorCaptureError> {
    let text = std::str::from_utf8(field).map_err(|_| VendorCaptureError::Invalid)?;
    let text = text.trim_matches(['\0', ' ']);
    u64::from_str_radix(text, 8).map_err(|_| VendorCaptureError::Invalid)
}

fn decode_name(block: &[u8; BLOCK]) -> Result<String, VendorCaptureError> {
    let cut = |field: &[u8]| -> Result<String, VendorCaptureError> {
        let end = field
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(field.len());
        std::str::from_utf8(&field[..end])
            .map(str::to_owned)
            .map_err(|_| VendorCaptureError::Invalid)
    };
    let name = cut(&block[..USTAR_NAME])?;
    let prefix = cut(&block[345..345 + USTAR_PREFIX])?;
    Ok(if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    })
}

/// Exactly one `path=` record, self-sized. More than one, a different keyword
/// or a length that does not describe the record is malformed framing.
fn decode_record(record: &[u8]) -> Result<String, VendorCaptureError> {
    let text = std::str::from_utf8(record).map_err(|_| VendorCaptureError::Invalid)?;
    let (length, rest) = text.split_once(' ').ok_or(VendorCaptureError::Invalid)?;
    if length.parse::<usize>() != Ok(text.len()) {
        return Err(VendorCaptureError::Invalid);
    }
    let value = rest
        .strip_prefix("path=")
        .and_then(|value| value.strip_suffix('\n'))
        .ok_or(VendorCaptureError::Invalid)?;
    Ok(value.to_owned())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;
    use crate::{SOURCE_MAX_PATH_BYTES, SourceError, validate_source_path};

    /// A content-sensitive stand-in for SHA-256. It is not a hash function and
    /// is never used outside this module: it exists so the framing this module
    /// owns can be tested where the algorithm is not.
    #[derive(Default)]
    struct Toy(u64);
    impl CaptureHasher for Toy {
        fn update(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        fn finish(self) -> Result<SourceFingerprint, VendorCaptureError> {
            let word = format!("{:016x}", self.0);
            format!("sha256:{}", word.repeat(4))
                .parse()
                .map_err(|_| VendorCaptureError::Invalid)
        }
    }

    /// Discards its input. Used only where the assertion is about a quota and
    /// not about a digest, so that half a gigabyte of zeros costs nothing.
    #[derive(Default)]
    struct Blind;
    impl CaptureHasher for Blind {
        fn update(&mut self, _: &[u8]) {}
        fn finish(self) -> Result<SourceFingerprint, VendorCaptureError> {
            format!("sha256:{}", "0".repeat(64))
                .parse()
                .map_err(|_| VendorCaptureError::Invalid)
        }
    }

    fn builder() -> VendorCaptureBuilder<Toy> {
        VendorCaptureBuilder::new(Toy::default(), Toy::default())
    }
    fn blind() -> VendorCaptureBuilder<Blind> {
        VendorCaptureBuilder::new(Blind, Blind)
    }
    fn verifier() -> VendorCaptureVerifier<Toy> {
        VendorCaptureVerifier::new(Toy::default(), Toy::default())
    }

    /// One entry description, used by the fixture builder below.
    enum Entry<'a> {
        Dir(&'a str),
        File(&'a str, &'a [u8]),
    }

    fn capture(entries: &[Entry<'_>]) -> Result<(VendorCapture, Vec<u8>), VendorCaptureError> {
        let mut builder = builder();
        let mut artifact = Vec::new();
        for entry in entries {
            match entry {
                Entry::Dir(path) => {
                    artifact.extend_from_slice(builder.directory(path)?.as_slice());
                }
                Entry::File(path, bytes) => {
                    artifact.extend_from_slice(
                        builder.begin_file(path, bytes.len() as u64)?.as_slice(),
                    );
                    // Deliberately fed in two pieces: the digest must not
                    // depend on how the reader happened to split the file.
                    let split = bytes.len() / 2;
                    for piece in [&bytes[..split], &bytes[split..]] {
                        builder.chunk(piece)?;
                        artifact.extend_from_slice(piece);
                    }
                    artifact.extend_from_slice(builder.end_file()?.as_slice());
                }
            }
        }
        let (capture, trailer) = builder.finish()?;
        artifact.extend_from_slice(trailer.as_slice());
        Ok((capture, artifact))
    }

    fn sample() -> Result<(VendorCapture, Vec<u8>), VendorCaptureError> {
        capture(&[
            Entry::Dir("zerocopy-derive-0.8.56"),
            Entry::Dir("zerocopy-derive-0.8.56/src"),
            Entry::File(
                "zerocopy-derive-0.8.56/src/into_bytes_enum.repr(i8).rs",
                b"ok",
            ),
            Entry::File("zerocopy-derive-0.8.56/src/lib.rs", &[0, 255, 10, 9]),
        ])
    }

    // -- the alphabet ---------------------------------------------------------

    #[test]
    fn the_alphabet_admits_exactly_what_adr_078_widened_it_to() {
        for byte in 0..=127_u8 {
            let path = format!("p{}q", char::from(byte));
            let admitted = byte.is_ascii_alphanumeric()
                || b"._-".contains(&byte)
                || EXTRA_PATH_BYTES.contains(&byte);
            // `/` is a separator, not a name byte; it is covered by the
            // structural cases below.
            if byte == b'/' {
                continue;
            }
            assert_eq!(
                validate_capture_path(&path).is_ok(),
                admitted,
                "byte {byte:#04x} ({:?})",
                char::from(byte)
            );
        }
        for byte in EXTRA_PATH_BYTES {
            assert_eq!(
                validate_capture_path(&format!("a{}b", char::from(*byte))),
                Ok(()),
                "the ADR widened the alphabet to include {:?}",
                char::from(*byte)
            );
        }
    }

    #[test]
    fn every_control_byte_is_refused_one_by_one() {
        for byte in (0..=0x1f_u8).chain(std::iter::once(0x7f)) {
            let path = format!("a{}b", char::from(byte));
            assert_eq!(
                validate_capture_path(&path),
                Err(VendorCaptureError::Invalid),
                "control byte {byte:#04x} must stay outside the alphabet"
            );
        }
    }

    #[test]
    fn every_non_ascii_byte_is_refused_one_by_one() {
        for byte in 0x80..=0xff_u8 {
            let path = format!("a{}b", char::from(byte));
            assert_eq!(
                validate_capture_path(&path),
                Err(VendorCaptureError::Invalid),
                "non-ASCII {byte:#04x} must stay outside the alphabet"
            );
        }
        // A homograph of the separator, a zero-width joiner and an ideograph:
        // each is a distinct way of making a path read as something else.
        for path in ["a\u{ff0f}b", "a\u{200b}b", "a\u{4e2d}b", "café"] {
            assert_eq!(
                validate_capture_path(path),
                Err(VendorCaptureError::Invalid),
                "{path:?}"
            );
        }
    }

    #[test]
    fn the_backslash_is_refused() {
        assert_eq!(
            validate_capture_path("a\\b"),
            Err(VendorCaptureError::Invalid)
        );
        assert_eq!(
            validate_capture_path("\\"),
            Err(VendorCaptureError::Invalid)
        );
    }

    #[test]
    fn the_colon_is_refused() {
        assert_eq!(
            validate_capture_path("a:b"),
            Err(VendorCaptureError::Invalid)
        );
        assert_eq!(
            validate_capture_path("C:/x"),
            Err(VendorCaptureError::Invalid)
        );
    }

    #[test]
    fn an_empty_component_is_refused() {
        for path in ["", "a//b", "a/", "//", "a///b"] {
            assert_eq!(
                validate_capture_path(path),
                Err(VendorCaptureError::Invalid),
                "{path:?}"
            );
        }
    }

    #[test]
    fn a_dot_component_is_refused() {
        for path in [".", "./a", "a/./b", "a/."] {
            assert_eq!(
                validate_capture_path(path),
                Err(VendorCaptureError::Invalid),
                "{path:?}"
            );
        }
    }

    #[test]
    fn a_dot_dot_component_is_refused() {
        for path in ["..", "../a", "a/../b", "a/.."] {
            assert_eq!(
                validate_capture_path(path),
                Err(VendorCaptureError::Invalid),
                "{path:?}"
            );
        }
    }

    #[test]
    fn an_absolute_path_is_refused() {
        for path in ["/", "/a", "/a/b"] {
            assert_eq!(
                validate_capture_path(path),
                Err(VendorCaptureError::Invalid),
                "{path:?}"
            );
        }
    }

    #[test]
    fn the_punctuation_the_adr_did_not_widen_to_is_still_refused() {
        for byte in b"!\"#$%&'*;<>?^|`" {
            let path = format!("a{}b", char::from(*byte));
            assert_eq!(
                validate_capture_path(&path),
                Err(VendorCaptureError::Invalid),
                "{:?} is not on ADR-078's list",
                char::from(*byte)
            );
        }
    }

    #[test]
    fn source_bundles_alphabet_and_bounds_are_left_where_they_were() {
        // The thirteen paths this contract exists for. `SourceBundle` still
        // refuses them, and still refuses them as a grammar error.
        let parenthesised = "zerocopy-derive-0.8.56/src/into_bytes_enum.repr(i8).expected.rs";
        assert_eq!(
            validate_source_path(parenthesised),
            Err(SourceError::Invalid)
        );
        assert_eq!(validate_capture_path(parenthesised), Ok(()));
        assert_eq!(validate_source_path("a b"), Err(SourceError::Invalid));
        assert_eq!(validate_capture_path("a b"), Ok(()));
        assert_eq!(SOURCE_MAX_PATH_BYTES, 100);
        assert_eq!(VENDOR_CAPTURE_MAX_PATH_BYTES, 200);
    }

    // -- the limits table -----------------------------------------------------

    #[test]
    fn the_limits_are_the_ones_adr_078_fixed() {
        assert_eq!(VENDOR_CAPTURE_READ_BUFFER_BYTES, 64 * 1024);
        assert_eq!(VENDOR_CAPTURE_MAX_TOTAL_BYTES, 512 * 1024 * 1024);
        assert_eq!(VENDOR_CAPTURE_MAX_ENTRIES, 32_768);
        assert_eq!(VENDOR_CAPTURE_MAX_FILE_BYTES, 8 * 1024 * 1024);
        assert_eq!(VENDOR_CAPTURE_MAX_PATH_BYTES, 200);
        assert_eq!(VENDOR_CAPTURE_MAX_DEPTH, 16);
    }

    #[test]
    fn a_path_at_the_byte_limit_passes_and_one_byte_more_is_a_limit() {
        assert_eq!(
            validate_capture_path(&"a".repeat(VENDOR_CAPTURE_MAX_PATH_BYTES)),
            Ok(())
        );
        assert_eq!(
            validate_capture_path(&"a".repeat(VENDOR_CAPTURE_MAX_PATH_BYTES + 1)),
            Err(VendorCaptureError::Limits)
        );
    }

    #[test]
    fn an_over_deep_path_is_refused_during_the_read() {
        let deep = vec!["a"; VENDOR_CAPTURE_MAX_DEPTH].join("/");
        assert_eq!(validate_capture_path(&deep), Ok(()));
        let over = vec!["a"; VENDOR_CAPTURE_MAX_DEPTH + 1].join("/");
        assert_eq!(
            validate_capture_path(&over),
            Err(VendorCaptureError::Limits)
        );
        // And the builder refuses it where it is read, before any content.
        let mut builder = blind();
        for depth in 1..=VENDOR_CAPTURE_MAX_DEPTH {
            assert!(builder.directory(&vec!["a"; depth].join("/")).is_ok());
        }
        assert_eq!(
            builder.begin_file(&over, 1).err(),
            Some(VendorCaptureError::Limits)
        );
    }

    #[test]
    fn an_oversize_entry_is_refused_before_a_byte_of_it_is_read() {
        let mut builder = blind();
        assert!(
            builder
                .begin_file("f", VENDOR_CAPTURE_MAX_FILE_BYTES)
                .is_ok()
        );
        let mut builder = blind();
        assert_eq!(
            builder
                .begin_file("f", VENDOR_CAPTURE_MAX_FILE_BYTES + 1)
                .err(),
            Some(VendorCaptureError::Limits),
            "the quota is charged at the declaration, not after the read"
        );
    }

    #[test]
    fn an_oversize_total_is_refused_at_the_entry_that_crosses_it() {
        let mut builder = blind();
        let zeros = [0u8; VENDOR_CAPTURE_READ_BUFFER_BYTES];
        let per_file = VENDOR_CAPTURE_MAX_FILE_BYTES;
        let files = VENDOR_CAPTURE_MAX_TOTAL_BYTES / per_file;
        for index in 0..files {
            let path = format!("f{index:04}");
            assert!(builder.begin_file(&path, per_file).is_ok(), "{path}");
            let mut written = 0;
            while written < per_file {
                let take = zeros.len().min((per_file - written) as usize);
                assert!(builder.chunk(&zeros[..take]).is_ok());
                written += take as u64;
            }
            assert!(builder.end_file().is_ok());
        }
        assert_eq!(
            builder.begin_file("g", 1).err(),
            Some(VendorCaptureError::Limits),
            "one byte past 512 MiB is refused"
        );
    }

    #[test]
    fn the_entry_quota_counts_directories_and_files_alike() {
        let mut builder = blind();
        for index in 0..VENDOR_CAPTURE_MAX_ENTRIES {
            let path = format!("f{index:05}");
            assert!(builder.begin_file(&path, 0).is_ok());
            assert!(builder.end_file().is_ok());
        }
        assert_eq!(
            builder.directory("z").err(),
            Some(VendorCaptureError::Limits)
        );
    }

    // -- the canonical order and the topology ---------------------------------

    #[test]
    fn entries_out_of_canonical_order_or_repeated_are_refused() {
        let mut builder = blind();
        assert!(builder.directory("b").is_ok());
        assert_eq!(
            builder.directory("a").err(),
            Some(VendorCaptureError::Invalid),
            "a capture is not allowed to choose its own order"
        );
        let mut builder = blind();
        assert!(builder.directory("a").is_ok());
        assert_eq!(
            builder.directory("a").err(),
            Some(VendorCaptureError::Invalid)
        );
    }

    #[test]
    fn an_entry_whose_parent_was_never_declared_is_refused() {
        let mut builder = blind();
        assert_eq!(
            builder.begin_file("a/b", 0).err(),
            Some(VendorCaptureError::Invalid)
        );
        // And a directory that was closed cannot be re-entered later.
        let mut builder = blind();
        assert!(builder.directory("a").is_ok());
        assert!(builder.begin_file("a/x", 0).is_ok());
        assert!(builder.end_file().is_ok());
        assert!(builder.directory("b").is_ok());
        assert_eq!(
            builder.begin_file("a/y", 0).err(),
            Some(VendorCaptureError::Invalid)
        );
    }

    #[test]
    fn a_sibling_that_sorts_between_a_directory_and_its_subtree_is_still_admitted() {
        // `a-b` sits between `a` and `a/b` in byte order and would break a
        // byte-ordered reader's directory stack. Component order does not.
        let entries = [
            Entry::Dir("a"),
            Entry::File("a/b", b"x"),
            Entry::Dir("a-b"),
            Entry::File("a-b/c", b"y"),
        ];
        let (capture, artifact) = capture(&entries).expect("capture");
        assert_eq!(capture.entries(), 4);
        let mut check = verifier();
        check.chunk(&artifact).expect("verify");
        assert_eq!(&check.finish().expect("finish"), &capture);
    }

    // -- mid-read mutation ----------------------------------------------------

    #[test]
    fn a_file_that_grew_under_the_reader_is_refused_at_the_chunk_that_crosses() {
        let mut builder = blind();
        assert!(builder.begin_file("f", 4).is_ok());
        assert!(builder.chunk(&[0; 4]).is_ok());
        assert_eq!(
            builder.chunk(&[0; 1]).err(),
            Some(VendorCaptureError::Mutated)
        );
    }

    #[test]
    fn a_file_that_shrank_under_the_reader_is_refused_when_it_closes() {
        let mut builder = blind();
        assert!(builder.begin_file("f", 4).is_ok());
        assert!(builder.chunk(&[0; 3]).is_ok());
        assert_eq!(builder.end_file().err(), Some(VendorCaptureError::Mutated));
    }

    #[test]
    fn a_capture_left_with_an_open_entry_never_produces_an_identity() {
        let mut builder = blind();
        assert!(builder.begin_file("f", 4).is_ok());
        assert_eq!(builder.finish().err(), Some(VendorCaptureError::Invalid));
    }

    // -- the artifact and the verify path -------------------------------------

    #[test]
    fn the_artifact_round_trips_and_recovers_the_same_identity() {
        let (capture, artifact) = sample().expect("capture");
        assert_eq!(capture.files(), 2);
        assert_eq!(capture.directories(), 2);
        assert_eq!(capture.entries(), 4);
        assert_eq!(capture.total_bytes(), 6);
        assert_eq!(capture.artifact_bytes() as usize, artifact.len());
        assert_eq!(artifact.len() % BLOCK, 0);
        let mut check = verifier();
        check.chunk(&artifact).expect("verify");
        let recovered = check.finish().expect("finish");
        assert_eq!(recovered, capture);
        assert_eq!(recovered.tree_digest(), capture.tree_digest());
        assert_eq!(recovered.artifact_digest(), capture.artifact_digest());
    }

    #[test]
    fn the_digest_does_not_depend_on_how_the_artifact_was_split_into_reads() {
        let (capture, artifact) = sample().expect("capture");
        for size in [1, 7, 512, 1024, VENDOR_CAPTURE_READ_BUFFER_BYTES] {
            let mut check = verifier();
            for piece in artifact.chunks(size) {
                check.chunk(piece).expect("verify");
            }
            assert_eq!(
                check.finish().expect("finish"),
                capture,
                "chunked at {size}"
            );
        }
    }

    #[test]
    fn a_path_whose_last_component_exceeds_the_ustar_name_field_still_round_trips() {
        let long = format!("pkg-1.0.0/tests/{}.rs", "n".repeat(120));
        assert_eq!(validate_capture_path(&long), Ok(()));
        assert_eq!(split_name(&long), None, "this path needs a pax record");
        let (capture, artifact) = capture(&[
            Entry::Dir("pkg-1.0.0"),
            Entry::Dir("pkg-1.0.0/tests"),
            Entry::File(&long, b"body"),
        ])
        .expect("capture");
        let mut check = verifier();
        check.chunk(&artifact).expect("verify");
        assert_eq!(check.finish().expect("finish"), capture);
    }

    #[test]
    fn a_capture_whose_digest_is_not_the_declared_one_is_refused_rather_than_used() {
        let (capture, artifact) = sample().expect("capture");
        let mut check = verifier();
        check.chunk(&artifact).expect("verify");
        assert!(check.verified(capture.tree_digest()).is_ok());

        // One content byte flipped: same framing, same counts, different tree.
        let mut tampered = artifact.clone();
        let offset = tampered
            .windows(4)
            .position(|window| window == [0, 255, 10, 9])
            .expect("content");
        tampered[offset + 1] = 254;
        let mut check = verifier();
        check.chunk(&tampered).expect("framing is still valid");
        assert_eq!(
            check.verified(capture.tree_digest()).err(),
            Some(VendorCaptureError::Digest)
        );
    }

    #[test]
    fn a_truncated_artifact_is_never_taken_for_a_complete_one() {
        let (_, artifact) = sample().expect("capture");
        for cut in [
            artifact.len() - 1,
            artifact.len() - BLOCK,
            artifact.len() - 2 * BLOCK,
            BLOCK,
            0,
        ] {
            let mut check = verifier();
            let outcome = check
                .chunk(&artifact[..cut])
                .and_then(|()| check.finish().map(|_| ()));
            assert_eq!(
                outcome.err(),
                Some(VendorCaptureError::Invalid),
                "an artifact cut at {cut} is not complete"
            );
        }
    }

    #[test]
    fn an_artifact_with_anything_appended_after_its_trailer_is_refused() {
        let (_, artifact) = sample().expect("capture");
        let mut appended = artifact.clone();
        appended.push(1);
        let mut check = verifier();
        assert_eq!(
            check.chunk(&appended).err(),
            Some(VendorCaptureError::Invalid)
        );
        let mut padded = artifact;
        padded.extend_from_slice(&[0; BLOCK]);
        let mut check = verifier();
        assert_eq!(
            check.chunk(&padded).err(),
            Some(VendorCaptureError::Invalid)
        );
    }

    #[test]
    fn a_link_a_device_or_a_fifo_in_an_artifact_is_a_refusal_and_not_a_skip() {
        for kind in *b"123467LK" {
            let (_, artifact) = sample().expect("capture");
            let mut tampered = artifact;
            tampered[156] = kind;
            let sum = {
                let mut header = [0u8; BLOCK];
                header.copy_from_slice(&tampered[..BLOCK]);
                header_checksum(&header)
            };
            octal(&mut tampered[148..155], sum).expect("checksum");
            tampered[155] = b' ';
            let mut check = verifier();
            assert_eq!(
                check.chunk(&tampered).err(),
                Some(VendorCaptureError::Link),
                "type flag {:?}",
                char::from(kind)
            );
        }
    }

    #[test]
    fn an_artifact_header_with_a_broken_checksum_or_magic_is_refused() {
        let (_, artifact) = sample().expect("capture");
        let mut broken = artifact.clone();
        broken[0] = b'a';
        let mut check = verifier();
        assert_eq!(
            check.chunk(&broken).err(),
            Some(VendorCaptureError::Invalid)
        );

        let mut broken = artifact;
        broken[257] = b'g';
        let mut check = verifier();
        assert_eq!(
            check.chunk(&broken).err(),
            Some(VendorCaptureError::Invalid)
        );
    }

    #[test]
    fn an_artifact_that_hides_bytes_in_the_padding_is_refused() {
        let (_, artifact) = sample().expect("capture");
        // The last file is four bytes long, so its block has 508 bytes of
        // padding a smuggler could otherwise use without moving the digest.
        let position = artifact.len() - 2 * BLOCK - 1;
        let mut tampered = artifact;
        tampered[position] = 1;
        let mut check = verifier();
        assert_eq!(
            check.chunk(&tampered).err(),
            Some(VendorCaptureError::Invalid)
        );
    }

    #[test]
    fn an_artifact_carrying_a_path_outside_the_alphabet_is_refused_on_the_verify_path() {
        let (_, artifact) =
            capture(&[Entry::Dir("pkg"), Entry::File("pkg/a", b"x")]).expect("capture");
        let mut tampered = artifact;
        // `pkg/a` -> `pkg:a`, still five bytes, so only the header changes.
        let offset = tampered
            .windows(5)
            .position(|window| window == b"pkg/a")
            .expect("name");
        tampered[offset + 3] = b':';
        let sum = {
            let mut header = [0u8; BLOCK];
            let start = offset / BLOCK * BLOCK;
            header.copy_from_slice(&tampered[start..start + BLOCK]);
            (start, header_checksum(&header))
        };
        octal(&mut tampered[sum.0 + 148..sum.0 + 155], sum.1).expect("checksum");
        tampered[sum.0 + 155] = b' ';
        let mut check = verifier();
        assert_eq!(
            check.chunk(&tampered).err(),
            Some(VendorCaptureError::Invalid),
            "the verify path applies the same alphabet the capture path did"
        );
    }

    #[test]
    fn an_empty_capture_still_has_an_identity_and_it_is_not_a_non_empty_ones() {
        let (empty, artifact) = capture(&[]).expect("capture");
        assert_eq!(empty.entries(), 0);
        assert_eq!(artifact.len(), 2 * BLOCK);
        let (other, _) = sample().expect("capture");
        assert_ne!(empty.tree_digest(), other.tree_digest());
        let mut check = verifier();
        check.chunk(&artifact).expect("verify");
        assert_eq!(check.finish().expect("finish"), empty);
    }
}
