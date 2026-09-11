//! ADR-078: the incremental, handle-relative capture of a large vendor tree.
//!
//! This is the sibling of [`super::cargo_vendor`], not a replacement for it.
//! That module captures a `SourceBundle` and owns every byte it read; this one
//! never owns more than one read buffer, because the trees it exists for are
//! measured in hundreds of megabytes.
//!
//! What it enforces beyond the contract in `domain`:
//!
//! * every open is handle-relative, no-follow and beneath the authorized root,
//!   through the same [`SecureProjects`] capability the qualified flows use;
//! * an entry that is not a regular file or a directory — a symlink, a hard
//!   link, a device, a fifo — is a refusal, never a skip (ADR-078 §6);
//! * a tree that moved under the reader fails the capture instead of
//!   publishing a capture of a tree that no longer exists;
//! * the artifact is written to a `.partial` sibling and renamed only once its
//!   digest is known, so a cancelled or failed capture leaves neither residue
//!   nor a half-written file another run could take for complete.
use super::state_primitives::{durable, rename, unlink};
use super::*;
use crate::vendor_capture::{Sha256Hasher, capture_error};
use rust_engineering_domain::SourceFingerprint;
use rust_engineering_domain::vendor_capture::{
    VENDOR_CAPTURE_MAX_ENTRIES, VENDOR_CAPTURE_READ_BUFFER_BYTES, VendorCapture,
    VendorCaptureBuilder, VendorCaptureError, VendorCaptureVerifier,
};
use rustix::fs::{AtFlags, Dir, unlinkat};
use std::io::Write;

fn mutated() -> ProjectError {
    capture_error(VendorCaptureError::Mutated)
}

/// The stamp that decides whether one file moved under the reader.
///
/// [`FileStamp::from_stat`] already refuses anything that is not a regular file
/// with exactly one link, which is where ADR-078 §6's hard-link refusal lives:
/// a second link to the same inode is not a file this capture will read.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CaptureFileStamp {
    inner: FileStamp,
    mode: u16,
    uid: u32,
}
impl CaptureFileStamp {
    fn of(fd: &impl AsFd) -> Result<Self, ProjectError> {
        let stat = fstat(fd).map_err(map_io)?;
        let inner = FileStamp::from_stat(stat)?;
        if stat.st_uid != rustix::process::geteuid().as_raw() || stat.st_mode & 0o022 != 0 {
            return Err(denied());
        }
        Ok(Self {
            inner,
            mode: stat.st_mode,
            uid: stat.st_uid,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CaptureDirectoryStamp {
    node: Node,
    mode: u16,
    uid: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl CaptureDirectoryStamp {
    fn of(fd: &impl AsFd) -> Result<Self, ProjectError> {
        let stat = fstat(fd).map_err(map_io)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory
            || stat.st_uid != rustix::process::geteuid().as_raw()
            || stat.st_mode & 0o022 != 0
        {
            return Err(denied());
        }
        Ok(Self {
            node: Node {
                device: stat.st_dev,
                inode: stat.st_ino,
            },
            mode: stat.st_mode,
            uid: stat.st_uid,
            modified: (stat.st_mtime, stat.st_mtime_nsec),
            changed: (stat.st_ctime, stat.st_ctime_nsec),
        })
    }
}

/// Removes the half-written artifact unless the capture completed.
///
/// ADR-078 §6's cleanup clause is a `Drop`, not a branch, precisely because the
/// interesting cases are the ones that do not reach the end of the function: a
/// cancellation, a quota, a tree that moved, a write that failed.
struct Partial<'a> {
    directory: &'a OwnedFd,
    name: String,
    armed: bool,
}
impl Drop for Partial<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = unlinkat(self.directory, self.name.as_str(), AtFlags::empty());
        }
    }
}

struct Capture<'a> {
    backend: &'a SecureProjects,
    base: &'a Path,
    control: &'a dyn OperationControl,
    builder: VendorCaptureBuilder<Sha256Hasher>,
    out: File,
    written: u64,
    observed_files: BTreeMap<PathBuf, CaptureFileStamp>,
    observed_directories: BTreeMap<PathBuf, CaptureDirectoryStamp>,
}

