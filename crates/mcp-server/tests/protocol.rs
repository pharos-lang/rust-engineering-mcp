//! Wire fixtures deliberately do not use rmcp model types: the executable is the boundary.
use serde_json::{Value, json};
use std::error::Error;
use std::io::{self, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

type TestResult = Result<(), Box<dyn Error>>;
const TIMEOUT: Duration = Duration::from_secs(10);
// Session budget includes repeated discovery of both complete output schemas.
const OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
const FRAME_LIMIT: usize = 1024 * 1024;
const VERSION: &str = "2026-07-28";
const LEGACY: [&str; 4] = ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];
const SECRET: &str = "SECRET_PROTOCOL_PAYLOAD_7f62";

struct Server {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Receiver<io::Result<Vec<u8>>>,
    stderr: Receiver<io::Result<Vec<u8>>>,
}

impl Server {
    fn start() -> io::Result<Self> {
        Self::start_with_args(&[])
    }

    fn start_with_args(args: &[&str]) -> io::Result<Self> {
        Self::start_configured(None, args)
    }

    fn start_with_output(output: Option<(Stdio, Box<dyn Read + Send>)>) -> io::Result<Self> {
        Self::start_configured(output, &[])
    }

    fn start_configured(
        output: Option<(Stdio, Box<dyn Read + Send>)>,
        args: &[&str],
    ) -> io::Result<Self> {
        let (stdout_config, custom_reader) = match output {
            Some((config, reader)) => (config, Some(reader)),
            None => (Stdio::piped(), None),
        };
        // Test harness only; this never executes project-supplied programs.
        let mut child = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"))
            .args(["serve", "--stdio"])
            .args(args)
            .env_clear()
            .env("RUST_LOG", "trace")
            .stdin(Stdio::piped())
            .stdout(stdout_config)
            .stderr(Stdio::piped())
            .spawn()?;
        let (out_tx, stdout) = mpsc::sync_channel(32);
        let (err_tx, stderr) = mpsc::sync_channel(1);
        let mut server = Self {
            stdin: child.stdin.take(),
            child,
            stdout,
            stderr,
        };
        let output: Box<dyn Read + Send> = match custom_reader {
            Some(reader) => reader,
            None => Box::new(
                server
                    .child
                    .stdout
                    .take()
                    .ok_or_else(|| io::Error::other("missing stdout"))?,
            ),
        };
        let errors = server
            .child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing stderr"))?;
        thread::spawn(move || {
            let mut reader = BufReader::new(output);
            let mut line = Vec::new();
            let mut total = 0;
            let mut byte = [0];
            loop {
                match reader.read(&mut byte) {
                    Ok(0) => {
                        if !line.is_empty() {
                            let _ = out_tx.send(Err(io::Error::other("stdout ended mid-frame")));
                        }
                        break;
                    }
                    Ok(_) => {
                        total += 1;
                        if total > OUTPUT_LIMIT {
                            let _ =
                                out_tx.send(Err(io::Error::other("stdout exceeds harness limit")));
                            break;
                        }
                        if byte[0] == b'\n' {
                            if out_tx.send(Ok(std::mem::take(&mut line))).is_err() {
                                break;
                            }
                        } else {
                            line.push(byte[0]);
                        }
                    }
                    Err(error) => {
                        let _ = out_tx.send(Err(error));
                        break;
                    }
                }
            }
        });
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = errors
                .take((OUTPUT_LIMIT + 1) as u64)
                .read_to_end(&mut bytes)
                .and_then(|_| {
                    if bytes.len() > OUTPUT_LIMIT {
                        Err(io::Error::other("stderr exceeds harness limit"))
                    } else {
                        Ok(bytes)
                    }
                });
            let _ = err_tx.send(result);
        });
        Ok(server)
    }

    fn send_bytes(&mut self, bytes: Vec<u8>) -> TestResult {
        let mut input = self.stdin.take().ok_or("stdin already closed")?;
        let (tx, rx) = mpsc::sync_channel(1);
        // A blocked pipe write must not defeat the test deadline.
        thread::spawn(move || {
            let result = input.write_all(&bytes).and_then(|()| input.flush());
            let _ = tx.send((input, result));
        });
        let (input, result) = rx.recv_timeout(TIMEOUT)?;
        self.stdin = Some(input);
        result?;
        Ok(())
    }

    fn send(&mut self, value: Value) -> TestResult {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.send_bytes(bytes)
    }

    fn response(&self, id: Value) -> Result<Value, Box<dyn Error>> {
        let bytes = self.stdout.recv_timeout(TIMEOUT)??;
        let value: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value.get("id"), Some(&id));
        assert!(value.get("result").is_some() ^ value.get("error").is_some());
        assert!(!String::from_utf8_lossy(&bytes).contains(SECRET));
        Ok(value)
    }

    fn wait(&mut self) -> Result<ExitStatus, Box<dyn Error>> {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                return Err("server failed to exit before deadline".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn finish(mut self, expected: i32) -> Result<Vec<u8>, Box<dyn Error>> {
        self.stdin.take();
        assert_eq!(self.wait()?.code(), Some(expected));
        match self.stdout.recv_timeout(TIMEOUT) {
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
            other => return Err(format!("unexpected trailing stdout: {other:?}").into()),
        }
        let errors = self.stderr.recv_timeout(TIMEOUT)??;
        assert!(!String::from_utf8_lossy(&errors).contains(SECRET));
        if expected == 0 {
            assert!(errors.is_empty(), "unexpected stderr: {errors:?}");
        }
        Ok(errors)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Also runs on assertion failure or deadline expiry, preventing orphan children.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn modern(id: Value, method: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "method":method, "params":{"_meta":{
        "io.modelcontextprotocol/protocolVersion":VERSION,
        "io.modelcontextprotocol/clientCapabilities":{}
    }}})
}

#[test]
fn partial_frame_deadline_closes_a_live_input_pipe() -> TestResult {
    let mut server = Server::start()?;
    server.send_bytes(b"{\"jsonrpc\":".to_vec())?;
    // The writer remains alive. A byte-only cap or EOF check cannot pass this.
    let deadline = Instant::now() + Duration::from_secs(13);
    loop {
        if let Some(status) = server.child.try_wait()? {
            assert_eq!(status.code(), Some(1));
            break;
        }
        if Instant::now() >= deadline {
            return Err("partial frame was not deadlined".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    server.finish(1)?;
    Ok(())
}

#[test]
fn sequential_responses_release_admission_for_reused_request_ids() -> TestResult {
    let mut server = Server::start()?;
    bootstrap(&mut server, VERSION)?;
    // More than the ingress capacity after SDK bootstrap; error responses also
    // release admission and permit sequential reuse of the same request ID.
    for _ in 0..64 {
        server.send(modern(json!(1), "unknown.method"))?;
        assert_eq!(server.response(json!(1))?["error"]["code"], -32601);
    }
    server.finish(0)?;
    Ok(())
}

#[test]
fn batches_are_rejected_by_the_pinned_sdk_in_every_supported_mode() -> TestResult {
    for version in std::iter::once(VERSION).chain(LEGACY) {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        server.send(json!([modern(json!(700), "ping")]))?;
        let bytes = server.stdout.recv_timeout(TIMEOUT)??;
        let rejection: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(rejection["jsonrpc"], "2.0");
        assert_eq!(rejection["error"]["code"], -32600);
        assert!(rejection.get("id").is_none());
        server.send(if version == VERSION {
            modern(json!(3), "tools/list")
        } else {
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list"})
        })?;
        assert!(server.response(json!(3))?.get("result").is_some());
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn first_project_call_preserves_bounded_bootstrap_behavior() -> TestResult {
    let mut server = Server::start()?;
    server.send(project_call(1, json!({"path":"/unauthorized"}), VERSION))?;
    server.send(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}),
    )?;
    // Bootstrap handles this bounded call inline. It does not promise to observe
    // that cancellation before responding; future costly tools must reject here.
    assert_eq!(server.response(json!(1))?["result"]["isError"], true);
    server.send(modern(json!(2), "tools/list"))?;
    assert!(server.response(json!(2))?.get("result").is_some());
    server.finish(0)?;
    Ok(())
}

fn initialize(id: i64, version: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "method":"initialize", "params":{
        "protocolVersion":version,"capabilities":{},
        "clientInfo":{"name":"independent-wire-test","version":"1"}
    }})
}

fn project_call(id: i64, arguments: Value, version: &str) -> Value {
    let mut request = if version == VERSION {
        modern(json!(id), "tools/call")
    } else {
        json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{}})
    };
    request["params"]["name"] = json!("rust.project.open");
    request["params"]["arguments"] = arguments;
    request
}

