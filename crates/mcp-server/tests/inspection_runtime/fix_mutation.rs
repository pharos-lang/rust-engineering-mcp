use super::format_mutation_runtime::{mutation_output, tool_schema};
use super::*;

fn fix_call(id: i64, opened: &Value, action: Value) -> Value {
    call(
        id,
        "rust.fix.apply",
        json!({"project_ref":opened["project_ref"],"action":action}),
    )
}

fn preview(opened: &Value) -> Value {
    json!({"mode":"preview","expected_project_fingerprint":opened["fingerprint"]})
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial runtime qualification"]
fn fix_preview_commit_recheck_noop_and_restart_receipt_keep_exact_scope() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let original = b"#[cfg(not(feature = \"extra\"))]\ncompile_error!(\"default feature required\");\npub fn answer() -> u8 { let mut value = 42; value }\n";
    let expected = b"#[cfg(not(feature = \"extra\"))]\ncompile_error!(\"default feature required\");\npub fn answer() -> u8 { let value = 42; value }\n";
    fs::write(fixture.project.join("app/src/lib.rs"), original)?;
    let before = fixture.source_bytes()?;
    let mut denied_server = Server::start_with_grants(&fixture, true, true)?;
    let (denied_open, _) = denied_server.bootstrap_open(&fixture)?;
    let schema = tool_schema(&mut denied_server, 3, "rust.fix.apply")?;
    denied_server.send(fix_call(4, &denied_open, preview(&denied_open)))?;
    let denied = mutation_output(
        denied_server.receive(4, DISCOVERY_TIMEOUT)?,
        &schema,
        "blocked",
    )?;
    assert_eq!(denied["error_code"], "permission_denied");
    denied_server.finish()?;
    assert_fixture_tree(&fixture, &before)?;

    let mut server = Server::start_with_mutations(&fixture, false, true, true)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    server.send(fix_call(3, &opened, preview(&opened)))?;
    let planned = mutation_output(server.receive(3, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_fixture_tree(&fixture, &before)?;
    let validation = &planned["data"]["validation"];
    assert_eq!(validation["method"], "cargo_fix_then_check");
    assert_eq!(validation["image_id"], APPROVED_RUST_IMAGE);
    for name in [
        "execution_fingerprint",
        "mutation_execution_fingerprint",
        "candidate_source_fingerprint",
    ] {
        assert_fingerprint(&validation[name]);
    }
    assert_eq!(planned["data"]["files"].as_array().map(Vec::len), Some(1));
    assert_eq!(planned["data"]["files"][0]["path"], "app/src/lib.rs");
    assert!(
        planned["data"]["diff"]
            .as_str()
            .ok_or("diff missing")?
            .contains("-pub fn answer() -> u8 { let mut value = 42; value }")
    );
    let commit = json!({"mode":"commit","plan_id":planned["data"]["plan_id"],"plan_digest":planned["data"]["plan_digest"],"idempotency_key":"fixed-workspace"});
    server.send(call(
        4,
        "rust.fmt.apply",
        json!({"project_ref":opened["project_ref"],"action":commit}),
    ))?;
    let cross_kind = mutation_output(server.receive(4, DISCOVERY_TIMEOUT)?, &schema, "blocked")?;
    assert_eq!(cross_kind["error_code"], "permission_denied");
    server.send(fix_call(5, &opened, commit.clone()))?;
    let committed = mutation_output(server.receive(5, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_eq!(committed["data"]["state"], "committed");
    let mut after = before.clone();
    after.insert("app/src/lib.rs".into(), expected.to_vec());
    assert_fixture_tree(&fixture, &after)?;
    server.send(call(
        6,
        "rust.project.open",
        json!({"path":fixture.project}),
    ))?;
    let reopened_response = server.receive(6, DISCOVERY_TIMEOUT)?;
    let reopened = reopened_response["result"]["structuredContent"]["data"].clone();
    assert_ne!(opened["project_ref"], reopened["project_ref"]);
    let check_schema = tool_schema(&mut server, 7, "rust.check")?;
    server.send(call(
        8,
        "rust.check",
        json!({"project_ref":reopened["project_ref"],"workspace":true,"all_targets":true}),
    ))?;
    checked_output(
        &server.receive(8, JOIN_TIMEOUT)?,
        &json!({"outputSchema":check_schema}),
        &reopened,
        "passed",
    )?;
    server.send(fix_call(9, &reopened, preview(&reopened)))?;
    let noop = mutation_output(server.receive(9, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_eq!(noop["data"]["files"], json!([]));
    assert_eq!(noop["data"]["diff"], "");
    assert_fixture_tree(&fixture, &after)?;
    server.finish()?;
    fixture.assert_clean(None)?;

    let mut restarted = Server::start_with_mutations(&fixture, false, true, true)?;
    let (reopened, _) = restarted.bootstrap_open(&fixture)?;
    let receipt =
        json!({"mode":"receipt","operation_id":committed["data"]["operation_id"],"recover":true});
    restarted.send(fix_call(3, &reopened, receipt.clone()))?;
    let replay = mutation_output(restarted.receive(3, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_eq!(replay["data"], committed["data"]);
    restarted.send(call(
        4,
        "rust.fmt.apply",
        json!({"project_ref":reopened["project_ref"],"action":receipt}),
    ))?;
    let wrong = mutation_output(restarted.receive(4, DISCOVERY_TIMEOUT)?, &schema, "blocked")?;
    assert_eq!(wrong["error_code"], "permission_denied");
    restarted.finish()?;
    assert_fixture_tree(&fixture, &after)?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial runtime qualification"]
fn fix_nonzero_or_missing_lock_never_retains_a_candidate_or_changes_host() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    for missing_lock in [false, true] {
        let mut fixture = Fixture::new()?;
        fixture.assert_clean(None)?;
        if missing_lock {
            fs::remove_file(fixture.project.join("Cargo.lock"))?;
        } else {
            fs::write(
                fixture.project.join("app/src/lib.rs"),
                "pub fn broken() { let mut value = 1; no_such_function(value); }\n",
            )?;
        }
        let source_before = fs::read(fixture.project.join("app/src/lib.rs"))?;
        let manifest_before = fs::read(fixture.project.join("Cargo.toml"))?;
        let lock_before = fs::read(fixture.project.join("Cargo.lock")).ok();
        let mut server = Server::start_with_mutations(&fixture, false, false, true)?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        let schema = tool_schema(&mut server, 3, "rust.fix.apply")?;
        server.send(fix_call(4, &opened, preview(&opened)))?;
        let failed = mutation_output(server.receive(4, JOIN_TIMEOUT)?, &schema, "failed")?;
        assert!(failed["data"].is_null());
        assert_eq!(
            fs::read(fixture.project.join("app/src/lib.rs"))?,
            source_before
        );
        assert_eq!(
            fs::read(fixture.project.join("Cargo.toml"))?,
            manifest_before
        );
        assert_eq!(
            fs::read(fixture.project.join("Cargo.lock")).ok(),
            lock_before
        );
        server.finish()?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
    }
    Ok(())
}

fn active_fix_descendants(server: &mut Server, fixture: &Fixture, id: i64) -> Result<String> {
    let deadline = Instant::now() + Duration::from_secs(20);
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
            "status=running",
            "--format",
            "{{.Names}}\t{{.Command}}",
        ]);
        let records = String::from_utf8(bounded_command(list)?)?;
        for record in records.lines() {
            let Some((name, command)) = record.split_once('\t') else {
                continue;
            };
            let Some(nonce) = name.strip_prefix("rust-mcp-mutation-fix-") else {
                continue;
            };
            if nonce.len() != 32
                || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
                || !command
                    .trim_matches('"')
                    .starts_with("/opt/rust/bin/cargo fix --workspace --all-targets")
            {
                continue;
            }
            let mut top = runtime_observer(fixture);
            top.args(["container", "top", name, "-eo", "pid,args"]);
            let processes = String::from_utf8(bounded_command(top)?)?;
            if processes.contains("/build-script-build") && processes.contains("/usr/bin/sleep 120")
            {
                server.assert_no_response(id)?;
                return Ok(nonce.into());
            }
        }
        if Instant::now() >= deadline {
            return Err("active cargo fix build script and descendant were not observed".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial runtime qualification"]
fn fix_active_descendants_join_on_cancellation_timeout_and_eof_without_host_changes() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let mut server = Server::start_with_mutations(&fixture, false, false, true)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let schema = tool_schema(&mut server, 3, "rust.fix.apply")?;
    // Calibrate before observing project jobs; no host Cargo touches this fixture.
    server.send(fix_call(4, &opened, preview(&opened)))?;
    mutation_output(server.receive(4, JOIN_TIMEOUT)?, &schema, "passed")?;
    fixture.assert_clean(None)?;
    fs::write(
        fixture.project.join("app/build.rs"),
        "fn main() { let mut child = std::process::Command::new(\"/usr/bin/sleep\").arg(\"120\").spawn().unwrap(); let _ = child.wait(); }\n",
    )?;
    let mut expected = fixture.source_bytes()?;
    expected.insert(
        "app/build.rs".into(),
        fs::read(fixture.project.join("app/build.rs"))?,
    );
    server.send(fix_call(10, &opened, preview(&opened)))?;
    let cancelled = active_fix_descendants(&mut server, &fixture, 10)?;
    server.send(
        json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":10}}),
    )?;
    let deadline = Instant::now() + JOIN_TIMEOUT;
    loop {
        if fixture.objects("container", Some(&cancelled))?.is_empty()
            && fixture.objects("volume", Some(&cancelled))?.is_empty()
        {
            break;
        }
        if Instant::now() >= deadline {
            return Err("cancelled Fix objects survived cleanup".into());
        }
        thread::sleep(Duration::from_millis(25));
    }
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;
    server.send(request(11, "tools/list"))?;
    assert_eq!(
        server.receive(11, DISCOVERY_TIMEOUT)?["result"]["tools"]
            .as_array()
            .map(Vec::len),
        Some(22)
    );

    server.send(fix_call(20, &opened, preview(&opened)))?;
    let timed_out = active_fix_descendants(&mut server, &fixture, 20)?;
    assert_ne!(timed_out, cancelled);
    let failed = mutation_output(server.receive(20, JOIN_TIMEOUT)?, &schema, "failed")?;
    assert_eq!(failed["error_code"], "command_timeout");
    assert!(failed["data"].is_null());
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    server.send(fix_call(30, &opened, preview(&opened)))?;
    let eof = active_fix_descendants(&mut server, &fixture, 30)?;
    assert_ne!(eof, timed_out);
    server.finish()?;
    // Cancellation can either suppress the response or report the explicit M2
    // cancelled outcome. Neither may deliver a candidate or receipt of effects.
    for id in [10, 30] {
        if let Some(response) = server.pending.remove(&id) {
            let cancelled = mutation_output(response, &schema, "cancelled")?;
            assert!(cancelled["data"].is_null());
        }
    }
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;
    println!(
        "M2_FIX_LIFECYCLE {}",
        json!({"active_build_scripts_and_descendants":3,
        "cancelled_nonce":cancelled,"timeout_nonce":timed_out,"eof_nonce":eof,
        "host_source_unchanged":true,"owned_containers":[],"owned_volumes":[]})
    );
    fixture.successful = true;
    Ok(())
}
