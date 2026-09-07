//! M3-01 composed MCP qualification for the synchronous nextest vertical.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

pub(super) fn prepare(fixture: &Fixture, source: &str) -> Result {
    fs::write(
        fixture.project.join("Cargo.toml"),
        "[workspace]\nmembers=[\"app\"]\nexclude=[\"helper\"]\nresolver=\"3\"\n",
    )?;
    fs::write(
        fixture.project.join("app/Cargo.toml"),
        "[package]\nname=\"nextest-mcp-fixture\"\nversion=\"0.1.0\"\nedition=\"2024\"\n",
    )?;
    fs::write(
        fixture.project.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"nextest-mcp-fixture\"\nversion = \"0.1.0\"\n",
    )?;
    fs::write(fixture.project.join("app/src/lib.rs"), source)?;
    Ok(())
}

pub(super) fn fixture_source(name: &str) -> Result<String> {
    Ok(fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../fixtures/nextest/{name}/src/lib.rs")),
    )?)
}

fn nextest_tool(server: &mut Server) -> Result<Value> {
    server.send(request(3, "tools/list"))?;
    let listed = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tools = listed["result"]["tools"]
        .as_array()
        .ok_or("tools missing")?;
    // Coverage is concurrently integrated after the 19-tool nextest cut.
    assert_eq!(tools.len(), 22);
    tools
        .iter()
        .find(|tool| tool["name"] == "rust.test.nextest")
        .cloned()
        .ok_or_else(|| "nextest tool missing".into())
}

fn read_artifacts(server: &mut Server, output: &Value, first_id: i64) -> Result<usize> {
    let artifacts = output["data"]["artifacts"]
        .as_array()
        .ok_or("artifacts missing")?;
    let mut quality_job = None;
    let mut quality_owner = None;
    for (offset, artifact) in artifacts.iter().enumerate() {
        let uri = artifact["uri"].as_str().ok_or("artifact URI")?;
        assert!(uri.starts_with("rust-quality-artifact://"), "{artifact}");
        server.send(resource_read_request(
            first_id + i64::try_from(offset)?,
            uri,
        ))?;
        let response = server.receive(first_id + i64::try_from(offset)?, DISCOVERY_TIMEOUT)?;
        assert!(response.get("error").is_none(), "{response}");
        assert_eq!(response["result"]["cacheScope"], "private");
        let content = &response["result"]["contents"][0];
        let bytes = STANDARD.decode(content["blob"].as_str().ok_or("artifact blob")?)?;
        let hash = Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(artifact["sha256"], hash);
        assert_eq!(artifact["size_bytes"], bytes.len());
        assert_eq!(content["_meta"]["size_bytes"], bytes.len());
        quality_job = content["_meta"]["job_id"].as_str().map(str::to_owned);
        quality_owner = uri
            .strip_prefix("rust-quality-artifact://")
            .and_then(|rest| rest.split_once('/'))
            .map(|(owner, _)| owner.to_owned());
    }
    let job = quality_job.ok_or("quality job id")?;
    let owner = quality_owner.ok_or("quality owner")?;
    let index_id = first_id + i64::try_from(artifacts.len())?;
    server.send(resource_read_request(
        index_id,
        &format!("rust-quality-artifact://{owner}/{job}"),
    ))?;
    let index = server.receive(index_id, DISCOVERY_TIMEOUT)?;
    assert!(index.get("error").is_none(), "{index}");
    assert_eq!(index["result"]["cacheScope"], "private");
    let page: Value = serde_json::from_str(
        index["result"]["contents"][0]["text"]
            .as_str()
            .ok_or("quality index")?,
    )?;
    assert_eq!(page["job_id"], job);
    assert_eq!(
        page["members"].as_array().map(Vec::len),
        Some(artifacts.len())
    );
    Ok(artifacts.len())
}

