//! Source capture uses the same original-root authority and OS assumptions as M0.
use super::*;
use rust_engineering_application::ProjectSourceBackend;
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

fn capture_error(error: ProjectError) -> ProjectError {
    match error {
        ProjectError::Rejected(OperationalErrorCode::ProjectNotFound) => invalid(),
        other => other,
    }
}
fn excluded_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(".git") || name.eq_ignore_ascii_case("target")
}
fn cargo_config_path(path: &Path) -> bool {
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(".cargo"))
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.eq_ignore_ascii_case("config") || name.eq_ignore_ascii_case("config.toml")
            })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DirectoryStamp {
    node: Node,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl DirectoryStamp {
    fn of(fd: &impl AsFd) -> Result<Self, ProjectError> {
        let stat = fstat(fd).map_err(map_io)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::Directory {
            return Err(denied());
        }
        Ok(Self {
            node: Node {
                device: stat.st_dev,
                inode: stat.st_ino,
            },
            modified: (stat.st_mtime, stat.st_mtime_nsec),
            changed: (stat.st_ctime, stat.st_ctime_nsec),
        })
    }
}
struct Capture<'a> {
    backend: &'a SecureProjects,
    base: &'a Path,
    control: &'a dyn OperationControl,
    files: Vec<SourceFile>,
    observed_files: BTreeMap<PathBuf, FileStamp>,
    observed_directories: BTreeMap<PathBuf, DirectoryStamp>,
    entries: usize,
    total: usize,
}
impl Capture<'_> {
    fn directory(&mut self, path: &Path) -> Result<(), ProjectError> {
        self.control.check()?;
        let fd = self.backend.open_path(path, true).map_err(capture_error)?;
        let before = DirectoryStamp::of(&fd)?;
        let mut directory = Dir::read_from(&fd).map_err(|error| capture_error(map_io(error)))?;
        let mut names = Vec::new();
        for entry in &mut directory {
            self.control.check()?;
            let entry = entry.map_err(|error| capture_error(map_io(error)))?;
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
            // The directory-entry kind selects the open flags only. Metadata on
            // the no-follow descriptor independently validates the actual kind.
            names.push((name.to_owned(), entry.file_type()));
        }
        names.sort_by(|a, b| a.0.cmp(&b.0));
        if names.windows(2).any(|p| p[0].0 == p[1].0) {
            return Err(invalid());
        }
        for (name, kind) in names {
            self.control.check()?;
            let full = path.join(&name);
            if cargo_config_path(&full) {
                return Err(denied());
            }
            if kind == FileType::Directory {
                // Even excluded directory bindings are opened no-follow, so a
                // symlink named target or .git does not bypass the link policy.
                let child = self.backend.open_path(&full, true).map_err(capture_error)?;
                let child_stamp = DirectoryStamp::of(&child)?;
                if !excluded_name(&name) {
                    self.directory(&full)?;
                }
                if DirectoryStamp::of(&child)? != child_stamp
                    || DirectoryStamp::of(
                        &self.backend.open_path(&full, true).map_err(capture_error)?,
                    )? != child_stamp
                {
                    return Err(invalid());
                }
            } else if kind == FileType::RegularFile {
                self.file(&full)?;
            } else {
                // Reject FIFO/device/socket/link/unknown without opening it.
                return Err(denied());
            }
        }
        if DirectoryStamp::of(&fd)? != before {
            return Err(invalid());
        }
        self.observed_directories.insert(path.to_path_buf(), before);
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
        if cargo_config_path(path) {
            return Err(denied());
        }
        let fd = self.backend.open_path(path, false).map_err(capture_error)?;
        let before =
            FileStamp::from_stat(fstat(&fd).map_err(|error| capture_error(map_io(error)))?)?;
        if before.size < 0 || before.size as u64 > SOURCE_MAX_FILE_BYTES as u64 {
            return Err(limit());
        }
        let mut file = File::from(fd);
        let mut bytes = Vec::new();
        // Stop at the remaining shared budget as well as the per-file limit.
        let budget = SOURCE_MAX_FILE_BYTES.min(SOURCE_MAX_TOTAL_BYTES - self.total);
        (&mut file)
            .take(budget as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid())?;
        self.control.check()?;
        if bytes.len() > budget {
            return Err(limit());
        }
        if before
            != FileStamp::from_stat(fstat(&file).map_err(|error| capture_error(map_io(error)))?)?
            || bytes.len() as i64 != before.size
        {
            return Err(invalid());
        }
        validate_configuration(relative, &bytes)?;
        self.total += bytes.len();
        self.observed_files.insert(path.to_owned(), before);
        self.files
            .push(SourceFile::new(relative.to_owned(), bytes).map_err(source_error)?);
        Ok(())
    }
    fn recheck(&self) -> Result<(), ProjectError> {
        for (path, stamp) in &self.observed_files {
            self.control.check()?;
            if FileStamp::from_stat(
                fstat(&self.backend.open_path(path, false).map_err(capture_error)?)
                    .map_err(|error| capture_error(map_io(error)))?,
            )? != *stamp
            {
                return Err(invalid());
            }
        }
        for (path, stamp) in &self.observed_directories {
            self.control.check()?;
            if DirectoryStamp::of(&self.backend.open_path(path, true).map_err(capture_error)?)?
                != *stamp
            {
                return Err(invalid());
            }
        }
        Ok(())
    }
}

