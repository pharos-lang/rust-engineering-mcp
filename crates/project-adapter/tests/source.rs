//! Real APFS source-transfer boundary; never compile or execute fixture code.
#![cfg(target_os = "macos")]
use rust_engineering_application::{
    OperationControl, ProjectBackend, ProjectError, ProjectRegistry, ProjectSourceBackend,
    ReferenceGenerator,
};
use rust_engineering_domain::{OperationalErrorCode, SOURCE_MAX_FILE_BYTES};
use rust_engineering_project::{MonotonicClock, OsReferences, SecureProjects};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
type TestResult<T = ()> = Result<T, String>;
fn ck<T, E: std::fmt::Debug>(r: Result<T, E>) -> TestResult<T> {
    r.map_err(|e| format!("{e:?}"))
}
struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
struct Fixture {
    base: PathBuf,
    project: PathBuf,
}
impl Fixture {
    fn new() -> TestResult<Self> {
        let base =
            PathBuf::from("/private/tmp").join(format!("rms-{}", ck(OsReferences.generate())?));
        // Exclusive creation rejects a preexisting attacker-controlled base.
        ck(fs::create_dir(&base))?;
        let project = base.join("project");
        ck(fs::create_dir_all(project.join("src")))?;
        ck(fs::write(
            project.join("Cargo.toml"),
            "[package]\nname='source'\nversion='0.1.0'\nedition='2024'\n",
        ))?;
        ck(fs::write(project.join("src/lib.rs"), b"pub fn f() {}\n"))?;
        Ok(Self { base, project })
    }
    fn backend(&self) -> TestResult<SecureProjects> {
        ck(SecureProjects::new(std::slice::from_ref(&self.base)))
    }
    fn path(&self) -> TestResult<&str> {
        self.project.to_str().ok_or("path".into())
    }
    fn capture(&self) -> TestResult<Result<rust_engineering_domain::SourceBundle, ProjectError>> {
        let backend = self.backend()?;
        let opened = ck(backend.open(self.path()?, &Continue))?;
        Ok(backend.source(&opened.lease, &Continue))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
#[test]
fn registry_captures_exact_sorted_bytes_without_mutation_and_excludes_build_git_dirs() -> TestResult
{
    let f = Fixture::new()?;
    for dir in ["target", ".git", "nested/target", "nested/.git"] {
        ck(fs::create_dir_all(f.project.join(dir)))?;
        ck(fs::write(f.project.join(dir).join("ignored"), b"ignored"))?;
    }
    ck(fs::write(f.project.join("binary"), [0, 255, 1]))?;
    ck(fs::write(f.project.join("Cargo.lock"), "# exact lock\n"))?;
    let mut registry = ck(ProjectRegistry::new(
        f.backend()?,
        OsReferences,
        MonotonicClock::default(),
        30,
        2,
    ))?;
    let opened = ck(registry.open(f.path()?, &Continue))?;
    let bundle = ck(registry.source(&opened.project_ref, &Continue))?;
    assert_eq!(
        bundle.files().iter().map(|v| v.path()).collect::<Vec<_>>(),
        ["Cargo.lock", "Cargo.toml", "binary", "src/lib.rs"]
    );
    for file in bundle.files() {
        assert_eq!(ck(fs::read(f.project.join(file.path())))?, file.bytes());
    }
    assert_eq!(
        opened.identity,
        ck(registry.resolve(&opened.project_ref, &Continue))?
    );
    Ok(())
}
#[test]
fn rejects_source_links_hardlinks_fifos_and_socket_without_reading_content() -> TestResult {
    for kind in [
        "symlink",
        "hardlink",
        "fifo",
        "socket",
        "directory-link",
        "excluded-link",
    ] {
        let f = Fixture::new()?;
        let path = f.project.join(if kind == "excluded-link" {
            "target"
        } else {
            "attack"
        });
        let _socket = match kind {
            "symlink" | "directory-link" | "excluded-link" => {
                ck(std::os::unix::fs::symlink(&f.base, &path))?;
                None
            }
            "hardlink" => {
                ck(fs::hard_link(f.project.join("src/lib.rs"), path))?;
                None
            }
            "fifo" => {
                assert!(
                    ck(std::process::Command::new("/usr/bin/mkfifo")
                        .env_clear()
                        .arg(path)
                        .status())?
                    .success()
                );
                None
            }
            _ => Some(ck(std::os::unix::net::UnixListener::bind(path))?),
        };
        let backend = f.backend()?;
        // Use an unrelated file for hardlink so open's required target remains valid.
        if kind == "hardlink" {
            ck(fs::remove_file(f.project.join("attack")))?;
            ck(fs::write(f.project.join("attack"), "x"))?;
            ck(fs::hard_link(
                f.project.join("attack"),
                f.base.join("alias"),
            ))?;
        }
        let lease = ck(backend.open(f.path()?, &Continue))?;
        assert!(backend.source(&lease.lease, &Continue).is_err(), "{kind}");
    }
    Ok(())
}
#[test]
fn rejects_project_cargo_configuration_and_non_bare_toolchains() -> TestResult {
    for (path, bytes) in [
        (".cargo/config", ""),
        ("nested/.cargo/config.toml", "[build]\nrustc-wrapper='bad'"),
        ("rust-toolchain", "stable"),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel='1.98.1'\ncomponents=[]",
        ),
        (
            "rust-toolchain.toml",
            "[toolchain]\nchannel='1.98.1'\ntargets=[]",
        ),
        ("rust-toolchain.toml", "[toolchain]\npath='/outside'"),
    ] {
        let f = Fixture::new()?;
        let target = f.project.join(path);
        ck(fs::create_dir_all(target.parent().ok_or("parent")?))?;
        ck(fs::write(target, bytes))?;
        assert!(f.capture()?.is_err(), "{path}: {bytes}");
    }
    for (name, value) in [
        ("rust-toolchain", "1.98.1\n"),
        ("rust-toolchain.toml", "[toolchain]\nchannel='1.98.1'\n"),
    ] {
        let f = Fixture::new()?;
        ck(fs::write(f.project.join(name), value))?;
        ck(f.capture()?)?;
    }
    Ok(())
}
#[test]
fn rejects_external_and_absolute_dependencies_even_when_host_authorized() -> TestResult {
    for absolute in [false, true] {
        let f = Fixture::new()?;
        ck(fs::create_dir_all(f.base.join("outside/src")))?;
        ck(fs::write(
            f.base.join("outside/Cargo.toml"),
            "[package]\nname='outside'\n",
        ))?;
        ck(fs::write(f.base.join("outside/src/lib.rs"), ""))?;
        let dep = if absolute {
            f.base.join("outside").display().to_string()
        } else {
            "../outside".into()
        };
        ck(fs::write(
            f.project.join("Cargo.toml"),
            format!(
                "[package]\nname='source'\n[target.'cfg(windows)'.dev-dependencies]\noutside={{path='{dep}'}}\n"
            ),
        ))?;
        assert!(f.capture()?.is_err());
    }
    Ok(())
}
#[test]
fn accepts_internal_parent_dependency_without_rewriting() -> TestResult {
    let f = Fixture::new()?;
    ck(fs::write(
        f.project.join("Cargo.toml"),
        "[workspace]\nmembers=['a','b']\n",
    ))?;
    for name in ["a", "b"] {
        ck(fs::create_dir_all(f.project.join(name).join("src")))?;
        ck(fs::write(f.project.join(name).join("src/lib.rs"), ""))?;
        ck(fs::write(
            f.project.join(name).join("Cargo.toml"),
            format!(
                "[package]\nname='{name}'\n{}",
                if name == "a" {
                    "[dependencies]\nb={path='../b'}\n"
                } else {
                    ""
                }
            ),
        ))?;
    }
    ck(f.capture()?)?;
    Ok(())
}
#[test]
fn source_limits_cover_bytes_names_depth_and_directory_count() -> TestResult {
    for scenario in ["file", "total", "name", "unicode", "depth", "entries"] {
        let f = Fixture::new()?;
        match scenario {
            "file" => ck(fs::write(
                f.project.join("large"),
                vec![0; SOURCE_MAX_FILE_BYTES + 1],
            ))?,
            "total" => {
                for n in 0..17 {
                    ck(fs::write(
                        f.project.join(format!("large{n}")),
                        vec![0; SOURCE_MAX_FILE_BYTES],
                    ))?;
                }
            }
            "name" => ck(fs::write(f.project.join("a".repeat(101)), ""))?,
            "unicode" => ck(fs::write(f.project.join("é"), ""))?,
            "depth" => ck(fs::create_dir_all(f.project.join(vec!["a"; 33].join("/"))))?,
            _ => {
                for n in 0..4097 {
                    ck(fs::create_dir(f.project.join(format!("d{n}"))))?;
                }
            }
        }
        assert!(f.capture()?.is_err(), "{scenario}");
    }
    Ok(())
}
struct At<F> {
    calls: AtomicUsize,
    at: usize,
    action: F,
}
impl<F: Fn() -> Result<(), ProjectError> + Send + Sync> OperationControl for At<F> {
    fn check(&self) -> Result<(), ProjectError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) + 1 == self.at {
            (self.action)()?;
        }
        Ok(())
    }
}
#[test]
fn cancellation_stops_capture_without_mutation() -> TestResult {
    let f = Fixture::new()?;
    let backend = f.backend()?;
    let opened = ck(backend.open(f.path()?, &Continue))?;
    let control = At {
        calls: AtomicUsize::new(0),
        at: 20,
        action: || Err(ProjectError::Cancelled),
    };
    assert_eq!(
        backend.source(&opened.lease, &control),
        Err(ProjectError::Cancelled)
    );
    assert_eq!(
        ck(fs::read(f.project.join("src/lib.rs")))?,
        b"pub fn f() {}\n"
    );
    ck(backend.source(&opened.lease, &Continue))?;
    Ok(())
}
#[test]
fn observed_file_and_directory_mutations_are_rejected() -> TestResult {
    // Sweep all checkpoints and require detected races plus safe captured bytes.
    // Changes before capture or after the final observation are not atomicity claims.
    for directory_swap in [false, true] {
        let baseline = Fixture::new()?;
        let backend = baseline.backend()?;
        let opened = ck(backend.open(baseline.path()?, &Continue))?;
        let count = At {
            calls: AtomicUsize::new(0),
            at: usize::MAX,
            action: || Ok(()),
        };
        ck(backend.source(&opened.lease, &count))?;
        let checkpoints = count.calls.load(Ordering::SeqCst);
        let mut rejected_count = 0;
        for at in 1..=checkpoints {
            let f = Fixture::new()?;
            let backend = f.backend()?;
            let opened = ck(backend.open(f.path()?, &Continue))?;
            let control = At {
                calls: AtomicUsize::new(0),
                at,
                action: || {
                    if directory_swap {
                        fs::rename(f.project.join("src"), f.base.join("moved"))
                            .map_err(|_| ProjectError::Internal)?;
                        fs::write(f.base.join("moved/lib.rs"), "outside sentinel")
                            .map_err(|_| ProjectError::Internal)?;
                        std::os::unix::fs::symlink(f.base.join("moved"), f.project.join("src"))
                            .map_err(|_| ProjectError::Internal)
                    } else {
                        fs::write(f.project.join("src/lib.rs"), "changed source bytes")
                            .map_err(|_| ProjectError::Internal)
                    }
                },
            };
            match backend.source(&opened.lease, &control) {
                Err(_) => rejected_count += 1,
                Ok(bundle) => {
                    // A final-checkpoint rename may follow the last observation;
                    // accepted bytes must still be from the selected subtree.
                    assert!(bundle.files().iter().any(|file| file.path() == "src/lib.rs"
                        && (file.bytes() == b"changed source bytes"
                            || file.bytes() == b"pub fn f() {}\n")));
                }
            }
        }
        assert!(rejected_count > 0);
    }
    Ok(())
}
#[test]
fn replaced_registered_directory_is_rejected() -> TestResult {
    let f = Fixture::new()?;
    let mut registry = ck(ProjectRegistry::new(
        f.backend()?,
        OsReferences,
        MonotonicClock::default(),
        30,
        1,
    ))?;
    let opened = ck(registry.open(f.path()?, &Continue))?;
    ck(fs::rename(&f.project, f.base.join("old")))?;
    ck(fs::create_dir(&f.project))?;
    assert!(registry.source(&opened.project_ref, &Continue).is_err());
    assert_eq!(
        registry.source(&opened.project_ref, &Continue),
        Err(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound
        ))
    );
    Ok(())
}

