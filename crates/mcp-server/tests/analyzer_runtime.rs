//! Native evidence for `rust.analyzer.symbols` against the real M6 guest
//! image: one ignored test, not a suite (pattern: `tests/inspection_runtime.rs`).
#![cfg(target_os = "macos")]

use rust_engineering_execution::APPROVED_M6_IMAGE;
use serde_json::{Value, json};
use std::error::Error;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
// The analyzer's own total-call ceiling is 180s (ADR-084 §8); this leaves
// margin for calibration, capture and cleanup around a single call.
const CALL_TIMEOUT: Duration = Duration::from_secs(220);
const PIPE_LIMIT: usize = 2 * 1024 * 1024;

fn fixture_root() -> Result<PathBuf> {
    fixture_root_named("valid-basic")
}

fn fixture_root_named(name: &str) -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
        .canonicalize()?)
}

// The protocol version every other native runtime test negotiates
// (`tests/inspection_runtime.rs`): embedded in every request's `_meta`
// rather than a separate `initialize` round trip, matching that file's own
// handshake convention exactly.
const VERSION: &str = "2026-07-28";

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

fn bounded_reader(mut reader: impl io::Read + Send + 'static) -> Receiver<io::Result<Vec<u8>>> {
    let (tx, rx) = mpsc::sync_channel(4);
    thread::spawn(move || {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => {
                    let _ = tx.send(Ok(std::mem::take(&mut buffer)));
                    break;
                }
                Ok(count) => {
                    buffer.extend_from_slice(&chunk[..count]);
                    if buffer.len() > PIPE_LIMIT {
                        let _ = tx.send(Err(io::Error::other("stderr budget exceeded")));
                        break;
                    }
                }
                Err(error) => {
                    let _ = tx.send(Err(error));
                    break;
                }
            }
        }
    });
    rx
}

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Receiver<io::Result<Vec<u8>>>,
    stderr: Receiver<io::Result<Vec<u8>>>,
}
impl Server {
    fn start(image: &str, root: &Path) -> Result<Self> {
        Self::start_with(image, root, &[])
    }
    /// [`Self::start`] plus host flags such as a write grant.
    fn start_with(image: &str, root: &Path, extra: &[&std::ffi::OsStr]) -> Result<Self> {
        let docker = std::env::var("RUST_MCP_TEST_DOCKER").unwrap_or_else(|_| {
            "/Applications/Docker.app/Contents/Resources/bin/docker".to_owned()
        });
        let socket = std::env::var("RUST_MCP_TEST_SOCKET")?;
        if !socket.starts_with('/') || socket.chars().any(char::is_control) {
            return Err("RUST_MCP_TEST_SOCKET must be an explicit absolute socket path".into());
        }
        let state = std::env::temp_dir().canonicalize()?.join(format!(
            "rust-mcp-analyzer-wire-state-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&state)?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"));
        command
            .env_clear()
            .args(["serve", "--stdio", "--root"])
            .arg(root)
            .arg("--docker")
            .arg(&docker)
            .arg("--docker-socket")
            .arg(&socket)
            .arg("--state-root")
            .arg(&state)
            .arg("--rust-image")
            .arg(image)
            .args(extra)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn()?;
        let stdin = child.stdin.take();
        let stdout_pipe = child.stdout.take().ok_or("missing server stdout")?;
        let stderr_pipe = child.stderr.take().ok_or("missing server stderr")?;
        let (tx, stdout) = mpsc::sync_channel(32);
        thread::spawn(move || {
            let mut reader = BufReader::new(stdout_pipe);
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = tx.send(Err(error));
                        break;
                    }
                }
            }
        });
        let stderr = bounded_reader(stderr_pipe);
        Ok(Self {
            child,
            stdin,
            stdout,
            stderr,
        })
    }
    fn send(&mut self, value: Value) -> Result {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.stdin
            .as_mut()
            .ok_or("stdin closed")?
            .write_all(&bytes)?;
        self.stdin.as_mut().ok_or("stdin closed")?.flush()?;
        Ok(())
    }
    fn response(&mut self, id: Value, timeout: Duration) -> Result<Value> {
        let started = Instant::now();
        let deadline = started + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(self.stall_error(&id, started.elapsed(), false));
            }
            let frame = match self.stdout.recv_timeout(remaining) {
                Ok(frame) => frame?,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(self.stall_error(&id, started.elapsed(), false));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(self.stall_error(&id, started.elapsed(), true));
                }
            };
            let value: Value = serde_json::from_slice(&frame)?;
            if value["id"] == id {
                return Ok(value);
            }
        }
    }
    /// No response for `id` arrived within `waited`: names the stalled
    /// request, reports whether the child is still alive (`try_wait`) and
    /// drains its bounded stderr, instead of a bare `Timeout`/`Disconnected`.
    fn stall_error(&mut self, id: &Value, waited: Duration, disconnected: bool) -> Box<dyn Error> {
        let alive = match self.child.try_wait() {
            Ok(None) => "still running".to_owned(),
            Ok(Some(status)) => format!("exited {status}"),
            Err(error) => format!("could not be joined: {error}"),
        };
        let stderr = self
            .stderr
            .recv_timeout(DISCOVERY_TIMEOUT)
            .ok()
            .and_then(|result| result.ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        let cause = if disconnected {
            "the server closed stdout"
        } else {
            "timed out"
        };
        format!(
            "no response for request id {id}: {cause} after {waited:?}; child {alive}; \
             stderr {stderr:?}"
        )
        .into()
    }
    fn finish(&mut self) -> Result {
        self.stdin.take();
        let deadline = Instant::now() + CALL_TIMEOUT;
        loop {
            // The reader thread keeps handing off any further frames on this
            // bounded channel; draining them here stops it from blocking on a
            // full channel while this loop only polls `try_wait`.
            while self.stdout.try_recv().is_ok() {}
            if let Some(status) = self.child.try_wait()? {
                if !status.success() {
                    let stderr = self.stderr.recv_timeout(DISCOVERY_TIMEOUT)??;
                    return Err(format!(
                        "server exited {status}: {}",
                        String::from_utf8_lossy(&stderr)
                    )
                    .into());
                }
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("server did not join before deadline".into());
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stdin.take();
        let _ = self.child.wait();
    }
}

/// Native end-to-end evidence: real server, real `--rust` runtime pointed at
/// the admitted M6 image and socket, real `fixtures/valid-basic`. Not part of
/// `cargo test`'s default run; the orchestrator's suite runs it explicitly.
#[test]
#[ignore]
fn analyzer_symbols_document_and_workspace_scope_answer_on_the_real_m6_image() -> Result {
    let image =
        std::env::var("RUST_MCP_TEST_IMAGE").unwrap_or_else(|_| APPROVED_M6_IMAGE.to_owned());
    if image != APPROVED_M6_IMAGE {
        return Err(format!(
            "RUST_MCP_TEST_IMAGE={image} is not the admitted M6 runtime {APPROVED_M6_IMAGE}"
        )
        .into());
    }
    let root = fixture_root()?;
    let mut server = Server::start(&image, &root)?;
    server.send(request(1, "tools/list"))?;
    server.response(json!(1), DISCOVERY_TIMEOUT)?;
    server.send(call(2, "rust.project.open", json!({"path": root})))?;
    let opened = server.response(json!(2), DISCOVERY_TIMEOUT)?;
    assert_eq!(
        opened["result"]["structuredContent"]["status"], "passed",
        "{opened}"
    );
    let project_ref = opened["result"]["structuredContent"]["data"]["project_ref"].clone();

    server.send(call(
        3,
        "rust.analyzer.symbols",
        json!({
            "project_ref": project_ref,
            "scope": "document",
            "file": "src/lib.rs"
        }),
    ))?;
    let document = server.response(json!(3), CALL_TIMEOUT)?;
    let data = &document["result"]["structuredContent"];
    assert_eq!(data["status"], "passed", "{document}");
    assert_eq!(data["data"]["readiness"]["state"], "quiescent");
    assert_eq!(
        data["data"]["analyzer"]["version"],
        "rust-analyzer 1.98.1 (48a229c 2026-09-01)"
    );
    assert_eq!(data["data"]["analyzer"]["position_encoding"], "utf-8");
    assert_eq!(data["data"]["completeness"]["state"], "complete");
    let symbols = data["data"]["symbols"]
        .as_array()
        .ok_or("missing symbols array")?;
    assert!(!symbols.is_empty(), "{document}");
    let source = std::fs::read_to_string(root.join("src/lib.rs"))?;
    let lines: Vec<&str> = source.split('\n').collect();
    let sliced = |range: &Value| -> Result<String> {
        let start_line = range["start"]["line"].as_u64().ok_or("missing line")? as usize - 1;
        let start_col = range["start"]["column"].as_u64().ok_or("missing column")? as usize - 1;
        let end_col = range["end"]["column"].as_u64().ok_or("missing column")? as usize - 1;
        let line = lines.get(start_line).ok_or("line out of range")?;
        let chars: Vec<char> = line.chars().collect();
        Ok(chars
            .get(start_col..end_col)
            .ok_or("column out of range")?
            .iter()
            .collect())
    };
    let mut sliced_own_name = false;
    for symbol in symbols {
        let name = symbol["name"].as_str().ok_or("missing name")?;
        if sliced(&symbol["selection_range"])? == name {
            sliced_own_name = true;
            break;
        }
    }
    assert!(
        sliced_own_name,
        "at least one symbol's selection_range must slice its own name: {document}"
    );

    server.send(call(
        4,
        "rust.analyzer.symbols",
        json!({
            "project_ref": project_ref,
            "scope": "workspace",
            "query": "add"
        }),
    ))?;
    let workspace = server.response(json!(4), CALL_TIMEOUT)?;
    let workspace_data = &workspace["result"]["structuredContent"];
    assert_eq!(workspace_data["status"], "passed", "{workspace}");
    let workspace_symbols = workspace_data["data"]["symbols"]
        .as_array()
        .ok_or("missing workspace symbols array")?;
    assert!(
        workspace_symbols.iter().any(|symbol| symbol["name"]
            .as_str()
            .is_some_and(|name| name.contains("add"))),
        "{workspace}"
    );

    server.finish()
}

/// Native end-to-end evidence for `rust.analyzer.references` (M6-02): real
/// server, real `--rust` runtime, `fixtures/analyzer-references`. Unlike
/// `valid-basic` (whose only use of `add` is inside a `#[test] fn`, which
/// rust-analyzer cfg-excludes under the M6 minimal config), this fixture's
/// `twice` calls `add` outside any test cfg, so the answer distinguishes a
/// declaration from a use. Not part of `cargo test`'s default run; the
/// orchestrator's suite runs it explicitly.
#[test]
#[ignore]
fn analyzer_references_flags_the_declaration_on_the_real_m6_image() -> Result {
    let image =
        std::env::var("RUST_MCP_TEST_IMAGE").unwrap_or_else(|_| APPROVED_M6_IMAGE.to_owned());
    if image != APPROVED_M6_IMAGE {
        return Err(format!(
            "RUST_MCP_TEST_IMAGE={image} is not the admitted M6 runtime {APPROVED_M6_IMAGE}"
        )
        .into());
    }
    let root = fixture_root_named("analyzer-references")?;
    let mut server = Server::start(&image, &root)?;
    server.send(request(1, "tools/list"))?;
    server.response(json!(1), DISCOVERY_TIMEOUT)?;
    server.send(call(2, "rust.project.open", json!({"path": root})))?;
    let opened = server.response(json!(2), DISCOVERY_TIMEOUT)?;
    assert_eq!(
        opened["result"]["structuredContent"]["status"], "passed",
        "{opened}"
    );
    let project_ref = opened["result"]["structuredContent"]["data"]["project_ref"].clone();

    // "pub fn add(a: u32, b: u32) -> u32 { a + b }" on line 1: `add` starts at
    // column 8. Line 2's "pub fn twice(x: u32) -> u32 { add(x, x) }" calls
    // `add` at column 31.
    server.send(call(
        3,
        "rust.analyzer.references",
        json!({
            "project_ref": project_ref,
            "file": "src/lib.rs",
            "position": {"line": 1, "column": 8},
            "include_declaration": true
        }),
    ))?;
    let response = server.response(json!(3), CALL_TIMEOUT)?;
    let data = &response["result"]["structuredContent"];
    assert_eq!(data["status"], "passed", "{response}");
    assert_eq!(data["data"]["completeness"]["state"], "complete");
    let references = data["data"]["references"]
        .as_array()
        .ok_or("missing references array")?;
    assert_eq!(
        references.len(),
        2,
        "analyzer-references' `add` has exactly the declaration and the `twice` call site: {response}"
    );
    let declarations = references
        .iter()
        .filter(|reference| reference["is_declaration"] == json!(true))
        .count();
    assert_eq!(
        declarations, 1,
        "exactly one location is flagged as the declaration: {response}"
    );
    assert_eq!(data["data"]["omitted_declarations"], json!(0));

    server.send(call(
        4,
        "rust.analyzer.references",
        json!({
            "project_ref": project_ref,
            "file": "src/lib.rs",
            "position": {"line": 1, "column": 8},
            "include_declaration": false
        }),
    ))?;
    let response = server.response(json!(4), CALL_TIMEOUT)?;
    let data = &response["result"]["structuredContent"];
    assert_eq!(data["status"], "passed", "{response}");
    assert_eq!(data["data"]["completeness"]["state"], "complete");
    let references = data["data"]["references"]
        .as_array()
        .ok_or("missing references array")?;
    assert_eq!(
        references.len(),
        1,
        "excluding the declaration leaves only the `twice` use: {response}"
    );
    assert!(
        references
            .iter()
            .all(|reference| reference["is_declaration"] == json!(false)),
        "no remaining reference may be flagged a declaration: {response}"
    );
    assert_eq!(data["data"]["omitted_declarations"], json!(1));

    server.finish()
}

/// Native end-to-end evidence for `rust.analyzer.diagnostics` and
/// `rust.analyzer.symbols` (M6-03): real server, real `--rust` runtime,
/// `fixtures/build-script`. Owner decision (Option A, 2026-09-12): under the
/// M6 minimal config (`diagnostics.experimental.enable=false`), diagnostics
/// surfaces only syntax-level diagnostics, so it is proven answered/complete
/// here but is not the containment proof. The in-band build-script oracle
/// (`01.md` R1) is that `rust.analyzer.symbols` (document scope) never
/// expands `include!(concat!(env!("OUT_DIR"), ...))`: the generated
/// `GENERATED` constant is absent while the fixture's own `generated_fact`
/// test is present. Not part of `cargo test`'s default run; the
/// orchestrator's suite runs it explicitly.
#[test]
#[ignore]
fn analyzer_diagnostics_and_symbols_on_build_script_prove_no_build_script_ran_on_the_real_m6_image()
-> Result {
    let image =
        std::env::var("RUST_MCP_TEST_IMAGE").unwrap_or_else(|_| APPROVED_M6_IMAGE.to_owned());
    if image != APPROVED_M6_IMAGE {
        return Err(format!(
            "RUST_MCP_TEST_IMAGE={image} is not the admitted M6 runtime {APPROVED_M6_IMAGE}"
        )
        .into());
    }
    let root = fixture_root_named("build-script")?;
    let mut server = Server::start(&image, &root)?;
    server.send(request(1, "tools/list"))?;
    server.response(json!(1), DISCOVERY_TIMEOUT)?;
    server.send(call(2, "rust.project.open", json!({"path": root})))?;
    let opened = server.response(json!(2), DISCOVERY_TIMEOUT)?;
    assert_eq!(
        opened["result"]["structuredContent"]["status"], "passed",
        "{opened}"
    );
    let project_ref = opened["result"]["structuredContent"]["data"]["project_ref"].clone();

    server.send(call(
        3,
        "rust.analyzer.diagnostics",
        json!({
            "project_ref": project_ref,
            "file": "src/lib.rs"
        }),
    ))?;
    let response = server.response(json!(3), CALL_TIMEOUT)?;
    let data = &response["result"]["structuredContent"];
    assert_eq!(data["status"], "passed", "{response}");
    assert_eq!(data["data"]["readiness"]["state"], "quiescent");
    assert_eq!(data["data"]["readiness"]["health"], "ok");
    assert_eq!(
        data["data"]["completeness"]["state"], "complete",
        "an answered, unwarned, unlimited call is exhaustive: {response}"
    );
    data["data"]["diagnostics"]
        .as_array()
        .ok_or("missing diagnostics array")?;

    server.send(call(
        4,
        "rust.analyzer.symbols",
        json!({
            "project_ref": project_ref,
            "scope": "document",
            "file": "src/lib.rs"
        }),
    ))?;
    let symbols_response = server.response(json!(4), CALL_TIMEOUT)?;
    let symbols_data = &symbols_response["result"]["structuredContent"];
    assert_eq!(symbols_data["status"], "passed", "{symbols_response}");
    assert_eq!(symbols_data["data"]["readiness"]["state"], "quiescent");
    assert_eq!(symbols_data["data"]["readiness"]["health"], "ok");
    assert_eq!(
        symbols_data["data"]["completeness"]["state"], "complete",
        "an answered, unwarned, unlimited call is exhaustive: {symbols_response}"
    );
    let symbols = symbols_data["data"]["symbols"]
        .as_array()
        .ok_or("missing symbols array")?;
    let names = symbols
        .iter()
        .filter_map(|symbol| symbol["name"].as_str())
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"generated_fact"),
        "the test function must be visible whether or not the include! expands: \
         {symbols_response}"
    );
    assert!(
        !names.contains(&"GENERATED"),
        "GENERATED is only defined by the generated file; its presence would mean a build \
         script ran: {symbols_response}"
    );

    server.finish()
}

