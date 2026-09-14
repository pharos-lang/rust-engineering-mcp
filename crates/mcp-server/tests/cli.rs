use std::ffi::OsStr;
use std::io;
use std::process::{Command, Output};

// Harness only: run the Cargo-built bootstrap, never project-supplied commands.
fn run(args: &[impl AsRef<OsStr>]) -> io::Result<Output> {
    Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"))
        .env_clear()
        .args(args)
        .output()
}

#[test]
fn version_comes_from_package_metadata() -> io::Result<()> {
    for flag in ["version", "--version", "-V"] {
        let output = run(&[flag])?;
        assert!(output.status.success());
        assert_eq!(
            output.stdout,
            format!("rust-engineering-mcp {}\n", env!("CARGO_PKG_VERSION")).as_bytes()
        );
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn help_describes_only_implemented_commands() -> io::Result<()> {
    for flag in ["help", "--help", "-h"] {
        let output = run(&[flag])?;
        assert!(output.status.success());
        let help = String::from_utf8_lossy(&output.stdout);
        assert!(help.contains("version"));
        assert!(help.contains("serve --stdio"));
        assert!(help.contains("rust.project.open"));
        assert!(help.contains("rust.project.inspect"));
        assert!(help.contains("rust.toolchain.inspect"));
        assert!(help.contains("catalog sync"));
        assert!(help.contains("contract [--json | --human]"));
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn no_arguments_fail_without_polluting_stdout() -> io::Result<()> {
    let output = run(&[] as &[&str])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    Ok(())
}

#[test]
fn unsupported_modes_fail_without_claiming_mcp_support() -> io::Result<()> {
    for args in [
        vec!["serve"],
        vec!["serve", "--http"],
        vec!["serve", "--stdio", "extra"],
        vec!["--stdio"],
        vec!["doctor", "--unknown"],
        vec!["capabilities"],
        vec!["catalog", "sync"],
        vec!["contract", "--unknown"],
        vec!["contract", "--json", "--human"],
        vec!["contract", "--json", "extra"],
    ] {
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(!output.stderr.is_empty(), "{args:?}");
    }
    Ok(())
}

#[test]
fn trailing_arguments_are_not_ignored() -> io::Result<()> {
    for args in [["version", "--stdio"], ["--help", "extra"], ["-V", "extra"]] {
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    Ok(())
}

#[test]
fn untrusted_arguments_are_not_echoed() -> io::Result<()> {
    let argument = "secret-token-123\n\u{1b}[31m";
    let output = run(&[argument])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, run(&["unknown"])?.stderr);
    Ok(())
}

#[cfg(unix)]
#[test]
fn non_utf8_argument_is_rejected_without_panicking() -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;

    let output = run(&[OsStr::from_bytes(b"\xff")])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("panicked"));
    Ok(())
}

#[cfg(unix)]
#[test]
fn closed_output_stream_returns_one_without_panicking() -> io::Result<()> {
    use std::process::Stdio;

    for argument in ["--help", "version", "unknown"] {
        // A completed sink closes the only pipe reader before the product starts.
        // A socket-pair shutdown can still accept a short write on some kernels,
        // making the version case a race rather than a closed-output oracle.
        let mut sink = Command::new("/usr/bin/true")
            .env_clear()
            .stdin(Stdio::piped())
            .spawn()?;
        let closed_stream = Stdio::from(
            sink.stdin
                .take()
                .ok_or_else(|| io::Error::other("missing sink input"))?,
        );
        assert!(sink.wait()?.success());
        let mut command = Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"));
        command.env_clear().arg(argument);
        if argument == "unknown" {
            command.stderr(closed_stream);
        } else {
            command.stdout(closed_stream);
        }
        let output = command.output()?;
        assert_eq!(output.status.code(), Some(1), "{argument}");
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[test]
fn host_policy_arguments_are_bounded_and_not_ignored() -> io::Result<()> {
    let baseline = run(&["unknown"])?;
    for args in [
        vec!["serve", "--stdio", "--root"],
        vec!["serve", "--stdio", "--project-ttl-secs"],
        vec!["serve", "--stdio", "--project-ttl-secs", "0"],
        vec!["serve", "--stdio", "--project-ttl-secs", "86401"],
        vec!["serve", "--stdio", "--project-ttl-secs", "-1"],
        vec!["serve", "--stdio", "--project-ttl-secs", "secret-token"],
        vec![
            "serve",
            "--stdio",
            "--project-ttl-secs",
            "1",
            "--project-ttl-secs",
            "2",
        ],
        vec![
            "serve",
            "--stdio",
            "--root",
            "/secret",
            "--network",
            "allow",
        ],
    ] {
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, baseline.stderr);
    }
    let mut excessive = vec!["serve", "--stdio"];
    for _ in 0..17 {
        excessive.extend(["--root", "/secret"]);
    }
    let output = run(&excessive)?;
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(output.stderr, baseline.stderr);
    for ttl in ["1", "86400"] {
        let output = run(&["serve", "--stdio", "--project-ttl-secs", ttl])?;
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn host_root_non_utf8_is_rejected_before_server_startup() -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let output = run(&[
        OsStr::new("serve"),
        OsStr::new("--stdio"),
        OsStr::new("--root"),
        OsStr::from_bytes(b"/secret-\xff"),
    ])?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, run(&["unknown"])?.stderr);
    Ok(())
}

#[test]
fn rust_runtime_options_require_a_complete_unique_approved_tuple() -> io::Result<()> {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    let baseline = run(&["unknown"])?;
    let state_root = std::env::temp_dir().canonicalize()?.join(format!(
        "rust-mcp-cli-state-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos()
    ));
    fs::create_dir(&state_root)?;
    // The scratch state root is owner-only where the platform expresses that
    // as a POSIX mode. The assertions below are about CLI parsing and hold
    // identically on every platform, so only this setup step is gated.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&state_root, fs::Permissions::from_mode(0o700))?;
    }
    let state_root = state_root
        .to_str()
        .ok_or_else(|| io::Error::other("temporary state root is not UTF-8"))?;
    let options = [
        ("--docker", "/nonexistent/trusted-docker"),
        ("--docker-socket", "/nonexistent/trusted.sock"),
        ("--state-root", state_root),
        (
            "--rust-image",
            rust_engineering_execution::APPROVED_RUST_IMAGE,
        ),
    ];
    // Every nonempty proper subset must fail during CLI parsing.
    for mask in 1..15 {
        let mut args = vec!["serve", "--stdio"];
        for (index, (flag, value)) in options.iter().enumerate() {
            if mask & (1 << index) != 0 {
                args.extend([*flag, *value]);
            }
        }
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2), "subset {mask}");
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, baseline.stderr);
    }
    let mut complete = vec!["serve", "--stdio"];
    for (flag, value) in options {
        complete.extend([flag, value]);
    }
    for (flag, value) in options {
        let mut duplicate = complete.clone();
        duplicate.extend([flag, value]);
        let output = run(&duplicate)?;
        assert_eq!(output.status.code(), Some(2), "duplicate {flag}");
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, baseline.stderr);
        let mut missing_value = complete.clone();
        missing_value.push(flag);
        assert_eq!(run(&missing_value)?.status.code(), Some(2));
    }
    for image in [
        "rust:latest",
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "secret-token\n\u{1b}[31m",
    ] {
        let mut args = complete.clone();
        let last = args
            .last_mut()
            .ok_or_else(|| io::Error::other("missing image argument"))?;
        *last = image;
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, baseline.stderr);
    }
    // EOF starts and stops stdio without calibration or Docker execution. The
    // nonexistent executable also prevents any accidental real runtime access.
    let output = run(&complete)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    fs::remove_dir_all(state_root)?;
    Ok(())
}

