//! Target-independent logic for `rust-mcp-profile-helper`.
//!
//! Everything in this module is free of syscalls and of platform assumptions
//! beyond little-endian byte order, so the argument grammar, the perf ring
//! buffer decoder, the ELF64 symbol lookup, the frame-name sanitiser, the
//! folded-stack aggregator and the manifest serialiser can all be unit tested
//! on the macOS ARM64 development hosts even though the profiler itself only
//! runs on `aarch64-unknown-linux-gnu`.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Exit codes
// ---------------------------------------------------------------------------

/// The profile ran; the child's own outcome is reported in the manifest.
pub const EXIT_SUCCESS: i32 = 0;
/// The argv did not match the closed CLI contract.
pub const EXIT_INVALID_ARGUMENTS: i32 = 2;
/// `perf_event_open` (or the ring buffer setup that follows it) was denied.
pub const EXIT_PROFILER_UNAVAILABLE: i32 = 3;
/// An internal or I/O failure prevented the helper from producing a profile.
pub const EXIT_INTERNAL_FAILURE: i32 = 4;

// ---------------------------------------------------------------------------
// Argument grammar
// ---------------------------------------------------------------------------

pub const MIN_FREQUENCY_HZ: u64 = 1;
pub const MAX_FREQUENCY_HZ: u64 = 999;
pub const MIN_DURATION_MS: u64 = 100;
pub const MAX_DURATION_MS: u64 = 60_000;
pub const MIN_MAX_SAMPLES: u64 = 1;
pub const MAX_MAX_SAMPLES: u64 = 2_000_000;
pub const MIN_MAX_DEPTH: u64 = 1;
pub const MAX_MAX_DEPTH: u64 = 256;

const FLAG_FREQUENCY_HZ: &str = "--frequency-hz";
const FLAG_DURATION_MS: &str = "--duration-ms";
const FLAG_MAX_SAMPLES: &str = "--max-samples";
const FLAG_MAX_DEPTH: &str = "--max-depth";
const FLAG_STACKS: &str = "--stacks";
const FLAG_MANIFEST: &str = "--manifest";
const FLAG_SEPARATOR: &str = "--";

/// Every way the closed argv contract can be violated. Each maps to exactly
/// one fixed single-line stderr message; no caller-supplied bytes are echoed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArgumentError {
    UnknownArgument,
    DuplicateFlag,
    MissingFlag,
    MissingValue,
    NonUtf8Value,
    NotANumber,
    OutOfRange,
    EmptyPath,
    MissingProgram,
    ProgramNotAbsolute,
}

impl ArgumentError {
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::UnknownArgument => "invalid arguments: unknown argument",
            Self::DuplicateFlag => "invalid arguments: repeated flag",
            Self::MissingFlag => "invalid arguments: missing required flag",
            Self::MissingValue => "invalid arguments: flag without a value",
            Self::NonUtf8Value => "invalid arguments: value is not valid UTF-8",
            Self::NotANumber => "invalid arguments: value is not a decimal number",
            Self::OutOfRange => "invalid arguments: value out of range",
            Self::EmptyPath => "invalid arguments: empty output path",
            Self::MissingProgram => "invalid arguments: no program after --",
            Self::ProgramNotAbsolute => "invalid arguments: program path is not absolute",
        }
    }
}

/// The fully validated invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Arguments {
    pub frequency_hz: u32,
    pub duration_ms: u64,
    pub max_samples: u64,
    pub max_depth: u32,
    pub stacks_path: PathBuf,
    pub manifest_path: PathBuf,
    pub program: PathBuf,
    pub program_args: Vec<OsString>,
}

/// Parses the argv tail (everything after `argv[0]`) against the closed
/// contract. Any deviation is a hard rejection; nothing is inferred.
///
/// # Errors
///
/// Returns the specific [`ArgumentError`] describing the first violation.
pub fn parse_arguments<I>(raw: I) -> Result<Arguments, ArgumentError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut frequency_hz: Option<u64> = None;
    let mut duration_ms: Option<u64> = None;
    let mut max_samples: Option<u64> = None;
    let mut max_depth: Option<u64> = None;
    let mut stacks_path: Option<PathBuf> = None;
    let mut manifest_path: Option<PathBuf> = None;
    let mut trailing: Option<Vec<OsString>> = None;

    let mut items = raw.into_iter();
    while let Some(item) = items.next() {
        let Some(flag) = item.to_str() else {
            return Err(ArgumentError::UnknownArgument);
        };
        match flag {
            FLAG_SEPARATOR => {
                trailing = Some(items.by_ref().collect());
                break;
            }
            FLAG_FREQUENCY_HZ => {
                let value =
                    parse_number(&next_value(&mut items)?, MIN_FREQUENCY_HZ, MAX_FREQUENCY_HZ)?;
                set_once(&mut frequency_hz, value)?;
            }
            FLAG_DURATION_MS => {
                let value =
                    parse_number(&next_value(&mut items)?, MIN_DURATION_MS, MAX_DURATION_MS)?;
                set_once(&mut duration_ms, value)?;
            }
            FLAG_MAX_SAMPLES => {
                let value =
                    parse_number(&next_value(&mut items)?, MIN_MAX_SAMPLES, MAX_MAX_SAMPLES)?;
                set_once(&mut max_samples, value)?;
            }
            FLAG_MAX_DEPTH => {
                let value = parse_number(&next_value(&mut items)?, MIN_MAX_DEPTH, MAX_MAX_DEPTH)?;
                set_once(&mut max_depth, value)?;
            }
            FLAG_STACKS => {
                let value = output_path(&next_value(&mut items)?)?;
                set_once(&mut stacks_path, value)?;
            }
            FLAG_MANIFEST => {
                let value = output_path(&next_value(&mut items)?)?;
                set_once(&mut manifest_path, value)?;
            }
            _ => return Err(ArgumentError::UnknownArgument),
        }
    }

    let mut program_and_args = trailing.ok_or(ArgumentError::MissingProgram)?.into_iter();
    let program = PathBuf::from(
        program_and_args
            .next()
            .ok_or(ArgumentError::MissingProgram)?,
    );
    if !program.is_absolute() {
        return Err(ArgumentError::ProgramNotAbsolute);
    }

    Ok(Arguments {
        frequency_hz: narrow(frequency_hz.ok_or(ArgumentError::MissingFlag)?)?,
        duration_ms: duration_ms.ok_or(ArgumentError::MissingFlag)?,
        max_samples: max_samples.ok_or(ArgumentError::MissingFlag)?,
        max_depth: narrow(max_depth.ok_or(ArgumentError::MissingFlag)?)?,
        stacks_path: stacks_path.ok_or(ArgumentError::MissingFlag)?,
        manifest_path: manifest_path.ok_or(ArgumentError::MissingFlag)?,
        program,
        program_args: program_and_args.collect(),
    })
}

fn narrow(value: u64) -> Result<u32, ArgumentError> {
    u32::try_from(value).map_err(|_| ArgumentError::OutOfRange)
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Result<(), ArgumentError> {
    if slot.is_some() {
        return Err(ArgumentError::DuplicateFlag);
    }
    *slot = Some(value);
    Ok(())
}

fn next_value<I>(items: &mut I) -> Result<OsString, ArgumentError>
where
    I: Iterator<Item = OsString>,
{
    items.next().ok_or(ArgumentError::MissingValue)
}

fn parse_number(raw: &OsStr, low: u64, high: u64) -> Result<u64, ArgumentError> {
    let text = raw.to_str().ok_or(ArgumentError::NonUtf8Value)?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ArgumentError::NotANumber);
    }
    let value = text.parse::<u64>().map_err(|_| ArgumentError::OutOfRange)?;
    if value < low || value > high {
        return Err(ArgumentError::OutOfRange);
    }
    Ok(value)
}

fn output_path(raw: &OsStr) -> Result<PathBuf, ArgumentError> {
    if raw.is_empty() {
        return Err(ArgumentError::EmptyPath);
    }
    Ok(Path::new(raw).to_path_buf())
}

// ---------------------------------------------------------------------------
// perf ring-buffer records
// ---------------------------------------------------------------------------

pub const PERF_RECORD_MMAP: u32 = 1;
pub const PERF_RECORD_LOST: u32 = 2;
pub const PERF_RECORD_SAMPLE: u32 = 9;
pub const PERF_RECORD_MMAP2: u32 = 10;

/// `struct perf_event_header { u32 type; u16 misc; u16 size; }`
pub const PERF_RECORD_HEADER_BYTES: usize = 8;
/// `PERF_RECORD_MISC_MMAP_DATA`: the mapping is not executable.
pub const PERF_RECORD_MISC_MMAP_DATA: u16 = 0x2000;
/// `PERF_RECORD_MISC_MMAP_BUILD_ID`: alternate MMAP2 body layout we never ask for.
pub const PERF_RECORD_MISC_MMAP_BUILD_ID: u16 = 0x4000;

/// Callchain entries at or above this value are `PERF_CONTEXT_*` markers.
pub const PERF_CONTEXT_MIN: u64 = 0xffff_ffff_ffff_f000;
/// `PERF_CONTEXT_USER` (`-512`).
pub const PERF_CONTEXT_USER: u64 = 0xffff_ffff_ffff_fe00;

/// Upper bound on distinct executable mappings retained, so a hostile child
/// cannot grow the module table without limit.
pub const MAX_MAPPINGS: usize = 16_384;
/// Upper bound on distinct folded stacks retained.
pub const MAX_DISTINCT_STACKS: usize = 250_000;
/// Upper bound on records decoded in one [`drain_ring`] pass, so a single call
/// cannot allocate without limit. The caller loops until the window is empty.
pub const MAX_RECORDS_PER_PASS: usize = 16_384;