// ---------------------------------------------------------------------
// M6-04/M6-05: `rust.analyzer.actions` and `rust.analyzer.action.apply`
// ---------------------------------------------------------------------

const APPLY: &str = "rust.analyzer.action.apply";

/// An empty selection on `sum` in `    let sum = values.iter().sum::<u32>();`
/// (line 2, column 9), where the m6-11 cut observed applicable assists.
fn cursor() -> Value {
    json!({"start": {"line": 2, "column": 9}, "end": {"line": 2, "column": 9}})
}

fn m6_image() -> Result<String> {
    let image =
        std::env::var("RUST_MCP_TEST_IMAGE").unwrap_or_else(|_| APPROVED_M6_IMAGE.to_owned());
    if image != APPROVED_M6_IMAGE {
        return Err(format!(
            "RUST_MCP_TEST_IMAGE={image} is not the admitted M6 runtime {APPROVED_M6_IMAGE}"
        )
        .into());
    }
    Ok(image)
}

/// A private, canonical copy of `fixtures/analyzer-actions`: the apply tests
/// write source, so they never run against the checked-in fixture.
struct ActionsProject(PathBuf);
impl ActionsProject {
    fn copy(tag: &str) -> Result<Self> {
        let fixture = fixture_root_named("analyzer-actions")?;
        let root = std::env::temp_dir().canonicalize()?.join(format!(
            "rust-mcp-analyzer-actions-{}-{tag}",
            std::process::id()
        ));
        if root.exists() {
            std::fs::remove_dir_all(&root)?;
        }
        std::fs::create_dir_all(root.join("src"))?;
        for file in ["Cargo.toml", "Cargo.lock", "src/lib.rs"] {
            std::fs::copy(fixture.join(file), root.join(file))?;
        }
        Ok(Self(root))
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn lib(&self) -> PathBuf {
        self.0.join("src/lib.rs")
    }
    fn start_with_grant(&self, image: &str) -> Result<Server> {
        Server::start_with(
            image,
            self.path(),
            &[
                std::ffi::OsStr::new("--allow-analyzer-action-write"),
                self.path().as_os_str(),
            ],
        )
    }
}
impl Drop for ActionsProject {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `tools/list`, then `rust.project.open`; the opened `data`.
fn discover_and_open(server: &mut Server, root: &Path) -> Result<Value> {
    server.send(request(1, "tools/list"))?;
    server.response(json!(1), DISCOVERY_TIMEOUT)?;
    server.send(call(2, "rust.project.open", json!({"path": root})))?;
    let opened = server.response(json!(2), DISCOVERY_TIMEOUT)?;
    assert_eq!(
        opened["result"]["structuredContent"]["status"], "passed",
        "{opened}"
    );
    Ok(opened["result"]["structuredContent"]["data"].clone())
}

fn actions_arguments(opened: &Value) -> Value {
    json!({
        "project_ref": opened["project_ref"],
        "expected_project_fingerprint": opened["fingerprint"],
        "file": "src/lib.rs",
        "range": cursor()
    })
}

/// The first applicable action `rust.analyzer.actions` lists at the cursor.
fn first_applicable(server: &mut Server, id: i64, opened: &Value) -> Result<Value> {
    server.send(call(id, "rust.analyzer.actions", actions_arguments(opened)))?;
    let response = server.response(json!(id), CALL_TIMEOUT)?;
    let data = &response["result"]["structuredContent"];
    assert_eq!(data["status"], "passed", "{response}");
    data["data"]["actions"]
        .as_array()
        .ok_or("missing actions array")?
        .iter()
        .find(|action| action["applicability"] == "applicable")
        .cloned()
        .ok_or_else(|| format!("no applicable action at the cursor: {response}").into())
}

fn preview_action(opened: &Value, action: &Value) -> Value {
    json!({
        "project_ref": opened["project_ref"],
        "action": {
            "mode": "preview",
            "expected_project_fingerprint": opened["fingerprint"],
            "action_digest": action["action_digest"],
            "file": "src/lib.rs",
            "range": cursor()
        }
    })
}

/// The complete post-edit text of `path` from the exact whole-file diff
/// `rust.analyzer.action.apply` preview publishes.
fn after_text(diff: &str, path: &str) -> Result<String> {
    let header = format!("--- a/{path}\n+++ b/{path}\n");
    let start = diff.find(&header).ok_or("file absent from the diff")? + header.len();
    let mut lines = diff[start..].split_inclusive('\n');
    if !lines.next().is_some_and(|hunk| hunk.starts_with("@@ ")) {
        return Err("hunk header absent".into());
    }
    let mut after = String::new();
    let mut last_was_added = false;
    for line in lines {
        if line.starts_with("--- a/") {
            break;
        }
        if let Some(text) = line.strip_prefix('+') {
            after.push_str(text);
            last_was_added = true;
        } else if line.starts_with("\\ No newline at end of file") {
            if last_was_added {
                after.pop();
            }
        } else {
            last_was_added = false;
        }
    }
    Ok(after)
}

/// M6-04 native evidence: the real binary lists at least one applicable action
/// with a digest and an edits summary over `fixtures/analyzer-actions`.
#[test]
#[ignore]
fn analyzer_actions_list_an_applicable_action_with_a_digest_on_the_real_m6_image() -> Result {
    let image = m6_image()?;
    let project = ActionsProject::copy("list")?;
    let before = std::fs::read(project.lib())?;
    let mut server = Server::start(&image, project.path())?;
    let opened = discover_and_open(&mut server, project.path())?;
    let action = first_applicable(&mut server, 3, &opened)?;
    let digest = action["action_digest"]
        .as_str()
        .ok_or("missing action_digest")?;
    assert!(
        digest.len() == 71
            && digest.starts_with("sha256:")
            && digest[7..].bytes().all(|b| b.is_ascii_hexdigit()),
        "{action}"
    );
    assert!(
        action["title"]
            .as_str()
            .is_some_and(|title| !title.is_empty())
    );
    assert!(
        action["edits_summary"]["files"].as_u64() >= Some(1),
        "{action}"
    );
    assert!(
        action["edits_summary"]["edits"].as_u64() >= Some(1),
        "{action}"
    );
    assert_eq!(
        std::fs::read(project.lib())?,
        before,
        "listing never writes"
    );
    server.finish()
}

/// M6-05 native evidence: preview returns the exact diff without writing,
/// commit publishes those bytes through the M2 writer, the receipt reflects
/// it, and the pre-commit project_ref no longer resolves.
#[test]
#[ignore]
fn analyzer_action_apply_commits_through_the_writer_on_the_real_m6_image() -> Result {
    let image = m6_image()?;
    let project = ActionsProject::copy("apply")?;
    let before = std::fs::read_to_string(project.lib())?;
    let mut server = project.start_with_grant(&image)?;
    let opened = discover_and_open(&mut server, project.path())?;
    let action = first_applicable(&mut server, 3, &opened)?;

    server.send(call(4, APPLY, preview_action(&opened, &action)))?;
    let response = server.response(json!(4), CALL_TIMEOUT)?;
    let preview = &response["result"]["structuredContent"];
    assert_eq!(preview["status"], "passed", "{response}");
    assert_eq!(preview["data"]["kind"], "preview");
    assert_eq!(
        preview["data"]["validation"]["method"],
        "workspace_edit_structural_only"
    );
    assert_eq!(
        preview["data"]["validation"]["action_digest"],
        action["action_digest"]
    );
    assert!(
        preview["guarantees_not_provided"]
            .as_array()
            .ok_or("missing guarantees")?
            .contains(&json!("compile_verification"))
    );
    let files = preview["data"]["files"].as_array().ok_or("missing files")?;
    assert!(
        files.iter().any(|file| file["path"] == "src/lib.rs"),
        "{response}"
    );
    let diff = preview["data"]["diff"].as_str().ok_or("missing diff")?;
    let expected = after_text(diff, "src/lib.rs")?;
    assert_ne!(expected, before, "{diff}");
    assert_eq!(
        std::fs::read_to_string(project.lib())?,
        before,
        "preview never writes source"
    );

    server.send(call(
        5,
        APPLY,
        json!({
            "project_ref": opened["project_ref"],
            "action": {
                "mode": "commit",
                "plan_id": preview["data"]["plan_id"],
                "plan_digest": preview["data"]["plan_digest"],
                "idempotency_key": "m6-action-apply"
            }
        }),
    ))?;
    let response = server.response(json!(5), CALL_TIMEOUT)?;
    let receipt = &response["result"]["structuredContent"];
    assert_eq!(receipt["status"], "passed", "{response}");
    assert_eq!(receipt["data"]["state"], "committed");
    assert_eq!(receipt["data"]["operation_id"], preview["data"]["plan_id"]);
    let changed = receipt["data"]["files"]
        .as_array()
        .ok_or("missing receipt files")?
        .iter()
        .find(|file| file["path"] == "src/lib.rs")
        .ok_or("src/lib.rs absent from the receipt")?;
    assert_eq!(
        changed["effect_after_sha256"],
        changed["intended_after_sha256"]
    );
    assert_eq!(std::fs::read_to_string(project.lib())?, expected);

    server.send(call(6, "rust.analyzer.actions", actions_arguments(&opened)))?;
    let response = server.response(json!(6), CALL_TIMEOUT)?;
    assert_eq!(
        response["result"]["structuredContent"]["error_code"], "PROJECT_NOT_FOUND",
        "the pre-commit project_ref is invalidated: {response}"
    );

    server.send(call(
        7,
        "rust.project.open",
        json!({"path": project.path()}),
    ))?;
    let reopened = server.response(json!(7), DISCOVERY_TIMEOUT)?;
    let reopened = &reopened["result"]["structuredContent"]["data"];
    server.send(call(
        8,
        APPLY,
        json!({
            "project_ref": reopened["project_ref"],
            "action": {
                "mode": "receipt",
                "operation_id": preview["data"]["plan_id"],
                "recover": false
            }
        }),
    ))?;
    let response = server.response(json!(8), CALL_TIMEOUT)?;
    let observed = &response["result"]["structuredContent"];
    assert_eq!(observed["status"], "passed", "{response}");
    assert_eq!(observed["data"]["state"], "committed");
    server.finish()
}

/// ADR-083 §2/§6 native evidence: a source change after preview makes the
/// commit `ACTION_STALE` without any write, and the same digest no longer
/// resolves over the changed capture.
#[test]
#[ignore]
fn analyzer_action_apply_is_action_stale_after_a_source_change_on_the_real_m6_image() -> Result {
    let image = m6_image()?;
    let project = ActionsProject::copy("stale")?;
    let mut server = project.start_with_grant(&image)?;
    let opened = discover_and_open(&mut server, project.path())?;
    let action = first_applicable(&mut server, 3, &opened)?;
    server.send(call(4, APPLY, preview_action(&opened, &action)))?;
    let response = server.response(json!(4), CALL_TIMEOUT)?;
    let preview = &response["result"]["structuredContent"];
    assert_eq!(preview["status"], "passed", "{response}");

    let edited = format!(
        "{}// edited after preview\n",
        std::fs::read_to_string(project.lib())?
    );
    std::fs::write(project.lib(), &edited)?;
    server.send(call(
        5,
        APPLY,
        json!({
            "project_ref": opened["project_ref"],
            "action": {
                "mode": "commit",
                "plan_id": preview["data"]["plan_id"],
                "plan_digest": preview["data"]["plan_digest"],
                "idempotency_key": "m6-action-stale"
            }
        }),
    ))?;
    let response = server.response(json!(5), CALL_TIMEOUT)?;
    let refused = &response["result"]["structuredContent"];
    assert_eq!(refused["status"], "blocked", "{response}");
    assert_eq!(refused["error_code"], "ACTION_STALE", "{response}");
    assert_eq!(std::fs::read_to_string(project.lib())?, edited);

    server.send(call(6, APPLY, preview_action(&opened, &action)))?;
    let response = server.response(json!(6), CALL_TIMEOUT)?;
    assert_eq!(
        response["result"]["structuredContent"]["error_code"], "ACTION_STALE",
        "the listed digest does not resolve over the changed capture: {response}"
    );
    assert_eq!(std::fs::read_to_string(project.lib())?, edited);
    server.finish()
}
