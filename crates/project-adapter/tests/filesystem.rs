//! Real filesystem boundary tests. Fixtures are created and mutated only by this
//! test harness; the production adapter neither writes nor launches Cargo.
use rust_engineering_application::{
    OperationControl, ProjectBackend, ProjectError, ReferenceGenerator,
};
use rust_engineering_domain::OperationalErrorCode;
use rust_engineering_project::{OsReferences, SecureProjects};

struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}

#[test]
fn os_references_are_distinct_opaque_128_bit_values() -> Result<(), ProjectError> {
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..256 {
        let reference = OsReferences.generate()?;
        let encoded = reference.to_string();
        assert_eq!(encoded.len(), 36);
        assert!(encoded.starts_with("prj_"));
        assert!(
            encoded[4..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        assert!(seen.insert(encoded));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[test]
fn unsupported_platform_rejects_paths_without_filesystem_access() -> Result<(), ProjectError> {
    // Deliberately no fixture I/O: rejection is a platform gate, not a claim
    // that Windows junctions or Linux symlinks have been contained.
    let backend = SecureProjects::new(&[std::path::PathBuf::from("/nonexistent/authority")])?;
    for path in [
        "/",
        "/nonexistent/authority",
        "../escape",
        r"C:\project",
        r"\\server\share\project",
        r"\\?\C:\junction\project",
    ] {
        assert!(matches!(
            backend.open(path, &Continue),
            Err(ProjectError::Rejected(
                OperationalErrorCode::UnsupportedPlatform
            ))
        ));
    }
    assert_eq!(
        backend.revalidate(&rust_engineering_project::ProjectLease, &Continue),
        Err(ProjectError::Rejected(
            OperationalErrorCode::UnsupportedPlatform
        ))
    );
    Ok(())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    type TestResult<T = ()> = Result<T, String>;
    fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> TestResult<T> {
        result.map_err(|error| format!("{error:?}"))
    }
    fn text(path: &Path) -> TestResult<&str> {
        path.to_str()
            .ok_or_else(|| "fixture path is not UTF-8".to_owned())
    }
    const PACKAGE: &str = "[package]\nname='fixture'\nversion='0.1.0'\nedition='2024'\n";

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> TestResult<Self> {
            // Canonicalization is confined to harness-owned paths, to avoid the
            // macOS /var alias. Production must reject that alias if supplied.
            let temp = checked(std::env::temp_dir().canonicalize())?;
            let name = checked(OsReferences.generate())?;
            let path = temp.join(format!("rust-mcp-filesystem-{name}"));
            checked(fs::create_dir(&path))?;
            Ok(Self(path))
        }
        fn package(&self, relative: &str, manifest: &str) -> TestResult<PathBuf> {
            let path = self.0.join(relative);
            checked(fs::create_dir_all(path.join("src")))?;
            checked(fs::write(path.join("Cargo.toml"), manifest))?;
            checked(fs::write(path.join("src/lib.rs"), "pub fn fixture() {}\n"))?;
            Ok(path)
        }
        fn backend(&self) -> TestResult<SecureProjects> {
            checked(SecureProjects::new(std::slice::from_ref(&self.0)))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn rejected<T>(result: Result<T, ProjectError>, code: OperationalErrorCode) {
        assert!(matches!(result, Err(ProjectError::Rejected(actual)) if actual == code));
    }

    #[test]
    fn package_fingerprint_is_deterministic_and_tracks_manifests_not_source_bytes() -> TestResult {
        let f = Fixture::new()?;
        let project = f.package("project", PACKAGE)?;
        let backend = f.backend()?;
        let first = checked(backend.open(text(&project)?, &Continue))?;
        assert_eq!(
            first.identity,
            checked(backend.open(text(&project)?, &Continue))?.identity
        );
        assert_eq!(first.identity.workspace_root, text(&project)?);
        checked(fs::write(
            project.join("src/lib.rs"),
            "pub fn changed() {}\n",
        ))?;
        assert_eq!(
            first.identity,
            checked(backend.revalidate(&first.lease, &Continue))?
        );
        checked(fs::write(
            project.join("Cargo.toml"),
            format!("{PACKAGE}description='changed'\n"),
        ))?;
        assert_ne!(
            first.identity.fingerprint,
            checked(backend.revalidate(&first.lease, &Continue))?.fingerprint
        );
        Ok(())
    }

    #[test]
    fn virtual_workspace_inheritance_and_external_dependency_need_explicit_authority() -> TestResult
    {
        let f = Fixture::new()?;
        let workspace = f.0.join("workspace");
        f.package("workspace/member", "[package]\nname='member'\nversion.workspace=true\nedition.workspace=true\n[dependencies]\nexternal.workspace=true\n")?;
        let external = f.package("external", "[package]\nname='external'\nversion='1.0.0'\n")?;
        checked(fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers=['member']\nresolver='3'\n[workspace.package]\nversion='1.0.0'\nedition='2024'\n[workspace.dependencies]\nexternal={path='../external'}\n",
        ))?;
        let restricted = checked(SecureProjects::new(std::slice::from_ref(&workspace)))?;
        rejected(
            restricted.open(text(&workspace)?, &Continue),
            OperationalErrorCode::SandboxDenied,
        );
        let backend = checked(SecureProjects::new(&[workspace.clone(), external.clone()]))?;
        let first = checked(backend.open(text(&workspace)?, &Continue))?;
        checked(fs::write(
            external.join("Cargo.toml"),
            "[package]\nname='external'\nversion='1.0.1'\n",
        ))?;
        assert_ne!(
            first.identity.fingerprint,
            checked(backend.revalidate(&first.lease, &Continue))?.fingerprint
        );
        Ok(())
    }

    #[test]
    fn empty_roots_and_sibling_prefix_do_not_authorize_projects() -> TestResult {
        let f = Fixture::new()?;
        let root = f.package("allowed", PACKAGE)?;
        let sibling = f.package("allowed-other", PACKAGE)?;
        rejected(
            checked(SecureProjects::new(&[]))?.open(text(&root)?, &Continue),
            OperationalErrorCode::SandboxDenied,
        );
        let backend = checked(SecureProjects::new(&[root]))?;
        rejected(
            backend.open(text(&sibling)?, &Continue),
            OperationalErrorCode::SandboxDenied,
        );
        Ok(())
    }

    #[test]
    fn ambiguous_and_relative_requested_paths_are_rejected() -> TestResult {
        let f = Fixture::new()?;
        let project = f.package("project", PACKAGE)?;
        let backend = f.backend()?;
        for path in [
            "project".to_owned(),
            format!("{}/../project", project.display()),
            format!("{}/./project", f.0.display()),
            format!("{}//project", f.0.display()),
            format!("{}\n", project.display()),
        ] {
            rejected(
                backend.open(&path, &Continue),
                OperationalErrorCode::InvalidProject,
            );
        }
        Ok(())
    }

    #[test]
    fn symlink_authority_and_intermediate_components_are_denied() -> TestResult {
        let f = Fixture::new()?;
        f.package("real/project", PACKAGE)?;
        let link = f.0.join("alias");
        checked(symlink(f.0.join("real"), &link))?;
        assert!(SecureProjects::new(std::slice::from_ref(&link)).is_err());
        assert!(SecureProjects::new(&[link.join("project")]).is_err());
        rejected(
            f.backend()?.open(text(&link.join("project"))?, &Continue),
            OperationalErrorCode::SandboxDenied,
        );
        Ok(())
    }

    #[test]
    fn project_manifest_and_source_symlinks_are_denied_even_inside_authority() -> TestResult {
        for component in ["project", "Cargo.toml", "src/lib.rs"] {
            let f = Fixture::new()?;
            let project = f.package("project", PACKAGE)?;
            let original = if component == "project" {
                project.clone()
            } else {
                project.join(component)
            };
            let saved = f.0.join("saved");
            checked(fs::rename(&original, &saved))?;
            checked(symlink(&saved, &original))?;
            rejected(
                f.backend()?.open(text(&project)?, &Continue),
                OperationalErrorCode::SandboxDenied,
            );
        }
        Ok(())
    }

    #[test]
    fn hardlinked_manifest_and_source_are_denied() -> TestResult {
        for component in ["Cargo.toml", "src/lib.rs"] {
            let f = Fixture::new()?;
            let project = f.package("project", PACKAGE)?;
            checked(fs::hard_link(
                project.join(component),
                f.0.join("second-name"),
            ))?;
            assert!(f.backend()?.open(text(&project)?, &Continue).is_err());
        }
        Ok(())
    }

    #[test]
    fn fifo_and_directory_manifests_are_rejected_without_blocking() -> TestResult {
        for fifo in [true, false] {
            let f = Fixture::new()?;
            let project = f.package("project", PACKAGE)?;
            let manifest = project.join("Cargo.toml");
            checked(fs::remove_file(&manifest))?;
            if fifo {
                // rustix excludes mkfifoat on Apple. The trusted system utility
                // creates only this harness-owned FIFO; no project code runs.
                let status = checked(
                    std::process::Command::new("/usr/bin/mkfifo")
                        .env_clear()
                        .arg(&manifest)
                        .status(),
                )?;
                assert!(status.success());
            } else {
                checked(fs::create_dir(&manifest))?;
            }
            let start = Instant::now();
            assert!(f.backend()?.open(text(&project)?, &Continue).is_err());
            assert!(start.elapsed() < Duration::from_secs(5));
        }
        Ok(())
    }

    #[test]
    fn oversized_or_invalid_manifest_is_rejected() -> TestResult {
        let f = Fixture::new()?;
        let project = f.package("project", PACKAGE)?;
        let backend = f.backend()?;
        let mut at_limit = PACKAGE.as_bytes().to_vec();
        at_limit.resize(256 * 1024, b'#');
        checked(fs::write(project.join("Cargo.toml"), &at_limit))?;
        checked(backend.open(text(&project)?, &Continue))?;
        checked(fs::write(
            project.join("Cargo.toml"),
            vec![b'#'; 256 * 1024 + 1],
        ))?;
        rejected(
            backend.open(text(&project)?, &Continue),
            OperationalErrorCode::OutputLimitExceeded,
        );
        checked(fs::write(
            project.join("Cargo.toml"),
            "[package]\nname='bad'\nversion='invalid'\n",
        ))?;
        rejected(
            backend.open(text(&project)?, &Continue),
            OperationalErrorCode::InvalidProject,
        );
        Ok(())
    }

    #[test]
    fn replacing_project_or_root_invalidates_old_lease() -> TestResult {
        for replace_root in [false, true] {
            let f = Fixture::new()?;
            let project = f.package("authority/project", PACKAGE)?;
            let root = f.0.join("authority");
            let backend = checked(SecureProjects::new(std::slice::from_ref(&root)))?;
            let opened = checked(backend.open(text(&project)?, &Continue))?;
            let replaced = if replace_root { &root } else { &project };
            checked(fs::rename(replaced, f.0.join("old")))?;
            f.package("authority/project", PACKAGE)?;
            assert!(backend.revalidate(&opened.lease, &Continue).is_err());
            if replace_root {
                assert!(backend.open(text(&project)?, &Continue).is_err());
            } else {
                assert_ne!(
                    opened.identity,
                    checked(backend.open(text(&project)?, &Continue))?.identity
                );
            }
        }
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
    fn manifest_mutation_after_read_is_detected_before_identity_is_issued() -> TestResult {
        let f = Fixture::new()?;
        let project = f.package("project", PACKAGE)?;
        let control = AtCheckpoint {
            calls: AtomicUsize::new(0),
            at: 4,
            action: || {
                fs::write(
                    project.join("Cargo.toml"),
                    format!("{PACKAGE}description='tampered'\n"),
                )
                .map_err(|_| ProjectError::Internal)
            },
        };
        rejected(
            f.backend()?.open(text(&project)?, &control),
            OperationalErrorCode::InvalidProject,
        );
        assert!(control.calls.load(Ordering::SeqCst) >= 4);
        Ok(())
    }

    #[test]
    fn repeated_observation_cannot_hide_a_changed_manifest() -> TestResult {
        let f = Fixture::new()?;
        let source = format!("{PACKAGE}[lib]\npath='Cargo.toml'\n");
        let project = f.package("project", &source)?;
        let control = AtCheckpoint {
            calls: AtomicUsize::new(0),
            // After manifest bytes and metadata were recorded, before target checks.
            at: 5,
            action: || {
                fs::write(project.join("Cargo.toml"), format!("{source}# changed\n"))
                    .map_err(|_| ProjectError::Internal)
            },
        };
        rejected(
            f.backend()?.open(text(&project)?, &control),
            OperationalErrorCode::InvalidProject,
        );
        Ok(())
    }

    #[test]
    fn project_replacement_during_manifest_read_is_detected() -> TestResult {
        let f = Fixture::new()?;
        let project = f.package("project", PACKAGE)?;
        let replacement = f.package("replacement", PACKAGE)?;
        let control = AtCheckpoint {
            calls: AtomicUsize::new(0),
            at: 4,
            action: || {
                fs::rename(&project, f.0.join("old")).map_err(|_| ProjectError::Internal)?;
                fs::rename(&replacement, &project).map_err(|_| ProjectError::Internal)
            },
        };
        rejected(
            f.backend()?.open(text(&project)?, &control),
            OperationalErrorCode::InvalidProject,
        );
        assert!(control.calls.load(Ordering::SeqCst) >= 4);
        Ok(())
    }

    #[test]
    fn cancellation_and_deadline_stop_at_filesystem_checkpoints() -> TestResult {
        let f = Fixture::new()?;
        let project = f.package("project", PACKAGE)?;
        let backend = f.backend()?;
        let opened = checked(backend.open(text(&project)?, &Continue))?;
        for error in [
            ProjectError::Cancelled,
            ProjectError::Rejected(OperationalErrorCode::CommandTimeout),
        ] {
            let control = AtCheckpoint {
                calls: AtomicUsize::new(0),
                at: 1,
                action: || Err(error),
            };
            assert!(
                matches!(backend.open(text(&project)?, &control), Err(actual) if actual == error)
            );
            let control = AtCheckpoint {
                calls: AtomicUsize::new(0),
                at: 1,
                action: || Err(error),
            };
            assert_eq!(backend.revalidate(&opened.lease, &control), Err(error));
            let control = AtCheckpoint {
                calls: AtomicUsize::new(0),
                at: 4,
                action: || Err(error),
            };
            assert!(
                matches!(backend.open(text(&project)?, &control), Err(actual) if actual == error)
            );
        }
        Ok(())
    }

    #[test]
    fn bounded_concurrent_project_symlink_swaps_never_return_outside_identity() -> TestResult {
        let f = Fixture::new()?;
        let project = f.package("authority/project", PACKAGE)?;
        let outside = f.package(
            "outside",
            "[package]\nname='outside_sentinel'\nversion='9.9.9'\n",
        )?;
        let backend = checked(SecureProjects::new(&[f.0.join("authority")]))?;
        let baseline = checked(backend.open(text(&project)?, &Continue))?.identity;
        let parked = f.0.join("authority/parked");
        let active = AtomicBool::new(true);
        let attempts = AtomicUsize::new(0);
        let result = std::thread::scope(|scope| -> TestResult {
            let worker = scope.spawn(|| -> TestResult {
                let deadline = Instant::now() + Duration::from_secs(5);
                while active.load(Ordering::Acquire) && Instant::now() < deadline {
                    checked(fs::rename(&project, &parked))?;
                    checked(symlink(&outside, &project))?;
                    attempts.fetch_add(1, Ordering::Relaxed);
                    std::thread::yield_now();
                    checked(fs::remove_file(&project))?;
                    checked(fs::rename(&parked, &project))?;
                }
                Ok(())
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            while attempts.load(Ordering::Relaxed) == 0 && Instant::now() < deadline {
                std::thread::yield_now();
            }
            for _ in 0..100 {
                if let Ok(opened) = backend.open(text(&project)?, &Continue) {
                    assert_eq!(opened.identity, baseline);
                }
            }
            active.store(false, Ordering::Release);
            checked(worker.join())??;
            Ok(())
        });
        result?;
        assert!(attempts.load(Ordering::Relaxed) > 0);
        assert_eq!(
            checked(backend.open(text(&project)?, &Continue))?.identity,
            baseline
        );
        Ok(())
    }
}