/// An executable (or rejected data) mapping announced by `PERF_RECORD_MMAP`
/// or `PERF_RECORD_MMAP2`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MappingRecord {
    pub pid: u32,
    pub tid: u32,
    pub addr: u64,
    pub len: u64,
    pub pgoff: u64,
    pub filename: Vec<u8>,
    pub executable: bool,
}

impl MappingRecord {
    /// The mapping's backing file, if it can be used for symbolization at all.
    /// Anonymous regions, the vDSO and anything that is not an absolute path
    /// are rejected here and become `[unknown]` frames.
    #[must_use]
    pub fn module_path(&self) -> Option<&str> {
        let name = std::str::from_utf8(&self.filename).ok()?;
        if !name.starts_with('/') || name.starts_with('[') || name.contains('\0') {
            return None;
        }
        Some(name)
    }

    /// `file_offset = addr - map.addr + map.pgoff`.
    #[must_use]
    pub fn file_offset(&self, addr: u64) -> Option<u64> {
        addr.checked_sub(self.addr)?.checked_add(self.pgoff)
    }

    #[must_use]
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.addr && addr - self.addr < self.len
    }
}

/// One `PERF_RECORD_SAMPLE` decoded under
/// `PERF_SAMPLE_IP|PERF_SAMPLE_TID|PERF_SAMPLE_TIME|PERF_SAMPLE_CALLCHAIN`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SampleRecord {
    pub ip: u64,
    pub pid: u32,
    pub tid: u32,
    pub time: u64,
    pub callchain: Vec<u64>,
}

/// A decoded ring-buffer record. Record types the helper does not model are
/// preserved as [`Record::Ignored`] so the caller can still account for them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Record {
    Mapping(MappingRecord),
    Lost(u64),
    Sample(SampleRecord),
    Ignored(u32),
}

/// The outcome of one pass over `[data_tail, data_head)`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RingDrain {
    pub records: Vec<Record>,
    /// The value to publish back into `data_tail`.
    pub tail: u64,
    /// Set when the writer lapped the reader and the window was resynchronised.
    pub overrun: bool,
}

/// Decodes every complete record in `[tail, head)` of a perf ring buffer whose
/// data area is `data` (a power-of-two byte count). Records that wrap the end
/// of the buffer are copied into a contiguous scratch buffer first. A trailing
/// partial record is left in place and its bytes are not consumed.
#[must_use]
pub fn drain_ring(data: &[u8], tail: u64, head: u64) -> RingDrain {
    let mut cursor = tail;
    if data.len() < PERF_RECORD_HEADER_BYTES || head <= tail {
        return RingDrain {
            records: Vec::new(),
            tail: cursor,
            overrun: false,
        };
    }

    let capacity = data.len() as u64;
    if head - cursor > capacity {
        // The kernel writes LOST records rather than overwriting unread bytes,
        // so this should not happen; if it ever does, the window is garbage and
        // the only safe move is to resynchronise on `head`.
        return RingDrain {
            records: Vec::new(),
            tail: head,
            overrun: true,
        };
    }

    let mut records = Vec::new();
    let mut scratch: Vec<u8> = Vec::new();
    while cursor < head {
        let remaining = head - cursor;
        if remaining < PERF_RECORD_HEADER_BYTES as u64 {
            break;
        }
        copy_wrapped(data, cursor, PERF_RECORD_HEADER_BYTES, &mut scratch);
        let Some(kind) = read_u32(&scratch, 0) else {
            break;
        };
        let Some(misc) = read_u16(&scratch, 4) else {
            break;
        };
        let Some(size) = read_u16(&scratch, 6) else {
            break;
        };
        let size = u64::from(size);
        if size < PERF_RECORD_HEADER_BYTES as u64 || size > remaining {
            // Truncated tail: the writer has not finished this record yet.
            break;
        }
        let Ok(size_usize) = usize::try_from(size) else {
            break;
        };
        copy_wrapped(data, cursor, size_usize, &mut scratch);
        records.push(decode_record(
            kind,
            misc,
            &scratch[PERF_RECORD_HEADER_BYTES..],
        ));
        cursor += size;
        if records.len() >= MAX_RECORDS_PER_PASS {
            break;
        }
    }

    RingDrain {
        records,
        tail: cursor,
        overrun: false,
    }
}

/// Copies `len` bytes starting at ring offset `offset` into `scratch`, joining
/// the two halves when the run wraps past the end of the data area.
fn copy_wrapped(data: &[u8], offset: u64, len: usize, scratch: &mut Vec<u8>) {
    scratch.clear();
    if data.is_empty() || len > data.len() {
        return;
    }
    let capacity = data.len();
    let start = usize::try_from(offset % capacity as u64).unwrap_or(0);
    let first = len.min(capacity - start);
    scratch.extend_from_slice(&data[start..start + first]);
    if first < len {
        scratch.extend_from_slice(&data[..len - first]);
    }
}

fn decode_record(kind: u32, misc: u16, body: &[u8]) -> Record {
    match kind {
        PERF_RECORD_SAMPLE => decode_sample(body).map_or(Record::Ignored(kind), Record::Sample),
        PERF_RECORD_LOST => decode_lost(body).map_or(Record::Ignored(kind), Record::Lost),
        PERF_RECORD_MMAP => decode_mmap(body, misc).map_or(Record::Ignored(kind), Record::Mapping),
        PERF_RECORD_MMAP2 => {
            decode_mmap2(body, misc).map_or(Record::Ignored(kind), Record::Mapping)
        }
        other => Record::Ignored(other),
    }
}

fn decode_lost(body: &[u8]) -> Option<u64> {
    let _id = read_u64(body, 0)?;
    read_u64(body, 8)
}

fn decode_sample(body: &[u8]) -> Option<SampleRecord> {
    let ip = read_u64(body, 0)?;
    let pid = read_u32(body, 8)?;
    let tid = read_u32(body, 12)?;
    let time = read_u64(body, 16)?;
    let nr = read_u64(body, 24)?;
    let count = usize::try_from(nr).ok()?;
    let bytes = count.checked_mul(8)?;
    let end = 32usize.checked_add(bytes)?;
    if body.len() < end {
        return None;
    }
    let mut callchain = Vec::with_capacity(count);
    for index in 0..count {
        callchain.push(read_u64(body, 32 + index * 8)?);
    }
    Some(SampleRecord {
        ip,
        pid,
        tid,
        time,
        callchain,
    })
}

/// `PERF_RECORD_MMAP`: `u32 pid, u32 tid, u64 addr, u64 len, u64 pgoff, char filename[]`.
fn decode_mmap(body: &[u8], misc: u16) -> Option<MappingRecord> {
    let pid = read_u32(body, 0)?;
    let tid = read_u32(body, 4)?;
    let addr = read_u64(body, 8)?;
    let len = read_u64(body, 16)?;
    let pgoff = read_u64(body, 24)?;
    let filename = trim_filename(body.get(32..)?);
    Some(MappingRecord {
        pid,
        tid,
        addr,
        len,
        pgoff,
        filename,
        executable: misc & PERF_RECORD_MISC_MMAP_DATA == 0,
    })
}

/// `PERF_RECORD_MMAP2`: the `MMAP` prefix, then `u32 maj, u32 min, u64 ino,
/// u64 ino_generation, u32 prot, u32 flags, char filename[]`. The build-id
/// variant carries a different body and is never requested, so it is refused.
fn decode_mmap2(body: &[u8], misc: u16) -> Option<MappingRecord> {
    if misc & PERF_RECORD_MISC_MMAP_BUILD_ID != 0 {
        return None;
    }
    let pid = read_u32(body, 0)?;
    let tid = read_u32(body, 4)?;
    let addr = read_u64(body, 8)?;
    let len = read_u64(body, 16)?;
    let pgoff = read_u64(body, 24)?;
    let prot = read_u32(body, 56)?;
    let filename = trim_filename(body.get(64..)?);
    Some(MappingRecord {
        pid,
        tid,
        addr,
        len,
        pgoff,
        filename,
        // PROT_EXEC
        executable: prot & 0x4 != 0,
    })
}

fn trim_filename(raw: &[u8]) -> Vec<u8> {
    let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
    raw[..end].to_vec()
}

fn read_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    let array: [u8; 2] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u16::from_le_bytes(array))
}

fn read_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let array: [u8; 4] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(array))
}

fn read_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let end = offset.checked_add(8)?;
    let array: [u8; 8] = bytes.get(offset..end)?.try_into().ok()?;
    Some(u64::from_le_bytes(array))
}

// ---------------------------------------------------------------------------
// Callchains
// ---------------------------------------------------------------------------

/// The user-space part of one sample's callchain, leaf first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserStack {
    pub frames: Vec<u64>,
    pub truncated: bool,
}

/// Strips `PERF_CONTEXT_*` markers, keeps only frames recorded while the
/// context is user space, drops null addresses and truncates to `max_depth`.
#[must_use]
pub fn user_frames(sample: &SampleRecord, max_depth: usize) -> UserStack {
    let mut frames = Vec::new();
    let mut truncated = false;
    // `exclude_kernel = 1` means the first marker is normally PERF_CONTEXT_USER;
    // start in user space so a marker-less callchain is still usable.
    let mut in_user = true;
    for &entry in &sample.callchain {
        if entry >= PERF_CONTEXT_MIN {
            in_user = entry == PERF_CONTEXT_USER;
            continue;
        }
        if !in_user || entry == 0 {
            continue;
        }
        if frames.len() >= max_depth {
            truncated = true;
            break;
        }
        frames.push(entry);
    }
    if frames.is_empty() && sample.ip != 0 && max_depth > 0 {
        frames.push(sample.ip);
    }
    UserStack { frames, truncated }
}

// ---------------------------------------------------------------------------
// Module map
// ---------------------------------------------------------------------------

