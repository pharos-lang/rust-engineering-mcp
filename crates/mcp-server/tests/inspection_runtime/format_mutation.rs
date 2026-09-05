use super::*;

fn fmt_call(id: i64, opened: &Value, action: Value) -> Value {
    call(
        id,
        "rust.fmt.apply",
        json!({"project_ref":opened["project_ref"], "action":action}),
    )
}

fn fmt_preview(opened: &Value) -> Value {
    json!({
        "mode":"preview",
        "expected_project_fingerprint":opened["fingerprint"]
    })
}

fn mutation_output(response: Value, schema: &Value, status: &str) -> Result<Value> {
    assert!(response.get("error").is_none(), "{response}");
    let output = &response["result"]["structuredContent"];
    assert_eq!(output["status"], status, "{response}");
    assert_eq!(output["concurrency_contract"], "local_coordinated");
    assert_eq!(
        output["guarantees_not_provided"],
        json!([
            "os_exclusion_of_external_writers",
            "multi_file_atomicity",
            "malicious_host_protection",
            "demonstrated_power_loss_survival"
        ])
    );
    jsonschema::validator_for(schema)?
        .validate(output)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        serde_json::from_str::<Value>(
            response["result"]["content"][0]["text"]
                .as_str()
                .ok_or("missing text fallback")?
        )?,
        *output
    );
    assert_eq!(
        response["result"]["isError"],
        matches!(status, "blocked" | "unavailable" | "cancelled")
    );
    Ok(output.clone())
}

fn tool_schema(server: &mut Server, id: i64, name: &str) -> Result<Value> {
    server.send(request(id, "tools/list"))?;
    let response = server.receive(id, DISCOVERY_TIMEOUT)?;
    let tool = response["result"]["tools"]
        .as_array()
        .ok_or("tools absent")?
        .iter()
        .find(|tool| tool["name"] == name)
        .ok_or("requested tool absent")?;
    Ok(tool["outputSchema"].clone())
}

