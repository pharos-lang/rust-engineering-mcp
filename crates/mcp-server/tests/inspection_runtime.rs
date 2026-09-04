//! Explicit, serial runtime tests. Only the server's approved gateway executes
//! Cargo; this harness creates benign source and runs read-only Docker listings.
#![cfg(target_os = "macos")]

use rust_engineering_execution::APPROVED_RUST_IMAGE;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
const DOCKER: &str = "/Applications/Docker.app/Contents/Resources/bin/docker";
const VERSION: &str = "2026-07-28";
// The request deadline is 120s. Cleanup can perform several 10s control calls;
// this outer harness budget deliberately includes calibration AND joined cleanup.
const JOIN_TIMEOUT: Duration = Duration::from_secs(300);
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(15);
const PIPE_LIMIT: usize = 2 * 1024 * 1024;
static SERIAL: Mutex<()> = Mutex::new(());
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture {
    root: PathBuf,
    project: PathBuf,
    state: PathBuf,
    socket: String,
    successful: bool,
}
impl Fixture {
    fn new() -> Result<Self> {
        let socket = std::env::var("RUST_MCP_TEST_SOCKET")?;
        if !socket.starts_with('/') || socket.chars().any(char::is_control) {
            return Err("RUST_MCP_TEST_SOCKET must be an explicit absolute socket path".into());
        }
        let root = std::env::temp_dir().canonicalize()?.join(format!(
            "rust-mcp-inspection-wire-{}-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        // Exclusive creation precedes every child path. No shared temp directory
        // or caller-controlled workspace is reused by these tests.
        fs::create_dir(&root)?;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        let fixture = Self {
            project: root.join("workspace"),
            state: root.join("state"),
            root,
            socket,
            successful: false,
        };
        fs::create_dir(&fixture.project)?;
        fs::create_dir(&fixture.state)?;
        fs::set_permissions(&fixture.state, fs::Permissions::from_mode(0o700))?;
        fs::create_dir(fixture.root.join("docker-config"))?;
        fs::write(fixture.root.join("docker-config/config.json"), "{}")?;
        fixture.write_workspace()?;
        Ok(fixture)
    }
    fn write_workspace(&self) -> Result {
        fs::write(
            self.project.join("Cargo.toml"),
            r#"[workspace]
members = ["app", "helper"]
default-members = ["app"]
resolver = "3"
[workspace.package]
version = "0.1.0"
edition = "2024"
[workspace.dependencies]
renamed = { package = "helper", path = "helper", default-features = false, features = ["extra"] }
[profile.dev]
opt-level = 1
incremental = true
[profile.release]
lto = "thin"
[profile.release.package.helper]
opt-level = 2
"#,
        )?;
        for package in ["app", "helper"] {
            fs::create_dir(self.project.join(package))?;
            fs::create_dir(self.project.join(package).join("src"))?;
            fs::write(
                self.project.join(package).join("src/lib.rs"),
                "pub fn benign() {}\n",
            )?;
        }
        fs::write(
            self.project.join("app/Cargo.toml"),
            r#"[package]
name = "app"
version.workspace = true
edition.workspace = true
[dependencies]
renamed.workspace = true
[dev-dependencies]
renamed.workspace = true
[target.'cfg(unix)'.build-dependencies]
renamed.workspace = true
[features]
default = ["extra"]
extra = ["renamed/extra"]
"#,
        )?;
        fs::write(
            self.project.join("helper/Cargo.toml"),
            r#"[package]
name = "helper"
version.workspace = true
edition.workspace = true
[features]
extra = []
"#,
        )?;
        // Hand-authored lock for these path-only declarations: no host Cargo
        // invocation, resolver downloads, build scripts or proc macros.
        fs::write(
            self.project.join("Cargo.lock"),
            r#"version = 4
[[package]]
name = "app"
version = "0.1.0"
dependencies = ["helper"]
[[package]]
name = "helper"
version = "0.1.0"
"#,
        )?;
        Ok(())
    }
    fn source_bytes(&self) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut files = BTreeMap::new();
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "app/Cargo.toml",
            "app/src/lib.rs",
            "helper/Cargo.toml",
            "helper/src/lib.rs",
        ] {
            files.insert(path.into(), fs::read(self.project.join(path))?);
        }
        Ok(files)
    }
    fn objects(&self, kind: &str, job: Option<&str>) -> Result<Vec<String>> {
        let mut command = Command::new(DOCKER);
        command
            .env_clear()
            .current_dir(&self.root)
            .arg("--config")
            .arg(self.root.join("docker-config"))
            .arg("--host")
            .arg(format!("unix://{}", self.socket))
            .args([kind, "ls"]);
        if kind == "container" {
            command.args(["--all", "--no-trunc"]);
        }
        command.args(["--filter", "label=org.rust-mcp.execution=true"]);
        let job_filter = job.map_or_else(
            || "label=org.rust-mcp.rust-job".into(),
            |nonce| format!("label=org.rust-mcp.rust-job={nonce}"),
        );
        command.args([
            "--filter",
            &job_filter,
            "--format",
            if kind == "container" {
                "{{.Names}}\t{{.Command}}"
            } else {
                "{{.Name}}"
            },
        ]);
        let bytes = bounded_command(command)?;
        let text = String::from_utf8(bytes)?;
        Ok(text
            .lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }
    fn assert_clean(&self, job: Option<&str>) -> Result {
        for kind in ["container", "volume"] {
            let objects = self.objects(kind, job)?;
            if !objects.is_empty() {
                return Err(format!(
                    "owned {kind} objects remain: {objects:?}; fixture state path {}",
                    self.state.display()
                )
                .into());
            }
        }
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Default to retaining diagnostics on every early return or panic. Only
        // explicit successful exit + object checks permit removing private state.
        if self.successful {
            let _ = fs::remove_dir_all(&self.root);
        } else {
            eprintln!(
                "inspection fixture retained after failure: {}",
                self.root.display()
            );
        }
    }
}

struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn bounded_reader(reader: impl Read + Send + 'static) -> Receiver<io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = reader
            .take((PIPE_LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .and_then(|_| {
                if bytes.len() > PIPE_LIMIT {
                    Err(io::Error::other("harness pipe budget exceeded"))
                } else {
                    Ok(bytes)
                }
            });
        let _ = sender.send(result);
    });
    receiver
}
fn bounded_command(mut command: Command) -> Result<Vec<u8>> {
    let mut child = Reap(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let out = bounded_reader(child.0.stdout.take().ok_or("missing Docker stdout")?);
    let err = bounded_reader(child.0.stderr.take().ok_or("missing Docker stderr")?);
    let deadline = Instant::now() + CONTROL_TIMEOUT;
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err("read-only Docker listing timed out".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = out.recv_timeout(CONTROL_TIMEOUT)??;
    let stderr = err.recv_timeout(CONTROL_TIMEOUT)??;
    if !status.success() {
        return Err(format!(
            "Docker listing failed: {}",
            String::from_utf8_lossy(&stderr)
        )
        .into());
    }
    Ok(stdout)
}

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Receiver<io::Result<Vec<u8>>>,
    stderr: Receiver<io::Result<Vec<u8>>>,
    pending: BTreeMap<i64, Value>,
}
impl Server {
    fn start(fixture: &Fixture) -> Result<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"))
            .env_clear()
            .current_dir(&fixture.root)
            .args(["serve", "--stdio", "--root"])
            .arg(&fixture.project)
            .arg("--docker")
            .arg(DOCKER)
            .arg("--docker-socket")
            .arg(&fixture.socket)
            .arg("--state-root")
            .arg(&fixture.state)
            .arg("--rust-image")
            .arg(APPROVED_RUST_IMAGE)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (sender, stdout) = mpsc::sync_channel(32);
        let input = child.stdin.take();
        let output = child.stdout.take().ok_or("missing server stdout")?;
        let stderr = bounded_reader(child.stderr.take().ok_or("missing server stderr")?);
        thread::spawn(move || {
            let mut reader = BufReader::new(output).take((PIPE_LIMIT + 1) as u64);
            let mut total = 0;
            loop {
                let mut line = Vec::new();
                let result = reader.read_until(b'\n', &mut line).and_then(|count| {
                    total += count;
                    if total > PIPE_LIMIT {
                        Err(io::Error::other("server stdout budget exceeded"))
                    } else if count > 0 && line.last() != Some(&b'\n') {
                        Err(io::Error::other("partial server frame"))
                    } else {
                        Ok(count)
                    }
                });
                match result {
                    Ok(0) => break,
                    Ok(_) => {
                        if sender.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(Err(error));
                        break;
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin: input,
            stdout,
            stderr,
            pending: BTreeMap::new(),
        })
    }
    fn send(&mut self, value: Value) -> Result {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        let mut input = self.stdin.take().ok_or("stdin closed")?;
        let (tx, rx) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let result = input.write_all(&bytes).and_then(|()| input.flush());
            let _ = tx.send((input, result));
        });
        let (input, result) = rx.recv_timeout(DISCOVERY_TIMEOUT)?;
        self.stdin = Some(input);
        result?;
        Ok(())
    }
    fn receive(&mut self, id: i64, timeout: Duration) -> Result<Value> {
        if let Some(response) = self.pending.remove(&id) {
            return Ok(response);
        }
        let deadline = Instant::now() + timeout;
        loop {
            let frame = self
                .stdout
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))??;
            let value: Value = serde_json::from_slice(&frame)?;
            assert_eq!(value["jsonrpc"], "2.0");
            let actual = value["id"]
                .as_i64()
                .ok_or("unexpected server notification")?;
            if actual == id {
                return Ok(value);
            }
            if self.pending.insert(actual, value).is_some() {
                return Err("duplicate response ID".into());
            }
        }
    }
    fn bootstrap_open(&mut self, fixture: &Fixture) -> Result<(Value, Value)> {
        self.send(request(1, "tools/list"))?;
        let list = self.receive(1, DISCOVERY_TIMEOUT)?;
        let tool = list["result"]["tools"]
            .as_array()
            .ok_or("tools missing")?
            .iter()
            .find(|tool| tool["name"] == "rust.project.inspect")
            .ok_or("inspect missing")?
            .clone();
        self.send(call(
            2,
            "rust.project.open",
            json!({"path":fixture.project}),
        ))?;
        let opened = self.receive(2, DISCOVERY_TIMEOUT)?;
        assert_eq!(
            opened["result"]["structuredContent"]["status"], "passed",
            "{opened}"
        );
        Ok((opened["result"]["structuredContent"]["data"].clone(), tool))
    }
    fn calibration_job(&mut self, fixture: &Fixture) -> Result<String> {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            // Labels, names and full command come from RustGateway. `check`
            // identifies calibration; project inspection runs `metadata`. This
            // checkpoint cannot silently pass after calibration has completed.
            for record in fixture.objects("container", None)? {
                let Some((name, command)) = record.split_once('\t') else {
                    return Err("missing full Docker command in calibration observation".into());
                };
                if let Some(nonce) = name.strip_prefix("rust-mcp-cargo-")
                    && nonce.len() == 32
                    && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
                    && command.trim_matches('"')
                        == "/opt/rust/bin/cargo check --frozen --message-format=json --jobs=1"
                {
                    return Ok(nonce.into());
                }
            }
            if let Ok(frame) = self.stdout.try_recv() {
                return Err(format!(
                    "inspection returned before calibration checkpoint: {}",
                    String::from_utf8_lossy(&frame?)
                )
                .into());
            }
            if Instant::now() >= deadline {
                return Err("never observed a calibration container".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
    fn assert_no_response(&mut self, id: i64) -> Result {
        if self.pending.contains_key(&id) {
            return Err("cancelled request unexpectedly returned a response".into());
        }
        while let Ok(frame) = self.stdout.try_recv() {
            let value: Value = serde_json::from_slice(&frame?)?;
            let actual = value["id"].as_i64().ok_or("unexpected notification")?;
            if actual == id {
                return Err(format!("cancelled request unexpectedly returned: {value}").into());
            }
            if self.pending.insert(actual, value).is_some() {
                return Err("duplicate response ID".into());
            }
        }
        Ok(())
    }
    fn finish(&mut self) -> Result {
        self.stdin.take();
        let deadline = Instant::now() + JOIN_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                if !status.success() {
                    return Err(format!("server exited {status}").into());
                }
                let stderr = self.stderr.recv_timeout(CONTROL_TIMEOUT)??;
                if !stderr.is_empty() {
                    return Err(format!(
                        "unexpected server stderr: {}",
                        String::from_utf8_lossy(&stderr)
                    )
                    .into());
                }
                // Drain through actual pipe EOF after process exit. This closes
                // the race between the reader thread and the final no-id check.
                loop {
                    match self.stdout.recv_timeout(CONTROL_TIMEOUT) {
                        Ok(frame) => {
                            let value: Value = serde_json::from_slice(&frame?)?;
                            let id = value["id"]
                                .as_i64()
                                .ok_or("unexpected final notification")?;
                            if self.pending.insert(id, value).is_some() {
                                return Err("duplicate final response ID".into());
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        Err(error) => return Err(error.into()),
                    }
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("server did not join calibration and cleanup before deadline".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stdin.take();
        // Assertion failures also request EOF and allow the production joined
        // worker to clean up. Killing immediately would orphan its Docker job.
        let deadline = Instant::now() + JOIN_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        eprintln!(
            "inspection harness forced server termination after cleanup deadline; inspect labelled Docker objects before any rerun"
        );
    }
}
fn request(id: i64, method: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":{"_meta":{
        "io.modelcontextprotocol/protocolVersion":VERSION,
        "io.modelcontextprotocol/clientCapabilities":{}
    }}})
}
fn call(id: i64, name: &str, arguments: Value) -> Value {
    let mut request = request(id, "tools/call");
    request["params"]["name"] = json!(name);
    request["params"]["arguments"] = arguments;
    request
}
fn assert_fingerprint(value: &Value) {
    let text = value.as_str().unwrap_or_default();
    assert!(
        text.starts_with("sha256:")
            && text.len() == 71
            && text[7..].bytes().all(|b| b.is_ascii_hexdigit()),
        "{value}"
    );
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn toolchain_inspect_observes_installed_runtime_with_shared_calibration() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let original = fixture.source_bytes()?;
    let mut server = Server::start(&fixture)?;
    let (opened, tool) = server.bootstrap_open(&fixture)?;
    server.send(call(
        3,
        "rust.project.inspect",
        json!({"project_ref":opened["project_ref"]}),
    ))?;
    let job = server.calibration_job(&fixture)?;
    server.send(request(4, "tools/list"))?;
    let list = server.receive(4, DISCOVERY_TIMEOUT)?;
    assert!(list["result"]["tools"].as_array().is_some());
    assert!(
        !server.pending.contains_key(&3),
        "discovery only replied after inspection completed"
    );
    let response = server.receive(3, JOIN_TIMEOUT)?;
    let toolchain_tool = list["result"]["tools"]
        .as_array()
        .ok_or("tools missing")?
        .iter()
        .find(|tool| tool["name"] == "rust.toolchain.inspect")
        .ok_or("toolchain tool missing")?
        .clone();
    server.send(call(
        5,
        "rust.toolchain.inspect",
        json!({"project_ref":opened["project_ref"]}),
    ))?;
    let toolchain_response = server.receive(5, JOIN_TIMEOUT)?;
    // Run exit/cleanup checks before propagating output assertion failures.
    let exit = server.finish();
    let clean = fixture
        .assert_clean(Some(&job))
        .and_then(|()| fixture.assert_clean(None));
    exit?;
    clean?;
    assert_eq!(response["result"]["isError"], false, "{response}");
    let output = &response["result"]["structuredContent"];
    let fallback: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .ok_or("fallback missing")?,
    )?;
    assert_eq!(&fallback, output);
    jsonschema::validator_for(&tool["outputSchema"])?
        .validate(output)
        .map_err(|error| error.to_string())?;
    assert_eq!(output["status"], "passed", "{response}");
    assert_eq!(output["data"]["project_ref"], opened["project_ref"]);
    assert_eq!(
        output["data"]["project_identity_fingerprint"],
        opened["fingerprint"]
    );
    assert_eq!(output["data"]["semantics"], "latest_known");
    let structure = &output["data"]["structure"];
    assert_eq!(structure["workspace_members"], json!([0, 1]));
    assert_eq!(structure["workspace_default_members"], json!([0]));
    let packages = structure["packages"].as_array().ok_or("packages missing")?;
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0]["name"], "app");
    assert_eq!(packages[1]["name"], "helper");
    for package in packages {
        assert_eq!(package["version"], "0.1.0");
        assert_eq!(package["edition"], "2024");
        assert_eq!(package.get("rust_version"), Some(&Value::Null));
        assert_eq!(
            package["targets"][0]["source_path"],
            format!("{}/src/lib.rs", package["name"].as_str().ok_or("name")?)
        );
    }
    let dependencies = packages[0]["direct_dependencies"]
        .as_array()
        .ok_or("dependencies missing")?;
    assert_eq!(dependencies.len(), 3);
    for kind in ["normal", "build", "dev"] {
        let dependency = dependencies
            .iter()
            .find(|dep| dep["kind"] == kind)
            .ok_or("dependency kind missing")?;
        assert_eq!(dependency["name"], "helper");
        assert_eq!(dependency["rename"], "renamed");
        assert_eq!(dependency["uses_default_features"], false);
        assert_eq!(dependency["features"], json!(["extra"]));
        assert_eq!(dependency["origin"]["kind"], "path");
        assert_eq!(dependency["origin"]["relative_path"], "helper");
        assert_fingerprint(&dependency["origin"]["identity"]);
        assert_eq!(
            dependency["target_condition"],
            if kind == "build" {
                json!("cfg(unix)")
            } else {
                Value::Null
            }
        );
    }
    assert!(
        packages[0]["features"]
            .as_array()
            .ok_or("features missing")?
            .iter()
            .any(|feature| feature == &json!({"name":"extra","activations":["renamed/extra"]}))
    );
    let profiles = structure["profiles"].as_array().ok_or("profiles missing")?;
    let dev = profiles
        .iter()
        .find(|profile| profile["name"] == "dev")
        .ok_or("dev profile missing")?;
    assert!(
        dev["settings"]
            .as_array()
            .ok_or("dev settings")?
            .contains(&json!({"name":"incremental","value":{"kind":"boolean","value":true}}))
    );
    let release = profiles
        .iter()
        .find(|profile| profile["name"] == "release")
        .ok_or("release profile missing")?;
    assert_eq!(
        release["package_overrides"],
        json!([{"package":"helper","settings":[{"name":"opt-level","value":{"kind":"integer","value":2}}]}])
    );
    assert_eq!(
        structure["cargo_configuration"],
        json!({"project_config_policy":"rejected","frozen":true,"offline":true,"incremental":false,"target_directory_ephemeral":true})
    );
    assert_eq!(structure["runtime"]["platform"], "linux/aarch64");
    assert_eq!(structure["runtime"]["image_id"], APPROVED_RUST_IMAGE);
    assert_eq!(structure["runtime"]["rust_version"], "1.98.1");
    assert_eq!(structure["runtime"]["cargo_version"], "1.98.1");
    assert_eq!(
        structure["runtime"].get("declared_toolchain"),
        Some(&Value::Null)
    );
    for field in ["configuration_fingerprint", "execution_fingerprint"] {
        assert_fingerprint(&structure["runtime"][field]);
    }
    assert_fingerprint(&structure["source_fingerprint"]);
    assert_ne!(
        structure["source_fingerprint"],
        output["data"]["project_identity_fingerprint"]
    );
    assert_eq!(output["evidence"]["kind"], "snapshot");
    let evidence = &output["evidence"]["details"];
    assert_eq!(evidence["provenance"]["source_kind"], "project_snapshot");
    assert_eq!(
        evidence["provenance"]["source_id"],
        structure["source_fingerprint"]
    );
    assert_eq!(evidence["provenance"]["integrity"], "verified");
    assert_eq!(evidence["provenance"]["network_used"], false);
    let created = evidence["provenance"]["created_at"]
        .as_u64()
        .ok_or("created_at missing")?;
    let observed = evidence["provenance"]["observed_at"]
        .as_u64()
        .ok_or("observed_at missing")?;
    let assessed = evidence["freshness"]["assessed_at"]
        .as_u64()
        .ok_or("assessed_at missing")?;
    assert!(created <= observed && observed <= assessed);
    assert_eq!(evidence["freshness"]["policy"]["id"], "captured-project-v1");
    assert_eq!(fixture.source_bytes()?, original);
    assert!(!fixture.project.join("target").exists());
    assert!(!fixture.project.join("app/target").exists());
    assert_eq!(
        toolchain_response["result"]["isError"], false,
        "{toolchain_response}"
    );
    let toolchain_output = &toolchain_response["result"]["structuredContent"];
    let fallback: Value = serde_json::from_str(
        toolchain_response["result"]["content"][0]["text"]
            .as_str()
            .ok_or("toolchain fallback missing")?,
    )?;
    assert_eq!(&fallback, toolchain_output);
    jsonschema::validator_for(&toolchain_tool["outputSchema"])?
        .validate(toolchain_output)
        .map_err(|error| error.to_string())?;
    assert_eq!(toolchain_output["status"], "passed");
    let data = &toolchain_output["data"];
    assert_eq!(data["project_ref"], opened["project_ref"]);
    assert_eq!(data["project_identity_fingerprint"], opened["fingerprint"]);
    assert_eq!(data["semantics"], "latest_known");
    let observation = &data["observation"];
    assert_eq!(observation.get("declared_toolchain"), Some(&Value::Null));
    assert_eq!(
        observation["source_fingerprint"],
        structure["source_fingerprint"]
    );
    assert_eq!(
        observation["inventory"],
        json!({
            "rustc_version":"1.98.1", "cargo_version":"1.98.1", "channel":"stable",
            "host_triple":"aarch64-unknown-linux-gnu", "installed_targets":["aarch64-unknown-linux-gnu"],
            "installed_components":[
                {"component":"cargo","target":null}, {"component":"clippy","target":null},
                {"component":"rust_std","target":"aarch64-unknown-linux-gnu"},
                {"component":"rustc","target":null}, {"component":"rustfmt","target":null}
            ]
        })
    );
    let runtime = &observation["runtime"];
    for field in ["platform", "image_id", "configuration_fingerprint"] {
        assert_eq!(runtime[field], structure["runtime"][field], "{field}");
    }
    let executions = runtime["executions"]
        .as_array()
        .ok_or("execution evidence missing")?;
    assert_eq!(executions.len(), 3);
    let mut fingerprints = std::collections::BTreeSet::new();
    for (execution, command) in
        executions
            .iter()
            .zip(["compiler_version", "cargo_version", "installed_components"])
    {
        assert_eq!(execution["command"], command);
        assert_fingerprint(&execution["execution_fingerprint"]);
        assert!(
            fingerprints.insert(
                execution["execution_fingerprint"]
                    .as_str()
                    .ok_or("fingerprint missing")?
            )
        );
    }
    assert_eq!(toolchain_output["evidence"]["kind"], "snapshot");
    let evidence = &toolchain_output["evidence"]["details"];
    assert_eq!(
        evidence["provenance"]["source_id"],
        observation["source_fingerprint"]
    );
    assert_eq!(evidence["provenance"]["source_kind"], "project_snapshot");
    assert_eq!(evidence["provenance"]["integrity"], "verified");
    assert_eq!(evidence["provenance"]["network_used"], false);
    assert_ne!(evidence["freshness"]["state"], "live");
    let mut missing_nullable = toolchain_output.clone();
    missing_nullable["data"]["observation"]
        .as_object_mut()
        .ok_or("observation missing")?
        .remove("declared_toolchain");
    assert!(
        !jsonschema::validator_for(&toolchain_tool["outputSchema"])?.is_valid(&missing_nullable)
    );
    println!(
        "M1_TOOLCHAIN_RECEIPT {}",
        serde_json::to_string(toolchain_output)?
    );
    println!("M1_INSPECTION_RECEIPT {}", serde_json::to_string(output)?);
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn eof_and_cancellation_during_calibration_join_workers_and_leave_no_owned_objects() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    for cancel in [false, true] {
        let mut fixture = Fixture::new()?;
        fixture.assert_clean(None)?;
        let mut server = Server::start(&fixture)?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        server.send(call(
            3,
            "rust.project.inspect",
            json!({"project_ref":opened["project_ref"]}),
        ))?;
        let job = server.calibration_job(&fixture)?;
        if cancel {
            server.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":3}}))?;
            // rmcp 3.2.0 removes the cancelled ID from local_ct_pool and
            // suppresses the eventual response (service.rs). Discovery proves
            // receive-loop liveness; independent object observations prove cleanup.
            server.send(request(4, "tools/list"))?;
            assert!(
                server.receive(4, DISCOVERY_TIMEOUT)?["result"]["tools"]
                    .as_array()
                    .is_some()
            );
            server.assert_no_response(3)?;
            let deadline = Instant::now() + JOIN_TIMEOUT;
            loop {
                let containers = fixture.objects("container", Some(&job))?;
                let volumes = fixture.objects("volume", Some(&job))?;
                server.assert_no_response(3)?;
                if containers.is_empty() && volumes.is_empty() {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err("cancelled calibration did not clean up before deadline".into());
                }
                thread::sleep(Duration::from_millis(25));
            }
            fixture.assert_clean(None)?;
            server.send(request(5, "tools/list"))?;
            assert!(
                server.receive(5, DISCOVERY_TIMEOUT)?["result"]["tools"]
                    .as_array()
                    .is_some()
            );
            server.assert_no_response(3)?;
        }
        // In the EOF branch the pipe closes while calibration owns a container.
        let exit = server.finish();
        let clean = fixture
            .assert_clean(Some(&job))
            .and_then(|()| fixture.assert_clean(None));
        exit?;
        clean?;
        if cancel {
            server.assert_no_response(3)?;
        }
        fixture.successful = true;
    }
    Ok(())
}