#[test]
fn audit_snapshot_flags_require_unique_complete_valid_host_configuration() -> io::Result<()> {
    let fingerprint = format!("sha256:{:064x}", 42);
    let snapshot = std::env::temp_dir().join("rust-mcp-nonexistent-trusted-snapshot.json");
    let snapshot = snapshot
        .to_str()
        .ok_or_else(|| io::Error::other("temporary directory is not UTF-8"))?;
    let baseline = run(&["unknown"])?;
    let complete = [
        "serve",
        "--stdio",
        "--rustsec-snapshot",
        snapshot,
        "--rustsec-sha256",
        fingerprint.as_str(),
    ];
    for args in [
        vec!["serve", "--stdio", "--rustsec-snapshot"],
        vec!["serve", "--stdio", "--rustsec-snapshot", "/secret"],
        vec!["serve", "--stdio", "--rustsec-sha256", &fingerprint],
        vec![
            "serve",
            "--stdio",
            "--rustsec-snapshot",
            "relative.json",
            "--rustsec-sha256",
            &fingerprint,
        ],
    ] {
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, baseline.stderr);
    }
    for flag in ["--rustsec-snapshot", "--rustsec-sha256"] {
        let mut args = complete.to_vec();
        args.extend([
            flag,
            if flag == "--rustsec-snapshot" {
                "/secret"
            } else {
                &fingerprint
            },
        ]);
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2));
        assert_eq!(output.stderr, baseline.stderr);
    }
    for invalid in [
        "sha256:ABC",
        "sha256:0000",
        "secret-token\n\u{1b}[31m",
        "SHA256:0000000000000000000000000000000000000000000000000000000000000000",
        "sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    ] {
        let mut args = complete.to_vec();
        args[5] = invalid;
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, baseline.stderr);
    }
    // Explicit snapshot configuration is lazy: EOF does not read the file,
    // contact a service, or initialize a runtime.
    let output = run(&complete)?;
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn non_utf8_snapshot_configuration_is_rejected_without_echo() -> io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let fingerprint = format!("sha256:{:064x}", 42);
    for invalid_index in [3, 5] {
        let mut args = [
            OsStr::new("serve"),
            OsStr::new("--stdio"),
            OsStr::new("--rustsec-snapshot"),
            OsStr::new("/snapshot.json"),
            OsStr::new("--rustsec-sha256"),
            OsStr::new(&fingerprint),
        ];
        args[invalid_index] = OsStr::from_bytes(b"/secret-\xff");
        let output = run(&args)?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, run(&["unknown"])?.stderr);
    }
    Ok(())
}

