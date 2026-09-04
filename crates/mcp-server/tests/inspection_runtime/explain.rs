//! Compiler evidence via MCP without a project capability or root.
use super::*;
use sha2::{Digest, Sha256};

fn start_without_roots(fixture: &Fixture) -> Result<Server> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"))
        .env_clear()
        .current_dir(&fixture.root)
        .args(["serve", "--stdio"])
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
    Ok(Server {
        child,
        stdin: input,
        stdout,
        stderr,
        pending: BTreeMap::new(),
    })
}
fn fingerprint(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}
fn checked(response: &Value, tool: &Value, status: &str) -> Result<Value> {
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(
        response["result"]["isError"],
        status == "unavailable",
        "{response}"
    );
    let output = response["result"]["structuredContent"].clone();
    let fallback: Value = serde_json::from_str(
        response["result"]["content"][0]["text"]
            .as_str()
            .ok_or("missing fallback")?,
    )?;
    assert_eq!(fallback, output);
    jsonschema::validator_for(&tool["outputSchema"])?
        .validate(&output)
        .map_err(|error| error.to_string())?;
    assert_eq!(output["status"], status, "{output}");
    assert_eq!(output["data"]["semantics"], "latest_known");
    let observation = &output["data"]["observation"];
    assert_eq!(observation["complete"], true);
    assert_eq!(observation["termination"], "exited");
    assert_eq!(observation["stdout_truncated"], false);
    assert_eq!(observation["stderr_truncated"], false);
    let runtime = &observation["runtime"];
    assert_eq!(runtime["image_id"], APPROVED_RUST_IMAGE);
    assert_eq!(runtime["platform"], "linux/aarch64");
    assert_eq!(runtime["rust_version"], "1.98.1");
    assert!(runtime["declared_toolchain"].is_null());
    for field in ["configuration_fingerprint", "execution_fingerprint"] {
        assert_fingerprint(&runtime[field]);
    }
    let text = observation["explanation"].as_str().unwrap_or("");
    assert_eq!(
        observation["content_fingerprint"],
        fingerprint(text.as_bytes())
    );
    assert_eq!(output["evidence"]["kind"], "snapshot");
    let provenance = &output["evidence"]["details"]["provenance"];
    assert_eq!(provenance["source_kind"], "artifact");
    assert_eq!(provenance["source_id"], observation["content_fingerprint"]);
    assert_eq!(provenance["integrity"], "verified");
    assert_eq!(provenance["network_used"], false);
    let created = provenance["created_at"]
        .as_u64()
        .ok_or("missing created time")?;
    let observed = provenance["observed_at"]
        .as_u64()
        .ok_or("missing observed time")?;
    assert!(created <= observed);
    assert!(output["data"].get("project_ref").is_none());
    assert!(!serde_json::to_string(&output)?.contains("artifact://"));
    Ok(output)
}
#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn compiler_explanation_requires_no_project_and_preserves_actual_evidence() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let expected = fixture.source_bytes()?;
    let mut server = start_without_roots(&fixture)?;
    server.send(request(1, "tools/list"))?;
    let list = server.receive(1, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("missing tools")?
        .iter()
        .find(|tool| tool["name"] == "rust.diagnostics.explain")
        .ok_or("missing explain tool")?
        .clone();
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(tool["inputSchema"]["required"], json!(["code"]));
    // Every invalid case completes before lazy calibration is allowed to start.
    let invalid = [
        "E0502;id",
        "--help",
        "E0502 --help",
        "E0502\n",
        "E５０２０",
        "e0502",
        "E502",
        "E00502",
    ];
    for (index, code) in invalid.iter().enumerate() {
        let id = 10 + i64::try_from(index)?;
        server.send(call(id, "rust.diagnostics.explain", json!({"code":code})))?;
        let response = server.receive(id, DISCOVERY_TIMEOUT)?;
        assert_eq!(response["error"]["code"], -32602, "{code:?}: {response}");
        fixture.assert_clean(None)?;
        assert!(
            fs::read_dir(&fixture.state)?.next().is_none(),
            "invalid input started calibration"
        );
    }
    server.send(call(
        30,
        "rust.diagnostics.explain",
        json!({"code":"E0502"}),
    ))?;
    let response = server.receive(30, JOIN_TIMEOUT)?;
    let output = checked(&response, &tool, "passed")?;
    let observation = &output["data"]["observation"];
    assert_eq!(observation["code"], "E0502");
    assert_eq!(observation["exit_code"], 0);
    let text = observation["explanation"]
        .as_str()
        .ok_or("compiler explanation missing")?;
    assert!(text.contains("borrow"), "unexpected compiler text: {text}");
    assert!(text.len() <= 64 * 1024);
    fixture.assert_clean(None)?;
    server.send(call(
        31,
        "rust.diagnostics.explain",
        json!({"code":"E9999"}),
    ))?;
    let unavailable = checked(&server.receive(31, JOIN_TIMEOUT)?, &tool, "unavailable")?;
    assert_eq!(unavailable["data"]["observation"]["code"], "E9999");
    assert!(unavailable["data"]["observation"]["explanation"].is_null());
    assert_eq!(unavailable["data"]["observation"]["exit_code"], 1);
    assert_eq!(
        observation["runtime"]["configuration_fingerprint"],
        unavailable["data"]["observation"]["runtime"]["configuration_fingerprint"]
    );
    server.send(request(32, "resources/list"))?;
    let resources = server.receive(32, DISCOVERY_TIMEOUT)?;
    assert_eq!(resources["result"]["resources"], json!([]), "{resources}");
    server.finish()?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M1_EXPLAIN_RECEIPT {}",
        json!({
            "cases": invalid.len() + 2,
            "invalid_codes_rejected_before_work": invalid.len(),
            "no_roots_or_project_handle": true, "actual_compiler_borrow_explanation": true,
            "unknown_code_unavailable_without_fabrication":true,
            "content_sha256_verified":2,"no_resources_created":true,"eof_cleanup":true,"cleanup":true,
            "configuration_fingerprint":observation["runtime"]["configuration_fingerprint"],
            "execution_fingerprint":observation["runtime"]["execution_fingerprint"],
            "content_fingerprint":observation["content_fingerprint"]
        })
    );
    fixture.successful = true;
    Ok(())
}
