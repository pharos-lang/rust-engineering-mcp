//! Actual CLI diagnostics, fixed catalog fixtures and hostile PATH sentinels.
#![cfg(target_os = "macos")]
use rust_engineering_application::ReferenceGenerator;
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let id = rust_engineering_project::OsReferences
            .generate()
            .map_err(|e| format!("{e:?}"))?;
        let root = PathBuf::from("/private/tmp").join(format!("doctor-{id}"));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        Ok(Self(root))
    }
    fn trust(&self) -> TestResult {
        fs::copy(
            fixtures().join("fixture-trust.json"),
            self.0.join("trust.json"),
        )?;
        fs::set_permissions(self.0.join("trust.json"), fs::Permissions::from_mode(0o600))?;
        Ok(())
    }
    fn flags(&self) -> Vec<String> {
        vec![
            "--catalog-store".into(),
            self.0.display().to_string(),
            "--catalog-trust".into(),
            self.0.join("trust.json").display().to_string(),
        ]
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/catalog")
}
fn run(args: &[String], path: Option<&Path>) -> Result<Output, Box<dyn std::error::Error>> {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"));
    cmd.args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = path {
        cmd.env("PATH", path);
    }
    let mut child = cmd.spawn()?;
    let out = child.stdout.take().ok_or("stdout")?;
    let err = child.stderr.take().ok_or("stderr")?;
    let (tx, rx) = mpsc::channel();
    let (etx, erx) = mpsc::channel();
    thread::spawn(move || {
        let mut b = Vec::new();
        let r = out.take(128 * 1024 + 1).read_to_end(&mut b).map(|_| b);
        let _ = tx.send(r);
    });
    thread::spawn(move || {
        let mut b = Vec::new();
        let r = err.take(128 * 1024 + 1).read_to_end(&mut b).map(|_| b);
        let _ = etx.send(r);
    });
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(s) = child.try_wait()? {
            break s;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("doctor CLI deadline".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let output = Output {
        status,
        stdout: rx.recv_timeout(Duration::from_secs(2))??,
        stderr: erx.recv_timeout(Duration::from_secs(2))??,
    };
    assert!(output.stdout.len() <= 128 * 1024 && output.stderr.len() <= 128 * 1024);
    Ok(output)
}
fn doctor(args: Vec<String>) -> Result<(Output, Value), Box<dyn std::error::Error>> {
    let mut full = vec!["doctor".into(), "--json".into()];
    full.extend(args);
    let output = run(&full, None)?;
    assert!(output.stderr.is_empty());
    let report = serde_json::from_slice(&output.stdout)?;
    Ok((output, report))
}
fn check<'a>(report: &'a Value, id: &str) -> Result<&'a Value, Box<dyn std::error::Error>> {
    report["checks"]
        .as_array()
        .ok_or("checks")?
        .iter()
        .find(|v| v["id"] == id)
        .ok_or_else(|| format!("check {id} missing").into())
}
fn deny_control() {
    if std::env::var("RUST_MCP_NETWORK_DENIED").as_deref() == Ok("1") {
        assert!(std::net::TcpListener::bind("127.0.0.1:0").is_err());
        assert!(std::net::TcpListener::bind("[::1]:0").is_err());
    }
}
#[test]
fn passive_doctor_and_version_are_bounded_and_never_execute_path_tools() -> TestResult {
    deny_control();
    let (_, mut initial) = doctor(vec![])?;
    initial["duration_ms"] = json!(0);
    assert_eq!(
        initial,
        serde_json::from_str::<Value>(include_str!("snapshots/doctor-report.json"))?
    );
    let f = Fixture::new()?;
    for tool in [
        "rustc",
        "cargo",
        "rustfmt",
        "clippy-driver",
        "cargo-audit",
        "rustup",
        "docker",
    ] {
        let file = f.0.join(tool);
        fs::write(&file, "#!/bin/sh\nprintf called > \"$0.called\"\nexit 0\n")?;
        fs::set_permissions(file, fs::Permissions::from_mode(0o700))?;
    }
    let args = vec![
        "doctor".into(),
        "--json".into(),
        "--docker".into(),
        f.0.join("docker").display().to_string(),
        "--docker-socket".into(),
        f.0.join("missing.sock").display().to_string(),
        "--state-root".into(),
        f.0.join("not-created").display().to_string(),
        "--rust-image".into(),
        rust_engineering_execution::APPROVED_RUST_IMAGE.into(),
    ];
    let output = run(&args, Some(&f.0))?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(report["format_version"], 1);
    assert_eq!(report["operation"], "doctor");
    assert_eq!(report["mode"], "passive");
    assert_eq!(check(&report, "rustc")?["status"], "not_checked");
    assert_eq!(check(&report, "audit_engine")?["status"], "available");
    assert_eq!(check(&report, "cargo_audit")?["status"], "not_used");
    assert!(report["runtime"].is_null());
    assert!(!f.0.join("not-created").exists());
    for e in fs::read_dir(&f.0)? {
        assert!(!e?.file_name().to_string_lossy().ends_with(".called"));
    }
    let human = run(&["doctor".into()], Some(&f.0))?;
    assert!(human.status.success());
    assert!(String::from_utf8(human.stdout)?.contains("diagnostic scope only"));
    let v = run(&["version".into(), "--json".into()], Some(&f.0))?;
    let v: Value = serde_json::from_slice(&v.stdout)?;
    assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(v["compiled_local"], cfg!(feature = "local"));
    let v = run(&["version".into()], Some(&f.0))?;
    assert_eq!(
        v.stdout,
        format!("rust-engineering-mcp {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
    );
    Ok(())
}
#[test]
fn doctor_reads_signed_catalog_without_admin_lease_or_staging_mutation() -> TestResult {
    deny_control();
    let f = Fixture::new()?;
    f.trust()?;
    let import = run(
        &[
            "catalog".into(),
            "import".into(),
            fixtures()
                .join("fixture-1.tar.zst")
                .canonicalize()?
                .display()
                .to_string(),
            "--store".into(),
            f.0.display().to_string(),
            "--trust".into(),
            f.0.join("trust.json").display().to_string(),
            "--json".into(),
        ],
        None,
    )?;
    assert!(
        import.status.success(),
        "{} {}",
        String::from_utf8_lossy(&import.stdout),
        String::from_utf8_lossy(&import.stderr)
    );
    let _lease = rust_engineering_project::catalog_store::CatalogStore::open(&f.0)
        .map_err(|e| format!("{e:?}"))?;
    fs::write(f.0.join("staging.bundle"), b"sentinel")?;
    let (output, r) = doctor(f.flags())?;
    assert!(output.status.success(), "{r}");
    assert_eq!(check(&r, "catalog")?["status"], "available");
    assert_eq!(r["catalog"]["catalog"]["value"]["sequence"], 1);
    let serialized = serde_json::to_string(&r)?;
    assert!(!serialized.contains(&f.0.display().to_string()));
    assert!(!serialized.contains("/private/tmp"));
    assert_eq!(fs::read(f.0.join("staging.bundle"))?, b"sentinel");
    let before = fs::read(f.0.join("active.bundle"))?;
    let mut args = f.flags();
    args.extend([
        "--catalog-model-dir".into(),
        f.0.join("missing-model").display().to_string(),
    ]);
    let (out, r) = doctor(args)?;
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(check(&r, "catalog")?["status"], "available");
    assert_eq!(check(&r, "model")?["status"], "unavailable");
    assert_eq!(fs::read(f.0.join("active.bundle"))?, before);
    fs::rename(f.0.join("trust.json"), f.0.join("real-trust.json"))?;
    symlink(f.0.join("real-trust.json"), f.0.join("trust.json"))?;
    let (out, r) = doctor(f.flags())?;
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(check(&r, "catalog")?["component_reason"], "denied");
    assert_eq!(fs::read(f.0.join("staging.bundle"))?, b"sentinel");
    Ok(())
}
#[test]
fn doctor_rejects_closed_cli_syntax_and_reports_configured_access_failures() -> TestResult {
    deny_control();
    for args in [
        vec!["doctor", "--json", "--json"],
        vec!["doctor", "--active", "--active"],
        vec!["doctor", "--catalog-store", "/private/tmp"],
        vec!["doctor", "--download", "yes"],
        vec!["version", "--json", "--json"],
        vec!["serve", "--stdio", "--active"],
    ] {
        let args: Vec<String> = args.into_iter().map(str::to_owned).collect();
        let out = run(&args, None)?;
        assert_eq!(out.status.code(), Some(2));
        assert!(out.stdout.is_empty());
    }
    let f = Fixture::new()?;
    f.trust()?;
    let (out, r) = doctor(f.flags())?;
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(check(&r, "catalog")?["status"], "unavailable");
    assert!(!f.0.join("store.lock").exists());
    assert!(r.as_object().ok_or("report")?.contains_key("runtime"));
    assert_eq!(r["operation"], json!("doctor"));
    Ok(())
}