/// The executable mappings announced for the profiled child, newest first on
/// lookup so a replaced mapping wins over the region it replaced.
#[derive(Clone, Debug, Default)]
pub struct ModuleMap {
    mappings: Vec<MappingRecord>,
    modules: BTreeSet<Vec<u8>>,
}

impl ModuleMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, mapping: MappingRecord) {
        if !mapping.executable || mapping.len == 0 || self.mappings.len() >= MAX_MAPPINGS {
            return;
        }
        self.modules.insert(mapping.filename.clone());
        self.mappings.push(mapping);
    }

    #[must_use]
    pub fn find(&self, addr: u64) -> Option<&MappingRecord> {
        self.mappings
            .iter()
            .rev()
            .find(|mapping| mapping.contains(addr))
    }

    #[must_use]
    pub fn modules_seen(&self) -> u64 {
        self.modules.len() as u64
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.mappings.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.mappings.is_empty()
    }
}

// ---------------------------------------------------------------------------
// ELF64 symbol tables
// ---------------------------------------------------------------------------

const ELF64_HEADER_BYTES: usize = 64;
const ELF64_PHDR_BYTES: u16 = 56;
const ELF64_SHDR_BYTES: u16 = 64;
const ELF64_SYM_BYTES: u64 = 24;
const ELF64_SYM_ENTRY: usize = 24;
const PT_LOAD: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHT_STRTAB: u32 = 3;
const SHT_DYNSYM: u32 = 11;
const STT_FUNC: u8 = 2;
const MAX_SYMBOL_NAME_BYTES: usize = 4096;

/// Why an ELF image could not be used for symbolization.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ElfError {
    TooSmall,
    BadMagic,
    UnsupportedClass,
    UnsupportedEndianness,
    BadHeaderTable,
    NoLoadSegments,
    BadSection,
    NoSymbols,
}

/// One `PT_LOAD` segment, used to turn a file offset back into a virtual address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LoadSegment {
    pub offset: u64,
    pub file_size: u64,
    pub vaddr: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct FunctionSymbol {
    value: u64,
    size: u64,
    name: Box<[u8]>,
}

#[derive(Clone, Copy, Debug)]
struct SectionHeader {
    kind: u32,
    offset: u64,
    size: u64,
    link: u32,
    entry_size: u64,
}

/// The `STT_FUNC` symbols of one module plus the `PT_LOAD` table needed to map
/// file offsets onto the virtual addresses those symbols are expressed in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolTable {
    segments: Vec<LoadSegment>,
    symbols: Vec<FunctionSymbol>,
    widest: u64,
}

impl SymbolTable {
    /// `vaddr = file_offset - p_offset + p_vaddr` for the `PT_LOAD` segment
    /// that contains `file_offset`.
    #[must_use]
    pub fn vaddr_for_offset(&self, file_offset: u64) -> Option<u64> {
        for segment in &self.segments {
            if file_offset >= segment.offset && file_offset - segment.offset < segment.file_size {
                return file_offset
                    .checked_sub(segment.offset)?
                    .checked_add(segment.vaddr);
            }
        }
        None
    }

    /// The `STT_FUNC` symbol whose `[value, value + size)` contains `vaddr`.
    #[must_use]
    pub fn lookup(&self, vaddr: u64) -> Option<&[u8]> {
        let upper = self.symbols.partition_point(|symbol| symbol.value <= vaddr);
        let floor = vaddr.saturating_sub(self.widest);
        for symbol in self.symbols[..upper].iter().rev() {
            if symbol.value < floor {
                break;
            }
            if vaddr - symbol.value < symbol.size {
                return Some(&symbol.name);
            }
        }
        None
    }

    /// Resolves a mapping-relative file offset straight to a symbol name.
    #[must_use]
    pub fn resolve(&self, file_offset: u64) -> Option<&[u8]> {
        self.lookup(self.vaddr_for_offset(file_offset)?)
    }

    #[must_use]
    pub fn symbol_count(&self) -> usize {
        self.symbols.len()
    }

    #[must_use]
    pub fn segments(&self) -> &[LoadSegment] {
        &self.segments
    }
}

/// Parses a little-endian ELF64 image into its `PT_LOAD` table and its
/// `STT_FUNC` symbols, preferring `.symtab` and falling back to `.dynsym`.
///
/// Every table offset, count and entry size is bounds-checked against the
/// image; nothing is trusted.
///
/// # Errors
///
/// Returns the [`ElfError`] describing why the image is unusable.
pub fn parse_elf_symbols(image: &[u8]) -> Result<SymbolTable, ElfError> {
    if image.len() < ELF64_HEADER_BYTES {
        return Err(ElfError::TooSmall);
    }
    if image.get(..4) != Some(b"\x7fELF".as_slice()) {
        return Err(ElfError::BadMagic);
    }
    // EI_CLASS must be ELFCLASS64, EI_DATA must be ELFDATA2LSB.
    if image.get(4) != Some(&2) {
        return Err(ElfError::UnsupportedClass);
    }
    if image.get(5) != Some(&1) {
        return Err(ElfError::UnsupportedEndianness);
    }

    let phoff = read_u64(image, 0x20).ok_or(ElfError::BadHeaderTable)?;
    let shoff = read_u64(image, 0x28).ok_or(ElfError::BadHeaderTable)?;
    let phentsize = read_u16(image, 0x36).ok_or(ElfError::BadHeaderTable)?;
    let phnum = read_u16(image, 0x38).ok_or(ElfError::BadHeaderTable)?;
    let shentsize = read_u16(image, 0x3a).ok_or(ElfError::BadHeaderTable)?;
    let shnum = read_u16(image, 0x3c).ok_or(ElfError::BadHeaderTable)?;

    let segments = parse_load_segments(image, phoff, phentsize, phnum)?;
    if segments.is_empty() {
        return Err(ElfError::NoLoadSegments);
    }
    let sections = parse_sections(image, shoff, shentsize, shnum)?;
    let mut symbols = parse_symbols(image, &sections)?;
    symbols.sort_by(|left, right| {
        left.value
            .cmp(&right.value)
            .then_with(|| left.size.cmp(&right.size))
    });
    let widest = symbols.iter().map(|symbol| symbol.size).max().unwrap_or(0);
    Ok(SymbolTable {
        segments,
        symbols,
        widest,
    })
}

fn table_bytes(
    image: &[u8],
    offset: u64,
    entry_size: u16,
    count: u16,
    expected_entry_size: u16,
) -> Result<Option<(usize, usize)>, ElfError> {
    if count == 0 {
        return Ok(None);
    }
    if entry_size != expected_entry_size {
        return Err(ElfError::BadHeaderTable);
    }
    let start = usize::try_from(offset).map_err(|_| ElfError::BadHeaderTable)?;
    let span = usize::from(count)
        .checked_mul(usize::from(entry_size))
        .ok_or(ElfError::BadHeaderTable)?;
    let end = start.checked_add(span).ok_or(ElfError::BadHeaderTable)?;
    if end > image.len() {
        return Err(ElfError::BadHeaderTable);
    }
    Ok(Some((start, end)))
}

fn parse_load_segments(
    image: &[u8],
    phoff: u64,
    phentsize: u16,
    phnum: u16,
) -> Result<Vec<LoadSegment>, ElfError> {
    let Some((start, end)) = table_bytes(image, phoff, phentsize, phnum, ELF64_PHDR_BYTES)? else {
        return Ok(Vec::new());
    };
    let mut segments = Vec::new();
    for entry in image[start..end].chunks_exact(usize::from(ELF64_PHDR_BYTES)) {
        let kind = read_u32(entry, 0x00).ok_or(ElfError::BadHeaderTable)?;
        if kind != PT_LOAD {
            continue;
        }
        let offset = read_u64(entry, 0x08).ok_or(ElfError::BadHeaderTable)?;
        let vaddr = read_u64(entry, 0x10).ok_or(ElfError::BadHeaderTable)?;
        let file_size = read_u64(entry, 0x20).ok_or(ElfError::BadHeaderTable)?;
        if file_size == 0 {
            continue;
        }
        offset
            .checked_add(file_size)
            .ok_or(ElfError::BadHeaderTable)?;
        segments.push(LoadSegment {
            offset,
            file_size,
            vaddr,
        });
    }
    Ok(segments)
}

fn parse_sections(
    image: &[u8],
    shoff: u64,
    shentsize: u16,
    shnum: u16,
) -> Result<Vec<SectionHeader>, ElfError> {
    let Some((start, end)) = table_bytes(image, shoff, shentsize, shnum, ELF64_SHDR_BYTES)? else {
        return Ok(Vec::new());
    };
    let mut sections = Vec::with_capacity(usize::from(shnum));
    for entry in image[start..end].chunks_exact(usize::from(ELF64_SHDR_BYTES)) {
        sections.push(SectionHeader {
            kind: read_u32(entry, 0x04).ok_or(ElfError::BadSection)?,
            offset: read_u64(entry, 0x18).ok_or(ElfError::BadSection)?,
            size: read_u64(entry, 0x20).ok_or(ElfError::BadSection)?,
            link: read_u32(entry, 0x28).ok_or(ElfError::BadSection)?,
            entry_size: read_u64(entry, 0x38).ok_or(ElfError::BadSection)?,
        });
    }
    Ok(sections)
}

fn section_range(image: &[u8], section: &SectionHeader) -> Result<(usize, usize), ElfError> {
    let start = usize::try_from(section.offset).map_err(|_| ElfError::BadSection)?;
    let span = usize::try_from(section.size).map_err(|_| ElfError::BadSection)?;
    let end = start.checked_add(span).ok_or(ElfError::BadSection)?;
    if end > image.len() {
        return Err(ElfError::BadSection);
    }
    Ok((start, end))
}

