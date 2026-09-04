//! Real MCP calls against captured dependency data; no host Cargo on fixtures.
use super::*;
use sha2::{Digest, Sha256};
use std::path::Path;

const RSA: &str =
    include_str!("../../../catalog-adapter/tests/fixtures/rustsec/RUSTSEC-2023-0071.md");
const SOURCE_ID: &str = "fixture-rustsec-runtime-rsa-not-publisher-authenticated";

pub(super) fn start(fixture: &Fixture, snapshot: Option<(&Path, &str)>) -> Result<Server> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"));
    command
        .env_clear()
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
        .arg(APPROVED_RUST_IMAGE);
    if let Some((path, hash)) = snapshot {
        command
            .arg("--rustsec-snapshot")
            .arg(path)
            .arg("--rustsec-sha256")
            .arg(hash);
    }
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
    let hex: String = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("sha256:{hex}")
}
fn lock(version: &str) -> String {
    format!(
        "version = 4\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\"rsa\"]\n[[package]]\nname = \"rsa\"\nversion = \"{version}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n"
    )
}
fn prepare(fixture: &Fixture) -> Result<BTreeMap<String, Vec<u8>>> {
    fs::write(
        fixture.project.join("Cargo.toml"),
        "[workspace]\nmembers = [\"app\"]\nexclude = [\"helper\"]\nresolver = \"3\"\n",
    )?;
    fs::write(
        fixture.project.join("app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nrsa = \"0.9.6\"\n",
    )?;
    fs::write(fixture.project.join("Cargo.lock"), lock("0.9.6"))?;
    // If any project build script executes, even inside the sandbox, it fails.
    // The successful metadata/audit path proves it never needs project execution.
    let build = b"fn main() { panic!(\"AUDIT_MUST_NOT_EXECUTE_PROJECT_BUILD_SCRIPT\"); }\n";
    fs::write(fixture.project.join("app/build.rs"), build)?;
    let mut expected = fixture.source_bytes()?;
    expected.insert("app/build.rs".into(), build.to_vec());
    Ok(expected)
}
fn document(markdown: &str, created: Option<u64>, observed: Option<u64>) -> Value {
    json!({"format_version":1,"sequence":1,"source_id":SOURCE_ID,
        "created_at":created,"observed_at":observed,"records":[{
            "path":"crates/rsa/RUSTSEC-2023-0071.md","markdown":markdown}]})
}
fn snapshot(fixture: &Fixture, document: &Value) -> Result<(PathBuf, Vec<u8>, String)> {
    let bytes = serde_json::to_vec(document)?;
    let path = fixture.root.join("rustsec.json");
    assert!(!path.starts_with(&fixture.project));
    fs::write(&path, &bytes)?;
    let hash = fingerprint(&bytes);
    Ok((path, bytes, hash))
}
fn bootstrap(server: &mut Server, fixture: &Fixture) -> Result<(Value, Value)> {
    let (opened, _) = server.bootstrap_open(fixture)?;
    server.send(request(3, "tools/list"))?;
    let list = server.receive(3, DISCOVERY_TIMEOUT)?;
    let tool = list["result"]["tools"]
        .as_array()
        .ok_or("tools missing")?
        .iter()
        .find(|tool| tool["name"] == "rust.dependencies.audit")
        .ok_or("audit missing")?
        .clone();
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    Ok((opened, tool))
}
fn run(
    server: &mut Server,
    tool: &Value,
    opened: &Value,
    id: i64,
    status: &str,
    code: Option<&str>,
) -> Result<Value> {
    server.send(call(
        id,
        "rust.dependencies.audit",
        json!({"project_ref":opened["project_ref"]}),
    ))?;
    let response = server.receive(id, JOIN_TIMEOUT)?;
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(
        response["result"]["isError"],
        matches!(status, "blocked" | "unavailable"),
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
        .map_err(|e| e.to_string())?;
    assert_eq!(output["status"], status, "{output}");
    assert_eq!(output["error_code"], json!(code), "{output}");
    if !output["data"].is_null() {
        let data = &output["data"];
        assert_eq!(data["observation"]["state"], status, "{output}");
        assert_eq!(data["project_ref"], opened["project_ref"]);
        assert_eq!(data["project_identity_fingerprint"], opened["fingerprint"]);
        assert_eq!(data["semantics"], "latest_known");
        assert_eq!(data["runtime"]["image_id"], APPROVED_RUST_IMAGE);
        assert_eq!(data["runtime"]["platform"], "linux/aarch64");
        for fp in ["configuration_fingerprint", "execution_fingerprint"] {
            assert_fingerprint(&data["runtime"][fp]);
        }
        assert_fingerprint(&data["source_fingerprint"]);
    }
    Ok(output)
}
fn assert_snapshot(
    output: &Value,
    hash: &str,
    created: Option<u64>,
    observed: Option<u64>,
    lock_bytes: &[u8],
) {
    let audit = &output["data"]["observation"];
    assert_eq!(audit["snapshot_fingerprint"], hash);
    assert_eq!(audit["snapshot_record_count"], 1);
    assert_eq!(audit["snapshot_sequence"], 1);
    assert_eq!(audit["lock_fingerprint"], fingerprint(lock_bytes));
    assert_eq!(audit["packages_total"], 2);
    assert_eq!(audit["crates_io_scanned"], 1);
    assert_eq!(audit["workspace_packages_excluded"], 1);
    assert_eq!(audit["unsupported_packages"], json!([]));
    assert_eq!(audit["findings_omitted"], 0);
    let provenance = &audit["snapshot"]["provenance"];
    assert_eq!(provenance["source_kind"], "rustsec_snapshot");
    assert_eq!(provenance["source_id"], SOURCE_ID);
    assert_eq!(provenance["created_at"], json!(created));
    assert_eq!(provenance["observed_at"], json!(observed));
    assert_eq!(provenance["integrity"], "verified");
    assert_eq!(provenance["network_used"], false);
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn audit_real_rsa_and_lock_generations_are_captured_without_building() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime lock poisoned")?;
    let mut fixture = Fixture::new()?;
    fixture.assert_clean(None)?;
    let mut expected = prepare(&fixture)?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let (path, bytes, hash) = snapshot(&fixture, &document(RSA, Some(now), Some(now)))?;
    let mut server = start(&fixture, Some((&path, &hash)))?;
    let (opened, tool) = bootstrap(&mut server, &fixture)?;
    let first = run(&mut server, &tool, &opened, 10, "failed", None)?;
    assert_snapshot(&first, &hash, Some(now), Some(now), &expected["Cargo.lock"]);
    let audit = &first["data"]["observation"];
    assert_eq!(audit["validation_complete"], true);
    assert_eq!(audit["findings"].as_array().map(Vec::len), Some(1));
    let finding = &audit["findings"][0];
    assert_eq!(finding["advisory_id"], "RUSTSEC-2023-0071");
    assert_eq!(finding["severity"], "medium");
    assert_eq!(finding["package"]["version"], "0.9.6");
    assert_eq!(finding["patched_requirements"], json!([]));
    assert_eq!(finding["paths"][0]["workspace_root"]["name"], "app");
    assert_eq!(finding["paths"][0]["packages"][0]["name"], "app");
    assert_eq!(finding["paths"][0]["packages"][1]["name"], "rsa");
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    // Metadata --no-deps does not establish full manifest/lock synchronization.
    // Verify only the newly captured resolved lock generation and unchanged handle.
    let changed = lock("0.9.7");
    fs::write(fixture.project.join("Cargo.lock"), &changed)?;
    expected.insert("Cargo.lock".into(), changed.as_bytes().to_vec());
    let second = run(&mut server, &tool, &opened, 11, "failed", None)?;
    assert_snapshot(&second, &hash, Some(now), Some(now), changed.as_bytes());
    assert_eq!(
        second["data"]["observation"]["findings"][0]["package"]["version"],
        "0.9.7"
    );
    assert_ne!(
        first["data"]["source_fingerprint"],
        second["data"]["source_fingerprint"]
    );
    assert_ne!(
        first["data"]["observation"]["lock_fingerprint"],
        second["data"]["observation"]["lock_fingerprint"]
    );
    fixture.assert_clean(None)?;
    assert_fixture_tree(&fixture, &expected)?;

    let ambiguous = format!(
        "{}[[package]]\nname = \"rsa\"\nversion = \"0.9.8\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n",
        lock("0.9.6")
    );
    for (i, invalid) in ["not valid TOML".to_owned(), ambiguous]
        .into_iter()
        .enumerate()
    {
        fs::write(fixture.project.join("Cargo.lock"), &invalid)?;
        expected.insert("Cargo.lock".into(), invalid.into_bytes());
        run(
            &mut server,
            &tool,
            &opened,
            12 + i64::try_from(i)?,
            "blocked",
            Some("AUDIT_LOCKFILE_INVALID"),
        )?;
        fixture.assert_clean(None)?;
        assert_fixture_tree(&fixture, &expected)?;
    }
    server.finish()?;
    fixture.assert_clean(None)?;
    assert_eq!(fs::read(&path)?, bytes);
    println!(
        "M1_AUDIT_RECEIPT {}",
        json!({"cases":4,"real_rsa_finding":true,"severity":"medium","no_invented_patch":true,"root_path_verified":true,"lock_generations_sha256_verified":2,"invalid_and_ambiguous_lock_blocked":true,"source_unchanged_between_explicit_mutations":true,"project_build_script_not_executed":true,"cleanup":true,"configuration_fingerprint":first["data"]["runtime"]["configuration_fingerprint"],"snapshot_fingerprint":hash})
    );
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn audit_snapshot_freshness_and_advisory_classification_are_distinct() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime lock poisoned")?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let patched = RSA.replace("patched = []", "patched = [\">=0.9.6\"]");
    let withdrawn = RSA.replace(
        "package = \"rsa\"",
        "package = \"rsa\"\nwithdrawn = \"2024-01-01\"",
    );
    let informational = RSA.replace(
        "package = \"rsa\"",
        "package = \"rsa\"\ninformational = \"unmaintained\"",
    );
    let cases = [
        (
            "synthetic_patched",
            patched.as_str(),
            Some(now),
            Some(now),
            "passed",
            None,
        ),
        (
            "synthetic_withdrawn",
            withdrawn.as_str(),
            Some(now),
            Some(now),
            "passed",
            None,
        ),
        (
            "synthetic_informational",
            informational.as_str(),
            Some(now),
            Some(now),
            "passed",
            None,
        ),
        (
            "stale",
            patched.as_str(),
            Some(now - 700_000),
            Some(now - 700_000),
            "unavailable",
            Some("AUDIT_SNAPSHOT_STALE"),
        ),
        (
            "stale_with_findings",
            RSA,
            Some(now - 700_000),
            Some(now - 700_000),
            "unavailable",
            Some("AUDIT_SNAPSHOT_STALE"),
        ),
        (
            "unknown_age",
            patched.as_str(),
            None,
            None,
            "unavailable",
            Some("AUDIT_SNAPSHOT_UNKNOWN_AGE"),
        ),
    ];
    let mut configurations = Vec::new();
    for (case, markdown, created, observed, status, code) in cases {
        let mut fixture = Fixture::new()?;
        fixture.assert_clean(None)?;
        let expected = prepare(&fixture)?;
        let (path, bytes, hash) = snapshot(&fixture, &document(markdown, created, observed))?;
        let mut server = start(&fixture, Some((&path, &hash)))?;
        let (opened, tool) = bootstrap(&mut server, &fixture)?;
        let output = run(&mut server, &tool, &opened, 10, status, code)?;
        assert_snapshot(&output, &hash, created, observed, &expected["Cargo.lock"]);
        let audit = &output["data"]["observation"];
        if case == "stale_with_findings" {
            assert_eq!(audit["findings"].as_array().map(Vec::len), Some(1));
            assert_eq!(audit["findings"][0]["advisory_id"], "RUSTSEC-2023-0071");
        } else {
            assert_eq!(audit["findings"], json!([]), "{case}: {output}");
        }
        assert_eq!(audit["validation_complete"], status == "passed");
        if case == "synthetic_informational" {
            assert_eq!(audit["informational"].as_array().map(Vec::len), Some(1));
            assert_eq!(audit["informational"][0]["informational"], "unmaintained");
            assert_eq!(
                audit["informational"][0]["advisory_id"],
                "RUSTSEC-2023-0071"
            );
        } else {
            assert_eq!(audit["informational"], json!([]));
        }
        if status == "passed" {
            assert_eq!(audit["snapshot"]["freshness"]["state"], "fresh");
            assert!(audit["issue"].is_null());
        } else {
            assert_eq!(
                audit["issue"],
                if matches!(case, "stale" | "stale_with_findings") {
                    "snapshot_stale"
                } else {
                    "snapshot_unknown_age"
                }
            );
            assert_ne!(audit["snapshot"]["freshness"]["state"], "fresh");
        }
        server.finish()?;
        fixture.assert_clean(None)?;
        assert_fixture_tree(&fixture, &expected)?;
        assert_eq!(fs::read(&path)?, bytes);
        configurations.push(output["data"]["runtime"]["configuration_fingerprint"].clone());
        println!(
            "M1_AUDIT_CASE {}",
            json!({"case":case,"status":status,"snapshot_fingerprint":hash,"source_unchanged":true,"cleanup":true})
        );
        fixture.successful = true;
    }
    println!(
        "M1_AUDIT_RECEIPT {}",
        json!({"cases":6,"synthetic_patched_clean":true,"withdrawn_and_informational_distinct":true,"stale_and_unknown_never_pass":true,"snapshot_and_lock_sha256_verified":6,"stale_findings_retained_with_unavailable_state":true,"fixture_identity_not_publisher_authentication":true,"source_unchanged":true,"cleanup":true,"configuration_fingerprints":configurations})
    );
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; run this binary serially"]
fn audit_missing_integrity_path_and_symlink_fail_closed() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "runtime lock poisoned")?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    for case in [
        "absent",
        "configured_missing",
        "bad_hash",
        "bad_record_path",
        "symlink",
    ] {
        let mut fixture = Fixture::new()?;
        fixture.assert_clean(None)?;
        let expected = prepare(&fixture)?;
        let mut doc = document(RSA, Some(now), Some(now));
        if case == "bad_record_path" {
            doc["records"][0]["path"] = json!("crates/rsa/../RUSTSEC-2023-0071.md");
        }
        let (path, bytes, hash) = snapshot(&fixture, &doc)?;
        let bad_hash = format!("sha256:{}", "0".repeat(64));
        let configured_path = if case == "symlink" {
            let link = fixture.root.join("rustsec-link.json");
            std::os::unix::fs::symlink(&path, &link)?;
            link
        } else if case == "configured_missing" {
            let missing = fixture.root.join("missing-rustsec.json");
            assert!(!missing.exists());
            missing
        } else {
            path.clone()
        };
        let configuration = if case == "absent" {
            None
        } else {
            Some((
                configured_path.as_path(),
                if case == "bad_hash" {
                    bad_hash.as_str()
                } else {
                    hash.as_str()
                },
            ))
        };
        let mut server = start(&fixture, configuration)?;
        let (opened, tool) = bootstrap(&mut server, &fixture)?;
        let code = match case {
            "absent" | "configured_missing" => "AUDIT_SNAPSHOT_UNAVAILABLE",
            "symlink" => "SANDBOX_DENIED",
            "bad_hash" => "AUDIT_INTEGRITY_FAILED",
            _ => "AUDIT_SNAPSHOT_INVALID",
        };
        let status = if matches!(case, "absent" | "configured_missing") {
            "unavailable"
        } else {
            "blocked"
        };
        let output = run(&mut server, &tool, &opened, 10, status, Some(code))?;
        if matches!(case, "absent" | "configured_missing") {
            assert_eq!(
                output["data"]["observation"]["issue"],
                "snapshot_unavailable"
            );
            assert_eq!(output["data"]["observation"]["validation_complete"], false);
            assert!(output["data"]["observation"]["snapshot"].is_null());
            assert!(output["data"]["observation"]["snapshot_record_count"].is_null());
            assert!(output["data"]["observation"]["snapshot_sequence"].is_null());
        } else {
            assert!(output["data"].is_null(), "{output}");
        }
        if case == "configured_missing" {
            assert!(!configured_path.exists());
        }
        server.finish()?;
        fixture.assert_clean(None)?;
        assert_fixture_tree(&fixture, &expected)?;
        assert_eq!(fs::read(&path)?, bytes);
        if case == "symlink" {
            assert_eq!(fs::read_link(configured_path)?, path);
        }
        println!(
            "M1_AUDIT_CASE {}",
            json!({"case":case,"status":status,"error_code":code,"source_unchanged":true,"cleanup":true})
        );
        fixture.successful = true;
    }
    println!(
        "M1_AUDIT_RECEIPT {}",
        json!({"cases":5,"configured_runtime_missing_snapshot_unavailable":true,"configured_missing_file_unavailable":true,"bad_sha256_record_path_and_source_symlink_blocked":true,"snapshot_source_unchanged":true,"project_source_unchanged":true,"cleanup":true})
    );
    Ok(())
}
