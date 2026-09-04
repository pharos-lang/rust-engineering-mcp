#![cfg(target_os = "macos")]
use rust_engineering_application::ReferenceGenerator;
use std::os::unix::fs::PermissionsExt;
use std::{
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[path = "catalog_cli/bundle_fixture.rs"]
mod bundle_fixture;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let id = rust_engineering_project::OsReferences
            .generate()
            .map_err(|e| format!("{e:?}"))?;
        let root = std::env::temp_dir()
            .canonicalize()?
            .join(format!("catalog-cli-{id}"));
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        fs::copy(
            fixtures().join("fixture-trust.json"),
            root.join("trust.json"),
        )?;
        fs::set_permissions(root.join("trust.json"), fs::Permissions::from_mode(0o600))?;
        Ok(Self(root))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/catalog")
        .canonicalize()
        .unwrap_or_default()
}
fn run(root: &Path, args: &[&str]) -> io::Result<Output> {
    run_mode(root, args, true)
}
fn run_mode(root: &Path, args: &[&str], json: bool) -> io::Result<Output> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"));
    command
        .env_clear()
        .arg("catalog")
        .args(args)
        .arg("--store")
        .arg(root)
        .arg("--trust")
        .arg(root.join("trust.json"));
    if json {
        command.arg("--json");
    }
    command.output()
}

fn report(output: &Output) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    assert!(output.stderr.is_empty(), "{:?}", output.stderr);
    Ok(serde_json::from_slice(&output.stdout)?)
}
#[test]
fn separate_process_import_status_sync_and_durable_rollback() -> TestResult {
    let f = Fixture::new()?;
    assert_eq!(
        report(&run(&f.0, &["status"])?)?["error_code"],
        "CATALOG_UNAVAILABLE"
    );
    let one = fixtures().join("fixture-1.tar.zst");
    let two = fixtures().join("fixture-2.tar.zst");
    let one = one.to_str().ok_or("path")?;
    let two = two.to_str().ok_or("path")?;
    let first = run(&f.0, &["import", one])?;
    assert!(first.status.success());
    assert_eq!(report(&first)?["catalog"]["sequence"], 1);
    let second = run(&f.0, &["sync", "--source", two])?;
    assert!(second.status.success());
    assert_eq!(report(&second)?["catalog"]["sequence"], 2);
    let rollback = run(&f.0, &["import", one])?;
    assert!(!rollback.status.success());
    assert_eq!(report(&rollback)?["error_code"], "CATALOG_ROLLBACK");
    let equal = run(&f.0, &["import", two])?;
    assert_eq!(report(&equal)?["error_code"], "CATALOG_ROLLBACK");
    let status = report(&run(&f.0, &["status"])?)?;
    assert_eq!(status["catalog"]["sequence"], 2);
    assert_eq!(status["catalog"]["floor_sequence"], 2);
    assert_eq!(status["catalog"]["reservation_pending"], false);
    assert_eq!(status["catalog"]["semantics"], "latest_known");
    assert_eq!(status["network_used"], false);
    assert_eq!(
        status["catalog"]["evidence"]["provenance"]["observed_at"],
        100
    );
    assert_eq!(fs::read(f.0.join("active.bundle"))?, fs::read(two)?);
    Ok(())
}
#[test]
fn invalid_candidate_preserves_active_and_corrupt_active_recovery_keeps_floor() -> TestResult {
    let f = Fixture::new()?;
    let one = fixtures().join("fixture-1.tar.zst");
    let one = one.to_str().ok_or("path")?;
    assert!(run(&f.0, &["import", one])?.status.success());
    let original = fs::read(f.0.join("active.bundle"))?;
    let bad = f.0.join("bad.tar.zst");
    fs::write(&bad, b"untrusted")?;
    assert!(
        !run(&f.0, &["import", bad.to_str().ok_or("path")?])?
            .status
            .success()
    );
    assert_eq!(fs::read(f.0.join("active.bundle"))?, original);
    fs::write(f.0.join("active.bundle"), b"corrupt active")?;
    assert!(run(&f.0, &["import", one])?.status.success());
    assert_eq!(fs::read(f.0.join("active.bundle"))?, original);
    let two = fixtures().join("fixture-2.tar.zst");
    assert!(
        run(&f.0, &["import", two.to_str().ok_or("path")?])?
            .status
            .success()
    );
    let floor = fs::read(f.0.join("floor.record"))?;
    fs::write(f.0.join("active.bundle"), b"corrupt active")?;
    assert_eq!(
        report(&run(&f.0, &["import", one])?)?["error_code"],
        "CATALOG_ROLLBACK"
    );
    assert_eq!(fs::read(f.0.join("floor.record"))?, floor);
    assert!(
        run(&f.0, &["import", two.to_str().ok_or("path")?])?
            .status
            .success()
    );
    fs::write(f.0.join("floor.record"), b"corrupt floor")?;
    assert_eq!(
        report(&run(&f.0, &["import", two.to_str().ok_or("path")?])?)?["error_code"],
        "CATALOG_STATE_INVALID"
    );
    Ok(())
}
#[test]
fn stale_staging_does_not_override_authenticated_active() -> TestResult {
    let f = Fixture::new()?;
    let one = fixtures().join("fixture-1.tar.zst");
    let two = fixtures().join("fixture-2.tar.zst");
    assert!(
        run(&f.0, &["import", two.to_str().ok_or("path")?])?
            .status
            .success()
    );
    fs::copy(one, f.0.join("staging.bundle"))?;
    fs::set_permissions(
        f.0.join("staging.bundle"),
        fs::Permissions::from_mode(0o600),
    )?;
    assert_eq!(report(&run(&f.0, &["status"])?)?["catalog"]["sequence"], 2);
    assert!(!f.0.join("staging.bundle").exists());
    Ok(())
}

