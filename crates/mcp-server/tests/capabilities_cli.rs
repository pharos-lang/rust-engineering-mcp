//! Real CLI boundary tests. Every complete configuration is syntactically
//! invalid before executable/state I/O, so these tests never invoke Docker.
use std::ffi::OsString;
use std::io::{self, Read};
use std::process::{Child, Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

type TestResult<T = ()> = Result<T, String>;
const USAGE_ERROR: &[u8] = b"Unsupported invocation. Use 'rust-engineering-mcp --help'.\n";
const IMAGE: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> TestResult<T> {
    value.map_err(|error| format!("{error:?}"))
}

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run(arguments: &[OsString]) -> TestResult<Output> {
    const LIMIT: u64 = 8192;
    let mut child = ChildGuard(checked(
        Command::new(env!("CARGO_BIN_EXE_rust-engineering-mcp"))
            .env_clear()
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn(),
    )?);
    let stdout = child
        .0
        .stdout
        .take()
        .ok_or_else(|| "missing stdout pipe".to_owned())?;
    let stderr = child
        .0
        .stderr
        .take()
        .ok_or_else(|| "missing stderr pipe".to_owned())?;
    let (sender, receiver) = mpsc::channel();
    for (is_stdout, stream) in [
        (true, Box::new(stdout) as Box<dyn Read + Send>),
        (false, Box::new(stderr) as Box<dyn Read + Send>),
    ] {
        let sender = sender.clone();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let result = stream.take(LIMIT + 1).read_to_end(&mut bytes);
            let _ = sender.send((is_stdout, result, bytes));
        });
    }
    drop(sender);
    let mut captured = (None, None);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        while let Ok((is_stdout, result, bytes)) = receiver.try_recv() {
            capture(&mut captured, is_stdout, result, bytes, LIMIT)?;
        }
        if let Some(status) = checked(child.0.try_wait())? {
            break status;
        }
        if Instant::now() >= deadline {
            return Err("capabilities CLI exceeded deadline".to_owned());
        }
        thread::sleep(Duration::from_millis(5));
    };
    while captured.0.is_none() || captured.1.is_none() {
        let (is_stdout, result, bytes) =
            checked(receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())))?;
        capture(&mut captured, is_stdout, result, bytes, LIMIT)?;
    }
    Ok(Output {
        status,
        stdout: captured.0.ok_or_else(|| "stdout absent".to_owned())?,
        stderr: captured.1.ok_or_else(|| "stderr absent".to_owned())?,
    })
}

fn capture(
    captured: &mut (Option<Vec<u8>>, Option<Vec<u8>>),
    is_stdout: bool,
    result: io::Result<usize>,
    bytes: Vec<u8>,
    limit: u64,
) -> TestResult {
    checked(result)?;
    if bytes.len() > limit as usize {
        return Err("capabilities CLI exceeded output budget".to_owned());
    }
    if is_stdout {
        captured.0 = Some(bytes);
    } else {
        captured.1 = Some(bytes);
    }
    Ok(())
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

fn complete() -> Vec<OsString> {
    args(&[
        "capabilities",
        "--docker",
        "invalid-relative-docker",
        "--docker-socket",
        "/never-opened-socket",
        "--state-root",
        "/never-created-state",
        "--probe-image",
        IMAGE,
    ])
}

fn usage_rejected(arguments: &[OsString]) -> TestResult {
    let output = run(arguments)?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, USAGE_ERROR);
    Ok(())
}

fn unavailable(arguments: &[OsString]) -> TestResult {
    let output = run(arguments)?;
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert!(output.stdout.ends_with(b"\n"));
    assert!(output.stdout.len() < 1024);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("host-secret"));
    let report: Value = checked(serde_json::from_slice(&output.stdout))?;
    assert_eq!(report["status"], "unavailable");
    assert_eq!(report["scope"], "trusted_probe_image_only");
    for field in [
        "strict_available",
        "restricted_available",
        "project_code_available",
    ] {
        assert_eq!(report[field], false, "{field}");
    }
    assert_eq!(report["reason"], "InvalidConfiguration");
    let capabilities = report["capabilities"]
        .as_object()
        .ok_or_else(|| "capabilities is not an object".to_owned())?;
    let required = [
        "filesystem_isolated",
        "network_isolated",
        "environment_isolated",
        "children_contained",
        "wall_time_limited",
        "output_limited",
        "cpu_quota",
        "memory_limited",
        "pids_limited",
        "disk_limited",
    ];
    for capability in required {
        assert_eq!(
            capabilities.get(capability),
            Some(&Value::Bool(false)),
            "{capability}"
        );
    }
    assert!(
        capabilities
            .values()
            .all(|value| value == &Value::Bool(false))
    );
    Ok(())
}

#[test]
fn capabilities_requires_every_flag_and_every_value() -> TestResult {
    usage_rejected(&args(&["capabilities"]))?;
    for index in [1, 3, 5, 7] {
        let mut arguments = complete();
        arguments.drain(index..index + 2);
        usage_rejected(&arguments)?;
    }
    for flag in [
        "--docker",
        "--docker-socket",
        "--state-root",
        "--probe-image",
    ] {
        usage_rejected(&args(&["capabilities", flag]))?;
        let mut arguments = complete();
        arguments.push(OsString::from(flag));
        usage_rejected(&arguments)?;
    }
    Ok(())
}