impl Capture<'_> {
    fn relative(&self, path: &Path) -> Result<String, ProjectError> {
        Ok(path
            .strip_prefix(self.base)
            .map_err(|_| denied())?
            .to_str()
            .ok_or_else(invalid)?
            .to_owned())
    }

    fn write(&mut self, bytes: &[u8]) -> Result<(), ProjectError> {
        self.out
            .write_all(bytes)
            .map_err(|_| ProjectError::Internal)?;
        self.written += bytes.len() as u64;
        Ok(())
    }

    fn walk(&mut self, path: &Path, buffer: &mut [u8]) -> Result<(), ProjectError> {
        self.control.check()?;
        let fd = self.backend.open_path(path, true)?;
        let before = CaptureDirectoryStamp::of(&fd)?;
        let mut directory = Dir::read_from(&fd).map_err(map_io)?;
        let mut names = Vec::new();
        for entry in &mut directory {
            self.control.check()?;
            let entry = entry.map_err(map_io)?;
            let name = entry.file_name().to_str().map_err(|_| invalid())?;
            if matches!(name, "." | "..") {
                continue;
            }
            // One directory cannot hold more entries than the whole capture
            // admits; this is the same quota, charged before the listing grows.
            if names.len() >= VENDOR_CAPTURE_MAX_ENTRIES {
                return Err(capture_error(VendorCaptureError::Limits));
            }
            names.push((name.to_owned(), entry.file_type()));
        }
        names.sort_by(|left, right| left.0.cmp(&right.0));
        if names.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(invalid());
        }
        for (name, kind) in names {
            self.control.check()?;
            let full = path.join(name);
            let relative = self.relative(&full)?;
            match kind {
                FileType::Directory => {
                    let blocks = self.builder.directory(&relative).map_err(capture_error)?;
                    self.write(blocks.as_slice())?;
                    self.walk(&full, buffer)?;
                }
                FileType::RegularFile => self.file(&full, &relative, buffer)?,
                // ADR-078 §6: a symlink, a device, a fifo, a socket or a type
                // this kernel did not name is refused, not skipped.
                _ => return Err(denied()),
            }
        }
        if CaptureDirectoryStamp::of(&fd)? != before {
            return Err(mutated());
        }
        self.observed_directories.insert(path.to_owned(), before);
        Ok(())
    }

    fn file(&mut self, path: &Path, relative: &str, buffer: &mut [u8]) -> Result<(), ProjectError> {
        self.control.check()?;
        let fd = self.backend.open_path(path, false)?;
        let before = CaptureFileStamp::of(&fd)?;
        if before.inner.size < 0 {
            return Err(invalid());
        }
        let size = before.inner.size as u64;
        // The quota is charged here, from the size the kernel reported, so a
        // file over the per-file or the total ceiling is refused before a byte
        // of it is read.
        let blocks = self
            .builder
            .begin_file(relative, size)
            .map_err(capture_error)?;
        self.write(blocks.as_slice())?;
        let mut file = File::from(fd);
        let mut remaining = size;
        while remaining > 0 {
            self.control.check()?;
            let take = usize::try_from(remaining)
                .unwrap_or(buffer.len())
                .min(buffer.len());
            let read = file.read(&mut buffer[..take]).map_err(|_| invalid())?;
            if read == 0 {
                // Short of what it declared: the file shrank under the reader.
                return Err(mutated());
            }
            self.builder.chunk(&buffer[..read]).map_err(capture_error)?;
            self.out
                .write_all(&buffer[..read])
                .map_err(|_| ProjectError::Internal)?;
            self.written += read as u64;
            remaining -= read as u64;
        }
        // One read past the declared end must find the end. A file that grew
        // while it was being read is a tree that moved.
        if file.read(&mut buffer[..1]).map_err(|_| invalid())? != 0 {
            return Err(mutated());
        }
        let padding = self.builder.end_file().map_err(capture_error)?;
        self.write(padding.as_slice())?;
        self.control.check()?;
        if CaptureFileStamp::of(&file)? != before {
            return Err(mutated());
        }
        self.observed_files.insert(path.to_owned(), before);
        Ok(())
    }

    /// Re-observes every entry through the original root authority.
    ///
    /// The per-entry stamps already refuse a file that moved while it was being
    /// read; this refuses one that moved after it was read and before the
    /// capture closed. It keeps one stamp per entry — metadata bounded by the
    /// entry quota, never content — which is the only thing this path holds
    /// that grows with the tree.
    fn recheck(&self) -> Result<(), ProjectError> {
        for (path, stamp) in &self.observed_files {
            self.control.check()?;
            if CaptureFileStamp::of(&self.backend.open_path(path, false)?)? != *stamp {
                return Err(mutated());
            }
        }
        for (path, stamp) in &self.observed_directories {
            self.control.check()?;
            if CaptureDirectoryStamp::of(&self.backend.open_path(path, true)?)? != *stamp {
                return Err(mutated());
            }
        }
        Ok(())
    }
}