#[test]
fn captured_bundle_preserves_empty_directories_but_not_excluded_directories() -> TestResult {
    let f = Fixture::new()?;
    for directory in ["assets/empty", "other-empty", "target/empty", ".git/empty"] {
        ck(fs::create_dir_all(f.project.join(directory)))?;
    }
    let bundle = ck(f.capture()?)?;
    assert_eq!(
        bundle.directories(),
        &["assets", "assets/empty", "other-empty", "src"]
    );
    Ok(())
}

#[test]
fn absolute_target_and_build_paths_accepted_by_m0_are_rejected_for_transfer() -> TestResult {
    for kind in ["lib", "bin", "build", "readme", "license-file"] {
        let f = Fixture::new()?;
        let path = f.project.join("src/lib.rs").display().to_string();
        let tail = match kind {
            "lib" => format!("[lib]\npath='{path}'\n"),
            "bin" => format!("[[bin]]\nname='bin'\npath='{path}'\n"),
            _ => format!("{kind}='{path}'\n"),
        };
        ck(fs::write(
            f.project.join("Cargo.toml"),
            format!("[package]\nname='source'\n{tail}"),
        ))?;
        // M0 open is intentionally broader. A capture rejection is the discriminator.
        assert!(f.capture()?.is_err(), "{kind}");
    }
    Ok(())
}

