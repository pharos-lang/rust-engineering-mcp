//! Exact-image native qualification, explicitly invoked under exclusive Docker ownership.
use crate::*;
use rust_engineering_domain::{CargoVendorSnapshot, SourceBundle, SourceFile};
#[test]
#[ignore = "explicit M4 runtime, native interpreter oracles and exclusive Docker"]
fn m4_miri_gateway_classifies_native_oracles() -> Result<(), Box<dyn std::error::Error>> {
    let state_root = std::path::PathBuf::from("/private/tmp").join(format!(
        "m4-miri-native-{}",
        state::nonce().map_err(|e| format!("{e:?}"))?
    ));
    std::fs::create_dir(&state_root)?;
    let gateway = RustGateway::new_m4_for_qualification(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
        state_root: state_root.clone(),
        image_id: "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635".into(),
    })
    .map_err(|e| format!("{e:?}"))?;
    gateway.set_verified(true);
    let empty = SourceBundle::new(vec![]).map_err(|e| format!("{e:?}"))?;
    let vendor = CargoVendorSnapshot {
        tree_fingerprint: resolution_gateway::tree_fingerprint(&empty)
            .map_err(|e| format!("{e:?}"))?,
        source: empty,
        packages: vec![],
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/m4-runtime-oracles/miri-classification");
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-miri-gateway");
    std::fs::create_dir_all(&output)?;
    let mut results = Vec::new();
    for name in [
        "optimized-panic",
        "binary-only",
        "runner-timeout",
        "clean",
        "uaf",
        "uninit",
        "alias",
        "race",
        "ffi",
        "benign-forged",
        "compile-fail",
        "empty",
        "ignored",
    ] {
        let source = SourceBundle::new(
            (if name == "binary-only" {
                vec!["Cargo.toml", "Cargo.lock", "src/main.rs", "tests/benign.rs"]
            } else {
                vec!["Cargo.toml", "Cargo.lock", "src/lib.rs"]
            })
            .into_iter()
            .map(|path| {
                Ok(
                    SourceFile::new(path.into(), std::fs::read(root.join(name).join(path))?)
                        .map_err(|e| format!("{e:?}"))?,
                )
            })
            .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?,
        )
        .map_err(|e| format!("{e:?}"))?;
        let result = security_gateway::execute_miri(
            &gateway,
            &source,
            &vendor,
            ExecutionLimits::new_job(180_000, 1024 * 1024).ok_or("limits")?,
            &NeverCancel,
        )
        .map_err(|e| format!("{name}: {e:?}"))?;
        std::fs::write(
            output.join(format!("{name}.stdout")),
            &result.capture.stdout,
        )?;
        std::fs::write(
            output.join(format!("{name}.stderr")),
            &result.capture.stderr,
        )?;
        if let Some(junit) = &result.junit {
            std::fs::write(output.join(format!("{name}.junit.xml")), junit)?;
        }
        let report = miri_output::parse(
            result.junit.as_deref(),
            &result.capture.stdout,
            &result.capture.stderr,
            result.capture.code.ok_or("exit")?,
        )
        .map_err(|e| format!("{name}: {e:?}"))?;
        assert!(report.validate(), "{name}: {report:?}");
        assert_eq!(
            report.clean,
            matches!(name, "clean" | "binary-only"),
            "{name}: {report:?}"
        );
        match name {
            "runner-timeout" => {
                assert_eq!(report.counts.timeouts, 1);
                assert!(!report.complete);
            }
            "uaf" | "uninit" | "alias" | "race" => {
                assert_eq!(report.counts.undefined_behavior, 1, "{name}: {report:?}")
            }
            "ffi" => assert_eq!(report.counts.unsupported_operation, 1),
            "benign-forged" | "optimized-panic" => {
                assert_eq!(report.counts.test_failures, 1);
                assert_eq!(report.counts.undefined_behavior, 0);
            }
            "compile-fail" => assert_eq!(report.counts.compile_failures, 1),
            "empty" | "ignored" => assert!(!report.complete),
            _ => (),
        }
        results.push(serde_json::json!({"case":name,"report":report,"source":result.source_fingerprint,"metadata":result.metadata.original_fingerprint,"execution":result.execution_fingerprint,"stdout_sha256":digest(&result.capture.stdout),"stderr_sha256":digest(&result.capture.stderr),"junit_sha256":result.junit.as_ref().map(|b|digest(b))}));
        println!("M4_MIRI_GATEWAY {name}");
    }
    for kind in ["container", "volume"] {
        let mut args = vec![kind.into(), "ls".into()];
        if kind == "container" {
            args.push("--all".into());
        }
        args.push("--filter=label=org.rust-mcp.execution=true".into());
        args.push(if kind == "container" {
            "--format={{.ID}}".into()
        } else {
            "--format={{.Name}}".into()
        });
        let result = gateway.inner.control(&args).map_err(|e| format!("{e:?}"))?;
        assert_eq!(result.code, Some(0));
        assert!(result.stdout.iter().all(u8::is_ascii_whitespace));
    }
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"image_id":gateway.image_id(),"cases":results,"cleanup_verified":true}),
        )?,
    )?;
    drop(gateway);
    std::fs::remove_dir_all(state_root)?;
    Ok(())
}