fn resource_read_request(id: i64, uri: &str) -> Value {
    let mut value = request(id, "resources/read");
    value["params"]["uri"] = json!(uri);
    value
}

fn checked_output(response: &Value, tool: &Value, opened: &Value, status: &str) -> Result<Value> {
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["isError"], false, "{response}");
    let value = &response["result"]["structuredContent"];
    let fallback: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .ok_or("check fallback missing")?,
    )?;
    assert_eq!(&fallback, value);
    jsonschema::validator_for(&tool["outputSchema"])?
        .validate(value)
        .map_err(|error| error.to_string())?;
    assert_eq!(value["status"], status, "{value}");
    assert_eq!(value["error_code"], Value::Null);
    assert_eq!(value["error_message"], Value::Null);
    let data = &value["data"];
    assert_eq!(data["project_ref"], opened["project_ref"]);
    assert_eq!(data["project_identity_fingerprint"], opened["fingerprint"]);
    assert_eq!(data["semantics"], "latest_known");
    assert_eq!(data["validation_complete"], true, "{value}");
    assert_eq!(data["termination"], "exited");
    if status == "passed" {
        assert_eq!(data["exit_code"], 0);
    } else {
        assert!(data["exit_code"].as_i64().is_some_and(|code| code != 0));
    }
    for field in ["stdout", "stderr", "raw_stdout", "raw_stderr"] {
        assert!(
            data.get(field).is_none(),
            "raw stream published as check data"
        );
    }
    assert_eq!(
        value["truncation"],
        json!({"stdout_truncated":false,"stderr_truncated":false,"diagnostics_omitted":0})
    );
    assert_eq!(data["runtime"]["image_id"], APPROVED_RUST_IMAGE);
    assert_eq!(data["runtime"]["platform"], "linux/aarch64");
    assert_fingerprint(&data["source_fingerprint"]);
    for field in ["execution_fingerprint", "configuration_fingerprint"] {
        assert_fingerprint(&data["runtime"][field]);
    }
    assert_ne!(
        data["source_fingerprint"],
        data["project_identity_fingerprint"]
    );
    assert_eq!(value["evidence"]["kind"], "snapshot");
    let evidence = &value["evidence"]["details"];
    assert_eq!(evidence["provenance"]["source_kind"], "project_snapshot");
    assert_eq!(
        evidence["provenance"]["source_id"],
        data["source_fingerprint"]
    );
    assert_eq!(evidence["provenance"]["network_used"], false);
    assert_eq!(evidence["provenance"]["integrity"], "verified");
    assert_eq!(evidence["freshness"]["policy"]["id"], "captured-project-v1");
    let created = evidence["provenance"]["created_at"]
        .as_u64()
        .ok_or("created_at missing")?;
    let observed = evidence["provenance"]["observed_at"]
        .as_u64()
        .ok_or("observed_at missing")?;
    let assessed = evidence["freshness"]["assessed_at"]
        .as_u64()
        .ok_or("assessed_at missing")?;
    assert!(created <= observed && observed <= assessed);
    Ok(value.clone())
}