#[test]
fn unvisited_manifests_cannot_hide_escaping_paths_aliases_or_replacements() -> TestResult {
    let cases = [
        "[package]\nname='nested'\nreadme='../../outside'\n",
        "[package]\nname='nested'\nlicense-file='/outside'\n",
        "[package]\nname='nested'\nbuild='../../outside'\n",
        "[package]\nname='nested'\nworkspace='/outside'\n",
        "[package]\nname='nested'\ninclude=['../../outside']\n",
        "[lib]\npath='../../outside'\n",
        "[[bin]]\npath='/outside'\n",
        "[[test]]\npath='../../outside'\n",
        "[[example]]\npath='/outside'\n",
        "[[bench]]\npath='../../outside'\n",
        "[workspace.package]\nreadme='/outside'\n",
        "[workspace.package]\nlicense-file='../../outside'\n",
        "[workspace]\nmembers=['../../outside']\n",
        "[workspace]\nexclude=['/outside']\n",
        "[workspace]\ndefault-members=['../../outside']\n",
        "[dev_dependencies]\nx={path='../../outside'}\n",
        "[build_dependencies]\nx={path='/outside'}\n",
        "[target.'cfg(windows)'.dev_dependencies]\nx={path='../../outside'}\n",
        "[workspace.build_dependencies]\nx={path='/outside'}\n",
        "[replace]\n'x:1.0.0'={path='../../outside'}\n",
        "[patch.crates-io]\nx={path='/outside'}\n",
        "[dependencies]\nx={path='../../outside'}\n",
    ];
    for manifest in cases {
        let f = Fixture::new()?;
        ck(fs::create_dir(f.project.join("unused")))?;
        ck(fs::write(f.project.join("unused/Cargo.toml"), manifest))?;
        assert!(f.capture()?.is_err(), "{manifest}");
    }
    Ok(())
}

