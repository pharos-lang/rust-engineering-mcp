//! Differential structural fixtures against the pinned Cargo executable.
//! Child processes exist only in this harness, never in the project adapter.
#![cfg(target_os = "macos")]

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use rust_engineering_application::{
    OperationControl, ProjectBackend, ProjectError, ReferenceGenerator,
};
use rust_engineering_project::{OsReferences, SecureProjects};

type TestResult<T = ()> = Result<T, String>;
fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> TestResult<T> {
    value.map_err(|error| format!("{error:?}"))
}
struct Continue;
impl OperationControl for Continue {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> TestResult<Self> {
        let physical = checked(std::env::temp_dir().canonicalize())?;
        let unique = checked(OsReferences.generate())?;
        let root = physical.join(format!("rust-mcp-cargo-oracle-{unique}"));
        checked(fs::create_dir(&root))?;
        checked(fs::create_dir(root.join("cargo-home")))?;
        Ok(Self(root))
    }
    fn file(&self, relative: &str, content: &str) -> TestResult {
        let path = self.0.join(relative);
        let parent = path
            .parent()
            .ok_or_else(|| "fixture parent absent".to_owned())?;
        checked(fs::create_dir_all(parent))?;
        checked(fs::write(path, content))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct Output {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn cargo(fixture: &Fixture, project: &Path, arguments: &[&str]) -> TestResult<Output> {
    const LIMIT: u64 = 64 * 1024;
    let executable = Path::new(env!("CARGO"));
    if !executable.is_absolute() {
        return Err("CARGO must name an absolute toolchain executable".to_owned());
    }
    let bin = executable
        .parent()
        .ok_or_else(|| "Cargo binary parent absent".to_owned())?;
    let rustc = bin.join("rustc");
    let mut command = Command::new(executable);
    command
        .args(arguments)
        .current_dir(project)
        .env_clear()
        .env("PATH", bin)
        .env("RUSTC", rustc)
        .env("CARGO_HOME", fixture.0.join("cargo-home"))
        .env("HOME", &fixture.0)
        .env("TMPDIR", &fixture.0)
        .env("CARGO_NET_OFFLINE", "true")
        .env("CARGO_TERM_COLOR", "never")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = ChildGuard(checked(command.spawn())?);
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| "missing stdout pipe".to_owned())?;
    let stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| "missing stderr pipe".to_owned())?;
    let (sender, receiver) = mpsc::channel();
    for (is_stdout, stream) in [
        (true, Box::new(stdout) as Box<dyn Read + Send>),
        (false, Box::new(stderr) as Box<dyn Read + Send>),
    ] {
        let sender = sender.clone();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stream.take(LIMIT + 1).read_to_end(&mut bytes);
            let _ = sender.send((is_stdout, result, bytes));
        });
    }
    drop(sender);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut captured = (None, None);
    let status = loop {
        while let Ok((is_stdout, result, bytes)) = receiver.try_recv() {
            checked(result)?;
            if bytes.len() > LIMIT as usize {
                return Err("Cargo exceeded bounded output".to_owned());
            }
            if is_stdout {
                captured.0 = Some(bytes);
            } else {
                captured.1 = Some(bytes);
            }
        }
        if let Some(status) = checked(child.0.try_wait())? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err("Cargo exceeded 10 second deadline".to_owned());
        }
        thread::sleep(Duration::from_millis(10));
    };
    while captured.0.is_none() || captured.1.is_none() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let (is_stdout, result, bytes) = checked(receiver.recv_timeout(remaining))?;
        checked(result)?;
        if bytes.len() > LIMIT as usize {
            return Err("Cargo exceeded bounded output".to_owned());
        }
        if is_stdout {
            captured.0 = Some(bytes);
        } else {
            captured.1 = Some(bytes);
        }
    }
    Ok(Output {
        success: status.success(),
        stdout: captured.0.ok_or_else(|| "stdout absent".to_owned())?,
        stderr: captured.1.ok_or_else(|| "stderr absent".to_owned())?,
    })
}

