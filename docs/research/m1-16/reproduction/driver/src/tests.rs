use super::*;
#[test]
fn closed_requests_reject_arbitrary_execution_and_configuration() {
    for raw in [
        r#"{"op":"execute","files":[],"command":"shell"}"#,
        r#"{"op":"execute","files":[],"command":"check","argv":["--offline"]}"#,
        r#"{"op":"call","name":"x","arguments":{},"init":{}}"#,
        r#"{"op":"close","extra":true}"#,
    ] {
        assert!(serde_json::from_str::<Input>(raw).is_err());
    }
}
#[test]
fn source_rejects_untrusted_paths_and_duplicate_files() {
    for path in [
        "/tmp/escape",
        "../escape",
        "src/../lib.rs",
        "build.rs",
        ".cargo/config.toml",
    ] {
        assert!(
            source(vec![File {
                path: path.into(),
                text: String::new()
            }])
            .is_err()
        );
    }
    let files = vec![
        File {
            path: "Cargo.toml".into(),
            text: String::new(),
        },
        File {
            path: "Cargo.toml".into(),
            text: String::new(),
        },
    ];
    assert!(source(files).is_err());
    let files = [
        "Cargo.toml",
        "Cargo.lock",
        "src/lib.rs",
        "tests/behavior.rs",
    ]
    .into_iter()
    .map(|path| File {
        path: path.into(),
        text: String::new(),
    })
    .collect();
    assert!(source(files).is_ok());
}
#[test]
fn host_init_is_closed_and_complete() {
    let base = json!({"mode":"raw","server_binary":"/trusted/server","root":"/trusted/root","state_root":"/trusted/state","docker_socket":"/trusted/socket"});
    let parsed: Init = serde_json::from_value(base.clone()).expect("fixed test value");
    assert!(parsed.validate().is_ok());
    let mut relative = base.clone();
    relative["root"] = json!("relative");
    assert!(
        serde_json::from_value::<Init>(relative)
            .expect("fixed")
            .validate()
            .is_err()
    );
    let mut incomplete = base.clone();
    incomplete["catalog_store"] = json!("/trusted/catalog");
    assert!(
        serde_json::from_value::<Init>(incomplete)
            .expect("fixed")
            .validate()
            .is_err()
    );
    let mut extra = base;
    extra["env"] = json!({"SECRET":"value"});
    assert!(serde_json::from_value::<Init>(extra).is_err());
}
#[test]
fn resource_identity_has_no_filesystem_or_network_authority() {
    assert!(valid_resource(&format!(
        "rust-artifact://prj_{}/art_{}",
        "0".repeat(32),
        "a".repeat(32)
    )));
    for value in [
        "file:///etc/passwd",
        "https://example.org",
        "rust-artifact://../x",
        "rust-artifact://prj_x/art_x",
    ] {
        assert!(!valid_resource(value));
    }
}
#[test]
fn commands_have_fixed_strict_selection_and_limits() {
    let RustCommand::ClippyProject(options) = command(Command::Clippy, None).expect("fixed") else {
        panic!("wrong variant")
    };
    assert_eq!(options.lint_profile(), LintProfile::Strict);
    assert!(options.features().is_empty());
    let RustCommand::TestProject(options) = command(Command::Test, None).expect("fixed") else {
        panic!("wrong variant")
    };
    assert_eq!(options.timeout(), 30);
    assert!(ExecutionLimits::new(60_000, 256 * 1024).is_some());
    assert!(ExecutionLimits::new(120_000, 256 * 1024).is_none());
}
#[test]
fn bounded_lines_and_cancel_are_permanent() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let mut good = BufReader::new(&b"{}\n"[..]);
        assert_eq!(line(&mut good).await.expect("line"), Some(b"{}\n".to_vec()));
        let large = vec![b'x'; LIMIT + 1];
        let mut excessive = BufReader::new(large.as_slice());
        assert_eq!(line(&mut excessive).await, Err("input_limit"));
        let mut partial = BufReader::new(&b"{}"[..]);
        assert_eq!(line(&mut partial).await, Err("unterminated_line"));
        let cancel = Cancel::default();
        cancel.cancel();
        cancel.wait().await;
        cancel.cancel();
        assert!(cancel.is_cancelled());
    });
}

#[test]
fn explain_has_closed_validated_code() {
    assert!(matches!(
        command(Command::Explain, Some("E0502".into())),
        Ok(RustCommand::Explain(_))
    ));
    assert!(command(Command::Explain, Some("--help".into())).is_err());
    assert!(command(Command::Explain, None).is_err());
    assert!(command(Command::Check, Some("E0502".into())).is_err());
}

#[test]
fn cancellation_never_masks_cleanup_or_other_execution_errors() {
    let cancel = Cancel::default();
    cancel.cancel();
    for stage in [
        "gateway_initialization",
        "gateway_calibration",
        "gateway_execution_or_cleanup",
    ] {
        let error = cancel.gateway_error(ExecutionError::CleanupUncertain, stage);
        assert_eq!(
            cancel.after_cancellation::<()>(Err(error)),
            Err("gateway_cleanup_uncertain")
        );
        assert!(cancel.cleanup_uncertain.load(Ordering::SeqCst));
        let infrastructure = cancel.gateway_error(ExecutionError::Infrastructure, stage);
        assert_eq!(
            cancel.after_cancellation::<()>(Err(infrastructure)),
            Err(stage)
        );
    }
    // A later clean result cannot clear previously observed uncertainty.
    assert_eq!(cancel.after_cancellation(Ok(())), Err("cancelled"));
    cancel.execution_joined.store(true, Ordering::SeqCst);
    let ack = cancel.acknowledgement(Some("gateway_cleanup_uncertain"));
    assert_eq!(ack["execution_joined"], true);
    assert_eq!(ack["cleanup_uncertain"], true);
    assert_eq!(ack["driver_error"], "gateway_cleanup_uncertain");
}