/// The 64 lowercase hex digits of a `sha256:` fingerprint: the artifact's own
/// name, so a complete capture is self-identifying and a run can find an
/// existing one by digest without capturing again (ADR-078 §2).
pub(crate) fn artifact_name(digest: &SourceFingerprint) -> Result<String, ProjectError> {
    crate::vendor_capture::capture_artifact_name(digest).ok_or_else(invalid)
}

/// Capture `directory` into a new artifact under `store`, and return its
/// identity. Never called as a side effect of a measurement (ADR-078 §3).
pub(crate) fn capture_vendor_tree(
    directory: &Path,
    store: &Path,
    control: &dyn OperationControl,
) -> Result<VendorCapture, ProjectError> {
    control.check()?;
    let directory = checked_path(directory)?;
    let store = checked_path(store)?;
    if directory == store || store.starts_with(&directory) || directory.starts_with(&store) {
        // The artifact would otherwise become an entry of the tree it captures.
        return Err(invalid());
    }
    let backend = SecureProjects::new(std::slice::from_ref(&directory))?;
    let root = backend.open_path(&directory, true)?;
    let root_before = CaptureDirectoryStamp::of(&root)?;

    let store_backend = SecureProjects::new(std::slice::from_ref(&store))?;
    let store_fd = store_backend.open_path(&store, true)?;
    let mut nonce = [0_u8; 16];
    getrandom::fill(&mut nonce).map_err(|_| ProjectError::Internal)?;
    let mut name = String::new();
    for byte in nonce {
        use std::fmt::Write;
        write!(&mut name, "{byte:02x}").map_err(|_| ProjectError::Internal)?;
    }
    name.push_str(".partial");
    let partial_fd = openat(
        &store_fd,
        name.as_str(),
        flags(false) | OFlags::RDWR | OFlags::CREATE | OFlags::EXCL,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(map_io)?;
    let mut guard = Partial {
        directory: &store_fd,
        name: name.clone(),
        armed: true,
    };

    let mut buffer = vec![0_u8; VENDOR_CAPTURE_READ_BUFFER_BYTES];
    let mut capture = Capture {
        backend: &backend,
        base: &directory,
        control,
        builder: VendorCaptureBuilder::new(Sha256Hasher::default(), Sha256Hasher::default()),
        out: File::from(partial_fd),
        written: 0,
        observed_files: BTreeMap::new(),
        observed_directories: BTreeMap::new(),
    };
    capture.walk(&directory, &mut buffer)?;
    capture.recheck()?;
    if CaptureDirectoryStamp::of(&root)? != root_before
        || CaptureDirectoryStamp::of(&backend.open_path(&directory, true)?)? != root_before
    {
        return Err(mutated());
    }
    control.check()?;
    let Capture {
        builder,
        mut out,
        written,
        ..
    } = capture;
    let (identity, trailer) = builder.finish().map_err(capture_error)?;
    out.write_all(trailer.as_slice())
        .map_err(|_| ProjectError::Internal)?;
    if identity.artifact_bytes() != written + trailer.as_slice().len() as u64 {
        return Err(ProjectError::Internal);
    }
    out.flush().map_err(|_| ProjectError::Internal)?;
    durable(&out).map_err(|_| ProjectError::Internal)?;
    drop(out);

    // The rename is the publication: until it happens the artifact has a name
    // no reader looks for, and after it the name *is* the digest.
    let final_name = artifact_name(identity.tree_digest())?;
    match openat(&store_fd, final_name.as_str(), flags(false), Mode::empty()) {
        // Reuse by digest: an artifact under this name is already this capture.
        Ok(_) => {
            let _ = unlink(&store_fd, name.as_str());
            guard.armed = false;
            return Ok(identity);
        }
        Err(rustix::io::Errno::NOENT) => {}
        Err(error) => return Err(map_io(error)),
    }
    // The last cooperative checkpoint is *before* the rename. After it the
    // capture is committed, and answering `Cancelled` while a complete artifact
    // sits in the store would describe a run that left nothing when it left a
    // capture other runs will find by digest.
    control.check()?;
    rename(&store_fd, name.as_str(), &store_fd, final_name.as_str())
        .map_err(|_| ProjectError::Internal)?;
    guard.armed = false;
    durable(&store_fd).map_err(|_| ProjectError::Internal)?;
    Ok(identity)
}

/// Read an artifact back, re-deriving its identity one buffer at a time, and
/// refuse it when the digest is not the declared one (ADR-078 §2).
///
/// The returned file is positioned at its first byte and is the same open
/// object the verification read, so nothing can substitute the artifact between
/// the check and the use.
pub(crate) fn verify_vendor_capture(
    artifact: &Path,
    declared: &SourceFingerprint,
    control: &dyn OperationControl,
) -> Result<(VendorCapture, File), ProjectError> {
    control.check()?;
    let artifact = checked_path(artifact)?;
    let parent = artifact.parent().ok_or_else(invalid)?.to_path_buf();
    let backend = SecureProjects::new(&[parent])?;
    let fd = backend.open_path(&artifact, false)?;
    let before = CaptureFileStamp::of(&fd)?;
    let mut file = File::from(fd);
    let mut verifier = VendorCaptureVerifier::new(Sha256Hasher::default(), Sha256Hasher::default());
    let mut buffer = vec![0_u8; VENDOR_CAPTURE_READ_BUFFER_BYTES];
    loop {
        control.check()?;
        let read = file.read(&mut buffer).map_err(|_| invalid())?;
        if read == 0 {
            break;
        }
        verifier.chunk(&buffer[..read]).map_err(capture_error)?;
    }
    let capture = verifier.verified(declared).map_err(capture_error)?;
    control.check()?;
    if CaptureFileStamp::of(&file)? != before
        || capture.artifact_bytes() != before.inner.size.max(0) as u64
    {
        return Err(mutated());
    }
    file.rewind_to_start()?;
    Ok((capture, file))
}

/// `File::seek` is the only rewind this path needs, and it is spelled out so
/// the error becomes a `ProjectError` rather than an `io::Error`.
trait RewindToStart {
    fn rewind_to_start(&mut self) -> Result<(), ProjectError>;
}
impl RewindToStart for File {
    fn rewind_to_start(&mut self) -> Result<(), ProjectError> {
        use std::io::Seek;
        self.seek(std::io::SeekFrom::Start(0))
            .map(|_| ())
            .map_err(|_| ProjectError::Internal)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
// Fixed fixtures are malformed only by mistake; fail immediately.
mod tests;