#[test]
#[ignore = "explicit M4 runtime, adversarial interpreter admission and exclusive Docker"]
fn m4_miri_rejects_native_producers_and_joins_timeout_and_cancel()
-> Result<(), Box<dyn std::error::Error>> {
    use rust_engineering_application::security::SecurityError;
    use rust_engineering_application::{InspectionError, ProjectError};
    use std::time::Instant;
    struct ObserveMiri<'a> {
        gateway: &'a RustGateway,
        observed: std::sync::atomic::AtomicBool,
        failed: std::sync::atomic::AtomicBool,
        cancel: bool,
    }
    impl ExecutionCancellation for ObserveMiri<'_> {
        fn is_cancelled(&self) -> bool {
            use std::sync::atomic::Ordering;
            if !self.observed.load(Ordering::SeqCst) {
                let result = self.gateway.inner.control(&[
                    "container".into(),
                    "ls".into(),
                    "--filter=label=org.rust-mcp.execution=true".into(),
                    "--no-trunc".into(),
                    "--format={{.Command}}".into(),
                ]);
                match result {
                    Ok(result) if result.code == Some(0) => {
                        if String::from_utf8_lossy(&result.stdout)
                            .contains("/opt/rust-nightly-2026-09-07/bin/cargo miri nextest run")
                        {
                            self.observed.store(true, Ordering::SeqCst);
                        }
                    }
                    _ => {
                        self.failed.store(true, Ordering::SeqCst);
                        return true;
                    }
                }
            }
            self.cancel && self.observed.load(Ordering::SeqCst)
        }
    }
    let state_root = std::path::PathBuf::from("/private/tmp").join(format!(
        "m4-miri-adversarial-{}",
        state::nonce().map_err(|e| format!("{e:?}"))?
    ));
    std::fs::create_dir(&state_root)?;
    let gateway = RustGateway::new_m4_for_qualification(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
        state_root: state_root.clone(),
        image_id: "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635".into(),
    })
    .map_err(|e| format!("{e:?}"))?;
    gateway.set_verified(true);
    let empty = SourceBundle::new(vec![]).map_err(|e| format!("{e:?}"))?;
    let vendor = CargoVendorSnapshot {
        tree_fingerprint: resolution_gateway::tree_fingerprint(&empty)
            .map_err(|e| format!("{e:?}"))?,
        source: empty,
        packages: vec![],
    };
    let mut cases = Vec::new();
    for name in [
        "build-script",
        "proc-macro",
        "custom-harness",
        "cargo-config",
        "nested-toolchain",
        "timeout",
        "cancel",
    ] {
        let mut manifest =
            "[package]\nname='miri_admission'\nversion='0.1.0'\nedition='2024'\n".to_string();
        let lib = if matches!(name, "timeout" | "cancel") {
            "#[test] fn unbounded() { let mut x=0u64; loop { x=x.wrapping_add(1); std::hint::black_box(x); } }"
        } else {
            "#[test] fn clean() {}"
        };
        if name == "proc-macro" {
            manifest.push_str("[lib]\nproc-macro=true\n");
        }
        if name == "custom-harness" {
            manifest.push_str("[lib]\nharness=false\n");
        }
        let mut files = vec![
            ("Cargo.toml", manifest.into_bytes()),
            (
                "Cargo.lock",
                b"version = 4\n[[package]]\nname = \"miri_admission\"\nversion = \"0.1.0\"\n"
                    .to_vec(),
            ),
            ("src/lib.rs", lib.as_bytes().to_vec()),
        ];
        if name == "build-script" {
            files.push((
                "build.rs",
                b"fn main() { panic!(\"NATIVE_PRODUCER_MUST_NOT_EXECUTE\"); }".to_vec(),
            ));
        }
        if name == "cargo-config" {
            files.push((
                ".cargo/config.toml",
                b"[env]\nMIRIFLAGS={value='-Zmiri-disable-validation',force=true}\n".to_vec(),
            ));
        }
        if name == "nested-toolchain" {
            files.push((
                "member/rust-toolchain.toml",
                b"[toolchain]\nchannel='nightly'\n".to_vec(),
            ));
        }
        let source = SourceBundle::new(
            files
                .into_iter()
                .map(|(path, bytes)| SourceFile::new(path.into(), bytes))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("{e:?}"))?,
        )
        .map_err(|e| format!("{e:?}"))?;
        let monitor = ObserveMiri {
            gateway: &gateway,
            observed: std::sync::atomic::AtomicBool::new(false),
            failed: std::sync::atomic::AtomicBool::new(false),
            cancel: name == "cancel",
        };
        let control: &dyn ExecutionCancellation = if matches!(name, "cancel" | "timeout") {
            &monitor
        } else {
            &NeverCancel
        };
        let started = Instant::now();
        let result = security_gateway::execute_miri(
            &gateway,
            &source,
            &vendor,
            ExecutionLimits::new_job(if name == "timeout" { 30_000 } else { 60_000 }, 1024 * 1024)
                .ok_or("limits")?,
            control,
        );
        let error = match result {
            Ok(_) => return Err(format!("{name} unexpectedly executed successfully").into()),
            Err(e) => e,
        };
        match name {
            "timeout" => assert_eq!(error, SecurityError::Timeout),
            "cancel" => assert_eq!(
                error,
                SecurityError::Inspection(InspectionError::Project(ProjectError::Cancelled))
            ),
            _ => assert_eq!(error, SecurityError::ClassificationIntegrityUnsupported),
        }
        if matches!(name, "cancel" | "timeout") {
            assert!(
                monitor.observed.load(std::sync::atomic::Ordering::SeqCst),
                "Miri process was never observed running: {name}"
            );
            assert!(!monitor.failed.load(std::sync::atomic::Ordering::SeqCst));
        }
        for kind in ["container", "volume"] {
            let mut args = vec![kind.into(), "ls".into()];
            if kind == "container" {
                args.push("--all".into());
            }
            args.push("--filter=label=org.rust-mcp.execution=true".into());
            args.push(if kind == "container" {
                "--format={{.ID}}".into()
            } else {
                "--format={{.Name}}".into()
            });
            let inventory = gateway.inner.control(&args).map_err(|e| format!("{e:?}"))?;
            assert_eq!(inventory.code, Some(0));
            assert!(
                inventory.stdout.iter().all(u8::is_ascii_whitespace),
                "{name}: residual {kind}"
            );
        }
        cases.push(serde_json::json!({"case":name,"error":format!("{error:?}"),"elapsed_ms":started.elapsed().as_millis(),"miri_running_observed":monitor.observed.load(std::sync::atomic::Ordering::SeqCst),"cleanup_verified":true,"source":resolution_gateway::tree_fingerprint(&source).map_err(|e|format!("{e:?}"))?}));
        println!("M4_MIRI_ADVERSARIAL {name}");
    }
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-miri-gateway");
    std::fs::create_dir_all(&output)?;
    std::fs::write(
        output.join("adversarial-receipt.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"image_id":gateway.image_id(),"cases":cases}),
        )?,
    )?;
    drop(gateway);
    std::fs::remove_dir_all(state_root)?;
    Ok(())
}
