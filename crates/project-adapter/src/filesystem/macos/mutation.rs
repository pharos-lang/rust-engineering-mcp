//! Native APFS mutation writer. All source I/O remains rooted in host authority.
use super::*;
use crate::mutation_store::mutation_digest;
use crate::semantic_delta::{DependencyDelta, validate_dependency_delta, validate_manifest_patch};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rust_engineering_application::{OperationControl, ProjectSourceBackend};
use rust_engineering_domain::{
    IdempotencyKey, M2_RECOVERY_HEADROOM_BYTES, MutationCandidate, MutationCommit, MutationError,
    MutationFileReceipt, MutationId, MutationKind, MutationReceipt, MutationRecordSummary,
    MutationState, SourceBundle, SourceFile, SourceFingerprint,
};
use rustix::buffer::spare_capacity;
use rustix::fs::{
    AtFlags, CloneFlags, Dir, FlockOperation, RenameFlags, fclonefileat, fcntl_fullfsync,
    fgetxattr, flistxattr, flock, fsync, renameat, renameat_with, unlinkat,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Write};

const GLOBAL_LOCK: &str = "mutation-store.lock";
const MAX_JOURNALS: usize = 128;
const MAX_STORE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_JOURNAL_BYTES: usize = 48 * 1024 * 1024;
const RECOVERY_STAGING_HEADROOM_BYTES: u64 = MAX_JOURNAL_BYTES as u64;
const RETAINED_METADATA_GROWTH_BYTES: u64 = MAX_JOURNALS as u64 * 8 * 1024;
const _: () = assert!(
    M2_RECOVERY_HEADROOM_BYTES == RECOVERY_STAGING_HEADROOM_BYTES + RETAINED_METADATA_GROWTH_BYTES
);
// XNU renameatx_np: SWAP | NOFOLLOW_ANY | RESOLVE_BENEATH.
const RENAME_SAFE_SWAP: u32 = 2 | 16 | 32;
const CLONE_SAFE_METADATA: u32 = 1 | 4 | 8 | 16;
const MAX_XATTR_NAME_BYTES: usize = 64 * 1024;
const MAX_XATTR_VALUE_BYTES: usize = 1024 * 1024;
const MAX_XATTR_TOTAL_BYTES: usize = 4 * 1024 * 1024;
const MAX_MANIFEST_BYTES: usize = 256 * 1024;
const WORKSPACE_LOCK_SHARDS: u8 = 64;
const MAX_STORE_ENTRIES: usize = MAX_JOURNALS * 2 + WORKSPACE_LOCK_SHARDS as usize + 3;

