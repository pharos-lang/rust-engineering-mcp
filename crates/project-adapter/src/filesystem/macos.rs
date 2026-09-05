use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};

use rust_engineering_application::{
    OperationControl, ProjectBackend, ProjectError, ProjectIdentity, ValidatedProject,
};
use rust_engineering_domain::OperationalErrorCode;
use rustix::fs::{CWD, FileType, Mode, OFlags, Stat, fstat, fstatfs, openat};
use rustix::io::Errno;
use sha2::{Digest, Sha256};

use crate::{ManifestIo, manifest};

mod mutation;
pub use mutation::NativeMutationStore;

// XNU fcntl.h / open(2), verified by real positive and negative fixtures.
// rustix 1.1.4 does not name these Apple flags. Never use them on another OS.
// XNU rejects NOFOLLOW + NOFOLLOW_ANY together; ANY protects every component.
const NOFOLLOW_ANY: u32 = 0x2000_0000;
const RESOLVE_BENEATH: u32 = 0x0000_1000;
const UNIQUE: u32 = 0x0000_2000;
const ENOTCAPABLE: i32 = 107;
const MAX_FILE_BYTES: u64 = 256 * 1024;

fn rejected(code: OperationalErrorCode) -> ProjectError {
    ProjectError::Rejected(code)
}
fn invalid() -> ProjectError {
    rejected(OperationalErrorCode::InvalidProject)
}
fn denied() -> ProjectError {
    rejected(OperationalErrorCode::SandboxDenied)
}
fn unsupported() -> ProjectError {
    rejected(OperationalErrorCode::UnsupportedPlatform)
}

fn map_io(error: Errno) -> ProjectError {
    if error == Errno::NOENT {
        rejected(OperationalErrorCode::ProjectNotFound)
    } else if error == Errno::LOOP
        || error == Errno::ACCESS
        || error == Errno::PERM
        || error.raw_os_error() == ENOTCAPABLE
    {
        denied()
    } else {
        invalid()
    }
}

fn flags(directory: bool) -> OFlags {
    let common =
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::from_bits_retain(NOFOLLOW_ANY | RESOLVE_BENEATH);
    if directory {
        common | OFlags::DIRECTORY
    } else {
        common | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::from_bits_retain(UNIQUE)
    }
}

