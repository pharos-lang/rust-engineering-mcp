//! Actual CLI/MCP process boundary; no rmcp client, project execution or acquisition.
#![cfg(target_os = "macos")]
use rust_engineering_application::ReferenceGenerator;
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
const TIMEOUT: Duration = Duration::from_secs(180);
const LIMIT: usize = 2 * 1024 * 1024;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let id = rust_engineering_project::OsReferences
            .generate()
            .map_err(|e| format!("{e:?}"))?;
        let path = PathBuf::from("/private/tmp").join(format!("catalog-status-{id}"));
        fs::create_dir(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        fs::copy(
            fixtures().join("fixture-trust.json"),
            path.join("trust.json"),
        )?;
        fs::set_permissions(path.join("trust.json"), fs::Permissions::from_mode(0o600))?;
        Ok(Self(path))
    }
    fn serve_args(&self) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        Ok(vec![
            "--catalog-store".into(),
            text(&self.0)?,
            "--catalog-trust".into(),
            text(&self.0.join("trust.json"))?,
        ])
    }
    fn admin(&self, args: &[String]) -> Result<Value, Box<dyn std::error::Error>> {
        let mut full = vec!["catalog".to_owned()];
        full.extend_from_slice(args);
        full.extend([
            "--store".into(),
            text(&self.0)?,
            "--trust".into(),
            text(&self.0.join("trust.json"))?,
            "--json".into(),
        ]);
        let (success, out, err) = command(&full)?;
        assert!(success, "{}", String::from_utf8_lossy(&out));
        assert!(err.is_empty());
        Ok(serde_json::from_slice(&out)?)
    }
    fn import(&self) -> TestResult {
        self.admin(&[
            "import".into(),
            text(&fixtures().join("fixture-1.tar.zst").canonicalize()?)?,
        ])?;
        Ok(())
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
fn text(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    Ok(path.to_str().ok_or("non-UTF8 fixture path")?.to_owned())
}
fn bounded_read(reader: impl Read) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > LIMIT {
        return Err(io::Error::other("harness output exceeded limit"));
    }
    Ok(bytes)
}
fn wait(child: &mut Child) -> Result<bool, Box<dyn std::error::Error>> {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("child exceeded harness deadline".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}
type CommandResult = Result<(bool, Vec<u8>, Vec<u8>), Box<dyn std::error::Error>>;
fn command(args: &[String]) -> CommandResult {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"))
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out = child.stdout.take().ok_or("stdout")?;
    let err = child.stderr.take().ok_or("stderr")?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(bounded_read(out));
    });
    let (etx, erx) = mpsc::channel();
    thread::spawn(move || {
        let _ = etx.send(bounded_read(err));
    });
    Ok((
        wait(&mut child)?,
        rx.recv_timeout(TIMEOUT)??,
        erx.recv_timeout(TIMEOUT)??,
    ))
}
struct Server {
    child: Child,
    input: Option<ChildStdin>,
    output: Receiver<io::Result<Vec<u8>>>,
    errors: Receiver<io::Result<Vec<u8>>>,
    schema: Value,
}
impl Server {
    fn start(args: &[String]) -> Result<Self, Box<dyn std::error::Error>> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"))
            .args(["serve", "--stdio"])
            .args(args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let out = child.stdout.take().ok_or("stdout")?;
        let err = child.stderr.take().ok_or("stderr")?;
        let (tx, output) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(out.take((LIMIT + 1) as u64));
            let mut total = 0;
            loop {
                let mut line = Vec::new();
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break,
                    Ok(n) => {
                        total += n;
                        if total > LIMIT || line.last() != Some(&b'\n') {
                            let _ = tx.send(Err(io::Error::other("harness framing/output limit")));
                            break;
                        }
                        if tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        break;
                    }
                }
            }
        });
        let (tx, errors) = mpsc::channel();
        thread::spawn(move || {
            let _ = tx.send(bounded_read(err));
        });
        let mut server = Self {
            input: child.stdin.take(),
            child,
            output,
            errors,
            schema: Value::Null,
        };
        server.request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"catalog-status-fixture","version":"1"}}}))?;
        server.response(1)?;
        server.request(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        server.request(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))?;
        let listing = server.response(2)?;
        let tools = listing["result"]["tools"].as_array().ok_or("tools array")?;
        assert_eq!(tools.len(), 15);
        let tool = tools
            .iter()
            .find(|t| t["name"] == "rust.catalog.status")
            .ok_or("catalog tool")?;
        let input = jsonschema::validator_for(&tool["inputSchema"])?;
        assert!(input.is_valid(&json!({})));
        assert!(!input.is_valid(&json!({"path":"/arbitrary"})));
        server.schema = tool["outputSchema"].clone();
        Ok(server)
    }
    fn request(&mut self, value: Value) -> TestResult {
        let mut input = self.input.take().ok_or("closed input")?;
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = input.write_all(&bytes).and_then(|()| input.flush());
            let _ = tx.send((input, result));
        });
        let (input, result) = rx.recv_timeout(TIMEOUT)?;
        self.input = Some(input);
        result?;
        Ok(())
    }
    fn response(&self, id: u32) -> Result<Value, Box<dyn std::error::Error>> {
        let value: Value = serde_json::from_slice(&self.output.recv_timeout(TIMEOUT)??)?;
        assert_eq!(value["jsonrpc"], "2.0");
        assert_eq!(value["id"], id);
        assert!(value.get("error").is_none(), "{value}");
        Ok(value)
    }
    fn status(&mut self, id: u32) -> Result<Value, Box<dyn std::error::Error>> {
        self.request(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"rust.catalog.status","arguments":{}}}))?;
        let response = self.response(id)?;
        assert_eq!(response["result"]["isError"], false);
        let structured = &response["result"]["structuredContent"];
        let schema = jsonschema::validator_for(&self.schema)?;
        assert!(schema.is_valid(structured), "schema rejected {structured}");
        let textual: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .ok_or("text")?,
        )?;
        assert_eq!(*structured, textual);
        assert_eq!(structured["status"], "passed");
        assert_eq!(
            structured["data"]["network"],
            json!({"acquisition_allowed":false,"enforcement":"runtime_api_disabled"})
        );
        assert_eq!(
            structured["data"]["lifecycle"],
            "session_generation_restart_to_reload"
        );
        assert_eq!(structured["data"]["semantics"], "latest_known");
        Ok(structured["data"]["context"].clone())
    }
    fn finish(mut self) -> TestResult {
        self.input.take();
        assert!(wait(&mut self.child)?);
        assert!(self.errors.recv_timeout(TIMEOUT)??.is_empty());
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn denied_if_required() {
    if std::env::var("RUST_MCP_NETWORK_DENIED").as_deref() == Ok("1") {
        assert!(std::net::TcpListener::bind("127.0.0.1:0").is_err());
        assert!(std::net::TcpListener::bind("[::1]:0").is_err());
    }
}

#[test]
fn actual_wire_catalog_status_authenticates_fixture_and_schema() -> TestResult {
    denied_if_required();
    let fixture = Fixture::new()?;
    fixture.import()?;
    let before = fs::read(fixture.0.join("active.bundle"))?;
    let mut server = Server::start(&fixture.serve_args()?)?;
    let context = server.status(3)?;
    let catalog = &context["catalog"]["value"];
    assert_eq!(context["catalog"]["status"], "available");
    assert_eq!(catalog["publisher"], "fixture-only");
    assert_eq!(catalog["channel"], "test");
    assert_eq!(catalog["sequence"], 1);
    assert_eq!(catalog["crate_count"], 1);
    assert_eq!(
        catalog["bundle_fingerprint"],
        format!(
            "sha256:{}",
            rust_engineering_catalog::bundle::sha256(&before)
        )
    );
    assert_eq!(catalog["evidence"]["provenance"]["observed_at"], 100);
    assert_eq!(
        context["model"],
        json!({"status":"unavailable","reason":"not_configured"})
    );
    assert_eq!(
        context["rustsec"],
        json!({"status":"unavailable","reason":"not_configured"})
    );
    assert_eq!(context["reservation"]["pending"], false);
    server.finish()?;
    assert_eq!(fs::read(fixture.0.join("active.bundle"))?, before);
    Ok(())
}

#[test]
fn catalog_runtime_cli_rejects_incomplete_relative_and_duplicate_authority() -> TestResult {
    for tail in [
        vec!["--catalog-store", "/private/tmp"],
        vec!["--catalog-trust", "/private/tmp/key"],
        vec!["--catalog-model-dir", "/private/tmp/model"],
        vec![
            "--catalog-store",
            "relative",
            "--catalog-trust",
            "/private/tmp/key",
        ],
        vec![
            "--catalog-store",
            "/private/tmp",
            "--catalog-trust",
            "/private/tmp/key",
            "--catalog-index-store",
            "/private/tmp/index",
        ],
        vec![
            "--catalog-store",
            "/private/tmp",
            "--catalog-store",
            "/private/tmp",
            "--catalog-trust",
            "/private/tmp/key",
        ],
    ] {
        let mut args = vec!["serve".to_owned(), "--stdio".to_owned()];
        args.extend(tail.into_iter().map(str::to_owned));
        let (success, out, err) = command(&args)?;
        assert!(!success);
        assert!(out.is_empty());
        assert_eq!(
            String::from_utf8(err)?,
            "Unsupported invocation. Use 'rust-engineering-mcp --help'.\n"
        );
    }
    Ok(())
}

#[cfg(feature = "local")]
#[test]
#[ignore = "full gate: pinned E5/ORT and inherited enforced macOS network deny"]
fn actual_wire_native_catalog_status_preserves_identity_and_degrades_independently() -> TestResult {
    assert_eq!(std::env::var("RUST_MCP_NETWORK_DENIED").as_deref(), Ok("1"));
    denied_if_required();
    let model = std::env::var("RUST_MCP_E5_DIR")?;
    let fixture = Fixture::new()?;
    fixture.import()?;
    let index = Fixture::new()?;
    fixture.admin(&[
        "rebuild-index".into(),
        "--model-dir".into(),
        model.clone(),
        "--index-store".into(),
        text(&index.0)?,
    ])?;
    let mut args = fixture.serve_args()?;
    args.extend([
        "--catalog-model-dir".into(),
        model,
        "--catalog-index-store".into(),
        text(&index.0)?,
    ]);
    let mut server = Server::start(&args)?;
    let first = server.status(3)?;
    assert_eq!(first["model"]["status"], "available");
    assert_eq!(first["semantic_index"]["status"], "available");
    let metadata = &first["semantic_index"]["value"]["metadata"];
    assert_eq!(metadata["model"], first["model"]["value"]["identity"]);
    assert_eq!(
        metadata["snapshot_fingerprint"],
        first["catalog"]["value"]["fingerprint"]
    );
    assert_eq!(
        first["semantic_index"]["value"]["documents"],
        first["catalog"]["value"]["crate_count"]
    );
    assert_eq!(
        first["model"]["value"]["identity"]["model"],
        "intfloat/multilingual-e5-small"
    );
    let original_index = fs::read(index.0.join("active.bundle"))?;
    fs::write(
        index.0.join("active.bundle"),
        b"corrupt persisted native index",
    )?;
    let retained = server.status(4)?;
    assert_eq!(retained["semantic_index"], first["semantic_index"]);
    assert_eq!(
        retained["model"]["value"]["identity"],
        first["model"]["value"]["identity"]
    );
    server.finish()?;
    let mut restarted = Server::start(&args)?;
    let degraded = restarted.status(3)?;
    assert_eq!(degraded["catalog"]["status"], "available");
    assert_eq!(degraded["model"]["status"], "available");
    assert_eq!(degraded["semantic_index"]["status"], "unavailable");
    assert_eq!(degraded["semantic_index"]["reason"], "invalid");
    restarted.finish()?;
    // Restore the intact generation-1 artifact, then activate authenticated
    // catalog generation 2. A new session must reject the stale binding even
    // though native bytes and their object hashes are internally valid.
    fs::write(index.0.join("active.bundle"), &original_index)?;
    fixture.admin(&[
        "import".into(),
        text(&fixtures().join("fixture-2.tar.zst").canonicalize()?)?,
    ])?;
    let mut mismatched = Server::start(&args)?;
    let context = mismatched.status(3)?;
    assert_eq!(context["catalog"]["status"], "available");
    assert_eq!(context["catalog"]["value"]["sequence"], 2);
    assert_eq!(context["model"]["status"], "available");
    assert_eq!(
        context["semantic_index"],
        json!({"status":"unavailable","reason":"identity_mismatch"})
    );
    mismatched.finish()?;
    let mut args = fixture.serve_args()?;
    args.extend([
        "--catalog-model-dir".into(),
        text(&fixture.0.join("missing-model"))?,
        "--catalog-index-store".into(),
        text(&index.0)?,
    ]);
    let mut missing = Server::start(&args)?;
    let context = missing.status(3)?;
    assert_eq!(context["catalog"]["status"], "available");
    assert_eq!(context["model"]["status"], "unavailable");
    assert_eq!(
        context["semantic_index"],
        json!({"status":"unavailable","reason":"dependency_unavailable"})
    );
    missing.finish()?;
    println!(
        "PASS real MCP E5/native index bindings, retained session, restart invalid/mismatched-index fallback and missing-model independence"
    );
    Ok(())
}