fn validate_configuration(path: &str, bytes: &[u8]) -> Result<(), ProjectError> {
    let name = path.rsplit('/').next().ok_or_else(invalid)?;
    if matches!(name, "Cargo.toml" | "rust-toolchain.toml") && bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(limit());
    }
    if name == "rust-toolchain" {
        if std::str::from_utf8(bytes).map_err(|_| invalid())?.trim() != "1.98.1" {
            return Err(denied());
        }
    } else if name == "rust-toolchain.toml" {
        let parsed: toml::Value =
            toml::from_str(std::str::from_utf8(bytes).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        let root = parsed.as_table().ok_or_else(invalid)?;
        let toolchain = root
            .get("toolchain")
            .and_then(toml::Value::as_table)
            .ok_or_else(invalid)?;
        if root.len() != 1
            || toolchain.len() != 1
            || toolchain.get("channel").and_then(toml::Value::as_str) != Some("1.98.1")
        {
            return Err(denied());
        }
    } else if name == "Cargo.toml" {
        let parsed: toml::Value =
            toml::from_str(std::str::from_utf8(bytes).map_err(|_| invalid())?)
                .map_err(|_| invalid())?;
        validate_cargo_paths(parsed.as_table().ok_or_else(invalid)?, path)?;
    }
    Ok(())
}

