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
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/valid-basic")
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