fn verify_log_resource(
    server: &mut Server,
    id: i64,
    output: &Value,
    expected_code: Option<&str>,
) -> Result {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let log = &output["data"]["log"];
    let uri = log["uri"].as_str().ok_or("log URI missing")?;
    assert!(uri.starts_with(&format!(
        "rust-artifact://{}/",
        output["data"]["project_ref"].as_str().ok_or("owner")?
    )));
    server.send(resource_read_request(id, uri))?;
    let response = server.receive(id, DISCOVERY_TIMEOUT)?;
    assert!(response.get("error").is_none(), "{response}");
    let result = &response["result"];
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["cacheScope"], "private");
    assert_eq!(result["ttlMs"], 0);
    assert_eq!(result["contents"].as_array().map(Vec::len), Some(1));
    let content = &result["contents"][0];
    assert_eq!(content["uri"], uri);
    assert_eq!(content["mimeType"], "application/octet-stream");
    assert!(content.get("text").is_none());
    let bytes = STANDARD.decode(content["blob"].as_str().ok_or("base64 blob missing")?)?;
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(&bytes) {
        use std::fmt::Write as _;
        write!(&mut hash, "{byte:02x}")?;
    }
    assert_eq!(log["sha256"], hash);
    assert_eq!(content["_meta"]["sha256"], hash);
    assert_eq!(log["size_bytes"], bytes.len());
    assert_eq!(content["_meta"]["size_bytes"], bytes.len());
    assert_eq!(log["truncated"], false);
    assert_eq!(content["_meta"]["truncated"], false);
    let initial_retention = log["retention_remaining_seconds"]
        .as_u64()
        .ok_or("log retention missing")?;
    let remaining = content["_meta"]["retention_remaining_seconds"]
        .as_u64()
        .ok_or("resource retention missing")?;
    assert!(remaining > 0 && remaining <= initial_retention && initial_retention <= 3600);
    let raw = std::str::from_utf8(&bytes)?;
    let (stdout, stderr) = raw
        .strip_prefix("=== stdout ===\n")
        .and_then(|text| text.split_once("\n=== stderr ===\n"))
        .ok_or("separate retained streams missing")?;
    let records: Vec<Value> = stdout
        .lines()
        .map(serde_json::from_str)
        .collect::<std::result::Result<_, _>>()?;
    let finished: Vec<_> = records
        .iter()
        .filter(|record| record["reason"] == "build-finished")
        .collect();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0]["success"], expected_code.is_none());
    if let Some(code) = expected_code {
        assert!(
            records
                .iter()
                .any(|record| record["reason"] == "compiler-message"
                    && record["message"]["code"]["code"] == code),
            "error absent from actual retained Cargo stdout"
        );
        assert!(
            !stderr.is_empty(),
            "failed Cargo check should retain Cargo status stderr"
        );
    }
    Ok(())
}

