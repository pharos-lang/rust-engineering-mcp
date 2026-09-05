//! Real signed SQLite and MCP search boundaries; illustrative corpus, not a utility benchmark.
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
const LIMIT: usize = 8 * 1024 * 1024;
#[path = "crate_inspect/fixture.rs"]
mod fixture;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let id = rust_engineering_project::OsReferences
            .generate()
            .map_err(|e| format!("{e:?}"))?;
        let path = PathBuf::from("/private/tmp").join(format!("crate-search-{id}"));
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
    fn import(&self, sequence: u64) -> TestResult {
        fs::write(self.0.join("search.tar.zst"), fixture::bundle(sequence)?)?;
        self.admin(&["import".into(), text(&self.0.join("search.tar.zst"))?])?;
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
    search_schema: Value,
    search_input: Value,
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
            search_schema: Value::Null,
            search_input: Value::Null,
        };
        server.request(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"catalog-status-fixture","version":"1"}}}))?;
        server.response(1)?;
        server.request(json!({"jsonrpc":"2.0","method":"notifications/initialized"}))?;
        server.request(json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}))?;
        let listing = server.response(2)?;
        let tools = listing["result"]["tools"].as_array().ok_or("tools array")?;
        assert_eq!(tools.len(), 18);
        let tool = tools
            .iter()
            .find(|t| t["name"] == "rust.catalog.status")
            .ok_or("catalog tool")?;
        let input = jsonschema::validator_for(&tool["inputSchema"])?;
        assert!(input.is_valid(&json!({})));
        assert!(!input.is_valid(&json!({"path":"/arbitrary"})));
        server.schema = tool["outputSchema"].clone();
        let search = tools
            .iter()
            .find(|tool| tool["name"] == "rust.crate.inspect")
            .ok_or("search tool")?;
        server.search_schema = search["outputSchema"].clone();
        server.search_input = search["inputSchema"].clone();
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

