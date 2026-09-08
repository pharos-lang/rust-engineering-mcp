//! Explicit M4 native qualification; never called by normal runtime discovery.
use crate::*;
use rust_engineering_domain::security::SecurityPolicy;
use rust_engineering_domain::{CargoVendorSnapshot, SourceBundle, SourceFile};

#[test]
#[ignore = "explicit M4 image; exclusive native base-containment calibration"]
fn m4_runtime_base_containment_is_requalified() -> Result<(), Box<dyn std::error::Error>> {
    let state_root = std::path::PathBuf::from("/private/tmp").join(format!(
        "m4-runtime-qualification-{}",
        state::nonce().map_err(|e| format!("{e:?}"))?
    ));
    std::fs::create_dir(&state_root)?;
    let gateway = RustGateway::new_m4_for_qualification(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
        state_root: state_root.clone(),
        image_id: crate::security_port::M4_IMAGE.into(),
    })
    .map_err(|e| format!("gateway: {e:?}"))?;
    let report = gateway
        .calibrate(&NeverCancel)
        .map_err(|e| format!("calibration: {e:?}"))?;
    let bytes = serde_json::to_vec_pretty(&report)?;
    let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/m4-base-calibration.json");
    std::fs::write(output, &bytes)?;
    assert!(report.verified);
    assert_eq!(report.image_id, crate::security_port::M4_IMAGE);
    println!("M4_BASE_CALIBRATION {}", digest(&bytes));
    drop(gateway);
    std::fs::remove_dir_all(state_root)?;
    Ok(())
}

#[test]
#[ignore = "explicit M4 image; exclusive native base-containment calibration"]
fn m4_scanner_runtime_base_containment_is_requalified() -> Result<(), Box<dyn std::error::Error>> {
    let state_root = std::path::PathBuf::from("/private/tmp").join(format!(
        "m4-runtime-qualification-{}",
        state::nonce().map_err(|e| format!("{e:?}"))?
    ));
    std::fs::create_dir(&state_root)?;
    let gateway = RustGateway::new_m4_for_qualification(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
        state_root: state_root.clone(),
        image_id: "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635".into(),
    })
    .map_err(|e| format!("gateway: {e:?}"))?;
    let report = gateway
        .calibrate(&NeverCancel)
        .map_err(|e| format!("calibration: {e:?}"))?;
    let bytes = serde_json::to_vec_pretty(&report)?;
    let output = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/m4-scanner-base-calibration.json");
    std::fs::write(output, &bytes)?;
    assert!(report.verified);
    assert_eq!(
        report.image_id,
        "sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635"
    );
    println!("M4_SCANNER_BASE_CALIBRATION {}", digest(&bytes));
    drop(gateway);
    std::fs::remove_dir_all(state_root)?;
    Ok(())
}

fn source(license: bool) -> Result<SourceBundle, Box<dyn std::error::Error>> {
    let mut files = vec![
        SourceFile::new(
            "Cargo.toml".into(),
            b"[package]\nname='m4-deny-probe'\nversion='0.1.0'\nedition='2024'\nlicense='MIT'\n"
                .to_vec(),
        ),
        SourceFile::new(
            "Cargo.lock".into(),
            b"version=4\n[[package]]\nname='m4-deny-probe'\nversion='0.1.0'\n".to_vec(),
        ),
        SourceFile::new("src/lib.rs".into(), b"pub fn fixture() {}\n".to_vec()),
    ]
    .into_iter()
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| format!("{e:?}"))?;
    if license {
        files.push(
            SourceFile::new(
                "LICENSE".into(),
                include_bytes!("../../../fixtures/m4-deny-native/LICENSE-MIT").to_vec(),
            )
            .map_err(|e| format!("{e:?}"))?,
        );
    }
    SourceBundle::new(files).map_err(|e| format!("{e:?}").into())
}
fn policy(allowed: &str, ban: bool) -> Result<SecurityPolicy, Box<dyn std::error::Error>> {
    let bytes = serde_json::to_vec(&serde_json::json!({"schema_version":1,"rules":{
        "allowed_licenses":[allowed],"banned_packages":if ban {serde_json::json!([{"name":"m4-deny-probe","version_requirement":"=0.1.0"}])} else {serde_json::json!([])},
        "multiple_versions":"deny","wildcards":"deny"},"suppressions":[]}))?;
    security_policy::parse_security_policy(&bytes, &digest(&bytes).parse()?, 100)
        .map_err(|e| format!("{e:?}").into())
}

