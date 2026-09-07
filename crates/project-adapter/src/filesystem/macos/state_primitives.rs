//! Private fixed-child state-root primitives shared by native durable stores.
//!
//! Every operation here is handle-relative and no-follow: a name is resolved
//! only through an already validated directory descriptor, never re-walked from
//! a string. Nothing in this module interprets guest data.
use rust_engineering_domain::{QUALITY_MAX_STORE_ENTRIES, QualityArtifactError};
use rustix::fs::{
    AtFlags, CWD, Dir, FileType, Mode, OFlags, fcntl_fullfsync, fstat, fstatfs, fsync, mkdirat,
    openat, renameat, unlinkat,
};
use std::io::Read;
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

use super::{SecureProjects, checked_path, flags};

fn denied<T>() -> Result<T, QualityArtifactError> {
    Err(QualityArtifactError::Io)
}

/// Device/inode pair of an open handle, used for binding and identity checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) device: i64,
    pub(crate) inode: u64,
}

pub(crate) fn node(fd: &impl AsFd) -> Result<Node, QualityArtifactError> {
    let stat = fstat(fd).map_err(|_| QualityArtifactError::Io)?;
    Ok(Node {
        device: i64::from(stat.st_dev),
        inode: stat.st_ino,
    })
}

/// The state-root qualification M2's `prepare_mutation_state` performs before
/// creating its own sibling: this uid's directory, writable by nobody else.
///
/// It deliberately does **not** require mode `0700`, because the state root is
/// an operator-supplied `--state-root PATH` that M2 already accepts at `0755`.
/// The directories this store creates *inside* it are still exactly `0700`.
pub(crate) fn qualified_state_root(fd: &impl AsFd) -> Result<(), QualityArtifactError> {
    let stat = fstat(fd).map_err(|_| QualityArtifactError::Io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o022 != 0
    {
        return Err(QualityArtifactError::UnsupportedStateRoot);
    }
    Ok(())
}

pub(crate) fn private_directory(fd: &impl AsFd) -> Result<(), QualityArtifactError> {
    let stat = fstat(fd).map_err(|_| QualityArtifactError::Io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o7777 != 0o700
    {
        return denied();
    }
    Ok(())
}

/// Accepts only a private regular file with exactly one link, owned by this
/// uid, mode 0600 and at most `max` bytes. Returns its exact size.
pub(crate) fn private_file(fd: &impl AsFd, max: u64) -> Result<u64, QualityArtifactError> {
    let stat = fstat(fd).map_err(|_| QualityArtifactError::Io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || stat.st_nlink != 1
        || stat.st_uid != rustix::process::geteuid().as_raw()
        || stat.st_mode & 0o7777 != 0o600
        || stat.st_size < 0
        || stat.st_size as u64 > max
    {
        return denied();
    }
    Ok(stat.st_size as u64)
}

/// APFS needs `F_FULLFSYNC` after `fsync` for a real device-level barrier.
pub(crate) fn durable(fd: &impl AsFd) -> Result<(), QualityArtifactError> {
    fsync(fd)
        .and_then(|_| fcntl_fullfsync(fd))
        .map_err(|_| QualityArtifactError::Io)
}

/// Free bytes usable by an unprivileged writer on the volume holding `fd`.
pub(crate) fn free_bytes(fd: &impl AsFd) -> Result<u64, QualityArtifactError> {
    let stat = fstatfs(fd).map_err(|_| QualityArtifactError::Io)?;
    stat.f_bavail
        .checked_mul(u64::from(stat.f_bsize))
        .ok_or(QualityArtifactError::Io)
}

/// Fixed ASCII directory names only: never a guest name, MIME, URI or member.
fn fixed_name(name: &str) -> Result<(), QualityArtifactError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return denied();
    }
    Ok(())
}

/// Names this store generates: canonical locators and fixed control files.
/// Guest text can never satisfy it, and it cannot express a path component.
pub(crate) fn generated_name(name: &str) -> Result<(), QualityArtifactError> {
    if name.is_empty()
        || name.len() > 64
        || name.starts_with('.')
        || name.contains("..")
        || !name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_' | b'.')
        })
    {
        return denied();
    }
    Ok(())
}

pub(crate) struct FixedChild {
    pub(crate) directory: OwnedFd,
    pub(crate) parent: Node,
}

/// Opens `parent/child`, creating the private child directory if needed.
///
/// The parent must qualify as a protected APFS state root under exactly M2's
/// probe (`qualified_state_root`); the child directory this store creates is
/// held to the stricter `0700` of `private_directory`.
pub(crate) fn fixed_child(parent: &Path, child: &str) -> Result<FixedChild, QualityArtifactError> {
    fixed_name(child)?;
    let parent = checked_path(parent).map_err(|_| QualityArtifactError::Io)?;
    if parent == Path::new("/") {
        return denied();
    }
    // Linux/Windows never reach here; on macOS a non-APFS or unqualified root
    // is reported as an unsupported platform before any effect.
    let _qualified = SecureProjects::new(std::slice::from_ref(&parent))
        .map_err(|_| QualityArtifactError::UnsupportedPlatform)?;
    let slash = openat(
        CWD,
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| QualityArtifactError::Io)?;
    let relative = parent
        .strip_prefix("/")
        .map_err(|_| QualityArtifactError::Io)?;
    let root = openat(&slash, relative, flags(true), Mode::empty())
        .map_err(|_| QualityArtifactError::Io)?;
    qualified_state_root(&root)?;
    let parent_node = node(&root)?;
    let directory = open_or_create_directory(&root, child)?;
    durable(&directory)?;
    durable(&root)?;
    // The root must still be the same object after creation.
    let current = openat(&slash, relative, flags(true), Mode::empty())
        .map_err(|_| QualityArtifactError::Io)?;
    if node(&current)? != parent_node {
        return denied();
    }
    Ok(FixedChild {
        directory,
        parent: parent_node,
    })
}