#[test]
fn internal_relative_package_paths_and_workspace_inheritance_are_preserved() -> TestResult {
    let f = Fixture::new()?;
    ck(fs::write(
        f.project.join("Cargo.toml"),
        "[workspace]\nmembers=['member']\n[workspace.package]\nreadme='README.md'\nlicense-file='LICENSE'\n",
    ))?;
    ck(fs::write(f.project.join("README.md"), "readme"))?;
    ck(fs::write(f.project.join("LICENSE"), "license"))?;
    ck(fs::create_dir_all(f.project.join("member/src")))?;
    let manifest = "[package]\nname='member'\nworkspace='..'\nreadme.workspace=true\nlicense-file.workspace=true\nbuild=false\n[lib]\npath='src/lib.rs'\n";
    ck(fs::write(f.project.join("member/Cargo.toml"), manifest))?;
    ck(fs::write(f.project.join("member/src/lib.rs"), ""))?;
    let bundle = ck(f.capture()?)?;
    assert!(
        bundle
            .files()
            .iter()
            .any(|file| file.path() == "member/Cargo.toml" && file.bytes() == manifest.as_bytes())
    );
    Ok(())
}

#[test]
fn stale_regular_entry_swapped_to_fifo_before_open_is_rejected_without_blocking() -> TestResult {
    let f = Fixture::new()?;
    let attack = f.project.join("000_attack");
    ck(fs::write(&attack, "original"))?;
    let fifo = f.base.join("prepared-fifo");
    // rustix1.1.4 gates mkfifoat/mknodat out on Apple. The integration
    // harness may create fixtures with this trusted utility; src never spawns it.
    assert!(
        ck(std::process::Command::new("/usr/bin/mkfifo")
            .env_clear()
            .arg(&fifo)
            .status())?
        .success()
    );
    let backend = f.backend()?;
    let opened = ck(backend.open(f.path()?, &Continue))?;
    let count = At {
        calls: AtomicUsize::new(0),
        at: usize::MAX,
        action: || Ok(()),
    };
    ck(backend.revalidate(&opened.lease, &count))?;
    let before_capture = count.calls.load(Ordering::SeqCst);
    // Count readdir entries (including dot entries) in this harness-owned tree.
    let fd = ck(rustix::fs::openat(
        rustix::fs::CWD,
        &f.project,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY,
        rustix::fs::Mode::empty(),
    ))?;
    let entries = ck(ck(rustix::fs::Dir::read_from(&fd))?.collect::<Result<Vec<_>, _>>())?.len();
    // source() first revalidates; directory() then checks once before opening,
    // once per entry, and once before consuming sorted names. 000_attack sorts
    // first: this checkpoint is after its regular d_type was captured and before
    // file() opens it. Renaming a prebuilt FIFO avoids a process inside the callback.
    let control = At {
        calls: AtomicUsize::new(0),
        at: before_capture + entries + 2,
        action: || {
            fs::rename(&attack, f.base.join("saved-regular"))
                .map_err(|_| ProjectError::Internal)?;
            fs::rename(&fifo, &attack).map_err(|_| ProjectError::Internal)
        },
    };
    let start = std::time::Instant::now();
    assert_eq!(
        backend.source(&opened.lease, &control),
        Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied))
    );
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
    assert!(control.calls.load(Ordering::SeqCst) >= control.at);
    Ok(())
}