#[test]
#[ignore = "explicit provisioned M4 image and local Docker; exclusive native qualification"]
fn m4_deny_native_text_licenses_and_bans_are_real_and_cleanup_is_joined()
-> Result<(), Box<dyn std::error::Error>> {
    let state_root = std::path::PathBuf::from("/private/tmp").join(format!(
        "m4-deny-qualification-{}",
        state::nonce().map_err(|e| format!("{e:?}"))?
    ));
    std::fs::create_dir(&state_root)?;
    let gateway = RustGateway::new_m4_for_qualification(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
        state_root: state_root.clone(),
        image_id: crate::APPROVED_M4_IMAGE.into(),
    })
    .map_err(|e| format!("gateway: {e:?}"))?;
    // This fixture tests the new phase path; it does not claim base calibration
    // on its own. The separate calibration and full MCP cases qualify admission.
    gateway.set_verified(true);
    let empty = SourceBundle::new(vec![]).map_err(|e| format!("{e:?}"))?;
    let vendor = CargoVendorSnapshot {
        tree_fingerprint: resolution_gateway::tree_fingerprint(&empty)
            .map_err(|e| format!("{e:?}"))?,
        source: empty,
        packages: vec![],
    };
    let output =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-deny-native");
    std::fs::create_dir_all(&output)?;
    for (name, license, allowed, banned, expected) in [
        ("clean", true, "MIT", false, 0),
        ("manifest-without-text", false, "MIT", false, 4),
        ("text-not-allowed", true, "Apache-2.0", false, 4),
        ("banned-package", true, "MIT", true, 2),
    ] {
        let result = security_gateway::execute(
            &gateway,
            &source(license)?,
            &vendor,
            &policy(allowed, banned)?,
            ExecutionLimits::new_job(120_000, 1024 * 1024).ok_or("limits")?,
            &NeverCancel,
        )
        .map_err(|e| format!("{name}: {e:?}"))?;
        std::fs::write(
            output.join(format!("{name}.stderr.jsonl")),
            &result.capture.stderr,
        )?;
        std::fs::write(
            output.join(format!("{name}.stdout")),
            &result.capture.stdout,
        )?;
        assert_eq!(
            result.capture.code,
            Some(expected),
            "{name}: {}",
            String::from_utf8_lossy(&result.capture.stderr)
        );
        let parsed = deny_json::parse(
            &result.capture.stderr,
            &result.capture.stdout,
            expected,
            result.capture.stdout_truncated || result.capture.stderr_truncated,
            &result.metadata.packages,
        )
        .map_err(|e| format!("{name}: {e:?}"))?;
        assert!(parsed.parse_complete, "{name}");
        assert_eq!(parsed.findings_omitted, 0);
        assert!(
            result
                .metadata
                .declared_licenses
                .iter()
                .all(|l| l.as_deref() == Some("MIT"))
        );
        assert_eq!(!result.metadata.license_files[0].is_empty(), license);
        println!(
            "M4_DENY_ORACLE {}",
            serde_json::json!({"case":name,"exit":expected,"licenses":parsed.licenses,"bans":parsed.bans,"sources":parsed.sources,
            "execution_fingerprint":result.execution_fingerprint,"metadata":result.metadata.original_fingerprint,"stderr_sha256":digest(&result.capture.stderr)})
        );
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
        let found = gateway.inner.control(&args).map_err(|e| format!("{e:?}"))?;
        assert_eq!(found.code, Some(0));
        assert!(found.stdout.iter().all(u8::is_ascii_whitespace));
    }
    drop(gateway);
    std::fs::remove_dir_all(state_root)?;
    Ok(())
}