fn checked_path(path: &Path) -> Result<PathBuf, ProjectError> {
    let text = path.to_str().ok_or_else(invalid)?;
    if !path.is_absolute()
        || text.len() > 4096
        || text.bytes().any(|byte| byte.is_ascii_control())
        || text.contains("//")
        || text.split('/').any(|part| part == "." || part == "..")
        || path.components().count() > 64
    {
        return Err(invalid());
    }
    Ok(path.to_path_buf())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Node {
    device: i32,
    inode: u64,
}
impl Node {
    fn of(fd: &impl AsFd) -> Result<Self, ProjectError> {
        let stat = fstat(fd).map_err(map_io)?;
        Ok(Self {
            device: stat.st_dev,
            inode: stat.st_ino,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    node: Node,
    size: i64,
    modified: (i64, i64),
    changed: (i64, i64),
}

impl FileStamp {
    fn from_stat(stat: Stat) -> Result<Self, ProjectError> {
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_nlink != 1 {
            return Err(denied());
        }
        Ok(Self {
            node: Node {
                device: stat.st_dev,
                inode: stat.st_ino,
            },
            size: stat.st_size,
            modified: (stat.st_mtime, stat.st_mtime_nsec),
            changed: (stat.st_ctime, stat.st_ctime_nsec),
        })
    }
}

fn require_apfs(fd: &impl AsFd) -> Result<(), ProjectError> {
    let stat = fstatfs(fd).map_err(map_io)?;
    let name: Vec<u8> = stat
        .f_fstypename
        .iter()
        .take_while(|byte| **byte != 0)
        .map(|byte| *byte as u8)
        .collect();
    if name != b"apfs" {
        return Err(unsupported());
    }
    Ok(())
}

struct Root {
    path: PathBuf,
    directory: OwnedFd,
    node: Node,
}

pub struct SecureProjects {
    slash: OwnedFd,
    roots: Vec<Root>,
}

/// Keeps the original directory alive, preventing inode reuse on revalidation.
pub struct ProjectLease {
    path: PathBuf,
    directory: OwnedFd,
    node: Node,
}

impl SecureProjects {
    pub fn new(paths: &[PathBuf]) -> Result<Self, ProjectError> {
        if paths.len() > 16 {
            return Err(denied());
        }
        // macOS 26+ is the initial supported kernel family; probe actual flags
        // as well. Headers and OS branding alone do not demonstrate enforcement.
        let kernel = rustix::system::uname();
        let major = kernel
            .release()
            .to_str()
            .ok()
            .and_then(|release| release.split('.').next())
            .and_then(|major| major.parse::<u32>().ok());
        if !major.is_some_and(|major| major >= 25) {
            return Err(unsupported());
        }
        let slash = openat(
            CWD,
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(map_io)?;
        // Read-only probe from '/', never obtains a capability outside authority.
        let dot = openat(&slash, ".", flags(true), Mode::empty()).map_err(|_| unsupported())?;
        if Node::of(&dot)? != Node::of(&slash)? {
            return Err(unsupported());
        }
        // XNU must recognize NOFOLLOW_ANY: its documented conflict with
        // NOFOLLOW is a read-only probe, unlike creating a symlink at startup.
        match openat(&slash, ".", flags(true) | OFlags::NOFOLLOW, Mode::empty()) {
            Err(Errno::INVAL) => {}
            _ => return Err(unsupported()),
        }
        // An absolute operand discriminates BENEATH enforcement.
        match openat(&slash, "/", flags(true), Mode::empty()) {
            Err(error) if error.raw_os_error() == ENOTCAPABLE => {}
            _ => return Err(unsupported()),
        }
        let mut roots = Vec::new();
        for path in paths {
            let path = checked_path(path)?;
            if path == Path::new("/") {
                return Err(denied());
            }
            let relative = path.strip_prefix("/").map_err(|_| invalid())?;
            let directory = openat(&slash, relative, flags(true), Mode::empty()).map_err(map_io)?;
            require_apfs(&directory)?;
            let node = Node::of(&directory)?;
            if roots.iter().any(|root: &Root| root.path == path) {
                return Err(invalid());
            }
            roots.push(Root {
                path,
                directory,
                node,
            });
        }
        roots.sort_by_key(|root| std::cmp::Reverse(root.path.components().count()));
        Ok(Self { slash, roots })
    }

    fn check_root(&self, root: &Root) -> Result<(), ProjectError> {
        let current = openat(
            &self.slash,
            root.path.strip_prefix("/").map_err(|_| invalid())?,
            flags(true),
            Mode::empty(),
        )
        .map_err(map_io)?;
        if Node::of(&current)? != root.node || Node::of(&root.directory)? != root.node {
            return Err(denied());
        }
        Ok(())
    }

    fn open_path(&self, path: &Path, directory: bool) -> Result<OwnedFd, ProjectError> {
        let path = checked_path(path)?;
        let root = self
            .roots
            .iter()
            .find(|root| path.starts_with(&root.path))
            .ok_or_else(denied)?;
        self.check_root(root)?;
        let relative = path.strip_prefix(&root.path).map_err(|_| denied())?;
        let relative = if relative.as_os_str().is_empty() {
            Path::new(".")
        } else {
            relative
        };
        // Full path resolved from the original authority, never from a moved
        // descendant capability. Kernel rejects links in every component.
        let fd =
            openat(&root.directory, relative, flags(directory), Mode::empty()).map_err(map_io)?;
        require_apfs(&fd)?;
        if Node::of(&fd)?.device != root.node.device {
            return Err(denied());
        }
        self.check_root(root)?;
        Ok(fd)
    }

    fn collect(
        &self,
        path: &Path,
        control: &dyn OperationControl,
    ) -> Result<ValidatedProject<ProjectLease>, ProjectError> {
        control.check()?;
        let path = checked_path(path)?;
        let directory = self.open_path(&path, true)?;
        let node = Node::of(&directory)?;
        let io = Access {
            projects: self,
            control,
            observed: RefCell::new(BTreeMap::new()),
        };
        let graph = manifest::validate(&io, &path, control)?;
        // Recheck all observed identities/metadata before issuing a capability.
        for (file_path, stamp) in io.observed.borrow().iter() {
            control.check()?;
            let fd = self.open_path(file_path, false)?;
            if FileStamp::from_stat(fstat(&fd).map_err(map_io)?)? != *stamp {
                return Err(invalid());
            }
        }
        if Node::of(&self.open_path(&path, true)?)? != node {
            return Err(invalid());
        }
        control.check()?;
        let mut hash = Sha256::new();
        hash.update(b"rust-engineering-mcp/project-identity/v1\0");
        hash.update(node.device.to_le_bytes());
        hash.update(node.inode.to_le_bytes());
        add_hash_field(&mut hash, path.to_str().ok_or_else(invalid)?.as_bytes());
        for (manifest_path, bytes) in graph.manifests {
            add_hash_field(
                &mut hash,
                manifest_path.to_str().ok_or_else(invalid)?.as_bytes(),
            );
            add_hash_field(&mut hash, &bytes);
        }
        let mut encoded = String::from("sha256:");
        for byte in hash.finalize() {
            use std::fmt::Write;
            write!(&mut encoded, "{byte:02x}").map_err(|_| ProjectError::Internal)?;
        }
        let fingerprint = encoded.parse().map_err(|_| ProjectError::Internal)?;
        Ok(ValidatedProject {
            identity: ProjectIdentity {
                workspace_root: path.to_str().ok_or_else(invalid)?.to_owned(),
                fingerprint,
            },
            lease: ProjectLease {
                path,
                directory,
                node,
            },
        })
    }
}

fn add_hash_field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

impl ProjectBackend for SecureProjects {
    type Lease = ProjectLease;
    fn open(
        &self,
        path: &str,
        control: &dyn OperationControl,
    ) -> Result<ValidatedProject<ProjectLease>, ProjectError> {
        self.collect(Path::new(path), control)
    }
    fn revalidate(
        &self,
        lease: &ProjectLease,
        control: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        control.check()?;
        if Node::of(&lease.directory)? != lease.node
            || Node::of(&self.open_path(&lease.path, true)?)? != lease.node
        {
            return Err(invalid());
        }
        Ok(self.collect(&lease.path, control)?.identity)
    }
}

struct Access<'a> {
    projects: &'a SecureProjects,
    control: &'a dyn OperationControl,
    observed: RefCell<BTreeMap<PathBuf, FileStamp>>,
}

impl Access<'_> {
    fn remember(&self, path: &Path, stamp: FileStamp) -> Result<(), ProjectError> {
        // A target can name a manifest already read. Never replace the first
        // observation and thereby hide a mutation between the two accesses.
        let mut observed = self.observed.borrow_mut();
        if let Some(previous) = observed.get(path) {
            if previous != &stamp {
                return Err(invalid());
            }
        } else {
            observed.insert(path.to_path_buf(), stamp);
        }
        Ok(())
    }

    fn file(&self, path: &Path) -> Result<Option<(File, FileStamp)>, ProjectError> {
        self.control.check()?;
        let fd = match self.projects.open_path(path, false) {
            Ok(fd) => fd,
            Err(ProjectError::Rejected(OperationalErrorCode::ProjectNotFound)) => return Ok(None),
            Err(error) => return Err(error),
        };
        let stamp = FileStamp::from_stat(fstat(&fd).map_err(map_io)?)?;
        Ok(Some((File::from(fd), stamp)))
    }
}

impl ManifestIo for Access<'_> {
    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, ProjectError> {
        let Some((mut file, before)) = self.file(path)? else {
            return Ok(None);
        };
        if before.size < 0 || before.size as u64 > MAX_FILE_BYTES {
            return Err(rejected(OperationalErrorCode::OutputLimitExceeded));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        self.control.check()?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(rejected(OperationalErrorCode::OutputLimitExceeded));
        }
        let after = FileStamp::from_stat(fstat(&file).map_err(map_io)?)?;
        if before != after {
            return Err(invalid());
        }
        self.remember(path, after)?;
        Ok(Some(bytes))
    }
    fn is_file(&self, path: &Path) -> Result<bool, ProjectError> {
        let Some((_file, stamp)) = self.file(path)? else {
            return Ok(false);
        };
        self.remember(path, stamp)?;
        Ok(true)
    }
}

mod snapshot;
mod source;
pub use snapshot::read_host_snapshot;
mod cargo_vendor;
pub(crate) use cargo_vendor::capture_cargo_vendor;