#[test]
fn nested_configuration_toml_uses_m0_size_cap_and_pinned_parser_depth_guard() -> TestResult {
    for name in ["Cargo.toml", "rust-toolchain.toml"] {
        let f = Fixture::new()?;
        ck(fs::create_dir(f.project.join("unused")))?;
        let prefix = if name == "Cargo.toml" {
            "[package]\nname='unused'\n#"
        } else {
            "[toolchain]\nchannel='1.98.1'\n#"
        };
        let mut bytes = prefix.as_bytes().to_vec();
        bytes.resize(256 * 1024, b'x');
        let path = f.project.join("unused").join(name);
        ck(fs::write(&path, &bytes))?;
        ck(f.capture()?)?;
        bytes.push(b'x');
        ck(fs::write(&path, bytes))?;
        assert_eq!(
            f.capture()?,
            Err(ProjectError::Rejected(
                OperationalErrorCode::OutputLimitExceeded
            ))
        );
        for value in [
            format!("{}0{}", "[".repeat(512), "]".repeat(512)),
            format!("{}0{}", "{ a=".repeat(512), "}".repeat(512)),
        ] {
            let deep = format!("[package.metadata]\ndeep={value}\n");
            // Exercise the pinned parser's own depth rejection; no custom pre-parser.
            let error = toml::from_str::<toml::Value>(&deep)
                .err()
                .ok_or("expected parser depth rejection")?;
            assert!(error.to_string().contains("max recursion depth"), "{error}");
            ck(fs::write(&path, deep))?;
            assert_eq!(
                f.capture()?,
                Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject))
            );
        }
    }
    Ok(())
}

