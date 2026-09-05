use super::format_mutation_runtime::{mutation_output, tool_schema};
use super::*;

fn m1_arguments(fixture: &Fixture, opened: &Value) -> Vec<(&'static str, Value)> {
    [
        ("rust.project.open", json!({"path":fixture.project})),
        (
            "rust.project.inspect",
            json!({"project_ref":opened["project_ref"]}),
        ),
        (
            "rust.toolchain.inspect",
            json!({"project_ref":opened["project_ref"]}),
        ),
        ("rust.check", json!({"project_ref":opened["project_ref"]})),
        (
            "rust.fmt.check",
            json!({"project_ref":opened["project_ref"]}),
        ),
        ("rust.clippy", json!({"project_ref":opened["project_ref"]})),
        ("rust.test", json!({"project_ref":opened["project_ref"]})),
        (
            "rust.dependencies.audit",
            json!({"project_ref":opened["project_ref"]}),
        ),
        ("rust.diagnostics.explain", json!({"code":"E0502"})),
        (
            "rust.quality.gate",
            json!({"project_ref":opened["project_ref"],"profile":"fast"}),
        ),
        ("rust.catalog.status", json!({})),
        (
            "rust.crate.search",
            json!({"query":"serde","mode":"lexical"}),
        ),
        ("rust.crate.inspect", json!({"name":"serde"})),
    ]
    .into()
}

fn without_timing(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.remove("duration_ms");
            for value in fields.values_mut() {
                without_timing(value);
            }
        }
        Value::Array(items) => {
            for item in items {
                without_timing(item);
            }
        }
        _ => {}
    }
}

fn m1_during_busy(
    server: &mut Server,
    fixture: &Fixture,
    opened: &Value,
    base: i64,
) -> Result<Vec<Value>> {
    let calls = m1_arguments(fixture, opened);
    for (index, (name, arguments)) in calls.iter().enumerate() {
        server.send(call(base + index as i64, name, arguments.clone()))?;
    }
    let mut outcomes = Vec::new();
    for index in 0..calls.len() {
        let response = server.receive(base + index as i64, DISCOVERY_TIMEOUT)?;
        let mut normalized = if let Some(result) = response.get("result") {
            // Compare the authoritative structured result, not the duplicated text
            // fallback containing the same per-request duration.
            json!({"isError":result["isError"],"resultType":result["resultType"],"structuredContent":result["structuredContent"]})
        } else {
            json!({"error":response["error"]})
        };
        without_timing(&mut normalized);
        outcomes.push(normalized);
    }
    Ok(outcomes)
}