// spec §56 (M8-02 decision 3): the static `contract` document must describe
// exactly the same 36 tools the live server's `tools/list` snapshots do. This
// mirrors tests/protocol.rs's `bootstrap` snapshot set rather than spawning a
// second server, so it stays portable and Docker-free.
fn contract_snapshots() -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    [
        include_str!("snapshots/project-open-tool.json"),
        include_str!("snapshots/project-inspect-tool.json"),
        include_str!("snapshots/toolchain-inspect-tool.json"),
        include_str!("snapshots/check-tool.json"),
        include_str!("snapshots/format-tool.json"),
        include_str!("snapshots/clippy-tool.json"),
        include_str!("snapshots/test-tool.json"),
        include_str!("snapshots/nextest-tool.json"),
        include_str!("snapshots/audit-tool.json"),
        include_str!("snapshots/explain-tool.json"),
        include_str!("snapshots/quality-tool.json"),
        include_str!("snapshots/catalog-status-tool.json"),
        include_str!("snapshots/crate-search-tool.json"),
        include_str!("snapshots/crate-inspect-tool.json"),
        include_str!("snapshots/manifest-patch-tool.json"),
        include_str!("snapshots/fmt-apply-tool.json"),
        include_str!("snapshots/fix-apply-tool.json"),
        include_str!("snapshots/dependency-add-tool.json"),
        include_str!("snapshots/dependency-remove-tool.json"),
        include_str!("snapshots/coverage-tool.json"),
        include_str!("snapshots/semver-tool.json"),
        include_str!("snapshots/mutation-test-tool.json"),
        include_str!("snapshots/deny-tool.json"),
        include_str!("snapshots/unsafe-scan-tool.json"),
        include_str!("snapshots/supply-chain-tool.json"),
        include_str!("snapshots/quality-v2-tool.json"),
        include_str!("snapshots/miri-tool.json"),
        include_str!("snapshots/benchmark-run-tool.json"),
        include_str!("snapshots/benchmark-compare-tool.json"),
        include_str!("snapshots/profile-flamegraph-tool.json"),
        include_str!("snapshots/binary-bloat-tool.json"),
        include_str!("snapshots/analyzer-symbols-tool.json"),
        include_str!("snapshots/analyzer-references-tool.json"),
        include_str!("snapshots/analyzer-diagnostics-tool.json"),
        include_str!("snapshots/analyzer-actions-tool.json"),
        include_str!("snapshots/analyzer-action-apply-tool.json"),
    ]
    .into_iter()
    .map(|snapshot| serde_json::from_str::<serde_json::Value>(snapshot).map_err(Into::into))
    .collect()
}