fn parse_symbols(
    image: &[u8],
    sections: &[SectionHeader],
) -> Result<Vec<FunctionSymbol>, ElfError> {
    let chosen = sections
        .iter()
        .find(|section| section.kind == SHT_SYMTAB)
        .or_else(|| sections.iter().find(|section| section.kind == SHT_DYNSYM))
        .ok_or(ElfError::NoSymbols)?;
    if chosen.entry_size != ELF64_SYM_BYTES {
        return Err(ElfError::BadSection);
    }
    let (sym_start, sym_end) = section_range(image, chosen)?;

    let string_index = usize::try_from(chosen.link).map_err(|_| ElfError::BadSection)?;
    let string_section = sections.get(string_index).ok_or(ElfError::BadSection)?;
    if string_section.kind != SHT_STRTAB {
        return Err(ElfError::BadSection);
    }
    let (str_start, str_end) = section_range(image, string_section)?;
    let strings = &image[str_start..str_end];

    let mut symbols = Vec::new();
    for entry in image[sym_start..sym_end].as_chunks::<ELF64_SYM_ENTRY>().0 {
        let name_offset = read_u32(entry, 0x00).ok_or(ElfError::BadSection)?;
        let info = entry.get(4).copied().ok_or(ElfError::BadSection)?;
        let value = read_u64(entry, 0x08).ok_or(ElfError::BadSection)?;
        let size = read_u64(entry, 0x10).ok_or(ElfError::BadSection)?;
        if info & 0x0f != STT_FUNC || size == 0 {
            continue;
        }
        let Ok(offset) = usize::try_from(name_offset) else {
            continue;
        };
        let Some(name) = c_string(strings, offset) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        symbols.push(FunctionSymbol {
            value,
            size,
            name: name.into(),
        });
    }
    if symbols.is_empty() {
        return Err(ElfError::NoSymbols);
    }
    Ok(symbols)
}

fn c_string(strings: &[u8], offset: usize) -> Option<&[u8]> {
    let tail = strings.get(offset..)?;
    let end = tail
        .iter()
        .take(MAX_SYMBOL_NAME_BYTES)
        .position(|&byte| byte == 0)
        .unwrap_or_else(|| tail.len().min(MAX_SYMBOL_NAME_BYTES));
    tail.get(..end)
}

// ---------------------------------------------------------------------------
// Frame naming
// ---------------------------------------------------------------------------

/// The placeholder emitted whenever a frame cannot be attributed to a symbol.
pub const UNKNOWN_FRAME: &str = "[unknown]";
/// Hard cap on the bytes of any emitted frame name.
pub const MAX_FRAME_BYTES: usize = 200;

const fn is_frame_byte(byte: u8) -> bool {
    matches!(byte,
        b'0'..=b'9'
        | b'A'..=b'Z'
        | b'a'..=b'z'
        | b'_' | b'.' | b':' | b'$' | b'<' | b'>' | b',' | b'*' | b'&' | b'[' | b']' | b'+' | b'-')
}

/// Rewrites a raw symbol name into the closed alphabet the folded-stack format
/// permits. Each byte outside `[0-9A-Za-z_.:$<>,*&\[\]+-]` becomes exactly one
/// `_`, and the result is capped at [`MAX_FRAME_BYTES`]. A `;`, an LF and every
/// path separator are outside the alphabet, so neither the folded-stack grammar
/// nor a filesystem path can survive this function.
///
/// The substitution is one for one and nothing is collapsed, so an authentic
/// symbol name that already contains underscores is reproduced verbatim.
#[must_use]
pub fn sanitize_frame(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len().min(MAX_FRAME_BYTES));
    for &byte in raw {
        if out.len() >= MAX_FRAME_BYTES {
            break;
        }
        out.push(char::from(if is_frame_byte(byte) { byte } else { b'_' }));
    }
    if out.is_empty() {
        return UNKNOWN_FRAME.to_owned();
    }
    out
}

/// Turns one instruction pointer into a sanitized frame name.
///
/// `lookup` receives the module's absolute path and the mapping-relative file
/// offset and returns the raw symbol name, if any. Mappings without a usable
/// backing file, addresses outside every known mapping and symbols that cannot
/// be found all collapse to [`UNKNOWN_FRAME`].
pub fn resolve_frame<F>(map: &ModuleMap, addr: u64, mut lookup: F) -> String
where
    F: FnMut(&str, u64) -> Option<Vec<u8>>,
{
    let Some(mapping) = map.find(addr) else {
        return UNKNOWN_FRAME.to_owned();
    };
    let Some(path) = mapping.module_path() else {
        return UNKNOWN_FRAME.to_owned();
    };
    let Some(file_offset) = mapping.file_offset(addr) else {
        return UNKNOWN_FRAME.to_owned();
    };
    lookup(path, file_offset).map_or_else(|| UNKNOWN_FRAME.to_owned(), |name| sanitize_frame(&name))
}

// ---------------------------------------------------------------------------
// Folded stacks
// ---------------------------------------------------------------------------

/// Aggregates samples into the collapsed/folded stack format consumed by
/// flamegraph renderers: `root;next;...;leaf <count>`, one line per distinct
/// stack, sorted byte-lexicographically, LF terminated.
#[derive(Clone, Debug, Default)]
pub struct FoldedStacks {
    counts: BTreeMap<String, u64>,
}

impl FoldedStacks {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one stack given leaf-first frame names (the order perf reports).
    pub fn record(&mut self, frames_leaf_first: &[String], count: u64) {
        let key = fold_key(frames_leaf_first);
        let key = if self.counts.contains_key(&key) || self.counts.len() < MAX_DISTINCT_STACKS {
            key
        } else {
            UNKNOWN_FRAME.to_owned()
        };
        let entry = self.counts.entry(key).or_insert(0);
        *entry = entry.saturating_add(count);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Renders the folded-stack file. `BTreeMap` iteration is already byte
    /// lexicographic over the stack keys, and because `' '` sorts below every
    /// byte of the frame alphabet, that ordering is also the byte-lexicographic
    /// ordering of the rendered lines.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        for (stack, count) in &self.counts {
            out.push_str(stack);
            out.push(' ');
            out.push_str(&count.to_string());
            out.push('\n');
        }
        out
    }
}

fn fold_key(frames_leaf_first: &[String]) -> String {
    let mut key = String::new();
    for frame in frames_leaf_first.iter().rev() {
        if !key.is_empty() {
            key.push(';');
        }
        key.push_str(frame);
    }
    if key.is_empty() {
        key.push_str(UNKNOWN_FRAME);
    }
    key
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

pub const MANIFEST_SCHEMA: &str = "rust-engineering-mcp.profile-helper.v1";

/// Why the profiling run ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProfileStatus {
    /// The child ran to completion inside the window and was sampled.
    Complete,
    /// `--max-samples` was reached; the child was killed.
    SampleLimit,
    /// `--duration-ms` elapsed; the child was killed.
    DurationLimit,
    /// The child was gone before any sample could be taken, or never started.
    ChildExited,
    /// `perf_event_open` (or its ring buffer setup) was denied.
    ProfilerUnavailable,
}

impl ProfileStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::SampleLimit => "sample_limit",
            Self::DurationLimit => "duration_limit",
            Self::ChildExited => "child_exited",
            Self::ProfilerUnavailable => "profiler_unavailable",
        }
    }
}

/// The observations that decide the reported status.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RunOutcome {
    pub profiler_unavailable: bool,
    pub sample_limit_reached: bool,
    pub duration_elapsed: bool,
    pub child_exited_on_its_own: bool,
    pub samples_collected: u64,
}

/// Maps the run's observations onto the closed status enum. The ordering is
/// deliberate: an unavailable profiler outranks everything, a sample cap
/// outranks the clock, and a child that ended by itself is `complete` only if
/// the profiler actually saw it run.
#[must_use]
pub const fn classify_status(outcome: &RunOutcome) -> ProfileStatus {
    if outcome.profiler_unavailable {
        ProfileStatus::ProfilerUnavailable
    } else if outcome.sample_limit_reached {
        ProfileStatus::SampleLimit
    } else if outcome.child_exited_on_its_own {
        if outcome.samples_collected > 0 {
            ProfileStatus::Complete
        } else {
            ProfileStatus::ChildExited
        }
    } else if outcome.duration_elapsed {
        ProfileStatus::DurationLimit
    } else {
        ProfileStatus::ChildExited
    }
}

/// The manifest written to `--manifest`. Every field is computed by the helper;
/// nothing is defaulted at serialization time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Manifest {
    pub status: ProfileStatus,
    pub frequency_hz: u32,
    pub requested_duration_ms: u64,
    pub observed_duration_ms: u64,
    pub samples_collected: u64,
    pub samples_lost: u64,
    pub stacks_written: u64,
    pub frames_total: u64,
    pub frames_unresolved: u64,
    pub stacks_truncated: u64,
    pub max_depth: u32,
    pub modules_seen: u64,
    pub child_exit_code: Option<i32>,
    pub child_signal: Option<i32>,
    pub perf_errno: Option<i32>,
}

