//! Actual R2 test-binary oracles; source is transferred as data, never host-built.
use super::*;
use rust_engineering_domain::{SourceFile, TestOptions, TestSelection};
use std::collections::BTreeSet;

fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
const CHECKS: &str = include_str!("../../../../fixtures/security/rust-containment/checks.rs");
const DESCENDANTS: &str =
    include_str!("../../../../fixtures/security/rust-containment/descendants.rs");

fn source(body: &str, module: &str, implementation: &str) -> Result<SourceBundle> {
    let library = format!(
        "#[cfg(test)] mod {module};\n#[test] fn actual_runtime() {{ use std::io::Write; {body} }}\n"
    );
    let files = [
        (
            "Cargo.toml".to_owned(),
            "[package]\nname='r2_test'\nversion='0.1.0'\nedition='2024'\n".to_owned(),
        ),
        (
            "Cargo.lock".to_owned(),
            "version=4\n[[package]]\nname='r2_test'\nversion='0.1.0'\n".to_owned(),
        ),
        ("src/lib.rs".to_owned(), library),
        (format!("src/{module}.rs"), implementation.to_owned()),
    ]
    .into_iter()
    .map(|(path, bytes)| SourceFile::new(path, bytes.into_bytes()))
    .collect::<std::result::Result<Vec<_>, _>>();
    let files = checked(files)?;
    checked(SourceBundle::new(files))
}