const PREVIEW_TOOL_NAMES: [&str; 5] = [
    "rust.analyzer.symbols",
    "rust.analyzer.references",
    "rust.analyzer.diagnostics",
    "rust.analyzer.actions",
    "rust.analyzer.action.apply",
];

/// `sha256(json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False))`:
/// an independent implementation of the contract document's canonicalization,
/// so this test is an oracle rather than a restatement of the source.
fn canonicalize(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            let mut object = serde_json::Map::new();
            for (key, value) in entries {
                object.insert(key.clone(), canonicalize(value));
            }
            serde_json::Value::Object(object)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonicalize).collect())
        }
        other => other.clone(),
    }
}

fn canonical_hash(value: &serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&canonicalize(value))?;
    Ok(Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

#[test]
fn contract_json_describes_exactly_the_36_snapshot_tools() -> Result<(), Box<dyn std::error::Error>>
{
    let output = run(&["contract", "--json"])?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert_eq!(document["document_kind"], "rust_engineering_capabilities");
    assert_eq!(document["tool_count"], 36);
    let tools = document["tools"].as_object().ok_or("tools object")?;
    assert_eq!(tools.len(), 36);

    let snapshots = contract_snapshots()?;
    let mut snapshot_names: Vec<&str> = snapshots
        .iter()
        .map(|snapshot| snapshot["name"].as_str().ok_or("name field"))
        .collect::<Result<_, _>>()?;
    snapshot_names.sort_unstable();
    let mut document_names: Vec<&str> = tools.keys().map(String::as_str).collect();
    document_names.sort_unstable();
    assert_eq!(snapshot_names, document_names);

    let mut preview_count = 0;
    let mut stable_count = 0;
    for snapshot in &snapshots {
        let name = snapshot["name"].as_str().ok_or("name field")?;
        let tool = tools
            .get(name)
            .ok_or_else(|| format!("{name} missing from contract"))?;
        let is_preview = PREVIEW_TOOL_NAMES.contains(&name);
        assert_eq!(
            tool["stability"],
            if is_preview { "preview" } else { "stable" },
            "{name}"
        );
        if is_preview {
            preview_count += 1;
        } else {
            stable_count += 1;
        }
        assert_eq!(
            tool["input_schema_sha256"],
            canonical_hash(&snapshot["inputSchema"])?,
            "{name} input_schema_sha256"
        );
        assert_eq!(
            tool["output_schema_sha256"],
            canonical_hash(&snapshot["outputSchema"])?,
            "{name} output_schema_sha256"
        );
        assert_eq!(
            tool["description_sha256"],
            canonical_hash(&snapshot["description"])?,
            "{name} description_sha256"
        );
        assert_eq!(
            tool["annotations"], snapshot["annotations"],
            "{name} annotations"
        );
        let description = snapshot["description"]
            .as_str()
            .ok_or("description field")?;
        assert_eq!(
            description.starts_with("Preview (ADR-086): "),
            is_preview,
            "{name} description prefix"
        );
    }
    assert_eq!(preview_count, 5);
    assert_eq!(stable_count, 31);
    Ok(())
}

#[test]
fn contract_defaults_to_json_and_accepts_human() -> Result<(), Box<dyn std::error::Error>> {
    let default_output = run(&["contract"])?;
    let json_output = run(&["contract", "--json"])?;
    assert_eq!(default_output.stdout, json_output.stdout);

    let human_output = run(&["contract", "--human"])?;
    assert!(human_output.status.success());
    assert!(human_output.stderr.is_empty());
    let human = String::from_utf8_lossy(&human_output.stdout);
    assert!(human.starts_with("rust-engineering-mcp contract: 36 tools (5 preview)"));
    for snapshot in contract_snapshots()? {
        let name = snapshot["name"].as_str().ok_or("name field")?;
        assert!(human.contains(name), "{name} missing from human report");
    }
    Ok(())
}