impl Server {
    fn inspect(&mut self, id: u32, args: Value) -> Result<Value, Box<dyn std::error::Error>> {
        self.request(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"rust.crate.inspect","arguments":args}}))?;
        let response = self.response(id)?;
        assert!(serde_json::to_vec(&response["result"])?.len() <= 512 * 1024);
        let structured = &response["result"]["structuredContent"];
        assert!(
            jsonschema::validator_for(&self.search_schema)?.is_valid(structured),
            "{structured}"
        );
        let text: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .ok_or("text")?,
        )?;
        assert_eq!(*structured, text);
        Ok(structured.clone())
    }
}
fn data(output: &Value) -> &Value {
    assert_eq!(output["status"], "passed", "{output}");
    assert_eq!(output["data"]["semantics"], "latest_known");
    assert_eq!(
        output["data"]["advisory_interpretation"],
        "snapshot_listed_ids_only"
    );
    &output["data"]["inspection"]
}
#[test]
fn actual_cli_mcp_paginates_exact_sqlite_sections_and_preserves_unknowns() -> TestResult {
    denied_if_required();
    let fixture = Fixture::new()?;
    fixture.import(1)?;
    let mut server = Server::start(&fixture.serve_args()?)?;
    let first = server.inspect(3, json!({"name":"choice","version":"1.0.0"}))?;
    let first = data(&first);
    let fingerprint = first["snapshot_fingerprint"].clone();
    let page = &first["lookup"]["page"];
    assert_eq!(page["overview"]["latest_known_stable"]["version"], "3.0.0");
    assert_eq!(page["overview"]["latest_known_stable"]["yanked"], true);
    assert_eq!(page["data"]["selected_version"]["version"], "1.0.0");
    assert_eq!(page["data"]["selected_version"]["feature_count"], 3);
    for key in ["documentation", "source"] {
        assert_eq!(
            page["overview"][key],
            json!({"status":"unknown","reason":"not_recorded_in_snapshot"})
        );
    }
    assert_eq!(first["evidence"]["freshness"]["state"], "stale");
    for (section, version, expected) in [
        (
            "versions",
            None,
            json!(["4.0.0-alpha", "3.0.0", "2.0.0", "1.0.0"]),
        ),
        ("features", Some("1.0.0"), json!(["alpha", "beta", "gamma"])),
        (
            "dependencies",
            Some("1.0.0"),
            json!(["alpha:build", "alpha:normal", "beta:dev"]),
        ),
        (
            "advisories",
            Some("1.0.0"),
            json!(["RUSTSEC-2020-0001", "RUSTSEC-2020-0002"]),
        ),
    ] {
        let mut offset = 0;
        let mut values = Vec::new();
        loop {
            let output=server.inspect(4,json!({"name":"choice","section":section,"version":version,"limit":1,"offset":offset,"snapshot_fingerprint":fingerprint}))?;
            let found = data(&output);
            assert_eq!(found["snapshot_fingerprint"], fingerprint);
            let page = &found["lookup"]["page"];
            let items = page["data"]["items"].as_array().ok_or("items")?;
            assert_eq!(items.len(), 1);
            assert_eq!(page["pagination"]["offset"], offset);
            assert_eq!(page["pagination"]["returned"], 1);
            for item in items {
                values.push(match section {
                    "versions" => item["version"].clone(),
                    "dependencies" => json!(format!(
                        "{}:{}",
                        item["name"].as_str().ok_or("name")?,
                        item["kind"].as_str().ok_or("kind")?
                    )),
                    _ => item.clone(),
                });
            }
            match page["pagination"]["next_offset"].as_u64() {
                Some(next) => {
                    assert!(next > offset);
                    offset = next;
                }
                None => break,
            }
        }
        assert_eq!(Value::Array(values), expected);
    }
    for (args, kind) in [
        (json!({"name":"absent"}), "crate_not_found"),
        (
            json!({"name":"choice","version":"9.0.0"}),
            "version_not_found",
        ),
    ] {
        assert_eq!(data(&server.inspect(5, args)?)["lookup"]["kind"], kind);
    }
    assert!(data(&server.inspect(6,json!({"name":"preview"}))?)["lookup"]["page"]["overview"]["latest_known_stable"].is_null());
    let empty = server.inspect(
        7,
        json!({"name":"alpha","section":"features","version":"1.0.0"}),
    )?;
    assert_eq!(data(&empty)["lookup"]["page"]["pagination"]["total"], 0);
    assert_eq!(data(&empty)["lookup"]["page"]["data"]["items"], json!([]));
    let status = server.status(8)?;
    assert_eq!(status["catalog"]["value"]["fingerprint"], fingerprint);
    server.finish()?;
    Ok(())
}
#[test]
fn actual_mcp_continuation_detects_restart_generation_and_rejects_invalid_pages() -> TestResult {
    denied_if_required();
    let fixture = Fixture::new()?;
    fixture.import(1)?;
    let mut server = Server::start(&fixture.serve_args()?)?;
    let initial = server.inspect(3, json!({"name":"choice","section":"versions","limit":1}))?;
    let fingerprint = data(&initial)["snapshot_fingerprint"].clone();
    fixture.import(2)?;
    let args = json!({"name":"choice","section":"versions","limit":1,"offset":1,"snapshot_fingerprint":fingerprint});
    assert_eq!(
        data(&server.inspect(4, args.clone())?)["snapshot_fingerprint"],
        fingerprint
    );
    let mut fresh = Server::start(&fixture.serve_args()?)?;
    let mismatch = fresh.inspect(3, args)?;
    assert_eq!(mismatch["error_code"], "SNAPSHOT_MISMATCH");
    let current = fresh.inspect(4, json!({"name":"choice"}))?;
    assert_eq!(data(&current)["sequence"], 2);
    assert_ne!(data(&current)["snapshot_fingerprint"], fingerprint);
    for args in [
        json!({"name":"choice","section":"features"}),
        json!({"name":"choice","version":"bad version"}),
        json!({"name":"choice","section":"versions","version":"1.0.0"}),
        json!({"name":"choice","section":"versions","offset":1}),
        json!({"name":"choice","section":"versions","offset":5,"snapshot_fingerprint":data(&current)["snapshot_fingerprint"]}),
        json!({"name":"choice","path":"/tmp/other"}),
    ] {
        fresh.request(json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"rust.crate.inspect","arguments":args}}))?;
        let response: Value = serde_json::from_slice(&fresh.output.recv_timeout(TIMEOUT)??)?;
        assert_eq!(response["error"]["code"], -32602, "{response}");
    }
    server.finish()?;
    fresh.finish()?;
    Ok(())
}
