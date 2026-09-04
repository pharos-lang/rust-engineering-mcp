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
#[path = "crate_search/fixture.rs"]
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
    fn import(&self) -> TestResult {
        fs::write(self.0.join("search.tar.zst"), fixture::bundle()?)?;
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
        assert_eq!(tools.len(), 13);
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
            .find(|tool| tool["name"] == "rust.crate.search")
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
    fn search(&mut self, id: u32, arguments: Value) -> Result<Value, Box<dyn std::error::Error>> {
        self.request(json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"rust.crate.search","arguments":arguments}}))?;
        let response = self.response(id)?;
        assert!(serde_json::to_vec(&response["result"])?.len() <= 512 * 1024);
        assert_eq!(response["result"]["isError"], false);
        let structured = &response["result"]["structuredContent"];
        assert!(
            jsonschema::validator_for(&self.search_schema)?.is_valid(structured),
            "{structured}"
        );
        let textual: Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .ok_or("text")?,
        )?;
        assert_eq!(*structured, textual);
        assert_eq!(structured["status"], "passed");
        assert_eq!(structured["data"]["semantics"], "latest_known");
        assert_eq!(structured["data"]["coverage"], "candidate_window_only");
        assert_eq!(
            structured["data"]["advisory_interpretation"],
            "snapshot_listed_ids_only"
        );
        let result = structured["data"]["search"].clone();
        assert_eq!(result["evidence"]["provenance"]["observed_at"], 100);
        assert_eq!(result["evidence"]["provenance"]["network_used"], false);
        assert_eq!(result["evidence"]["freshness"]["state"], "stale");
        assert_eq!(result["window"]["candidate_limit_per_channel"], 50);
        assert_facts(&result)?;
        Ok(result)
    }
}
fn assert_facts(search: &Value) -> TestResult {
    let records = fixture::records();
    for row in search["results"].as_array().ok_or("results")? {
        let facts = &row["facts"];
        let source = records
            .iter()
            .find(|c| facts["name"] == c.name)
            .ok_or("non-SQLite identity")?;
        assert_eq!(facts["description"], source.description);
        assert_eq!(
            facts["repository"],
            serde_json::to_value(&source.repository)?
        );
        assert_eq!(facts["version_count"], source.versions.len());
        let selected = &facts["selected_version"];
        let version = source
            .versions
            .iter()
            .find(|v| selected["version"] == v.version)
            .ok_or("non-SQLite version")?;
        assert_eq!(selected["yanked"], version.yanked);
        assert_eq!(
            selected["rust_version"],
            serde_json::to_value(&version.rust_version)?
        );
        assert_eq!(selected["license"], serde_json::to_value(&version.license)?);
        assert_eq!(
            selected["published_at"],
            serde_json::to_value(version.published_at)?
        );
        assert_eq!(
            selected["known_advisory_ids"],
            serde_json::to_value(&version.advisories)?
        );
        if !row["lexical"].is_null() {
            assert!(row["lexical"]["bm25"].as_f64().ok_or("bm25")?.is_finite());
        }
        if !row["semantic"].is_null() {
            let score = row["semantic"]["squared_l2"].as_f64().ok_or("distance")?;
            assert!(score.is_finite() && score >= 0.0);
        }
    }
    Ok(())
}
fn names(search: &Value) -> Result<std::collections::BTreeSet<String>, Box<dyn std::error::Error>> {
    search["results"]
        .as_array()
        .ok_or("results")?
        .iter()
        .map(|r| Ok(r["facts"]["name"].as_str().ok_or("name")?.to_owned()))
        .collect()
}
fn choice(search: &Value) -> Result<&Value, &'static str> {
    search["results"]
        .as_array()
        .ok_or("results")?
        .iter()
        .find(|r| r["facts"]["name"] == "choice")
        .map(|r| &r["facts"])
        .ok_or("choice")
}
fn request(query: &str, mode: &str) -> Value {
    json!({"query":query,"mode":mode,"limit":50,"filters":{"msrv_lte":"1.70"}})
}