#[test]
fn durable_reservation_recovers_interruption_and_missing_floor_never_resets() -> TestResult {
    let f = Fixture::new()?;
    let one = fixtures().join("fixture-1.tar.zst");
    let two = fixtures().join("fixture-2.tar.zst");
    assert!(
        run(&f.0, &["import", one.to_str().ok_or("path")?])?
            .status
            .success()
    );
    let older = fs::read(f.0.join("active.bundle"))?;
    assert!(
        run(&f.0, &["import", two.to_str().ok_or("path")?])?
            .status
            .success()
    );
    // Deterministic representation of a crash after reserving 2 before activating it.
    fs::write(f.0.join("active.bundle"), &older)?;
    assert_eq!(report(&run(&f.0, &["status"])?)?["catalog"]["sequence"], 1);
    assert_eq!(
        report(&run(&f.0, &["status"])?)?["catalog"]["floor_sequence"],
        2
    );
    assert_eq!(
        report(&run(&f.0, &["status"])?)?["catalog"]["reservation_pending"],
        true
    );
    let human = run_mode(&f.0, &["status"], false)?;
    assert!(human.status.success());
    assert!(human.stderr.is_empty());
    let text = std::str::from_utf8(&human.stdout)?;
    assert!(text.contains("Sequence: 1\nReserved sequence: 2\nReservation pending: true\n"));
    assert!(text.contains("Semantics: latest_known\nNetwork acquisition attempted: false\n"));
    assert_eq!(
        report(&run(&f.0, &["import", one.to_str().ok_or("path")?])?)?["error_code"],
        "CATALOG_ROLLBACK"
    );
    assert!(
        run(&f.0, &["import", two.to_str().ok_or("path")?])?
            .status
            .success()
    );
    fs::remove_file(f.0.join("active.bundle"))?;
    assert_eq!(
        report(&run(&f.0, &["import", one.to_str().ok_or("path")?])?)?["error_code"],
        "CATALOG_ROLLBACK"
    );
    assert!(
        run(&f.0, &["import", two.to_str().ok_or("path")?])?
            .status
            .success()
    );
    fs::remove_file(f.0.join("floor.record"))?;
    assert_eq!(
        report(&run(&f.0, &["import", two.to_str().ok_or("path")?])?)?["error_code"],
        "CATALOG_STATE_INVALID"
    );
    Ok(())
}

