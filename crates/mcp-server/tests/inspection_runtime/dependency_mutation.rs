use super::format_mutation_runtime::{mutation_output, tool_schema};
use super::*;
use std::ffi::OsString;
use std::path::Path;

const VENDOR_FINGERPRINT: &str =
    "sha256:743947d5788c1a4385a4b59869c5b8bd0535f7fc0d875b51288f9b26b2d0eba1";

fn copy_tree(source: &Path, destination: &Path) -> Result {
    fs::create_dir(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            return Err("vendor fixture contains a non-file entry".into());
        }
    }
    Ok(())
}

fn install_vendor(fixture: &Fixture) -> Result<PathBuf> {
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/cargo-vendor-data/vendor");
    let vendor = fixture.root.join("vendor");
    copy_tree(&source, &vendor)?;
    Ok(vendor)
}

fn mutation_arguments(fixture: &Fixture, vendor: Option<&Path>) -> Vec<OsString> {
    let mut arguments = Vec::new();
    for flag in [
        "--allow-manifest-write",
        "--allow-dependency-add",
        "--allow-dependency-remove",
    ] {
        arguments.push(flag.into());
        arguments.push(fixture.project.as_os_str().to_owned());
    }
    if let Some(vendor) = vendor {
        arguments.push("--cargo-vendor-dir".into());
        arguments.push(vendor.as_os_str().to_owned());
        arguments.push("--cargo-vendor-tree-sha256".into());
        arguments.push(VENDOR_FINGERPRINT.into());
    }
    arguments
}