/// Hand-rolled serializer for the v1 manifest. The key order is fixed, every
/// key is always present, numbers are plain JSON numbers and absent optional
/// numbers are `null`. No caller-supplied text ever reaches this function, so
/// no escaping is required or performed.
#[must_use]
pub fn render_manifest(manifest: &Manifest) -> String {
    let mut out = String::with_capacity(512);
    out.push_str("{\"schema\":\"");
    out.push_str(MANIFEST_SCHEMA);
    out.push_str("\",\"status\":\"");
    out.push_str(manifest.status.as_str());
    out.push_str("\",\"frequency_hz\":");
    out.push_str(&manifest.frequency_hz.to_string());
    out.push_str(",\"requested_duration_ms\":");
    out.push_str(&manifest.requested_duration_ms.to_string());
    out.push_str(",\"observed_duration_ms\":");
    out.push_str(&manifest.observed_duration_ms.to_string());
    out.push_str(",\"samples_collected\":");
    out.push_str(&manifest.samples_collected.to_string());
    out.push_str(",\"samples_lost\":");
    out.push_str(&manifest.samples_lost.to_string());
    out.push_str(",\"stacks_written\":");
    out.push_str(&manifest.stacks_written.to_string());
    out.push_str(",\"frames_total\":");
    out.push_str(&manifest.frames_total.to_string());
    out.push_str(",\"frames_unresolved\":");
    out.push_str(&manifest.frames_unresolved.to_string());
    out.push_str(",\"stacks_truncated\":");
    out.push_str(&manifest.stacks_truncated.to_string());
    out.push_str(",\"max_depth\":");
    out.push_str(&manifest.max_depth.to_string());
    out.push_str(",\"modules_seen\":");
    out.push_str(&manifest.modules_seen.to_string());
    out.push_str(",\"child_exit_code\":");
    out.push_str(&optional_number(manifest.child_exit_code));
    out.push_str(",\"child_signal\":");
    out.push_str(&optional_number(manifest.child_signal));
    out.push_str(",\"perf_errno\":");
    out.push_str(&optional_number(manifest.perf_errno));
    out.push_str("}\n");
    out
}