fn bootstrap(server: &mut Server, version: &str) -> Result<Value, Box<dyn Error>> {
    if version != VERSION {
        server.send(initialize(1, version))?;
        assert_eq!(
            server.response(json!(1))?["result"]["protocolVersion"],
            version
        );
        server.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
    }
    server.send(if version == VERSION {
        modern(json!(2), "tools/list")
    } else {
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}})
    })?;
    let response = server.response(json!(2))?;
    assert_project_list(&response, version == VERSION);
    for (index, snapshot) in [
        (1, include_str!("snapshots/project-inspect-tool.json")),
        (2, include_str!("snapshots/toolchain-inspect-tool.json")),
        (3, include_str!("snapshots/check-tool.json")),
        (4, include_str!("snapshots/format-tool.json")),
        (5, include_str!("snapshots/clippy-tool.json")),
        (6, include_str!("snapshots/test-tool.json")),
        (7, include_str!("snapshots/audit-tool.json")),
        (8, include_str!("snapshots/explain-tool.json")),
        (9, include_str!("snapshots/quality-tool.json")),
        (10, include_str!("snapshots/catalog-status-tool.json")),
        (11, include_str!("snapshots/crate-search-tool.json")),
        (12, include_str!("snapshots/crate-inspect-tool.json")),
    ] {
        assert_eq!(
            response["result"]["tools"][index],
            serde_json::from_str::<Value>(snapshot)?
        );
    }
    Ok(response["result"]["tools"][0].clone())
}

fn assert_output(response: &Value, tool: &Value, is_error: bool, version: &str) -> TestResult {
    assert!(response.get("error").is_none(), "{response}");
    let result = &response["result"];
    assert_eq!(result["isError"], is_error);
    if version == VERSION {
        assert_eq!(result["resultType"], "complete");
    } else {
        assert!(result.get("resultType").is_none());
    }
    let content = result["content"].as_array().ok_or("missing text content")?;
    assert_eq!(content.len(), 1);
    assert_eq!(content[0]["type"], "text");
    let fallback: Value =
        serde_json::from_str(content[0]["text"].as_str().ok_or("missing fallback")?)?;
    assert_eq!(fallback, result["structuredContent"]);
    let validator = jsonschema::validator_for(&tool["outputSchema"])?;
    validator
        .validate(&fallback)
        .map_err(|error| error.to_string())?;
    // Discriminating checks for the public output contract, beyond accepting the
    // implementation's own output: unrelated fields and inconsistent outcomes fail.
    let mut extra = fallback.clone();
    extra["unrecognized"] = json!(true);
    assert!(!validator.is_valid(&extra));
    let mut inconsistent = fallback;
    inconsistent["status"] = json!("passed");
    inconsistent["data"] = Value::Null;
    assert!(!validator.is_valid(&inconsistent));
    Ok(())
}