fn mutation_io(error: Errno) -> MutationError {
    if error == Errno::WOULDBLOCK {
        MutationError::Busy
    } else if error == Errno::LOOP
        || error == Errno::ACCESS
        || error == Errno::PERM
        || error.raw_os_error() == ENOTCAPABLE
    {
        MutationError::PermissionDenied
    } else {
        MutationError::Io
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IoFaultPoint {
    JournalWrite(JournalPhase),
    TempContentWrite,
    PostSwapDurability,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestIoFaultMode {
    NoSpaceBefore,
    ShortWriteThenNoSpace,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TestIoFault {
    point: IoFaultPoint,
    mode: TestIoFaultMode,
}

#[cfg(test)]
std::thread_local! {
    static TEST_IO_FAULT: std::cell::RefCell<Option<TestIoFault>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(test)]
struct TestIoFaultGuard;

#[cfg(test)]
impl Drop for TestIoFaultGuard {
    fn drop(&mut self) {
        TEST_IO_FAULT.with(|slot| *slot.borrow_mut() = None);
    }
}

#[cfg(test)]
fn inject_test_io_fault(point: IoFaultPoint, mode: TestIoFaultMode) -> TestIoFaultGuard {
    TEST_IO_FAULT.with(|slot| {
        assert!(slot.borrow().is_none(), "test I/O fault already installed");
        *slot.borrow_mut() = Some(TestIoFault { point, mode });
    });
    TestIoFaultGuard
}

#[cfg(test)]
fn take_test_io_fault(point: IoFaultPoint) -> Option<TestIoFaultMode> {
    TEST_IO_FAULT.with(|slot| {
        let mut fault = slot.borrow_mut();
        if fault.as_ref().is_some_and(|fault| fault.point == point) {
            fault.take().map(|fault| fault.mode)
        } else {
            None
        }
    })
}

#[cfg(test)]
fn test_nospace() -> MutationError {
    mutation_io(Errno::NOSPC)
}

#[cfg(test)]
fn write_all_with_test_fault(
    file: &mut File,
    bytes: &[u8],
    point: IoFaultPoint,
) -> Result<(), MutationError> {
    match take_test_io_fault(point) {
        Some(TestIoFaultMode::NoSpaceBefore) => Err(test_nospace()),
        Some(TestIoFaultMode::ShortWriteThenNoSpace) => {
            let prefix = bytes.len() / 2;
            file.write_all(&bytes[..prefix])
                .map_err(|_| MutationError::Io)?;
            Err(test_nospace())
        }
        None => file.write_all(bytes).map_err(|_| MutationError::Io),
    }
}

#[cfg(test)]
fn test_io_fault(point: IoFaultPoint) -> Result<(), MutationError> {
    if take_test_io_fault(point).is_some() {
        Err(test_nospace())
    } else {
        Ok(())
    }
}

#[cfg(not(test))]
fn test_io_fault(_point: IoFaultPoint) -> Result<(), MutationError> {
    Ok(())
}

#[cfg(not(test))]
fn write_all_with_test_fault(
    file: &mut File,
    bytes: &[u8],
    _point: IoFaultPoint,
) -> Result<(), MutationError> {
    file.write_all(bytes).map_err(|_| MutationError::Io)
}

fn project_error(error: ProjectError) -> MutationError {
    match error {
        ProjectError::Cancelled => MutationError::Cancelled,
        ProjectError::Rejected(OperationalErrorCode::SandboxDenied) => {
            MutationError::PermissionDenied
        }
        ProjectError::Rejected(OperationalErrorCode::UnsupportedPlatform) => {
            MutationError::UnsupportedPlatform
        }
        ProjectError::Rejected(OperationalErrorCode::OutputLimitExceeded) => {
            MutationError::LimitExceeded
        }
        ProjectError::Rejected(
            OperationalErrorCode::InvalidProject | OperationalErrorCode::ProjectNotFound,
        ) => MutationError::Conflict,
        _ => MutationError::Io,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StoreNode(i32, u64);

fn store_node(fd: &impl AsFd) -> Result<StoreNode, MutationError> {
    let stat = fstat(fd).map_err(mutation_io)?;
    Ok(StoreNode(stat.st_dev, stat.st_ino))
}

fn require_private_directory(fd: &impl AsFd) -> Result<(), MutationError> {
    let stat = fstat(fd).map_err(mutation_io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o7777 != 0o700
    {
        return Err(MutationError::PermissionDenied);
    }
    Ok(())
}

type PrivateStamp = (StoreNode, usize, (i64, i64), (i64, i64));
type XattrSnapshot = Vec<(Vec<u8>, Vec<u8>)>;

fn require_private_file(fd: &impl AsFd, max: usize) -> Result<PrivateStamp, MutationError> {
    let stat = fstat(fd).map_err(mutation_io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_nlink != 1
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o7777 != 0o600
    {
        return Err(MutationError::PermissionDenied);
    }
    if stat.st_size < 0 || stat.st_size as u64 > max as u64 {
        return Err(MutationError::LimitExceeded);
    }
    Ok((
        StoreNode(stat.st_dev, stat.st_ino),
        stat.st_size as usize,
        (stat.st_mtime, stat.st_mtime_nsec),
        (stat.st_ctime, stat.st_ctime_nsec),
    ))
}

fn xattrs(fd: &impl AsFd) -> Result<XattrSnapshot, MutationError> {
    let mut names = Vec::with_capacity(MAX_XATTR_NAME_BYTES);
    flistxattr(fd, spare_capacity(&mut names)).map_err(|error| {
        if error == Errno::RANGE {
            MutationError::LimitExceeded
        } else {
            mutation_io(error)
        }
    })?;
    let mut total = names.len();
    let mut attributes = Vec::new();
    for name in names
        .split(|byte| *byte == 0)
        .filter(|name| !name.is_empty())
    {
        let mut value = Vec::with_capacity(MAX_XATTR_VALUE_BYTES);
        fgetxattr(fd, name, spare_capacity(&mut value)).map_err(|error| {
            if error == Errno::RANGE {
                MutationError::LimitExceeded
            } else {
                mutation_io(error)
            }
        })?;
        total = total
            .checked_add(value.len())
            .ok_or(MutationError::LimitExceeded)?;
        if total > MAX_XATTR_TOTAL_BYTES {
            return Err(MutationError::LimitExceeded);
        }
        attributes.push((name.to_vec(), value));
    }
    Ok(attributes)
}

struct StateRoot {
    slash: OwnedFd,
    directory: OwnedFd,
    path: PathBuf,
    node: StoreNode,
}

impl StateRoot {
    fn open(path: &Path) -> Result<Self, MutationError> {
        let path = checked_path(path).map_err(project_error)?;
        if path == Path::new("/") {
            return Err(MutationError::PermissionDenied);
        }
        let slash = openat(
            CWD,
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(mutation_io)?;
        let directory = openat(
            &slash,
            path.strip_prefix("/").map_err(|_| MutationError::Invalid)?,
            flags(true),
            Mode::empty(),
        )
        .map_err(mutation_io)?;
        require_apfs(&directory).map_err(project_error)?;
        require_private_directory(&directory)?;
        let state = Self {
            slash,
            node: store_node(&directory)?,
            directory,
            path,
        };
        state.check()?;
        Ok(state)
    }

    fn check(&self) -> Result<(), MutationError> {
        let current = openat(
            &self.slash,
            self.path
                .strip_prefix("/")
                .map_err(|_| MutationError::Invalid)?,
            flags(true),
            Mode::empty(),
        )
        .map_err(|_| MutationError::PermissionDenied)?;
        for fd in [&current, &self.directory] {
            require_private_directory(fd)?;
            if store_node(fd)? != self.node {
                return Err(MutationError::PermissionDenied);
            }
        }
        Ok(())
    }

    fn durable(&self) -> Result<(), MutationError> {
        self.check()?;
        fsync(&self.directory).map_err(mutation_io)?;
        fcntl_fullfsync(&self.directory).map_err(mutation_io)?;
        self.check()
    }

    fn open_lock(&self, name: &str) -> Result<OwnedFd, MutationError> {
        self.check()?;
        let lock = openat(
            &self.directory,
            name,
            flags(false) | OFlags::RDWR | OFlags::CREATE,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(mutation_io)?;
        require_private_file(&lock, 0)?;
        self.check()?;
        Ok(lock)
    }

    fn lock(&self, name: &str) -> Result<OwnedFd, MutationError> {
        let lock = self.open_lock(name)?;
        flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(mutation_io)?;
        self.check()?;
        Ok(lock)
    }

    fn read_optional(&self, name: &str) -> Result<Option<Vec<u8>>, MutationError> {
        self.check()?;
        let fd = match openat(&self.directory, name, flags(false), Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::NOENT) => return Ok(None),
            Err(error) => return Err(mutation_io(error)),
        };
        let before = require_private_file(&fd, MAX_JOURNAL_BYTES)?;
        let mut file = File::from(fd);
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_JOURNAL_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| MutationError::Io)?;
        if bytes.len() > MAX_JOURNAL_BYTES
            || bytes.len() != before.1
            || require_private_file(&file, MAX_JOURNAL_BYTES)? != before
        {
            return Err(MutationError::RecoveryRequired);
        }
        self.check()?;
        Ok(Some(bytes))
    }

    fn write_new(
        &self,
        name: &str,
        bytes: &[u8],
        phase: JournalPhase,
    ) -> Result<(), MutationError> {
        if bytes.len() > MAX_JOURNAL_BYTES {
            return Err(MutationError::LimitExceeded);
        }
        self.check()?;
        let fd = openat(
            &self.directory,
            name,
            flags(false) | OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|error| {
            if error == Errno::EXIST {
                MutationError::RecoveryRequired
            } else {
                mutation_io(error)
            }
        })?;
        let mut file = File::from(fd);
        require_private_file(&file, 0)?;
        write_all_with_test_fault(&mut file, bytes, IoFaultPoint::JournalWrite(phase))?;
        fsync(&file).map_err(mutation_io)?;
        fcntl_fullfsync(&file).map_err(mutation_io)?;
        if require_private_file(&file, MAX_JOURNAL_BYTES)?.1 != bytes.len() {
            return Err(MutationError::Io);
        }
        self.durable()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalFile {
    path: String,
    bytes: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalBundle {
    directories: Vec<String>,
    files: Vec<JournalFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalNode {
    device: i32,
    inode: u64,
}

impl From<Node> for JournalNode {
    fn from(node: Node) -> Self {
        Self {
            device: node.device,
            inode: node.inode,
        }
    }
}

impl From<&SourceBundle> for JournalBundle {
    fn from(bundle: &SourceBundle) -> Self {
        Self {
            directories: bundle.directories().to_vec(),
            files: bundle
                .files()
                .iter()
                .map(|file| JournalFile {
                    path: file.path().to_owned(),
                    bytes: STANDARD.encode(file.bytes()),
                })
                .collect(),
        }
    }
}

impl JournalBundle {
    fn decode(&self) -> Result<SourceBundle, MutationError> {
        let files = self
            .files
            .iter()
            .map(|file| {
                let bytes = STANDARD
                    .decode(&file.bytes)
                    .map_err(|_| MutationError::RecoveryRequired)?;
                SourceFile::new(file.path.clone(), bytes)
                    .map_err(|_| MutationError::RecoveryRequired)
            })
            .collect::<Result<Vec<_>, _>>()?;
        SourceBundle::with_directories(files, self.directories.clone())
            .map_err(|_| MutationError::RecoveryRequired)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum JournalPhase {
    Prepared,
    Scratch,
    Staged,
    Applying,
    Published,
    AbortedCleanup,
    Committed,
    NoChange,
    Aborted,
    RecoveryRequired,
}

impl JournalPhase {
    fn rank(self) -> u8 {
        match self {
            Self::Prepared => 0,
            Self::Scratch => 1,
            Self::Staged => 2,
            Self::Applying => 3,
            Self::Published
            | Self::AbortedCleanup
            | Self::Committed
            | Self::NoChange
            | Self::Aborted
            | Self::RecoveryRequired => 4,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum JournalFormat {
    V1,
    #[default]
    V2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalMutationFile {
    path: String,
    temp_path: String,
    source_node: JournalNode,
    staged_node: Option<JournalNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalBody {
    id: String,
    digest: String,
    key: String,
    operation: String,
    workspace_path: String,
    workspace_device: i32,
    workspace_inode: u64,
    files: Vec<JournalMutationFile>,
    validation: String,
    sequence: u64,
    phase: JournalPhase,
    before: JournalBundle,
    after: JournalBundle,
    legacy_v1: bool,
    #[serde(skip)]
    format: JournalFormat,
}

impl JournalBody {
    fn same_binding(&self, other: &Self) -> bool {
        self.id == other.id
            && self.digest == other.digest
            && self.key == other.key
            && self.operation == other.operation
            && self.workspace_path == other.workspace_path
            && self.workspace_device == other.workspace_device
            && self.workspace_inode == other.workspace_inode
            && self.files.len() == other.files.len()
            && self.files.iter().zip(&other.files).all(|(current, next)| {
                current.path == next.path
                    && current.temp_path == next.temp_path
                    && current.source_node == next.source_node
                    && (current.staged_node == next.staged_node
                        || (current.staged_node.is_none() && next.staged_node.is_some()))
            })
            && self.validation == other.validation
            && self.before == other.before
            && self.after == other.after
    }
}

fn require_newer_staging(
    final_body: &JournalBody,
    staging_body: &JournalBody,
) -> Result<(), MutationError> {
    if !final_body.same_binding(staging_body)
        || staging_body.sequence <= final_body.sequence
        || staging_body.phase.rank() < final_body.phase.rank()
    {
        return Err(MutationError::RecoveryRequired);
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalRecordV2 {
    format: String,
    checksum: String,
    body: JournalBody,
}

#[derive(Serialize)]
struct BorrowedJournalRecordV2<'a> {
    format: &'static str,
    checksum: &'a str,
    body: &'a JournalBody,
}

#[derive(Deserialize)]
struct JournalFormatProbe {
    format: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LegacyJournalPhaseV1 {
    Prepared,
    Scratch,
    Staged,
    Published,
    AbortedCleanup,
    Committed,
    NoChange,
    Aborted,
    RecoveryRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyJournalBodyV1 {
    id: String,
    digest: String,
    key: String,
    operation: String,
    workspace_path: String,
    workspace_device: i32,
    workspace_inode: u64,
    temp_path: String,
    source_node: JournalNode,
    staged_node: Option<JournalNode>,
    validation: String,
    sequence: u64,
    phase: LegacyJournalPhaseV1,
    before: JournalBundle,
    after: JournalBundle,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyJournalRecordV1 {
    format: String,
    checksum: String,
    body: LegacyJournalBodyV1,
}

fn sha256(bytes: &[u8]) -> String {
    let mut encoded = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write;
        // Writing into a String cannot fail; preserve the typed I/O surface anyway.
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    encoded
}

struct Sha256Writer(Sha256);

impl Write for Sha256Writer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| std::io::Error::other("journal length overflow"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn canonical_checksum(value: &impl Serialize) -> Result<String, MutationError> {
    let mut writer = Sha256Writer(Sha256::new());
    serde_json::to_writer(&mut writer, value).map_err(|_| MutationError::Io)?;
    let mut encoded = String::from("sha256:");
    for byte in writer.0.finalize() {
        use std::fmt::Write;
        // Writing into a String cannot fail; preserve the typed I/O surface anyway.
        let _ = write!(&mut encoded, "{byte:02x}");
    }
    Ok(encoded)
}

fn borrowed_record<'a>(body: &'a JournalBody, checksum: &'a str) -> BorrowedJournalRecordV2<'a> {
    BorrowedJournalRecordV2 {
        format: "rust-engineering-mcp-mutation-journal-v2",
        checksum,
        body,
    }
}

fn encode(body: &JournalBody) -> Result<Vec<u8>, MutationError> {
    let checksum = canonical_checksum(body)?;
    let record = borrowed_record(body, &checksum);
    let bytes = serde_json::to_vec(&record).map_err(|_| MutationError::Io)?;
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(MutationError::LimitExceeded);
    }
    Ok(bytes)
}

fn worst_case_record_len(body: &JournalBody) -> Result<usize, MutationError> {
    let mut worst_case = body.clone();
    worst_case.sequence = u64::MAX;
    worst_case.phase = JournalPhase::RecoveryRequired;
    for file in &mut worst_case.files {
        file.staged_node = Some(JournalNode {
            device: i32::MIN,
            inode: u64::MAX,
        });
    }
    let checksum = canonical_checksum(&worst_case)?;
    let record = borrowed_record(&worst_case, &checksum);
    let mut writer = CountingWriter::default();
    serde_json::to_writer(&mut writer, &record).map_err(|_| MutationError::Io)?;
    if writer.bytes > MAX_JOURNAL_BYTES {
        return Err(MutationError::LimitExceeded);
    }
    Ok(writer.bytes)
}

fn decode_envelope(bytes: &[u8]) -> Result<JournalBody, MutationError> {
    if bytes.len() > MAX_JOURNAL_BYTES {
        return Err(MutationError::RecoveryRequired);
    }
    let probe: JournalFormatProbe =
        serde_json::from_slice(bytes).map_err(|_| MutationError::RecoveryRequired)?;
    let body = match probe.format.as_str() {
        "rust-engineering-mcp-mutation-journal-v2" => {
            let record: JournalRecordV2 =
                serde_json::from_slice(bytes).map_err(|_| MutationError::RecoveryRequired)?;
            if record.checksum
                != canonical_checksum(&record.body).map_err(|_| MutationError::RecoveryRequired)?
            {
                return Err(MutationError::RecoveryRequired);
            }
            record.body
        }
        "rust-engineering-mcp-mutation-journal-v1" => {
            let record: LegacyJournalRecordV1 =
                serde_json::from_slice(bytes).map_err(|_| MutationError::RecoveryRequired)?;
            if record.checksum
                != canonical_checksum(&record.body).map_err(|_| MutationError::RecoveryRequired)?
                || record.body.operation != "manifest_patch"
            {
                return Err(MutationError::RecoveryRequired);
            }
            let phase = match record.body.phase {
                LegacyJournalPhaseV1::Prepared => JournalPhase::Prepared,
                LegacyJournalPhaseV1::Scratch => JournalPhase::Scratch,
                LegacyJournalPhaseV1::Staged => JournalPhase::Staged,
                LegacyJournalPhaseV1::Published => JournalPhase::Published,
                LegacyJournalPhaseV1::AbortedCleanup => JournalPhase::AbortedCleanup,
                LegacyJournalPhaseV1::Committed => JournalPhase::Committed,
                LegacyJournalPhaseV1::NoChange => JournalPhase::NoChange,
                LegacyJournalPhaseV1::Aborted => JournalPhase::Aborted,
                LegacyJournalPhaseV1::RecoveryRequired => JournalPhase::RecoveryRequired,
            };
            JournalBody {
                id: record.body.id,
                digest: record.body.digest,
                key: record.body.key,
                operation: record.body.operation,
                workspace_path: record.body.workspace_path,
                workspace_device: record.body.workspace_device,
                workspace_inode: record.body.workspace_inode,
                files: vec![JournalMutationFile {
                    path: "Cargo.toml".to_owned(),
                    temp_path: record.body.temp_path,
                    source_node: record.body.source_node,
                    staged_node: record.body.staged_node,
                }],
                validation: record.body.validation,
                sequence: record.body.sequence,
                phase,
                before: record.body.before,
                after: record.body.after,
                legacy_v1: true,
                format: JournalFormat::V1,
            }
        }
        _ => return Err(MutationError::RecoveryRequired),
    };
    let id = MutationId::new(body.id.clone()).map_err(|_| MutationError::RecoveryRequired)?;
    IdempotencyKey::new(body.key.clone()).map_err(|_| MutationError::RecoveryRequired)?;
    body.digest
        .parse::<SourceFingerprint>()
        .map_err(|_| MutationError::RecoveryRequired)?;
    if operation_kind(&body.operation).is_err()
        || body.files.len() > 128
        || (body.legacy_v1
            && (body.operation != "manifest_patch"
                || body.files.len() != 1
                || body.files[0].path != "Cargo.toml"))
        || body.files.iter().enumerate().any(|(index, file)| {
            file.temp_path.contains('/')
                || file.path.is_empty()
                || file.temp_path
                    != if body.legacy_v1 {
                        legacy_temp_name(&id)
                    } else {
                        temp_name(&id, index)
                    }
        })
    {
        return Err(MutationError::RecoveryRequired);
    }
    Ok(body)
}

fn decode(bytes: &[u8]) -> Result<JournalBody, MutationError> {
    let body = decode_envelope(bytes)?;
    let before = body.before.decode()?;
    let after = body.after.decode()?;
    let candidate = MutationCandidate {
        kind: operation_kind(&body.operation)?,
        before,
        after,
        validation: body.validation.clone(),
    };
    if mutation_digest(&candidate).map_err(|_| MutationError::RecoveryRequired)?
        != body
            .digest
            .parse::<SourceFingerprint>()
            .map_err(|_| MutationError::RecoveryRequired)?
    {
        return Err(MutationError::RecoveryRequired);
    }
    let expected = candidate_files(&candidate).map_err(|_| MutationError::RecoveryRequired)?;
    if expected.len() != body.files.len()
        || expected
            .iter()
            .zip(&body.files)
            .any(|(expected, actual)| expected.path != actual.path)
        || body
            .files
            .windows(2)
            .any(|pair| pair[0].path >= pair[1].path || pair[0].temp_path == pair[1].temp_path)
    {
        return Err(MutationError::RecoveryRequired);
    }
    Ok(body)
}

fn journal_name(id: &MutationId) -> String {
    format!("journal-{}.json", id.as_str())
}

fn legacy_temp_name(id: &MutationId) -> String {
    format!(".rust-mcp-mut-{}.swap", &id.as_str()[4..])
}

fn temp_name(id: &MutationId, index: usize) -> String {
    format!(".rust-mcp-mut-{}-{index:03}.swap", &id.as_str()[4..])
}

fn operation_kind(operation: &str) -> Result<MutationKind, MutationError> {
    match operation {
        "manifest_patch" => Ok(MutationKind::ManifestPatch),
        "format_apply" => Ok(MutationKind::FormatApply),
        "fix_apply" => Ok(MutationKind::FixApply),
        "dependency_add" => Ok(MutationKind::DependencyAdd),
        "dependency_remove" => Ok(MutationKind::DependencyRemove),
        _ => Err(MutationError::RecoveryRequired),
    }
}

fn operation_name(kind: MutationKind) -> &'static str {
    match kind {
        MutationKind::ManifestPatch => "manifest_patch",
        MutationKind::FormatApply => "format_apply",
        MutationKind::FixApply => "fix_apply",
        MutationKind::DependencyAdd => "dependency_add",
        MutationKind::DependencyRemove => "dependency_remove",
    }
}

fn source_file<'a>(bundle: &'a SourceBundle, path: &str) -> Option<&'a SourceFile> {
    bundle.files().iter().find(|file| file.path() == path)
}

#[derive(Clone, Copy)]
struct CandidateFile<'a> {
    path: &'a str,
    before: &'a [u8],
    after: &'a [u8],
}

fn candidate_files(candidate: &MutationCandidate) -> Result<Vec<CandidateFile<'_>>, MutationError> {
    if candidate.before.directories() != candidate.after.directories()
        || candidate.before.files().len() != candidate.after.files().len()
    {
        return Err(MutationError::Invalid);
    }
    let mut changed = Vec::new();
    for (before, after) in candidate.before.files().iter().zip(candidate.after.files()) {
        if before.path() != after.path() {
            return Err(MutationError::Invalid);
        }
        if before.bytes() != after.bytes() {
            changed.push(CandidateFile {
                path: before.path(),
                before: before.bytes(),
                after: after.bytes(),
            });
        }
    }
    match candidate.kind {
        MutationKind::ManifestPatch => {
            let before =
                source_file(&candidate.before, "Cargo.toml").ok_or(MutationError::Invalid)?;
            let after =
                source_file(&candidate.after, "Cargo.toml").ok_or(MutationError::Invalid)?;
            if changed
                .iter()
                .any(|file| !matches!(file.path, "Cargo.toml" | "Cargo.lock"))
            {
                return Err(MutationError::Invalid);
            }
            if after.bytes().len() > MAX_MANIFEST_BYTES {
                return Err(MutationError::LimitExceeded);
            }
            validate_manifest_patch(before.bytes(), after.bytes())?;
            if changed.is_empty() {
                // Preserve the M2-01 receipt contract for a semantic no-op.
                Ok(vec![CandidateFile {
                    path: "Cargo.toml",
                    before: before.bytes(),
                    after: after.bytes(),
                }])
            } else {
                Ok(changed)
            }
        }
        MutationKind::DependencyAdd | MutationKind::DependencyRemove => {
            let manifests = changed
                .iter()
                .filter(|file| file.path == "Cargo.toml" || file.path.ends_with("/Cargo.toml"))
                .copied()
                .collect::<Vec<_>>();
            if manifests.len() > 1
                || changed.iter().any(|file| {
                    file.path != "Cargo.lock"
                        && file.path != "Cargo.toml"
                        && !file.path.ends_with("/Cargo.toml")
                })
            {
                return Err(MutationError::Invalid);
            }
            if let Some(manifest) = manifests.first() {
                if manifest.after.len() > MAX_MANIFEST_BYTES {
                    return Err(MutationError::LimitExceeded);
                }
                validate_dependency_delta(
                    manifest.before,
                    manifest.after,
                    if candidate.kind == MutationKind::DependencyAdd {
                        DependencyDelta::Add
                    } else {
                        DependencyDelta::Remove
                    },
                )?;
            }
            Ok(changed)
        }
        MutationKind::FormatApply | MutationKind::FixApply => {
            if changed.len() > 128 || changed.iter().any(|file| !file.path.ends_with(".rs")) {
                return Err(if changed.len() > 128 {
                    MutationError::LimitExceeded
                } else {
                    MutationError::Invalid
                });
            }
            Ok(changed)
        }
    }
}

fn validate_candidate(request: &MutationCommit) -> Result<Vec<CandidateFile<'_>>, MutationError> {
    if mutation_digest(&request.candidate)? != request.digest {
        return Err(MutationError::Conflict);
    }
    candidate_files(&request.candidate)
}

fn mixed_bundle(
    before: &SourceBundle,
    after: &SourceBundle,
    files: &[JournalMutationFile],
    published_prefix: usize,
) -> Result<SourceBundle, MutationError> {
    let published: std::collections::BTreeSet<_> = files
        .iter()
        .take(published_prefix)
        .map(|file| file.path.as_str())
        .collect();
    let output = before
        .files()
        .iter()
        .map(|file| {
            let bytes = if published.contains(file.path()) {
                source_file(after, file.path())
                    .ok_or(MutationError::RecoveryRequired)?
                    .bytes()
            } else {
                file.bytes()
            };
            SourceFile::new(file.path().to_owned(), bytes.to_vec())
                .map_err(|_| MutationError::RecoveryRequired)
        })
        .collect::<Result<Vec<_>, _>>()?;
    SourceBundle::with_directories(output, before.directories().to_vec())
        .map_err(|_| MutationError::RecoveryRequired)
}

fn file_fingerprint(bytes: &[u8]) -> Result<SourceFingerprint, MutationError> {
    sha256(bytes).parse().map_err(|_| MutationError::Io)
}

fn mutation_state(phase: JournalPhase) -> MutationState {
    match phase {
        JournalPhase::Committed => MutationState::Committed,
        JournalPhase::NoChange => MutationState::NoChange,
        JournalPhase::Aborted => MutationState::Aborted,
        JournalPhase::Published
        | JournalPhase::AbortedCleanup
        | JournalPhase::RecoveryRequired
        | JournalPhase::Prepared
        | JournalPhase::Scratch
        | JournalPhase::Staged
        | JournalPhase::Applying => MutationState::RecoveryRequired,
    }
}

fn make_receipt(body: &JournalBody) -> Result<MutationReceipt, MutationError> {
    let state = mutation_state(body.phase);
    let before = body.before.decode()?;
    let after = body.after.decode()?;
    let files = body
        .files
        .iter()
        .map(|file| {
            let before_file =
                source_file(&before, &file.path).ok_or(MutationError::RecoveryRequired)?;
            let after_file =
                source_file(&after, &file.path).ok_or(MutationError::RecoveryRequired)?;
            let (effect_after, effect_after_bytes) = match state {
                MutationState::Committed => (
                    Some(file_fingerprint(after_file.bytes())?),
                    Some(after_file.bytes().len() as u64),
                ),
                MutationState::NoChange | MutationState::Aborted => (
                    Some(file_fingerprint(before_file.bytes())?),
                    Some(before_file.bytes().len() as u64),
                ),
                MutationState::RecoveryRequired => (None, None),
            };
            Ok(MutationFileReceipt {
                path: file.path.clone(),
                before: file_fingerprint(before_file.bytes())?,
                after: file_fingerprint(after_file.bytes())?,
                before_bytes: before_file.bytes().len() as u64,
                after_bytes: after_file.bytes().len() as u64,
                effect_after,
                effect_after_bytes,
            })
        })
        .collect::<Result<Vec<_>, MutationError>>()?;
    Ok(MutationReceipt {
        id: MutationId::new(body.id.clone()).map_err(|_| MutationError::RecoveryRequired)?,
        digest: body
            .digest
            .parse()
            .map_err(|_| MutationError::RecoveryRequired)?,
        state,
        validation: body.validation.clone(),
        files,
    })
}

struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommitCheckpoint {
    Prepared,
    FileCloned(usize),
    CloneCreated,
    Scratch,
    WrittenBeforeSync,
    BeforeStagedPersist,
    Staged,
    Applying,
    FileSwapped(usize),
    FileCleaned(usize),
    Swapped,
    Verified,
    Published,
    Cleaned,
    Committed,
}

fn workspace_lock_name(node: Node) -> String {
    let mut hash = Sha256::new();
    hash.update(node.device.to_le_bytes());
    hash.update(node.inode.to_le_bytes());
    let shard = hash.finalize()[0] % WORKSPACE_LOCK_SHARDS;
    format!("workspace-lock-{shard:02}.lock")
}

fn is_workspace_lock_name(name: &str) -> bool {
    name.strip_prefix("workspace-lock-")
        .and_then(|name| name.strip_suffix(".lock"))
        .filter(|shard| shard.len() == 2 && shard.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|shard| shard.parse::<u8>().ok())
        .is_some_and(|shard| shard < WORKSPACE_LOCK_SHARDS)
}

pub struct NativeMutationStore {
    state: StateRoot,
    projects: SecureProjects,
    kind: MutationKind,
}

struct StoreIndex {
    journal_count: usize,
    bytes: u64,
    bodies: Vec<(JournalBody, bool)>,
    summaries: Vec<MutationRecordSummary>,
}

fn ensure_new_record_quota(
    index: &StoreIndex,
    transient_reservation: u64,
    retained_future_record: u64,
) -> Result<(), MutationError> {
    let retained_ceiling = MAX_STORE_BYTES
        .checked_sub(M2_RECOVERY_HEADROOM_BYTES)
        .ok_or(MutationError::LimitExceeded)?;
    if index.journal_count >= MAX_JOURNALS
        || index
            .bytes
            .checked_add(transient_reservation)
            .is_none_or(|total| total > MAX_STORE_BYTES)
        || index
            .bytes
            .checked_add(retained_future_record)
            .is_none_or(|total| total > retained_ceiling)
    {
        return Err(MutationError::LimitExceeded);
    }
    Ok(())
}

impl NativeMutationStore {
    pub fn open(state_path: &Path, write_roots: &[PathBuf]) -> Result<Self, MutationError> {
        Self::open_for_kind(state_path, write_roots, MutationKind::ManifestPatch)
    }

    pub fn open_for_kind(
        state_path: &Path,
        write_roots: &[PathBuf],
        kind: MutationKind,
    ) -> Result<Self, MutationError> {
        let checked_state = checked_path(state_path).map_err(project_error)?;
        for root in write_roots {
            let root = checked_path(root).map_err(project_error)?;
            if checked_state.starts_with(&root) || root.starts_with(&checked_state) {
                return Err(MutationError::PermissionDenied);
            }
        }
        let projects = SecureProjects::new(write_roots).map_err(project_error)?;
        let state = StateRoot::open(&checked_state)?;
        state.open_lock(GLOBAL_LOCK)?;
        state.durable()?;
        Ok(Self {
            state,
            projects,
            kind,
        })
    }

    /// Checks the live host grant and the lease's retained physical identity.
    pub fn authorize(&self, lease: &ProjectLease) -> Result<(), MutationError> {
        self.authorize_with(lease, &Continue)
    }

    /// Lists bounded durable records for the local operator without project I/O.
    pub fn list_records(&self) -> Result<Vec<MutationRecordSummary>, MutationError> {
        let _global = self.state.lock(GLOBAL_LOCK)?;
        Ok(self.scan_store()?.summaries)
    }

    /// Removes one explicitly identified terminal replay record.
    pub fn prune_record(
        &self,
        id: &MutationId,
        digest: &SourceFingerprint,
    ) -> Result<(), MutationError> {
        let _global = self.state.lock(GLOBAL_LOCK)?;
        let name = journal_name(id);
        let staging = format!(".{name}.staging");
        if self.state.read_optional(&staging)?.is_some() {
            return Err(MutationError::RecoveryRequired);
        }
        let raw = self
            .state
            .read_optional(&name)?
            .ok_or(MutationError::NotFound)?;
        let body = decode(&raw)?;
        if body.id != id.as_str() {
            return Err(MutationError::RecoveryRequired);
        }
        if body.digest != digest.as_str() {
            return Err(MutationError::Conflict);
        }
        if !matches!(
            body.phase,
            JournalPhase::Committed | JournalPhase::NoChange | JournalPhase::Aborted
        ) {
            return Err(MutationError::RecoveryRequired);
        }
        unlinkat(&self.state.directory, &name, AtFlags::empty()).map_err(|error| {
            if error == Errno::NOENT {
                MutationError::NotFound
            } else {
                mutation_io(error)
            }
        })?;
        self.state.durable()
    }

    pub fn commit(
        &self,
        lease: &ProjectLease,
        request: &MutationCommit,
        control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        self.commit_checked(lease, request, control, |_| Ok(()))
    }

    /// Replays only an existing journal binding; it never creates a journal.
    pub fn replay(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
        digest: &SourceFingerprint,
        key: &IdempotencyKey,
        control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        control.check().map_err(project_error)?;
        let _global = self.state.lock(GLOBAL_LOCK)?;
        self.authorize_with(lease, control)?;
        let workspace_lock = workspace_lock_name(lease.node);
        let _workspace = self.state.lock(&workspace_lock)?;
        let observed = match self.load_readonly(lease, id)? {
            Some(body) => body,
            None => {
                if self.orphan_temp_exists(lease, id)? {
                    return Err(MutationError::RecoveryRequired);
                }
                return Err(MutationError::NotFound);
            }
        };
        self.replay_loaded_locked(lease, id, digest, key, control, observed)
    }

    fn commit_checked(
        &self,
        lease: &ProjectLease,
        request: &MutationCommit,
        control: &dyn OperationControl,
        mut checkpoint: impl FnMut(CommitCheckpoint) -> Result<(), MutationError>,
    ) -> Result<MutationReceipt, MutationError> {
        if request.candidate.kind != self.kind {
            return Err(MutationError::PermissionDenied);
        }
        control.check().map_err(project_error)?;
        let _global = self.state.lock(GLOBAL_LOCK)?;
        self.authorize_with(lease, control)?;
        let workspace_lock = workspace_lock_name(lease.node);
        let _workspace = self.state.lock(&workspace_lock)?;
        let planned = validate_candidate(request)?;
        let changed = planned.iter().any(|file| file.before != file.after);

        if let Some(observed) = self.load_readonly(lease, &request.id)? {
            return self.replay_loaded_locked(
                lease,
                &request.id,
                &request.digest,
                &request.key,
                control,
                observed,
            );
        }
        let index = self.scan_store()?;
        self.reject_new_commit(&index, lease, request)?;

        let before_live = self
            .projects
            .source(lease, control)
            .map_err(project_error)?;
        if before_live != request.candidate.before {
            return Err(MutationError::Conflict);
        }
        control.check().map_err(project_error)?;
        let mut files = Vec::with_capacity(planned.len());
        for (index, file) in planned.iter().enumerate() {
            let (source_bytes, source_node) =
                self.read_workspace_file_with_node(lease, file.path)?;
            if source_bytes != file.before {
                return Err(MutationError::Conflict);
            }
            files.push(JournalMutationFile {
                path: file.path.to_owned(),
                temp_path: temp_name(&request.id, index),
                source_node: source_node.into(),
                staged_node: None,
            });
        }

        let mut body = JournalBody {
            id: request.id.as_str().to_owned(),
            digest: request.digest.as_str().to_owned(),
            key: request.key.as_str().to_owned(),
            operation: operation_name(self.kind).to_owned(),
            workspace_path: lease
                .path
                .to_str()
                .ok_or(MutationError::Invalid)?
                .to_owned(),
            workspace_device: lease.node.device,
            workspace_inode: lease.node.inode,
            files,
            validation: request.candidate.validation.clone(),
            sequence: 0,
            phase: JournalPhase::Prepared,
            before: JournalBundle::from(&request.candidate.before),
            after: JournalBundle::from(&request.candidate.after),
            legacy_v1: false,
            format: JournalFormat::V2,
        };
        let name = journal_name(&request.id);
        let worst_case_bytes = worst_case_record_len(&body)?;
        // Reserve two maximum-size copies because phase persistence briefly
        // holds the durable final record and its complete staging successor.
        let reservation = (worst_case_bytes as u64)
            .checked_mul(2)
            .ok_or(MutationError::LimitExceeded)?;
        ensure_new_record_quota(&index, reservation, worst_case_bytes as u64)?;
        let encoded = encode(&body)?;
        self.state
            .write_new(&name, &encoded, JournalPhase::Prepared)?;
        if let Err(error) = checkpoint(CommitCheckpoint::Prepared) {
            return Err(self.abort_before_effect(lease, body, error));
        }

        if !changed {
            body.phase = JournalPhase::NoChange;
            self.persist(lease, &mut body)?;
            return make_receipt(&body);
        }

        for (index, plan) in planned.iter().enumerate() {
            let staged_node = match self.clone_temp_before(
                lease,
                &body.files[index].path,
                &body.files[index].temp_path,
                plan.before,
                body.files[index].source_node,
            ) {
                Ok(node) => node,
                Err(error) => return Err(self.abort_before_effect(lease, body, error)),
            };
            body.files[index].staged_node = Some(staged_node);
            if let Err(error) = checkpoint(CommitCheckpoint::FileCloned(index)) {
                return Err(self.abort_before_effect(lease, body, error));
            }
        }
        if let Err(error) = checkpoint(CommitCheckpoint::CloneCreated) {
            return Err(self.abort_before_effect(lease, body, error));
        }
        body.phase = JournalPhase::Scratch;
        if let Err(error) = self.persist(lease, &mut body) {
            return Err(self.abort_before_effect(lease, body, error));
        }
        if let Err(error) = checkpoint(CommitCheckpoint::Scratch) {
            return Err(self.abort_before_effect(lease, body, error));
        }
        for (index, plan) in planned.iter().enumerate() {
            let staged_node = body.files[index]
                .staged_node
                .ok_or(MutationError::RecoveryRequired)?;
            if let Err(error) = self.rewrite_temp_after(
                lease,
                &body.files[index].path,
                &body.files[index].temp_path,
                (plan.before, plan.after),
                body.files[index].source_node,
                staged_node,
                || checkpoint(CommitCheckpoint::WrittenBeforeSync),
            ) {
                return Err(self.abort_before_effect(lease, body, error));
            }
        }
        if let Err(error) = checkpoint(CommitCheckpoint::BeforeStagedPersist) {
            return Err(self.abort_before_effect(lease, body, error));
        }
        body.phase = JournalPhase::Staged;
        if let Err(error) = self.persist(lease, &mut body) {
            return Err(self.abort_before_effect(lease, body, error));
        }
        if let Err(error) = checkpoint(CommitCheckpoint::Staged) {
            return Err(self.abort_before_effect(lease, body, error));
        }

        let exclusions = match self.temp_exclusions(lease, &body, false) {
            Ok(exclusions) => exclusions,
            Err(error) => return Err(self.abort_before_effect(lease, body, error)),
        };
        let live = match self
            .projects
            .source_excluding(lease, control, &exclusions)
            .map_err(project_error)
        {
            Ok(live) => live,
            Err(error) => return Err(self.abort_before_effect(lease, body, error)),
        };
        if live != request.candidate.before {
            return Err(self.abort_before_effect(lease, body, MutationError::Conflict));
        }
        if let Err(error) = control.check().map_err(project_error) {
            return Err(self.abort_before_effect(lease, body, error));
        }
        body.phase = JournalPhase::Applying;
        if let Err(error) = self.persist(lease, &mut body) {
            return Err(self.abort_before_effect(lease, body, error));
        }
        if let Err(error) = checkpoint(CommitCheckpoint::Applying) {
            return Err(self.abort_before_effect(lease, body, error));
        }
        let mut published_files = 0usize;
        let post_effect = (|| {
            for (index, plan) in planned.iter().enumerate() {
                let file = &body.files[index];
                let staged_node = file.staged_node.ok_or(MutationError::RecoveryRequired)?;
                self.swap(lease, &file.path, &file.temp_path)?;
                published_files += 1;
                checkpoint(CommitCheckpoint::FileSwapped(index))?;
                self.verify_swap(
                    lease,
                    &file.path,
                    &file.temp_path,
                    plan.before,
                    plan.after,
                    file.source_node,
                    staged_node,
                )?;
            }
            checkpoint(CommitCheckpoint::Swapped)?;
            checkpoint(CommitCheckpoint::Verified)?;
            let exclusions = self.temp_exclusions(lease, &body, true)?;
            let live = self
                .projects
                .source_excluding(lease, &Continue, &exclusions)
                .map_err(project_error)?;
            if live != request.candidate.after {
                return Err(MutationError::RecoveryRequired);
            }
            body.phase = JournalPhase::Published;
            self.persist(lease, &mut body)?;
            checkpoint(CommitCheckpoint::Published)?;
            for (index, plan) in planned.iter().enumerate() {
                let file = &body.files[index];
                self.cleanup_temp(
                    lease,
                    &file.path,
                    &file.temp_path,
                    plan.before,
                    plan.after,
                    file.source_node,
                    file.staged_node.ok_or(MutationError::RecoveryRequired)?,
                )?;
                checkpoint(CommitCheckpoint::FileCleaned(index))?;
            }
            checkpoint(CommitCheckpoint::Cleaned)?;
            let mut committed = body.clone();
            committed.phase = JournalPhase::Committed;
            self.persist(lease, &mut committed)?;
            body = committed;
            checkpoint(CommitCheckpoint::Committed)?;
            make_receipt(&body)
        })();
        match post_effect {
            Ok(value) => Ok(value),
            Err(error) if published_files == 0 => Err(self.abort_before_effect(lease, body, error)),
            Err(_) => Err(MutationError::RecoveryRequired),
        }
    }

    fn replay_loaded_locked(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
        digest: &SourceFingerprint,
        key: &IdempotencyKey,
        control: &dyn OperationControl,
        mut observed: JournalBody,
    ) -> Result<MutationReceipt, MutationError> {
        if observed.id != id.as_str() {
            return Err(MutationError::RecoveryRequired);
        }
        if observed.digest != digest.as_str() || observed.key != key.as_str() {
            return Err(MutationError::Conflict);
        }
        control.check().map_err(project_error)?;
        if matches!(
            observed.phase,
            JournalPhase::Committed | JournalPhase::NoChange | JournalPhase::Aborted
        ) && self.all_temps_absent(lease, &observed)?
        {
            if observed.format == JournalFormat::V1 {
                self.persist(lease, &mut observed)?;
            }
            return make_receipt(&observed);
        }
        let body = self
            .load_repair_for(lease, id, Some(&observed))?
            .ok_or(MutationError::RecoveryRequired)?;
        self.recover_locked(lease, body)
    }

    pub fn receipt(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
    ) -> Result<MutationReceipt, MutationError> {
        let _global = self.state.lock(GLOBAL_LOCK)?;
        self.authorize_with(lease, &Continue)?;
        let body = match self.load_readonly(lease, id)? {
            Some(body) => body,
            None => {
                if self.orphan_temp_exists(lease, id)? {
                    return Err(MutationError::RecoveryRequired);
                }
                return Err(MutationError::NotFound);
            }
        };
        self.authorize_body(lease, &body)?;
        if !self.all_temps_absent(lease, &body)? {
            let mut pending = body;
            pending.phase = JournalPhase::RecoveryRequired;
            return make_receipt(&pending);
        }
        make_receipt(&body)
    }

    pub fn recover(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
    ) -> Result<MutationReceipt, MutationError> {
        let _global = self.state.lock(GLOBAL_LOCK)?;
        self.authorize_with(lease, &Continue)?;
        let workspace_lock = workspace_lock_name(lease.node);
        let _workspace = self.state.lock(&workspace_lock)?;
        let body = match self.load_repair(lease, id)? {
            Some(body) => body,
            None => {
                if self.orphan_temp_exists(lease, id)? {
                    return Err(MutationError::RecoveryRequired);
                }
                return Err(MutationError::NotFound);
            }
        };
        self.authorize_body(lease, &body)?;
        self.recover_locked(lease, body)
    }

    fn authorize_with(
        &self,
        lease: &ProjectLease,
        control: &dyn OperationControl,
    ) -> Result<(), MutationError> {
        control.check().map_err(project_error)?;
        self.state.check()?;
        if Node::of(&lease.directory).map_err(project_error)? != lease.node
            || Node::of(
                &self
                    .projects
                    .open_path(&lease.path, true)
                    .map_err(project_error)?,
            )
            .map_err(project_error)?
                != lease.node
        {
            return Err(MutationError::PermissionDenied);
        }
        control.check().map_err(project_error)?;
        let granted_root = self
            .projects
            .roots
            .iter()
            .find(|root| lease.path == root.path && root.node == lease.node);
        if granted_root.is_none() {
            return Err(MutationError::PermissionDenied);
        }
        Ok(())
    }

    fn authorize_body(
        &self,
        lease: &ProjectLease,
        body: &JournalBody,
    ) -> Result<(), MutationError> {
        if body.workspace_path != lease.path.to_str().ok_or(MutationError::Invalid)?
            || body.workspace_device != lease.node.device
            || body.workspace_inode != lease.node.inode
            || operation_kind(&body.operation)? != self.kind
        {
            return Err(MutationError::PermissionDenied);
        }
        Ok(())
    }

    fn scan_store(&self) -> Result<StoreIndex, MutationError> {
        self.state.check()?;
        let mut dir = Dir::read_from(&self.state.directory).map_err(mutation_io)?;
        let mut entry_count = 0usize;
        let mut bytes = 0u64;
        let mut records: BTreeMap<String, (Option<JournalBody>, Option<JournalBody>, u64)> =
            BTreeMap::new();
        for entry in &mut dir {
            let entry = entry.map_err(mutation_io)?;
            let name = entry
                .file_name()
                .to_str()
                .map_err(|_| MutationError::RecoveryRequired)?;
            if matches!(name, "." | "..") {
                continue;
            }
            entry_count += 1;
            if entry_count > MAX_STORE_ENTRIES {
                return Err(MutationError::LimitExceeded);
            }
            if name == GLOBAL_LOCK || is_workspace_lock_name(name) {
                let fd = openat(&self.state.directory, name, flags(false), Mode::empty())
                    .map_err(mutation_io)?;
                require_private_file(&fd, 0)?;
                continue;
            }
            let is_final = name.starts_with("journal-") && name.ends_with(".json");
            let is_staging = name.starts_with(".journal-") && name.ends_with(".json.staging");
            if !is_final && !is_staging {
                return Err(MutationError::RecoveryRequired);
            }
            let raw = self
                .state
                .read_optional(name)?
                .ok_or(MutationError::RecoveryRequired)?;
            bytes = bytes
                .checked_add(raw.len() as u64)
                .ok_or(MutationError::LimitExceeded)?;
            if bytes > MAX_STORE_BYTES {
                return Err(MutationError::LimitExceeded);
            }
            let body = decode_envelope(&raw)?;
            let expected = if is_final {
                format!("journal-{}.json", body.id)
            } else {
                format!(".journal-{}.json.staging", body.id)
            };
            if name != expected {
                return Err(MutationError::RecoveryRequired);
            }
            let slot = records.entry(body.id.clone()).or_default();
            let target = if is_final { &mut slot.0 } else { &mut slot.1 };
            if target.replace(body).is_some() {
                return Err(MutationError::RecoveryRequired);
            }
            slot.2 = slot
                .2
                .checked_add(raw.len() as u64)
                .ok_or(MutationError::LimitExceeded)?;
            if records.len() > MAX_JOURNALS {
                return Err(MutationError::LimitExceeded);
            }
        }
        let journal_count = records.len();
        let mut bodies = Vec::with_capacity(journal_count * 2);
        let mut summaries = Vec::with_capacity(journal_count);
        for (_, (final_body, staging_body, stored_bytes)) in records {
            if let (Some(final_body), Some(staging_body)) = (&final_body, &staging_body) {
                require_newer_staging(final_body, staging_body)?;
            }
            let summary_body = staging_body
                .as_ref()
                .or(final_body.as_ref())
                .ok_or(MutationError::RecoveryRequired)?;
            summaries.push(MutationRecordSummary {
                id: MutationId::new(summary_body.id.clone())
                    .map_err(|_| MutationError::RecoveryRequired)?,
                digest: summary_body
                    .digest
                    .parse()
                    .map_err(|_| MutationError::RecoveryRequired)?,
                state: if staging_body.is_some() {
                    MutationState::RecoveryRequired
                } else {
                    mutation_state(summary_body.phase)
                },
                stored_bytes,
            });
            bodies.extend(final_body.map(|body| (body, false)));
            bodies.extend(staging_body.map(|body| (body, true)));
        }
        self.state.check()?;
        Ok(StoreIndex {
            journal_count,
            bytes,
            bodies,
            summaries,
        })
    }

    fn read_pair(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
    ) -> Result<(Option<JournalBody>, Option<JournalBody>), MutationError> {
        let name = journal_name(id);
        let staging_name = format!(".{name}.staging");
        let final_bytes = self.state.read_optional(&name)?;
        let staging_bytes = self.state.read_optional(&staging_name)?;
        let final_body = final_bytes.as_deref().map(decode).transpose()?;
        let staging_body = staging_bytes.as_deref().map(decode).transpose()?;
        if let Some(body) = final_body.as_ref() {
            self.authorize_body(lease, body)?;
        }
        if let Some(body) = staging_body.as_ref() {
            self.authorize_body(lease, body)?;
        }
        if final_body
            .as_ref()
            .is_some_and(|body| body.id != id.as_str())
            || staging_body
                .as_ref()
                .is_some_and(|body| body.id != id.as_str())
        {
            return Err(MutationError::RecoveryRequired);
        }
        if let (Some(final_body), Some(staging_body)) = (&final_body, &staging_body) {
            require_newer_staging(final_body, staging_body)?;
        }
        Ok((final_body, staging_body))
    }

    fn load_readonly(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
    ) -> Result<Option<JournalBody>, MutationError> {
        match self.read_pair(lease, id)? {
            (None, None) => Ok(None),
            (Some(body), None) => Ok(Some(body)),
            (None, Some(mut body)) | (Some(_), Some(mut body)) => {
                body.phase = JournalPhase::RecoveryRequired;
                Ok(Some(body))
            }
        }
    }

    fn load_repair(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
    ) -> Result<Option<JournalBody>, MutationError> {
        self.load_repair_for(lease, id, None)
    }

    fn load_repair_for(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
        expected: Option<&JournalBody>,
    ) -> Result<Option<JournalBody>, MutationError> {
        let name = journal_name(id);
        let staging_name = format!(".{name}.staging");
        match self.read_pair(lease, id)? {
            (None, None) => Ok(None),
            (Some(body), None) => {
                if expected.is_some_and(|expected| !body.same_binding(expected)) {
                    return Err(MutationError::RecoveryRequired);
                }
                Ok(Some(body))
            }
            (None, Some(body)) | (Some(_), Some(body)) => {
                if expected.is_some_and(|expected| !body.same_binding(expected)) {
                    return Err(MutationError::RecoveryRequired);
                }
                renameat(
                    &self.state.directory,
                    &staging_name,
                    &self.state.directory,
                    &name,
                )
                .map_err(|_| MutationError::RecoveryRequired)?;
                self.state.durable()?;
                Ok(Some(body))
            }
        }
    }

    fn reject_new_commit(
        &self,
        index: &StoreIndex,
        lease: &ProjectLease,
        request: &MutationCommit,
    ) -> Result<(), MutationError> {
        for (body, is_staging) in &index.bodies {
            if body.key == request.key.as_str() && body.id != request.id.as_str() {
                return Err(MutationError::Conflict);
            }
            if body.id != request.id.as_str()
                && body.workspace_device == lease.node.device
                && body.workspace_inode == lease.node.inode
                && (*is_staging
                    || matches!(
                        body.phase,
                        JournalPhase::Prepared
                            | JournalPhase::Scratch
                            | JournalPhase::Staged
                            | JournalPhase::Applying
                            | JournalPhase::Published
                            | JournalPhase::AbortedCleanup
                            | JournalPhase::RecoveryRequired
                    ))
            {
                return Err(MutationError::RecoveryRequired);
            }
        }
        Ok(())
    }

    fn persist(&self, lease: &ProjectLease, body: &mut JournalBody) -> Result<(), MutationError> {
        let id = MutationId::new(body.id.clone()).map_err(|_| MutationError::RecoveryRequired)?;
        let current = self
            .load_repair_for(lease, &id, Some(body))?
            .ok_or(MutationError::RecoveryRequired)?;
        if current.format == JournalFormat::V2
            && current.phase == body.phase
            && current
                .files
                .iter()
                .map(|file| file.staged_node)
                .eq(body.files.iter().map(|file| file.staged_node))
        {
            *body = current;
            return Ok(());
        }
        let mut next = body.clone();
        next.format = JournalFormat::V2;
        next.sequence = current
            .sequence
            .checked_add(1)
            .ok_or(MutationError::LimitExceeded)?;
        let encoded = encode(&next)?;
        let name = journal_name(&id);
        let staging = format!(".{name}.staging");
        self.state.write_new(&staging, &encoded, next.phase)?;
        renameat(
            &self.state.directory,
            &staging,
            &self.state.directory,
            &name,
        )
        .map_err(mutation_io)?;
        self.state.durable()?;
        *body = next;
        Ok(())
    }

    fn authority<'a>(
        &'a self,
        lease: &ProjectLease,
        path: &Path,
    ) -> Result<(&'a Root, PathBuf), MutationError> {
        let root = self
            .projects
            .roots
            .iter()
            .find(|root| root.path == lease.path && root.node == lease.node)
            .ok_or(MutationError::PermissionDenied)?;
        self.projects.check_root(root).map_err(project_error)?;
        let relative = path
            .strip_prefix(&root.path)
            .map_err(|_| MutationError::PermissionDenied)?;
        Ok((root, relative.to_path_buf()))
    }

    fn workspace_entry(
        &self,
        lease: &ProjectLease,
        name: &str,
    ) -> Result<Option<OwnedFd>, MutationError> {
        let path = lease.path.join(name);
        match self.projects.open_path(&path, false) {
            Ok(fd) => Ok(Some(fd)),
            Err(ProjectError::Rejected(OperationalErrorCode::ProjectNotFound)) => Ok(None),
            Err(error) => Err(project_error(error)),
        }
    }

    fn orphan_temp_exists(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
    ) -> Result<bool, MutationError> {
        if self
            .workspace_entry(lease, &legacy_temp_name(id))?
            .is_some()
        {
            return Ok(true);
        }
        for index in 0..128 {
            if self
                .workspace_entry(lease, &temp_name(id, index))?
                .is_some()
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn all_temps_absent(
        &self,
        lease: &ProjectLease,
        body: &JournalBody,
    ) -> Result<bool, MutationError> {
        for file in &body.files {
            if self.workspace_entry(lease, &file.temp_path)?.is_some() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn temp_exclusions(
        &self,
        lease: &ProjectLease,
        body: &JournalBody,
        displaced_sources: bool,
    ) -> Result<BTreeMap<PathBuf, Node>, MutationError> {
        let mut excluded = BTreeMap::new();
        for file in &body.files {
            let Some(fd) = self.workspace_entry(lease, &file.temp_path)? else {
                if !displaced_sources && body.operation == "manifest_patch" {
                    continue;
                }
                return Err(MutationError::RecoveryRequired);
            };
            let node = Node::of(&fd).map_err(project_error)?;
            let expected = if displaced_sources {
                file.source_node
            } else {
                file.staged_node.ok_or(MutationError::RecoveryRequired)?
            };
            if JournalNode::from(node) != expected {
                return Err(MutationError::RecoveryRequired);
            }
            excluded.insert(lease.path.join(&file.temp_path), node);
        }
        Ok(excluded)
    }

    fn clone_temp_before(
        &self,
        lease: &ProjectLease,
        source_path: &str,
        temp: &str,
        before: &[u8],
        expected_source: JournalNode,
    ) -> Result<JournalNode, MutationError> {
        let cargo_path = lease.path.join(source_path);
        let source = self
            .projects
            .open_path(&cargo_path, false)
            .map_err(project_error)?;
        let source_stat = fstat(&source).map_err(mutation_io)?;
        let source_stamp = FileStamp::from_stat(source_stat).map_err(project_error)?;
        if JournalNode::from(source_stamp.node) != expected_source
            || source_stat.st_uid != rustix::process::geteuid().as_raw()
            || source_stat.st_mode & 0o7000 != 0
            || source_stat.st_mode & 0o600 != 0o600
            || source_stat.st_mode & 0o133 != 0
        {
            return Err(MutationError::PermissionDenied);
        }
        let source_xattrs = xattrs(&source)?;
        if self.read_workspace_file(lease, source_path)? != before {
            return Err(MutationError::Conflict);
        }
        if self.workspace_entry(lease, temp)?.is_some() {
            return Err(MutationError::RecoveryRequired);
        }
        let temp_path = lease.path.join(temp);
        let (root, relative) = self.authority(lease, &temp_path)?;
        fclonefileat(
            &source,
            &root.directory,
            &relative,
            CloneFlags::from_bits_retain(CLONE_SAFE_METADATA),
        )
        .map_err(mutation_io)?;
        let staged = self.validate_unpublished_temp(
            lease,
            source_path,
            temp,
            before,
            Some(before),
            expected_source,
            None,
        )?;
        let cloned = self
            .workspace_entry(lease, temp)?
            .ok_or(MutationError::RecoveryRequired)?;
        fsync(&cloned).map_err(mutation_io)?;
        fcntl_fullfsync(&cloned).map_err(mutation_io)?;
        self.durable_workspace(lease)?;
        // Recheck after durability: the node and exact clone must still be the
        // one validated above before its identity can be journaled.
        let durable = self.validate_unpublished_temp(
            lease,
            source_path,
            temp,
            before,
            Some(before),
            expected_source,
            Some(staged),
        )?;
        if durable != staged
            || FileStamp::from_stat(fstat(&source).map_err(mutation_io)?).map_err(project_error)?
                != source_stamp
            || source_xattrs != xattrs(&source)?
        {
            return Err(MutationError::RecoveryRequired);
        }
        Ok(staged)
    }

    // These explicit values are the complete before/after inode binding; a
    // parameter bundle would make it easier to accidentally reuse the wrong
    // file's authority while iterating a multi-file plan.
    #[allow(clippy::too_many_arguments)]
    fn rewrite_temp_after(
        &self,
        lease: &ProjectLease,
        source_path: &str,
        temp: &str,
        bytes: (&[u8], &[u8]),
        source_node: JournalNode,
        staged_node: JournalNode,
        before_sync: impl FnOnce() -> Result<(), MutationError>,
    ) -> Result<(), MutationError> {
        let (before, after) = bytes;
        self.validate_unpublished_temp(
            lease,
            source_path,
            temp,
            before,
            Some(before),
            source_node,
            Some(staged_node),
        )?;
        let temp_path = lease.path.join(temp);
        let (root, relative) = self.authority(lease, &temp_path)?;
        let writable = openat(
            &root.directory,
            &relative,
            flags(false) | OFlags::WRONLY,
            Mode::empty(),
        )
        .map_err(mutation_io)?;
        if JournalNode::from(Node::of(&writable).map_err(project_error)?) != staged_node {
            return Err(MutationError::RecoveryRequired);
        }
        let mut file = File::from(writable);
        file.set_len(0).map_err(|_| MutationError::Io)?;
        write_all_with_test_fault(&mut file, after, IoFaultPoint::TempContentWrite)?;
        before_sync()?;
        fsync(&file).map_err(mutation_io)?;
        fcntl_fullfsync(&file).map_err(mutation_io)?;
        self.durable_workspace(lease)?;
        self.validate_unpublished_temp(
            lease,
            source_path,
            temp,
            before,
            Some(after),
            source_node,
            Some(staged_node),
        )?;
        Ok(())
    }

    /// Validates a prepublication clone. `expected_temp_bytes == None` is only
    /// used after a durable phase has made the node protocol-owned scratch.
    #[allow(clippy::too_many_arguments)]
    fn validate_unpublished_temp(
        &self,
        lease: &ProjectLease,
        source_path: &str,
        temp: &str,
        before: &[u8],
        expected_temp_bytes: Option<&[u8]>,
        source_node: JournalNode,
        staged_node: Option<JournalNode>,
    ) -> Result<JournalNode, MutationError> {
        let source = self
            .workspace_entry(lease, source_path)?
            .ok_or(MutationError::RecoveryRequired)?;
        let staged = self
            .workspace_entry(lease, temp)?
            .ok_or(MutationError::RecoveryRequired)?;
        let source_stat = fstat(&source).map_err(mutation_io)?;
        let staged_stat = fstat(&staged).map_err(mutation_io)?;
        // FileStamp is the load-bearing regular-file and st_nlink == 1 check
        // for both descriptors; the comparisons below bind remaining metadata.
        let source_stamp = FileStamp::from_stat(source_stat).map_err(project_error)?;
        let staged_stamp = FileStamp::from_stat(staged_stat).map_err(project_error)?;
        let current_uid = rustix::process::geteuid().as_raw();
        if JournalNode::from(source_stamp.node) != source_node
            || source_stamp.node == staged_stamp.node
            || staged_node.is_some_and(|expected| JournalNode::from(staged_stamp.node) != expected)
            || source_stat.st_uid != current_uid
            || staged_stat.st_uid != current_uid
            || source_stat.st_uid != staged_stat.st_uid
            || source_stat.st_gid != staged_stat.st_gid
            || source_stat.st_mode != staged_stat.st_mode
            || source_stat.st_flags != staged_stat.st_flags
            || xattrs(&source)? != xattrs(&staged)?
        {
            return Err(MutationError::RecoveryRequired);
        }
        let (source_bytes, observed_source) =
            self.read_workspace_file_with_node(lease, source_path)?;
        let (staged_bytes, observed_staged) = self.read_workspace_file_with_node(lease, temp)?;
        if source_bytes != before
            || JournalNode::from(observed_source) != source_node
            || JournalNode::from(observed_staged) != JournalNode::from(staged_stamp.node)
            || expected_temp_bytes.is_some_and(|expected| staged_bytes != expected)
        {
            return Err(MutationError::RecoveryRequired);
        }
        Ok(staged_stamp.node.into())
    }

    fn swap(
        &self,
        lease: &ProjectLease,
        source_path: &str,
        temp: &str,
    ) -> Result<(), MutationError> {
        self.authorize_with(lease, &Continue)?;
        let cargo_path = lease.path.join(source_path);
        let temp_path = lease.path.join(temp);
        let (root, cargo_relative) = self.authority(lease, &cargo_path)?;
        let (temp_root, temp_relative) = self.authority(lease, &temp_path)?;
        if root.node != temp_root.node {
            return Err(MutationError::PermissionDenied);
        }
        renameat_with(
            &root.directory,
            &temp_relative,
            &root.directory,
            &cargo_relative,
            RenameFlags::from_bits_retain(RENAME_SAFE_SWAP),
        )
        .map_err(mutation_io)
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_swap(
        &self,
        lease: &ProjectLease,
        source_path: &str,
        temp: &str,
        before: &[u8],
        after: &[u8],
        source_node: JournalNode,
        staged_node: JournalNode,
    ) -> Result<(), MutationError> {
        let (active_bytes, active_node) = self.read_workspace_file_with_node(lease, source_path)?;
        let (displaced_bytes, displaced_node) = self.read_workspace_file_with_node(lease, temp)?;
        if active_bytes != after
            || displaced_bytes != before
            || JournalNode::from(active_node) != staged_node
            || JournalNode::from(displaced_node) != source_node
        {
            return Err(MutationError::RecoveryRequired);
        }
        let active = self
            .workspace_entry(lease, source_path)?
            .ok_or(MutationError::RecoveryRequired)?;
        let displaced = self
            .workspace_entry(lease, temp)?
            .ok_or(MutationError::RecoveryRequired)?;
        fsync(&active).map_err(mutation_io)?;
        fcntl_fullfsync(&active).map_err(mutation_io)?;
        fsync(&displaced).map_err(mutation_io)?;
        fcntl_fullfsync(&displaced).map_err(mutation_io)?;
        test_io_fault(IoFaultPoint::PostSwapDurability)?;
        self.durable_source_parent(lease, source_path)?;
        self.durable_workspace(lease)?;
        self.authorize_with(lease, &Continue)
    }

    #[allow(clippy::too_many_arguments)]
    fn cleanup_temp(
        &self,
        lease: &ProjectLease,
        source_path: &str,
        temp: &str,
        before: &[u8],
        after: &[u8],
        source_node: JournalNode,
        staged_node: JournalNode,
    ) -> Result<(), MutationError> {
        let (active, active_node) = self.read_workspace_file_with_node(lease, source_path)?;
        let (displaced, displaced_node) = self.read_workspace_file_with_node(lease, temp)?;
        if active != after
            || displaced != before
            || JournalNode::from(active_node) != staged_node
            || JournalNode::from(displaced_node) != source_node
        {
            return Err(MutationError::RecoveryRequired);
        }
        self.authorize_with(lease, &Continue)?;
        let root = self
            .projects
            .roots
            .iter()
            .find(|root| root.path == lease.path && root.node == lease.node)
            .ok_or(MutationError::PermissionDenied)?;
        self.projects.check_root(root).map_err(project_error)?;
        unlinkat(&root.directory, temp, AtFlags::empty()).map_err(mutation_io)?;
        self.durable_workspace(lease)?;
        if self.workspace_entry(lease, temp)?.is_some() {
            return Err(MutationError::RecoveryRequired);
        }
        let (active, active_node) = self.read_workspace_file_with_node(lease, source_path)?;
        if active != after || JournalNode::from(active_node) != staged_node {
            return Err(MutationError::RecoveryRequired);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn cleanup_unpublished_temp(
        &self,
        lease: &ProjectLease,
        source_path: &str,
        temp: &str,
        before: &[u8],
        expected_temp: Option<&[u8]>,
        source_node: JournalNode,
        staged_node: JournalNode,
    ) -> Result<(), MutationError> {
        self.validate_unpublished_temp(
            lease,
            source_path,
            temp,
            before,
            expected_temp,
            source_node,
            Some(staged_node),
        )?;
        self.authorize_with(lease, &Continue)?;
        let root = self
            .projects
            .roots
            .iter()
            .find(|root| root.path == lease.path && root.node == lease.node)
            .ok_or(MutationError::PermissionDenied)?;
        self.projects.check_root(root).map_err(project_error)?;
        unlinkat(&root.directory, temp, AtFlags::empty()).map_err(mutation_io)?;
        self.durable_workspace(lease)?;
        if self.workspace_entry(lease, temp)?.is_some() {
            return Err(MutationError::RecoveryRequired);
        }
        let (active, active_node) = self.read_workspace_file_with_node(lease, source_path)?;
        if active != before || JournalNode::from(active_node) != source_node {
            return Err(MutationError::RecoveryRequired);
        }
        Ok(())
    }

    fn durable_workspace(&self, lease: &ProjectLease) -> Result<(), MutationError> {
        let directory = self
            .projects
            .open_path(&lease.path, true)
            .map_err(project_error)?;
        fsync(&directory).map_err(mutation_io)?;
        fcntl_fullfsync(&directory).map_err(mutation_io)?;
        self.authorize_with(lease, &Continue)
    }

    fn durable_source_parent(
        &self,
        lease: &ProjectLease,
        source_path: &str,
    ) -> Result<(), MutationError> {
        let relative = Path::new(source_path);
        let parent = relative.parent().ok_or(MutationError::Invalid)?;
        if parent.as_os_str().is_empty() {
            return Ok(());
        }
        let directory = self
            .projects
            .open_path(&lease.path.join(parent), true)
            .map_err(project_error)?;
        fsync(&directory).map_err(mutation_io)?;
        fcntl_fullfsync(&directory).map_err(mutation_io)?;
        self.authorize_with(lease, &Continue)
    }

    fn read_workspace_file(
        &self,
        lease: &ProjectLease,
        name: &str,
    ) -> Result<Vec<u8>, MutationError> {
        self.read_workspace_file_with_node(lease, name)
            .map(|(bytes, _)| bytes)
    }

    fn read_workspace_file_with_node(
        &self,
        lease: &ProjectLease,
        name: &str,
    ) -> Result<(Vec<u8>, Node), MutationError> {
        let fd = self
            .workspace_entry(lease, name)?
            .ok_or(MutationError::RecoveryRequired)?;
        let before =
            FileStamp::from_stat(fstat(&fd).map_err(mutation_io)?).map_err(project_error)?;
        if before.size < 0 || before.size as usize > rust_engineering_domain::SOURCE_MAX_FILE_BYTES
        {
            return Err(MutationError::LimitExceeded);
        }
        let mut file = File::from(fd);
        let mut bytes = Vec::new();
        (&mut file)
            .take(rust_engineering_domain::SOURCE_MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| MutationError::Io)?;
        let after =
            FileStamp::from_stat(fstat(&file).map_err(mutation_io)?).map_err(project_error)?;
        if before != after || bytes.len() != before.size as usize {
            return Err(MutationError::Conflict);
        }
        Ok((bytes, after.node))
    }

    fn abort_before_effect(
        &self,
        lease: &ProjectLease,
        body: JournalBody,
        original: MutationError,
    ) -> MutationError {
        match self.recover_locked(lease, body) {
            Ok(receipt)
                if matches!(
                    receipt.state,
                    MutationState::NoChange | MutationState::Aborted
                ) =>
            {
                original
            }
            _ => MutationError::RecoveryRequired,
        }
    }

    fn recover_legacy_v1_locked(
        &self,
        lease: &ProjectLease,
        mut body: JournalBody,
    ) -> Result<MutationReceipt, MutationError> {
        let before = body.before.decode()?;
        let after = body.after.decode()?;
        let file = body.files.first().ok_or(MutationError::RecoveryRequired)?;
        let path = file.path.clone();
        let temp_path = file.temp_path.clone();
        let source_node = file.source_node;
        let before_bytes = source_file(&before, &path)
            .ok_or(MutationError::RecoveryRequired)?
            .bytes();
        let after_bytes = source_file(&after, &path)
            .ok_or(MutationError::RecoveryRequired)?
            .bytes();
        let (active, active_node) = self.read_workspace_file_with_node(lease, &path)?;
        let temp = self
            .workspace_entry(lease, &temp_path)?
            .map(|_| self.read_workspace_file_with_node(lease, &temp_path))
            .transpose()?;
        if temp.is_none()
            && matches!(
                body.phase,
                JournalPhase::Committed | JournalPhase::NoChange | JournalPhase::Aborted
            )
        {
            self.persist(lease, &mut body)?;
            return make_receipt(&body);
        }
        let active_node = JournalNode::from(active_node);
        let unchanged_phase = if before_bytes == after_bytes {
            JournalPhase::NoChange
        } else {
            JournalPhase::Aborted
        };
        let staged_node = body.files[0].staged_node;
        match (body.phase, staged_node, temp.as_ref()) {
            (JournalPhase::Prepared, None, Some((temp, _)))
                if active == before_bytes
                    && temp.as_slice() == before_bytes
                    && active_node == source_node =>
            {
                let Ok(staged_node) = self.validate_unpublished_temp(
                    lease,
                    &path,
                    &temp_path,
                    before_bytes,
                    Some(before_bytes),
                    source_node,
                    None,
                ) else {
                    return make_receipt(&body);
                };
                body.files[0].staged_node = Some(staged_node);
                body.phase = JournalPhase::AbortedCleanup;
                self.persist(lease, &mut body)?;
                self.cleanup_unpublished_temp(
                    lease,
                    &path,
                    &temp_path,
                    before_bytes,
                    None,
                    source_node,
                    staged_node,
                )?;
                body.phase = unchanged_phase;
                self.persist(lease, &mut body)?;
            }
            (JournalPhase::Scratch, Some(staged_node), Some(_))
                if active == before_bytes && active_node == source_node =>
            {
                if self
                    .validate_unpublished_temp(
                        lease,
                        &path,
                        &temp_path,
                        before_bytes,
                        None,
                        source_node,
                        Some(staged_node),
                    )
                    .is_err()
                {
                    return make_receipt(&body);
                }
                body.phase = JournalPhase::AbortedCleanup;
                self.persist(lease, &mut body)?;
                self.cleanup_unpublished_temp(
                    lease,
                    &path,
                    &temp_path,
                    before_bytes,
                    None,
                    source_node,
                    staged_node,
                )?;
                body.phase = unchanged_phase;
                self.persist(lease, &mut body)?;
            }
            (JournalPhase::AbortedCleanup, Some(staged_node), Some(_))
                if active == before_bytes && active_node == source_node =>
            {
                self.cleanup_unpublished_temp(
                    lease,
                    &path,
                    &temp_path,
                    before_bytes,
                    None,
                    source_node,
                    staged_node,
                )?;
                body.phase = unchanged_phase;
                self.persist(lease, &mut body)?;
            }
            (phase, Some(staged_node), Some((temp, temp_node)))
                if matches!(
                    phase,
                    JournalPhase::Staged | JournalPhase::Published | JournalPhase::RecoveryRequired
                ) && active == after_bytes
                    && temp.as_slice() == before_bytes
                    && active_node == staged_node
                    && JournalNode::from(*temp_node) == source_node =>
            {
                body.phase = JournalPhase::Published;
                self.persist(lease, &mut body)?;
                self.cleanup_temp(
                    lease,
                    &path,
                    &temp_path,
                    before_bytes,
                    after_bytes,
                    source_node,
                    staged_node,
                )?;
                body.phase = JournalPhase::Committed;
                self.persist(lease, &mut body)?;
            }
            (phase, Some(staged_node), None)
                if matches!(
                    phase,
                    JournalPhase::Staged
                        | JournalPhase::Published
                        | JournalPhase::Committed
                        | JournalPhase::RecoveryRequired
                ) && active == after_bytes
                    && active_node == staged_node =>
            {
                body.phase = JournalPhase::Committed;
                self.persist(lease, &mut body)?;
            }
            (JournalPhase::Staged, Some(staged_node), Some((temp, temp_node)))
                if active == before_bytes
                    && temp.as_slice() == after_bytes
                    && active_node == source_node
                    && JournalNode::from(*temp_node) == staged_node =>
            {
                body.phase = JournalPhase::AbortedCleanup;
                self.persist(lease, &mut body)?;
                self.cleanup_unpublished_temp(
                    lease,
                    &path,
                    &temp_path,
                    before_bytes,
                    Some(after_bytes),
                    source_node,
                    staged_node,
                )?;
                body.phase = unchanged_phase;
                self.persist(lease, &mut body)?;
            }
            (phase, _, None)
                if matches!(
                    phase,
                    JournalPhase::Prepared
                        | JournalPhase::Scratch
                        | JournalPhase::Staged
                        | JournalPhase::AbortedCleanup
                ) && active == before_bytes
                    && active_node == source_node =>
            {
                body.phase = unchanged_phase;
                self.persist(lease, &mut body)?;
            }
            _ => {}
        }
        make_receipt(&body)
    }

    fn recover_locked(
        &self,
        lease: &ProjectLease,
        body: JournalBody,
    ) -> Result<MutationReceipt, MutationError> {
        self.recover_locked_checked(lease, body, |_| Ok(()))
    }

    fn recover_locked_checked(
        &self,
        lease: &ProjectLease,
        mut body: JournalBody,
        mut checkpoint: impl FnMut(CommitCheckpoint) -> Result<(), MutationError>,
    ) -> Result<MutationReceipt, MutationError> {
        if body.legacy_v1 {
            return self.recover_legacy_v1_locked(lease, body);
        }
        let before = body.before.decode()?;
        let after = body.after.decode()?;
        if matches!(
            body.phase,
            JournalPhase::Committed | JournalPhase::NoChange | JournalPhase::Aborted
        ) {
            if self.all_temps_absent(lease, &body)? {
                return make_receipt(&body);
            }
            body.phase = JournalPhase::RecoveryRequired;
            return make_receipt(&body);
        }
        let unchanged_phase = if before == after {
            JournalPhase::NoChange
        } else {
            JournalPhase::Aborted
        };
        if matches!(
            body.phase,
            JournalPhase::Prepared
                | JournalPhase::Scratch
                | JournalPhase::Staged
                | JournalPhase::AbortedCleanup
        ) {
            let phase = body.phase;
            for index in 0..body.files.len() {
                let file = &body.files[index];
                let before_bytes = source_file(&before, &file.path)
                    .ok_or(MutationError::RecoveryRequired)?
                    .bytes();
                let (active, active_node) =
                    self.read_workspace_file_with_node(lease, &file.path)?;
                if active != before_bytes || JournalNode::from(active_node) != file.source_node {
                    return make_receipt(&body);
                }
                let temp = self.workspace_entry(lease, &file.temp_path)?;
                if phase == JournalPhase::Prepared {
                    if temp.is_some() {
                        let staged_node = match self.validate_unpublished_temp(
                            lease,
                            &file.path,
                            &file.temp_path,
                            before_bytes,
                            Some(before_bytes),
                            file.source_node,
                            None,
                        ) {
                            Ok(node) => node,
                            Err(_) => return make_receipt(&body),
                        };
                        body.files[index].staged_node = Some(staged_node);
                    }
                } else if temp.is_none()
                    && body.operation != "manifest_patch"
                    && phase != JournalPhase::AbortedCleanup
                {
                    return make_receipt(&body);
                } else if temp.is_some() {
                    let expected = if phase == JournalPhase::Staged {
                        Some(
                            source_file(&after, &file.path)
                                .ok_or(MutationError::RecoveryRequired)?
                                .bytes(),
                        )
                    } else {
                        None
                    };
                    if self
                        .validate_unpublished_temp(
                            lease,
                            &file.path,
                            &file.temp_path,
                            before_bytes,
                            expected,
                            file.source_node,
                            file.staged_node,
                        )
                        .is_err()
                    {
                        return make_receipt(&body);
                    }
                }
            }
            body.phase = JournalPhase::AbortedCleanup;
            self.persist(lease, &mut body)?;
            for (index, file) in body.files.iter().enumerate() {
                if self.workspace_entry(lease, &file.temp_path)?.is_some() {
                    self.cleanup_unpublished_temp(
                        lease,
                        &file.path,
                        &file.temp_path,
                        source_file(&before, &file.path)
                            .ok_or(MutationError::RecoveryRequired)?
                            .bytes(),
                        None,
                        file.source_node,
                        file.staged_node.ok_or(MutationError::RecoveryRequired)?,
                    )?;
                    checkpoint(CommitCheckpoint::FileCleaned(index))?;
                }
            }
            body.phase = unchanged_phase;
            self.persist(lease, &mut body)?;
            return make_receipt(&body);
        }

        if matches!(
            body.phase,
            JournalPhase::Applying | JournalPhase::RecoveryRequired
        ) {
            let mut prefix = 0usize;
            let mut saw_before = false;
            let mut exclusions = BTreeMap::new();
            for file in &body.files {
                let before_bytes = source_file(&before, &file.path)
                    .ok_or(MutationError::RecoveryRequired)?
                    .bytes();
                let after_bytes = source_file(&after, &file.path)
                    .ok_or(MutationError::RecoveryRequired)?
                    .bytes();
                let staged_node = file.staged_node.ok_or(MutationError::RecoveryRequired)?;
                let (active, active_node) =
                    self.read_workspace_file_with_node(lease, &file.path)?;
                let temp = self
                    .workspace_entry(lease, &file.temp_path)?
                    .map(|_| self.read_workspace_file_with_node(lease, &file.temp_path))
                    .transpose()?;
                if temp.is_none()
                    && body.operation == "manifest_patch"
                    && active == before_bytes
                    && JournalNode::from(active_node) == file.source_node
                {
                    saw_before = true;
                    continue;
                }
                let Some((temp, temp_node)) = temp else {
                    return make_receipt(&body);
                };
                let published = active == after_bytes
                    && JournalNode::from(active_node) == staged_node
                    && temp == before_bytes
                    && JournalNode::from(temp_node) == file.source_node;
                let pending = active == before_bytes
                    && JournalNode::from(active_node) == file.source_node
                    && temp == after_bytes
                    && JournalNode::from(temp_node) == staged_node;
                if published && !saw_before {
                    prefix += 1;
                    exclusions.insert(lease.path.join(&file.temp_path), temp_node);
                } else if pending {
                    saw_before = true;
                    exclusions.insert(lease.path.join(&file.temp_path), temp_node);
                } else {
                    return make_receipt(&body);
                }
            }
            let expected = mixed_bundle(&before, &after, &body.files, prefix)?;
            let live = self
                .projects
                .source_excluding(lease, &Continue, &exclusions)
                .map_err(project_error)?;
            if live != expected {
                return make_receipt(&body);
            }
            if prefix == 0 {
                body.phase = JournalPhase::AbortedCleanup;
                self.persist(lease, &mut body)?;
                for (index, file) in body.files.iter().enumerate() {
                    if self.workspace_entry(lease, &file.temp_path)?.is_some() {
                        self.cleanup_unpublished_temp(
                            lease,
                            &file.path,
                            &file.temp_path,
                            source_file(&before, &file.path)
                                .ok_or(MutationError::RecoveryRequired)?
                                .bytes(),
                            Some(
                                source_file(&after, &file.path)
                                    .ok_or(MutationError::RecoveryRequired)?
                                    .bytes(),
                            ),
                            file.source_node,
                            file.staged_node.ok_or(MutationError::RecoveryRequired)?,
                        )?;
                        checkpoint(CommitCheckpoint::FileCleaned(index))?;
                    }
                }
                body.phase = unchanged_phase;
                self.persist(lease, &mut body)?;
                return make_receipt(&body);
            }
            for file in body.files.iter().skip(prefix) {
                let before_bytes = source_file(&before, &file.path)
                    .ok_or(MutationError::RecoveryRequired)?
                    .bytes();
                let after_bytes = source_file(&after, &file.path)
                    .ok_or(MutationError::RecoveryRequired)?
                    .bytes();
                let staged_node = file.staged_node.ok_or(MutationError::RecoveryRequired)?;
                self.swap(lease, &file.path, &file.temp_path)?;
                self.verify_swap(
                    lease,
                    &file.path,
                    &file.temp_path,
                    before_bytes,
                    after_bytes,
                    file.source_node,
                    staged_node,
                )?;
            }
            let exclusions = self.temp_exclusions(lease, &body, true)?;
            let live = self
                .projects
                .source_excluding(lease, &Continue, &exclusions)
                .map_err(project_error)?;
            if live != after {
                return make_receipt(&body);
            }
            body.phase = JournalPhase::Published;
            self.persist(lease, &mut body)?;
        }

        if body.phase == JournalPhase::Published {
            let mut exclusions = BTreeMap::new();
            for file in &body.files {
                let after_bytes = source_file(&after, &file.path)
                    .ok_or(MutationError::RecoveryRequired)?
                    .bytes();
                let staged_node = file.staged_node.ok_or(MutationError::RecoveryRequired)?;
                let (active, active_node) =
                    self.read_workspace_file_with_node(lease, &file.path)?;
                if active != after_bytes || JournalNode::from(active_node) != staged_node {
                    return make_receipt(&body);
                }
                if self.workspace_entry(lease, &file.temp_path)?.is_some() {
                    let (temp, temp_node) =
                        self.read_workspace_file_with_node(lease, &file.temp_path)?;
                    let before_bytes = source_file(&before, &file.path)
                        .ok_or(MutationError::RecoveryRequired)?
                        .bytes();
                    if temp != before_bytes || JournalNode::from(temp_node) != file.source_node {
                        return make_receipt(&body);
                    }
                    exclusions.insert(lease.path.join(&file.temp_path), temp_node);
                }
            }
            let live = self
                .projects
                .source_excluding(lease, &Continue, &exclusions)
                .map_err(project_error)?;
            if live != after {
                return make_receipt(&body);
            }
            for file in &body.files {
                if self.workspace_entry(lease, &file.temp_path)?.is_some() {
                    self.cleanup_temp(
                        lease,
                        &file.path,
                        &file.temp_path,
                        source_file(&before, &file.path)
                            .ok_or(MutationError::RecoveryRequired)?
                            .bytes(),
                        source_file(&after, &file.path)
                            .ok_or(MutationError::RecoveryRequired)?
                            .bytes(),
                        file.source_node,
                        file.staged_node.ok_or(MutationError::RecoveryRequired)?,
                    )?;
                }
            }
            body.phase = JournalPhase::Committed;
            self.persist(lease, &mut body)?;
        }
        make_receipt(&body)
    }
}

#[cfg(test)]
#[path = "../../../tests/support/native_mutation.rs"]
mod tests;