fn assert_validation(value: &Value) {
    assert_eq!(value["method"], "rustfmt_then_fmt_check");
    assert_eq!(value["semantics"], "latest_known");
    assert_eq!(value["platform"], "linux/aarch64");
    assert_eq!(value["image_id"], APPROVED_RUST_IMAGE);
    for name in [
        "configuration_fingerprint",
        "execution_fingerprint",
        "candidate_source_fingerprint",
        "mutation_execution_fingerprint",
    ] {
        assert_fingerprint(&value[name]);
    }
    assert_eq!(value["rust_version"], "1.98.1");
    assert_eq!(value["cargo_version"], "1.98.1");
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn fmt_preview_is_default_denied_and_cargo_alias_is_rejected_before_gateway() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;

    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let before = fixture.source_bytes()?;
    let mut server = Server::start(&fixture)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let schema = tool_schema(&mut server, 3, "rust.fmt.apply")?;
    server.send(fmt_call(4, &opened, fmt_preview(&opened)))?;
    let denied = mutation_output(server.receive(4, DISCOVERY_TIMEOUT)?, &schema, "blocked")?;
    assert_eq!(denied["error_code"], "permission_denied");
    assert_fixture_tree(&fixture, &before)?;
    server.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;

    // Cargo discovers aliases in ancestor and nested project configuration. The
    // full source reader rejects this case-insensitively before rustfmt or any
    // other gateway operation can be admitted.
    let mut hostile = Fixture::new()?;
    hostile.assert_clean(None)?;
    fs::create_dir_all(hostile.project.join("nested/.Cargo"))?;
    fs::write(
        hostile.project.join("nested/.Cargo/config"),
        "[alias]\nfmt = \"run --\"\n",
    )?;
    let alias_before = fs::read(hostile.project.join("nested/.Cargo/config"))?;
    let mut server = Server::start_with_grants(&hostile, false, true)?;
    let (hostile_open, _) = server.bootstrap_open(&hostile)?;
    let hostile_schema = tool_schema(&mut server, 3, "rust.fmt.apply")?;
    server.send(fmt_call(4, &hostile_open, fmt_preview(&hostile_open)))?;
    let rejected = mutation_output(
        server.receive(4, DISCOVERY_TIMEOUT)?,
        &hostile_schema,
        "blocked",
    )?;
    assert_eq!(rejected["error_code"], "permission_denied");
    assert_eq!(
        fs::read(hostile.project.join("nested/.Cargo/config"))?,
        alias_before
    );
    hostile.assert_clean(None)?;
    server.finish()?;
    hostile.assert_clean(None)?;
    hostile.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn fmt_preview_commit_conflict_reopen_check_restart_receipt_and_noop() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let app = b"mod multi;pub fn benign( ){let _=multi::value();}\n";
    let multi = b"pub fn value( )->u8{42}\n";
    let helper = b"pub fn benign( ){println!(\"helper\");}\n";
    fs::write(fixture.project.join("app/src/lib.rs"), app)?;
    fs::write(fixture.project.join("app/src/multi.rs"), multi)?;
    fs::write(fixture.project.join("helper/src/lib.rs"), helper)?;
    let mut before = fixture.source_bytes()?;
    before.insert("app/src/multi.rs".into(), multi.to_vec());

    let mut server = Server::start_with_grants(&fixture, true, true)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let fmt_schema = tool_schema(&mut server, 3, "rust.fmt.apply")?;
    let manifest_schema = tool_schema(&mut server, 4, "rust.manifest.patch")?;
    server.send(fmt_call(5, &opened, fmt_preview(&opened)))?;
    let planned = mutation_output(server.receive(5, JOIN_TIMEOUT)?, &fmt_schema, "passed")?;
    assert_fixture_tree(&fixture, &before)?;
    assert_fingerprint(&planned["data"]["plan_digest"]);
    assert_validation(&planned["data"]["validation"]);
    assert_eq!(
        planned["data"]["files"]
            .as_array()
            .ok_or("preview files absent")?
            .iter()
            .map(|file| file["path"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        ["app/src/lib.rs", "app/src/multi.rs", "helper/src/lib.rs"]
    );
    let expected_diff = concat!(
        "--- a/app/src/lib.rs\n+++ b/app/src/lib.rs\n@@ -1,1 +1,4 @@\n",
        "-mod multi;pub fn benign( ){let _=multi::value();}\n",
        "+mod multi;\n+pub fn benign() {\n+    let _ = multi::value();\n+}\n",
        "--- a/app/src/multi.rs\n+++ b/app/src/multi.rs\n@@ -1,1 +1,3 @@\n",
        "-pub fn value( )->u8{42}\n+pub fn value() -> u8 {\n+    42\n+}\n",
        "--- a/helper/src/lib.rs\n+++ b/helper/src/lib.rs\n@@ -1,1 +1,3 @@\n",
        "-pub fn benign( ){println!(\"helper\");}\n",
        "+pub fn benign() {\n+    println!(\"helper\");\n+}\n"
    );
    assert_eq!(planned["data"]["diff"], expected_diff);

    let commit = json!({
        "mode":"commit",
        "plan_id":planned["data"]["plan_id"],
        "plan_digest":planned["data"]["plan_digest"],
        "idempotency_key":"fmt_multi_commit"
    });
    // Plans are shared only to enforce their global memory bound. A different
    // operation kind cannot consume a valid format plan even with both grants.
    server.send(call(
        6,
        "rust.manifest.patch",
        json!({"project_ref":opened["project_ref"], "action":commit.clone()}),
    ))?;
    let wrong_plan = mutation_output(
        server.receive(6, DISCOVERY_TIMEOUT)?,
        &manifest_schema,
        "blocked",
    )?;
    assert_eq!(wrong_plan["error_code"], "permission_denied");
    assert_fixture_tree(&fixture, &before)?;

    // Any external source edit between approval and commit invalidates the full
    // candidate, even when that particular file would format to the same bytes.
    let external = b"pub fn changed_after_approval() {}\n";
    fs::write(fixture.project.join("helper/src/lib.rs"), external)?;
    server.send(fmt_call(7, &opened, commit.clone()))?;
    let stale = mutation_output(server.receive(7, JOIN_TIMEOUT)?, &fmt_schema, "blocked")?;
    assert_eq!(stale["error_code"], "conflict");
    assert_eq!(fs::read(fixture.project.join("app/src/lib.rs"))?, app);
    assert_eq!(fs::read(fixture.project.join("app/src/multi.rs"))?, multi);
    assert_eq!(
        fs::read(fixture.project.join("helper/src/lib.rs"))?,
        external
    );
    fs::write(fixture.project.join("helper/src/lib.rs"), helper)?;

    server.send(fmt_call(8, &opened, commit.clone()))?;
    let receipt = mutation_output(server.receive(8, JOIN_TIMEOUT)?, &fmt_schema, "passed")?;
    assert_eq!(receipt["data"]["state"], "committed");
    assert_eq!(receipt["data"]["files"].as_array().map(Vec::len), Some(3));
    assert_validation(&receipt["data"]["validation"]);
    let after = [
        (
            "app/src/lib.rs",
            b"mod multi;\npub fn benign() {\n    let _ = multi::value();\n}\n".as_slice(),
        ),
        (
            "app/src/multi.rs",
            b"pub fn value() -> u8 {\n    42\n}\n".as_slice(),
        ),
        (
            "helper/src/lib.rs",
            b"pub fn benign() {\n    println!(\"helper\");\n}\n".as_slice(),
        ),
    ];
    let mut expected_after = before.clone();
    for (path, bytes) in after {
        assert_eq!(fs::read(fixture.project.join(path))?, bytes);
        expected_after.insert(path.into(), bytes.to_vec());
    }
    assert_fixture_tree(&fixture, &expected_after)?;

    // Successful publication retires the old reference. Reopening proves the
    // host tree is independently readable and fmt.check accepts the result.
    server.send(fmt_call(
        9,
        &opened,
        json!({"mode":"receipt","operation_id":receipt["data"]["operation_id"],"recover":false}),
    ))?;
    let retired = mutation_output(
        server.receive(9, DISCOVERY_TIMEOUT)?,
        &fmt_schema,
        "blocked",
    )?;
    assert_eq!(retired["error_code"], "permission_denied");
    server.send(call(
        10,
        "rust.project.open",
        json!({"path":fixture.project}),
    ))?;
    let reopened_response = server.receive(10, DISCOVERY_TIMEOUT)?;
    assert_eq!(
        reopened_response["result"]["structuredContent"]["status"],
        "passed"
    );
    let reopened = reopened_response["result"]["structuredContent"]["data"].clone();
    assert_ne!(reopened["project_ref"], opened["project_ref"]);
    // The structural identity covers manifests and workspace membership; source
    // formatting retires the capability but intentionally preserves that identity.
    assert_eq!(reopened["fingerprint"], opened["fingerprint"]);
    let fmt_check_schema = tool_schema(&mut server, 11, "rust.fmt.check")?;
    server.send(call(
        12,
        "rust.fmt.check",
        json!({"project_ref":reopened["project_ref"]}),
    ))?;
    let fmt_check_response = server.receive(12, JOIN_TIMEOUT)?;
    let fmt_check_tool = json!({"outputSchema":fmt_check_schema});
    checked_output(&fmt_check_response, &fmt_check_tool, &reopened, "passed")?;

    server.send(fmt_call(
        13,
        &reopened,
        json!({"mode":"receipt","operation_id":receipt["data"]["operation_id"],"recover":false}),
    ))?;
    let current = mutation_output(
        server.receive(13, DISCOVERY_TIMEOUT)?,
        &fmt_schema,
        "passed",
    )?;
    assert_eq!(current["data"], receipt["data"]);
    server.send(call(
        14,
        "rust.manifest.patch",
        json!({"project_ref":reopened["project_ref"],"action":{
            "mode":"receipt","operation_id":receipt["data"]["operation_id"],"recover":false
        }}),
    ))?;
    let wrong_receipt = mutation_output(
        server.receive(14, DISCOVERY_TIMEOUT)?,
        &manifest_schema,
        "blocked",
    )?;
    assert_eq!(wrong_receipt["error_code"], "permission_denied");

    // A second preview over already formatted bytes is a successful exact no-op.
    server.send(fmt_call(15, &reopened, fmt_preview(&reopened)))?;
    let no_change = mutation_output(server.receive(15, JOIN_TIMEOUT)?, &fmt_schema, "passed")?;
    assert_eq!(no_change["data"]["files"], json!([]));
    assert_eq!(no_change["data"]["diff"], "");
    assert_validation(&no_change["data"]["validation"]);
    assert_fixture_tree(&fixture, &expected_after)?;
    server.finish()?;
    fixture.assert_clean(None)?;

    // Persisted receipts remain capability-gated after restart and are scoped to
    // the exact operation kind recorded in the journal.
    let mut denied = Server::start(&fixture)?;
    let (denied_open, _) = denied.bootstrap_open(&fixture)?;
    denied.send(fmt_call(
        3,
        &denied_open,
        json!({"mode":"receipt","operation_id":receipt["data"]["operation_id"],"recover":false}),
    ))?;
    let denied_receipt = mutation_output(
        denied.receive(3, DISCOVERY_TIMEOUT)?,
        &fmt_schema,
        "blocked",
    )?;
    assert_eq!(denied_receipt["error_code"], "permission_denied");
    denied.finish()?;

    let mut restarted = Server::start_with_grants(&fixture, false, true)?;
    let (restart_open, _) = restarted.bootstrap_open(&fixture)?;
    restarted.send(fmt_call(
        3,
        &restart_open,
        json!({"mode":"receipt","operation_id":receipt["data"]["operation_id"],"recover":false}),
    ))?;
    let recovered = mutation_output(
        restarted.receive(3, DISCOVERY_TIMEOUT)?,
        &fmt_schema,
        "passed",
    )?;
    assert_eq!(recovered["data"], receipt["data"]);
    restarted.finish()?;
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected_after)?;
    fixture.successful = true;
    Ok(())
}