fn read_tree(root: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    fn walk(root: &Path, at: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> Result {
        for entry in fs::read_dir(at)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                walk(root, &path, files)?;
            } else {
                let relative = path
                    .strip_prefix(root)?
                    .to_str()
                    .ok_or("non-UTF8 fixture")?;
                files.insert(relative.replace('\\', "/"), fs::read(path)?);
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    walk(root, root, &mut files)?;
    Ok(files)
}

fn semantic_call(id: i64, tool: &str, opened: &Value, action: Value) -> Value {
    call(
        id,
        tool,
        json!({"project_ref":opened["project_ref"],"action":action}),
    )
}

fn commit_action(preview: &Value, key: &str) -> Value {
    json!({
        "mode":"commit",
        "plan_id":preview["data"]["plan_id"],
        "plan_digest":preview["data"]["plan_digest"],
        "idempotency_key":key
    })
}

fn assert_resolution(preview: &Value, manifest: &str, disposition: &str) {
    let validation = &preview["data"]["validation"];
    assert_eq!(validation["method"], "cargo_metadata_offline_then_frozen");
    assert_eq!(validation["image_id"], APPROVED_RUST_IMAGE);
    assert_eq!(validation["resolution"]["manifest_path"], manifest);
    assert_eq!(validation["resolution"]["lock_policy"], "preserve_presence");
    assert_eq!(validation["resolution"]["lock_disposition"], disposition);
    assert_eq!(
        validation["resolution"]["dataset_fingerprint"],
        VENDOR_FINGERPRINT
    );
    for field in [
        "configuration_fingerprint",
        "execution_fingerprint",
        "candidate_source_fingerprint",
    ] {
        assert_fingerprint(&validation[field]);
    }
    for field in [
        "resolution_execution_fingerprint",
        "resolved_lock_fingerprint",
    ] {
        assert_fingerprint(&validation["resolution"][field]);
    }
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial runtime qualification"]
fn dependency_add_remove_preview_commit_restart_and_scope() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    let vendor = install_vendor(&fixture)?;
    fixture.assert_clean(None)?;
    let before = read_tree(&fixture.project)?;
    let mut server =
        Server::start_with_arguments(&fixture, mutation_arguments(&fixture, Some(&vendor)))?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let add_schema = tool_schema(&mut server, 3, "rust.dependency.add")?;
    let remove_schema = tool_schema(&mut server, 4, "rust.dependency.remove")?;
    server.send(semantic_call(
        5,
        "rust.dependency.add",
        &opened,
        json!({
            "mode":"preview",
            "expected_project_fingerprint":opened["fingerprint"],
            "manifest_path":"app/Cargo.toml",
            "dependency_kind":"normal",
            "name":"quoted",
            "requirement":"=1.0.47",
            "package":"quote",
            "features":["proc-macro"],
            "optional":true,
            "default_features":false
        }),
    ))?;
    let added = mutation_output(server.receive(5, JOIN_TIMEOUT)?, &add_schema, "passed")?;
    assert_resolution(&added, "app/Cargo.toml", "updated_existing");
    assert_eq!(read_tree(&fixture.project)?, before);
    let paths = added["data"]["files"]
        .as_array()
        .ok_or("files")?
        .iter()
        .map(|file| file["path"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(paths, ["Cargo.lock", "app/Cargo.toml"]);
    let diff = added["data"]["diff"].as_str().ok_or("diff")?;
    assert!(diff.contains("quoted = { version = \"=1.0.47\", package = \"quote\", features = [\"proc-macro\"], optional = true, default-features = false }"), "{diff}");
    assert!(diff.contains("name = \"quote\""), "{diff}");

    let add_commit = commit_action(&added, "add_quoted");
    server.send(semantic_call(
        6,
        "rust.dependency.remove",
        &opened,
        add_commit.clone(),
    ))?;
    let cross = mutation_output(
        server.receive(6, DISCOVERY_TIMEOUT)?,
        &remove_schema,
        "blocked",
    )?;
    assert_eq!(cross["error_code"], "permission_denied");
    server.send(semantic_call(7, "rust.dependency.add", &opened, add_commit))?;
    let add_receipt = mutation_output(server.receive(7, JOIN_TIMEOUT)?, &add_schema, "passed")?;
    assert_eq!(add_receipt["data"]["state"], "committed");
    assert!(
        String::from_utf8(fs::read(fixture.project.join("app/Cargo.toml"))?)?.contains("quoted =")
    );
    assert!(
        String::from_utf8(fs::read(fixture.project.join("Cargo.lock"))?)?
            .contains("name = \"quote\"")
    );
    server.finish()?;
    fixture.assert_clean(None)?;

    let mut restarted =
        Server::start_with_arguments(&fixture, mutation_arguments(&fixture, Some(&vendor)))?;
    let (reopened, _) = restarted.bootstrap_open(&fixture)?;
    restarted.send(semantic_call(
        3,
        "rust.dependency.add",
        &reopened,
        json!({
            "mode":"receipt","operation_id":add_receipt["data"]["operation_id"],"recover":true
        }),
    ))?;
    let replay = mutation_output(restarted.receive(3, JOIN_TIMEOUT)?, &add_schema, "passed")?;
    assert_eq!(replay["data"], add_receipt["data"]);
    restarted.send(semantic_call(4, "rust.dependency.remove", &reopened, json!({
        "mode":"preview","expected_project_fingerprint":reopened["fingerprint"],
        "manifest_path":"app/Cargo.toml","dependency_kind":"normal","target":null,"name":"quoted"
    })))?;
    let removed = mutation_output(
        restarted.receive(4, JOIN_TIMEOUT)?,
        &remove_schema,
        "passed",
    )?;
    assert_resolution(&removed, "app/Cargo.toml", "updated_existing");
    let remove_diff = removed["data"]["diff"].as_str().ok_or("diff")?;
    assert!(remove_diff.contains("-quoted ="), "{remove_diff}");
    restarted.send(semantic_call(
        5,
        "rust.dependency.remove",
        &reopened,
        commit_action(&removed, "remove_quoted"),
    ))?;
    let receipt = mutation_output(
        restarted.receive(5, JOIN_TIMEOUT)?,
        &remove_schema,
        "passed",
    )?;
    assert_eq!(receipt["data"]["state"], "committed");
    assert!(
        !String::from_utf8(fs::read(fixture.project.join("app/Cargo.toml"))?)?.contains("quoted =")
    );
    assert!(
        !String::from_utf8(fs::read(fixture.project.join("Cargo.lock"))?)?
            .contains("name = \"quote\"")
    );
    restarted.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial runtime qualification"]
fn dependency_dataset_selection_target_and_inherited_removal_fail_closed() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut missing = Fixture::new()?;
    let before = read_tree(&missing.project)?;
    let mut server = Server::start_with_arguments(&missing, mutation_arguments(&missing, None))?;
    let (opened, _) = server.bootstrap_open(&missing)?;
    let schema = tool_schema(&mut server, 3, "rust.dependency.add")?;
    server.send(semantic_call(
        4,
        "rust.dependency.add",
        &opened,
        json!({
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"],
            "manifest_path":"app/Cargo.toml","name":"quote","requirement":"=1.0.47"
        }),
    ))?;
    let unavailable = mutation_output(server.receive(4, JOIN_TIMEOUT)?, &schema, "unavailable")?;
    assert_eq!(unavailable["error_code"], "offline_data_missing");
    server.finish()?;
    assert_eq!(read_tree(&missing.project)?, before);
    missing.assert_clean(None)?;
    missing.successful = true;

    let mut invalid = Fixture::new()?;
    let vendor = install_vendor(&invalid)?;
    fs::write(vendor.join("quote-1.0.47/src/lib.rs"), b"altered")?;
    let mut server =
        Server::start_with_arguments(&invalid, mutation_arguments(&invalid, Some(&vendor)))?;
    let (opened, _) = server.bootstrap_open(&invalid)?;
    server.send(semantic_call(
        3,
        "rust.dependency.add",
        &opened,
        json!({
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"],
            "manifest_path":"app/Cargo.toml","name":"quote","requirement":"=1.0.47"
        }),
    ))?;
    let blocked = mutation_output(server.receive(3, JOIN_TIMEOUT)?, &schema, "blocked")?;
    assert_eq!(blocked["error_code"], "offline_data_invalid");
    server.finish()?;
    invalid.assert_clean(None)?;
    invalid.successful = true;

    let mut selected = Fixture::new()?;
    let vendor = install_vendor(&selected)?;
    let mut server =
        Server::start_with_arguments(&selected, mutation_arguments(&selected, Some(&vendor)))?;
    let (opened, _) = server.bootstrap_open(&selected)?;
    server.send(semantic_call(
        3,
        "rust.dependency.add",
        &opened,
        json!({
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"],
            "name":"quote","requirement":"=1.0.47"
        }),
    ))?;
    let root = mutation_output(server.receive(3, JOIN_TIMEOUT)?, &schema, "blocked")?;
    assert_eq!(root["error_code"], "invalid_operation");
    let remove_schema = tool_schema(&mut server, 4, "rust.dependency.remove")?;
    server.send(semantic_call(
        5,
        "rust.dependency.remove",
        &opened,
        json!({
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"],
            "manifest_path":"app/Cargo.toml","dependency_kind":"build",
            "target":"cfg(unix)","name":"renamed"
        }),
    ))?;
    let inherited = mutation_output(server.receive(5, JOIN_TIMEOUT)?, &remove_schema, "passed")?;
    assert_resolution(&inherited, "app/Cargo.toml", "updated_existing");
    let diff = inherited["data"]["diff"].as_str().ok_or("diff")?;
    assert!(diff.contains("[target.'cfg(unix)'.build-dependencies]"));
    assert!(diff.contains("-renamed.workspace = true"));
    assert!(!diff.contains("helper/Cargo.toml"));
    server.finish()?;
    selected.assert_clean(None)?;
    selected.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial runtime qualification"]
fn absent_lock_stays_absent_and_manifest_patch_families_use_resolution_oracle() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    let vendor = install_vendor(&fixture)?;
    fs::remove_file(fixture.project.join("Cargo.lock"))?;
    let mut server =
        Server::start_with_arguments(&fixture, mutation_arguments(&fixture, Some(&vendor)))?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let manifest_schema = tool_schema(&mut server, 3, "rust.manifest.patch")?;
    server.send(semantic_call(
        4,
        "rust.manifest.patch",
        &opened,
        json!({
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"],
            "edit":{"operation":"workspace_dependency_set","name":"quoted","spec":{
                "requirement":"=1.0.47","package":"quote","features":["proc-macro"],
                "optional":false,"default_features":false
            }}
        }),
    ))?;
    let transient = mutation_output(server.receive(4, JOIN_TIMEOUT)?, &manifest_schema, "passed")?;
    assert_resolution(&transient, "Cargo.toml", "transient_unpublished");
    assert!(!fixture.project.join("Cargo.lock").exists());
    assert_eq!(transient["data"]["files"].as_array().map(Vec::len), Some(1));
    assert_eq!(transient["data"]["files"][0]["path"], "Cargo.toml");
    server.send(semantic_call(
        5,
        "rust.manifest.patch",
        &opened,
        commit_action(&transient, "workspace_quote"),
    ))?;
    let committed = mutation_output(server.receive(5, JOIN_TIMEOUT)?, &manifest_schema, "passed")?;
    assert_eq!(committed["data"]["state"], "committed");
    assert!(!fixture.project.join("Cargo.lock").exists());
    server.send(call(
        6,
        "rust.project.open",
        json!({"path":fixture.project}),
    ))?;
    let reopened_response = server.receive(6, DISCOVERY_TIMEOUT)?;
    let reopened = &reopened_response["result"]["structuredContent"]["data"];
    server.send(semantic_call(
        7,
        "rust.manifest.patch",
        reopened,
        json!({
            "mode":"preview","expected_project_fingerprint":reopened["fingerprint"],
            "edit":{"operation":"workspace_dependency_remove","name":"quoted"}
        }),
    ))?;
    let remove = mutation_output(server.receive(7, JOIN_TIMEOUT)?, &manifest_schema, "passed")?;
    assert_resolution(&remove, "Cargo.toml", "transient_unpublished");
    assert!(
        remove["data"]["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("-quoted =")
    );
    assert!(!fixture.project.join("Cargo.lock").exists());
    server.finish()?;
    fixture.assert_clean(None)?;

    let mut patch = Fixture::new()?;
    let vendor = install_vendor(&patch)?;
    let mut server =
        Server::start_with_arguments(&patch, mutation_arguments(&patch, Some(&vendor)))?;
    let (opened, _) = server.bootstrap_open(&patch)?;
    let schema = tool_schema(&mut server, 3, "rust.manifest.patch")?;
    for (id, edit, needle, resolved) in [
        (
            4,
            json!({"operation":"lint_set","scope":"workspace","tool":"rust","name":"unsafe_code","level":"forbid","priority":0}),
            "[workspace.lints.rust]",
            false,
        ),
        (
            5,
            json!({"operation":"profile_set","profile":"dev","setting":{"name":"opt-level","value":2}}),
            "opt-level = 2",
            false,
        ),
        (
            6,
            json!({"operation":"workspace_dependency_set","name":"quoted","spec":{"requirement":"=1.0.47","package":"quote","features":[],"optional":false,"default_features":true}}),
            "quoted =",
            true,
        ),
    ] {
        server.send(semantic_call(
            id,
            "rust.manifest.patch",
            &opened,
            json!({
                "mode":"preview","expected_project_fingerprint":opened["fingerprint"],"edit":edit
            }),
        ))?;
        let output = mutation_output(server.receive(id, JOIN_TIMEOUT)?, &schema, "passed")?;
        assert!(
            output["data"]["diff"]
                .as_str()
                .unwrap_or_default()
                .contains(needle)
        );
        if resolved {
            assert_resolution(&output, "Cargo.toml", "updated_existing");
        }
    }
    server.send(semantic_call(
        7,
        "rust.manifest.patch",
        &opened,
        json!({
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"],
            "edit":{"operation":"workspace_dependency_set","name":"bad","spec":{
                "requirement":"1","features":[],"optional":true,"default_features":true
            }}
        }),
    ))?;
    let optional = mutation_output(server.receive(7, DISCOVERY_TIMEOUT)?, &schema, "blocked")?;
    assert_eq!(optional["error_code"], "invalid_operation");
    server.finish()?;
    patch.assert_clean(None)?;
    patch.successful = true;
    fixture.successful = true;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket on macOS APFS; serial runtime qualification"]
fn package_root_feature_set_and_remove_are_exact_and_lock_preserving() -> Result {
    let _serial = SERIAL.lock().map_err(|_| "serial lock poisoned")?;
    let mut fixture = Fixture::new()?;
    let vendor = install_vendor(&fixture)?;
    fs::create_dir_all(fixture.project.join("src"))?;
    fs::write(fixture.project.join("src/lib.rs"), "pub fn root() {}\n")?;
    fs::write(
        fixture.project.join("Cargo.toml"),
        "[package]\nname = \"root\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[features]\ndefault = []\nold = []\n",
    )?;
    fs::write(
        fixture.project.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"root\"\nversion = \"0.1.0\"\n",
    )?;
    let before = read_tree(&fixture.project)?;
    let mut server =
        Server::start_with_arguments(&fixture, mutation_arguments(&fixture, Some(&vendor)))?;
    let (opened, _) = server.bootstrap_open(&fixture)?;
    let schema = tool_schema(&mut server, 3, "rust.manifest.patch")?;
    server.send(semantic_call(
        4,
        "rust.manifest.patch",
        &opened,
        json!({
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"],
            "edit":{"operation":"feature_set","name":"new","values":["old"]}
        }),
    ))?;
    let set = mutation_output(server.receive(4, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_resolution(&set, "Cargo.toml", "updated_existing");
    assert_eq!(
        set["data"]["files"]
            .as_array()
            .ok_or("files")?
            .iter()
            .map(|file| file["path"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        ["Cargo.lock", "Cargo.toml"]
    );
    assert!(
        set["data"]["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("+new = [\"old\"]")
    );
    assert_eq!(read_tree(&fixture.project)?, before);
    server.send(semantic_call(
        5,
        "rust.manifest.patch",
        &opened,
        json!({
            "mode":"preview","expected_project_fingerprint":opened["fingerprint"],
            "edit":{"operation":"feature_remove","name":"old"}
        }),
    ))?;
    let removed = mutation_output(server.receive(5, JOIN_TIMEOUT)?, &schema, "passed")?;
    assert_resolution(&removed, "Cargo.toml", "updated_existing");
    assert!(
        removed["data"]["diff"]
            .as_str()
            .unwrap_or_default()
            .contains("-old = []")
    );
    assert_eq!(read_tree(&fixture.project)?, before);
    server.finish()?;
    fixture.assert_clean(None)?;
    fixture.successful = true;
    Ok(())
}