fn compare(fixture: &Fixture, expected: bool) -> TestResult {
    let project = fixture.0.join("project");
    let version = cargo(fixture, &project, &["--version"])?;
    if !version.success || !version.stdout.starts_with(b"cargo 1.98.1 ") {
        return Err(format!(
            "expected pinned Cargo 1.98.1, observed {}",
            String::from_utf8_lossy(&version.stdout)
        ));
    }
    let backend = checked(SecureProjects::new(std::slice::from_ref(&fixture.0)))?;
    let path = project
        .to_str()
        .ok_or_else(|| "non-UTF8 fixture".to_owned())?;
    let adapter = backend.open(path, &Continue);
    let output = cargo(
        fixture,
        &project,
        &[
            "metadata",
            "--no-deps",
            "--offline",
            "--format-version",
            "1",
        ],
    )?;
    assert_eq!(
        adapter.is_ok(),
        expected,
        "adapter outcome: {:?}",
        adapter.as_ref().err()
    );
    assert_eq!(
        output.success,
        expected,
        "Cargo disagrees: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    if expected {
        assert!(output.stdout.starts_with(b"{"));
        assert!(!output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn package_metadata_agrees_without_executing_build_script() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.file(
        "project/Cargo.toml",
        "[package]\nname='oracle_basic'\nversion='0.1.0'\nedition='2024'\n",
    )?;
    fixture.file("project/src/lib.rs", "pub fn basic() {}\n")?;
    fixture.file("project/build.rs", "fn main() { std::fs::write(\"BUILD_SCRIPT_EXECUTED\", b\"bad\").unwrap(); panic!(\"must never execute\"); }\n")?;
    compare(&fixture, true)?;
    assert!(!fixture.0.join("project/BUILD_SCRIPT_EXECUTED").exists());
    Ok(())
}

#[test]
fn virtual_workspace_literal_members_inheritance_and_paths_agree() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.file("project/Cargo.toml", "[workspace]\nmembers=['a','b']\ndefault-members=['a']\nresolver='3'\n[workspace.package]\nversion='1.2.3'\nedition='2024'\n[workspace.dependencies]\nb={path='b',version='1.2.3'}\n")?;
    fixture.file("project/a/Cargo.toml", "[package]\nname='a'\nversion.workspace=true\nedition.workspace=true\n[dependencies]\nb.workspace=true\n")?;
    fixture.file("project/a/src/lib.rs", "pub fn a() {}\n")?;
    fixture.file(
        "project/b/Cargo.toml",
        "[package]\nname='b'\nversion.workspace=true\nedition.workspace=true\n",
    )?;
    fixture.file("project/b/src/lib.rs", "pub fn b() {}\n")?;
    compare(&fixture, true)
}

#[test]
fn missing_workspace_member_agrees() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.file(
        "project/Cargo.toml",
        "[workspace]\nmembers=['missing']\nresolver='3'\n",
    )?;
    compare(&fixture, false)
}

#[test]
fn missing_targets_agree() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.file(
        "project/Cargo.toml",
        "[package]\nname='missing_targets'\nversion='0.1.0'\n",
    )?;
    compare(&fixture, false)
}

#[test]
fn invalid_semver_agrees() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.file(
        "project/Cargo.toml",
        "[package]\nname='invalid_semver'\nversion='not-semver'\n",
    )?;
    fixture.file("project/src/lib.rs", "")?;
    compare(&fixture, false)
}

#[test]
fn default_members_follow_automatic_membership_and_exclude_prefixes() -> TestResult {
    for excluded in [false, true] {
        let fixture = Fixture::new()?;
        fixture.file(
            "project/Cargo.toml",
            &format!(
                "[workspace]\nresolver='3'\nmembers=['app']\ndefault-members=['vendor/sub']\n{}",
                if excluded { "exclude=['vendor']\n" } else { "" }
            ),
        )?;
        fixture.file(
            "project/app/Cargo.toml",
            "[package]\nname='app'\nversion='0.1.0'\n[dependencies]\nsub={path='../vendor/sub'}\n",
        )?;
        fixture.file("project/app/src/lib.rs", "")?;
        fixture.file(
            "project/vendor/sub/Cargo.toml",
            "[package]\nname='sub'\nversion='0.1.0'\n",
        )?;
        fixture.file("project/vendor/sub/src/lib.rs", "")?;
        compare(&fixture, !excluded)?;
    }
    Ok(())
}
