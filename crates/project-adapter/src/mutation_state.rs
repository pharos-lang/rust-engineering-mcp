//! One fixed private child of the host's existing execution state directory.
use rust_engineering_domain::MutationError;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
fn checked_parent(path: &Path) -> Result<PathBuf, MutationError> {
    let text = path.to_str().ok_or(MutationError::Invalid)?;
    if !path.is_absolute()
        || text.len() > 4096
        || text.bytes().any(|byte| byte.is_ascii_control())
        || text.contains("//")
        || text.split('/').any(|part| part == "." || part == "..")
        || path.components().count() > 64
    {
        return Err(MutationError::Invalid);
    }
    Ok(path.to_path_buf())
}

#[cfg(not(target_os = "macos"))]
pub fn prepare_mutation_state(_: &Path) -> Result<PathBuf, MutationError> {
    Err(MutationError::UnsupportedPlatform)
}

#[cfg(target_os = "macos")]
pub fn prepare_mutation_state(parent: &Path) -> Result<PathBuf, MutationError> {
    use rustix::fs::{CWD, FileType, Mode, OFlags, fcntl_fullfsync, fstat, fsync, mkdirat, openat};
    let denied = |_| MutationError::PermissionDenied;
    let parent = checked_parent(parent)?;
    // Reuse the existing capability probe/APFS qualification before creation.
    let _qualification =
        crate::SecureProjects::new(std::slice::from_ref(&parent)).map_err(denied)?;
    let relative = parent
        .strip_prefix("/")
        .map_err(|_| MutationError::Invalid)?;
    let flags = OFlags::RDONLY
        | OFlags::DIRECTORY
        | OFlags::CLOEXEC
        | OFlags::from_bits_retain(0x2000_1000);
    let slash = openat(
        CWD,
        "/",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )
    .map_err(|_| MutationError::PermissionDenied)?;
    let root = openat(&slash, relative, flags, Mode::empty())
        .map_err(|_| MutationError::PermissionDenied)?;
    let before = fstat(&root).map_err(|_| MutationError::Io)?;
    if before.st_uid != rustix::process::geteuid().as_raw() || before.st_mode & 0o022 != 0 {
        return Err(MutationError::PermissionDenied);
    }
    const CHILD: &str = "rust-mcp-mutations-v1";
    match mkdirat(&root, CHILD, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
        Ok(()) | Err(rustix::io::Errno::EXIST) => {}
        Err(_) => return Err(MutationError::Io),
    }
    let child =
        openat(&root, CHILD, flags, Mode::empty()).map_err(|_| MutationError::PermissionDenied)?;
    let stat = fstat(&child).map_err(|_| MutationError::Io)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
        || stat.st_uid != before.st_uid
        || stat.st_mode & 0o7777 != 0o700
    {
        return Err(MutationError::PermissionDenied);
    }
    fsync(&child)
        .and_then(|_| fcntl_fullfsync(&child))
        .map_err(|_| MutationError::Io)?;
    fsync(&root)
        .and_then(|_| fcntl_fullfsync(&root))
        .map_err(|_| MutationError::Io)?;
    let current =
        openat(&slash, relative, flags, Mode::empty()).map_err(|_| MutationError::Conflict)?;
    let current = fstat(&current).map_err(|_| MutationError::Io)?;
    if (current.st_dev, current.st_ino) != (before.st_dev, before.st_ino) {
        return Err(MutationError::Conflict);
    }
    Ok(parent.join(CHILD))
}