#[test]
fn capabilities_rejects_duplicate_unknown_and_positional_flags() -> TestResult {
    for flag in [
        "--docker",
        "--docker-socket",
        "--state-root",
        "--probe-image",
        "--network",
        "positional",
        "--docker=/untrusted",
    ] {
        let mut arguments = complete();
        arguments.extend(args(&[flag, "untrusted-value"]));
        usage_rejected(&arguments)?;
    }
    let mut arguments = complete();
    arguments.push(OsString::from("unexpected-tail"));
    usage_rejected(&arguments)
}

#[test]
fn capabilities_invalid_paths_and_images_report_unavailable_json() -> TestResult {
    unavailable(&complete())?;
    for (index, replacement) in [
        (2, ""),
        (2, "/never/../docker"),
        (2, "/never//docker"),
        (2, "/never/docker\ncanary"),
        (4, "relative-socket"),
        (4, "/never/./socket"),
        (8, "latest"),
        (8, "sha256:abc"),
        (
            8,
            "sha256:ZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ",
        ),
    ] {
        let mut arguments = complete();
        // A valid-looking but nonexistent path proves socket/image validation
        // rejects before attempting to read the executable.
        arguments[2] = OsString::from("/never-opened-docker");
        arguments[index] = OsString::from(replacement);
        unavailable(&arguments)?;
    }
    // State roots are not read: the deliberately relative executable fails
    // configuration validation before the adapter creates any state.
    let mut arguments = complete();
    arguments[6] = OsString::from("relative-state-root");
    unavailable(&arguments)
}

#[test]
fn capabilities_rejects_long_or_control_arguments_without_reflection() -> TestResult {
    let secret = format!("host-secret-canary\n\u{1b}[31m{}", "x".repeat(16_384));
    let mut malformed = complete();
    malformed.extend([OsString::from(&secret), OsString::from("value")]);
    usage_rejected(&malformed)?;
    for index in [2, 4, 6, 8] {
        let mut arguments = complete();
        arguments[index] = OsString::from(&secret);
        // Each case retains an invalid executable or has its own invalid path
        // or image, so no host executable/state access is needed.
        unavailable(&arguments)?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn capabilities_non_utf8_flags_and_image_are_usage_errors() -> TestResult {
    use std::os::unix::ffi::OsStringExt;
    for index in [1, 3, 5, 7, 8] {
        let mut arguments = complete();
        arguments[index] = OsString::from_vec(b"host-secret-\xff".to_vec());
        usage_rejected(&arguments)?;
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn capabilities_non_utf8_paths_are_configuration_errors_without_panics() -> TestResult {
    use std::os::unix::ffi::OsStringExt;
    for index in [2, 4, 6] {
        let mut arguments = complete();
        arguments[index] = OsString::from_vec(b"/host-secret-\xff".to_vec());
        unavailable(&arguments)?;
    }
    Ok(())
}

#[test]
fn invalid_configuration_preserves_default_json_bytes_and_explicit_alias() -> TestResult {
    // Invalid adapter configuration rejects before executable reads or subprocess creation.
    let default = run(&complete())?;
    let mut explicit_args = complete();
    explicit_args.push(OsString::from("--json"));
    let explicit = run(&explicit_args)?;
    assert_eq!(default.status.code(), Some(1));
    assert_eq!(explicit.status.code(), Some(1));
    assert_eq!(default.stdout, explicit.stdout);
    assert_eq!(default.stdout, b"{\"status\":\"unavailable\",\"scope\":\"trusted_probe_image_only\",\"capabilities\":{\"filesystem_isolated\":false,\"network_isolated\":false,\"environment_isolated\":false,\"children_contained\":false,\"wall_time_limited\":false,\"output_limited\":false,\"cpu_quota\":false,\"memory_limited\":false,\"pids_limited\":false,\"disk_limited\":false},\"strict_available\":false,\"restricted_available\":false,\"project_code_available\":false,\"reason\":\"InvalidConfiguration\"}\n");
    assert!(default.stderr.is_empty() && explicit.stderr.is_empty());
    Ok(())
}
#[test]
fn invalid_configuration_human_output_is_fixed_and_does_not_echo_paths() -> TestResult {
    let mut human_args = complete();
    human_args.push(OsString::from("--human"));
    let output = run(&human_args)?;
    assert_eq!(output.status.code(), Some(1));
    let text = checked(String::from_utf8(output.stdout))?;
    assert!(text.starts_with("Sandbox capabilities: unavailable\nScope: trusted_probe_image_only\nReason: InvalidConfiguration\n"));
    assert!(text.contains("Project code available: false"));
    assert!(!text.contains("secret"));
    assert!(!text.contains("/unused"));
    assert!(output.stderr.is_empty());
    Ok(())
}
#[test]
fn duplicate_or_missing_format_configuration_is_syntax_failure() -> TestResult {
    for args in [
        vec!["capabilities"],
        vec!["capabilities", "--human"],
        vec!["capabilities", "--json", "--human"],
        vec!["capabilities", "--json", "--json"],
    ] {
        let output = run(&args.into_iter().map(OsString::from).collect::<Vec<_>>())?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
    Ok(())
}