#[test]
fn project_open_tool_contract_matches_snapshot_and_rejects_invalid_arguments() -> TestResult {
    let snapshot: Value = serde_json::from_str(include_str!("snapshots/project-open-tool.json"))?;
    for version in [VERSION].into_iter().chain(LEGACY) {
        let mut server = Server::start()?;
        let tool = bootstrap(&mut server, version)?;
        assert_eq!(tool, snapshot);
        let input = jsonschema::validator_for(&tool["inputSchema"])?;
        for arguments in [
            json!({}),
            json!({"path":""}),
            json!({"path":17}),
            json!({"path":"/host", "roots":["/host"]}),
            json!({"path":"a".repeat(4097)}),
        ] {
            assert!(!input.is_valid(&arguments));
            server.send(project_call(3, arguments, version))?;
            assert_eq!(server.response(json!(3))?["error"]["code"], -32602);
        }
        assert!(input.is_valid(&json!({"path":"a".repeat(4096)})));
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn project_open_without_host_authority_is_a_structured_operational_failure() -> TestResult {
    for version in [VERSION].into_iter().chain(LEGACY) {
        let mut server = Server::start()?;
        let tool = bootstrap(&mut server, version)?;
        let mut call = project_call(3, json!({"path":"/"}), version);
        // Untrusted caller metadata must never grant a root capability.
        call["params"]["_meta"]["roots"] = json!([{"uri":"file:///"}]);
        server.send(call)?;
        let response = server.response(json!(3))?;
        assert_output(&response, &tool, true, version)?;
        let data = &response["result"]["structuredContent"];
        #[cfg(target_os = "macos")]
        assert_eq!(
            (data["status"].as_str(), data["error_code"].as_str()),
            (Some("blocked"), Some("SANDBOX_DENIED"))
        );
        #[cfg(not(target_os = "macos"))]
        assert_eq!(
            (data["status"].as_str(), data["error_code"].as_str()),
            (Some("unavailable"), Some("UNSUPPORTED_PLATFORM"))
        );
        assert_eq!(data["data"], Value::Null);
        assert_eq!(data["evidence"], json!({"kind":"local"}));
        server.finish(0)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod project_fixtures {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Result<Self, Box<dyn Error>> {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            // Fixture setup alone canonicalizes the macOS temporary-directory
            // alias; the actual host CLI receives a physical, owned path.
            let root = std::env::temp_dir().canonicalize()?.join(format!(
                "rust-mcp-wire-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir(&root)?;
            Ok(Self(root))
        }
        fn path(&self) -> Result<&str, Box<dyn Error>> {
            self.0
                .to_str()
                .ok_or_else(|| "non-UTF8 fixture path".into())
        }
        fn package(&self) -> TestResult {
            fs::create_dir(self.0.join("src"))?;
            fs::write(self.0.join("src/lib.rs"), "pub fn fixture() {}\n")?;
            fs::write(
                self.0.join("Cargo.toml"),
                "[package]\nname='wire-fixture'\nversion='0.1.0'\nedition='2024'\n",
            )?;
            // This trap proves structural open does not invoke build.rs.
            fs::write(
                self.0.join("build.rs"),
                "fn main() { panic!(\"must never execute\"); }\n",
            )?;
            Ok(())
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn project_open_succeeds_with_host_root_in_modern_and_all_legacy_versions() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.package()?;
        for version in [VERSION].into_iter().chain(LEGACY) {
            let mut server = Server::start_with_args(&["--root", fixture.path()?])?;
            let tool = bootstrap(&mut server, version)?;
            let mut references = Vec::new();
            let mut fingerprints = Vec::new();
            for id in [3, 4] {
                server.send(project_call(id, json!({"path":fixture.path()?}), version))?;
                let response = server.response(json!(id))?;
                assert_output(&response, &tool, false, version)?;
                let output = &response["result"]["structuredContent"];
                assert_eq!(output["status"], "passed");
                assert_eq!(output["error_code"], Value::Null);
                assert_eq!(output["error_message"], Value::Null);
                assert_eq!(output["data"]["workspace_root"], fixture.path()?);
                assert_eq!(output["data"]["validation"], "structural");
                assert_eq!(output["evidence"], json!({"kind":"local"}));
                references.push(output["data"]["project_ref"].clone());
                fingerprints.push(output["data"]["fingerprint"].clone());
            }
            assert_ne!(references[0], references[1]);
            assert_eq!(fingerprints[0], fingerprints[1]);
            assert!(!fixture.0.join("target").exists());
            assert!(!fixture.0.join("Cargo.lock").exists());
            server.finish(0)?;
        }
        Ok(())
    }

    #[test]
    fn inspect_live_reference_without_runtime_is_denied_without_project_execution() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.package()?;
        for version in std::iter::once(VERSION).chain(LEGACY) {
            let mut server = Server::start_with_args(&["--root", fixture.path()?])?;
            bootstrap(&mut server, version)?;
            server.send(project_call(3, json!({"path":fixture.path()?}), version))?;
            let opened = server.response(json!(3))?;
            assert_eq!(opened["result"]["isError"], false);
            let reference = opened["result"]["structuredContent"]["data"]["project_ref"].clone();
            server.send(if version == VERSION {
                modern(json!(4), "tools/list")
            } else {
                json!({"jsonrpc":"2.0","id":4,"method":"tools/list"})
            })?;
            let tool = server.response(json!(4))?["result"]["tools"][1].clone();
            server.send(inspect_call(5, json!({"project_ref":reference}), version))?;
            let response = server.response(json!(5))?;
            assert_output(&response, &tool, true, version)?;
            assert_eq!(
                response["result"]["structuredContent"]["error_code"],
                "SANDBOX_DENIED"
            );
            assert!(!fixture.0.join("target").exists());
            assert!(!fixture.0.join("Cargo.lock").exists());
            server.finish(0)?;
        }
        Ok(())
    }

    #[test]
    fn deeply_nested_toml_is_rejected_without_aborting_the_server() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.package()?;
        let mut server = Server::start_with_args(&["--root", fixture.path()?])?;
        let tool = bootstrap(&mut server, VERSION)?;
        let valid = fs::read_to_string(fixture.0.join("Cargo.toml"))?;
        for value in [
            format!("{}0{}", "[".repeat(100_000), "]".repeat(100_000)),
            format!("{}0{}", "{a=".repeat(50_000), "}".repeat(50_000)),
        ] {
            let source = format!("{valid}[package.metadata]\nx={value}\n");
            assert!(source.len() < 256 * 1024);
            fs::write(fixture.0.join("Cargo.toml"), source)?;
            server.send(project_call(3, json!({"path":fixture.path()?}), VERSION))?;
            let response = server.response(json!(3))?;
            assert_output(&response, &tool, true, VERSION)?;
            assert_eq!(
                response["result"]["structuredContent"]["error_code"],
                "INVALID_PROJECT"
            );
        }
        let source = format!(
            "{valid}[package.metadata.{}end]\nx=1\n",
            "a.".repeat(50_000)
        );
        assert!(source.len() < 256 * 1024);
        fs::write(fixture.0.join("Cargo.toml"), source)?;
        server.send(project_call(3, json!({"path":fixture.path()?}), VERSION))?;
        let response = server.response(json!(3))?;
        assert_output(&response, &tool, true, VERSION)?;
        assert_eq!(
            response["result"]["structuredContent"]["error_code"],
            "INVALID_PROJECT"
        );
        fs::write(fixture.0.join("Cargo.toml"), valid)?;
        server.send(project_call(4, json!({"path":fixture.path()?}), VERSION))?;
        assert_output(&server.response(json!(4))?, &tool, false, VERSION)?;
        server.finish(0)?;
        Ok(())
    }

    #[test]
    fn project_open_missing_or_invalid_manifest_returns_tool_error_and_service_survives()
    -> TestResult {
        let fixture = Fixture::new()?;
        let mut server = Server::start_with_args(&["--root", fixture.path()?])?;
        let tool = bootstrap(&mut server, VERSION)?;
        for invalid in [
            None,
            Some("[package\nnot valid TOML"),
            Some("[package]\nname='invalid'\n"),
        ] {
            if let Some(contents) = invalid {
                fs::write(fixture.0.join("Cargo.toml"), contents)?;
            }
            server.send(project_call(3, json!({"path":fixture.path()?}), VERSION))?;
            let response = server.response(json!(3))?;
            assert_output(&response, &tool, true, VERSION)?;
            assert_eq!(response["result"]["structuredContent"]["status"], "blocked");
            assert_eq!(
                response["result"]["structuredContent"]["error_code"],
                "INVALID_PROJECT"
            );
        }
        fixture.package()?;
        server.send(project_call(4, json!({"path":fixture.path()?}), VERSION))?;
        assert_output(&server.response(json!(4))?, &tool, false, VERSION)?;
        server.finish(0)?;
        Ok(())
    }
}

fn assert_project_list(response: &Value, modern: bool) {
    assert!(response.get("error").is_none(), "{response}");
    let tools = response["result"]["tools"].as_array();
    assert_eq!(tools.map(Vec::len), Some(13));
    let names: Vec<_> = response["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["name"].as_str())
        .collect();
    assert_eq!(
        names,
        [
            "rust.project.open",
            "rust.project.inspect",
            "rust.toolchain.inspect",
            "rust.check",
            "rust.fmt.check",
            "rust.clippy",
            "rust.test",
            "rust.dependencies.audit",
            "rust.diagnostics.explain",
            "rust.quality.gate",
            "rust.catalog.status",
            "rust.crate.search",
            "rust.crate.inspect"
        ]
    );
    let tool = &response["result"]["tools"][0];
    assert_eq!(tool["name"], "rust.project.open");
    assert_eq!(tool["inputSchema"]["type"], "object");
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(tool["inputSchema"]["required"], json!(["path"]));
    assert_eq!(
        tool["annotations"],
        json!({
            "readOnlyHint":true,"destructiveHint":false,
            "idempotentHint":false,"openWorldHint":false
        })
    );
    let inspect = &response["result"]["tools"][1];
    assert_eq!(inspect["name"], "rust.project.inspect");
    assert_eq!(inspect["inputSchema"]["type"], "object");
    assert_eq!(inspect["inputSchema"]["additionalProperties"], false);
    assert_eq!(inspect["inputSchema"]["required"], json!(["project_ref"]));
    assert_eq!(
        inspect["annotations"],
        json!({
            "readOnlyHint":true,"destructiveHint":false,
            "idempotentHint":true,"openWorldHint":false
        })
    );
    let toolchain = &response["result"]["tools"][2];
    assert_eq!(toolchain["name"], "rust.toolchain.inspect");
    assert_eq!(toolchain["inputSchema"]["type"], "object");
    assert_eq!(toolchain["inputSchema"]["additionalProperties"], false);
    assert_eq!(toolchain["inputSchema"]["required"], json!(["project_ref"]));
    assert!(response["result"].get("nextCursor").is_none());
    if modern {
        assert_eq!(response["result"]["resultType"], "complete");
    } else {
        assert!(response["result"].get("resultType").is_none());
    }
}

#[test]
fn modern_discovery_and_deterministic_project_tool() -> TestResult {
    let mut server = Server::start()?;
    server.send(modern(json!(1), "server/discover"))?;
    let response = server.response(json!(1))?;
    let result = &response["result"];
    assert_eq!(result["resultType"], "complete");
    assert_eq!(
        result["supportedVersions"],
        json!([
            "2024-11-05",
            "2025-03-26",
            "2025-06-18",
            "2025-11-25",
            "2026-07-28"
        ])
    );
    assert_eq!(result["capabilities"], json!({"tools":{},"resources":{}}));
    assert!(result.get("serverInfo").is_none());
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "rust-engineering-mcp"
    );
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"]["version"],
        env!("CARGO_PKG_VERSION")
    );
    server.send(modern(json!("list-a"), "tools/list"))?;
    let first = server.response(json!("list-a"))?;
    assert_project_list(&first, true);
    server.send(modern(json!("list-b"), "tools/list"))?;
    let second = server.response(json!("list-b"))?;
    assert_eq!(first["result"], second["result"]);
    server.finish(0)?;
    Ok(())
}

#[test]
fn modern_request_bootstraps_without_discovery() -> TestResult {
    let mut server = Server::start()?;
    server.send(modern(json!(1), "tools/list"))?;
    assert_project_list(&server.response(json!(1))?, true);
    server.finish(0)?;
    Ok(())
}

#[test]
fn legacy_versions_and_initialize_fallback() -> TestResult {
    for version in LEGACY.into_iter().chain([VERSION, "2099-01-01"]) {
        let mut server = Server::start()?;
        server.send(initialize(1, version))?;
        let response = server.response(json!(1))?;
        let negotiated = if LEGACY.contains(&version) {
            version
        } else {
            "2025-11-25"
        };
        assert_eq!(response["result"]["protocolVersion"], negotiated);
        assert_eq!(
            response["result"]["capabilities"],
            json!({"tools":{},"resources":{}})
        );
        assert_eq!(
            response["result"]["serverInfo"]["name"],
            "rust-engineering-mcp"
        );
        assert_eq!(
            response["result"]["serverInfo"]["version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(response["result"].get("resultType").is_none());
        server.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        server.send(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))?;
        assert_project_list(&server.response(json!(2))?, false);
        server.finish(0)?;
    }
    Ok(())
}

fn bad_requests() -> Vec<(Value, i64)> {
    let mut unknown = modern(json!(2), "tools/list");
    unknown["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("2099-01-01");
    let mut malformed = modern(json!(2), "tools/list");
    malformed["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"] = json!(SECRET);
    let mut malformed_version = modern(json!(2), "tools/list");
    malformed_version["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!(42);
    let mut missing_capabilities = modern(json!(2), "tools/list");
    missing_capabilities["params"]["_meta"] =
        json!({"io.modelcontextprotocol/protocolVersion": VERSION});
    vec![
        (unknown, -32022),
        (
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
            -32602,
        ),
        (malformed, -32602),
        (malformed_version, -32602),
        (missing_capabilities, -32602),
    ]
}

#[test]
fn initial_metadata_errors_follow_sdk_lifecycle() -> TestResult {
    for (request, code) in bad_requests() {
        let mut server = Server::start()?;
        server.send(request)?;
        assert_eq!(server.response(json!(2))?["error"]["code"], code);
        if code == -32022 {
            server.send(modern(json!(3), "tools/list"))?;
            assert_project_list(&server.response(json!(3))?, true);
            server.finish(0)?;
        } else {
            // Missing keys terminate even while the client keeps stdin open.
            assert_eq!(server.wait()?.code(), Some(1));
            server.finish(1)?;
        }
    }
    Ok(())
}

#[test]
fn metadata_errors_after_bootstrap_are_recoverable() -> TestResult {
    let mut server = Server::start()?;
    server.send(modern(json!(1), "tools/list"))?;
    assert_project_list(&server.response(json!(1))?, true);
    for (request, code) in bad_requests() {
        server.send(request)?;
        assert_eq!(server.response(json!(2))?["error"]["code"], code);
        server.send(modern(json!(3), "tools/list"))?;
        assert_project_list(&server.response(json!(3))?, true);
    }
    server.finish(0)?;
    Ok(())
}

#[test]
fn unavailable_tool_and_unknown_cancellation_preserve_service() -> TestResult {
    let mut server = Server::start()?;
    server.send(modern(json!(1), "tools/list"))?;
    assert_project_list(&server.response(json!(1))?, true);
    let mut call = modern(json!(2), "tools/call");
    call["params"]["name"] = json!("rust.dependencies.inspect");
    call["params"]["arguments"] = json!({"secret":SECRET});
    server.send(call)?;
    assert_eq!(server.response(json!(2))?["error"]["code"], -32601);
    server.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":999,"reason":SECRET}}))?;
    server.send(modern(json!(3), "tools/list"))?;
    assert_project_list(&server.response(json!(3))?, true);
    server.send(modern(json!(4), SECRET))?;
    let unknown = server.response(json!(4))?;
    assert_eq!(unknown["error"]["code"], -32601);
    server.finish(0)?;
    Ok(())
}

#[test]
fn empty_eof_exits_cleanly_without_protocol_output() -> TestResult {
    Server::start()?.finish(0)?;
    Ok(())
}

#[test]
fn eof_drains_complete_requests_already_written() -> TestResult {
    let mut server = Server::start()?;
    server.send(modern(json!(1), "server/discover"))?;
    server.send(modern(json!(2), "tools/list"))?;
    server.stdin.take();
    assert_eq!(
        server.response(json!(1))?["result"]["capabilities"],
        json!({"tools":{},"resources":{}})
    );
    assert_project_list(&server.response(json!(2))?, true);
    server.finish(0)?;
    Ok(())
}

#[test]
fn fragmented_crlf_frames_and_coalesced_requests() -> TestResult {
    let mut server = Server::start()?;
    let bytes = serde_json::to_vec(&modern(json!(1), "tools/list"))?;
    let middle = bytes.len() / 2;
    server.send_bytes(bytes[..middle].to_vec())?;
    let mut tail = bytes[middle..].to_vec();
    tail.extend_from_slice(b"\r\n");
    server.send_bytes(tail)?;
    assert_project_list(&server.response(json!(1))?, true);
    let mut coalesced = serde_json::to_vec(&modern(json!(2), "tools/list"))?;
    coalesced.push(b'\n');
    coalesced.extend(serde_json::to_vec(&modern(json!(3), "tools/list"))?);
    coalesced.push(b'\n');
    server.send_bytes(coalesced)?;
    // SDK execution can reorder concurrent requests; correlation must survive.
    let mut ids = Vec::new();
    for _ in 0..2 {
        let bytes = server.stdout.recv_timeout(TIMEOUT)??;
        let value: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(value["jsonrpc"], "2.0");
        assert_project_list(&value, true);
        ids.push(value["id"].as_i64().ok_or("non-numeric response id")?);
    }
    ids.sort_unstable();
    assert_eq!(ids, [2, 3]);
    server.finish(0)?;
    Ok(())
}

#[test]
fn invalid_json_syntax_is_ignored_and_invalid_shape_is_rejected() -> TestResult {
    let mut server = Server::start()?;
    server.send_bytes(format!("{{ invalid {SECRET}\n").into_bytes())?;
    server.send(modern(json!(1), "tools/list"))?;
    assert_project_list(&server.response(json!(1))?, true);
    for shape in [json!({"secret":SECRET}), json!([])] {
        server.send(shape)?;
        let bytes = server.stdout.recv_timeout(TIMEOUT)??;
        let response: Value = serde_json::from_slice(&bytes)?;
        assert_eq!(response["jsonrpc"], "2.0");
        assert_eq!(response["error"]["code"], -32600);
        assert!(response.get("id").is_none());
        assert!(!String::from_utf8_lossy(&bytes).contains(SECRET));
    }
    server.send(modern(json!(2), "tools/list"))?;
    assert_project_list(&server.response(json!(2))?, true);
    server.finish(0)?;
    Ok(())
}

#[test]
fn exact_frame_limit_accepts_lf_and_counts_cr() -> TestResult {
    for ending in [b"\n".as_slice(), b"\r\n".as_slice()] {
        let mut server = Server::start()?;
        let mut frame = serde_json::to_vec(&modern(json!(1), "tools/list"))?;
        frame.resize(FRAME_LIMIT - (ending.len() - 1), b' ');
        frame.extend_from_slice(ending);
        assert_eq!(frame.len(), FRAME_LIMIT + 1);
        server.send_bytes(frame.clone())?;
        assert_project_list(&server.response(json!(1))?, true);
        // The budget resets at each LF, not at a read boundary or per session.
        server.send_bytes(frame)?;
        assert_project_list(&server.response(json!(1))?, true);
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn oversized_frames_fail_without_waiting_for_eof_or_leaking_input() -> TestResult {
    let mut baseline = None;
    for ending in [b"".as_slice(), b"\n".as_slice(), b"\r\n".as_slice()] {
        let mut server = Server::start()?;
        let mut frame = SECRET.as_bytes().to_vec();
        frame.resize(FRAME_LIMIT + usize::from(ending != b"\r\n"), b' ');
        frame.extend_from_slice(ending);
        server.send_bytes(frame)?;
        assert_eq!(server.wait()?.code(), Some(1));
        let errors = server.finish(1)?;
        assert!(!errors.is_empty());
        if let Some(expected) = &baseline {
            assert_eq!(&errors, expected);
        } else {
            baseline = Some(errors);
        }
    }
    Ok(())
}

#[test]
fn eof_with_unterminated_frame_is_rejected() -> TestResult {
    for bytes in [
        serde_json::to_vec(&modern(json!(1), "tools/list"))?,
        SECRET.as_bytes().to_vec(),
    ] {
        let mut server = Server::start()?;
        server.send_bytes(bytes)?;
        assert!(!server.finish(1)?.is_empty());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn closed_stdout_exits_even_when_stdin_remains_open() -> TestResult {
    use std::net::Shutdown;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;

    for bootstrap in [false, true] {
        let (writer, reader) = UnixStream::pair()?;
        let shutdown = reader.try_clone()?;
        let mut server = Server::start_with_output(Some((
            Stdio::from(OwnedFd::from(writer)),
            Box::new(reader),
        )))?;
        if bootstrap {
            server.send(modern(json!(1), "tools/list"))?;
            assert_project_list(&server.response(json!(1))?, true);
        }
        shutdown.shutdown(Shutdown::Both)?;
        drop(shutdown);
        // macOS may accept writes to a shutdown socket until every peer handle
        // closes. EOF on the channel proves the reader thread dropped its handle.
        assert!(matches!(
            server.stdout.recv_timeout(TIMEOUT),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
        server.send(modern(json!(2), "tools/list"))?;
        let status = server
            .wait()
            .map_err(|error| format!("closed stdout (bootstrap={bootstrap}): {error}"))?;
        assert_eq!(status.code(), Some(1));
        assert!(!server.finish(1)?.is_empty());
    }
    Ok(())
}

fn inspect_call(id: i64, arguments: Value, version: &str) -> Value {
    let mut request = project_call(id, arguments, version);
    request["params"]["name"] = json!("rust.project.inspect");
    request
}

#[test]
fn inspect_rejects_malformed_input_and_unknown_references_in_all_versions() -> TestResult {
    for (name, index) in [
        ("rust.project.inspect", 1),
        ("rust.toolchain.inspect", 2),
        ("rust.check", 3),
        ("rust.fmt.check", 4),
        ("rust.clippy", 5),
        ("rust.test", 6),
        ("rust.dependencies.audit", 7),
    ] {
        for version in std::iter::once(VERSION).chain(LEGACY) {
            let mut server = Server::start()?;
            bootstrap(&mut server, version)?;
            server.send(if version == VERSION {
                modern(json!(10), "tools/list")
            } else {
                json!({"jsonrpc":"2.0","id":10,"method":"tools/list"})
            })?;
            let tool = server.response(json!(10))?["result"]["tools"][index].clone();
            let validator = jsonschema::validator_for(&tool["inputSchema"])?;
            for arguments in [
                json!({}),
                json!({"project_ref":42}),
                json!({"project_ref":"prj_00000000000000000000000000000001","options":{}}),
                json!({"project_ref":SECRET}),
                json!({"project_ref":"prj_00000000000000000000000000000001","secret":SECRET}),
                json!({"project_ref":"prj_A0000000000000000000000000000000"}),
            ] {
                assert!(!validator.is_valid(&arguments));
                server.send(named_inspect_call(11, name, arguments, version))?;
                assert_eq!(server.response(json!(11))?["error"]["code"], -32602);
            }
            let arguments = json!({"project_ref":"prj_00000000000000000000000000000001"});
            assert!(validator.is_valid(&arguments));
            server.send(named_inspect_call(12, name, arguments, version))?;
            let response = server.response(json!(12))?;
            assert_output(&response, &tool, true, version)?;
            assert_eq!(
                response["result"]["structuredContent"]["error_code"],
                "PROJECT_NOT_FOUND"
            );
            assert_eq!(
                response["result"]["structuredContent"]["evidence"],
                json!({"kind":"local"})
            );
            server.finish(0)?;
        }
    }
    Ok(())
}

#[test]
fn first_inspect_is_denied_until_discovery_then_reference_validation_runs() -> TestResult {
    for (name, index) in [
        ("rust.project.inspect", 1),
        ("rust.toolchain.inspect", 2),
        ("rust.check", 3),
        ("rust.fmt.check", 4),
        ("rust.clippy", 5),
        ("rust.test", 6),
        ("rust.dependencies.audit", 7),
    ] {
        let mut server = Server::start()?;
        let arguments = json!({"project_ref":"prj_00000000000000000000000000000001"});
        server.send(named_inspect_call(1, name, arguments.clone(), VERSION))?;
        let first = server.response(json!(1))?;
        assert_eq!(
            first["result"]["structuredContent"]["error_code"],
            "SANDBOX_DENIED"
        );
        assert!(
            first["result"]["structuredContent"]["error_message"]
                .as_str()
                .ok_or("missing hint")?
                .contains("discovery")
        );
        bootstrap(&mut server, VERSION)?;
        server.send(modern(json!(3), "tools/list"))?;
        let tool = server.response(json!(3))?["result"]["tools"][index].clone();
        assert_output(&first, &tool, true, VERSION)?;
        server.send(named_inspect_call(4, name, arguments, VERSION))?;
        let response = server.response(json!(4))?;
        assert_output(&response, &tool, true, VERSION)?;
        assert_eq!(
            response["result"]["structuredContent"]["error_code"],
            "PROJECT_NOT_FOUND"
        );
        server.finish(0)?;
    }
    Ok(())
}

fn named_inspect_call(id: i64, name: &str, arguments: Value, version: &str) -> Value {
    let mut request = inspect_call(id, arguments, version);
    request["params"]["name"] = json!(name);
    request
}

#[test]
fn opaque_resources_are_not_enumerated_and_invalid_authority_is_uniform() -> TestResult {
    for version in [
        "2024-11-05",
        "2025-03-26",
        "2025-06-18",
        "2025-11-25",
        VERSION,
    ] {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        let make = |id, method| {
            if version == VERSION {
                modern(json!(id), method)
            } else {
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":{}})
            }
        };
        server.send(make(10, "resources/list"))?;
        assert_eq!(
            server.response(json!(10))?["result"]["resources"],
            json!([])
        );
        for uri in [
            "file:///private/secret",
            "rust-artifact://prj_00000000000000000000000000000001/art_00000000000000000000000000000001",
            "rust-artifact://prj_00000000000000000000000000000001/art_00000000000000000000000000000001?x=1",
        ] {
            let mut request = make(11, "resources/read");
            request["params"]["uri"] = json!(uri);
            server.send(request)?;
            let response = server.response(json!(11))?;
            assert_eq!(
                response["error"]["code"],
                if version == VERSION { -32602 } else { -32002 },
                "{version}: {response}"
            );
            assert_eq!(response["error"]["message"], "Artifact resource not found");
            assert!(response["error"].get("data").is_none());
        }
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn clippy_closed_profiles_and_options_are_enforced_in_all_wire_versions() -> TestResult {
    for version in std::iter::once(VERSION).chain(LEGACY) {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        for patch in [
            json!({"target":"aarch64-unknown-linux-gnu"}),
            json!({"all_features":false}),
            json!({"no_default_features":false}),
            json!({"args":["--fix"]}),
            json!({"lint_profile":"unknown"}),
            json!({"lint_profile":null}),
            json!({"package":"member","workspace":true}),
            json!({"features":["a","a"]}),
        ] {
            let mut arguments = json!({"project_ref":"prj_00000000000000000000000000000001"});
            arguments
                .as_object_mut()
                .ok_or("arguments")?
                .extend(patch.as_object().ok_or("patch")?.clone());
            server.send(named_inspect_call(11, "rust.clippy", arguments, version))?;
            assert_eq!(server.response(json!(11))?["error"]["code"], -32602);
        }
        for profile in ["default", "strict", "pedantic", "project"] {
            server.send(named_inspect_call(12,"rust.clippy",json!({"project_ref":"prj_00000000000000000000000000000001","lint_profile":profile}),version))?;
            let response = server.response(json!(12))?;
            assert_eq!(
                response["result"]["structuredContent"]["error_code"],
                "PROJECT_NOT_FOUND"
            );
        }
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn test_closed_options_are_enforced_in_all_wire_versions() -> TestResult {
    for version in std::iter::once(VERSION).chain(LEGACY) {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        for patch in [
            json!({"workspace":false}),
            json!({"all_targets":false}),
            json!({"no_default_features":false}),
            json!({"args":["--ignored"]}),
            json!({"test_filter":"--ignored"}),
            json!({"test_filter":"a b"}),
            json!({"test_filter":"x".repeat(129)}),
            json!({"timeout":0}),
            json!({"timeout":61}),
            json!({"timeout":1.5}),
            json!({"timeout":null}),
            json!({"target":"x86_64-unknown-linux-gnu"}),
            json!({"features":["a","a"]}),
            json!({"all_features":true,"features":["a"]}),
        ] {
            let mut arguments = json!({"project_ref":"prj_00000000000000000000000000000001"});
            arguments
                .as_object_mut()
                .ok_or("arguments")?
                .extend(patch.as_object().ok_or("patch")?.clone());
            server.send(named_inspect_call(11, "rust.test", arguments, version))?;
            assert_eq!(server.response(json!(11))?["error"]["code"], -32602);
        }
        for timeout in [1, 30, 60] {
            server.send(named_inspect_call(12,"rust.test",json!({"project_ref":"prj_00000000000000000000000000000001","test_filter":"module::case","timeout":timeout}),version))?;
            assert_eq!(
                server.response(json!(12))?["result"]["structuredContent"]["error_code"],
                "PROJECT_NOT_FOUND"
            );
        }
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn diagnostic_explain_validates_code_without_project_authority_in_all_versions() -> TestResult {
    for version in std::iter::once(VERSION).chain(LEGACY) {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        server.send(if version == VERSION {
            modern(json!(10), "tools/list")
        } else {
            json!({"jsonrpc":"2.0","id":10,"method":"tools/list"})
        })?;
        let tool = server.response(json!(10))?["result"]["tools"][8].clone();
        assert_eq!(tool["name"], "rust.diagnostics.explain");
        let validator = jsonschema::validator_for(&tool["inputSchema"])?;
        for arguments in [
            json!({}),
            json!({"code":42}),
            json!({"code":"e0502"}),
            json!({"code":"E0502 --help"}),
            json!({"code":"E１２３４"}),
            json!({"code":"E0502","project_ref":"prj_00000000000000000000000000000001"}),
        ] {
            assert!(!validator.is_valid(&arguments));
            server.send(named_inspect_call(
                11,
                "rust.diagnostics.explain",
                arguments,
                version,
            ))?;
            assert_eq!(server.response(json!(11))?["error"]["code"], -32602);
        }
        let arguments = json!({"code":"E0502"});
        assert!(validator.is_valid(&arguments));
        server.send(named_inspect_call(
            12,
            "rust.diagnostics.explain",
            arguments,
            version,
        ))?;
        let response = server.response(json!(12))?;
        assert_output(&response, &tool, true, version)?;
        assert_eq!(
            response["result"]["structuredContent"]["error_code"],
            "SANDBOX_DENIED"
        );
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn quality_profiles_require_closed_input_and_live_reference_in_all_versions() -> TestResult {
    for version in std::iter::once(VERSION).chain(LEGACY) {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        server.send(if version == VERSION {
            modern(json!(10), "tools/list")
        } else {
            json!({"jsonrpc":"2.0","id":10,"method":"tools/list"})
        })?;
        let tool = server.response(json!(10))?["result"]["tools"][9].clone();
        assert_eq!(tool["name"], "rust.quality.gate");
        let validator = jsonschema::validator_for(&tool["inputSchema"])?;
        for arguments in [
            json!({}),
            json!({"project_ref":"prj_00000000000000000000000000000001"}),
            json!({"project_ref":"prj_00000000000000000000000000000001","profile":"release"}),
            json!({"project_ref":"prj_00000000000000000000000000000001","profile":"fast","timeout":240}),
            json!({"project_ref":"prj_00000000000000000000000000000001","profile":"standard","args":["--release"]}),
        ] {
            assert!(!validator.is_valid(&arguments));
            server.send(named_inspect_call(
                11,
                "rust.quality.gate",
                arguments,
                version,
            ))?;
            assert_eq!(server.response(json!(11))?["error"]["code"], -32602);
        }
        for profile in ["fast", "standard"] {
            let args =
                json!({"project_ref":"prj_00000000000000000000000000000001","profile":profile});
            assert!(validator.is_valid(&args));
            server.send(named_inspect_call(12, "rust.quality.gate", args, version))?;
            let response = server.response(json!(12))?;
            assert_output(&response, &tool, true, version)?;
            assert_eq!(
                response["result"]["structuredContent"]["error_code"],
                "PROJECT_NOT_FOUND"
            );
        }
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn catalog_status_closed_input_and_explicit_absence_in_all_versions() -> TestResult {
    for version in std::iter::once(VERSION).chain(LEGACY) {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        server.send(if version == VERSION {
            modern(json!(10), "tools/list")
        } else {
            json!({"jsonrpc":"2.0","id":10,"method":"tools/list"})
        })?;
        let tool = server.response(json!(10))?["result"]["tools"][10].clone();
        assert_eq!(tool["name"], "rust.catalog.status");
        let validator = jsonschema::validator_for(&tool["inputSchema"])?;
        for arguments in [
            json!({"path":"/tmp/catalog"}),
            json!({"refresh":true}),
            json!({"network":true}),
            json!({"model_dir":"/tmp/model"}),
            json!({"project_ref":"prj_00000000000000000000000000000001"}),
        ] {
            assert!(!validator.is_valid(&arguments));
            server.send(named_inspect_call(
                11,
                "rust.catalog.status",
                arguments,
                version,
            ))?;
            assert_eq!(server.response(json!(11))?["error"]["code"], -32602);
        }
        server.send(named_inspect_call(
            12,
            "rust.catalog.status",
            json!({}),
            version,
        ))?;
        let response = server.response(json!(12))?;
        assert_output(&response, &tool, false, version)?;
        let data = &response["result"]["structuredContent"]["data"];
        assert_eq!(data["semantics"], "latest_known");
        assert_eq!(
            data["network"],
            json!({"acquisition_allowed":false,"enforcement":"runtime_api_disabled"})
        );
        for component in ["catalog", "model", "semantic_index", "rustsec"] {
            assert_eq!(
                data["context"][component],
                json!({"status":"unavailable","reason":"not_configured"})
            );
        }
        assert!(data["context"]["reservation"].is_null());
        server.finish(0)?;
    }
    Ok(())
}

#[test]
fn first_catalog_status_is_denied_before_discovery() -> TestResult {
    let mut server = Server::start()?;
    server.send(named_inspect_call(
        1,
        "rust.catalog.status",
        json!({}),
        VERSION,
    ))?;
    let first = server.response(json!(1))?;
    assert_eq!(
        first["result"]["structuredContent"]["error_code"],
        "SANDBOX_DENIED"
    );
    bootstrap(&mut server, VERSION)?;
    server.send(named_inspect_call(
        3,
        "rust.catalog.status",
        json!({}),
        VERSION,
    ))?;
    assert_eq!(
        server.response(json!(3))?["result"]["structuredContent"]["status"],
        "passed"
    );
    server.finish(0)?;
    Ok(())
}

#[test]
fn crate_search_validates_closed_inputs_and_reports_missing_catalog_in_all_versions() -> TestResult
{
    for version in std::iter::once(VERSION).chain(LEGACY) {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        server.send(if version == VERSION {
            modern(json!(10), "tools/list")
        } else {
            json!({"jsonrpc":"2.0","id":10,"method":"tools/list"})
        })?;
        let tool = server.response(json!(10))?["result"]["tools"][11].clone();
        assert_eq!(tool["name"], "rust.crate.search");
        for args in [
            json!({}),
            json!({"query":"x","mode":"all"}),
            json!({"query":"x","filters":{"sql":"select"}}),
            json!({"query":"x","limit":51}),
            json!({"query":"x","filters":{"msrv_lte":"1.70-beta"}}),
            json!({"query":"é".repeat(129)}),
            json!({"query":"x","refresh":true}),
        ] {
            server.send(named_inspect_call(11, "rust.crate.search", args, version))?;
            assert_eq!(server.response(json!(11))?["error"]["code"], -32602);
        }
        server.send(named_inspect_call(
            12,
            "rust.crate.search",
            json!({"query":"binary serialization"}),
            version,
        ))?;
        let response = server.response(json!(12))?;
        assert_output(&response, &tool, true, version)?;
        assert_eq!(
            response["result"]["structuredContent"]["status"],
            "unavailable"
        );
        assert_eq!(
            response["result"]["structuredContent"]["error_code"],
            "CATALOG_UNAVAILABLE"
        );
        server.finish(0)?;
    }
    Ok(())
}
#[test]
fn first_crate_search_requires_bootstrap_before_catalog_access() -> TestResult {
    let mut server = Server::start()?;
    server.send(named_inspect_call(
        1,
        "rust.crate.search",
        json!({"query":"binary"}),
        VERSION,
    ))?;
    assert_eq!(
        server.response(json!(1))?["result"]["structuredContent"]["error_code"],
        "SANDBOX_DENIED"
    );
    bootstrap(&mut server, VERSION)?;
    server.send(named_inspect_call(
        3,
        "rust.crate.search",
        json!({"query":"binary"}),
        VERSION,
    ))?;
    assert_eq!(
        server.response(json!(3))?["result"]["structuredContent"]["error_code"],
        "CATALOG_UNAVAILABLE"
    );
    server.finish(0)?;
    Ok(())
}

#[test]
fn crate_inspect_closed_shape_and_missing_catalog_in_all_versions() -> TestResult {
    for version in std::iter::once(VERSION).chain(LEGACY) {
        let mut server = Server::start()?;
        bootstrap(&mut server, version)?;
        server.send(if version == VERSION {
            modern(json!(10), "tools/list")
        } else {
            json!({"jsonrpc":"2.0","id":10,"method":"tools/list"})
        })?;
        let tool = server.response(json!(10))?["result"]["tools"][12].clone();
        assert_eq!(tool["name"], "rust.crate.inspect");
        for args in [
            json!({}),
            json!({"name":"../x"}),
            json!({"name":"x","section":"features"}),
            json!({"name":"x","section":"versions","version":"1.0.0"}),
            json!({"name":"x","offset":1}),
            json!({"name":"x","limit":51}),
            json!({"name":"x","refresh":true}),
            json!({"name":"x","snapshot_fingerprint":"invalid"}),
        ] {
            server.send(named_inspect_call(11, "rust.crate.inspect", args, version))?;
            assert_eq!(server.response(json!(11))?["error"]["code"], -32602);
        }
        server.send(named_inspect_call(
            12,
            "rust.crate.inspect",
            json!({"name":"serde"}),
            version,
        ))?;
        let response = server.response(json!(12))?;
        assert_output(&response, &tool, true, version)?;
        assert_eq!(
            response["result"]["structuredContent"]["error_code"],
            "CATALOG_UNAVAILABLE"
        );
        server.finish(0)?;
    }
    Ok(())
}
#[test]
fn first_crate_inspect_requires_discovery() -> TestResult {
    let mut server = Server::start()?;
    server.send(named_inspect_call(
        1,
        "rust.crate.inspect",
        json!({"name":"serde"}),
        VERSION,
    ))?;
    assert_eq!(
        server.response(json!(1))?["result"]["structuredContent"]["error_code"],
        "SANDBOX_DENIED"
    );
    bootstrap(&mut server, VERSION)?;
    server.send(named_inspect_call(
        3,
        "rust.crate.inspect",
        json!({"name":"serde"}),
        VERSION,
    ))?;
    assert_eq!(
        server.response(json!(3))?["result"]["structuredContent"]["error_code"],
        "CATALOG_UNAVAILABLE"
    );
    server.finish(0)?;
    Ok(())
}
