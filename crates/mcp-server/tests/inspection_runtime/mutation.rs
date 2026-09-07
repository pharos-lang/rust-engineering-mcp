use super::*;

fn patch(id: i64, opened: &Value, mode: Value) -> Value {
    call(
        id,
        "rust.manifest.patch",
        json!({"project_ref": opened["project_ref"], "action":mode}),
    )
}
fn preview(opened: &Value, level: &str) -> Value {
    json!({"mode":"preview", "expected_project_fingerprint":opened["fingerprint"],
        "edit":{"operation":"lint_set","scope":"workspace","tool":"rust", "name":"unsafe_code","level":level,"priority":null}})
}
fn mutation_output(response: Value, schema: &Value, status: &str) -> Result<Value> {
    assert!(response.get("error").is_none(), "{response}");
    let output = &response["result"]["structuredContent"];
    assert_eq!(output["status"], status, "{response}");
    assert_eq!(output["concurrency_contract"], "local_coordinated");
    assert_eq!(
        output["guarantees_not_provided"].as_array().map(Vec::len),
        Some(4)
    );
    jsonschema::validator_for(schema)?
        .validate(output)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        serde_json::from_str::<Value>(
            response["result"]["content"][0]["text"]
                .as_str()
                .ok_or("missing text")?
        )?,
        *output
    );
    assert_eq!(
        response["result"]["isError"],
        matches!(status, "blocked" | "unavailable" | "cancelled")
    );
    Ok(output.clone())
}
fn mutation_schema(server: &mut Server, id: i64) -> Result<Value> {
    server.send(request(id, "tools/list"))?;
    let response = server.receive(id, DISCOVERY_TIMEOUT)?;
    let tool = response["result"]["tools"]
        .as_array()
        .ok_or("tools absent")?
        .iter()
        .find(|tool| tool["name"] == "rust.manifest.patch")
        .ok_or("mutation tool absent")?;
    assert_eq!(
        tool["annotations"],
        json!({"readOnlyHint":false,"destructiveHint":true,"idempotentHint":false,"openWorldHint":false})
    );
    Ok(tool["outputSchema"].clone())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn manifest_preview_commit_conflict_reopen_and_restart_receipt() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let before = fixture.source_bytes()?;
    let mut server = Server::start_with_manifest_write(&fixture, true)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let schema = mutation_schema(&mut server, 3)?;
    server.send(patch(4, &opened, preview(&opened, "deny")))?;
    let planned = mutation_output(server.receive(4, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_fixture_tree(&fixture, &before)?;
    assert_eq!(planned["data"]["files"][0]["path"], "Cargo.toml");
    assert!(
        planned["data"]["diff"]
            .as_str()
            .ok_or("diff absent")?
            .contains("unsafe_code = \"deny\"")
    );
    assert_fingerprint(&planned["data"]["plan_digest"]);

    // A different Rust source generation must invalidate a manifest-only plan.
    fs::write(
        fixture.project.join("app/src/lib.rs"),
        "pub fn edited_during_preview() {}\n",
    )?;
    let commit = json!({"mode":"commit", "plan_id":planned["data"]["plan_id"],
        "plan_digest":planned["data"]["plan_digest"],"idempotency_key":"first_commit"});
    server.send(patch(5, &opened, commit.clone()))?;
    let conflict = mutation_output(server.receive(5, JOIN_TIMEOUT)?, &schema, "blocked")?;
    assert_eq!(conflict["error_code"], "conflict");
    assert_eq!(
        fs::read(fixture.project.join("Cargo.toml"))?,
        before["Cargo.toml"]
    );
    fs::write(
        fixture.project.join("app/src/lib.rs"),
        &before["app/src/lib.rs"],
    )?;

    server.send(patch(6, &opened, commit.clone()))?;
    let receipt = mutation_output(server.receive(6, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_eq!(receipt["data"]["state"], "committed");
    let after_manifest = fs::read(fixture.project.join("Cargo.toml"))?;
    assert!(String::from_utf8(after_manifest.clone())?.contains("unsafe_code = \"deny\""));
    for (path, bytes) in &before {
        if path != "Cargo.toml" {
            assert_eq!(fs::read(fixture.project.join(path))?, *bytes);
        }
    }
    // Old manifest reference is not silently reissued with a new generation.
    server.send(patch(
        7,
        &opened,
        json!({"mode":"receipt","operation_id":receipt["data"]["operation_id"],"recover":false}),
    ))?;
    mutation_output(server.receive(7, JOIN_TIMEOUT)?, &schema, "blocked")?;
    server.send(call(
        8,
        "rust.project.open",
        json!({"path":fixture.project}),
    ))?;
    let reopened =
        server.receive(8, DISCOVERY_TIMEOUT)?["result"]["structuredContent"]["data"].clone();
    assert_ne!(reopened["fingerprint"], opened["fingerprint"]);
    server.send(patch(9, &reopened, commit))?;
    let replay = mutation_output(server.receive(9, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_eq!(replay["data"], receipt["data"]);
    assert_eq!(
        fs::read(fixture.project.join("Cargo.toml"))?,
        after_manifest
    );
    server.finish()?;
    fixture.assert_clean(None)?;

    // Receipt requires current host write grant even after process restart.
    let mut denied = Server::start(&fixture)?;
    let (denied_open, _) = denied.bootstrap_open(&fixture)?;
    denied.send(patch(
        3,
        &denied_open,
        json!({"mode":"receipt","operation_id":receipt["data"]["operation_id"],"recover":false}),
    ))?;
    let denied_result = mutation_output(denied.receive(3, DISCOVERY_TIMEOUT)?, &schema, "blocked")?;
    assert_eq!(denied_result["error_code"], "permission_denied");
    denied.finish()?;

    let mut restarted = Server::start_with_manifest_write(&fixture, true)?;
    let (reopened, _) = restarted.bootstrap_open(&fixture)?;
    restarted.send(patch(
        3,
        &reopened,
        json!({"mode":"receipt","operation_id":receipt["data"]["operation_id"],"recover":false}),
    ))?;
    let recovered = mutation_output(restarted.receive(3, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_eq!(recovered["data"], receipt["data"]);
    assert_eq!(
        fs::read(fixture.project.join("Cargo.toml"))?,
        after_manifest
    );
    restarted.finish()?;
    fixture.assert_clean(None)?;
    // Administrative retention is explicit and does not require a Cargo job or workspace edit.
    let admin = |action: &str| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"));
        command
            .env_clear()
            .args(["mutation", action, "--state-root"])
            .arg(&fixture.state)
            .arg("--json");
        command
    };
    let listed: Value = serde_json::from_slice(&bounded_command(admin("list"))?)?;
    assert_eq!(listed["records"].as_array().map(Vec::len), Some(1));
    assert_eq!(
        listed["records"][0]["operation_id"],
        receipt["data"]["operation_id"]
    );
    let mut prune = admin("prune");
    prune
        .arg("--operation-id")
        .arg(
            receipt["data"]["operation_id"]
                .as_str()
                .ok_or("id absent")?,
        )
        .arg("--plan-digest")
        .arg(
            receipt["data"]["plan_digest"]
                .as_str()
                .ok_or("digest absent")?,
        );
    let pruned: Value = serde_json::from_slice(&bounded_command(prune)?)?;
    assert_eq!(pruned["status"], "passed");
    let listed: Value = serde_json::from_slice(&bounded_command(admin("list"))?)?;
    assert_eq!(listed["records"], json!([]));
    assert_eq!(
        fs::read(fixture.project.join("Cargo.toml"))?,
        after_manifest
    );
    fixture.successful = true;
    Ok(())
}