fn run(
    server: &mut Server,
    tool: &Value,
    project_ref: &Value,
    id: i64,
    retries: u8,
) -> Result<Value> {
    let arguments = json!({
        "project_ref": project_ref,
        "execution_mode": "synchronous",
        "timeout_seconds": 60,
        "retries": retries
    });
    jsonschema::validator_for(&tool["inputSchema"])?
        .validate(&arguments)
        .map_err(|error| error.to_string())?;
    server.send(call(id, "rust.test.nextest", arguments))?;
    let response = server.receive(id, JOIN_TIMEOUT)?;
    assert!(response.get("error").is_none(), "{response}");
    let output = response["result"]["structuredContent"].clone();
    let fallback: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .ok_or("nextest text fallback")?,
    )?;
    assert_eq!(fallback, output);
    jsonschema::validator_for(&tool["outputSchema"])?
        .validate(&output)
        .map_err(|error| error.to_string())?;
    assert_eq!(&output["data"]["project_ref"], project_ref, "{output}");
    assert_eq!(output["data"]["profile"], "rust-mcp");
    assert_eq!(output["data"]["doctests_run"], false);
    assert_eq!(output["data"]["runtime"]["image_id"], APPROVED_RUST_IMAGE);
    Ok(output)
}

pub(super) fn observe_active_nextest(
    server: &mut Server,
    fixture: &Fixture,
    request_id: i64,
) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        server.assert_no_response(request_id)?;
        for record in fixture.objects("container", None)? {
            let Some((name, command)) = record.split_once('\t') else {
                continue;
            };
            if command.contains("/opt/rust/bin/cargo nextest run")
                && let Some(nonce) = name.strip_prefix("rust-mcp-cargo-")
            {
                return Ok(nonce.to_owned());
            }
        }
        if Instant::now() >= deadline {
            return Err("active cargo-nextest child was not observed".into());
        }
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run serially by M3 gate"]
fn synchronous_passing_failing_ignored_doc_only_and_no_tests_are_observable() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    prepare(&fixture, &fixture_source("passing")?)?;
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let tool = nextest_tool(&mut server)?;
    let cases = [
        ("passing", "passed", 3_u64, 3_u64, 0_u64, 0_u64),
        ("failing", "failed", 3, 2, 1, 0),
        ("ignored", "failed", 3, 2, 0, 1),
        ("doc-only", "failed", 0, 0, 0, 0),
        ("no-tests", "failed", 0, 0, 0, 0),
    ];
    let mut artifacts = 0;
    for (index, (name, status, selected, passed, failed, ignored)) in cases.into_iter().enumerate()
    {
        prepare(&fixture, &fixture_source(name)?)?;
        let id = 10 + i64::try_from(index)? * 10;
        let output = run(&mut server, &tool, &opened["project_ref"], id, 0)?;
        assert_eq!(output["status"], status, "{name}: {output}");
        assert_eq!(
            output["data"]["counts"]["selected"], selected,
            "{name}: {output}"
        );
        assert_eq!(
            output["data"]["counts"]["passed"], passed,
            "{name}: {output}"
        );
        assert_eq!(
            output["data"]["counts"]["failed"], failed,
            "{name}: {output}"
        );
        assert_eq!(
            output["data"]["counts"]["ignored"], ignored,
            "{name}: {output}"
        );
        artifacts += read_artifacts(&mut server, &output, id + 1)?;
        fixture.assert_clean(None)?;
    }
    server.finish()?;
    fixture.assert_clean(None)?;
    println!(
        "M3_NEXTEST_COUNTS_RECEIPT {}",
        json!({"cases":5,"resources_verified":artifacts,"cleanup":true,"image_id":APPROVED_RUST_IMAGE})
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run serially by M3 gate"]
fn synchronous_flaky_and_leaky_are_derived_from_junit() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    prepare(&fixture, &fixture_source("flaky")?)?;
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let tool = nextest_tool(&mut server)?;
    let flaky = run(&mut server, &tool, &opened["project_ref"], 10, 1)?;
    assert_eq!(flaky["status"], "passed", "{flaky}");
    assert_eq!(flaky["data"]["counts"]["flaky"], 1);
    assert_eq!(flaky["data"]["counts"]["retried"], 1);
    assert_eq!(flaky["data"]["tests"][0]["attempts"], 2);
    prepare(&fixture, &fixture_source("leaky")?)?;
    let leaky = run(&mut server, &tool, &opened["project_ref"], 20, 0)?;
    assert_eq!(leaky["status"], "failed", "{leaky}");
    assert_eq!(leaky["data"]["counts"]["leaked"], 1);
    server.finish()?;
    fixture.assert_clean(None)?;
    println!(
        "M3_NEXTEST_RETRY_LEAK_RECEIPT {}",
        json!({"cases":2,"junit_derived":true,"cleanup":true,"image_id":APPROVED_RUST_IMAGE})
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run serially by M3 gate"]
fn hostile_output_is_bounded_forged_markers_are_ignored_and_source_is_immutable() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    prepare(&fixture, &fixture_source("hostile-output")?)?;
    let expected = fixture.source_bytes()?;
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let tool = nextest_tool(&mut server)?;
    let output = run(&mut server, &tool, &opened["project_ref"], 10, 0)?;
    assert_ne!(
        output["status"], "passed",
        "truncated evidence must not pass"
    );
    assert_eq!(output["data"]["counts"]["selected"], 0);
    assert_eq!(output["data"]["counts"]["failed"], 0);
    assert!(
        output["data"]["omissions"]["stdout_truncated"] == true
            || output["data"]["omissions"]["stderr_truncated"] == true
    );
    read_artifacts(&mut server, &output, 11)?;
    assert_eq!(fixture.source_bytes()?, expected);
    assert!(!fixture.project.join("target").exists());
    assert!(!fixture.project.join("app/target").exists());
    server.finish()?;
    fixture.assert_clean(None)?;
    println!(
        "M3_NEXTEST_HOSTILE_RECEIPT {}",
        json!({"forged_markers_ignored":true,"bounded":true,"source_immutable":true,"cleanup":true,"image_id":APPROVED_RUST_IMAGE})
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run serially by M3 gate"]
fn slow_timeout_cancellation_and_eof_observe_active_children_and_join_cleanup() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime test lock poisoned")?;
    let slow = fixture_source("slow")?;
    let mut observed = Vec::new();

    // Gateway timeout returns bounded partial evidence only after cleanup.
    {
        let mut fixture = Fixture::new()?;
        prepare(&fixture, &slow)?;
        let mut server = Server::start(&fixture)?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        server.send(call(
            10,
            "rust.test.nextest",
            json!({"project_ref":opened["project_ref"],"execution_mode":"synchronous","timeout_seconds":2}),
        ))?;
        observed.push(observe_active_nextest(&mut server, &fixture, 10)?);
        let response = server.receive(10, JOIN_TIMEOUT)?;
        assert_eq!(response["result"]["structuredContent"]["status"], "blocked");
        assert_eq!(
            response["result"]["structuredContent"]["error_code"],
            "COMMAND_TIMEOUT"
        );
        assert_eq!(
            response["result"]["structuredContent"]["data"]["termination"],
            "timed_out"
        );
        fixture.assert_clean(None)?;
        server.finish()?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
    }

    // Request cancellation is joined and rmcp suppresses the cancelled reply.
    {
        let mut fixture = Fixture::new()?;
        prepare(&fixture, &slow)?;
        let mut server = Server::start(&fixture)?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        server.send(call(
            20,
            "rust.test.nextest",
            json!({"project_ref":opened["project_ref"],"execution_mode":"synchronous","timeout_seconds":60}),
        ))?;
        observed.push(observe_active_nextest(&mut server, &fixture, 20)?);
        server.send(json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":20,"reason":"M3 cancellation oracle"}}))?;
        let deadline = Instant::now() + JOIN_TIMEOUT;
        loop {
            server.assert_no_response(20)?;
            if fixture.assert_clean(None).is_ok() {
                break;
            }
            if Instant::now() >= deadline {
                return Err("cancelled nextest objects were not joined".into());
            }
            thread::sleep(Duration::from_millis(25));
        }
        server.finish()?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
    }

    // EOF during an active run drains the worker and cleanup before exit.
    {
        let mut fixture = Fixture::new()?;
        prepare(&fixture, &slow)?;
        let mut server = Server::start(&fixture)?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        server.send(call(
            30,
            "rust.test.nextest",
            json!({"project_ref":opened["project_ref"],"execution_mode":"synchronous","timeout_seconds":60}),
        ))?;
        observed.push(observe_active_nextest(&mut server, &fixture, 30)?);
        server.finish()?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
    }
    assert_eq!(observed.len(), 3);
    println!(
        "M3_NEXTEST_LIFECYCLE_RECEIPT {}",
        json!({"active_children_observed":3,"timeout":true,"cancel":true,"eof":true,"cleanup":true,"image_id":APPROVED_RUST_IMAGE})
    );
    Ok(())
}
