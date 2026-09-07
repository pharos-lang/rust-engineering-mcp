use super::format_mutation_runtime::{mutation_output, tool_schema};
use super::*;

const EXECUTED_MARKER: &str = "RUST_MCP_HOSTILE_PROC_MACRO_EXECUTED";

fn fix_call(id: i64, opened: &Value) -> Value {
    call(
        id,
        "rust.fix.apply",
        json!({"project_ref":opened["project_ref"],"action":{
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"]
        }}),
    )
}

fn configure_proc_macro(fixture: &Fixture, implementation: &str) -> Result {
    fs::write(
        fixture.project.join("helper/Cargo.toml"),
        r#"[package]
name = "helper"
version.workspace = true
edition.workspace = true
[lib]
proc-macro = true
[features]
extra = []
"#,
    )?;
    fs::write(
        fixture.project.join("app/Cargo.toml"),
        r#"[package]
name = "app"
version.workspace = true
edition.workspace = true
[dependencies]
helper = { path = "../helper" }
"#,
    )?;
    fs::write(
        fixture.project.join("app/src/lib.rs"),
        r#"#[helper::hostile]
pub fn answer() -> u8 { let mut value = 42; value }
"#,
    )?;
    fs::write(fixture.project.join("helper/src/lib.rs"), implementation)?;
    Ok(())
}

fn marker_macro() -> &'static str {
    r#"extern crate proc_macro;
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn hostile(_attribute: TokenStream, _item: TokenStream) -> TokenStream {
    "compile_error!(\"RUST_MCP_HOSTILE_PROC_MACRO_EXECUTED\");"
        .parse()
        .unwrap()
}
"#
}

fn passthrough_macro() -> &'static str {
    r#"extern crate proc_macro;
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn hostile(_attribute: TokenStream, item: TokenStream) -> TokenStream {
    item
}
"#
}

fn mutating_macro(host_canary: &std::path::Path) -> Result<String> {
    let host_path = host_canary.to_str().ok_or("non-UTF8 host canary path")?;
    Ok(format!(
        r#"extern crate proc_macro;
use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn hostile(_attribute: TokenStream, item: TokenStream) -> TokenStream {{
    let _ = std::fs::OpenOptions::new()
        .append(true)
        .open("/source/Cargo.toml")
        .and_then(|mut file| std::io::Write::write_all(&mut file, b"\n# proc macro guest mutation\n"));
    let _ = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open({host_path:?})
        .and_then(|mut file| std::io::Write::write_all(&mut file, b"escaped"));
    item
}}
"#
    ))
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

fn assert_no_plan(output: &Value) {
    assert!(
        output["data"].is_null(),
        "failed preview retained public data"
    );
    assert!(output.get("plan_id").is_none());
    assert!(output.get("plan_digest").is_none());
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial hostile proc-macro qualification"]
fn fix_executes_hostile_proc_macro_and_retains_no_plan_after_compile_error() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    configure_proc_macro(&fixture, marker_macro())?;
    let before = fixture.source_bytes()?;

    let mut server = Server::start_with_mutations(&fixture, false, false, true)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let check_schema = tool_schema(&mut server, 3, "rust.check")?;
    server.send(call(
        4,
        "rust.check",
        json!({"project_ref":opened["project_ref"],"workspace":true,"all_targets":true}),
    ))?;
    let checked = checked_output(
        &server.receive(4, JOIN_TIMEOUT)?,
        &json!({"outputSchema":check_schema}),
        &opened,
        "failed",
    )?;
    assert!(
        checked["diagnostics"]
            .as_array()
            .is_some_and(
                |diagnostics| diagnostics.iter().any(|diagnostic| diagnostic["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(EXECUTED_MARKER)))
            ),
        "the real proc macro execution marker was not observed: {checked}"
    );

    let fix_schema = tool_schema(&mut server, 5, "rust.fix.apply")?;
    server.send(fix_call(6, &opened))?;
    let failed = mutation_output(server.receive(6, JOIN_TIMEOUT)?, &fix_schema, "failed")?;
    assert_eq!(failed["error_code"], "candidate_invalid");
    assert_no_plan(&failed);
    assert_fixture_tree(&fixture, &before)?;

    fs::write(
        fixture.project.join("helper/src/lib.rs"),
        passthrough_macro(),
    )?;
    let recoverable = fixture.source_bytes()?;
    let reopened = reopen(&mut server, &fixture, 7)?;
    server.send(fix_call(8, &reopened))?;
    let planned = mutation_output(server.receive(8, JOIN_TIMEOUT)?, &fix_schema, "passed")?;
    assert!(planned["data"]["plan_id"].as_str().is_some());
    assert_eq!(planned["data"]["files"][0]["path"], "app/src/lib.rs");
    assert_fixture_tree(&fixture, &recoverable)?;

    server.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial hostile proc-macro qualification"]
fn fix_rejects_proc_macro_manifest_mutation_and_host_escape_without_retaining_plan() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let canary = fixture.root.join("host-canary");
    fs::write(&canary, b"host-before")?;
    configure_proc_macro(&fixture, &mutating_macro(&canary)?)?;
    let before = fixture.source_bytes()?;

    let mut server = Server::start_with_mutations(&fixture, false, false, true)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let fix_schema = tool_schema(&mut server, 3, "rust.fix.apply")?;
    server.send(fix_call(4, &opened))?;
    let blocked = mutation_output(server.receive(4, JOIN_TIMEOUT)?, &fix_schema, "unavailable")?;
    assert_eq!(blocked["error_code"], "toolchain_unavailable");
    assert_no_plan(&blocked);
    assert_fixture_tree(&fixture, &before)?;
    assert_eq!(fs::read(&canary)?, b"host-before");

    fs::write(
        fixture.project.join("helper/src/lib.rs"),
        passthrough_macro(),
    )?;
    let recoverable = fixture.source_bytes()?;
    let reopened = reopen(&mut server, &fixture, 5)?;
    server.send(fix_call(6, &reopened))?;
    let planned = mutation_output(server.receive(6, JOIN_TIMEOUT)?, &fix_schema, "passed")?;
    assert!(planned["data"]["plan_id"].as_str().is_some());
    assert_eq!(planned["data"]["files"][0]["path"], "app/src/lib.rs");
    assert_fixture_tree(&fixture, &recoverable)?;
    assert_eq!(fs::read(&canary)?, b"host-before");

    server.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    Ok(())
}
