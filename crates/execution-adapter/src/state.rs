//! Private control files; never mount this directory into the guest.
use rust_engineering_application::ExecutionError;
use std::path::Path;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

pub fn valid_path(path: &Path) -> bool {
    path.is_absolute()
        && path.to_str().is_some_and(|s| {
            !s.is_empty()
                && s.len() <= 4096
                && !s.bytes().any(|b| b.is_ascii_control())
                && !s.contains("//")
                && !s.split('/').any(|c| c == "." || c == "..")
        })
}

pub fn nonce() -> Result<String, ExecutionError> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| ExecutionError::Infrastructure)?;
    let mut text = String::new();
    for byte in bytes {
        use std::fmt::Write;
        write!(text, "{byte:02x}").map_err(|_| ExecutionError::Infrastructure)?;
    }
    Ok(text)
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use rustix::fs::{
        AtFlags, CWD, FileType, Mode, OFlags, fstat, fstatfs, mkdirat, openat, unlinkat,
    };
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, OwnedFd};
    const SAFE: u32 = 0x2000_1000; // XNU NOFOLLOW_ANY + RESOLVE_BENEATH; no NOFOLLOW.
    fn error(_: impl std::fmt::Debug) -> ExecutionError {
        ExecutionError::InvalidConfiguration
    }
    fn dir_flags() -> OFlags {
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::DIRECTORY | OFlags::from_bits_retain(SAFE)
    }
    fn node(fd: &impl AsFd) -> Result<(i32, u64), ExecutionError> {
        let s = fstat(fd).map_err(error)?;
        Ok((s.st_dev, s.st_ino))
    }
    fn slash() -> Result<OwnedFd, ExecutionError> {
        let fd = openat(
            CWD,
            "/",
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(error)?;
        let dot = openat(&fd, ".", dir_flags(), Mode::empty()).map_err(error)?;
        if node(&dot)? != node(&fd)? {
            return Err(ExecutionError::Unavailable);
        }
        if !matches!(openat(&fd,"/",dir_flags(),Mode::empty()),Err(e) if e.raw_os_error()==107 /* XNU ENOTCAPABLE: absolute path violates BENEATH */)
            || !matches!(
                openat(&fd, ".", dir_flags() | OFlags::NOFOLLOW, Mode::empty()),
                Err(rustix::io::Errno::INVAL)
            )
        {
            return Err(ExecutionError::Unavailable);
        }
        Ok(fd)
    }
    pub struct State {
        root: OwnedFd,
        root_path: PathBuf,
        directory: OwnedFd,
        name: String,
        path: PathBuf,
    }
    impl State {
        pub fn new(root: &Path) -> Result<Self, ExecutionError> {
            if !valid_path(root) || root == Path::new("/") {
                return Err(ExecutionError::InvalidConfiguration);
            }
            let slash = slash()?;
            let rootfd = openat(
                &slash,
                root.strip_prefix("/").map_err(error)?,
                dir_flags(),
                Mode::empty(),
            )
            .map_err(error)?;
            let stat = fstat(&rootfd).map_err(error)?;
            if stat.st_uid != rustix::process::geteuid().as_raw() || stat.st_mode & 0o022 != 0 {
                return Err(ExecutionError::InvalidConfiguration);
            }
            let fs = fstatfs(&rootfd).map_err(error)?;
            let name: Vec<u8> = fs
                .f_fstypename
                .iter()
                .take_while(|b| **b != 0)
                .map(|b| *b as u8)
                .collect();
            if name != b"apfs" {
                return Err(ExecutionError::Unavailable);
            }
            let name = format!("rust-mcp-control-{}", nonce()?);
            mkdirat(&rootfd, &name, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(error)?;
            let directory = match openat(&rootfd, &name, dir_flags(), Mode::empty()) {
                Ok(fd) => fd,
                Err(e) => {
                    let _ = unlinkat(&rootfd, &name, AtFlags::REMOVEDIR);
                    return Err(error(e));
                }
            };
            let state = Self {
                root: rootfd,
                root_path: root.to_owned(),
                directory,
                name: name.clone(),
                path: root.join(name),
            };
            state.write("config.json", b"{}")?;
            state.write("seccomp.json", include_bytes!("seccomp.json"))?;
            state.write("seccomp-socket.json", include_bytes!("seccomp-socket.json"))?;
            state.write("seccomp-rust.json", include_bytes!("seccomp-rust.json"))?;
            Ok(state)
        }
        fn write(&self, name: &str, bytes: &[u8]) -> Result<(), ExecutionError> {
            let fd = openat(
                &self.directory,
                name,
                OFlags::WRONLY
                    | OFlags::CREATE
                    | OFlags::EXCL
                    | OFlags::CLOEXEC
                    | OFlags::from_bits_retain(SAFE),
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(error)?;
            File::from(fd).write_all(bytes).map_err(error)
        }
        pub fn path(&self) -> &Path {
            &self.path
        }
        pub fn check(&self) -> Result<(), ExecutionError> {
            let root = openat(
                slash()?,
                self.root_path.strip_prefix("/").map_err(error)?,
                dir_flags(),
                Mode::empty(),
            )
            .map_err(error)?;
            if node(&root)? != node(&self.root)? {
                return Err(ExecutionError::InvalidConfiguration);
            }
            let current =
                openat(&self.root, &self.name, dir_flags(), Mode::empty()).map_err(error)?;
            if node(&current)? != node(&self.directory)? {
                return Err(ExecutionError::InvalidConfiguration);
            }
            Ok(())
        }
    }
    impl Drop for State {
        fn drop(&mut self) {
            let _ = unlinkat(&self.directory, "config.json", AtFlags::empty());
            let _ = unlinkat(&self.directory, "seccomp.json", AtFlags::empty());
            let _ = unlinkat(&self.directory, "seccomp-socket.json", AtFlags::empty());
            let _ = unlinkat(&self.directory, "seccomp-rust.json", AtFlags::empty());
            if self.check().is_ok() {
                let _ = unlinkat(&self.root, &self.name, AtFlags::REMOVEDIR);
            }
        }
    }
    pub fn executable_bytes(path: &Path) -> Result<Vec<u8>, ExecutionError> {
        if !valid_path(path) {
            return Err(ExecutionError::InvalidConfiguration);
        }
        let fd = openat(
            slash()?,
            path.strip_prefix("/").map_err(error)?,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK | OFlags::from_bits_retain(SAFE),
            Mode::empty(),
        )
        .map_err(error)?;
        let before = fstat(&fd).map_err(error)?;
        if FileType::from_raw_mode(before.st_mode) != FileType::RegularFile
            || before.st_nlink != 1
            || before.st_size > 128 * 1024 * 1024
            || before.st_mode & 0o111 == 0
        {
            return Err(ExecutionError::InvalidConfiguration);
        }
        let mut file = File::from(fd);
        let mut bytes = Vec::new();
        (&mut file)
            .take(128 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        let after = fstat(&file).map_err(error)?;
        if bytes.len() > 128 * 1024 * 1024
            || before.st_ino != after.st_ino
            || before.st_size != after.st_size
            || before.st_ctime != after.st_ctime
            || before.st_ctime_nsec != after.st_ctime_nsec
        {
            return Err(ExecutionError::InvalidConfiguration);
        }
        Ok(bytes)
    }
}
#[cfg(target_os = "macos")]
pub use mac::*;

#[cfg(not(target_os = "macos"))]
pub struct State;
#[cfg(not(target_os = "macos"))]
impl State {
    pub fn new(_: &Path) -> Result<Self, ExecutionError> {
        Err(ExecutionError::Unavailable)
    }
    pub fn path(&self) -> &Path {
        Path::new("/")
    }
    pub fn check(&self) -> Result<(), ExecutionError> {
        Err(ExecutionError::Unavailable)
    }
}
#[cfg(not(target_os = "macos"))]
pub fn executable_bytes(_: &Path) -> Result<Vec<u8>, ExecutionError> {
    Err(ExecutionError::Unavailable)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    #[test]
    fn replaced_state_root_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let base = std::env::temp_dir().canonicalize()?.join(format!(
            "gateway-root-{}",
            nonce().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&base)?;
        let root = base.join("root");
        std::fs::create_dir(&root)?;
        let state = State::new(&root).map_err(|e| format!("{e:?}"))?;
        assert!(state.check().is_ok());
        std::fs::rename(&root, base.join("old"))?;
        std::fs::create_dir(&root)?;
        assert!(state.check().is_err());
        drop(state);
        std::fs::remove_dir_all(base)?;
        Ok(())
    }
}
