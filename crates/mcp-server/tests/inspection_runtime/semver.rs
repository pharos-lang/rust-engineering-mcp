//! MCP-level M3-04 selection. Gateway fixture families live in the execution
//! adapter; this case qualifies protocol projection and private raw-log readback.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};

const QUALITY_LOCK_STATE: &str = "RUST_MCP_TEST_SEMVER_QUALITY_LOCK_STATE";
const QUALITY_LOCK_READY: &str = "RUST_MCP_TEST_SEMVER_QUALITY_LOCK_READY";
const QUALITY_LOCK_RELEASE: &str = "RUST_MCP_TEST_SEMVER_QUALITY_LOCK_RELEASE";

struct QualityLockProcess {
    child: Option<Child>,
    release: PathBuf,
}

impl QualityLockProcess {
    fn finish(&mut self) -> Result {
        fs::write(&self.release, b"release")?;
        let status = self
            .child
            .take()
            .ok_or("quality lock helper absent")?
            .wait()?;
        status
            .success()
            .then_some(())
            .ok_or_else(|| format!("quality lock helper failed: {status}").into())
    }
}

impl Drop for QualityLockProcess {
    fn drop(&mut self) {
        let _ = fs::write(&self.release, b"release");
        let Some(child) = self.child.as_mut() else {
            return;
        };
        for _ in 0..100 {
            match child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(_) => break,
            }
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn hold_quality_lock(fixture: &Fixture) -> Result<QualityLockProcess> {
    // Create and validate the fixed store layout before the helper opens its
    // already-known lock file. `open` itself intentionally retains no lock.
    drop(rust_engineering_project::NativeQualityArtifactStore::open(
        &fixture.state,
    )?);
    let ready = fixture.root.join("quality-lock-ready");
    let release = fixture.root.join("quality-lock-release");
    let child = Command::new(std::env::current_exe()?)
        .arg("--exact")
        .arg("semver_runtime::external_quality_lock_helper")
        .arg("--nocapture")
        .env(QUALITY_LOCK_STATE, &fixture.state)
        .env(QUALITY_LOCK_READY, &ready)
        .env(QUALITY_LOCK_RELEASE, &release)
        .spawn()?;
    for _ in 0..500 {
        if ready.exists() {
            return Ok(QualityLockProcess {
                child: Some(child),
                release,
            });
        }
        thread::sleep(Duration::from_millis(10));
    }
    let mut process = QualityLockProcess {
        child: Some(child),
        release,
    };
    let child = process.child.as_mut().ok_or("quality lock helper absent")?;
    let _ = child.kill();
    let status = child.wait()?;
    Err(format!("quality lock helper did not become ready: {status}").into())
}

#[test]
fn external_quality_lock_helper() -> Result {
    use rustix::fs::{CWD, FlockOperation, Mode, OFlags, flock, openat};
    let Some(state) = std::env::var_os(QUALITY_LOCK_STATE) else {
        return Ok(());
    };
    let ready = PathBuf::from(std::env::var_os(QUALITY_LOCK_READY).ok_or("ready path")?);
    let release = PathBuf::from(std::env::var_os(QUALITY_LOCK_RELEASE).ok_or("release path")?);
    let lock = openat(
        CWD,
        PathBuf::from(state)
            .join("rust-mcp-quality-artifacts-v1")
            .join("store.lock"),
        OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOFOLLOW,
        Mode::empty(),
    )?;
    flock(&lock, FlockOperation::NonBlockingLockExclusive)?;
    fs::write(&ready, b"ready")?;
    for _ in 0..30_000 {
        if release.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err("quality lock helper release timeout".into())
}

pub(super) fn prepare_side(root: &std::path::Path, body: &str) -> Result {
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='semver-wire'\nversion='1.0.0'\nedition='2024'\n",
    )?;
    fs::write(
        root.join("Cargo.lock"),
        "version=4\n[[package]]\nname='semver-wire'\nversion='1.0.0'\n",
    )?;
    fs::write(root.join("src/lib.rs"), body)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; pending M3-04 calibration"]
fn mcp_semver_projects_findings_and_reads_bounded_raw_resource() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    prepare_side(&fixture.project, "pub fn kept() {}\npub fn removed() {}\n")?;
    let candidate = fixture.root.join("candidate");
    prepare_side(&candidate, "pub fn kept() {}\n")?;
    let baseline_before = fs::read(fixture.project.join("src/lib.rs"))?;
    let candidate_before = fs::read(candidate.join("src/lib.rs"))?;
    let mut server = Server::start_with_arguments(
        &fixture,
        vec!["--root".into(), candidate.as_os_str().to_owned()],
    )?;
    server.send(request(1, "tools/list"))?;
    let listed = server.receive(1, DISCOVERY_TIMEOUT)?;
    let tool = listed["result"]["tools"]
        .as_array()
        .ok_or("tools")?
        .iter()
        .find(|tool| tool["name"] == "rust.semver.check")
        .cloned()
        .ok_or("semver tool")?;
    server.send(call(
        2,
        "rust.project.open",
        json!({"path": fixture.project}),
    ))?;
    let baseline =
        server.receive(2, DISCOVERY_TIMEOUT)?["result"]["structuredContent"]["data"]["project_ref"]
            .clone();
    server.send(call(3, "rust.project.open", json!({"path": candidate})))?;
    let candidate_ref =
        server.receive(3, DISCOVERY_TIMEOUT)?["result"]["structuredContent"]["data"]["project_ref"]
            .clone();
    let args = json!({"baseline_project_ref":baseline,"candidate_project_ref":candidate_ref,"execution_mode":"synchronous","timeout_seconds":60});
    jsonschema::validator_for(&tool["inputSchema"])?
        .validate(&args)
        .map_err(|error| error.to_string())?;
    server.send(call(4, "rust.semver.check", args))?;
    let response = server.receive(4, JOIN_TIMEOUT)?;
    let output = response["result"]["structuredContent"].clone();
    assert_eq!(
        serde_json::from_str::<Value>(
            response["result"]["content"][0]["text"]
                .as_str()
                .ok_or("text mirror")?
        )?,
        output
    );
    jsonschema::validator_for(&tool["outputSchema"])?
        .validate(&output)
        .map_err(|error| error.to_string())?;
    let uri = output["data"]["raw_output"]["uri"]
        .as_str()
        .ok_or("raw output URI")?;
    assert!(uri.starts_with("rust-quality-artifact://"), "{uri}");
    server.send(resource_read_request(5, uri))?;
    let resource = server.receive(5, DISCOVERY_TIMEOUT)?;
    let bytes = STANDARD.decode(
        resource["result"]["contents"][0]["blob"]
            .as_str()
            .ok_or("raw blob")?,
    )?;
    assert!(bytes.starts_with(b"=== stdout ===\n"));
    let job = resource["result"]["contents"][0]["_meta"]["job_id"]
        .as_str()
        .ok_or("quality job id")?;
    let owner = uri
        .strip_prefix("rust-quality-artifact://")
        .and_then(|rest| rest.split_once('/'))
        .map(|(owner, _)| owner)
        .ok_or("quality owner")?;
    server.send(resource_read_request(
        6,
        &format!("rust-quality-artifact://{owner}/{job}"),
    ))?;
    let index = server.receive(6, DISCOVERY_TIMEOUT)?;
    let page: Value = serde_json::from_str(
        index["result"]["contents"][0]["text"]
            .as_str()
            .ok_or("quality index")?,
    )?;
    assert_eq!(page["job_id"], job);
    assert_eq!(page["members"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        fs::read(fixture.project.join("src/lib.rs"))?,
        baseline_before
    );
    assert_eq!(fs::read(candidate.join("src/lib.rs"))?, candidate_before);
    server.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; pending M3-04 calibration"]
fn mcp_semver_busy_quality_store_uses_stage0_raw_resource() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    prepare_side(&fixture.project, "pub fn kept() {}\npub fn removed() {}\n")?;
    let candidate = fixture.root.join("candidate-stage0");
    prepare_side(&candidate, "pub fn kept() {}\n")?;
    let mut quality_lock = hold_quality_lock(&fixture)?;
    let mut server = Server::start_with_arguments(
        &fixture,
        vec!["--root".into(), candidate.as_os_str().to_owned()],
    )?;
    server.send(request(1, "tools/list"))?;
    let _ = server.receive(1, DISCOVERY_TIMEOUT)?;
    server.send(call(
        2,
        "rust.project.open",
        json!({"path": fixture.project}),
    ))?;
    let baseline =
        server.receive(2, DISCOVERY_TIMEOUT)?["result"]["structuredContent"]["data"]["project_ref"]
            .clone();
    server.send(call(3, "rust.project.open", json!({"path": candidate})))?;
    let candidate_ref =
        server.receive(3, DISCOVERY_TIMEOUT)?["result"]["structuredContent"]["data"]["project_ref"]
            .clone();
    server.send(call(
        4,
        "rust.semver.check",
        json!({"baseline_project_ref":baseline,"candidate_project_ref":candidate_ref,"execution_mode":"synchronous","timeout_seconds":60}),
    ))?;
    let response = server.receive(4, JOIN_TIMEOUT)?;
    let uri = response["result"]["structuredContent"]["data"]["raw_output"]["uri"]
        .as_str()
        .ok_or("stage0 raw output URI")?;
    assert!(uri.starts_with("rust-artifact://"), "{uri}");
    server.send(resource_read_request(5, uri))?;
    let resource = server.receive(5, DISCOVERY_TIMEOUT)?;
    assert!(resource.get("error").is_none(), "{resource}");
    server.finish()?;
    quality_lock.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    Ok(())
}