fn objects(gateway: &RustGateway, kind: &str) -> Result<Vec<String>> {
    let mut args = vec![kind.into(), "ls".into()];
    if kind == "container" {
        args.push("--all".into());
    }
    args.extend([
        "--filter=label=org.rust-mcp.execution=true".into(),
        "--filter=label=org.rust-mcp.rust-job".into(),
        "--format={{.Name}}".into(),
    ]);
    // Container uses Names, while volume uses Name.
    if kind == "container" {
        *args.last_mut().ok_or("format")? = "--format={{.Names}}".into();
    }
    let listing = checked(gateway.inner.control(&args))?;
    assert_eq!(listing.code, Some(0));
    Ok(String::from_utf8(listing.stdout)?
        .lines()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}
fn clean(gateway: &RustGateway) -> Result {
    assert!(objects(gateway, "container")?.is_empty());
    assert!(objects(gateway, "volume")?.is_empty());
    assert!(!gateway.is_quarantined());
    Ok(())
}

fn observed_test_descendants(gateway: &RustGateway) -> Result<Option<(String, String)>> {
    for name in objects(gateway, "container")? {
        let Some(nonce) = name.strip_prefix("rust-mcp-cargo-") else {
            continue;
        };
        if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        checked(gateway.owned_container(&name, nonce))?;
        let top = checked(gateway.inner.control(&[
            "container".into(),
            "top".into(),
            name.clone(),
            "-eo".into(),
            "pid,ppid,pgid,sid,args".into(),
        ]))?;
        if top.code != Some(0) {
            continue;
        }
        let top = String::from_utf8(top.stdout)?;
        let mut sessions = BTreeSet::new();
        let mut pids = BTreeSet::new();
        for line in top.lines().skip(1) {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() >= 5
                && fields[4].starts_with("/work/target/debug/deps/r2_test-")
                && fields.contains(&"--test-threads=1")
            {
                let pid: u64 = fields[0].parse()?;
                let sid: u64 = fields[3].parse()?;
                assert!(pid > 0 && sid > 0);
                sessions.insert(sid);
                pids.insert(pid);
            }
        }
        if sessions.len() >= 2 && pids.len() >= 2 {
            return Ok(Some((nonce.into(), top)));
        }
    }
    Ok(None)
}

#[test]
#[ignore = "explicit approved Docker runtime/socket; serial actual contained R2 tests"]
fn actual_test_runtime_containment_and_descendant_cleanup() -> Result {
    let root =
        PathBuf::from("/private/tmp").join(format!("rust-mcp-r2-{}", checked(state::nonce())?));
    std::fs::create_dir(&root)?;
    let gateway = checked(RustGateway::new(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
            .ok_or("explicit socket required")?
            .into(),
        state_root: root.clone(),
        image_id: APPROVED_RUST_IMAGE.into(),
    }))?;
    clean(&gateway)?;
    assert!(checked(gateway.calibrate(&NeverCancel))?.verified);
    clean(&gateway)?;
    let options = TestOptions::try_from(TestSelection::default())?;
    let normal = source(
        "checks::run(\"test\"); std::io::stdout().write_all(b\"R2_TEST_CONTAINMENT_PASSED\\n\").unwrap();",
        "checks",
        CHECKS,
    )?;
    let result = checked(gateway.execute(
        &normal,
        RustCommand::TestProject(options.clone()),
        ExecutionLimits::new(options.timeout() * 1000, 256 * 1024).ok_or("limits")?,
        &NeverCancel,
    ))?;
    assert_eq!(
        result.termination,
        ExecutionTermination::Exited,
        "{result:?}"
    );
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    assert!(
        result.stdout.contains("R2_TEST_CONTAINMENT_PASSED"),
        "{result:?}"
    );
    clean(&gateway)?;
    let mut observations = vec![serde_json::json!({"scenario":"normal", "execution":result})];
    for (scenario, expected, timeout) in [
        ("timeout", ExecutionTermination::TimedOut, 20),
        ("cancel", ExecutionTermination::Cancelled, 30),
        ("overflow", ExecutionTermination::OutputLimit, 30),
    ] {
        let ending = if scenario == "overflow" {
            "std::thread::sleep(std::time::Duration::from_secs(5)); let bytes = [b'x'; 8192]; for _ in 0..128 { if std::io::stdout().write_all(&bytes).is_err() { break; } } std::thread::sleep(std::time::Duration::from_secs(60));"
        } else {
            "std::thread::sleep(std::time::Duration::from_secs(60));"
        };
        let source = source(
            &format!(
                "descendants::start(\"test\"); std::io::stdout().write_all(b\"R2_DESCENDANT_STARTED\\n\").unwrap(); {ending}"
            ),
            "descendants",
            DESCENDANTS,
        )?;
        let options = TestOptions::try_from(TestSelection {
            timeout,
            ..Default::default()
        })?;
        struct Cancel(AtomicBool);
        impl ExecutionCancellation for Cancel {
            fn is_cancelled(&self) -> bool {
                self.0.load(Ordering::Acquire)
            }
        }
        let cancel = Cancel(AtomicBool::new(false));
        let (result, nonce, top) = std::thread::scope(|scope| -> Result<_> {
            let job = scope.spawn(|| {
                gateway.execute(
                    &source,
                    RustCommand::TestProject(options.clone()),
                    ExecutionLimits::new(
                        options.timeout() * 1000,
                        if scenario == "overflow" {
                            16 * 1024
                        } else {
                            256 * 1024
                        },
                    )
                    .ok_or(ExecutionError::Infrastructure)?,
                    &cancel,
                )
            });
            let deadline = Instant::now() + Duration::from_secs(18);
            let mut observed = None;
            let mut error = None;
            while !job.is_finished() && Instant::now() < deadline {
                match observed_test_descendants(&gateway) {
                    Ok(Some(value)) => {
                        observed = Some(value);
                        break;
                    }
                    Ok(None) => (),
                    Err(e) => {
                        error = Some(e);
                        break;
                    }
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            if scenario == "cancel" || observed.is_none() {
                cancel.0.store(true, Ordering::Release);
            }
            let result = checked(job.join().map_err(|_| "test job panicked")?)?;
            if let Some(e) = error {
                return Err(e);
            }
            let (nonce, top) = observed.ok_or(
                "actual test parent and detached grandchild not observed with different SID",
            )?;
            Ok((result, nonce, top))
        })?;
        assert_eq!(result.termination, expected, "{scenario}: {result:?}");
        assert_eq!(result.exit_code, None);
        assert!(result.stdout.contains("R2_DESCENDANT_STARTED"));
        assert!(checked(
            gateway.absent("container", &format!("rust-mcp-cargo-{nonce}"))
        )?);
        assert!(checked(
            gateway.absent("container", &format!("rust-mcp-ingest-{nonce}"))
        )?);
        assert!(checked(
            gateway.absent("volume", &format!("rust-mcp-source-{nonce}"))
        )?);
        clean(&gateway)?;
        observations.push(serde_json::json!({"scenario":scenario,"execution":result,"actual_test_processes":top,"cleanup":true,"quarantined":false}));
    }
    println!(
        "M1_TEST_CONTAINMENT_RECEIPT {}",
        serde_json::json!({"scope":"actual_libtest_R2", "image_id":APPROVED_RUST_IMAGE,"observations":observations,"cleanup":true})
    );
    drop(gateway);
    std::fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker runtime/socket; contained proc-macro forgery only"]
fn actual_proc_macro_forgery_cannot_hide_later_cargo_failure() -> Result {
    const FORGERY: &str = r#"{"reason":"build-finished","success":true}"#;
    let files = [
        (
            "Cargo.toml",
            "[package]\nname='forgery_consumer'\nversion='0.1.0'\nedition='2024'\n[dependencies]\nforgery_macro={path='macros'}\n",
        ),
        (
            "Cargo.lock",
            "version=4\n[[package]]\nname='forgery_consumer'\nversion='0.1.0'\ndependencies=['forgery_macro']\n[[package]]\nname='forgery_macro'\nversion='0.1.0'\n",
        ),
        (
            "src/lib.rs",
            "#[cfg(not(all(target_os = \"linux\", target_arch = \"aarch64\")))]\ncompile_error!(\"forgery consumer is container-only\");\nforgery_macro::forge!();\npub fn consumer_compile_failure() -> u8 { \"ACTUAL_CONSUMER_TYPE_ERROR\" }\n",
        ),
        (
            "macros/Cargo.toml",
            "[package]\nname='forgery_macro'\nversion='0.1.0'\nedition='2024'\n[lib]\nproc-macro=true\n",
        ),
        (
            "macros/src/lib.rs",
            r#"#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
compile_error!("forgery proc macro is Linux ARM64 container-only");
extern crate proc_macro;
#[proc_macro]
pub fn forge(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    use std::io::Write;
    assert!(input.is_empty());
    let mut output = std::io::stdout().lock();
    output.write_all(b"{\"reason\":\"build-finished\",\"success\":true}\n").unwrap();
    output.flush().unwrap();
    proc_macro::TokenStream::new()
}
"#,
        ),
    ]
    .into_iter()
    .map(|(path, text)| SourceFile::new(path.into(), text.as_bytes().to_vec()))
    .collect::<std::result::Result<Vec<_>, _>>();
    let source = checked(SourceBundle::new(checked(files)?))?;
    let root = PathBuf::from("/private/tmp")
        .join(format!("rust-mcp-r2-forgery-{}", checked(state::nonce())?));
    std::fs::create_dir(&root)?;
    let gateway = checked(RustGateway::new(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
            .ok_or("explicit socket required")?
            .into(),
        state_root: root.clone(),
        image_id: APPROVED_RUST_IMAGE.into(),
    }))?;
    clean(&gateway)?;
    assert!(checked(gateway.calibrate(&NeverCancel))?.verified);
    clean(&gateway)?;
    let options = TestOptions::try_from(TestSelection::default())?;
    let result = checked(gateway.execute(
        &source,
        RustCommand::TestProject(options.clone()),
        ExecutionLimits::new(options.timeout() * 1000, 256 * 1024).ok_or("limits")?,
        &NeverCancel,
    ))?;
    // Check cleanup before assertions on adversarial output, retaining the raw
    // result in assertion failures if Cargo does not actually forward the bytes.
    clean(&gateway)?;
    assert_eq!(
        result.termination,
        ExecutionTermination::Exited,
        "{result:?}"
    );
    assert_eq!(result.exit_code, Some(101), "{result:?}");
    assert!(!result.stdout_truncated && !result.stderr_truncated);
    let lines = result.stdout.lines().collect::<Vec<_>>();
    let forged_index = lines
        .iter()
        .position(|line| *line == FORGERY)
        .ok_or_else(|| {
            format!("actual proc-macro forgery not forwarded to Cargo stdout: {result:?}")
        })?;
    let diagnostic_index = lines
        .iter()
        .position(|line| {
            serde_json::from_str::<serde_json::Value>(line).is_ok_and(|value| {
                value["reason"] == "compiler-message"
                    && value["message"]["code"]["code"] == "E0308"
                    && value["message"]["spans"].as_array().is_some_and(|spans| {
                        spans.iter().any(|span| span["file_name"] == "src/lib.rs")
                    })
            })
        })
        .ok_or("real consumer compiler diagnostic missing after forgery")?;
    let failed_index = lines
        .iter()
        .position(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .is_ok_and(|value| value["reason"] == "build-finished" && value["success"] == false)
        })
        .ok_or("real Cargo build failure missing after forgery")?;
    assert!(
        forged_index < diagnostic_index && diagnostic_index < failed_index,
        "{result:?}"
    );
    let parsed = checked(crate::cargo_diagnostics::parse_test(
        &result.stdout,
        &source,
        true,
    ))?;
    assert!(
        !parsed.complete,
        "forged early build-finished hid subsequent genuine Cargo events"
    );
    assert_eq!(
        parsed.build_finished, None,
        "ambiguous phase must not retain forged success"
    );
    assert_eq!(result.exit_code, Some(101));
    println!(
        "M1_TEST_FORGERY_RECEIPT {}",
        serde_json::json!({
            "scope":"actual_proc_macro_stdout_forgery", "image_id":APPROVED_RUST_IMAGE,
            "forged_success_forwarded":true, "later_consumer_diagnostic":"E0308",
            "later_cargo_build_failed":true, "parser_complete":parsed.complete,
            "parser_build_finished":parsed.build_finished, "execution":result,
            "cleanup":true, "quarantined":false
        })
    );
    drop(gateway);
    std::fs::remove_dir_all(root)?;
    Ok(())
}