#[test]
fn clean_cancellation_retains_join_without_uncertainty() {
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(
        cancel.gateway_error(ExecutionError::Cancelled, "gateway_calibration"),
        "cancelled"
    );
    assert_eq!(cancel.after_cancellation(Ok(())), Err("cancelled"));
    cancel.execution_joined.store(true, Ordering::SeqCst);
    let ack = cancel.acknowledgement(Some("cancelled"));
    assert_eq!(ack["execution_joined"], true);
    assert_eq!(ack["server_joined"], false);
    assert_eq!(ack["cleanup_uncertain"], false);
}

#[test]
fn panicked_gateway_worker_is_joined_but_cleanup_is_uncertain() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        let cancel = Cancel::default();
        cancel.cancel();
        let joined =
            tokio::task::spawn_blocking(|| -> Result<()> { panic!("injected worker panic") }).await;
        let result = cancel.worker_result(joined);
        assert_eq!(
            cancel.after_cancellation(result),
            Err("gateway_worker_panicked")
        );
        cancel.execution_joined.store(true, Ordering::SeqCst);
        assert_eq!(
            cancel.acknowledgement(Some("gateway_worker_panicked"))["cleanup_uncertain"],
            true
        );
    });
}

#[test]
fn failed_server_wait_and_unsuccessful_exit_cannot_acknowledge_cleanup() {
    use std::os::unix::process::ExitStatusExt;
    let cancel = Cancel::default();
    cancel.cancel();
    let wait = cancel.server_exit(Err(std::io::Error::other("injected wait failure")));
    assert_eq!(cancel.after_cancellation(wait), Err("child_join"));
    let ack = cancel.acknowledgement(Some("child_join"));
    assert_eq!(ack["server_joined"], false);
    assert_eq!(ack["cleanup_uncertain"], true);
    let failed = Cancel::default();
    assert_eq!(
        failed.server_exit(Ok(std::process::ExitStatus::from_raw(256))),
        Err("child_failed")
    );
    assert_eq!(
        failed.acknowledgement(Some("child_failed"))["server_joined"],
        true
    );
    assert_eq!(
        failed.acknowledgement(Some("child_failed"))["cleanup_uncertain"],
        true
    );
    let clean = Cancel::default();
    assert!(
        clean
            .server_exit(Ok(std::process::ExitStatus::from_raw(0)))
            .is_ok()
    );
    assert_eq!(clean.acknowledgement(None)["server_joined"], true);
    assert_eq!(clean.acknowledgement(None)["cleanup_uncertain"], false);
    assert_eq!(
        clean.acknowledgement(None)["server_exit"],
        json!({"code":0,"signal":null,"success":true})
    );
    assert_eq!(
        failed.acknowledgement(Some("child_failed"))["server_exit"],
        json!({"code":1,"signal":null,"success":false})
    );
}

#[test]
fn expected_sdk_cancellation_is_not_a_transport_or_cleanup_failure() {
    use rmcp::service::ServiceError;
    let cancelled = Cancel::default();
    cancelled.cancel();
    assert_eq!(
        cancelled.mcp_failure(ServiceError::Cancelled { reason: None }),
        Err("cancelled")
    );
    assert!(!cancelled.cleanup_uncertain.load(Ordering::SeqCst));
    let unexpected = Cancel::default();
    assert_eq!(
        unexpected.mcp_failure(ServiceError::Cancelled { reason: None }),
        Err("mcp_request_failed")
    );
    assert!(unexpected.cleanup_uncertain.load(Ordering::SeqCst));
    assert_eq!(
        cancelled.mcp_failure(ServiceError::McpError(ErrorData::internal_error(
            "cleanup failure",
            None
        ))),
        Err("mcp_internal_error")
    );
    assert!(cancelled.cleanup_uncertain.load(Ordering::SeqCst));
}

#[test]
fn retained_pipe_drains_late_output_after_sdk_reader_is_dropped() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    runtime.block_on(async {
        // Trusted fixed OS fixture: no project program or shell is executed.
        let mut child = tokio::process::Command::new("/bin/cat")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("fixed cat fixture");
        let reader = child.stdout.take().expect("piped stdout");
        let retained = retain_stdout(&reader).expect("duplicate reader");
        drop(reader);
        let mut input = child.stdin.take().expect("piped stdin");
        let write = async move {
            input
                .write_all(&vec![b'x'; 256 * 1024])
                .await
                .expect("late response write");
            input.shutdown().await.expect("stdin shutdown");
        };
        let ((), drained, status) = tokio::time::timeout(Duration::from_secs(5), async {
            tokio::join!(write, drain_stdout(retained), child.wait())
        })
        .await
        .expect("bounded drain");
        assert_eq!(drained, Ok(256 * 1024));
        assert!(status.expect("wait").success());
        let excessive = vec![b'x'; LIMIT + 1];
        assert_eq!(
            drain_stdout(excessive.as_slice()).await,
            Err("shutdown_stdout_limit")
        );
    });
}
