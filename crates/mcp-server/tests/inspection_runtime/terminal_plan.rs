use super::format_mutation_runtime::{mutation_output, tool_schema};
use super::*;

fn patch_call(id: i64, opened: &Value, action: Value) -> Value {
    call(
        id,
        "rust.manifest.patch",
        json!({"project_ref":opened["project_ref"],"action":action}),
    )
}

fn format_call(id: i64, opened: &Value, action: Value) -> Value {
    call(
        id,
        "rust.fmt.apply",
        json!({"project_ref":opened["project_ref"],"action":action}),
    )
}

fn lint_preview(opened: &Value, level: &str) -> Value {
    json!({
        "mode":"preview",
        "expected_project_fingerprint":opened["fingerprint"],
        "edit":{
            "operation":"lint_set",
            "scope":"workspace",
            "tool":"rust",
            "name":"unsafe_code",
            "level":level,
            "priority":null
        }
    })
}

fn commit(preview: &Value, key: &str) -> Value {
    json!({
        "mode":"commit",
        "plan_id":preview["data"]["plan_id"],
        "plan_digest":preview["data"]["plan_digest"],
        "idempotency_key":key
    })
}

fn reopen(server: &mut Server, fixture: &Fixture, id: i64) -> Result<Value> {
    server.send(call(
        id,
        "rust.project.open",
        json!({"path":fixture.project}),
    ))?;
    let response = server.receive(id, DISCOVERY_TIMEOUT)?;
    assert_eq!(
        response["result"]["structuredContent"]["status"], "passed",
        "{response}"
    );
    Ok(response["result"]["structuredContent"]["data"].clone())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial runtime qualification"]
fn terminal_plans_free_quota_and_replay_only_from_exact_durable_identity() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let original = fixture.source_bytes()?;
    let mut server = Server::start_with_grants(&fixture, true, true)?;
    let (mut opened, _) = server.bootstrap_open(&fixture)?;
    let patch_schema = tool_schema(&mut server, 3, "rust.manifest.patch")?;
    let mut request_id = 4;
    let mut first_commit = None;
    let mut first_receipt = None;
    let mut first_after_manifest = None;

    // Five delivered previews and terminal commits in one server process exceed
    // the pending-plan quota unless each terminal plan releases its RAM budget.
    for (index, level) in ["deny", "warn", "allow", "forbid", "warn"]
        .into_iter()
        .enumerate()
    {
        let before = fixture.source_bytes()?;
        server.send(patch_call(
            request_id,
            &opened,
            lint_preview(&opened, level),
        ))?;
        let preview = mutation_output(
            server.receive(request_id, JOIN_TIMEOUT)?,
            &patch_schema,
            "passed",
        )?;
        request_id += 1;
        assert_fixture_tree(&fixture, &before)?;
        assert_eq!(preview["data"]["files"][0]["path"], "Cargo.toml");
        assert!(
            preview["data"]["diff"]
                .as_str()
                .is_some_and(|diff| diff.contains(&format!("unsafe_code = \"{level}\"")))
        );

        let action = commit(&preview, &format!("terminal_plan_{index}"));
        server.send(patch_call(request_id, &opened, action.clone()))?;
        let receipt = mutation_output(
            server.receive(request_id, JOIN_TIMEOUT)?,
            &patch_schema,
            "passed",
        )?;
        request_id += 1;
        assert_eq!(receipt["data"]["state"], "committed");
        let after = fixture.source_bytes()?;
        for (path, bytes) in &before {
            if path != "Cargo.toml" {
                assert_eq!(after[path], *bytes, "unexpected change to {path}");
            }
        }
        assert_fixture_tree(&fixture, &after)?;
        assert!(
            String::from_utf8(after["Cargo.toml"].clone())?
                .contains(&format!("unsafe_code = \"{level}\""))
        );
        opened = reopen(&mut server, &fixture, request_id)?;
        request_id += 1;

        if index == 0 {
            // The response was consumed above, but a caller that lost delivery
            // may repeat the exact commit. This must resolve from the terminal
            // journal after the in-memory plan has been retired.
            server.send(patch_call(request_id, &opened, action.clone()))?;
            let replay = mutation_output(
                server.receive(request_id, JOIN_TIMEOUT)?,
                &patch_schema,
                "passed",
            )?;
            request_id += 1;
            assert_eq!(replay["data"], receipt["data"]);
            assert_fixture_tree(&fixture, &after)?;
            opened = reopen(&mut server, &fixture, request_id)?;
            request_id += 1;
            first_commit = Some(action);
            first_receipt = Some(receipt);
            first_after_manifest = Some(after["Cargo.toml"].clone());
        }
    }

    let first_commit = first_commit.ok_or("first commit absent")?;
    let first_receipt = first_receipt.ok_or("first receipt absent")?;
    let first_after_manifest = first_after_manifest.ok_or("first manifest absent")?;
    let final_manifest = fs::read(fixture.project.join("Cargo.toml"))?;
    assert_ne!(final_manifest, first_after_manifest);
    assert_ne!(final_manifest, original["Cargo.toml"]);
    server.finish()?;
    fixture.assert_clean(None)?;

    // Advance the source again after the original operation. Durable replay is
    // historical evidence and must not resurrect either its old manifest or any
    // other old source byte after a server restart.
    fs::write(
        fixture.project.join("app/src/lib.rs"),
        "pub fn changed_after_terminal_commit() {}\n",
    )?;
    let advanced = fixture.source_bytes()?;
    let mut restarted = Server::start_with_grants(&fixture, true, true)?;
    let (reopened, _) = restarted.bootstrap_open(&fixture)?;
    let restarted_patch_schema = tool_schema(&mut restarted, 3, "rust.manifest.patch")?;
    let restarted_format_schema = tool_schema(&mut restarted, 4, "rust.fmt.apply")?;

    let mut wrong_key = first_commit.clone();
    wrong_key["idempotency_key"] = json!("different_key");
    restarted.send(patch_call(5, &reopened, wrong_key))?;
    let rejected = mutation_output(
        restarted.receive(5, JOIN_TIMEOUT)?,
        &restarted_patch_schema,
        "blocked",
    )?;
    assert_eq!(rejected["error_code"], "conflict");
    assert_fixture_tree(&fixture, &advanced)?;

    let mut wrong_digest = first_commit.clone();
    wrong_digest["plan_digest"] = json!(format!("sha256:{}", "0".repeat(64)));
    restarted.send(patch_call(6, &reopened, wrong_digest))?;
    let rejected = mutation_output(
        restarted.receive(6, JOIN_TIMEOUT)?,
        &restarted_patch_schema,
        "blocked",
    )?;
    assert_eq!(rejected["error_code"], "conflict");
    assert_fixture_tree(&fixture, &advanced)?;

    restarted.send(format_call(7, &reopened, first_commit.clone()))?;
    let rejected = mutation_output(
        restarted.receive(7, JOIN_TIMEOUT)?,
        &restarted_format_schema,
        "blocked",
    )?;
    assert_eq!(rejected["error_code"], "permission_denied");
    assert_fixture_tree(&fixture, &advanced)?;

    restarted.send(patch_call(8, &reopened, first_commit))?;
    let replay = mutation_output(
        restarted.receive(8, JOIN_TIMEOUT)?,
        &restarted_patch_schema,
        "passed",
    )?;
    assert_eq!(replay["data"], first_receipt["data"]);
    assert_fixture_tree(&fixture, &advanced)?;
    restarted.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    Ok(())
}
