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
    use std::os::unix::fs::PermissionsExt;
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
    fs::set_permissions(&state_root, fs::Permissions::from_mode(0o700))?;
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