/// These are Cargo filesystem fields, not arbitrary strings in package.metadata.
/// M0 still validates the reachable manifest graph; this scan also covers captured
/// standalone manifests which that graph does not visit.
fn validate_cargo_paths(
    table: &toml::map::Map<String, toml::Value>,
    manifest: &str,
) -> Result<(), ProjectError> {
    if ["replace", "project", "cargo-features"]
        .iter()
        .any(|key| table.contains_key(*key))
    {
        return Err(invalid());
    }
    validate_dependency_groups(table, manifest)?;
    if let Some(package) = table.get("package") {
        validate_package_paths(package.as_table().ok_or_else(invalid)?, manifest, true)?;
    }
    if let Some(workspace) = table.get("workspace") {
        let workspace = workspace.as_table().ok_or_else(invalid)?;
        validate_dependency_groups(workspace, manifest)?;
        if let Some(package) = workspace.get("package") {
            validate_package_paths(package.as_table().ok_or_else(invalid)?, manifest, false)?;
        }
        for key in ["members", "default-members", "exclude"] {
            if let Some(paths) = workspace.get(key) {
                for path in paths.as_array().ok_or_else(invalid)? {
                    cargo_path(path.as_str().ok_or_else(invalid)?, manifest)?;
                }
            }
        }
    }
    if let Some(targets) = table.get("target") {
        for target in targets.as_table().ok_or_else(invalid)?.values() {
            validate_dependency_groups(target.as_table().ok_or_else(invalid)?, manifest)?;
        }
    }
    if let Some(patches) = table.get("patch") {
        for dependencies in patches.as_table().ok_or_else(invalid)?.values() {
            validate_dependencies(dependencies, manifest)?;
        }
    }
    if let Some(lib) = table.get("lib") {
        validate_target_path(lib, manifest)?;
    }
    for key in ["bin", "example", "test", "bench"] {
        if let Some(targets) = table.get(key) {
            for target in targets.as_array().ok_or_else(invalid)? {
                validate_target_path(target, manifest)?;
            }
        }
    }
    Ok(())
}
fn validate_target_path(target: &toml::Value, manifest: &str) -> Result<(), ProjectError> {
    if let Some(path) = target.as_table().ok_or_else(invalid)?.get("path") {
        cargo_path(path.as_str().ok_or_else(invalid)?, manifest)?;
    }
    Ok(())
}
fn validate_package_paths(
    package: &toml::map::Map<String, toml::Value>,
    manifest: &str,
    inheritance: bool,
) -> Result<(), ProjectError> {
    for key in ["build", "workspace", "readme", "license-file"] {
        if let Some(value) = package.get(key) {
            if inheritance
                && matches!(key, "readme" | "license-file")
                && value.as_table().is_some_and(|v| {
                    v.len() == 1 && v.get("workspace").and_then(toml::Value::as_bool) == Some(true)
                })
            {
                continue;
            }
            if matches!(key, "readme" | "build") && value.is_bool() {
                continue;
            }
            cargo_path(value.as_str().ok_or_else(invalid)?, manifest)?;
        }
    }
    // include/exclude are Cargo path patterns. This initial literal subset rejects
    // glob syntax rather than pretending to preserve unsupported expansion rules.
    for key in ["include", "exclude"] {
        if let Some(value) = package.get(key) {
            if inheritance
                && value.as_table().is_some_and(|v| {
                    v.len() == 1 && v.get("workspace").and_then(toml::Value::as_bool) == Some(true)
                })
            {
                continue;
            }
            for path in value.as_array().ok_or_else(invalid)? {
                cargo_path(path.as_str().ok_or_else(invalid)?, manifest)?;
            }
        }
    }
    Ok(())
}
fn validate_dependency_groups(
    table: &toml::map::Map<String, toml::Value>,
    manifest: &str,
) -> Result<(), ProjectError> {
    if table.contains_key("dev_dependencies") || table.contains_key("build_dependencies") {
        return Err(invalid());
    }
    for name in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(dependencies) = table.get(name) {
            validate_dependencies(dependencies, manifest)?;
        }
    }
    Ok(())
}
fn validate_dependencies(dependencies: &toml::Value, manifest: &str) -> Result<(), ProjectError> {
    for dependency in dependencies.as_table().ok_or_else(invalid)?.values() {
        if let Some(path) = dependency.as_table().and_then(|d| d.get("path")) {
            cargo_path(path.as_str().ok_or_else(invalid)?, manifest)?;
        }
    }
    Ok(())
}
fn cargo_path(path: &str, manifest: &str) -> Result<(), ProjectError> {
    if path.is_empty() || path.len() > 4096 || Path::new(path).is_absolute() {
        return Err(denied());
    }
    let mut components: Vec<_> = manifest.split('/').collect();
    components.pop();
    for component in path.split('/') {
        match component {
            ".." => {
                components.pop().ok_or_else(denied)?;
            }
            "" | "." => {}
            value => components.push(value),
        }
    }
    // The selected root itself is a valid relative directory target (e.g. ".").
    if !components.is_empty() {
        validate_source_path(&components.join("/")).map_err(source_error)?;
    }
    // Check raw components as well: normalizing must not erase unsupported names.
    for component in path.split('/').filter(|p| !matches!(*p, "" | "." | "..")) {
        if excluded_name(component) {
            return Err(denied());
        }
        validate_source_path(component).map_err(source_error)?;
    }
    Ok(())
}

