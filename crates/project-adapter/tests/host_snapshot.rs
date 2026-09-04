//! Real host-file fixtures; no Cargo manifest or project execution is required.
use rust_engineering_application::{OperationControl, ProjectError};
use rust_engineering_domain::OperationalErrorCode;
use rust_engineering_project::read_host_snapshot;

struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
#[test]
fn unsupported_platform_fails_before_filesystem_access() {
    for path in ["/nonexistent/snapshot", "../snapshot", r"C:\snapshot"] {
        assert_eq!(
            read_host_snapshot(std::path::Path::new(path), &Continue),
            Err(ProjectError::Rejected(
                OperationalErrorCode::UnsupportedPlatform
            ))
        );
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use rust_engineering_application::ReferenceGenerator;
    use rust_engineering_project::{MAX_HOST_SNAPSHOT_BYTES, OsReferences};
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    type TestResult<T = ()> = Result<T, String>;
    fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> TestResult<T> {
        result.map_err(|error| format!("{error:?}"))
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> TestResult<Self> {
            // Only the harness resolves the macOS /var alias. Production rejects it.
            let temp = checked(std::env::temp_dir().canonicalize())?;
            let id = checked(OsReferences.generate())?;
            let root = temp.join(format!("rust-mcp-host-snapshot-{id}"));
            checked(fs::create_dir(&root))?;
            Ok(Self(root))
        }
        fn write(&self, name: &str, bytes: &[u8]) -> TestResult<PathBuf> {
            let path = self.0.join(name);
            checked(fs::write(&path, bytes))?;
            Ok(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn rejected(path: &Path, code: OperationalErrorCode) {
        assert_eq!(
            read_host_snapshot(path, &Continue),
            Err(ProjectError::Rejected(code))
        );
    }

    #[test]
    fn reads_exact_owned_bytes_without_cargo_manifest() -> TestResult {
        let f = Fixture::new()?;
        for bytes in [b"".as_slice(), b"snapshot\0\xff\n".as_slice()] {
            let path = f.write("snapshot", bytes)?;
            let owned = checked(read_host_snapshot(&path, &Continue))?;
            checked(fs::remove_file(&path))?;
            assert_eq!(owned, bytes);
        }
        Ok(())
    }

    #[test]
    fn absent_snapshot_is_reported_as_missing() -> TestResult {
        let f = Fixture::new()?;
        rejected(&f.0.join("absent"), OperationalErrorCode::ProjectNotFound);
        rejected(
            &f.0.join("absent-parent/snapshot"),
            OperationalErrorCode::ProjectNotFound,
        );
        Ok(())
    }

    #[test]
    fn relative_traversal_and_ambiguous_paths_are_rejected() -> TestResult {
        let f = Fixture::new()?;
        f.write("snapshot", b"data")?;
        for path in [
            "snapshot".to_owned(),
            format!("{}/../snapshot", f.0.display()),
            format!("{}/./snapshot", f.0.display()),
            format!("{}//snapshot", f.0.display()),
            format!("{}/snapshot\n", f.0.display()),
        ] {
            rejected(Path::new(&path), OperationalErrorCode::InvalidProject);
        }
        Ok(())
    }

    #[test]
    fn file_and_ancestor_symlinks_are_denied() -> TestResult {
        let f = Fixture::new()?;
        let path = f.write("snapshot", b"data")?;
        let alias = f.0.join("alias");
        checked(symlink(&path, &alias))?;
        rejected(&alias, OperationalErrorCode::SandboxDenied);
        let ancestor = f.0.join("ancestor");
        checked(symlink(&f.0, &ancestor))?;
        rejected(
            &ancestor.join("snapshot"),
            OperationalErrorCode::SandboxDenied,
        );
        rejected(
            &ancestor.join("ancestor/snapshot"),
            OperationalErrorCode::SandboxDenied,
        );
        Ok(())
    }

    #[test]
    fn hardlinks_fifo_and_directory_are_rejected_without_blocking() -> TestResult {
        let f = Fixture::new()?;
        let path = f.write("snapshot", b"data")?;
        checked(fs::hard_link(&path, f.0.join("second-name")))?;
        assert!(read_host_snapshot(&path, &Continue).is_err());
        let fifo = f.0.join("fifo");
        // rustix has no Apple mkfifoat; only the harness launches this fixed utility.
        let status = checked(
            std::process::Command::new("/usr/bin/mkfifo")
                .env_clear()
                .arg(&fifo)
                .status(),
        )?;
        assert!(status.success());
        let start = Instant::now();
        assert!(read_host_snapshot(&fifo, &Continue).is_err());
        assert!(read_host_snapshot(&f.0, &Continue).is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
        Ok(())
    }

    #[test]
    fn exact_limit_is_read_and_one_extra_byte_is_rejected() -> TestResult {
        assert_eq!(MAX_HOST_SNAPSHOT_BYTES, 8 * 1024 * 1024);
        let f = Fixture::new()?;
        let bytes = vec![b'x'; MAX_HOST_SNAPSHOT_BYTES];
        let path = f.write("snapshot", &bytes)?;
        assert_eq!(checked(read_host_snapshot(&path, &Continue))?, bytes);
        checked(fs::OpenOptions::new().write(true).open(&path))?
            .set_len((MAX_HOST_SNAPSHOT_BYTES + 1) as u64)
            .map_err(|error| error.to_string())?;
        rejected(&path, OperationalErrorCode::OutputLimitExceeded);
        Ok(())
    }

    struct AtCheckpoint<F> {
        calls: AtomicUsize,
        at: usize,
        action: F,
    }
    impl<F: Fn() -> Result<(), ProjectError> + Send + Sync> OperationControl for AtCheckpoint<F> {
        fn check(&self) -> Result<(), ProjectError> {
            if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.at {
                (self.action)()?;
            }
            Ok(())
        }
    }

    #[test]
    fn cancellation_and_deadline_stop_before_io_and_between_chunks() -> TestResult {
        let f = Fixture::new()?;
        let path = f.write("snapshot", &vec![b'x'; 256 * 1024])?;
        for at in [1, 3] {
            for error in [
                ProjectError::Cancelled,
                ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
            ] {
                let control = AtCheckpoint {
                    calls: AtomicUsize::new(0),
                    at,
                    action: || Err(error),
                };
                assert_eq!(read_host_snapshot(&path, &control), Err(error));
                assert_eq!(control.calls.load(Ordering::SeqCst), at);
            }
        }
        let cancelled = AtCheckpoint {
            calls: AtomicUsize::new(0),
            at: 1,
            action: || Err(ProjectError::Cancelled),
        };
        assert_eq!(
            read_host_snapshot(&f.0.join("absent"), &cancelled),
            Err(ProjectError::Cancelled)
        );
        Ok(())
    }

    #[test]
    fn growth_during_read_is_bounded() -> TestResult {
        let f = Fixture::new()?;
        let path = f.write("snapshot", &vec![b'x'; 128 * 1024])?;
        let control = AtCheckpoint {
            calls: AtomicUsize::new(0),
            at: 3, // One 64 KiB chunk has been read through the held file.
            action: || {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .and_then(|file| file.set_len((MAX_HOST_SNAPSHOT_BYTES + 1) as u64))
                    .map_err(|_| ProjectError::Internal)
            },
        };
        assert_eq!(
            read_host_snapshot(&path, &control),
            Err(ProjectError::Rejected(
                OperationalErrorCode::OutputLimitExceeded
            ))
        );
        assert!(control.calls.load(Ordering::SeqCst) >= 3);
        Ok(())
    }

    #[test]
    fn mutations_and_replacements_during_read_fail_closed() -> TestResult {
        for mutation in ["overwrite", "replace", "symlink", "hardlink", "root"] {
            let f = Fixture::new()?;
            let authority = f.0.join("authority");
            checked(fs::create_dir(&authority))?;
            let path = authority.join("snapshot");
            checked(fs::write(&path, vec![b'x'; 128 * 1024]))?;
            let control = AtCheckpoint {
                calls: AtomicUsize::new(0),
                at: 3,
                action: || {
                    let action = || -> std::io::Result<()> {
                        match mutation {
                            "overwrite" => fs::write(&path, vec![b'y'; 128 * 1024]),
                            "replace" => {
                                fs::rename(&path, f.0.join("old-file"))?;
                                fs::write(&path, vec![b'x'; 128 * 1024])
                            }
                            "symlink" => {
                                let old = f.0.join("old-file");
                                fs::rename(&path, &old)?;
                                symlink(&old, &path)
                            }
                            "hardlink" => fs::hard_link(&path, f.0.join("alias")),
                            _ => {
                                fs::rename(&authority, f.0.join("old-root"))?;
                                fs::create_dir(&authority)?;
                                fs::write(&path, vec![b'x'; 128 * 1024])
                            }
                        }
                    };
                    action().map_err(|_| ProjectError::Internal)
                },
            };
            assert!(
                matches!(
                    read_host_snapshot(&path, &control),
                    Err(ProjectError::Rejected(
                        OperationalErrorCode::InvalidProject | OperationalErrorCode::SandboxDenied
                    ))
                ),
                "mutation {mutation} must be rejected"
            );
            assert!(control.calls.load(Ordering::SeqCst) >= 3);
        }
        Ok(())
    }
}