fn assert_diagnostic_source(
    output: &Value,
    code: &str,
    source: &[u8],
    unicode_line: Option<u64>,
) -> Result {
    let diagnostics = output["diagnostics"]
        .as_array()
        .ok_or("diagnostics missing")?;
    let coded = diagnostics
        .iter()
        .find(|diagnostic| diagnostic["code"] == code)
        .ok_or_else(|| format!("normalized {code} absent: {output}"))?;
    assert_eq!(coded["severity"], "error");
    assert_eq!(coded["source"], "rustc");
    let mut unicode_observed = false;
    for diagnostic in diagnostics {
        assert_eq!(diagnostic.get("rendered"), Some(&Value::Null));
        assert_eq!(diagnostic["truncated"], false);
        for span in diagnostic["spans"].as_array().ok_or("spans missing")? {
            assert_eq!(span["file"], "app/src/lib.rs");
            let start: usize = span["bytes"]["start"]
                .as_u64()
                .ok_or("byte start")?
                .try_into()?;
            let end: usize = span["bytes"]["end"]
                .as_u64()
                .ok_or("byte end")?
                .try_into()?;
            assert!(start <= end && end <= source.len());
            for (offset, position) in [(start, &span["start"]), (end, &span["end"])] {
                let prefix =
                    std::str::from_utf8(source.get(..offset).ok_or("out-of-source offset")?)?;
                let line = prefix.bytes().filter(|byte| *byte == b'\n').count() as u64 + 1;
                let line_prefix = prefix.rsplit('\n').next().ok_or("line prefix")?;
                let column = line_prefix.chars().count() as u64 + 1;
                assert_eq!(position["line"], line);
                assert_eq!(position["column"], column);
                if unicode_line == Some(line) && line_prefix.len() > line_prefix.chars().count() {
                    unicode_observed = true;
                    assert_ne!(
                        position["column"],
                        line_prefix.len() + 1,
                        "column accidentally reports byte offset"
                    );
                }
            }
        }
    }
    assert!(
        coded["spans"]
            .as_array()
            .is_some_and(|spans| spans.iter().any(|span| span["is_primary"] == true))
    );
    if unicode_line.is_some() {
        assert!(
            unicode_observed,
            "no diagnostic position after a multibyte source character was checked"
        );
    }
    Ok(())
}

fn assert_fixture_tree(fixture: &Fixture, expected: &BTreeMap<String, Vec<u8>>) -> Result {
    let mut actual = BTreeMap::new();
    let mut directories = std::collections::BTreeSet::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        for entry in fs::read_dir(fixture.project.join(&relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            let name = path.to_str().ok_or("non-UTF8 fixture entry")?.to_owned();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                directories.insert(name);
                pending.push(path);
            } else if kind.is_file() {
                actual.insert(name, fs::read(entry.path())?);
            } else {
                return Err("unexpected non-regular fixture entry".into());
            }
        }
    }
    assert_eq!(&actual, expected, "unexpected host source mutation");
    assert_eq!(
        directories,
        ["app", "app/src", "helper", "helper/src"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn check_reports_success_and_borrow_errors_with_live_log_resources() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    // Benign compile-only inputs: these strings are transferred by the server to
    // its calibrated gateway. The harness never invokes host Cargo on this tree.
    let feature_source = b"#[cfg(not(feature = \"extra\"))]\ncompile_error!(\"explicit feature extra is required\");\npub fn benign() {}\n";
    fs::write(fixture.project.join("app/src/lib.rs"), feature_source)?;
    let mut expected = fixture.source_bytes()?;
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("tool list")?
        .iter()
        .find(|tool| tool["name"] == "rust.check")
        .ok_or("rust.check missing")?
        .clone();
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    server.send(request(4, "resources/list"))?;
    let resources = server.receive(4, DISCOVERY_TIMEOUT)?;
    assert_eq!(resources["result"]["resources"], json!([]));

    let selected = json!({"project_ref":opened["project_ref"],"package":"app","features":["extra"],"no_default_features":true,"all_targets":true});
    assert!(jsonschema::validator_for(&tool["inputSchema"])?.is_valid(&selected));
    server.send(call(10, "rust.check", selected.clone()))?;
    let passed = checked_output(&server.receive(10, JOIN_TIMEOUT)?, &tool, &opened, "passed")?;
    assert_eq!(passed["data"]["options"]["package"], "app");
    assert_eq!(passed["data"]["options"]["features"], json!(["extra"]));
    assert_eq!(passed["data"]["options"]["no_default_features"], true);
    assert_eq!(passed["data"]["options"]["all_targets"], true);
    verify_log_resource(&mut server, 11, &passed, None)?;
    assert_fixture_tree(&fixture, &expected)?;

    let workspace = json!({"project_ref":opened["project_ref"],"workspace":true,"all_targets":true,"all_features":true,"target":"aarch64-unknown-linux-gnu"});
    server.send(call(12, "rust.check", workspace))?;
    let alternate = checked_output(&server.receive(12, JOIN_TIMEOUT)?, &tool, &opened, "passed")?;
    assert_eq!(alternate["data"]["options"]["workspace"], true);
    assert_eq!(alternate["data"]["options"]["all_features"], true);
    assert_eq!(
        alternate["data"]["options"]["target"],
        "aarch64-unknown-linux-gnu"
    );
    assert_eq!(
        passed["data"]["source_fingerprint"],
        alternate["data"]["source_fingerprint"]
    );
    assert_eq!(
        passed["data"]["runtime"]["configuration_fingerprint"],
        alternate["data"]["runtime"]["configuration_fingerprint"]
    );
    assert_ne!(
        passed["data"]["runtime"]["execution_fingerprint"],
        alternate["data"]["runtime"]["execution_fingerprint"],
        "actual argument selections must bind execution identity"
    );
    verify_log_resource(&mut server, 13, &alternate, None)?;
    assert_fixture_tree(&fixture, &expected)?;

    let borrow_source = "pub fn borrow_error() {\n    let mut values = vec![1, 2, 3];\n    let café = &values;\n    let _café = 0; values.push(4);\n    println!(\"{café:?}\");\n}\n";
    fs::write(fixture.project.join("app/src/lib.rs"), borrow_source)?;
    expected.insert("app/src/lib.rs".into(), borrow_source.as_bytes().to_vec());
    server.send(call(20, "rust.check", selected.clone()))?;
    let borrow = checked_output(&server.receive(20, JOIN_TIMEOUT)?, &tool, &opened, "failed")?;
    assert_diagnostic_source(&borrow, "E0502", borrow_source.as_bytes(), Some(4))?;
    assert_ne!(
        borrow["data"]["source_fingerprint"],
        passed["data"]["source_fingerprint"]
    );
    assert_ne!(
        borrow["data"]["runtime"]["execution_fingerprint"],
        passed["data"]["runtime"]["execution_fingerprint"]
    );
    verify_log_resource(&mut server, 21, &borrow, Some("E0502"))?;
    assert_fixture_tree(&fixture, &expected)?;

    let lifetime_source = b"pub fn missing_lifetime(left: &str, right: &str) -> &str {\n    if left.len() > right.len() { left } else { right }\n}\n";
    fs::write(fixture.project.join("app/src/lib.rs"), lifetime_source)?;
    expected.insert("app/src/lib.rs".into(), lifetime_source.to_vec());
    server.send(call(22, "rust.check", selected))?;
    let lifetime = checked_output(&server.receive(22, JOIN_TIMEOUT)?, &tool, &opened, "failed")?;
    assert_diagnostic_source(&lifetime, "E0106", lifetime_source, None)?;
    assert_ne!(
        lifetime["data"]["source_fingerprint"],
        borrow["data"]["source_fingerprint"]
    );
    verify_log_resource(&mut server, 23, &lifetime, Some("E0106"))?;
    assert_fixture_tree(&fixture, &expected)?;

    // Frozen execution must classify lock creation/update as operational failure,
    // and never generate or modify a lock on the host or retained capture.
    let original_lock = expected.get("Cargo.lock").ok_or("fixture lock")?.clone();
    for (id, lock) in [(24, None), (26, Some(b"version = 4\n".as_slice()))] {
        if let Some(lock) = lock {
            fs::write(fixture.project.join("Cargo.lock"), lock)?;
            expected.insert("Cargo.lock".into(), lock.to_vec());
        } else {
            fs::remove_file(fixture.project.join("Cargo.lock"))?;
            expected.remove("Cargo.lock");
        }
        server.send(call(
            id,
            "rust.check",
            json!({"project_ref":opened["project_ref"]}),
        ))?;
        let response = server.receive(id, JOIN_TIMEOUT)?;
        let output = &response["result"]["structuredContent"];
        let uri = output["data"]["log"]["uri"]
            .as_str()
            .ok_or("lock failure log")?;
        server.send(resource_read_request(id + 1, uri))?;
        let resource = server.receive(id + 1, DISCOVERY_TIMEOUT)?;
        use base64::{Engine, engine::general_purpose::STANDARD};
        use sha2::{Digest, Sha256};
        let bytes = STANDARD.decode(
            resource["result"]["contents"][0]["blob"]
                .as_str()
                .ok_or("lock log blob")?,
        )?;
        assert_eq!(
            output["status"],
            "blocked",
            "{}",
            std::str::from_utf8(&bytes)?
        );
        assert_eq!(
            output["error_code"], "LOCKFILE_UPDATE_REQUIRED",
            "{response}"
        );
        assert_eq!(response["result"]["isError"], true);
        assert_eq!(output["data"]["validation_complete"], false);
        jsonschema::validator_for(&tool["outputSchema"])?
            .validate(output)
            .map_err(|error| error.to_string())?;
        let mut hash = String::with_capacity(64);
        for byte in Sha256::digest(&bytes) {
            use std::fmt::Write;
            write!(&mut hash, "{byte:02x}")?;
        }
        assert_eq!(output["data"]["log"]["sha256"], hash);
        assert!(std::str::from_utf8(&bytes)?.contains("--frozen was passed to prevent this"));
        assert_fixture_tree(&fixture, &expected)?;
    }
    fs::write(fixture.project.join("Cargo.lock"), &original_lock)?;
    expected.insert("Cargo.lock".into(), original_lock);

    // Another live reference to the SAME project must not read an artifact by
    // substituting its owner in a known canonical URI.
    server.send(call(
        30,
        "rust.project.open",
        json!({"path":fixture.project}),
    ))?;
    let second = server.receive(30, DISCOVERY_TIMEOUT)?;
    assert_eq!(second["result"]["structuredContent"]["status"], "passed");
    let second_ref = second["result"]["structuredContent"]["data"]["project_ref"]
        .as_str()
        .ok_or("second project ref")?;
    assert_ne!(json!(second_ref), opened["project_ref"]);
    assert_eq!(
        second["result"]["structuredContent"]["data"]["fingerprint"],
        opened["fingerprint"]
    );
    let old_uri = passed["data"]["log"]["uri"]
        .as_str()
        .ok_or("first log URI")?;
    let artifact_id = old_uri.rsplit('/').next().ok_or("artifact id")?;
    let forged = format!("rust-artifact://{second_ref}/{artifact_id}");
    server.send(resource_read_request(31, &forged))?;
    let denied = server.receive(31, DISCOVERY_TIMEOUT)?;
    assert_eq!(denied["error"]["code"], -32602, "{denied}");
    assert!(denied.get("result").is_none());
    verify_log_resource(&mut server, 32, &passed, None)?;

    // Move only this owned fixture. Update its cleanup path immediately; the
    // server retains the ORIGINAL root capability, whose identity is now revoked.
    let moved = fixture.root.join("revoked-workspace");
    fs::rename(&fixture.project, &moved)?;
    fixture.project = moved;
    server.send(resource_read_request(33, old_uri))?;
    let revoked = server.receive(33, DISCOVERY_TIMEOUT)?;
    assert_eq!(revoked["error"]["code"], -32602, "{revoked}");
    assert_eq!(
        revoked["error"], denied["error"],
        "expired/invalid owner must not leak existence"
    );
    server.send(request(34, "resources/list"))?;
    assert_eq!(
        server.receive(34, DISCOVERY_TIMEOUT)?["result"]["resources"],
        json!([])
    );
    let exit = server.finish();
    let clean = fixture.assert_clean(None);
    exit?;
    clean?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_CHECK_RECEIPT {}",
        serde_json::to_string(&json!({
            "passed": [passed["status"], alternate["status"]],
            "failed": [borrow["status"], lifetime["status"]],
            "error_codes": ["E0502", "E0106"],
            "validation_complete": true,
            "logs_verified": 6,
            "frozen_missing_and_stale_locks_blocked":true,
            "resource_reads_verified": 7,
            "wrong_owner_denied": true,
            "revoked_reference_denied": true,
            "cache_scope": "private", "ttl_ms": 0,
            "configuration_fingerprint": passed["data"]["runtime"]["configuration_fingerprint"],
            "source_fingerprints": [passed["data"]["source_fingerprint"],borrow["data"]["source_fingerprint"],lifetime["data"]["source_fingerprint"]],
            "execution_fingerprints": [passed["data"]["runtime"]["execution_fingerprint"],alternate["data"]["runtime"]["execution_fingerprint"],borrow["data"]["runtime"]["execution_fingerprint"]],
            "only_expected_source_mutations": true,
            "cleanup": true
        }))?
    );
    fixture.successful = true;
    Ok(())
}