#[test]
fn exclusions_and_cargo_config_denial_are_case_insensitive_including_directories() -> TestResult {
    for name in [".GiT", "TaRgEt"] {
        let f = Fixture::new()?;
        ck(fs::create_dir(f.project.join(name)))?;
        ck(fs::write(
            f.project.join(name).join("secret"),
            "must not transfer",
        ))?;
        let bundle = ck(f.capture()?)?;
        assert!(
            !bundle
                .files()
                .iter()
                .any(|file| file.path().starts_with(name))
        );
        assert!(
            !bundle
                .directories()
                .iter()
                .any(|directory| directory.starts_with(name))
        );
    }
    for name in ["CONFIG", "ConFig.ToMl"] {
        for directory in [false, true] {
            let f = Fixture::new()?;
            let parent = f.project.join(".CaRgO");
            ck(fs::create_dir(&parent))?;
            if directory {
                ck(fs::create_dir(parent.join(name)))?;
            } else {
                ck(fs::write(parent.join(name), ""))?;
            }
            assert_eq!(
                f.capture()?,
                Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied))
            );
        }
    }
    Ok(())
}

#[test]
fn cargo_paths_cannot_reference_excluded_components_at_any_depth_or_casing() -> TestResult {
    for value in [
        "target/readme",
        ".GiT/license",
        "nested/TARGET/generated.rs",
        "Target/../readme",
    ] {
        let f = Fixture::new()?;
        ck(fs::create_dir(f.project.join("unused")))?;
        ck(fs::write(
            f.project.join("unused/Cargo.toml"),
            format!("[package]\nname='unused'\nreadme='{value}'\n"),
        ))?;
        assert_eq!(
            f.capture()?,
            Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied))
        );
    }
    Ok(())
}

#[test]
fn vanished_inner_file_is_invalid_capture_but_registry_lease_stays_live() -> TestResult {
    let f = Fixture::new()?;
    let attack = f.project.join("000_attack");
    ck(fs::write(&attack, "original"))?;
    let backend = f.backend()?;
    let opened = ck(backend.open(f.path()?, &Continue))?;
    let count = At {
        calls: AtomicUsize::new(0),
        at: usize::MAX,
        action: || Ok(()),
    };
    ck(backend.revalidate(&opened.lease, &count))?;
    let revalidate_checks = count.calls.load(Ordering::SeqCst);
    let fd = ck(rustix::fs::openat(
        rustix::fs::CWD,
        &f.project,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::DIRECTORY,
        rustix::fs::Mode::empty(),
    ))?;
    let entries = ck(ck(rustix::fs::Dir::read_from(&fd))?.collect::<Result<Vec<_>, _>>())?.len();
    let mut registry = ck(ProjectRegistry::new(
        backend,
        OsReferences,
        MonotonicClock::default(),
        30,
        1,
    ))?;
    let registered = ck(registry.open(f.path()?, &Continue))?;
    // Registry resolve contributes check-before/check-after around revalidate;
    // source then revalidates once more. Delete the first sorted regular entry
    // at the same post-enumeration/pre-open checkpoint used by the FIFO test.
    let control = At {
        calls: AtomicUsize::new(0),
        at: 2 * revalidate_checks + entries + 4,
        action: || fs::remove_file(&attack).map_err(|_| ProjectError::Internal),
    };
    assert_eq!(
        registry.source(&registered.project_ref, &control),
        Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject))
    );
    assert!(control.calls.load(Ordering::SeqCst) >= control.at);
    assert_eq!(
        ck(registry.resolve(&registered.project_ref, &Continue))?,
        registered.identity
    );
    Ok(())
}
