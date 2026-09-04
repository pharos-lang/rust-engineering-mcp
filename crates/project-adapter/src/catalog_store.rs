//! Host-selected durable catalog storage. Bundle authentication belongs to the caller.
use std::path::Path;

pub const MAX_CATALOG_FILE_BYTES: usize = 80 * 1024 * 1024;
pub const MAX_MODEL_FILE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_FLOOR_FILE_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    UnsupportedPlatform,
    InvalidPath,
    Denied,
    Busy,
    LimitExceeded,
    Changed,
    Io,
    /// Replacement may have occurred. Reopen and authenticate the active record.
    DurabilityUncertain,
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "catalog store: {self:?}")
    }
}
impl std::error::Error for StoreError {}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use rustix::fs::{
        AtFlags, CWD, FileType, FlockOperation, Mode, OFlags, Stat, fcntl_fullfsync, flock, fstat,
        fstatfs, fsync, openat, renameat, unlinkat,
    };
    use rustix::io::Errno;
    use std::cell::Cell;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, OwnedFd};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    // XNU O_NOFOLLOW_ANY and O_RESOLVE_BENEATH, absent from rustix 1.1.4.
    // BENEATH on '/' only probes flag support; NOFOLLOW_ANY protects that initial
    // full path. Per-file BENEATH is anchored at the retained authorized directory.
    const SAFE: u32 = 0x2000_1000; // NOFOLLOW_ANY | RESOLVE_BENEATH
    // XNU SDK sys/fcntl.h: O_UNIQUE == 0x00002000 rejects multiply-linked vnodes.
    // Real hardlink fixtures exercise the flag; fstat(st_nlink == 1) remains an
    // independent mandatory check, so ignoring the flag never grants authority.
    const UNIQUE: u32 = 0x2000;
    const ACTIVE: &str = "active.bundle";
    const STAGING: &str = "staging.bundle";
    const LOCK: &str = "store.lock";
    const MAX_READ_TIME: Duration = Duration::from_secs(60);

    fn io(error: Errno) -> StoreError {
        match error {
            Errno::LOOP | Errno::ACCESS | Errno::PERM => StoreError::Denied,
            _ if error.raw_os_error() == 107 => StoreError::Denied,
            _ => StoreError::Io,
        }
    }
    fn path_checked(path: &Path) -> Result<(), StoreError> {
        let text = path.to_str().ok_or(StoreError::InvalidPath)?;
        if !path.is_absolute()
            || path == Path::new("/")
            || text.len() > 4096
            || text.bytes().any(|b| b.is_ascii_control())
            || text.contains("//")
            || text.split('/').any(|part| part == "." || part == "..")
            || path.components().count() > 64
        {
            return Err(StoreError::InvalidPath);
        }
        Ok(())
    }
    fn flags(directory: bool) -> OFlags {
        let common = OFlags::CLOEXEC | OFlags::from_bits_retain(SAFE);
        if directory {
            common | OFlags::RDONLY | OFlags::DIRECTORY
        } else {
            common | OFlags::NONBLOCK | OFlags::NOCTTY | OFlags::from_bits_retain(UNIQUE)
        }
    }
    fn apfs(fd: &impl AsFd) -> Result<(), StoreError> {
        let stat = fstatfs(fd).map_err(io)?;
        if !stat
            .f_fstypename
            .iter()
            .map(|b| *b as u8)
            .take_while(|b| *b != 0)
            .eq(b"apfs".iter().copied())
        {
            return Err(StoreError::UnsupportedPlatform);
        }
        Ok(())
    }
    fn private(stat: &Stat, mode: u16) -> Result<(), StoreError> {
        if stat.st_uid != rustix::process::geteuid().as_raw() || stat.st_mode & 0o7777 != mode {
            return Err(StoreError::Denied);
        }
        Ok(())
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Node(i32, u64);
    fn node(stat: &Stat) -> Node {
        Node(stat.st_dev, stat.st_ino)
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Stamp {
        node: Node,
        size: i64,
        modified: (i64, i64),
        changed: (i64, i64),
        mode: u16,
        owner: u32,
    }
    fn stamp(fd: &impl AsFd, max: usize, is_private: bool) -> Result<Stamp, StoreError> {
        let stat = fstat(fd).map_err(io)?;
        if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_nlink != 1 {
            return Err(StoreError::Denied);
        }
        if stat.st_size < 0 || stat.st_size as u64 > max as u64 {
            return Err(StoreError::LimitExceeded);
        }
        if is_private {
            private(&stat, 0o600)?;
        }
        Ok(Stamp {
            node: node(&stat),
            size: stat.st_size,
            modified: (stat.st_mtime, stat.st_mtime_nsec),
            changed: (stat.st_ctime, stat.st_ctime_nsec),
            mode: stat.st_mode,
            owner: stat.st_uid,
        })
    }

    struct Root {
        slash: OwnedFd,
        directory: OwnedFd,
        path: PathBuf,
        node: Node,
        private: bool,
    }
    impl Root {
        fn open(path: &Path, is_private: bool) -> Result<Self, StoreError> {
            path_checked(path)?;
            let kernel = rustix::system::uname();
            if !kernel
                .release()
                .to_str()
                .ok()
                .and_then(|s| s.split('.').next())
                .and_then(|s| s.parse::<u32>().ok())
                .is_some_and(|major| major >= 25)
            {
                return Err(StoreError::UnsupportedPlatform);
            }
            let slash = openat(
                CWD,
                "/",
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(io)?;
            let dot = openat(&slash, ".", flags(true), Mode::empty())
                .map_err(|_| StoreError::UnsupportedPlatform)?;
            if node(&fstat(&slash).map_err(io)?) != node(&fstat(&dot).map_err(io)?)
                || !matches!(
                    openat(&slash, ".", flags(true) | OFlags::NOFOLLOW, Mode::empty()),
                    Err(Errno::INVAL)
                )
                || !matches!(openat(&slash, "/", flags(true), Mode::empty()), Err(e) if e.raw_os_error() == 107)
            {
                return Err(StoreError::UnsupportedPlatform);
            }
            let directory = openat(
                &slash,
                path.strip_prefix("/")
                    .map_err(|_| StoreError::InvalidPath)?,
                flags(true),
                Mode::empty(),
            )
            .map_err(io)?;
            apfs(&directory)?;
            let stat = fstat(&directory).map_err(io)?;
            if is_private {
                private(&stat, 0o700)?;
            }
            let root = Self {
                slash,
                directory,
                path: path.to_owned(),
                node: node(&stat),
                private: is_private,
            };
            root.check()?;
            Ok(root)
        }
        fn check(&self) -> Result<(), StoreError> {
            let current = openat(
                &self.slash,
                self.path
                    .strip_prefix("/")
                    .map_err(|_| StoreError::InvalidPath)?,
                flags(true),
                Mode::empty(),
            )
            .map_err(|_| StoreError::Changed)?;
            for fd in [&current, &self.directory] {
                let stat = fstat(fd).map_err(io)?;
                if node(&stat) != self.node {
                    return Err(StoreError::Changed);
                }
                if self.private {
                    private(&stat, 0o700)?;
                }
            }
            Ok(())
        }
        fn file(&self, name: &std::ffi::OsStr) -> Result<Option<OwnedFd>, StoreError> {
            self.check()?;
            let fd = match openat(
                &self.directory,
                name,
                flags(false) | OFlags::RDONLY,
                Mode::empty(),
            ) {
                Ok(fd) => fd,
                Err(Errno::NOENT) => {
                    self.check()?;
                    return Ok(None);
                }
                Err(e) => return Err(io(e)),
            };
            apfs(&fd)?;
            if node(&fstat(&fd).map_err(io)?).0 != self.node.0 {
                return Err(StoreError::Denied);
            }
            self.check()?;
            Ok(Some(fd))
        }
        fn read(&self, name: &std::ffi::OsStr, max: usize) -> Result<Option<Vec<u8>>, StoreError> {
            // Cooperative budget only: individual native I/O calls cannot be
            // interrupted by this clock, but late reads never report success.
            let started = Instant::now();
            let Some(fd) = self.file(name)? else {
                return Ok(None);
            };
            let before = stamp(&fd, max, self.private)?;
            let mut file = File::from(fd);
            let mut bytes = Vec::new();
            let mut chunk = [0; 64 * 1024];
            loop {
                if started.elapsed() >= MAX_READ_TIME {
                    return Err(StoreError::LimitExceeded);
                }
                let allowed = chunk.len().min(max - bytes.len() + 1);
                let count = file
                    .read(&mut chunk[..allowed])
                    .map_err(|_| StoreError::Io)?;
                if count == 0 {
                    break;
                }
                if count > max - bytes.len() {
                    return Err(StoreError::LimitExceeded);
                }
                bytes
                    .try_reserve(count)
                    .map_err(|_| StoreError::LimitExceeded)?;
                bytes.extend_from_slice(&chunk[..count]);
            }
            let reopened = self.file(name)?.ok_or(StoreError::Changed)?;
            if stamp(&file, max, self.private)? != before
                || stamp(&reopened, max, self.private)? != before
                || bytes.len() as i64 != before.size
            {
                return Err(StoreError::Changed);
            }
            self.check()?;
            if started.elapsed() >= MAX_READ_TIME {
                return Err(StoreError::LimitExceeded);
            }
            Ok(Some(bytes))
        }
        fn durable(&self) -> Result<(), StoreError> {
            fsync(&self.directory).map_err(io)?;
            fcntl_fullfsync(&self.directory).map_err(io)
        }
    }

    struct TrustAncestor {
        relative: PathBuf,
        directory: OwnedFd,
        node: Node,
    }
    struct TrustAncestors(Vec<TrustAncestor>);

    fn trust_ancestor_mode(
        stat: &Stat,
        relative: &Path,
        below_sticky: bool,
    ) -> Result<bool, StoreError> {
        let owner = rustix::process::geteuid().as_raw();
        if stat.st_uid != 0 && stat.st_uid != owner {
            return Err(StoreError::Denied);
        }
        if below_sticky {
            // In root-owned /private/tmp the sticky bit prevents third parties
            // from unlinking/renaming this user-owned entry; 0700 prevents entry.
            private(stat, 0o700)?;
        }
        if stat.st_mode & 0o022 == 0 {
            return Ok(false);
        }
        if relative == Path::new("private/tmp")
            && stat.st_uid == 0
            && stat.st_mode & 0o7777 == 0o1777
        {
            return Ok(true);
        }
        Err(StoreError::Denied)
    }

    impl TrustAncestors {
        fn open(root: &Root) -> Result<Self, StoreError> {
            trust_ancestor_mode(&fstat(&root.slash).map_err(io)?, Path::new("."), false)?;
            let mut ancestors = Vec::new();
            let mut relative = PathBuf::new();
            let mut below_sticky = false;
            for component in root
                .path
                .strip_prefix("/")
                .map_err(|_| StoreError::InvalidPath)?
                .components()
            {
                relative.push(component);
                // Always resolve the complete prefix through '/', not through a
                // descendant descriptor that could have moved to another parent.
                let directory =
                    openat(&root.slash, &relative, flags(true), Mode::empty()).map_err(io)?;
                let stat = fstat(&directory).map_err(io)?;
                below_sticky = trust_ancestor_mode(&stat, &relative, below_sticky)?;
                ancestors.push(TrustAncestor {
                    relative: relative.clone(),
                    directory,
                    node: node(&stat),
                });
            }
            if below_sticky {
                return Err(StoreError::Denied);
            }
            let ancestors = Self(ancestors);
            ancestors.check(root)?;
            Ok(ancestors)
        }
        fn check(&self, root: &Root) -> Result<(), StoreError> {
            trust_ancestor_mode(&fstat(&root.slash).map_err(io)?, Path::new("."), false)?;
            let mut below_sticky = false;
            for ancestor in &self.0 {
                let current = openat(&root.slash, &ancestor.relative, flags(true), Mode::empty())
                    .map_err(|_| StoreError::Changed)?;
                let held_stat = fstat(&ancestor.directory).map_err(io)?;
                let current_stat = fstat(&current).map_err(io)?;
                if node(&held_stat) != ancestor.node || node(&current_stat) != ancestor.node {
                    return Err(StoreError::Changed);
                }
                let held_sticky =
                    trust_ancestor_mode(&held_stat, &ancestor.relative, below_sticky)?;
                below_sticky =
                    trust_ancestor_mode(&current_stat, &ancestor.relative, below_sticky)?;
                if held_sticky != below_sticky {
                    return Err(StoreError::Changed);
                }
            }
            if below_sticky {
                return Err(StoreError::Denied);
            }
            root.check()
        }
    }

    /// Exclusive lease on an existing host-owned mode-0700 APFS directory.
    /// The owner must preserve this trusted state, including the permanent lock.
    /// Authenticate read_active bytes and enforce sequence before calling commit.
    pub struct CatalogStore {
        root: Root,
        lock: OwnedFd,
        lock_node: Node,
        seen_active: Cell<bool>,
        seen_floor: Cell<bool>,
    }
    #[derive(Clone, Copy)]
    enum Record {
        Bundle,
        Floor,
    }
    impl Record {
        fn active(self) -> &'static str {
            match self {
                Self::Bundle => ACTIVE,
                Self::Floor => "floor.record",
            }
        }
        fn staging(self) -> &'static str {
            match self {
                Self::Bundle => STAGING,
                Self::Floor => "floor.staging",
            }
        }
        fn max_bytes(self) -> usize {
            match self {
                Self::Bundle => MAX_CATALOG_FILE_BYTES,
                Self::Floor => MAX_FLOOR_FILE_BYTES,
            }
        }
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum CommitPhase {
        Staged,
        Replaced,
    }
    impl CatalogStore {
        pub fn open(path: &Path) -> Result<Self, StoreError> {
            let root = Root::open(path, true)?;
            let lock = openat(
                &root.directory,
                LOCK,
                flags(false) | OFlags::RDWR | OFlags::CREATE,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(io)?;
            let lock_stamp = stamp(&lock, 0, true)?;
            flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(|e| {
                if e == Errno::WOULDBLOCK {
                    StoreError::Busy
                } else {
                    io(e)
                }
            })?;
            let store = Self {
                root,
                lock,
                lock_node: lock_stamp.node,
                seen_active: Cell::new(false),
                seen_floor: Cell::new(false),
            };
            store.check()?;
            // No record is ever promoted from staging, even when active is absent.
            store.discard_staging(Record::Bundle)?;
            store.discard_staging(Record::Floor)?;
            store.root.durable()?;
            Ok(store)
        }
        fn check(&self) -> Result<(), StoreError> {
            self.root.check()?;
            let current = self.root.file(LOCK.as_ref())?.ok_or(StoreError::Changed)?;
            if stamp(&current, 0, true)?.node != self.lock_node
                || stamp(&self.lock, 0, true)?.node != self.lock_node
            {
                return Err(StoreError::Changed);
            }
            Ok(())
        }
        fn seen(&self, record: Record) -> &Cell<bool> {
            match record {
                Record::Bundle => &self.seen_active,
                Record::Floor => &self.seen_floor,
            }
        }
        fn discard_staging(&self, record: Record) -> Result<(), StoreError> {
            self.check()?;
            if let Some(fd) = self.root.file(record.staging().as_ref())? {
                stamp(&fd, record.max_bytes(), true)?;
                self.check()?;
                unlinkat(&self.root.directory, record.staging(), AtFlags::empty()).map_err(io)?;
                self.root.durable()?;
            }
            self.check()
        }
        pub fn read_active(&self) -> Result<Option<Vec<u8>>, StoreError> {
            self.read_record(Record::Bundle)
        }
        pub fn read_floor(&self) -> Result<Option<Vec<u8>>, StoreError> {
            self.read_record(Record::Floor)
        }
        fn read_record(&self, record: Record) -> Result<Option<Vec<u8>>, StoreError> {
            self.check()?;
            let bytes = self
                .root
                .read(record.active().as_ref(), record.max_bytes())?;
            if bytes.is_none() && self.seen(record).get() {
                return Err(StoreError::Changed);
            }
            if bytes.is_some() {
                self.seen(record).set(true);
            }
            self.check()?;
            Ok(bytes)
        }
        pub fn commit(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
            self.commit_checked(bytes, |_| Ok(()))
        }
        /// Atomically persists opaque floor bytes. The caller validates the record
        /// and its monotonic sequence, then reserves it before committing a bundle.
        pub fn reserve_floor(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
            self.replace_record(Record::Floor, bytes, |_| Ok(()))
        }
        // Private checkpoint injection lets real-filesystem tests interrupt both
        // sides of the atomic rename, without exposing fault controls to callers.
        fn commit_checked(
            &mut self,
            bytes: &[u8],
            checkpoint: impl FnMut(CommitPhase) -> Result<(), StoreError>,
        ) -> Result<(), StoreError> {
            self.replace_record(Record::Bundle, bytes, checkpoint)
        }
        fn replace_record(
            &mut self,
            record: Record,
            bytes: &[u8],
            mut checkpoint: impl FnMut(CommitPhase) -> Result<(), StoreError>,
        ) -> Result<(), StoreError> {
            if bytes.len() > record.max_bytes() {
                return Err(StoreError::LimitExceeded);
            }
            self.check()?;
            // Structural validation never treats a hostile active name as absent.
            let active = self.root.file(record.active().as_ref())?;
            if let Some(fd) = &active {
                stamp(fd, record.max_bytes(), true)?;
            } else if self.seen(record).get() {
                return Err(StoreError::Changed);
            }
            self.discard_staging(record)?;
            let fd = openat(
                &self.root.directory,
                record.staging(),
                flags(false) | OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
                Mode::RUSR | Mode::WUSR,
            )
            .map_err(io)?;
            let mut file = File::from(fd);
            stamp(&file, 0, true)?;
            file.write_all(bytes).map_err(|_| StoreError::Io)?;
            fsync(&file).map_err(io)?;
            fcntl_fullfsync(&file).map_err(io)?;
            checkpoint(CommitPhase::Staged)?;
            let before = stamp(&file, record.max_bytes(), true)?;
            let staged = self
                .root
                .file(record.staging().as_ref())?
                .ok_or(StoreError::Changed)?;
            if before.size as usize != bytes.len()
                || stamp(&staged, record.max_bytes(), true)? != before
            {
                return Err(StoreError::Changed);
            }
            self.check()?;
            renameat(
                &self.root.directory,
                record.staging(),
                &self.root.directory,
                record.active(),
            )
            .map_err(io)?;
            // From this point every failure is explicitly uncertain; never undo.
            self.seen(record).set(true);
            let mut finish = || {
                checkpoint(CommitPhase::Replaced)?;
                self.root.durable()?;
                self.check()?;
                let current = self
                    .root
                    .file(record.active().as_ref())?
                    .ok_or(StoreError::Changed)?;
                if stamp(&current, record.max_bytes(), true)?.node != before.node {
                    return Err(StoreError::Changed);
                }
                Ok(())
            };
            finish().map_err(|_: StoreError| StoreError::DurabilityUncertain)
        }
    }

    /// Acquires a bounded, stable regular file from an explicit host input path.
    /// No filesystem-selected bytes are trusted until the caller authenticates them.
    pub fn read_catalog_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, StoreError> {
        if max_bytes > MAX_CATALOG_FILE_BYTES {
            return Err(StoreError::LimitExceeded);
        }
        path_checked(path)?;
        let root = Root::open(path.parent().ok_or(StoreError::InvalidPath)?, false)?;
        root.read(path.file_name().ok_or(StoreError::InvalidPath)?, max_bytes)?
            .ok_or(StoreError::Io)
    }

    /// Acquires a trust anchor owned by the current user, mode 0600 in a 0700
    /// parent. Root/current-user ancestors cannot be group/other-writable, except
    /// root-owned sticky /private/tmp immediately followed by a private user dir.
    /// Checks POSIX ownership/modes; host provisioning must not grant ACL access.
    pub fn read_trust_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, StoreError> {
        read_private_optional_file(path, max_bytes)?.ok_or(StoreError::Io)
    }

    /// Read-only acquisition of an optional private host file with the same
    /// ownership, ancestor, and no-follow checks as a trust anchor. Does not
    /// create a lock, clean staging files, or mutate the directory.
    pub fn read_private_optional_file(
        path: &Path,
        max_bytes: usize,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        read_private_checked(path, max_bytes, || Ok(()))
    }

    fn read_private_checked(
        path: &Path,
        max_bytes: usize,
        after_read: impl FnOnce() -> Result<(), StoreError>,
    ) -> Result<Option<Vec<u8>>, StoreError> {
        if max_bytes > MAX_CATALOG_FILE_BYTES {
            return Err(StoreError::LimitExceeded);
        }
        path_checked(path)?;
        let root = Root::open(path.parent().ok_or(StoreError::InvalidPath)?, true)?;
        let ancestors = TrustAncestors::open(&root)?;
        let bytes = root.read(path.file_name().ok_or(StoreError::InvalidPath)?, max_bytes)?;
        after_read()?;
        ancestors.check(&root)?;
        Ok(bytes)
    }

    /// Acquires explicit host model input with a separate 512 MiB ceiling.
    /// The caller must verify the pinned model size and hash before parsing it.
    /// Reads have a 60-second cooperative budget, not a hard native-I/O timeout.
    pub fn read_model_file(path: &Path, max_bytes: usize) -> Result<Vec<u8>, StoreError> {
        if max_bytes > MAX_MODEL_FILE_BYTES {
            return Err(StoreError::LimitExceeded);
        }
        path_checked(path)?;
        let root = Root::open(path.parent().ok_or(StoreError::InvalidPath)?, false)?;
        root.read(path.file_name().ok_or(StoreError::InvalidPath)?, max_bytes)?
            .ok_or(StoreError::Io)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use rust_engineering_application::ReferenceGenerator;
        use std::os::unix::fs::PermissionsExt;

        #[test]
        fn interrupted_commit_preserves_complete_generation_and_reports_uncertainty()
        -> Result<(), Box<dyn std::error::Error>> {
            for (phase, record) in [
                (CommitPhase::Staged, Record::Bundle),
                (CommitPhase::Replaced, Record::Bundle),
                (CommitPhase::Staged, Record::Floor),
                (CommitPhase::Replaced, Record::Floor),
            ] {
                let id = crate::OsReferences
                    .generate()
                    .map_err(|e| format!("{e:?}"))?;
                let root = std::env::temp_dir()
                    .canonicalize()?
                    .join(format!("catalog-fault-{id}"));
                struct Cleanup(PathBuf);
                impl Drop for Cleanup {
                    fn drop(&mut self) {
                        let _ = std::fs::remove_dir_all(&self.0);
                    }
                }
                std::fs::create_dir(&root)?;
                let _cleanup = Cleanup(root.clone());
                std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
                let mut store = CatalogStore::open(&root)?;
                store.commit(b"previous-complete")?;
                store.reserve_floor(b"previous-complete")?;
                let error = store.replace_record(record, b"next-complete", |at| {
                    if at == phase {
                        Err(StoreError::Io)
                    } else {
                        Ok(())
                    }
                });
                assert_eq!(
                    error,
                    Err(if phase == CommitPhase::Staged {
                        StoreError::Io
                    } else {
                        StoreError::DurabilityUncertain
                    })
                );
                drop(store);
                let mut reopened = CatalogStore::open(&root)?;
                let expected = if phase == CommitPhase::Staged {
                    b"previous-complete".as_slice()
                } else {
                    b"next-complete".as_slice()
                };
                assert_eq!(reopened.read_record(record)?.as_deref(), Some(expected));
                let other = match record {
                    Record::Bundle => Record::Floor,
                    Record::Floor => Record::Bundle,
                };
                assert_eq!(
                    reopened.read_record(other)?.as_deref(),
                    Some(b"previous-complete".as_slice())
                );
                assert!(!root.join(record.staging()).exists());
                reopened.replace_record(record, b"recovered-complete", |_| Ok(()))?;
                assert_eq!(
                    reopened.read_record(record)?.as_deref(),
                    Some(b"recovered-complete".as_slice())
                );
            }
            Ok(())
        }

        #[test]
        fn sdk_unique_flag_and_stat_both_reject_hardlinks() -> Result<(), Box<dyn std::error::Error>>
        {
            let id = crate::OsReferences
                .generate()
                .map_err(|e| format!("{e:?}"))?;
            let path = std::env::temp_dir()
                .canonicalize()?
                .join(format!("catalog-unique-{id}"));
            struct Cleanup(PathBuf);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    let _ = std::fs::remove_dir_all(&self.0);
                }
            }
            std::fs::create_dir(&path)?;
            let _cleanup = Cleanup(path.clone());
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))?;
            std::fs::write(path.join("first"), b"x")?;
            std::fs::set_permissions(path.join("first"), std::fs::Permissions::from_mode(0o600))?;
            std::fs::hard_link(path.join("first"), path.join("second"))?;
            let root = Root::open(&path, true)?;
            assert!(
                openat(
                    &root.directory,
                    "first",
                    flags(false) | OFlags::RDONLY,
                    Mode::empty()
                )
                .is_err()
            );
            let fd = openat(
                &root.directory,
                "first",
                (flags(false) & !OFlags::from_bits_retain(UNIQUE)) | OFlags::RDONLY,
                Mode::empty(),
            )?;
            assert_eq!(stamp(&fd, 1, true), Err(StoreError::Denied));
            Ok(())
        }

        #[test]
        fn private_read_rechecks_ancestors_even_when_file_is_absent()
        -> Result<(), Box<dyn std::error::Error>> {
            let id = crate::OsReferences
                .generate()
                .map_err(|e| format!("{e:?}"))?;
            let base = std::env::temp_dir()
                .canonicalize()?
                .join(format!("catalog-read-race-{id}"));
            struct Cleanup(PathBuf);
            impl Drop for Cleanup {
                fn drop(&mut self) {
                    let _ = std::fs::remove_dir_all(&self.0);
                }
            }
            std::fs::create_dir(&base)?;
            let _cleanup = Cleanup(base.clone());
            std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))?;
            let parent = base.join("parent");
            std::fs::create_dir(&parent)?;
            std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))?;
            for present in [false, true] {
                let path = parent.join("record");
                if present {
                    std::fs::write(&path, b"record")?;
                    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
                }
                let result = read_private_checked(&path, 6, || {
                    std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o777))
                        .map_err(|_| StoreError::Io)
                });
                assert_eq!(result, Err(StoreError::Denied));
                std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700))?;
            }
            let result = read_private_checked(&parent.join("record"), 6, || {
                std::fs::rename(&parent, base.join("moved")).map_err(|_| StoreError::Io)?;
                std::fs::create_dir(&parent).map_err(|_| StoreError::Io)?;
                std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(|_| StoreError::Io)
            });
            assert_eq!(result, Err(StoreError::Changed));
            assert!(!parent.join("store.lock").exists());
            Ok(())
        }

        #[test]
        fn ownership_predicates_reject_another_principal() -> Result<(), Box<dyn std::error::Error>>
        {
            let root = Root::open(Path::new("/private/tmp"), false)?;
            let mut stat = fstat(&root.directory)?;
            let owner = rustix::process::geteuid().as_raw();
            // Changing ownership of a real file requires privilege. Exercise the
            // exact predicates with real Stat metadata and a substituted owner.
            stat.st_uid = if owner == 1 { 2 } else { 1 };
            stat.st_mode = (stat.st_mode & !0o7777) | 0o700;
            assert_eq!(private(&stat, 0o700), Err(StoreError::Denied));
            assert_eq!(
                trust_ancestor_mode(&stat, Path::new("private/ancestor"), false),
                Err(StoreError::Denied)
            );
            stat.st_mode = (stat.st_mode & !0o7777) | 0o600;
            assert_eq!(private(&stat, 0o600), Err(StoreError::Denied));
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{
    CatalogStore, read_catalog_file, read_model_file, read_private_optional_file, read_trust_file,
};

#[cfg(not(target_os = "macos"))]
pub struct CatalogStore;
#[cfg(not(target_os = "macos"))]
impl CatalogStore {
    pub fn open(_: &Path) -> Result<Self, StoreError> {
        Err(StoreError::UnsupportedPlatform)
    }
    pub fn read_active(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Err(StoreError::UnsupportedPlatform)
    }
    pub fn commit(&mut self, _: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::UnsupportedPlatform)
    }
    pub fn read_floor(&self) -> Result<Option<Vec<u8>>, StoreError> {
        Err(StoreError::UnsupportedPlatform)
    }
    pub fn reserve_floor(&mut self, _: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::UnsupportedPlatform)
    }
}
#[cfg(not(target_os = "macos"))]
pub fn read_catalog_file(_: &Path, _: usize) -> Result<Vec<u8>, StoreError> {
    Err(StoreError::UnsupportedPlatform)
}
#[cfg(not(target_os = "macos"))]
pub fn read_model_file(_: &Path, _: usize) -> Result<Vec<u8>, StoreError> {
    Err(StoreError::UnsupportedPlatform)
}
#[cfg(not(target_os = "macos"))]
pub fn read_trust_file(_: &Path, _: usize) -> Result<Vec<u8>, StoreError> {
    Err(StoreError::UnsupportedPlatform)
}
#[cfg(not(target_os = "macos"))]
pub fn read_private_optional_file(_: &Path, _: usize) -> Result<Option<Vec<u8>>, StoreError> {
    Err(StoreError::UnsupportedPlatform)
}