fn optional_number(value: Option<i32>) -> String {
    value.map_or_else(|| "null".to_owned(), |number| number.to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<OsString> {
        items.iter().map(OsString::from).collect()
    }

    fn accepted() -> Vec<OsString> {
        args(&[
            "--frequency-hz",
            "99",
            "--duration-ms",
            "5000",
            "--max-samples",
            "10000",
            "--max-depth",
            "128",
            "--stacks",
            "/out/stacks.folded",
            "--manifest",
            "/out/manifest.json",
            "--",
            "/usr/bin/true",
            "--flag",
            "value",
        ])
    }

    fn without(flag: &str) -> Vec<OsString> {
        let mut out = Vec::new();
        let mut items = accepted().into_iter();
        while let Some(item) = items.next() {
            if item == flag {
                let _ = items.next();
                continue;
            }
            out.push(item);
        }
        out
    }

    fn replace_value(flag: &str, value: &str) -> Vec<OsString> {
        let mut out = Vec::new();
        let mut items = accepted().into_iter();
        while let Some(item) = items.next() {
            out.push(item.clone());
            if item == flag {
                let _ = items.next();
                out.push(OsString::from(value));
            }
        }
        out
    }

    // -- argument parsing ---------------------------------------------------

    #[test]
    fn accepts_the_exact_contract() {
        let parsed = parse_arguments(accepted()).unwrap();
        assert_eq!(parsed.frequency_hz, 99);
        assert_eq!(parsed.duration_ms, 5000);
        assert_eq!(parsed.max_samples, 10_000);
        assert_eq!(parsed.max_depth, 128);
        assert_eq!(parsed.stacks_path, PathBuf::from("/out/stacks.folded"));
        assert_eq!(parsed.manifest_path, PathBuf::from("/out/manifest.json"));
        assert_eq!(parsed.program, PathBuf::from("/usr/bin/true"));
        assert_eq!(parsed.program_args, args(&["--flag", "value"]));
    }

    #[test]
    fn accepts_a_program_with_no_arguments_of_its_own() {
        let mut items = accepted();
        items.truncate(items.len() - 2);
        let parsed = parse_arguments(items).unwrap();
        assert!(parsed.program_args.is_empty());
    }

    #[test]
    fn rejects_unknown_flags() {
        let mut items = accepted();
        items.insert(0, OsString::from("--verbose"));
        assert_eq!(parse_arguments(items), Err(ArgumentError::UnknownArgument));
        // The `--flag=value` spelling is not part of the closed contract.
        assert_eq!(
            parse_arguments(replace_flag_spelling()),
            Err(ArgumentError::UnknownArgument)
        );
    }

    fn replace_flag_spelling() -> Vec<OsString> {
        let mut items = without("--frequency-hz");
        items.insert(0, OsString::from("--frequency-hz=99"));
        items
    }

    #[test]
    fn rejects_every_missing_flag() {
        for flag in [
            "--frequency-hz",
            "--duration-ms",
            "--max-samples",
            "--max-depth",
            "--stacks",
            "--manifest",
        ] {
            assert_eq!(
                parse_arguments(without(flag)),
                Err(ArgumentError::MissingFlag),
                "flag {flag} should be required"
            );
        }
    }

    #[test]
    fn rejects_repeated_flags() {
        let mut items = accepted();
        items.insert(0, OsString::from("77"));
        items.insert(0, OsString::from("--frequency-hz"));
        assert_eq!(parse_arguments(items), Err(ArgumentError::DuplicateFlag));
    }

    #[test]
    fn rejects_a_flag_without_a_value() {
        let items = args(&["--frequency-hz"]);
        assert_eq!(parse_arguments(items), Err(ArgumentError::MissingValue));
    }

    #[test]
    fn rejects_non_numeric_values() {
        for value in ["", "abc", "-1", "+1", "1.5", " 1", "0x10"] {
            assert_eq!(
                parse_arguments(replace_value("--frequency-hz", value)),
                Err(ArgumentError::NotANumber),
                "value {value:?} should not parse"
            );
        }
    }

    #[test]
    fn rejects_out_of_range_values() {
        for (flag, value) in [
            ("--frequency-hz", "0"),
            ("--frequency-hz", "1000"),
            ("--duration-ms", "99"),
            ("--duration-ms", "60001"),
            ("--max-samples", "0"),
            ("--max-samples", "2000001"),
            ("--max-depth", "0"),
            ("--max-depth", "257"),
            ("--frequency-hz", "99999999999999999999999"),
        ] {
            assert_eq!(
                parse_arguments(replace_value(flag, value)),
                Err(ArgumentError::OutOfRange),
                "{flag} {value} should be out of range"
            );
        }
    }

    #[test]
    fn accepts_the_inclusive_range_endpoints() {
        for (flag, value) in [
            ("--frequency-hz", "1"),
            ("--frequency-hz", "999"),
            ("--duration-ms", "100"),
            ("--duration-ms", "60000"),
            ("--max-samples", "1"),
            ("--max-samples", "2000000"),
            ("--max-depth", "1"),
            ("--max-depth", "256"),
        ] {
            assert!(
                parse_arguments(replace_value(flag, value)).is_ok(),
                "{flag} {value} should be accepted"
            );
        }
    }

    #[test]
    fn rejects_an_empty_output_path() {
        assert_eq!(
            parse_arguments(replace_value("--stacks", "")),
            Err(ArgumentError::EmptyPath)
        );
        assert_eq!(
            parse_arguments(replace_value("--manifest", "")),
            Err(ArgumentError::EmptyPath)
        );
    }

    #[test]
    fn rejects_a_separator_with_no_program() {
        let mut items = accepted();
        items.truncate(items.len() - 3);
        assert_eq!(items.last(), Some(&OsString::from("--")));
        assert_eq!(parse_arguments(items), Err(ArgumentError::MissingProgram));
    }

    #[test]
    fn rejects_a_missing_separator() {
        let mut items = accepted();
        items.truncate(items.len() - 4);
        assert_ne!(items.last(), Some(&OsString::from("--")));
        assert_eq!(parse_arguments(items), Err(ArgumentError::MissingProgram));
    }

    #[test]
    fn rejects_a_relative_program_path() {
        for program in ["true", "./true", "../bin/true", ""] {
            let mut items = accepted();
            items.truncate(items.len() - 3);
            items.push(OsString::from("--"));
            items.push(OsString::from(program));
            assert_eq!(
                parse_arguments(items),
                Err(ArgumentError::ProgramNotAbsolute),
                "program {program:?} should be rejected"
            );
        }
    }

    #[test]
    fn every_rejection_has_a_single_line_message() {
        for error in [
            ArgumentError::UnknownArgument,
            ArgumentError::DuplicateFlag,
            ArgumentError::MissingFlag,
            ArgumentError::MissingValue,
            ArgumentError::NonUtf8Value,
            ArgumentError::NotANumber,
            ArgumentError::OutOfRange,
            ArgumentError::EmptyPath,
            ArgumentError::MissingProgram,
            ArgumentError::ProgramNotAbsolute,
        ] {
            let message = error.message();
            assert!(!message.is_empty());
            assert!(!message.contains('\n'), "{message:?} must be one line");
        }
    }

    // -- ring buffer decoding ----------------------------------------------

    fn header(kind: u32, misc: u16, size: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&misc.to_le_bytes());
        out.extend_from_slice(&size.to_le_bytes());
        out
    }

    fn lost_record(lost: u64) -> Vec<u8> {
        let mut out = header(PERF_RECORD_LOST, 0, 24);
        out.extend_from_slice(&7u64.to_le_bytes());
        out.extend_from_slice(&lost.to_le_bytes());
        out
    }

    fn sample_record(ip: u64, callchain: &[u64]) -> Vec<u8> {
        let size = 8 + 8 + 4 + 4 + 8 + 8 + callchain.len() * 8;
        let mut out = header(PERF_RECORD_SAMPLE, 0, u16::try_from(size).unwrap());
        out.extend_from_slice(&ip.to_le_bytes());
        out.extend_from_slice(&11u32.to_le_bytes());
        out.extend_from_slice(&12u32.to_le_bytes());
        out.extend_from_slice(&99u64.to_le_bytes());
        out.extend_from_slice(&(callchain.len() as u64).to_le_bytes());
        for entry in callchain {
            out.extend_from_slice(&entry.to_le_bytes());
        }
        out
    }

    fn mmap_record(addr: u64, len: u64, pgoff: u64, name: &str, misc: u16) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&1u32.to_le_bytes());
        body.extend_from_slice(&1u32.to_le_bytes());
        body.extend_from_slice(&addr.to_le_bytes());
        body.extend_from_slice(&len.to_le_bytes());
        body.extend_from_slice(&pgoff.to_le_bytes());
        body.extend_from_slice(name.as_bytes());
        body.push(0);
        while body.len() % 8 != 0 {
            body.push(0);
        }
        let size = u16::try_from(8 + body.len()).unwrap();
        let mut out = header(PERF_RECORD_MMAP, misc, size);
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn decodes_a_lost_record() {
        let mut data = vec![0u8; 64];
        let record = lost_record(42);
        data[..record.len()].copy_from_slice(&record);
        let drained = drain_ring(&data, 0, record.len() as u64);
        assert_eq!(drained.records, vec![Record::Lost(42)]);
        assert_eq!(drained.tail, record.len() as u64);
        assert!(!drained.overrun);
    }

    #[test]
    fn decodes_a_sample_and_skips_context_markers() {
        let chain = [PERF_CONTEXT_USER, 0x1000, 0x2000, PERF_CONTEXT_MIN, 0x3000];
        let record = sample_record(0x1000, &chain);
        let mut data = vec![0u8; 128];
        data[..record.len()].copy_from_slice(&record);
        let drained = drain_ring(&data, 0, record.len() as u64);
        let Some(Record::Sample(sample)) = drained.records.first() else {
            panic!("expected a sample, got {:?}", drained.records);
        };
        assert_eq!(sample.ip, 0x1000);
        assert_eq!(sample.pid, 11);
        assert_eq!(sample.tid, 12);
        assert_eq!(sample.time, 99);
        assert_eq!(sample.callchain, chain);

        // 0xffff_ffff_ffff_f000 is a marker that is not PERF_CONTEXT_USER, so
        // the frame behind it is dropped as non-user-space.
        let stack = user_frames(sample, 16);
        assert_eq!(stack.frames, vec![0x1000, 0x2000]);
        assert!(!stack.truncated);
    }

    #[test]
    fn truncates_a_callchain_to_max_depth() {
        let chain = [PERF_CONTEXT_USER, 0x10, 0x20, 0x30, 0x40];
        let record = sample_record(0x10, &chain);
        let mut data = vec![0u8; 128];
        data[..record.len()].copy_from_slice(&record);
        let drained = drain_ring(&data, 0, record.len() as u64);
        let Some(Record::Sample(sample)) = drained.records.first() else {
            panic!("expected a sample");
        };
        let stack = user_frames(sample, 2);
        assert_eq!(stack.frames, vec![0x10, 0x20]);
        assert!(stack.truncated);
    }

    #[test]
    fn falls_back_to_the_instruction_pointer_for_an_empty_callchain() {
        let sample = SampleRecord {
            ip: 0x4321,
            pid: 1,
            tid: 1,
            time: 0,
            callchain: vec![PERF_CONTEXT_USER],
        };
        let stack = user_frames(&sample, 8);
        assert_eq!(stack.frames, vec![0x4321]);
        assert!(!stack.truncated);
    }

    #[test]
    fn decodes_a_record_that_wraps_the_ring_end() {
        let data_len = 64usize;
        let record = lost_record(5);
        assert_eq!(record.len(), 24);
        let tail = 56u64;
        let head = tail + record.len() as u64;
        let mut data = vec![0u8; data_len];
        let start = (tail % data_len as u64) as usize;
        let first = data_len - start;
        data[start..].copy_from_slice(&record[..first]);
        data[..record.len() - first].copy_from_slice(&record[first..]);

        let drained = drain_ring(&data, tail, head);
        assert_eq!(drained.records, vec![Record::Lost(5)]);
        assert_eq!(drained.tail, head);
    }

    #[test]
    fn leaves_a_truncated_record_unconsumed() {
        let mut data = vec![0u8; 64];
        let record = lost_record(1);
        data[..record.len()].copy_from_slice(&record);
        // The writer has only published 16 of the record's 24 bytes.
        let drained = drain_ring(&data, 0, 16);
        assert!(drained.records.is_empty());
        assert_eq!(drained.tail, 0);

        // A header that does not even fit is also left alone.
        let drained = drain_ring(&data, 0, 4);
        assert!(drained.records.is_empty());
        assert_eq!(drained.tail, 0);
    }

    #[test]
    fn decodes_several_records_in_one_pass() {
        let mut data = vec![0u8; 256];
        let mut blob = Vec::new();
        blob.extend_from_slice(&mmap_record(0x1000, 0x100, 0, "/lib/libx.so", 0));
        blob.extend_from_slice(&lost_record(3));
        blob.extend_from_slice(&sample_record(0x1000, &[PERF_CONTEXT_USER, 0x1000]));
        data[..blob.len()].copy_from_slice(&blob);

        let drained = drain_ring(&data, 0, blob.len() as u64);
        assert_eq!(drained.records.len(), 3);
        assert_eq!(drained.tail, blob.len() as u64);
        let Some(Record::Mapping(mapping)) = drained.records.first() else {
            panic!("expected a mapping");
        };
        assert_eq!(mapping.addr, 0x1000);
        assert_eq!(mapping.len, 0x100);
        assert_eq!(mapping.filename, b"/lib/libx.so");
        assert!(mapping.executable);
    }

    #[test]
    fn treats_a_data_mapping_as_non_executable() {
        let record = mmap_record(0x1000, 0x100, 0, "/lib/libx.so", PERF_RECORD_MISC_MMAP_DATA);
        let mut data = vec![0u8; 128];
        data[..record.len()].copy_from_slice(&record);
        let drained = drain_ring(&data, 0, record.len() as u64);
        let Some(Record::Mapping(mapping)) = drained.records.first() else {
            panic!("expected a mapping");
        };
        assert!(!mapping.executable);

        let mut map = ModuleMap::new();
        map.insert(mapping.clone());
        assert!(map.is_empty());
        assert_eq!(map.modules_seen(), 0);
    }

    #[test]
    fn resynchronises_when_the_writer_laps_the_reader() {
        let data = vec![0u8; 64];
        let drained = drain_ring(&data, 0, 4096);
        assert!(drained.records.is_empty());
        assert!(drained.overrun);
        assert_eq!(drained.tail, 4096);
    }

    #[test]
    fn ignores_an_empty_window() {
        let data = vec![0u8; 64];
        let drained = drain_ring(&data, 128, 128);
        assert!(drained.records.is_empty());
        assert_eq!(drained.tail, 128);
    }

    // -- ELF ---------------------------------------------------------------

    const IMAGE_BYTES: usize = 0x1000;
    const SYMTAB_OFFSET: usize = 0x300;
    const STRTAB_OFFSET: usize = 0x200;
    const SHDR_OFFSET: usize = 0x400;
    const LOAD_VADDR: u64 = 0x0040_0000;

    fn put_u16(image: &mut [u8], offset: usize, value: u16) {
        image[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u32(image: &mut [u8], offset: usize, value: u32) {
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn put_u64(image: &mut [u8], offset: usize, value: u64) {
        image[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }

    /// A minimal but structurally valid ELF64 LSB image: one `PT_LOAD`, a
    /// `.symtab` with two `STT_FUNC` symbols and its `.strtab`.
    fn synthetic_elf() -> Vec<u8> {
        let mut image = vec![0u8; IMAGE_BYTES];
        image[..4].copy_from_slice(b"\x7fELF");
        image[4] = 2; // ELFCLASS64
        image[5] = 1; // ELFDATA2LSB
        image[6] = 1; // EV_CURRENT
        put_u16(&mut image, 0x10, 3); // ET_DYN
        put_u16(&mut image, 0x12, 183); // EM_AARCH64
        put_u64(&mut image, 0x20, 64); // e_phoff
        put_u64(&mut image, 0x28, SHDR_OFFSET as u64); // e_shoff
        put_u16(&mut image, 0x34, 64); // e_ehsize
        put_u16(&mut image, 0x36, 56); // e_phentsize
        put_u16(&mut image, 0x38, 1); // e_phnum
        put_u16(&mut image, 0x3a, 64); // e_shentsize
        put_u16(&mut image, 0x3c, 3); // e_shnum
        put_u16(&mut image, 0x3e, 0); // e_shstrndx

        // Program header 0: PT_LOAD covering the whole image.
        put_u32(&mut image, 64, PT_LOAD);
        put_u32(&mut image, 68, 5); // PF_R | PF_X
        put_u64(&mut image, 72, 0); // p_offset
        put_u64(&mut image, 80, LOAD_VADDR); // p_vaddr
        put_u64(&mut image, 88, LOAD_VADDR); // p_paddr
        put_u64(&mut image, 96, IMAGE_BYTES as u64); // p_filesz
        put_u64(&mut image, 104, IMAGE_BYTES as u64); // p_memsz

        // .strtab payload.
        let strings: &[u8] = b"\0alpha\0beta_symbol\0";
        image[STRTAB_OFFSET..STRTAB_OFFSET + strings.len()].copy_from_slice(strings);

        // .symtab payload: alpha at +0x100 (0x40 bytes), beta at +0x200 (0x30).
        put_u32(&mut image, SYMTAB_OFFSET, 1);
        image[SYMTAB_OFFSET + 4] = STT_FUNC;
        put_u16(&mut image, SYMTAB_OFFSET + 6, 1);
        put_u64(&mut image, SYMTAB_OFFSET + 8, LOAD_VADDR + 0x100);
        put_u64(&mut image, SYMTAB_OFFSET + 16, 0x40);

        put_u32(&mut image, SYMTAB_OFFSET + 24, 7);
        image[SYMTAB_OFFSET + 28] = STT_FUNC;
        put_u16(&mut image, SYMTAB_OFFSET + 30, 1);
        put_u64(&mut image, SYMTAB_OFFSET + 32, LOAD_VADDR + 0x200);
        put_u64(&mut image, SYMTAB_OFFSET + 40, 0x30);

        // Section 0: SHT_NULL (already zeroed).
        // Section 1: .symtab, linked to section 2.
        let symtab = SHDR_OFFSET + 64;
        put_u32(&mut image, symtab + 0x04, SHT_SYMTAB);
        put_u64(&mut image, symtab + 0x18, SYMTAB_OFFSET as u64);
        put_u64(&mut image, symtab + 0x20, 2 * ELF64_SYM_BYTES);
        put_u32(&mut image, symtab + 0x28, 2);
        put_u64(&mut image, symtab + 0x38, ELF64_SYM_BYTES);

        // Section 2: .strtab.
        let strtab = SHDR_OFFSET + 128;
        put_u32(&mut image, strtab + 0x04, SHT_STRTAB);
        put_u64(&mut image, strtab + 0x18, STRTAB_OFFSET as u64);
        put_u64(&mut image, strtab + 0x20, strings.len() as u64);

        image
    }

    #[test]
    fn resolves_symbols_from_a_synthetic_elf() {
        let table = parse_elf_symbols(&synthetic_elf()).unwrap();
        assert_eq!(table.symbol_count(), 2);
        assert_eq!(table.segments().len(), 1);
        assert_eq!(table.vaddr_for_offset(0x100), Some(LOAD_VADDR + 0x100));

        assert_eq!(table.resolve(0x100), Some(b"alpha".as_slice()));
        assert_eq!(table.resolve(0x13f), Some(b"alpha".as_slice()));
        assert_eq!(table.resolve(0x200), Some(b"beta_symbol".as_slice()));
        assert_eq!(table.resolve(0x22f), Some(b"beta_symbol".as_slice()));
    }

    #[test]
    fn misses_just_past_the_end_of_a_symbol() {
        let table = parse_elf_symbols(&synthetic_elf()).unwrap();
        assert_eq!(table.resolve(0x140), None);
        assert_eq!(table.resolve(0x0ff), None);
        assert_eq!(table.resolve(0x230), None);
        // Outside every PT_LOAD segment.
        assert_eq!(table.vaddr_for_offset(IMAGE_BYTES as u64), None);
        assert_eq!(table.resolve(IMAGE_BYTES as u64), None);
    }

    #[test]
    fn prefers_symtab_and_falls_back_to_dynsym() {
        let mut image = synthetic_elf();
        put_u32(&mut image, SHDR_OFFSET + 64 + 0x04, SHT_DYNSYM);
        let table = parse_elf_symbols(&image).unwrap();
        assert_eq!(table.resolve(0x100), Some(b"alpha".as_slice()));
    }

    #[test]
    fn rejects_malformed_elf_images() {
        assert_eq!(parse_elf_symbols(&[]), Err(ElfError::TooSmall));
        assert_eq!(parse_elf_symbols(&[0u8; 63]), Err(ElfError::TooSmall));

        let mut image = synthetic_elf();
        image[1] = b'X';
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadMagic));

        let mut image = synthetic_elf();
        image[4] = 1; // ELFCLASS32
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::UnsupportedClass));

        let mut image = synthetic_elf();
        image[5] = 2; // ELFDATA2MSB
        assert_eq!(
            parse_elf_symbols(&image),
            Err(ElfError::UnsupportedEndianness)
        );

        // Section header table past the end of the image.
        let mut image = synthetic_elf();
        put_u64(&mut image, 0x28, 0xffff_0000);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadHeaderTable));

        // Absurd section count.
        let mut image = synthetic_elf();
        put_u16(&mut image, 0x3c, u16::MAX);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadHeaderTable));

        // Absurd program header count.
        let mut image = synthetic_elf();
        put_u16(&mut image, 0x38, u16::MAX);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadHeaderTable));

        // Wrong entry sizes.
        let mut image = synthetic_elf();
        put_u16(&mut image, 0x36, 32);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadHeaderTable));

        // .symtab payload outside the image.
        let mut image = synthetic_elf();
        put_u64(&mut image, SHDR_OFFSET + 64 + 0x18, 0xffff_0000);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadSection));

        // .symtab size outside the image.
        let mut image = synthetic_elf();
        put_u64(&mut image, SHDR_OFFSET + 64 + 0x20, u64::MAX);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadSection));

        // .symtab linked to a section index that does not exist.
        let mut image = synthetic_elf();
        put_u32(&mut image, SHDR_OFFSET + 64 + 0x28, 99);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadSection));

        // .symtab linked to something that is not a string table.
        let mut image = synthetic_elf();
        put_u32(&mut image, SHDR_OFFSET + 128 + 0x04, SHT_SYMTAB);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadSection));

        // Wrong symbol entry size.
        let mut image = synthetic_elf();
        put_u64(&mut image, SHDR_OFFSET + 64 + 0x38, 16);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::BadSection));

        // No symbol sections at all.
        let mut image = synthetic_elf();
        put_u32(&mut image, SHDR_OFFSET + 64 + 0x04, 0);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::NoSymbols));

        // No PT_LOAD segments.
        let mut image = synthetic_elf();
        put_u32(&mut image, 64, 0);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::NoLoadSegments));
    }

    #[test]
    fn skips_symbols_that_are_not_sized_functions() {
        let mut image = synthetic_elf();
        // STT_OBJECT for alpha, zero size for beta: nothing usable is left.
        image[SYMTAB_OFFSET + 4] = 1;
        put_u64(&mut image, SYMTAB_OFFSET + 40, 0);
        assert_eq!(parse_elf_symbols(&image), Err(ElfError::NoSymbols));
    }

    // -- frame naming -------------------------------------------------------

    #[test]
    fn sanitizes_frame_names() {
        assert_eq!(sanitize_frame(b"alpha"), "alpha");
        assert_eq!(
            sanitize_frame(b"core::ptr::drop_in_place<T>"),
            "core::ptr::drop_in_place<T>"
        );
        assert_eq!(sanitize_frame(b"a;b"), "a_b");
        assert_eq!(sanitize_frame(b"a\nb"), "a_b");
        assert_eq!(sanitize_frame(b"a b"), "a_b");
        assert_eq!(sanitize_frame(b"a   b"), "a___b");
        assert_eq!(sanitize_frame(b"a\x00\x01\x02b"), "a___b");
        // `\u{e9}` is two UTF-8 bytes, so it yields two underscores.
        assert_eq!(sanitize_frame("caf\u{e9}".as_bytes()), "caf__");
        assert_eq!(sanitize_frame(b""), UNKNOWN_FRAME);
        assert_eq!(sanitize_frame(b"   "), "___");
        assert_eq!(sanitize_frame(UNKNOWN_FRAME.as_bytes()), UNKNOWN_FRAME);
    }

    #[test]
    fn authentic_underscores_survive_verbatim() {
        // Substitution is one for one, so a real symbol whose name already
        // contains underscore runs is reproduced exactly.
        assert_eq!(sanitize_frame(b"__libc_start_main"), "__libc_start_main");
        assert_eq!(
            sanitize_frame(b"_ZN4core3ptr13drop_in_place17h0a1bE"),
            "_ZN4core3ptr13drop_in_place17h0a1bE"
        );
        assert_eq!(
            sanitize_frame(b"__rust_begin_short_backtrace"),
            "__rust_begin_short_backtrace"
        );
        assert_eq!(sanitize_frame(b"___triple"), "___triple");
    }

    #[test]
    fn adjacent_disallowed_bytes_yield_one_underscore_each() {
        assert_eq!(sanitize_frame(b"a;;b"), "a__b");
        assert_eq!(sanitize_frame(b"a\r\nb"), "a__b");
        // A disallowed byte next to an authentic underscore keeps both.
        assert_eq!(sanitize_frame(b"a_;b"), "a__b");
        assert_eq!(sanitize_frame(b"a;_b"), "a__b");
        // The output length always equals the input length below the cap.
        for raw in [b"a;;b".as_slice(), b"////".as_slice(), b"a_ _b".as_slice()] {
            assert_eq!(sanitize_frame(raw).len(), raw.len(), "{raw:?}");
        }
    }

    #[test]
    fn sanitization_erases_paths_and_never_emits_the_folded_grammar() {
        let path = sanitize_frame(b"/usr/lib/aarch64-linux-gnu/libc.so.6");
        assert_eq!(path, "_usr_lib_aarch64-linux-gnu_libc.so.6");
        assert!(!path.contains('/'));

        for raw in [
            b"/etc/passwd".as_slice(),
            b"a;b;c".as_slice(),
            b"line\nline".as_slice(),
            b"tab\there".as_slice(),
            "\u{1f600}".as_bytes(),
        ] {
            let frame = sanitize_frame(raw);
            assert!(!frame.contains(';'), "{frame:?}");
            assert!(!frame.contains('\n'), "{frame:?}");
            assert!(!frame.contains('/'), "{frame:?}");
            assert!(frame.bytes().all(is_frame_byte), "{frame:?}");
        }
    }

    #[test]
    fn caps_over_long_frame_names() {
        let long = vec![b'a'; 4096];
        let frame = sanitize_frame(&long);
        assert_eq!(frame.len(), MAX_FRAME_BYTES);
        assert!(frame.bytes().all(|byte| byte == b'a'));

        // Substitution never changes the byte count, so an all-disallowed name
        // is capped at exactly the same length as an all-allowed one.
        let mut noisy = Vec::new();
        for _ in 0..4096 {
            noisy.extend_from_slice(b"x   ");
        }
        let frame = sanitize_frame(&noisy);
        assert_eq!(frame.len(), MAX_FRAME_BYTES);
        assert_eq!(sanitize_frame(&vec![b'/'; 4096]).len(), MAX_FRAME_BYTES);
    }

    fn mapping(addr: u64, len: u64, pgoff: u64, name: &[u8]) -> MappingRecord {
        MappingRecord {
            pid: 1,
            tid: 1,
            addr,
            len,
            pgoff,
            filename: name.to_vec(),
            executable: true,
        }
    }

    #[test]
    fn resolves_frames_through_the_module_map() {
        let mut map = ModuleMap::new();
        map.insert(mapping(0x1_0000, 0x1000, 0x200, b"/opt/app/bin/demo"));
        map.insert(mapping(0x2_0000, 0x1000, 0, b"[vdso]"));
        map.insert(mapping(0x3_0000, 0x1000, 0, b"relative/module.so"));
        map.insert(mapping(0x4_0000, 0x1000, 0, b"/not\xffutf8"));
        assert_eq!(map.modules_seen(), 4);

        let mut seen = Vec::new();
        let frame = resolve_frame(&map, 0x1_0010, |path, offset| {
            seen.push((path.to_owned(), offset));
            Some(b"demo::main".to_vec())
        });
        assert_eq!(frame, "demo::main");
        assert_eq!(seen, vec![("/opt/app/bin/demo".to_owned(), 0x210)]);

        // Bracketed, relative and non-UTF-8 names never reach the symbolizer.
        assert_eq!(
            resolve_frame(&map, 0x2_0010, |_, _| Some(b"x".to_vec())),
            UNKNOWN_FRAME
        );
        assert_eq!(
            resolve_frame(&map, 0x3_0010, |_, _| Some(b"x".to_vec())),
            UNKNOWN_FRAME
        );
        assert_eq!(
            resolve_frame(&map, 0x4_0010, |_, _| Some(b"x".to_vec())),
            UNKNOWN_FRAME
        );
        // Outside every mapping.
        assert_eq!(
            resolve_frame(&map, 0x9_0000, |_, _| Some(b"x".to_vec())),
            UNKNOWN_FRAME
        );
        // Inside a mapping but unresolved by the symbol table.
        assert_eq!(resolve_frame(&map, 0x1_0010, |_, _| None), UNKNOWN_FRAME);
    }

    #[test]
    fn newer_mappings_win_over_the_regions_they_replace() {
        let mut map = ModuleMap::new();
        map.insert(mapping(0x1_0000, 0x1000, 0, b"/old"));
        map.insert(mapping(0x1_0000, 0x1000, 0, b"/new"));
        let mapping = map.find(0x1_0004).unwrap();
        assert_eq!(mapping.filename, b"/new");
        assert_eq!(map.len(), 2);
    }

    // -- folded stacks ------------------------------------------------------

    fn frames(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn folds_and_sorts_stacks_deterministically() {
        let mut folded = FoldedStacks::new();
        // perf reports leaf first; the folded format is root first.
        folded.record(&frames(&["leaf", "middle", "root"]), 1);
        folded.record(&frames(&["leaf", "middle", "root"]), 1);
        folded.record(&frames(&["other", "root"]), 3);
        folded.record(&frames(&["root"]), 1);
        assert_eq!(folded.len(), 3);
        assert_eq!(
            folded.render(),
            "root 1\nroot;middle;leaf 2\nroot;other 3\n"
        );
    }

    #[test]
    fn folding_is_order_independent() {
        let mut first = FoldedStacks::new();
        first.record(&frames(&["b"]), 1);
        first.record(&frames(&["a"]), 1);
        first.record(&frames(&["b"]), 1);

        let mut second = FoldedStacks::new();
        second.record(&frames(&["b"]), 2);
        second.record(&frames(&["a"]), 1);

        assert_eq!(first.render(), second.render());
        assert_eq!(first.render(), "a 1\nb 2\n");
    }

    #[test]
    fn folded_lines_sort_byte_lexicographically() {
        let mut folded = FoldedStacks::new();
        for stack in [
            vec!["a"],
            vec!["b", "a"],
            vec!["Z"],
            vec!["a", "a"],
            vec!["0"],
        ] {
            folded.record(&frames(&stack), 1);
        }
        let rendered = folded.render();
        let mut lines: Vec<&str> = rendered.lines().collect();
        let original = lines.clone();
        lines.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        assert_eq!(lines, original);
        assert!(rendered.ends_with('\n'));
    }

    #[test]
    fn an_empty_stack_becomes_the_unknown_frame() {
        let mut folded = FoldedStacks::new();
        folded.record(&[], 4);
        assert_eq!(folded.render(), "[unknown] 4\n");
    }

    #[test]
    fn an_empty_aggregator_renders_nothing() {
        assert!(FoldedStacks::new().is_empty());
        assert_eq!(FoldedStacks::new().render(), "");
    }

    // -- manifest -----------------------------------------------------------

    fn manifest() -> Manifest {
        Manifest {
            status: ProfileStatus::Complete,
            frequency_hz: 99,
            requested_duration_ms: 5000,
            observed_duration_ms: 1234,
            samples_collected: 421,
            samples_lost: 2,
            stacks_written: 37,
            frames_total: 900,
            frames_unresolved: 11,
            stacks_truncated: 3,
            max_depth: 128,
            modules_seen: 5,
            child_exit_code: Some(0),
            child_signal: None,
            perf_errno: None,
        }
    }

    #[test]
    fn serializes_the_manifest_in_a_fixed_key_order() {
        assert_eq!(
            render_manifest(&manifest()),
            concat!(
                "{\"schema\":\"rust-engineering-mcp.profile-helper.v1\",",
                "\"status\":\"complete\",",
                "\"frequency_hz\":99,",
                "\"requested_duration_ms\":5000,",
                "\"observed_duration_ms\":1234,",
                "\"samples_collected\":421,",
                "\"samples_lost\":2,",
                "\"stacks_written\":37,",
                "\"frames_total\":900,",
                "\"frames_unresolved\":11,",
                "\"stacks_truncated\":3,",
                "\"max_depth\":128,",
                "\"modules_seen\":5,",
                "\"child_exit_code\":0,",
                "\"child_signal\":null,",
                "\"perf_errno\":null}\n",
            )
        );
    }

    #[test]
    fn serializes_nulls_for_absent_optional_numbers() {
        let mut absent = manifest();
        absent.child_exit_code = None;
        let rendered = render_manifest(&absent);
        assert!(rendered.contains("\"child_exit_code\":null,"));
        assert!(rendered.contains("\"child_signal\":null,"));
        assert!(rendered.contains("\"perf_errno\":null}"));

        let mut present = manifest();
        present.child_exit_code = None;
        present.child_signal = Some(9);
        present.perf_errno = Some(13);
        let rendered = render_manifest(&present);
        assert!(rendered.contains("\"child_exit_code\":null,"));
        assert!(rendered.contains("\"child_signal\":9,"));
        assert!(rendered.contains("\"perf_errno\":13}"));
    }

    #[test]
    fn the_unavailable_manifest_carries_the_errno_and_no_samples() {
        let unavailable = Manifest {
            status: ProfileStatus::ProfilerUnavailable,
            frequency_hz: 99,
            requested_duration_ms: 5000,
            observed_duration_ms: 0,
            samples_collected: 0,
            samples_lost: 0,
            stacks_written: 0,
            frames_total: 0,
            frames_unresolved: 0,
            stacks_truncated: 0,
            max_depth: 128,
            modules_seen: 0,
            child_exit_code: None,
            child_signal: Some(9),
            perf_errno: Some(1),
        };
        let rendered = render_manifest(&unavailable);
        assert!(rendered.contains("\"status\":\"profiler_unavailable\""));
        assert!(rendered.contains("\"perf_errno\":1}"));
        assert!(rendered.contains("\"samples_collected\":0,"));
    }

    #[test]
    fn every_status_spelling_is_reachable() {
        let base = RunOutcome::default();
        assert_eq!(
            classify_status(&RunOutcome {
                profiler_unavailable: true,
                ..base
            }),
            ProfileStatus::ProfilerUnavailable
        );
        assert_eq!(
            classify_status(&RunOutcome {
                sample_limit_reached: true,
                samples_collected: 10,
                ..base
            }),
            ProfileStatus::SampleLimit
        );
        assert_eq!(
            classify_status(&RunOutcome {
                duration_elapsed: true,
                samples_collected: 10,
                ..base
            }),
            ProfileStatus::DurationLimit
        );
        assert_eq!(
            classify_status(&RunOutcome {
                child_exited_on_its_own: true,
                samples_collected: 10,
                ..base
            }),
            ProfileStatus::Complete
        );
        assert_eq!(
            classify_status(&RunOutcome {
                child_exited_on_its_own: true,
                samples_collected: 0,
                ..base
            }),
            ProfileStatus::ChildExited
        );
        assert_eq!(classify_status(&base), ProfileStatus::ChildExited);

        // The sample cap outranks a child that also finished on its own.
        assert_eq!(
            classify_status(&RunOutcome {
                sample_limit_reached: true,
                child_exited_on_its_own: true,
                duration_elapsed: true,
                samples_collected: 10,
                ..base
            }),
            ProfileStatus::SampleLimit
        );
    }

    #[test]
    fn status_spellings_are_stable() {
        assert_eq!(ProfileStatus::Complete.as_str(), "complete");
        assert_eq!(ProfileStatus::SampleLimit.as_str(), "sample_limit");
        assert_eq!(ProfileStatus::DurationLimit.as_str(), "duration_limit");
        assert_eq!(ProfileStatus::ChildExited.as_str(), "child_exited");
        assert_eq!(
            ProfileStatus::ProfilerUnavailable.as_str(),
            "profiler_unavailable"
        );
    }
}