/// The structural validator reads only captured bytes and cannot extend authority.
struct CapturedIo<'a> {
    root: &'a Path,
    bundle: &'a SourceBundle,
}
impl ManifestIo for CapturedIo<'_> {
    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, ProjectError> {
        let relative = path
            .strip_prefix(self.root)
            .map_err(|_| denied())?
            .to_str()
            .ok_or_else(invalid)?;
        Ok(self
            .bundle
            .files()
            .iter()
            .find(|f| f.path() == relative)
            .map(|f| f.bytes().to_vec()))
    }
    fn is_file(&self, path: &Path) -> Result<bool, ProjectError> {
        Ok(self.read_file(path)?.is_some())
    }
}
impl ProjectSourceBackend for SecureProjects {
    fn source(
        &self,
        lease: &ProjectLease,
        control: &dyn OperationControl,
    ) -> Result<SourceBundle, ProjectError> {
        let before = self.revalidate(lease, control)?;
        let mut capture = Capture {
            backend: self,
            base: &lease.path,
            control,
            files: Vec::new(),
            observed_files: BTreeMap::new(),
            observed_directories: BTreeMap::new(),
            entries: 0,
            total: 0,
        };
        capture.directory(&lease.path)?;
        let directories = capture
            .observed_directories
            .keys()
            .filter(|path| path.as_path() != lease.path)
            .map(|path| {
                path.strip_prefix(&lease.path)
                    .map_err(|_| denied())?
                    .to_str()
                    .map(str::to_owned)
                    .ok_or_else(invalid)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let bundle =
            SourceBundle::with_directories(std::mem::take(&mut capture.files), directories)
                .map_err(source_error)?;
        manifest::validate(
            &CapturedIo {
                root: &lease.path,
                bundle: &bundle,
            },
            &lease.path,
            control,
        )?;
        if self.revalidate(lease, control)? != before {
            return Err(invalid());
        }
        capture.recheck()?;
        control.check()?;
        Ok(bundle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OsReferences;
    use rust_engineering_application::ReferenceGenerator;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[test]
    fn mutation_between_file_read_and_metadata_check_rejects() -> Result<(), String> {
        let root = PathBuf::from("/private/tmp").join(format!(
            "rms-read-{}",
            OsReferences.generate().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let path = root.join("file");
        std::fs::write(&path, "before").map_err(|e| e.to_string())?;
        struct Mutate {
            path: PathBuf,
            calls: AtomicUsize,
        }
        impl OperationControl for Mutate {
            fn check(&self) -> Result<(), ProjectError> {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 1 {
                    std::fs::write(&self.path, "after").map_err(|_| ProjectError::Internal)?;
                }
                Ok(())
            }
        }
        let backend =
            SecureProjects::new(std::slice::from_ref(&root)).map_err(|e| format!("{e:?}"))?;
        let control = Mutate {
            path: path.clone(),
            calls: AtomicUsize::new(0),
        };
        let mut capture = Capture {
            backend: &backend,
            base: &root,
            control: &control,
            files: Vec::new(),
            observed_files: BTreeMap::new(),
            observed_directories: BTreeMap::new(),
            entries: 0,
            total: 0,
        };
        assert_eq!(capture.file(&path), Err(invalid()));
        assert!(capture.files.is_empty());
        Ok(())
    }
}

#[cfg(test)]
mod entry_race_tests {
    use super::*;
    use crate::OsReferences;
    use rust_engineering_application::ReferenceGenerator;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Swap {
        calls: AtomicUsize,
        at: usize,
        file: PathBuf,
        alias: PathBuf,
    }
    impl OperationControl for Swap {
        fn check(&self) -> Result<(), ProjectError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.at {
                std::fs::hard_link(&self.file, &self.alias).map_err(|_| ProjectError::Internal)?;
            }
            Ok(())
        }
    }
    #[test]
    fn stale_regular_directory_entry_cannot_authorize_hardlink() -> Result<(), String> {
        let root = PathBuf::from("/private/tmp").join(format!(
            "rms-race-{}",
            OsReferences.generate().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let file = root.join("file");
        std::fs::write(&file, "original").map_err(|e| e.to_string())?;
        let backend =
            SecureProjects::new(std::slice::from_ref(&root)).map_err(|e| format!("{e:?}"))?;
        let fd = backend
            .open_path(&root, true)
            .map_err(|e| format!("{e:?}"))?;
        let entries = Dir::read_from(&fd)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
            .len();
        // directory() checks once before opening, once per readdir entry,
        // then once before consuming the already-collected regular d_type.
        let control = Swap {
            calls: AtomicUsize::new(0),
            at: entries + 2,
            file,
            alias: root.join("alias"),
        };
        let mut capture = Capture {
            backend: &backend,
            base: &root,
            control: &control,
            files: Vec::new(),
            observed_files: BTreeMap::new(),
            observed_directories: BTreeMap::new(),
            entries: 0,
            total: 0,
        };
        assert!(capture.directory(&root).is_err());
        assert!(control.calls.load(Ordering::SeqCst) >= control.at);
        assert!(capture.files.is_empty());
        Ok(())
    }
}

#[cfg(test)]
mod direct_file_validation_tests {
    use super::*;
    struct Continue;
    impl OperationControl for Continue {
        fn check(&self) -> Result<(), ProjectError> {
            Ok(())
        }
    }
    #[test]
    fn invalid_relative_path_is_rejected_before_any_filesystem_open() -> Result<(), ProjectError> {
        // No roots: a filesystem attempt would return SandboxDenied. These bad
        // relative names must instead fail the source path validator immediately.
        let backend = SecureProjects::new(&[])?;
        let base = Path::new("/not-an-authorized-root");
        let mut capture = Capture {
            backend: &backend,
            base,
            control: &Continue,
            files: Vec::new(),
            observed_files: BTreeMap::new(),
            observed_directories: BTreeMap::new(),
            entries: 0,
            total: 0,
        };
        for name in ["space name", "../outside", "back\\slash"] {
            assert_eq!(capture.file(&base.join(name)), Err(invalid()));
        }
        assert!(capture.files.is_empty());
        Ok(())
    }
}