#[cfg(feature = "local")]
#[test]
#[ignore = "full gate: exact E5/ORT assets and enforced macOS network deny required"]
fn actual_cli_rebuild_reopens_native_index_without_network() -> TestResult {
    assert_eq!(std::env::var("RUST_MCP_NETWORK_DENIED").as_deref(), Ok("1"));
    assert!(std::net::TcpListener::bind("127.0.0.1:0").is_err());
    assert!(std::net::TcpListener::bind("[::1]:0").is_err());
    let model = std::env::var("RUST_MCP_E5_DIR")?;
    let f = Fixture::new()?;
    let index = Fixture::new()?;
    let one = fixtures().join("fixture-1.tar.zst");
    assert!(
        run(&f.0, &["import", one.to_str().ok_or("path")?])?
            .status
            .success()
    );
    let rebuild = run(
        &f.0,
        &[
            "rebuild-index",
            "--index-store",
            index.0.to_str().ok_or("path")?,
            "--model-dir",
            &model,
        ],
    )?;
    assert!(
        rebuild.status.success(),
        "{}",
        String::from_utf8_lossy(&rebuild.stdout)
    );
    let bytes = fs::read(index.0.join("active.bundle"))?;
    assert!(bytes.starts_with(b"REMCP-LANCE8-V1\0"));
    let status = run(
        &f.0,
        &[
            "status",
            "--index-store",
            index.0.to_str().ok_or("path")?,
            "--model-dir",
            &model,
        ],
    )?;
    assert!(status.status.success());
    assert_eq!(
        report(&status)?["catalog"]["semantic_index_available"],
        true
    );
    fs::write(index.0.join("active.bundle"), b"corrupt derived index")?;
    let status = run(
        &f.0,
        &[
            "status",
            "--index-store",
            index.0.to_str().ok_or("path")?,
            "--model-dir",
            &model,
        ],
    )?;
    assert!(status.status.success());
    assert_eq!(
        report(&status)?["catalog"]["semantic_index_available"],
        false
    );
    assert_eq!(report(&status)?["catalog"]["sequence"], 1);

    // Embed the actual exported native objects, not synthetic vectors or merely
    // declared availability. Import and status are separate CLI processes.
    let embedded = Fixture::new()?;
    let bundle = bundle_fixture::with_native_index(&fs::read(&one)?, &bytes)?;
    let path = f.0.join("embedded-native.tar.zst");
    fs::write(&path, &bundle)?;
    let imported = run(
        &embedded.0,
        &[
            "import",
            path.to_str().ok_or("path")?,
            "--model-dir",
            &model,
        ],
    )?;
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stdout)
    );
    assert_eq!(
        report(&imported)?["catalog"]["semantic_index_available"],
        true
    );
    assert_eq!(report(&imported)?["network_used"], false);
    assert_eq!(fs::read(embedded.0.join("active.bundle"))?, bundle);
    assert!(embedded.0.join("floor.record").is_file());
    let status = run(&embedded.0, &["status", "--model-dir", &model])?;
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
    assert_eq!(
        report(&status)?["catalog"]["semantic_index_available"],
        true
    );
    assert_eq!(report(&status)?["catalog"]["sequence"], 1);
    assert_eq!(
        report(&status)?["catalog"]["evidence"]["provenance"]["observed_at"],
        100
    );

    let rejected = Fixture::new()?;
    let mut corrupt = bytes;
    *corrupt.last_mut().ok_or("empty native artifact")? ^= 1;
    // Refresh outer hashes/signature: transport authentication succeeds, but the
    // actual native artifact is corrupt and must fail before reserving the floor.
    let invalid_bundle = bundle_fixture::with_native_index(&fs::read(one)?, &corrupt)?;
    let trust = rust_engineering_catalog::bundle::PublisherTrust::parse(&fs::read(
        fixtures().join("fixture-trust.json"),
    )?)?;
    rust_engineering_catalog::bundle::verify(&invalid_bundle, &trust)?;
    let path = f.0.join("signed-corrupt-native.tar.zst");
    fs::write(&path, invalid_bundle)?;
    let output = run(
        &rejected.0,
        &[
            "import",
            path.to_str().ok_or("path")?,
            "--model-dir",
            &model,
        ],
    )?;
    assert!(!output.status.success());
    assert_eq!(
        report(&output)?["error_code"],
        "SEMANTIC_REBUILD_UNAVAILABLE"
    );
    assert_eq!(report(&output)?["network_used"], false);
    for name in [
        "active.bundle",
        "floor.record",
        "staging.bundle",
        "floor.staging",
    ] {
        assert!(!rejected.0.join(name).exists(), "unexpected record: {name}");
    }
    println!(
        "PASS real embedded native index import/restart status; signed corrupt native payload rejected before active/floor"
    );
    Ok(())
}

#[test]
fn key_rotation_retains_floor_and_channel_change_is_explicit() -> TestResult {
    let f = Fixture::new()?;
    let one = fixtures().join("fixture-1.tar.zst");
    let two = fixtures().join("fixture-2.tar.zst");
    assert!(
        run(&f.0, &["import", one.to_str().ok_or("path")?])?
            .status
            .success()
    );
    let (rotated, trust) = bundle_fixture::resign_with_new_key(&fs::read(two)?)?;
    fs::write(f.0.join("trust.json"), &trust)?;
    assert_eq!(
        report(&run(&f.0, &["status"])?)?["error_code"],
        "CATALOG_ACTIVE_UNVERIFIED"
    );
    let input = f.0.join("rotated.tar.zst");
    fs::write(&input, rotated)?;
    assert!(
        run(&f.0, &["import", input.to_str().ok_or("path")?])?
            .status
            .success()
    );
    let state = report(&run(&f.0, &["status"])?)?;
    assert_eq!(state["catalog"]["sequence"], 2);
    assert_eq!(state["catalog"]["floor_sequence"], 2);
    let floor = fs::read(f.0.join("floor.record"))?;
    let mut changed: serde_json::Value = serde_json::from_slice(&trust)?;
    changed["channel"] = serde_json::json!("another-channel");
    fs::write(f.0.join("trust.json"), serde_json::to_vec(&changed)?)?;
    assert_eq!(
        report(&run(&f.0, &["status"])?)?["error_code"],
        "CATALOG_TRUST_MISMATCH"
    );
    assert_eq!(fs::read(f.0.join("floor.record"))?, floor);
    fs::write(f.0.join("trust.json"), trust)?;
    let mut changed: serde_json::Value = serde_json::from_slice(&floor)?;
    changed["sequence"] = serde_json::json!(1);
    fs::write(f.0.join("floor.record"), serde_json::to_vec(&changed)?)?;
    assert_eq!(
        report(&run(&f.0, &["status"])?)?["error_code"],
        "CATALOG_STATE_INVALID"
    );
    Ok(())
}