pub(crate) fn open_or_create_directory(
    parent: &impl AsFd,
    child: &str,
) -> Result<OwnedFd, QualityArtifactError> {
    fixed_name(child)?;
    match mkdirat(parent, child, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(_) => return Err(QualityArtifactError::Io),
    }
    let fd =
        openat(parent, child, flags(true), Mode::empty()).map_err(|_| QualityArtifactError::Io)?;
    private_directory(&fd)?;
    Ok(fd)
}

/// Opens an existing private file for reading, or reports absence.
pub(crate) fn open_private_read(
    parent: &impl AsFd,
    name: &str,
) -> Result<Option<OwnedFd>, QualityArtifactError> {
    generated_name(name)?;
    match openat(parent, name, flags(false), Mode::empty()) {
        Ok(fd) => Ok(Some(fd)),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(_) => denied(),
    }
}

/// Opens an existing private file for writing, without following any link.
/// Used only to complete a truncation this store already decided on.
pub(crate) fn open_private_write(
    parent: &impl AsFd,
    name: &str,
) -> Result<Option<OwnedFd>, QualityArtifactError> {
    generated_name(name)?;
    match openat(parent, name, flags(false) | OFlags::RDWR, Mode::empty()) {
        Ok(fd) => Ok(Some(fd)),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(_) => denied(),
    }
}

/// Creates a new private file, failing if the name already exists.
pub(crate) fn create_private_exclusive(
    parent: &impl AsFd,
    name: &str,
) -> Result<OwnedFd, QualityArtifactError> {
    generated_name(name)?;
    let fd = openat(
        parent,
        name,
        flags(false) | OFlags::RDWR | OFlags::CREATE | OFlags::EXCL,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|error| {
        if error == rustix::io::Errno::EXIST {
            QualityArtifactError::RecoveryRequired
        } else {
            QualityArtifactError::Io
        }
    })?;
    private_file(&fd, 0)?;
    Ok(fd)
}

/// Creates the fixed control file if absent and returns it opened read/write.
pub(crate) fn open_or_create_private(
    parent: &impl AsFd,
    name: &str,
    max: u64,
) -> Result<OwnedFd, QualityArtifactError> {
    generated_name(name)?;
    let fd = openat(
        parent,
        name,
        flags(false) | OFlags::RDWR | OFlags::CREATE,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|_| QualityArtifactError::Io)?;
    private_file(&fd, max)?;
    Ok(fd)
}

/// Reads a whole bounded private file, rejecting concurrent size changes.
pub(crate) fn read_private(
    parent: &impl AsFd,
    name: &str,
    max: u64,
) -> Result<Option<Vec<u8>>, QualityArtifactError> {
    let Some(fd) = open_private_read(parent, name)? else {
        return Ok(None);
    };
    let before = private_file(&fd, max)?;
    let mut file = std::fs::File::from(fd);
    let mut bytes = Vec::new();
    (&mut file)
        .take(max.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| QualityArtifactError::Io)?;
    if bytes.len() as u64 != before || private_file(&file, max)? != before {
        return Err(QualityArtifactError::RecoveryRequired);
    }
    Ok(Some(bytes))
}

pub(crate) fn rename(
    from_dir: &impl AsFd,
    from: &str,
    to_dir: &impl AsFd,
    to: &str,
) -> Result<(), QualityArtifactError> {
    generated_name(from)?;
    generated_name(to)?;
    renameat(from_dir, from, to_dir, to).map_err(|_| QualityArtifactError::Io)
}

/// Moves an entry whose name was *observed* in a directory listing, not
/// generated. The observed name is only checked for the properties a directory
/// entry must already have; the destination name is always generated, so no
/// foreign text ever becomes a new host filename.
pub(crate) fn rename_observed(
    from_dir: &impl AsFd,
    observed: &str,
    to_dir: &impl AsFd,
    to: &str,
) -> Result<(), QualityArtifactError> {
    if observed.is_empty()
        || observed.len() > 255
        || observed.contains('/')
        || matches!(observed, "." | "..")
    {
        return denied();
    }
    generated_name(to)?;
    renameat(from_dir, observed, to_dir, to).map_err(|_| QualityArtifactError::Io)
}

pub(crate) fn unlink(parent: &impl AsFd, name: &str) -> Result<(), QualityArtifactError> {
    generated_name(name)?;
    match unlinkat(parent, name, AtFlags::empty()) {
        Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
        Err(_) => denied(),
    }
}

/// Bounded directory listing. `.` and `..` are dropped; a non-UTF-8 or
/// oversized directory fails closed rather than being partially interpreted.
pub(crate) fn list(parent: &impl AsFd) -> Result<Vec<String>, QualityArtifactError> {
    let mut directory = Dir::read_from(parent).map_err(|_| QualityArtifactError::Io)?;
    let mut names = Vec::new();
    for entry in &mut directory {
        let entry = entry.map_err(|_| QualityArtifactError::Io)?;
        let name = entry
            .file_name()
            .to_str()
            .map_err(|_| QualityArtifactError::RecoveryRequired)?;
        if matches!(name, "." | "..") {
            continue;
        }
        if names.len() >= QUALITY_MAX_STORE_ENTRIES {
            return Err(QualityArtifactError::RecoveryRequired);
        }
        names.push(name.to_owned());
    }
    names.sort();
    Ok(names)
}