#[test]
#[ignore = "explicit approved Docker runtime/socket and APFS; serial M1/M2 concurrency qualification"]
fn thirteen_m1_tools_keep_existing_busy_contract_during_real_native_commit() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let mut server = Server::start_with_grants(&fixture, true, false)?;
    let (opened, inspect_schema) = server.bootstrap_open(&fixture)?;
    server.send(request(3, "tools/list"))?;
    let listing = server.receive(3, DISCOVERY_TIMEOUT)?;
    for (name, arguments) in m1_arguments(&fixture, &opened) {
        let tool = listing["result"]["tools"]
            .as_array()
            .ok_or("tools missing")?
            .iter()
            .find(|tool| tool["name"] == name)
            .ok_or("M1 tool missing")?;
        assert!(
            jsonschema::validator_for(&tool["inputSchema"])?.is_valid(&arguments),
            "invalid fixture input for {name}"
        );
    }

    server.send(call(
        4,
        "rust.project.inspect",
        json!({"project_ref":opened["project_ref"]}),
    ))?;
    let inspected = server.receive(4, JOIN_TIMEOUT)?;
    assert_eq!(inspected["result"]["structuredContent"]["status"], "passed");
    jsonschema::validator_for(&inspect_schema["outputSchema"])?
        .validate(&inspected["result"]["structuredContent"])
        .map_err(|error| error.to_string())?;
    fs::write(
        fixture.project.join("app/build.rs"),
        "fn main() { std::thread::sleep(std::time::Duration::from_secs(60)); }\n",
    )?;
    server.send(call(
        10,
        "rust.check",
        json!({"project_ref":opened["project_ref"]}),
    ))?;
    let nonce = active_slow_check(&mut server, &fixture, 10)?;
    let baseline = m1_during_busy(&mut server, &fixture, &opened, 100)?;
    server.send(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10}}),
    )?;
    let deadline = Instant::now() + JOIN_TIMEOUT;
    while !fixture.objects("container", Some(&nonce))?.is_empty()
        || !fixture.objects("volume", Some(&nonce))?.is_empty()
    {
        if Instant::now() >= deadline {
            return Err("M1 baseline cleanup timed out".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
    fixture.assert_clean(None)?;
    fs::remove_file(fixture.project.join("app/build.rs"))?;
    // The large unchanged payload keeps a genuine durable commit observable; it
    // is never executed, changed, or emitted in the reviewed manifest diff.
    for index in 0..15 {
        fs::write(
            fixture.project.join(format!("payload{index:02}.txt")),
            vec![b'a'; 1024 * 1024],
        )?;
    }
    let schema = tool_schema(&mut server, 30, "rust.manifest.patch")?;
    let preview = json!({"mode":"preview","expected_project_fingerprint":opened["fingerprint"],
        "edit":{"operation":"lint_set","scope":"workspace","tool":"rust","name":"unsafe_code","level":"forbid","priority":null}});
    server.send(call(
        31,
        "rust.manifest.patch",
        json!({"project_ref":opened["project_ref"],"action":preview}),
    ))?;
    let planned = mutation_output(server.receive(31, JOIN_TIMEOUT)?, &schema, "passed")?;
    let commit = json!({"mode":"commit","plan_id":planned["data"]["plan_id"],"plan_digest":planned["data"]["plan_digest"],"idempotency_key":"m1-concurrency"});
    server.send(call(
        32,
        "rust.manifest.patch",
        json!({"project_ref":opened["project_ref"],"action":commit}),
    ))?;
    let deadline = Instant::now() + JOIN_TIMEOUT;
    loop {
        server.assert_no_response(32)?;
        let staged = fs::read_dir(&fixture.project)?
            .filter_map(std::result::Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".rust-mcp-mut-")
            });
        if staged {
            break;
        }
        if Instant::now() >= deadline {
            return Err("native commit staging not observed".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now();
    let concurrent = m1_during_busy(&mut server, &fixture, &opened, 200)?;
    let m1_elapsed = started.elapsed();
    assert!(
        m1_elapsed < DISCOVERY_TIMEOUT,
        "M1 calls exceeded existing discovery budget"
    );
    server.assert_no_response(32)?;
    assert_eq!(
        concurrent, baseline,
        "M2 changed an existing M1 admission/result contract"
    );
    let committed = mutation_output(server.receive(32, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_eq!(committed["data"]["state"], "committed");
    server.send(call(
        33,
        "rust.project.inspect",
        json!({"project_ref":opened["project_ref"]}),
    ))?;
    assert_eq!(
        server.receive(33, DISCOVERY_TIMEOUT)?["result"]["structuredContent"]["status"],
        "blocked"
    );
    for index in 0..15 {
        assert_eq!(
            fs::read(fixture.project.join(format!("payload{index:02}.txt")))?,
            vec![b'a'; 1024 * 1024]
        );
    }
    server.finish()?;
    fixture.assert_clean(None)?;
    println!(
        "M2_M1_CONCURRENCY {}",
        json!({"tools_compared":13,"native_staging_observed":true,
        "source_payload_bytes":15*1024*1024,"thirteen_m1_responses_ms":m1_elapsed.as_millis(),"thirteen_calls_and_commit_wait_ms":started.elapsed().as_millis(),
        "m1_busy_outcomes_equal":true,"old_reference_invalidated":true})
    );
    fixture.successful = true;
    Ok(())
}
