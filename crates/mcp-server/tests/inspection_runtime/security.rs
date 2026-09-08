//! M4-01 through the real stdio/Tasks/Resource boundary.
#![cfg(feature = "test-hooks")]
use super::*;
#[path = "security_catalog.rs"]
mod security_catalog;
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
fn start(fixture: &Fixture, flags: &[std::ffi::OsString]) -> Result<Server> {
    start_on_image(
        fixture,
        flags,
        rust_engineering_execution::APPROVED_M4_IMAGE,
    )
}
fn start_on_image(fixture: &Fixture, flags: &[std::ffi::OsString], image: &str) -> Result<Server> {
    start_with_binary(
        fixture,
        flags,
        image,
        std::path::Path::new(env!("CARGO_BIN_EXE_rust-engineering-mcp")),
    )
}
fn start_with_binary(
    fixture: &Fixture,
    flags: &[std::ffi::OsString],
    image: &str,
    binary: &std::path::Path,
) -> Result<Server> {
    let mut command = Command::new(binary);
    command
        .env_clear()
        .env("RUST_MCP_TEST_SECURITY_READY", "1")
        .env("RUST_MCP_TEST_SCANNER_READY", "1")
        .env("RUST_MCP_TEST_MIRI_READY", "1")
        .env("RUST_MCP_TEST_SUPPLY_READY", "1")
        .env("RUST_MCP_TEST_GATE_V2_READY", "1")
        .current_dir(&fixture.root)
        .args(["serve", "--stdio", "--root"])
        .arg(&fixture.project)
        .arg("--docker")
        .arg(DOCKER)
        .arg("--docker-socket")
        .arg(&fixture.socket)
        .arg("--state-root")
        .arg(&fixture.state)
        .arg("--rust-image")
        .arg(image)
        .args(flags);
    let mut child = command
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
    format!(
        "sha256:{}",
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    )
}
struct Proceed;
impl rust_engineering_application::OperationControl for Proceed {
    fn check(&self) -> std::result::Result<(), rust_engineering_application::ProjectError> {
        Ok(())
    }
}
fn task(server: &mut Server, reference: &Value, id: i64) -> Result<Value> {
    server.send(task_call(
        id,
        "rust.deny",
        json!({"project_ref":reference,"execution_mode":"task","timeout_seconds":120}),
    ))?;
    let created = server.receive(id, DISCOVERY_TIMEOUT)?;
    assert_eq!(created["result"]["status"], "working", "{created}");
    let task_id = created["result"]["taskId"].as_str().ok_or("task id")?;
    let deadline = Instant::now() + JOIN_TIMEOUT;
    let mut next = id + 1;
    loop {
        server.send(task_request(next, "tasks/get", json!({"taskId":task_id})))?;
        let state = server.receive(next, CONTROL_TIMEOUT)?;
        next += 1;
        match state["result"]["status"].as_str() {
            Some("working") => {}
            Some("completed") => return Ok(state["result"]["result"]["structuredContent"].clone()),
            _ => return Err(format!("unexpected task state: {state}").into()),
        }
        if Instant::now() > deadline {
            return Err("security task deadline".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}
#[test]
#[ignore = "explicit M4 image/socket and test-hooks, serial Docker"]
fn deny_native_mcp_tasks_policy_licenses_and_owner_bound_redacted_resource() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial poisoned")?;
    let mut fixture = Fixture::new()?;
    let flags = prepare_security_fixture(&fixture)?;
    let license = include_bytes!("../../../../fixtures/m4-deny-native/LICENSE-MIT");
    let policy_path = fixture.root.join("policy.json");
    let policy = fs::read(&policy_path)?;
    let mut server = start(&fixture, &flags)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let clean = task(&mut server, &opened["project_ref"], 10)?;
    assert_eq!(clean["status"], "passed", "{clean}");
    assert_eq!(clean["data"]["completeness"], "complete");
    assert_eq!(clean["data"]["audit"]["state"], "passed");
    assert_eq!(clean["data"]["engines"]["parse_complete"], true);
    assert_eq!(clean["data"]["engines"]["cargo_deny_version"], "0.19.7");
    let uri = clean["data"]["artifacts"][0]["uri"]
        .as_str()
        .ok_or("artifact uri")?;
    server.send(resource_read_request(1000, uri))?;
    let resource = server.receive(1000, DISCOVERY_TIMEOUT)?;
    let blob = resource["result"]["contents"][0]["blob"]
        .as_str()
        .ok_or("artifact blob")?;
    let bytes = STANDARD.decode(blob)?;
    assert_eq!(
        clean["data"]["artifacts"][0]["sha256"],
        fingerprint(&bytes).trim_start_matches("sha256:")
    );
    let log: Value = serde_json::from_slice(&bytes)?;
    assert_eq!(log["schema_version"], 1);
    assert!(
        log["raw"]["stderr"]["bytes"]
            .as_u64()
            .is_some_and(|n| n > 0)
    );
    assert!(!String::from_utf8_lossy(&bytes).contains("rust-mcp-inspection-wire"));
    fs::remove_file(fixture.project.join("app/LICENSE-MIT"))?;
    let missing = task(&mut server, &opened["project_ref"], 1100)?;
    assert_eq!(missing["status"], "failed", "{missing}");
    assert_eq!(missing["data"]["policy_state"], "violated");
    assert!(
        missing["data"]["engines"]["licenses"]["errors"]
            .as_u64()
            .is_some_and(|n| n > 0)
    );
    fs::write(fixture.project.join("app/LICENSE-MIT"), license)?;
    fs::write(&policy_path, b"{}")?;
    let tampered = task(&mut server, &opened["project_ref"], 2100)?;
    assert_eq!(tampered["status"], "blocked", "{tampered}");
    assert_eq!(tampered["error_code"], "SECURITY_POLICY_INVALID");
    // Withdrawing the pinned policy disables new work while historical audit
    // evidence remains readable by its original owner.
    server.send(resource_read_request(3000, uri))?;
    let retained = server.receive(3000, DISCOVERY_TIMEOUT)?;
    assert!(retained.get("error").is_none(), "{retained}");
    let retained_bytes = STANDARD.decode(
        retained["result"]["contents"][0]["blob"]
            .as_str()
            .ok_or("retained evidence")?,
    )?;
    assert_eq!(retained_bytes, bytes);
    fs::write(&policy_path, &policy)?;
    server.finish()?;
    fixture.assert_clean(None)?;
    // Withdraw the optional security plugin by selecting the admitted M3 runtime.
    // A fresh session reopens the same granted root and can still read v1 audit
    // evidence, but it cannot admit a new deny run on that runtime.
    let mut server = start_on_image(
        &fixture,
        &flags,
        rust_engineering_execution::APPROVED_RUST_IMAGE,
    )?;
    let (reopened, _) = server.bootstrap_open(&fixture)?;
    let disabled = task(&mut server, &reopened["project_ref"], 10)?;
    assert_eq!(disabled["status"], "unavailable", "{disabled}");
    assert_eq!(disabled["error_code"], "TOOL_NOT_INSTALLED", "{disabled}");
    let resumed_uri = uri.replace(
        opened["project_ref"].as_str().ok_or("old owner")?,
        reopened["project_ref"].as_str().ok_or("new owner")?,
    );
    server.send(resource_read_request(3000, &resumed_uri))?;
    let after_rollback = server.receive(3000, DISCOVERY_TIMEOUT)?;
    assert!(after_rollback.get("error").is_none(), "{after_rollback}");
    assert_eq!(
        STANDARD.decode(
            after_rollback["result"]["contents"][0]["blob"]
                .as_str()
                .ok_or("rollback evidence")?
        )?,
        bytes
    );
    fs::rename(&fixture.project, fixture.root.join("revoked"))?;
    server.send(resource_read_request(3100, &resumed_uri))?;
    let revoked = server.receive(3100, DISCOVERY_TIMEOUT)?;
    assert!(revoked.get("error").is_some(), "{revoked}");
    server.finish()?;
    fixture.assert_clean(None)?;
    let record = json!({"clean":clean,"missing_license":missing,"tampered_policy":tampered,"policy_withdrawal_preserves_historical_audit":true,"plugin_withdrawal_preserves_historical_audit":true,"withdrawn_runtime":rust_engineering_execution::APPROVED_RUST_IMAGE,"normalized_log":log,"revoked_resource":revoked,"cleanup":true,"image":rust_engineering_execution::APPROVED_M4_IMAGE});
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-deny-mcp.json");
    fs::write(&output, serde_json::to_vec_pretty(&record)?)?;
    println!("M4_DENY_MCP {}", fingerprint(&serde_json::to_vec(&record)?));
    fixture.successful = true;
    Ok(())
}

fn prepare_security_fixture(fixture: &Fixture) -> Result<Vec<std::ffi::OsString>> {
    let manifest_path = fixture.project.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)?.replace(
        "path = \"helper\",",
        "path = \"helper\", version = \"=0.1.0\",",
    );
    fs::write(manifest_path, manifest)?;
    let license = include_bytes!("../../../../fixtures/m4-deny-native/LICENSE-MIT");
    for package in ["app", "helper"] {
        let path = fixture.project.join(format!("{package}/Cargo.toml"));
        let manifest =
            fs::read_to_string(&path)?.replace("[package]\n", "[package]\nlicense=\"MIT\"\n");
        fs::write(path, manifest)?;
        fs::write(
            fixture.project.join(format!("{package}/LICENSE-MIT")),
            license,
        )?;
    }
    let vendor = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/cargo-vendor-data/vendor")
        .canonicalize()?;
    let vendor_hash = rust_engineering_project::inspect_cargo_vendor(&vendor, &Proceed)
        .map_err(|e| format!("{e:?}"))?
        .tree_fingerprint;
    let policy = serde_json::to_vec(
        &json!({"schema_version":1,"rules":{"allowed_licenses":["MIT"],"banned_packages":[],"multiple_versions":"deny","wildcards":"deny"},"suppressions":[]}),
    )?;
    let policy_path = fixture.root.join("policy.json");
    fs::write(&policy_path, &policy)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let snapshot = serde_json::to_vec(
        &json!({"format_version":1,"sequence":1,"source_id":"fixture-m4-not-publisher-authenticated","created_at":now,"observed_at":now,"records":[{"path":"crates/rsa/RUSTSEC-2023-0071.md","markdown":include_str!("../../../catalog-adapter/tests/fixtures/rustsec/RUSTSEC-2023-0071.md")}]}),
    )?;
    let snapshot_path = fixture.root.join("rustsec.json");
    fs::write(&snapshot_path, &snapshot)?;
    let flags = vec![
        "--security-policy".into(),
        policy_path.as_os_str().into(),
        "--security-policy-sha256".into(),
        fingerprint(&policy).into(),
        "--cargo-vendor-dir".into(),
        vendor.as_os_str().into(),
        "--cargo-vendor-tree-sha256".into(),
        vendor_hash.to_string().into(),
        "--rustsec-snapshot".into(),
        snapshot_path.as_os_str().into(),
        "--rustsec-sha256".into(),
        fingerprint(&snapshot).into(),
    ];
    Ok(flags)
}

fn security_task(server: &mut Server, name: &str, arguments: Value, id: i64) -> Result<Value> {
    server.send(task_call(id, name, arguments))?;
    let created = server.receive(id, DISCOVERY_TIMEOUT)?;
    assert_eq!(created["result"]["status"], "working", "{created}");
    let task_id = created["result"]["taskId"].as_str().ok_or("task id")?;
    let deadline = Instant::now() + JOIN_TIMEOUT;
    let mut next = id + 1;
    loop {
        server.send(task_request(next, "tasks/get", json!({"taskId":task_id})))?;
        let state = server.receive(next, CONTROL_TIMEOUT)?;
        next += 1;
        match state["result"]["status"].as_str() {
            Some("working") => (),
            Some("completed") => {
                let result = state["result"]["result"].clone();
                assert!(serde_json::to_vec(&result)?.len() <= 512 * 1024);
                let mirrored: Value = serde_json::from_str(
                    result["content"][0]["text"].as_str().ok_or("text mirror")?,
                )?;
                assert_eq!(mirrored, result["structuredContent"]);
                return Ok(result);
            }
            _ => return Err(format!("unexpected {name} state: {state}").into()),
        }
        if Instant::now() > deadline {
            return Err(format!("{name} deadline").into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "explicit M4 image, Tasks/resources, native composition and serial Docker"]
fn m4_tools_native_mcp_observations_composition_and_private_resources() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial poisoned")?;
    let mut fixture = Fixture::new()?;
    let flags = prepare_security_fixture(&fixture)?;
    let clean = "pub fn value() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn value_is_one() {\n        assert_eq!(super::value(), 1);\n    }\n}\n";
    for package in ["app", "helper"] {
        fs::write(fixture.project.join(package).join("src/lib.rs"), clean)?;
    }
    let image = "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635";
    let mut server = start_on_image(&fixture, &flags, image)?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let reference = &opened["project_ref"];
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let mut evidence = Vec::new();
    let mut uris = Vec::new();
    for (index, name) in [
        "rust.unsafe.scan",
        "rust.miri",
        "rust.supply_chain.inspect",
        "rust.quality.gate.v2",
    ]
    .into_iter()
    .enumerate()
    {
        let id = 10_000 + (index as i64) * 10_000;
        let mut arguments =
            json!({"project_ref":reference,"execution_mode":"task","timeout_seconds":120});
        if name == "rust.quality.gate.v2" {
            arguments["profile"] = json!("strict");
            arguments["timeout_seconds"] = json!(300);
        }
        let result = security_task(&mut server, name, arguments, id)?;
        let body = &result["structuredContent"];
        let definition = list["result"]["tools"]
            .as_array()
            .ok_or("tools")?
            .iter()
            .find(|t| t["name"] == name)
            .ok_or("new tool absent")?;
        let schema = jsonschema::validator_for(&definition["outputSchema"])?;
        assert!(schema.is_valid(body), "{name}: {body}");
        if name == "rust.supply_chain.inspect" {
            assert_eq!(body["status"], "blocked", "{body}");
            assert_eq!(
                body["data"]["observation"]["report"]["audit"]["state"],
                "passed"
            );
            assert_eq!(
                body["data"]["observation"]["report"]["deny_availability"],
                "available"
            );
            assert_eq!(body["data"]["observation"]["report"]["packages_total"], 2);
        } else {
            assert_eq!(body["status"], "passed", "{name}: {body}");
        }
        if name == "rust.quality.gate.v2" {
            let report = &body["data"]["observation"]["report"];
            assert_eq!(report["complete"], true);
            assert_eq!(report["stages"].as_array().ok_or("stages")?.len(), 7);
        }
        let uri = body["data"]["artifacts"][0]["uri"]
            .as_str()
            .ok_or("resource")?
            .to_string();
        server.send(resource_read_request(id + 9000, &uri))?;
        let resource = server.receive(id + 9000, DISCOVERY_TIMEOUT)?;
        let bytes = STANDARD.decode(
            resource["result"]["contents"][0]["blob"]
                .as_str()
                .ok_or("resource blob")?,
        )?;
        assert_eq!(
            body["data"]["artifacts"][0]["sha256"],
            fingerprint(&bytes).trim_start_matches("sha256:")
        );
        let normalized: Value = serde_json::from_slice(&bytes)?;
        assert!(!String::from_utf8_lossy(&bytes).contains("rust-mcp-inspection-wire"));
        uris.push(uri);
        evidence.push(json!({"tool":name,"result":result,"resource":normalized}));
        println!("M4_NATIVE_MCP {name}");
    }
    let release = security_task(
        &mut server,
        "rust.quality.gate.v2",
        json!({"project_ref":reference,"baseline_project_ref":reference,"profile":"release","execution_mode":"task","timeout_seconds":300}),
        60_000,
    )?;
    assert_eq!(
        release["structuredContent"]["status"], "blocked",
        "{release}"
    );
    let stages = release["structuredContent"]["data"]["observation"]["report"]["stages"]
        .as_array()
        .ok_or("release stages")?;
    assert_eq!(stages.len(), 8);
    assert_eq!(stages[7]["stage"], "semver");
    assert_eq!(stages[7]["status"], "blocked");
    evidence.push(json!({"tool":"release-incomplete-lock","result":release}));
    // The workspace fixture requires the plugin to rewrite its lock. A separate
    // already-locked single-library fixture provides the positive release oracle.
    fs::write(
        fixture.project.join("Cargo.toml"),
        "[package]\nname='release_positive'\nversion='1.0.0'\nedition='2024'\nlicense='MIT'\n",
    )?;
    fs::write(
        fixture.project.join("Cargo.lock"),
        include_str!("../../../../fixtures/semver/identical/candidate/Cargo.lock")
            .replace("semver-identical", "release_positive"),
    )?;
    fs::create_dir(fixture.project.join("src"))?;
    fs::write(fixture.project.join("src/lib.rs"), clean)?;
    fs::write(
        fixture.project.join("LICENSE-MIT"),
        include_bytes!("../../../../fixtures/m4-deny-native/LICENSE-MIT"),
    )?;
    server.send(call(
        69_000,
        "rust.project.open",
        json!({"path":fixture.project}),
    ))?;
    let reopened = server.receive(69_000, DISCOVERY_TIMEOUT)?;
    assert_eq!(
        reopened["result"]["structuredContent"]["status"], "passed",
        "{reopened}"
    );
    let release_reference = &reopened["result"]["structuredContent"]["data"]["project_ref"];
    let release = security_task(
        &mut server,
        "rust.quality.gate.v2",
        json!({"project_ref":release_reference,"baseline_project_ref":release_reference,"profile":"release","execution_mode":"task","timeout_seconds":300}),
        70_000,
    )?;
    assert_eq!(
        release["structuredContent"]["status"], "passed",
        "{release}"
    );
    evidence.push(json!({"tool":"release-complete","result":release}));
    fs::rename(&fixture.project, fixture.root.join("revoked"))?;
    for (index, uri) in uris.iter().enumerate() {
        let id = 80_000 + index as i64;
        server.send(resource_read_request(id, uri))?;
        assert!(
            server
                .receive(id, DISCOVERY_TIMEOUT)?
                .get("error")
                .is_some()
        );
    }
    server.finish()?;
    fixture.assert_clean(None)?;
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-tools-mcp.json");
    fs::write(
        output,
        serde_json::to_vec_pretty(
            &json!({"image_id":image,"cases":evidence,"cleanup_verified":true,"revoked_resources":uris.len()}),
        )?,
    )?;
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "30 cold and 30 warm M4 observations; exclusive Docker and explicit test hooks"]
fn m4_records_thirty_cold_and_warm_operation_budgets() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial poisoned")?;
    let mut frozen = Fixture::new()?;
    let binary = frozen.root.join("qualified-server");
    fs::copy(env!("CARGO_BIN_EXE_rust-engineering-mcp"), &binary)?;
    let binary_sha256 = fingerprint(&fs::read(&binary)?);
    let names = [
        "rust.deny",
        "rust.unsafe.scan",
        "rust.miri",
        "rust.supply_chain.inspect",
        "rust.quality.gate.v2",
    ];
    let mut evidence = Vec::new();
    for sample in 0..30 {
        let mut fixture = Fixture::new()?;
        let mut flags = prepare_security_fixture(&fixture)?;
        let clean = "pub fn value() -> u32 {\n    1\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn value_is_one() {\n        assert_eq!(super::value(), 1);\n    }\n}\n";
        for package in ["app", "helper"] {
            fs::write(fixture.project.join(package).join("src/lib.rs"), clean)?;
        }
        let catalog = fixture.root.join("catalog");
        security_catalog::install(
            &catalog,
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
        )?;
        flags.extend([
            "--catalog-store".into(),
            catalog.as_os_str().into(),
            "--catalog-trust".into(),
            catalog.join("trust.json").into_os_string(),
        ]);
        assert_eq!(fingerprint(&fs::read(&binary)?), binary_sha256);
        let mut server = start_with_binary(
            &fixture,
            &flags,
            rust_engineering_execution::APPROVED_M4_IMAGE,
            &binary,
        )?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        let reference = &opened["project_ref"];
        let calibration = Instant::now();
        server.send(call(
            4,
            "rust.project.inspect",
            json!({"project_ref":reference}),
        ))?;
        let inspected = server.receive(4, JOIN_TIMEOUT)?;
        assert_eq!(
            inspected["result"]["structuredContent"]["status"], "passed",
            "{inspected}"
        );
        let calibration_ms = calibration.elapsed().as_millis();
        for (index, name) in names.into_iter().enumerate() {
            for (temperature, offset) in [("cold", 0), ("warm", 5000)] {
                let mut arguments =
                    json!({"project_ref":reference,"execution_mode":"task","timeout_seconds":60});
                if name == "rust.quality.gate.v2" {
                    arguments["profile"] = json!("strict");
                }
                let started = Instant::now();
                let result = security_task(
                    &mut server,
                    name,
                    arguments,
                    10_000 + (index as i64) * 10_000 + offset,
                )?;
                let elapsed = started.elapsed().as_millis();
                assert_eq!(
                    result["structuredContent"]["status"], "passed",
                    "{name} {temperature}: {result}"
                );
                evidence.push(json!({"sample":sample,"tool":name,"temperature":temperature,"elapsed_ms":elapsed,"tool_duration_ms":result["structuredContent"]["duration_ms"],"calibration_ms":calibration_ms,"result_sha256":fingerprint(&serde_json::to_vec(&result)?),"reply_bytes":serde_json::to_vec(&result)?.len()}));
            }
        }
        server.finish()?;
        fixture.assert_clean(None)?;
        fixture.successful = true;
        let output =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-budgets-running.json");
        fs::write(
            output,
            serde_json::to_vec_pretty(
                &json!({"status":"running","completed_samples":sample+1,"measurements":evidence}),
            )?,
        )?;
        println!("M4_BUDGET_SAMPLE {}", sample + 1);
    }
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-budgets.json");
    fs::write(
        output,
        serde_json::to_vec_pretty(
            &json!({"status":"passed","samples_each":30,"image_id":rust_engineering_execution::APPROVED_M4_IMAGE,"binary_sha256":binary_sha256,"calibration_excluded_from_operation_timer":true,"cold_definition":"first execution of each tool in a new calibrated MCP session; warm repeats same tool in that session; each guest build remains ephemeral","measurements":evidence}),
        )?,
    )?;
    frozen.successful = true;
    Ok(())
}

fn observe_miri_task(fixture: &Fixture) -> Result {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        let output = Command::new(DOCKER)
            .args([
                "--host",
                &format!("unix://{}", fixture.socket),
                "container",
                "ls",
                "--filter=label=org.rust-mcp.execution=true",
                "--no-trunc",
                "--format={{.Command}}",
            ])
            .output()?;
        assert!(output.status.success());
        if String::from_utf8_lossy(&output.stdout)
            .contains("/opt/rust-nightly-2026-09-07/bin/cargo miri nextest run")
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("Miri container was never observed running".into());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[test]
#[ignore = "explicit M4 image, exclusive Docker and active Miri lifecycle"]
fn m4_miri_tasks_cancel_eof_and_revocation_join_before_releasing_authority() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial poisoned")?;
    let mut rows = Vec::new();
    for scenario in ["cancel", "eof", "revocation"] {
        let mut fixture = Fixture::new()?;
        let flags = prepare_security_fixture(&fixture)?;
        fs::write(
            fixture.project.join("app/src/lib.rs"),
            "#[test] fn running() { let mut x=0u64; loop { x=x.wrapping_add(1); std::hint::black_box(x); } }\n",
        )?;
        let mut server = start_on_image(
            &fixture,
            &flags,
            rust_engineering_execution::APPROVED_M4_IMAGE,
        )?;
        let (opened, _) = server.bootstrap_open(&fixture)?;
        server.send(task_call(10,"rust.miri",json!({"project_ref":opened["project_ref"],"execution_mode":"task","timeout_seconds":120})))?;
        let created = server.receive(10, DISCOVERY_TIMEOUT)?;
        assert_eq!(created["result"]["status"], "working", "{created}");
        let task_id = created["result"]["taskId"]
            .as_str()
            .ok_or("task id")?
            .to_owned();
        observe_miri_task(&fixture)?;
        let started = Instant::now();
        let renamed = fixture.root.join("revoked-source");
        if scenario == "revocation" {
            fs::rename(&fixture.project, &renamed)?;
        }
        if scenario == "cancel" {
            server.send(task_request(11, "tasks/cancel", json!({"taskId":task_id})))?;
            let cancelled = server.receive(11, CONTROL_TIMEOUT)?;
            assert!(cancelled.get("error").is_none(), "{cancelled}");
        }
        if scenario != "eof" {
            let deadline = Instant::now() + JOIN_TIMEOUT;
            let mut id = 12;
            loop {
                server.send(task_request(id, "tasks/get", json!({"taskId":task_id})))?;
                let state = server.receive(id, CONTROL_TIMEOUT)?;
                id += 1;
                if scenario == "revocation" {
                    assert_eq!(state["error"]["message"], "task unavailable", "{state}");
                    assert_eq!(state["error"]["code"], -32602, "{state}");
                    break;
                }
                if state["result"]["status"] == "cancelled" {
                    break;
                }
                assert_eq!(state["result"]["status"], "working", "{state}");
                if Instant::now() >= deadline {
                    return Err("Miri cancellation was not joined".into());
                }
                thread::sleep(Duration::from_millis(25));
            }
        }
        if scenario != "eof" {
            let deadline = Instant::now() + JOIN_TIMEOUT;
            while fixture.assert_clean(None).is_err() {
                if Instant::now() >= deadline {
                    return Err("Miri cleanup did not finish before EOF".into());
                }
                thread::sleep(Duration::from_millis(50));
            }
        }
        server.finish()?;
        fixture.assert_clean(None)?;
        if scenario == "revocation" {
            fs::rename(renamed, &fixture.project)?;
        }
        rows.push(json!({"scenario":scenario,"miri_running_observed":true,"join_ms":started.elapsed().as_millis(),"cleanup_verified":true,"cleanup_before_eof":scenario != "eof"}));
        fixture.successful = true;
    }
    fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-miri-task-lifecycle.json"),
        serde_json::to_vec_pretty(&rows)?,
    )?;
    Ok(())
}

#[test]
#[ignore = "explicit M4 runtime; hostile output canaries across MCP and durable Resources"]
fn m4_project_output_canaries_are_absent_from_security_results_and_resources() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial poisoned")?;
    let mut fixture = Fixture::new()?;
    let mut flags = prepare_security_fixture(&fixture)?;
    let catalog = fixture.root.join("catalog");
    security_catalog::install(
        &catalog,
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
    )?;
    flags.extend([
        "--catalog-store".into(),
        catalog.as_os_str().into(),
        "--catalog-trust".into(),
        catalog.join("trust.json").into_os_string(),
    ]);
    let canary = "M4_PRIVATE_TOKEN_8ae759ec53e04f77";
    let source = format!(
        "pub fn value() -> u32 {{ 1 }}\n#[test] fn hostile_output() {{ println!(\"Undefined Behavior: {{}}\", \"{canary}\"); panic!(\"ordinary test failure after forged text\"); }}\n"
    );
    fs::write(fixture.project.join("app/src/lib.rs"), source)?;
    let mut server = start_on_image(
        &fixture,
        &flags,
        rust_engineering_execution::APPROVED_M4_IMAGE,
    )?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let mut evidence = Vec::new();
    for (index, name) in [
        "rust.deny",
        "rust.unsafe.scan",
        "rust.miri",
        "rust.supply_chain.inspect",
        "rust.quality.gate.v2",
    ]
    .into_iter()
    .enumerate()
    {
        let mut arguments = json!({"project_ref":opened["project_ref"],"execution_mode":"task","timeout_seconds":120});
        if name == "rust.quality.gate.v2" {
            arguments["profile"] = json!("strict");
        }
        let id = 10_000 + index as i64 * 10_000;
        let result = security_task(&mut server, name, arguments, id)?;
        let encoded = serde_json::to_string(&result)?;
        assert!(!encoded.contains(canary), "{name} leaked project output");
        assert!(
            result["structuredContent"]["data"].is_object(),
            "{name}: {result}"
        );
        if name == "rust.miri" {
            assert_eq!(result["structuredContent"]["status"], "failed", "{result}");
            assert_eq!(
                result["structuredContent"]["data"]["observation"]["report"]["counts"]["undefined_behavior"],
                0,
                "{result}"
            );
            let report = &result["structuredContent"]["data"]["observation"]["report"];
            assert_eq!(report["counts"]["test_failures"], 1, "{report}");
            assert_eq!(report["counts"]["compile_failures"], 0, "{report}");
            assert!(
                report["findings"]
                    .as_array()
                    .ok_or("Miri findings")?
                    .iter()
                    .any(|finding| finding["test_name"] == "hostile_output"),
                "{report}"
            );
        } else if name != "rust.quality.gate.v2" {
            assert_eq!(result["structuredContent"]["status"], "passed", "{result}");
        }
        let artifacts = result["structuredContent"]["data"]["artifacts"]
            .as_array()
            .ok_or("artifacts")?;
        assert!(!artifacts.is_empty());
        let mut resources = Vec::new();
        for (artifact_index, artifact) in artifacts.iter().enumerate() {
            let uri = artifact["uri"]
                .as_str()
                .ok_or("security resource missing")?;
            let resource_id = id + 9000 + artifact_index as i64;
            server.send(resource_read_request(resource_id, uri))?;
            let resource = server.receive(resource_id, DISCOVERY_TIMEOUT)?;
            assert!(resource.get("error").is_none(), "{resource}");
            assert_eq!(resource["result"]["cacheScope"], "private");
            let content = &resource["result"]["contents"][0];
            let bytes = STANDARD.decode(content["blob"].as_str().ok_or("resource blob")?)?;
            assert!(
                !String::from_utf8_lossy(&bytes).contains(canary),
                "{name} resource leaked project output"
            );
            resources.push(json!({"bytes":bytes.len(),"sha256":fingerprint(&bytes)}));
        }
        evidence.push(json!({"tool":name,"status":result["structuredContent"]["status"],"result_bytes":encoded.len(),"resources":resources,"canary_absent":true}));
    }
    server.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-output-canaries.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    Ok(())
}
