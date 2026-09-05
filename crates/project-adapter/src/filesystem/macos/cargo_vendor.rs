//! Complete handle-relative capture for an immutable Cargo directory source.
use super::*;
use rust_engineering_domain::{
    SOURCE_MAX_ENTRIES, SOURCE_MAX_FILE_BYTES, SOURCE_MAX_TOTAL_BYTES, SourceBundle, SourceError,
    SourceFile, validate_source_path,
};
use rustix::fs::Dir;

fn source_error(error: SourceError) -> ProjectError {
    rejected(match error {
        SourceError::Invalid => OperationalErrorCode::InvalidProject,
        SourceError::Limits => OperationalErrorCode::OutputLimitExceeded,
    })
}
fn limit() -> ProjectError {
    source_error(SourceError::Limits)
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryStamp {
    node: Node,
    mode: u16,
    uid: u32,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl DirectoryStamp {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct VendorFileStamp {
    inner: FileStamp,
    mode: u16,
    uid: u32,
}
impl VendorFileStamp {
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

struct Capture<'a> {
    backend: &'a SecureProjects,
    base: &'a Path,
    control: &'a dyn OperationControl,
    files: Vec<SourceFile>,
    directories: Vec<String>,
    observed_files: BTreeMap<PathBuf, VendorFileStamp>,
    observed_directories: BTreeMap<PathBuf, DirectoryStamp>,
    entries: usize,
    total: usize,
}
impl Capture<'_> {
    fn directory(&mut self, path: &Path) -> Result<(), ProjectError> {
        self.control.check()?;
        let fd = self.backend.open_path(path, true)?;
        let before = DirectoryStamp::of(&fd)?;
        let mut directory = Dir::read_from(&fd).map_err(map_io)?;
        let mut names = Vec::new();
        for entry in &mut directory {
            self.control.check()?;
            let entry = entry.map_err(map_io)?;
            let name = entry.file_name().to_str().map_err(|_| invalid())?;
            if matches!(name, "." | "..") {
                continue;
            }
            self.entries += 1;
            if self.entries > SOURCE_MAX_ENTRIES {
                return Err(limit());
            }
            let full = path.join(name);
            let relative = full.strip_prefix(self.base).map_err(|_| denied())?;
            validate_source_path(relative.to_str().ok_or_else(invalid)?).map_err(source_error)?;
            names.push((name.to_owned(), entry.file_type()));
        }
        names.sort_by(|left, right| left.0.cmp(&right.0));
        if names.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(invalid());
        }
        for (name, kind) in names {
            self.control.check()?;
            let full = path.join(name);
            if kind == FileType::Directory {
                self.directory(&full)?;
            } else if kind == FileType::RegularFile {
                self.file(&full)?;
            } else {
                return Err(denied());
            }
        }
        if DirectoryStamp::of(&fd)? != before {
            return Err(invalid());
        }
        if path != self.base {
            self.directories.push(
                path.strip_prefix(self.base)
                    .map_err(|_| denied())?
                    .to_str()
                    .ok_or_else(invalid)?
                    .to_owned(),
            );
        }
        self.observed_directories.insert(path.to_owned(), before);
        Ok(())
    }

    fn file(&mut self, path: &Path) -> Result<(), ProjectError> {
        self.control.check()?;
        let relative = path
            .strip_prefix(self.base)
            .map_err(|_| denied())?
            .to_str()
            .ok_or_else(invalid)?;
        validate_source_path(relative).map_err(source_error)?;
        let fd = self.backend.open_path(path, false)?;
        let before = VendorFileStamp::of(&fd)?;
        if before.inner.size < 0 || before.inner.size as u64 > SOURCE_MAX_FILE_BYTES as u64 {
            return Err(limit());
        }
        let remaining = SOURCE_MAX_TOTAL_BYTES
            .checked_sub(self.total)
            .ok_or_else(limit)?;
        let budget = SOURCE_MAX_FILE_BYTES.min(remaining);
        let mut file = File::from(fd);
        let mut bytes = Vec::new();
        (&mut file)
            .take(budget as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        self.control.check()?;
        if bytes.len() > budget
            || bytes.len() as i64 != before.inner.size
            || VendorFileStamp::of(&file)? != before
        {
            return Err(if bytes.len() > budget {
                limit()
            } else {
                invalid()
            });
        }
        self.total += bytes.len();
        self.files
            .push(SourceFile::new(relative.to_owned(), bytes).map_err(source_error)?);
        self.observed_files.insert(path.to_owned(), before);
        Ok(())
    }

    fn recheck(&self) -> Result<(), ProjectError> {
        for (path, stamp) in &self.observed_files {
            self.control.check()?;
            if VendorFileStamp::of(&self.backend.open_path(path, false)?)? != *stamp {
                return Err(invalid());
            }
        }
        for (path, stamp) in &self.observed_directories {
            self.control.check()?;
            if DirectoryStamp::of(&self.backend.open_path(path, true)?)? != *stamp {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

pub(crate) fn capture_cargo_vendor(
    path: &Path,
    control: &dyn OperationControl,
) -> Result<SourceBundle, ProjectError> {
    control.check()?;
    let path = checked_path(path)?;
    let backend = SecureProjects::new(std::slice::from_ref(&path))?;
    let root = backend.open_path(&path, true)?;
    let root_before = DirectoryStamp::of(&root)?;
    let mut capture = Capture {
        backend: &backend,
        base: &path,
        control,
        files: Vec::new(),
        directories: Vec::new(),
        observed_files: BTreeMap::new(),
        observed_directories: BTreeMap::new(),
        entries: 0,
        total: 0,
    };
    capture.directory(&path)?;
    let source = SourceBundle::with_directories(
        std::mem::take(&mut capture.files),
        std::mem::take(&mut capture.directories),
    )
    .map_err(source_error)?;
    capture.recheck()?;
    if DirectoryStamp::of(&root)? != root_before
        || DirectoryStamp::of(&backend.open_path(&path, true)?)? != root_before
    {
        return Err(invalid());
    }
    control.check()?;
    Ok(source)
}
