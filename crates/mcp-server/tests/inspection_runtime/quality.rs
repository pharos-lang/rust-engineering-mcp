//! Actual composed MCP execution; fixtures are only executed by the contained gateway.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

const RSA: &str =
    include_str!("../../../catalog-adapter/tests/fixtures/rustsec/RUSTSEC-2023-0071.md");
const PASS: &str = "pub fn answer() -> i32 {\n    42\n}\n\n#[test]\nfn answer_is_correct() {\n    assert_eq!(answer(), 42);\n}\n";

fn fingerprint(bytes: &[u8]) -> String {
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn prepare(fixture: &Fixture) -> Result<BTreeMap<String, Vec<u8>>> {
    fs::write(
        fixture.project.join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\"]\nexclude = [\"helper\"]\nresolver = \"3\"\n",
    )?;
    fs::write(
        fixture.project.join("app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    fs::write(
        fixture.project.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(fixture.project.join("app/src/lib.rs"), PASS)?;
    fixture.source_bytes()
}

fn bootstrap(server: &mut Server, fixture: &Fixture) -> Result<(Value, Value)> {
    let (opened, _) = server.bootstrap_open(fixture)?;
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("tools missing")?
        .iter()
        .find(|tool| tool["name"] == "rust.quality.gate")
        .ok_or("quality gate missing")?
        .clone();
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    Ok((opened, tool))
}

fn run(
    server: &mut Server,
    tool: &Value,
    opened: &Value,
    id: i64,
    profile: &str,
    statuses: &[&str],
    overall: &str,
) -> Result<Value> {
    server.send(call(
        id,
        "rust.quality.gate",
        json!({"project_ref":opened["project_ref"],"profile":profile}),
    ))?;
    let response = server.receive(id, JOIN_TIMEOUT)?;
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(
        response["result"]["isError"],
        matches!(overall, "blocked" | "unavailable"),
        "{response}"
    );
    let output = response["result"]["structuredContent"].clone();
    let fallback: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .ok_or("fallback missing")?,
    )?;
    assert_eq!(fallback, output);
    jsonschema::validator_for(&tool["outputSchema"])?
        .validate(&output)
        .map_err(|error| error.to_string())?;
    assert_eq!(output["status"], overall, "{output}");
    let data = &output["data"];
    assert_eq!(data["project_ref"], opened["project_ref"]);
    assert_eq!(data["project_identity_fingerprint"], opened["fingerprint"]);
    assert_eq!(data["profile"], profile);
    assert_eq!(data["semantics"], "latest_known");
    assert_fingerprint(&data["source_fingerprint"]);
    let stages = data["stages"].as_array().ok_or("stages missing")?;
    assert_eq!(stages.len(), statuses.len());
    assert_eq!(stages.len(), if profile == "fast" { 3 } else { 5 });
    let selections = [
        ("format", "format_all"),
        ("check", "check_cargo_defaults"),
        ("clippy", "clippy_strict_cargo_defaults"),
        ("test", "test_cargo_defaults_30_seconds"),
        ("audit", "audit_captured_lockfile"),
    ];
    for ((stage, status), (name, selection)) in stages.iter().zip(statuses).zip(selections) {
        assert_eq!(stage["stage"], name);
        assert_eq!(stage["status"], *status, "{output}");
        assert_eq!(stage["applied_selection"], selection);
        assert!(stage["duration_ms"].is_u64());
        if name != "audit" {
            let execution = &stage["execution"];
            assert_eq!(execution["source_fingerprint"], data["source_fingerprint"]);
            assert_eq!(execution["validation_complete"], true, "{output}");
            assert_eq!(execution["runtime"]["image_id"], APPROVED_RUST_IMAGE);
            assert_eq!(execution["runtime"]["platform"], "linux/aarch64");
            assert_fingerprint(&execution["runtime"]["configuration_fingerprint"]);
            assert_fingerprint(&execution["runtime"]["execution_fingerprint"]);
            assert_eq!(execution["termination"], "exited");
            assert_eq!(execution["diagnostics_omitted"], 0);
        }
    }
    Ok(output)
}

fn logs(server: &mut Server, output: &Value, first_id: i64) -> Result<Vec<Vec<u8>>> {
    let mut retained = Vec::new();
    for (index, stage) in output["data"]["stages"]
        .as_array()
        .ok_or("stages missing")?
        .iter()
        .enumerate()
    {
        if stage["stage"] == "audit" {
            assert!(stage["log"].is_null());
            continue;
        }
        let log = &stage["log"];
        let uri = log["uri"].as_str().ok_or("stage log missing")?;
        assert!(uri.starts_with(&format!(
            "rust-artifact://{}/",
            output["data"]["project_ref"].as_str().ok_or("owner missing")?
        )));
        server.send(resource_read_request(first_id + i64::try_from(index)?, uri))?;
        let response = server.receive(first_id + i64::try_from(index)?, DISCOVERY_TIMEOUT)?;
        assert!(response.get("error").is_none(), "{response}");
        let resource = &response["result"];
        assert_eq!(resource["resultType"], "complete");
        assert_eq!(resource["cacheScope"], "private");
        assert_eq!(resource["ttlMs"], 0);
        let content = &resource["contents"][0];
        assert_eq!(content["uri"], uri);
        assert_eq!(content["mimeType"], "application/octet-stream");
        let bytes = STANDARD.decode(content["blob"].as_str().ok_or("log blob missing")?)?;
        let hash = fingerprint(&bytes);
        assert_eq!(log["sha256"], &hash[7..]);
        assert_eq!(content["_meta"]["sha256"], &hash[7..]);
        assert_eq!(log["size_bytes"], bytes.len());
        assert_eq!(content["_meta"]["size_bytes"], bytes.len());
        assert_eq!(log["truncated"], false);
        assert_eq!(content["_meta"]["truncated"], false);
        let remaining = content["_meta"]["retention_remaining_seconds"]
            .as_u64()
            .ok_or("resource retention missing")?;
        let initial = log["retention_remaining_seconds"]
            .as_u64()
            .ok_or("log retention missing")?;
        assert!(remaining > 0 && remaining <= initial && initial <= 3600);
        assert!(std::str::from_utf8(&bytes)?.starts_with("=== stdout ===\n"));
        retained.push(bytes);
    }
    Ok(retained)
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn quality_fast_retains_format_and_strict_clippy_failures_and_readonly_source() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let mut expected = prepare(&fixture)?;
    // Runs only inside the approved sandbox. A writable captured source would
    // replace the compilation input and make the check fail this assertion.
    let build = "fn main() {\n    assert!(std::fs::write(\"src/lib.rs\", \"compile_error!(\\\"SOURCE_MUTATED\\\");\").is_err());\n}\n";
    fs::write(fixture.project.join("app/build.rs"), build)?;
    expected.insert("app/build.rs".into(), build.as_bytes().to_vec());
    let mut server = Server::start(&fixture)?;
    let (opened, tool) = bootstrap(&mut server, &fixture)?;
    let pass = run(
        &mut server,
        &tool,
        &opened,
        10,
        "fast",
        &["passed"; 3],
        "passed",
    )?;
    let first_logs = logs(&mut server, &pass, 20)?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    let source = "pub fn answer()->i32{let value=42;value}\n";
    fs::write(fixture.project.join("app/src/lib.rs"), source)?;
    expected.insert("app/src/lib.rs".into(), source.as_bytes().to_vec());
    let failed = run(
        &mut server,
        &tool,
        &opened,
        30,
        "fast",
        &["failed", "passed", "failed"],
        "failed",
    )?;
    let stages = &failed["data"]["stages"];
    assert_eq!(
        stages[0]["format"]["affected_files"],
        json!(["app/src/lib.rs"])
    );
    assert!(
        stages[0]["format"]["diff"]
            .as_str()
            .is_some_and(|diff| diff.contains("answer"))
    );
    assert!(
        stages[2]["execution"]["diagnostics"]
            .as_array()
            .ok_or("diagnostics missing")?
            .iter()
            .any(|diagnostic| diagnostic.to_string().contains("let_and_return"))
    );
    assert_ne!(
        pass["data"]["source_fingerprint"],
        failed["data"]["source_fingerprint"]
    );
    let failed_logs = logs(&mut server, &failed, 40)?;
    // Publishing a second group preserves the first group while its lease lives.
    assert_eq!(logs(&mut server, &pass, 50)?, first_logs);
    server.finish()?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_QUALITY_RECEIPT {}",
        json!({"cases":2,"profiles":["fast"],"ordered_stages":3,"logs_sha256_verified":first_logs.len()+failed_logs.len(),"prior_log_group_retained":true,"format_diff_and_clippy_diagnostic_retained":true,"ordinary_failures_continue":true,"captured_source_write_denied":true,"source_unchanged_between_explicit_mutations":true,"cleanup":true,"configuration_fingerprint":pass["data"]["stages"][1]["execution"]["runtime"]["configuration_fingerprint"]})
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn quality_standard_distinguishes_pass_test_failure_and_unavailable_audit() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let mut expected = prepare(&fixture)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let snapshot = serde_json::to_vec(&json!({"format_version":1,"sequence":1,
        "source_id":"fixture-quality-rsa-not-publisher-authenticated",
        "created_at":now,"observed_at":now,"records":[{
            "path":"crates/rsa/RUSTSEC-2023-0071.md","markdown":RSA}]}))?;
    let snapshot_path = fixture.root.join("rustsec.json");
    fs::write(&snapshot_path, &snapshot)?;
    let snapshot_hash = fingerprint(&snapshot);
    let mut server = audit_runtime::start(&fixture, Some((&snapshot_path, &snapshot_hash)))?;
    let (opened, tool) = bootstrap(&mut server, &fixture)?;
    let passed = run(
        &mut server,
        &tool,
        &opened,
        10,
        "standard",
        &["passed"; 5],
        "passed",
    )?;
    let audit = &passed["data"]["stages"][4]["audit"]["observation"];
    assert_eq!(audit["snapshot_fingerprint"], snapshot_hash);
    assert_eq!(audit["snapshot_record_count"], 1);
    assert_eq!(
        audit["lock_fingerprint"],
        fingerprint(&expected["Cargo.lock"])
    );
    assert_eq!(audit["packages_total"], 1);
    assert_eq!(audit["workspace_packages_excluded"], 1);
    assert_eq!(audit["crates_io_scanned"], 0);
    assert_eq!(audit["findings"], json!([]));
    assert_eq!(audit["validation_complete"], true);
    assert_eq!(audit["snapshot"]["freshness"]["state"], "fresh");
    assert_eq!(audit["snapshot"]["provenance"]["integrity"], "verified");
    assert_eq!(audit["snapshot"]["provenance"]["network_used"], false);
    let mut log_count = logs(&mut server, &passed, 20)?.len();
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    let failing = "#[test]\nfn retained_test_failure() {\n    assert_eq!(std::hint::black_box(2 + 2), 5);\n}\n";
    fs::write(fixture.project.join("app/src/lib.rs"), failing)?;
    expected.insert("app/src/lib.rs".into(), failing.as_bytes().to_vec());
    let failed = run(
        &mut server,
        &tool,
        &opened,
        30,
        "standard",
        &["passed", "passed", "passed", "failed", "passed"],
        "failed",
    )?;
    assert_eq!(failed["data"]["stages"][3]["test"]["build_succeeded"], true);
    let failed_logs = logs(&mut server, &failed, 40)?;
    assert!(std::str::from_utf8(&failed_logs[3])?.contains("retained_test_failure"));
    assert!(std::str::from_utf8(&failed_logs[3])?.contains("FAILED"));
    log_count += failed_logs.len();
    server.finish()?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    fs::write(fixture.project.join("app/src/lib.rs"), PASS)?;
    expected.insert("app/src/lib.rs".into(), PASS.as_bytes().to_vec());
    let mut server = Server::start(&fixture)?;
    let (opened, tool) = bootstrap(&mut server, &fixture)?;
    let missing = run(
        &mut server,
        &tool,
        &opened,
        10,
        "standard",
        &["passed", "passed", "passed", "passed", "unavailable"],
        "unavailable",
    )?;
    assert_eq!(
        missing["data"]["stages"][4]["audit"]["observation"]["issue"],
        "snapshot_unavailable"
    );
    assert_eq!(
        missing["data"]["stages"][4]["audit"]["observation"]["validation_complete"],
        false
    );
    log_count += logs(&mut server, &missing, 20)?.len();
    server.finish()?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;
    assert_eq!(fs::read(snapshot_path)?, snapshot);
    println!(
        "M1_QUALITY_RECEIPT {}",
        json!({"cases":3,"profiles":["standard"],"ordered_stages":5,"logs_sha256_verified":log_count,"real_test_failure_retained":true,"audit_after_failed_test":true,"missing_snapshot_never_passes":true,"snapshot_fingerprint":snapshot_hash,"nonmatching_real_rsa_snapshot":true,"source_unchanged_between_explicit_mutations":true,"cleanup":true,"configuration_fingerprint":passed["data"]["stages"][1]["execution"]["runtime"]["configuration_fingerprint"]})
    );
    fixture.successful = true;
    Ok(())
}

fn active_quality_test(server: &mut Server, fixture: &Fixture, id: i64) -> Result<String> {
    // The earlier three stages run before Cargo test. Observe the actual test
    // process, rather than treating a calibration/staging container as execution.
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        server.assert_no_response(id)?;
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
                return Err("missing quality job command observation".into());
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
                line.split_whitespace()
                    .any(|part| part.starts_with("/work/target/debug/deps/app-"))
                    && line.split_whitespace().any(|arg| arg == "--test-threads=1")
            }) {
                server.assert_no_response(id)?;
                return Ok(nonce.to_owned());
            }
        }
        if Instant::now() >= deadline {
            return Err("quality test binary was never observed before cancellation".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn quality_cancellation_and_eof_join_the_active_test_stage() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let mut expected = prepare(&fixture)?;
    let mut server = Server::start(&fixture)?;
    let (opened, tool) = bootstrap(&mut server, &fixture)?;
    let initial = run(
        &mut server,
        &tool,
        &opened,
        10,
        "fast",
        &["passed"; 3],
        "passed",
    )?;
    let retained = logs(&mut server, &initial, 20)?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    let sleeping = "#[test]\nfn active_quality_test_binary() {\n    std::thread::sleep(std::time::Duration::from_secs(60));\n}\n";
    fs::write(fixture.project.join("app/src/lib.rs"), sleeping)?;
    expected.insert("app/src/lib.rs".into(), sleeping.as_bytes().to_vec());
    let arguments = json!({"project_ref":opened["project_ref"],"profile":"standard"});
    server.send(call(30, "rust.quality.gate", arguments.clone()))?;
    let cancelled_job = active_quality_test(&mut server, &fixture, 30)?;
    let observed_at = Instant::now();
    server.send(request(31, "tools/list"))?;
    assert!(server.receive(31, DISCOVERY_TIMEOUT)?["result"]["tools"].is_array());
    assert!(observed_at.elapsed() < Duration::from_secs(5));
    server.send(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":30}}),
    )?;
    let cleanup_deadline = Instant::now() + JOIN_TIMEOUT;
    loop {
        server.assert_no_response(30)?;
        if fixture
            .objects("container", Some(&cancelled_job))?
            .is_empty()
            && fixture.objects("volume", Some(&cancelled_job))?.is_empty()
        {
            break;
        }
        if Instant::now() >= cleanup_deadline {
            return Err("cancelled quality test retained owned objects".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;
    // Rollback/cancellation must not delete logs from a prior completed batch.
    assert_eq!(logs(&mut server, &initial, 40)?, retained);
    server.assert_no_response(30)?;

    server.send(call(50, "rust.quality.gate", arguments))?;
    let eof_job = active_quality_test(&mut server, &fixture, 50)?;
    assert_ne!(cancelled_job, eof_job);
    server.assert_no_response(30)?;
    server.assert_no_response(50)?;
    // finish closes stdin and joins process exit plus protocol pipe closure.
    let exit = server.finish();
    let clean = fixture
        .assert_clean(Some(&eof_job))
        .and_then(|()| fixture.assert_clean(None));
    exit?;
    clean?;
    server.assert_no_response(30)?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_QUALITY_RECEIPT {}",
        json!({"cases":3,"initial_fast_pass":true,"active_quality_test_binaries_observed":2,"cancelled_response_suppressed":true,"prior_log_group_survives_cancel":true,"worker_reused_after_joined_cleanup":true,"eof_joined_shutdown":true,"source_unchanged_between_explicit_mutations":true,"cleanup":true})
    );
    fixture.successful = true;
    Ok(())
}