fn runtime_observer(fixture: &Fixture) -> Command {
    let mut command = Command::new(DOCKER);
    command
        .env_clear()
        .current_dir(&fixture.root)
        .arg("--config")
        .arg(fixture.root.join("docker-config"))
        .arg("--host")
        .arg(format!("unix://{}", fixture.socket));
    command
}

fn active_slow_check(server: &mut Server, fixture: &Fixture, request_id: i64) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        server.assert_no_response(request_id)?;
        // Unlike calibration_job, select RUNNING containers and verify an actual
        // build-script process. The session is already calibrated, so this exact
        // default Cargo command belongs to the newly admitted project check.
        let mut list = runtime_observer(fixture);
        list.args([
            "container",
            "ls",
            "--no-trunc",
            "--filter",
            "label=org.rust-mcp.execution=true",
            "--filter",
            "label=org.rust-mcp.rust-job",
            "--filter",
            "status=running",
            "--format",
            "{{.Names}}\t{{.Command}}",
        ]);
        let records = String::from_utf8(bounded_command(list)?)?;
        for record in records.lines() {
            let Some((name, command)) = record.split_once('\t') else {
                return Err("missing active Cargo command observation".into());
            };
            let Some(nonce) = name.strip_prefix("rust-mcp-cargo-") else {
                continue;
            };
            if nonce.len() != 32
                || !nonce
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || command.trim_matches('"')
                    != "/opt/rust/bin/cargo check --frozen --message-format=json --jobs=1"
            {
                continue;
            }
            let mut top = runtime_observer(fixture);
            top.args(["container", "top", name, "-eo", "pid,args"]);
            let processes = String::from_utf8(bounded_command(top)?)?;
            if processes.lines().skip(1).any(|line| {
                line.split_whitespace().any(|part| {
                    part.starts_with("/work/target/") && part.ends_with("/build-script-build")
                })
            }) {
                server.assert_no_response(request_id)?;
                return Ok(nonce.to_owned());
            }
        }
        if Instant::now() >= deadline {
            return Err(
                "sleeping project build script was not observed before the check deadline margin"
                    .into(),
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn check_cancellation_and_eof_join_active_cargo_jobs() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let mut expected = fixture.source_bytes()?;
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("tools list")?
        .iter()
        .find(|tool| tool["name"] == "rust.check")
        .ok_or("rust.check missing")?
        .clone();
    let arguments = json!({"project_ref":opened["project_ref"]});
    // Complete one ordinary check first: all following containers are project
    // jobs, not lazy calibration fixtures. The same inspector/worker is reused.
    server.send(call(4, "rust.check", arguments.clone()))?;
    let initial = checked_output(&server.receive(4, JOIN_TIMEOUT)?, &tool, &opened, "passed")?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    // Cargo auto-detects build.rs. No manifest/ProjectRef identity is changed.
    // This benign sleep is compiled and executed ONLY by the contained gateway;
    // the harness never invokes Cargo/rustc against the host fixture tree.
    let build = b"fn main() { std::thread::sleep(std::time::Duration::from_secs(60)); }\n";
    fs::write(fixture.project.join("app/build.rs"), build)?;
    expected.insert("app/build.rs".into(), build.to_vec());

    let cancel_started = Instant::now();
    server.send(call(10, "rust.check", arguments.clone()))?;
    let cancelled_job = active_slow_check(&mut server, &fixture, 10)?;
    server.send(request(11, "tools/list"))?;
    assert!(
        server.receive(11, DISCOVERY_TIMEOUT)?["result"]["tools"]
            .as_array()
            .is_some()
    );
    server.send(resource_read_request(
        15,
        initial["data"]["log"]["uri"]
            .as_str()
            .ok_or("initial log")?,
    ))?;
    let busy = server.receive(15, DISCOVERY_TIMEOUT)?;
    assert_eq!(busy["error"]["code"], -32000);
    assert_eq!(
        busy["error"]["message"],
        "Artifact worker is busy; retry after the active operation"
    );
    server.assert_no_response(10)?;
    // The gateway's operation timeout is 30s. Sending before 20s distinguishes
    // explicit cancellation from a test that merely waits out that timeout.
    assert!(
        cancel_started.elapsed() < Duration::from_secs(20),
        "missed active cancellation window"
    );
    server.send(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10}}),
    )?;
    let cleanup_deadline = Instant::now() + JOIN_TIMEOUT;
    loop {
        let containers = fixture.objects("container", Some(&cancelled_job))?;
        let volumes = fixture.objects("volume", Some(&cancelled_job))?;
        server.assert_no_response(10)?;
        if containers.is_empty() && volumes.is_empty() {
            break;
        }
        if Instant::now() >= cleanup_deadline {
            return Err("cancelled active Cargo job retained owned objects".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
    fixture.assert_clean(None)?;
    server.send(request(12, "tools/list"))?;
    assert!(
        server.receive(12, DISCOVERY_TIMEOUT)?["result"]["tools"]
            .as_array()
            .is_some()
    );
    server.assert_no_response(10)?;
    assert_fixture_tree(&fixture, &expected)?;

    // Re-admission occurs only after observed cleanup. Seeing a second sleeping
    // build script proves the original joined worker released its permit and the
    // calibrated inspector remained usable after clean cancellation.
    let eof_started = Instant::now();
    server.send(call(20, "rust.check", arguments))?;
    let eof_job = active_slow_check(&mut server, &fixture, 20)?;
    assert_ne!(eof_job, cancelled_job);
    server.send(request(21, "tools/list"))?;
    assert!(
        server.receive(21, DISCOVERY_TIMEOUT)?["result"]["tools"]
            .as_array()
            .is_some()
    );
    server.assert_no_response(10)?;
    server.assert_no_response(20)?;
    assert!(
        eof_started.elapsed() < Duration::from_secs(20),
        "missed active EOF window"
    );
    // finish closes stdin, then waits for process exit AND output pipe closure;
    // its deadline includes joined worker shutdown and verified gateway cleanup.
    let exit = server.finish();
    let clean = fixture
        .assert_clean(Some(&eof_job))
        .and_then(|()| fixture.assert_clean(None));
    exit?;
    clean?;
    server.assert_no_response(10)?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_CHECK_CANCELLATION_RECEIPT {}",
        serde_json::to_string(&json!({
            "calibrated_by_successful_check":true,
            "active_build_scripts_observed":2,
            "discovery_responsive_during_checks":true,
            "cancelled_request_response_suppressed":true,
            "worker_reused_after_observed_cleanup":true,
            "eof_joined_shutdown":true,
            "only_expected_source_mutations":true,
            "cleanup":true
        }))?
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn format_reports_workspace_diffs_without_source_writes() -> Result {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    // Benign sentinel only: a formatter must parse, never execute this build script.
    let sentinel = "fn main() {\n    panic!(\"formatting must not execute build scripts\");\n}\n";
    fs::write(fixture.project.join("app/build.rs"), sentinel)?;
    let mut expected = fixture.source_bytes()?;
    expected.insert("app/build.rs".into(), sentinel.as_bytes().to_vec());
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("tools")?
        .iter()
        .find(|t| t["name"] == "rust.fmt.check")
        .ok_or("format tool")?
        .clone();
    let mut outputs = Vec::new();
    let mut verified_logs = 0;
    for case in 0..7 {
        match case {
            0 => (),
            1 => {
                for path in ["app/src/lib.rs", "helper/src/lib.rs"] {
                    let text = "pub fn benign(){let café=1; println!(\"{café}\");}\n";
                    fs::write(fixture.project.join(path), text)?;
                    expected.insert(path.into(), text.as_bytes().to_vec());
                }
            }
            2 => {
                let config = "disable_all_formatting = true\n";
                fs::write(fixture.project.join("rustfmt.toml"), config)?;
                expected.insert("rustfmt.toml".into(), config.as_bytes().to_vec());
            }
            3 => {
                let config = "hard_tabs = true\n";
                fs::write(fixture.project.join("rustfmt.toml"), config)?;
                expected.insert("rustfmt.toml".into(), config.as_bytes().to_vec());
                for (path, text) in [
                    (
                        "app/src/lib.rs",
                        "pub fn benign() {\n\tprintln!(\"tab style\");\n}\n",
                    ),
                    ("helper/src/lib.rs", "pub fn benign() {}\n"),
                    (
                        "app/build.rs",
                        "fn main() {\n\tpanic!(\"formatting must not execute build scripts\");\n}\n",
                    ),
                ] {
                    fs::write(fixture.project.join(path), text)?;
                    expected.insert(path.into(), text.as_bytes().to_vec());
                }
            }
            4 => {
                let text = "pub fn broken( {\n";
                fs::write(fixture.project.join("app/src/lib.rs"), text)?;
                expected.insert("app/src/lib.rs".into(), text.as_bytes().to_vec());
            }
            5 => {
                let config = "newline_style = \"Unix\"\n";
                fs::write(fixture.project.join("rustfmt.toml"), config)?;
                expected.insert("rustfmt.toml".into(), config.as_bytes().to_vec());
                for (path, text) in [
                    ("app/src/lib.rs", "pub fn benign() {}\r\n"),
                    ("app/build.rs", sentinel),
                ] {
                    fs::write(fixture.project.join(path), text)?;
                    expected.insert(path.into(), text.as_bytes().to_vec());
                }
            }
            6 => {
                let text = (0..1800)
                    .map(|n| format!("pub fn f_{n}(){{}}\n"))
                    .collect::<String>();
                fs::write(fixture.project.join("app/src/lib.rs"), &text)?;
                expected.insert("app/src/lib.rs".into(), text.into_bytes());
            }
            _ => unreachable!("closed fixture cases"),
        }
        let id = 10 + case * 2;
        server.send(call(
            id,
            "rust.fmt.check",
            json!({"project_ref":opened["project_ref"]}),
        ))?;
        let response = server.receive(id, JOIN_TIMEOUT)?;
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["isError"], false, "{response}");
        let value = &response["result"]["structuredContent"];
        jsonschema::validator_for(&tool["outputSchema"])?
            .validate(value)
            .map_err(|e| e.to_string())?;
        assert_eq!(
            serde_json::from_str::<Value>(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .ok_or("text")?
            )?,
            *value
        );
        let status = if matches!(case, 0 | 3) {
            "passed"
        } else {
            "failed"
        };
        assert_eq!(value["status"], status, "case {case}: {value}");
        let data = &value["data"];
        assert_eq!(data["project_ref"], opened["project_ref"]);
        assert_eq!(data["project_identity_fingerprint"], opened["fingerprint"]);
        assert_eq!(data["semantics"], "latest_known");
        assert_eq!(data["runtime"]["image_id"], APPROVED_RUST_IMAGE);
        assert_eq!(
            data["validation_complete"],
            case != 4,
            "case {case}: {value}"
        );
        assert_eq!(
            value["evidence"]["details"]["provenance"]["network_used"],
            false
        );
        for key in ["source_fingerprint", "project_identity_fingerprint"] {
            assert_fingerprint(&data[key]);
        }
        assert_fingerprint(&data["runtime"]["configuration_fingerprint"]);
        match case {
            0 | 3 => assert_eq!(data["affected_files"], json!([])),
            1 | 2 => {
                assert_eq!(
                    data["affected_files"],
                    json!(["app/src/lib.rs", "helper/src/lib.rs"])
                );
                let diff = data["diff"].as_str().ok_or("small diff missing")?;
                assert!(diff.contains("Diff in app/src/lib.rs:"));
                assert!(diff.contains("café"));
                assert!(!diff.contains("Diff in /source/"));
            }
            4 => assert!(data["diff"].is_null()),
            5 => assert_eq!(data["affected_files"], json!(["app/src/lib.rs"])),
            6 => {
                assert_eq!(data["affected_files"], json!(["app/src/lib.rs"]));
                assert!(data["diff"].is_null());
                assert_eq!(data["diff_omitted"], true);
            }
            _ => unreachable!("closed cases"),
        }
        let log = &data["log"];
        server.send(resource_read_request(
            id + 1,
            log["uri"].as_str().ok_or("log URI")?,
        ))?;
        let resource = server.receive(id + 1, DISCOVERY_TIMEOUT)?;
        let content = &resource["result"]["contents"][0];
        let bytes = STANDARD.decode(content["blob"].as_str().ok_or("log blob")?)?;
        assert_eq!(
            log["sha256"],
            Sha256::digest(&bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        assert_eq!(log["size_bytes"], bytes.len());
        assert_eq!(resource["result"]["cacheScope"], "private");
        assert_eq!(resource["result"]["ttlMs"], 0);
        if case == 4 {
            assert!(String::from_utf8_lossy(&bytes).contains("error"));
        }
        verified_logs += 1;
        outputs.push(value.clone());
        assert_fixture_tree(&fixture, &expected)?;
        fixture.assert_clean(None)?;
    }
    server.finish()?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_FORMAT_RECEIPT {}",
        serde_json::to_string(&json!({
            "status":"passed", "cases":7, "logs_verified":verified_logs,
            "configured_style_honored":true, "disable_all_overridden":true,
            "all_workspace_members":true, "parse_failure_incomplete":true,
            "newline_only_difference":true, "large_diff_omitted":true,
            "source_unchanged":true, "build_script_not_executed":true, "cleanup":true,
            "configuration_fingerprint":outputs[0]["data"]["runtime"]["configuration_fingerprint"],
            "execution_fingerprints":outputs.iter().map(|v| &v["data"]["runtime"]["execution_fingerprint"]).collect::<Vec<_>>()
        }))?
    );
    fixture.successful = true;
    Ok(())
}

fn assert_clippy_finding(output: &Value, code: &str, severity: &str, file: &str) -> Result {
    let diagnostic = output["diagnostics"]
        .as_array()
        .ok_or("Clippy diagnostics missing")?
        .iter()
        .find(|item| {
            item["code"] == code
                && item["spans"]
                    .as_array()
                    .is_some_and(|spans| spans.iter().any(|span| span["file"] == file))
        })
        .ok_or_else(|| format!("missing {code} in {file}: {output}"))?;
    assert_eq!(diagnostic["source"], "clippy");
    assert_eq!(diagnostic["severity"], severity);
    assert_eq!(diagnostic["rendered"], Value::Null);
    assert_eq!(diagnostic["truncated"], false);
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn clippy_profiles_report_findings_and_verified_log_resources() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    // These benign lint inputs compile only through the server's gateway.
    let source = "pub fn warning(value: u64) -> u8 {\n    return value as u8;\n}\n";
    fs::write(fixture.project.join("app/src/lib.rs"), source)?;
    let manifest = fs::read_to_string(fixture.project.join("app/Cargo.toml"))?
        .replace("default = [\"extra\"]", "default = []");
    fs::write(fixture.project.join("app/Cargo.toml"), manifest)?;
    // An independent member discriminates --workspace from Cargo's default
    // members; path dependencies can themselves receive Clippy diagnostics.
    let workspace = fs::read_to_string(fixture.project.join("Cargo.toml"))?.replace(
        "members = [\"app\", \"helper\"]",
        "members = [\"app\", \"helper\", \"solo\"]",
    );
    fs::write(fixture.project.join("Cargo.toml"), workspace)?;
    let lock = fs::read_to_string(fixture.project.join("Cargo.lock"))?
        + "\n[[package]]\nname = \"solo\"\nversion = \"0.1.0\"\n";
    fs::write(fixture.project.join("Cargo.lock"), lock)?;
    fs::create_dir(fixture.project.join("solo"))?;
    fs::create_dir(fixture.project.join("solo/src"))?;
    let mut expected = fixture.source_bytes()?;
    for (path, text) in [
        (
            "solo/Cargo.toml",
            "[package]\nname = \"solo\"\nversion.workspace = true\nedition.workspace = true\n",
        ),
        (
            "solo/src/lib.rs",
            "pub fn standalone_warning() -> u8 { return 1; }\n",
        ),
    ] {
        fs::write(fixture.project.join(path), text)?;
        expected.insert(path.into(), text.as_bytes().to_vec());
    }
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("tools missing")?
        .iter()
        .find(|tool| tool["name"] == "rust.clippy")
        .ok_or("Clippy tool missing")?
        .clone();
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    let selections = [
        json!({}),
        json!({"lint_profile":"strict"}),
        json!({"lint_profile":"project"}),
        json!({"lint_profile":"pedantic"}),
        json!({"package":"app","features":["extra"],"all_targets":true}),
        json!({"workspace":true,"features":["app/extra"],"all_targets":true}),
    ];
    let mut outputs = Vec::new();
    for (case, mut selection) in selections.into_iter().enumerate() {
        if case == 4 {
            // Explicit feature activation is necessary, and only --all-targets
            // exposes the test-only lint. The independent solo member is linted
            // only in the following --workspace case.
            let selected_source = format!(
                "#[cfg(not(feature = \"extra\"))]\ncompile_error!(\"feature extra required\");\n{source}\n#[cfg(test)]\nmod tests {{\n    #[test]\n    fn target_warning() {{\n        let values = vec![1, 2, 3];\n        assert_eq!(values.len(), 3);\n    }}\n}}\n"
            );
            fs::write(fixture.project.join("app/src/lib.rs"), &selected_source)?;
            expected.insert("app/src/lib.rs".into(), selected_source.into_bytes());
        }
        selection["project_ref"] = opened["project_ref"].clone();
        assert!(jsonschema::validator_for(&tool["inputSchema"])?.is_valid(&selection));
        let id = 10 + i64::try_from(case)? * 2;
        server.send(call(id, "rust.clippy", selection))?;
        let status = if case == 1 { "failed" } else { "passed" };
        let output = checked_output(&server.receive(id, JOIN_TIMEOUT)?, &tool, &opened, status)?;
        let severity = if case == 1 { "error" } else { "warning" };
        assert_clippy_finding(
            &output,
            "clippy::needless_return",
            severity,
            "app/src/lib.rs",
        )?;
        let diagnostics = output["diagnostics"].as_array().ok_or("diagnostics")?;
        assert_eq!(
            diagnostics
                .iter()
                .any(|d| d["code"] == "clippy::cast_possible_truncation"),
            case == 3,
            "pedantic must be an explicit opt-in: {output}"
        );
        assert!(
            diagnostics.iter().any(|d| {
                d["suggestions"].as_array().is_some_and(|suggestions| {
                    suggestions.iter().any(|suggestion| {
                        suggestion["applicability"] == "machine_applicable"
                            && suggestion["edits"].as_array().is_some_and(|edits| {
                                !edits.is_empty()
                                    && edits.iter().all(|edit| {
                                        edit["span"]["file"] == "app/src/lib.rs"
                                            && edit["replacement"].is_string()
                                            && edit["span"]["bytes"]["end"].as_u64().is_some_and(
                                                |end| {
                                                    end <= expected["app/src/lib.rs"].len() as u64
                                                },
                                            )
                                    })
                            })
                    })
                })
            }),
            "grouped machine-applicable source suggestions missing: {output}"
        );
        if case >= 4 {
            assert_eq!(output["data"]["options"]["all_targets"], true);
            assert_clippy_finding(&output, "clippy::useless_vec", "warning", "app/src/lib.rs")?;
            assert_eq!(
                diagnostics
                    .iter()
                    .any(|d| d["spans"].as_array().is_some_and(|spans| {
                        spans.iter().any(|span| span["file"] == "solo/src/lib.rs")
                    })),
                case == 5,
                "workspace selection must determine independent member linting"
            );
        }
        if case == 4 {
            assert_eq!(output["data"]["options"]["package"], "app");
            assert_eq!(output["data"]["options"]["features"], json!(["extra"]));
        }
        if case == 5 {
            assert_eq!(output["data"]["options"]["workspace"], true);
            assert_eq!(output["data"]["options"]["features"], json!(["app/extra"]));
            assert_clippy_finding(
                &output,
                "clippy::needless_return",
                "warning",
                "solo/src/lib.rs",
            )?;
        }
        verify_log_resource(
            &mut server,
            id + 1,
            &output,
            (case == 1).then_some("clippy::needless_return"),
        )?;
        assert_clippy_fixture_tree(&fixture, &expected)?;
        fixture.assert_clean(None)?;
        outputs.push(output);
    }
    assert_eq!(outputs[0]["diagnostics"], outputs[2]["diagnostics"]);
    assert_eq!(outputs[0]["data"]["options"]["lint_profile"], "default");
    assert_eq!(outputs[2]["data"]["options"]["lint_profile"], "project");
    assert_ne!(
        outputs[0]["data"]["runtime"]["execution_fingerprint"],
        outputs[1]["data"]["runtime"]["execution_fingerprint"]
    );
    server.finish()?;
    fixture.assert_clean(None)?;
    assert_clippy_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_CLIPPY_RECEIPT {}",
        serde_json::to_string(&json!({
            "status":"passed", "cases":6, "logs_verified":6,
            "default_warning_passed":true, "strict_warning_failed":true,
            "project_alias_verified":true, "pedantic_opt_in":true,
            "package_features_all_targets_verified":true, "workspace_verified":true,
            "structured_findings_suggestions":true, "source_unchanged":true,
            "cleanup":true,
            "configuration_fingerprint":outputs[0]["data"]["runtime"]["configuration_fingerprint"],
            "execution_fingerprints":outputs.iter().map(|value| &value["data"]["runtime"]["execution_fingerprint"]).collect::<Vec<_>>()
        }))?
    );
    fixture.successful = true;
    Ok(())
}

fn assert_clippy_fixture_tree(fixture: &Fixture, expected: &BTreeMap<String, Vec<u8>>) -> Result {
    let mut actual = BTreeMap::new();
    let mut directories = std::collections::BTreeSet::new();
    let mut pending = vec![PathBuf::new()];
    while let Some(relative) = pending.pop() {
        for entry in fs::read_dir(fixture.project.join(&relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            let name = path.to_str().ok_or("non-UTF8 fixture entry")?.to_owned();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                directories.insert(name);
                pending.push(path);
            } else if kind.is_file() {
                actual.insert(name, fs::read(entry.path())?);
            } else {
                return Err("unexpected non-regular fixture entry".into());
            }
        }
    }
    assert_eq!(&actual, expected, "unexpected host source mutation");
    assert_eq!(
        directories,
        ["app", "app/src", "helper", "helper/src", "solo", "solo/src"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    Ok(())
}

// Test stdout consists of Cargo records followed by arbitrary stable/custom
// harness text. Verify exact retained bytes without treating that tail as JSON.
fn verified_test_log(server: &mut Server, id: i64, output: &Value) -> Result<String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sha2::{Digest, Sha256};
    let log = &output["data"]["log"];
    let uri = log["uri"].as_str().ok_or("test log URI")?;
    assert!(uri.starts_with(&format!(
        "rust-artifact://{}/",
        output["data"]["project_ref"].as_str().ok_or("owner")?
    )));
    server.send(resource_read_request(id, uri))?;
    let response = server.receive(id, DISCOVERY_TIMEOUT)?;
    assert!(response.get("error").is_none(), "{response}");
    let result = &response["result"];
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["cacheScope"], "private");
    assert_eq!(result["ttlMs"], 0);
    assert_eq!(result["contents"].as_array().map(Vec::len), Some(1));
    let content = &result["contents"][0];
    assert_eq!(content["uri"], uri);
    assert_eq!(content["mimeType"], "application/octet-stream");
    let bytes = STANDARD.decode(content["blob"].as_str().ok_or("test log blob")?)?;
    let mut hash = String::with_capacity(64);
    for byte in Sha256::digest(&bytes) {
        use std::fmt::Write as _;
        write!(&mut hash, "{byte:02x}")?;
    }
    assert_eq!(log["sha256"], hash);
    assert_eq!(content["_meta"]["sha256"], hash);
    assert_eq!(log["size_bytes"], bytes.len());
    assert_eq!(content["_meta"]["size_bytes"], bytes.len());
    assert_eq!(log["truncated"], false);
    assert_eq!(content["_meta"]["truncated"], false);
    let retention = content["_meta"]["retention_remaining_seconds"]
        .as_u64()
        .ok_or("retention")?;
    assert!(
        retention > 0
            && retention
                <= log["retention_remaining_seconds"]
                    .as_u64()
                    .ok_or("initial retention")?
    );
    let raw = String::from_utf8(bytes)?;
    assert!(raw.starts_with("=== stdout ===\n") && raw.contains("\n=== stderr ===\n"));
    Ok(raw)
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn test_reports_results_selections_and_verified_harness_logs() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let manifest = fs::read_to_string(fixture.project.join("app/Cargo.toml"))?
        .replace("default = [\"extra\"]", "default = []");
    fs::write(fixture.project.join("app/Cargo.toml"), manifest)?;
    fs::write(
        fixture.project.join("helper/src/lib.rs"),
        "#[test] fn selected_helper() { use std::io::Write; std::io::stdout().write_all(b\"HELPER_PACKAGE_TEST_RAN\\n\").unwrap(); }\n",
    )?;
    let mut expected = fixture.source_bytes()?;
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("tools")?
        .iter()
        .find(|t| t["name"] == "rust.test")
        .ok_or("test tool")?
        .clone();
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    let pass = "#[test] fn selected_pass() { use std::io::Write; std::io::stdout().write_all(b\"RAW_TEST_TAIL {not-json}\\n\").unwrap(); assert_eq!(std::env::consts::ARCH, \"aarch64\"); }\n";
    let mixed = format!(
        "{pass}#[test] fn excluded_failure() {{ panic!(\"ACTUAL_ASSERTION_FAILURE\"); }}\n"
    );
    let feature = format!(
        "{pass}#[test] fn requires_extra() {{ assert!(cfg!(feature = \"extra\"), \"EXTRA_FEATURE_REQUIRED\"); }}\n"
    );
    let slow = "#[test] fn actual_slow_test() { use std::io::Write; std::io::stdout().write_all(b\"ACTUAL_TEST_TIMEOUT_STARTED\\n\").unwrap(); std::thread::sleep(std::time::Duration::from_secs(60)); }\n";
    let cases = [
        (
            pass,
            json!({}),
            "passed",
            Some(true),
            "RAW_TEST_TAIL {not-json}",
        ),
        (
            mixed.as_str(),
            json!({"package":"app","test_filter":"selected_pass"}),
            "passed",
            Some(true),
            "RAW_TEST_TAIL {not-json}",
        ),
        (
            mixed.as_str(),
            json!({}),
            "failed",
            Some(true),
            "ACTUAL_ASSERTION_FAILURE",
        ),
        (
            feature.as_str(),
            json!({}),
            "failed",
            Some(true),
            "EXTRA_FEATURE_REQUIRED",
        ),
        (
            feature.as_str(),
            json!({"package":"app","features":["extra"],"target":"aarch64-unknown-linux-gnu","timeout":60}),
            "passed",
            Some(true),
            "RAW_TEST_TAIL {not-json}",
        ),
        (
            feature.as_str(),
            json!({"all_features":true}),
            "passed",
            Some(true),
            "RAW_TEST_TAIL {not-json}",
        ),
        (
            "#[test] fn fails_to_compile() { let _: u8 = \"wrong\"; }\n",
            json!({}),
            "failed",
            Some(false),
            "E0308",
        ),
        (
            mixed.as_str(),
            json!({"package":"helper"}),
            "passed",
            Some(true),
            "HELPER_PACKAGE_TEST_RAN",
        ),
        (
            slow,
            json!({"timeout":15}),
            "blocked",
            Some(true),
            "ACTUAL_TEST_TIMEOUT_STARTED",
        ),
    ];
    let mut outputs = Vec::new();
    for (case, (source, mut selection, status, build, marker)) in cases.into_iter().enumerate() {
        fs::write(fixture.project.join("app/src/lib.rs"), source)?;
        expected.insert("app/src/lib.rs".into(), source.as_bytes().to_vec());
        selection["project_ref"] = opened["project_ref"].clone();
        jsonschema::validator_for(&tool["inputSchema"])?
            .validate(&selection)
            .map_err(|e| e.to_string())?;
        let id = 10 + i64::try_from(case)? * 2;
        server.send(call(id, "rust.test", selection.clone()))?;
        let response = server.receive(id, JOIN_TIMEOUT)?;
        let output = if status == "blocked" {
            assert!(response.get("error").is_none(), "{response}");
            assert_eq!(response["result"]["isError"], true, "{response}");
            let value = response["result"]["structuredContent"].clone();
            jsonschema::validator_for(&tool["outputSchema"])?
                .validate(&value)
                .map_err(|e| e.to_string())?;
            assert_eq!(value["status"], "blocked", "{value}");
            assert_eq!(value["error_code"], "COMMAND_TIMEOUT");
            assert_eq!(value["data"]["termination"], "timed_out");
            value
        } else {
            checked_output(&response, &tool, &opened, status)?
        };
        assert_eq!(output["data"]["build_succeeded"], json!(build), "{output}");
        assert_eq!(
            output["data"]["options"]["timeout"],
            selection.get("timeout").cloned().unwrap_or(json!(30))
        );
        for key in [
            "package",
            "features",
            "all_features",
            "test_filter",
            "target",
        ] {
            if let Some(selected) = selection.get(key) {
                assert_eq!(&output["data"]["options"][key], selected);
            }
        }
        let log = verified_test_log(&mut server, id + 1, &output)?;
        assert!(
            log.contains(marker),
            "actual harness/compilation marker missing: {log}"
        );
        if case == 1 || case == 7 {
            assert!(!log.contains("ACTUAL_ASSERTION_FAILURE"));
        }
        assert_eq!(log.contains("HELPER_PACKAGE_TEST_RAN"), case == 7);
        if case == 6 {
            assert_diagnostic_source(&output, "E0308", source.as_bytes(), None)?;
        }
        fixture.assert_clean(None)?;
        assert_fixture_tree(&fixture, &expected)?;
        outputs.push(output);
    }
    assert_ne!(
        outputs[0]["data"]["runtime"]["execution_fingerprint"],
        outputs[1]["data"]["runtime"]["execution_fingerprint"]
    );
    server.finish()?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_TEST_RECEIPT {}",
        json!({"cases":9,"logs_sha256_verified":9,"raw_harness_tail_retained":true,"passed_failed_compile_failed_distinguished":true,"package_filter_features_all_features_target_timeout_verified":true,"timeout_partial_build_evidence":true,"source_unchanged_between_explicit_mutations":true,"cleanup":true,"execution_fingerprints":outputs.iter().map(|o| &o["data"]["runtime"]["execution_fingerprint"]).collect::<Vec<_>>() })
    );
    fixture.successful = true;
    Ok(())
}

fn active_slow_test(server: &mut Server, fixture: &Fixture, request_id: i64) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        server.assert_no_response(request_id)?;
        // Unlike calibration_job, select RUNNING containers and verify an actual
        // test-binary process. The session is already calibrated, so this exact
        // default Cargo command belongs to the newly admitted project test.
        let mut list = runtime_observer(fixture);
        list.args([
            "container",
            "ls",
            "--no-trunc",
            "--filter",
            "label=org.rust-mcp.execution=true",
            "--filter",
            "label=org.rust-mcp.rust-job",
            "--filter",
            "status=running",
            "--format",
            "{{.Names}}\t{{.Command}}",
        ]);
        let records = String::from_utf8(bounded_command(list)?)?;
        for record in records.lines() {
            let Some((name, command)) = record.split_once('\t') else {
                return Err("missing active Cargo command observation".into());
            };
            let Some(nonce) = name.strip_prefix("rust-mcp-cargo-") else {
                continue;
            };
            if nonce.len() != 32
                || !nonce
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || command.trim_matches('"')
                    != "/opt/rust/bin/cargo test --frozen --message-format=json --jobs=1 --color=never -- --test-threads=1 --color=never"
            {
                continue;
            }
            let mut top = runtime_observer(fixture);
            top.args(["container", "top", name, "-eo", "pid,args"]);
            let processes = String::from_utf8(bounded_command(top)?)?;
            if processes.lines().skip(1).any(|line| {
                line.split_whitespace().any(|part| {
                    part.starts_with("/work/target/debug/deps/app-")
                        && line.split_whitespace().any(|arg| arg == "--test-threads=1")
                })
            }) {
                server.assert_no_response(request_id)?;
                return Ok(nonce.to_owned());
            }
        }
        if Instant::now() >= deadline {
            return Err(
                "sleeping actual test binary was not observed before the test deadline margin"
                    .into(),
            );
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn test_cancellation_and_eof_join_active_test_binaries() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let mut expected = fixture.source_bytes()?;
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("tools list")?
        .iter()
        .find(|tool| tool["name"] == "rust.test")
        .ok_or("rust.test missing")?
        .clone();
    let arguments = json!({"project_ref":opened["project_ref"]});
    // Complete one ordinary test first: all following containers are project
    // jobs, not lazy calibration fixtures. The same inspector/worker is reused.
    server.send(call(4, "rust.test", arguments.clone()))?;
    let initial = checked_output(&server.receive(4, JOIN_TIMEOUT)?, &tool, &opened, "passed")?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    // Only the test body changes; the manifest/ProjectRef identity is unchanged.
    // This benign sleep is compiled and executed ONLY by the contained gateway;
    // the harness never invokes Cargo/rustc against the host fixture tree.
    let build = b"#[test] fn active_test_binary() { std::thread::sleep(std::time::Duration::from_secs(60)); }\n";
    fs::write(fixture.project.join("app/src/lib.rs"), build)?;
    expected.insert("app/src/lib.rs".into(), build.to_vec());

    let cancel_started = Instant::now();
    server.send(call(10, "rust.test", arguments.clone()))?;
    let cancelled_job = active_slow_test(&mut server, &fixture, 10)?;
    server.send(request(11, "tools/list"))?;
    assert!(
        server.receive(11, DISCOVERY_TIMEOUT)?["result"]["tools"]
            .as_array()
            .is_some()
    );
    server.send(resource_read_request(
        15,
        initial["data"]["log"]["uri"]
            .as_str()
            .ok_or("initial log")?,
    ))?;
    let busy = server.receive(15, DISCOVERY_TIMEOUT)?;
    assert_eq!(busy["error"]["code"], -32000);
    assert_eq!(
        busy["error"]["message"],
        "Artifact worker is busy; retry after the active operation"
    );
    server.assert_no_response(10)?;
    // The gateway's operation timeout is 30s. Sending before 20s distinguishes
    // explicit cancellation from a test that merely waits out that timeout.
    assert!(
        cancel_started.elapsed() < Duration::from_secs(20),
        "missed active cancellation window"
    );
    server.send(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10}}),
    )?;
    let cleanup_deadline = Instant::now() + JOIN_TIMEOUT;
    loop {
        let containers = fixture.objects("container", Some(&cancelled_job))?;
        let volumes = fixture.objects("volume", Some(&cancelled_job))?;
        server.assert_no_response(10)?;
        if containers.is_empty() && volumes.is_empty() {
            break;
        }
        if Instant::now() >= cleanup_deadline {
            return Err("cancelled active Cargo job retained owned objects".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
    fixture.assert_clean(None)?;
    server.send(request(12, "tools/list"))?;
    assert!(
        server.receive(12, DISCOVERY_TIMEOUT)?["result"]["tools"]
            .as_array()
            .is_some()
    );
    server.assert_no_response(10)?;
    assert_fixture_tree(&fixture, &expected)?;

    // Re-admission occurs only after observed cleanup. Seeing a second sleeping
    // test binary proves the original joined worker released its permit and the
    // calibrated inspector remained usable after clean cancellation.
    let eof_started = Instant::now();
    server.send(call(20, "rust.test", arguments))?;
    let eof_job = active_slow_test(&mut server, &fixture, 20)?;
    assert_ne!(eof_job, cancelled_job);
    server.send(request(21, "tools/list"))?;
    assert!(
        server.receive(21, DISCOVERY_TIMEOUT)?["result"]["tools"]
            .as_array()
            .is_some()
    );
    server.assert_no_response(10)?;
    server.assert_no_response(20)?;
    assert!(
        eof_started.elapsed() < Duration::from_secs(20),
        "missed active EOF window"
    );
    // finish closes stdin, then waits for process exit AND output pipe closure;
    // its deadline includes joined worker shutdown and verified gateway cleanup.
    let exit = server.finish();
    let clean = fixture
        .assert_clean(Some(&eof_job))
        .and_then(|()| fixture.assert_clean(None));
    exit?;
    clean?;
    server.assert_no_response(10)?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_TEST_CANCELLATION_RECEIPT {}",
        serde_json::to_string(&json!({
            "calibrated_by_successful_test":true,
            "active_test_binaries_observed":2,
            "discovery_responsive_during_tests":true,
            "cancelled_request_response_suppressed":true,
            "worker_reused_after_observed_cleanup":true,
            "eof_joined_shutdown":true,
            "only_expected_source_mutations":true,
            "cleanup":true
        }))?
    );
    fixture.successful = true;
    Ok(())
}

#[path = "inspection_runtime/audit.rs"]
mod audit_runtime;

#[path = "inspection_runtime/explain.rs"]
mod explain_runtime;

#[path = "inspection_runtime/quality.rs"]
mod quality_runtime;