#[test]
fn actual_wire_lexical_filters_and_semantic_fallback_use_identical_sqlite_facts() -> TestResult {
    denied_if_required();
    let fixture = Fixture::new()?;
    fixture.import()?;
    let mut server = Server::start(&fixture.serve_args()?)?;
    let lexical = server.search(3, request("parser", "lexical"))?;
    assert_eq!(lexical["effective_mode"], "lexical");
    assert!(lexical["fallback"].is_null());
    assert_eq!(
        names(&lexical)?,
        ["alpha", "beta", "choice"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
    assert_eq!(choice(&lexical)?["selected_version"]["version"], "1.0.0");
    assert_eq!(choice(&lexical)?["latest_known_stable"]["version"], "3.0.0");
    assert_eq!(choice(&lexical)?["latest_known_stable"]["yanked"], true);
    for (id, mode) in [(4, "hybrid"), (5, "semantic")] {
        let fallback = server.search(id, request("parser", mode))?;
        assert_eq!(fallback["requested_mode"], mode);
        assert_eq!(fallback["effective_mode"], "lexical");
        assert_eq!(
            fallback["fallback"],
            json!({"kind":"unavailable","component":"model","reason":"not_configured"})
        );
        assert_eq!(fallback["results"], lexical["results"]);
    }
    let defaults = server.search(6, json!({"query":"parser","mode":"lexical"}))?;
    assert_eq!(choice(&defaults)?["selected_version"]["version"], "2.0.0");
    assert!(names(&defaults)?.contains("unknown"));
    assert!(names(&defaults)?.contains("unstable"));
    assert!(!names(&defaults)?.contains("preview"));
    let relaxed=server.search(7,json!({"query":"parser","mode":"lexical","filters":{"msrv_lte":"1.70","allow_yanked":true,"include_prerelease":true}}))?;
    assert_eq!(
        choice(&relaxed)?["selected_version"]["version"],
        "4.0.0-alpha"
    );
    assert!(names(&relaxed)?.contains("preview"));
    let status = server.status(8)?;
    assert_eq!(
        status["catalog"]["value"]["fingerprint"],
        lexical["snapshot_fingerprint"]
    );
    server.finish()?;
    Ok(())
}

#[test]
fn actual_wire_search_rejects_query_and_filter_authority_extensions() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.import()?;
    let mut server = Server::start(&fixture.serve_args()?)?;
    let input = jsonschema::validator_for(&server.search_input)?;
    assert!(input.is_valid(&json!({"query":"parser"})));
    for (i, args) in [
        json!({"query":"parser","path":"/untrusted"}),
        json!({"query":"parser","filters":{"sql":"1=1"}}),
        json!({"query":"parser","limit":0}),
        json!({"query":"parser","limit":51}),
        json!({"query":"parser","mode":"remote"}),
        json!({"query":"parser","filters":{"msrv_lte":"01.70"}}),
    ]
    .into_iter()
    .enumerate()
    {
        assert!(!input.is_valid(&args));
        server.request(json!({"jsonrpc":"2.0","id":i+3,"method":"tools/call","params":{"name":"rust.crate.search","arguments":args}}))?;
        let value: Value = serde_json::from_slice(&server.output.recv_timeout(TIMEOUT)??)?;
        assert_eq!(value["id"], i + 3);
        assert_eq!(value["error"]["code"], -32602);
        assert!(value.get("result").is_none());
    }
    for (i, query) in [
        " ".to_owned(),
        "bad\nquery".to_owned(),
        "é".repeat(129),
        vec!["term"; 17].join(" "),
    ]
    .into_iter()
    .enumerate()
    {
        server.request(json!({"jsonrpc":"2.0","id":i+20,"method":"tools/call","params":{"name":"rust.crate.search","arguments":{"query":query}}}))?;
        let value: Value = serde_json::from_slice(&server.output.recv_timeout(TIMEOUT)??)?;
        assert_eq!(value["id"], i + 20);
        assert_eq!(value["error"]["code"], -32602);
    }
    server.search(30, request("parser", "lexical"))?;
    server.finish()?;
    Ok(())
}

#[cfg(feature = "local")]
#[test]
#[ignore = "full gate: actual E5/native Lance objects and inherited macOS network deny"]
fn actual_wire_all_search_modes_and_native_fallback_are_bound_to_sqlite() -> TestResult {
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
    let mut id = 3;
    for query in ["parser", "normalización Unicode"] {
        let lexical = server.search(id, request(query, "lexical"))?;
        id += 1;
        let semantic = server.search(id, request(query, "semantic"))?;
        id += 1;
        let hybrid = server.search(id, request(query, "hybrid"))?;
        id += 1;
        assert_eq!(semantic["effective_mode"], "semantic");
        assert_eq!(hybrid["effective_mode"], "hybrid");
        assert!(semantic["fallback"].is_null());
        assert!(hybrid["fallback"].is_null());
        assert!(
            semantic["results"]
                .as_array()
                .ok_or("results")?
                .iter()
                .all(|r| r["lexical"].is_null() && !r["semantic"].is_null())
        );
        assert!(
            names(&semantic)?
                .difference(&names(&lexical)?)
                .next()
                .is_some()
        );
        let union = names(&lexical)?
            .union(&names(&semantic)?)
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names(&hybrid)?, union);
        let mut previous: Option<(f64, String)> = None;
        for row in hybrid["results"].as_array().ok_or("results")? {
            let mut expected = 0.0;
            for channel in ["lexical", "semantic"] {
                if let Some(rank) = row[channel]["rank"].as_u64() {
                    expected += 1.0 / (60.0 + rank as f64);
                }
            }
            let actual = row["fusion_score"].as_f64().ok_or("fusion")?;
            assert!((actual - expected).abs() < 1e-12);
            let name = row["facts"]["name"].as_str().ok_or("name")?.to_owned();
            if let Some((prior, prior_name)) = &previous {
                assert!(*prior > actual || (*prior == actual && prior_name < &name));
            }
            previous = Some((actual, name));
        }
        let repeated = server.search(id, request(query, "hybrid"))?;
        id += 1;
        assert_eq!(repeated["results"], hybrid["results"]);
        let status = server.status(id)?;
        id += 1;
        assert_eq!(
            status["catalog"]["value"]["fingerprint"],
            hybrid["snapshot_fingerprint"]
        );
        assert_eq!(
            status["model"]["value"]["identity"],
            hybrid["semantic_index"]["model"]
        );
    }
    server.finish()?;
    fs::write(index.0.join("active.bundle"), b"corrupt native index")?;
    let mut corrupt = Server::start(&args)?;
    let lexical = corrupt.search(3, request("parser", "lexical"))?;
    let fallback = corrupt.search(4, request("parser", "semantic"))?;
    assert_eq!(
        fallback["fallback"],
        json!({"kind":"unavailable","component":"semantic_index","reason":"invalid"})
    );
    assert_eq!(fallback["effective_mode"], "lexical");
    assert_eq!(fallback["results"], lexical["results"]);
    corrupt.finish()?;
    let mut absent = Server::start(&fixture.serve_args()?)?;
    let fallback = absent.search(3, request("parser", "hybrid"))?;
    assert_eq!(
        fallback["fallback"],
        json!({"kind":"unavailable","component":"model","reason":"not_configured"})
    );
    assert_eq!(fallback["results"], lexical["results"]);
    absent.finish()?;
    println!(
        "PASS actual E5/native lexical/semantic/hybrid ES+EN boundaries and SQLite-filtered fallbacks; no retrieval-quality claim"
    );
    Ok(())
}
