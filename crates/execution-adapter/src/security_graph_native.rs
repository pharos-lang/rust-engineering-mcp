//! Native cargo-deny graph-shape qualification for a path-only workspace.

use crate::*;
use rust_engineering_domain::security::SecurityPolicy;
use rust_engineering_domain::{CargoVendorSnapshot, SourceBundle, SourceFile};
use std::path::PathBuf;

const M4_IMAGE: &str = crate::APPROVED_M4_IMAGE;

fn file(path: &str, bytes: impl Into<Vec<u8>>) -> Result<SourceFile, Box<dyn std::error::Error>> {
    SourceFile::new(path.into(), bytes.into()).map_err(|error| format!("{path}: {error:?}").into())
}

fn source(exact: bool) -> Result<SourceBundle, Box<dyn std::error::Error>> {
    let version = if exact { ", version='=0.1.0'" } else { "" };
    SourceBundle::new(vec![
        file(
            "Cargo.toml",
            format!(
                "[workspace]\nmembers=['app','helper']\nresolver='3'\n\n[workspace.dependencies]\nrenamed={{package='helper',path='helper'{version},default-features=false,features=['extra']}}\n"
            ),
        )?,
        file(
            "Cargo.lock",
            b"version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\n \"helper\",\n]\n\n[[package]]\nname = \"helper\"\nversion = \"0.1.0\"\n",
        )?,
        file(
            "app/Cargo.toml",
            b"[package]\nname='app'\nversion='0.1.0'\nedition='2024'\nlicense='MIT'\n\n[dependencies]\nrenamed.workspace=true\n\n[dev-dependencies]\nrenamed.workspace=true\n\n[target.'cfg(unix)'.build-dependencies]\nrenamed.workspace=true\n",
        )?,
        file("app/src/lib.rs", b"pub fn app() { helper::helper(); }\n")?,
        file(
            "helper/Cargo.toml",
            b"[package]\nname='helper'\nversion='0.1.0'\nedition='2024'\nlicense='MIT'\n\n[features]\nextra=[]\n",
        )?,
        file("helper/src/lib.rs", b"pub fn helper() {}\n")?,
        file(
            "app/LICENSE-MIT",
            include_bytes!("../../../fixtures/m4-deny-native/LICENSE-MIT").to_vec(),
        )?,
        file(
            "helper/LICENSE-MIT",
            include_bytes!("../../../fixtures/m4-deny-native/LICENSE-MIT").to_vec(),
        )?,
    ])
    .map_err(|error| format!("source: {error:?}").into())
}

fn policy() -> Result<SecurityPolicy, Box<dyn std::error::Error>> {
    let bytes = serde_json::to_vec(&serde_json::json!({
        "schema_version": 1,
        "rules": {
            "allowed_licenses": ["MIT"],
            "banned_packages": [],
            "multiple_versions": "deny",
            "wildcards": "deny"
        },
        "suppressions": []
    }))?;
    security_policy::parse_security_policy(&bytes, &digest(&bytes).parse()?, 100)
        .map_err(|error| format!("policy: {error:?}").into())
}

fn vendor() -> Result<CargoVendorSnapshot, Box<dyn std::error::Error>> {
    let source = SourceBundle::new(vec![]).map_err(|error| format!("vendor: {error:?}"))?;
    Ok(CargoVendorSnapshot {
        tree_fingerprint: resolution_gateway::tree_fingerprint(&source)
            .map_err(|error| format!("vendor fingerprint: {error:?}"))?,
        source,
        packages: vec![],
    })
}

fn inventory_is_empty(gateway: &RustGateway) -> Result<(), Box<dyn std::error::Error>> {
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
        let found = gateway
            .inner
            .control(&args)
            .map_err(|error| format!("inventory: {error:?}"))?;
        assert_eq!(found.code, Some(0));
        assert!(found.stdout.iter().all(u8::is_ascii_whitespace));
    }
    Ok(())
}

#[test]
#[ignore = "explicit provisioned M4 image and local Docker; graph-shape qualification"]
fn captures_workspace_dependency_graphs_through_the_gateway()
-> Result<(), Box<dyn std::error::Error>> {
    let nonce = state::nonce().map_err(|error| format!("nonce: {error:?}"))?;
    let state_root = PathBuf::from("/private/tmp").join(format!("m4-deny-graph-{nonce}"));
    std::fs::create_dir(&state_root)?;
    let gateway = RustGateway::new_m4_for_qualification(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
        state_root: state_root.clone(),
        image_id: M4_IMAGE.into(),
    })
    .map_err(|error| format!("gateway: {error:?}"))?;
    gateway.set_verified(true);
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-deny-graph");
    std::fs::create_dir_all(&output)?;

    for (name, exact, expected_exit) in [("wildcard", false, 2), ("exact", true, 0)] {
        let result = security_gateway::execute(
            &gateway,
            &source(exact)?,
            &vendor()?,
            &policy()?,
            ExecutionLimits::new_job(120_000, 1024 * 1024).ok_or("limits")?,
            &NeverCancel,
        )
        .map_err(|error| format!("{name}: {error:?}"))?;
        std::fs::write(
            output.join(format!("native-workspace-{name}.stderr.jsonl")),
            &result.capture.stderr,
        )?;
        std::fs::write(
            output.join(format!("native-workspace-{name}.stdout")),
            &result.capture.stdout,
        )?;
        assert_eq!(result.capture.code, Some(expected_exit), "{name}");
        assert_eq!(result.metadata.packages.len(), 2, "{name}");
        assert_eq!(
            result
                .metadata
                .declared_licenses
                .iter()
                .map(Option::as_deref)
                .collect::<Vec<_>>(),
            [Some("MIT"), Some("MIT")],
            "{name}"
        );
        let parsed = deny_json::parse(
            &result.capture.stderr,
            &result.capture.stdout,
            expected_exit,
            result.capture.stdout_truncated || result.capture.stderr_truncated,
            &result.metadata.packages,
        )
        .map_err(|error| format!("{name} parse: {error:?}"))?;
        assert!(parsed.parse_complete, "{name}");
        assert_eq!(
            parsed.licenses,
            rust_engineering_domain::security::SecurityCounts {
                warnings: 2,
                helps: 2,
                ..Default::default()
            },
            "{name}"
        );
        assert_eq!(parsed.findings_omitted, 0, "{name}");
        if exact {
            assert_eq!(parsed.bans, Default::default(), "{name}");
            assert_eq!(parsed.findings.len(), 4, "{name}");
        } else {
            assert_eq!(
                parsed.bans,
                rust_engineering_domain::security::SecurityCounts {
                    errors: 1,
                    ..Default::default()
                },
                "{name}"
            );
            assert_eq!(parsed.findings.len(), 5, "{name}");
            assert_eq!(parsed.findings[0].rule, "wildcard", "{name}");
        }
        assert!(parsed.findings.iter().all(|finding| {
            finding.package.as_ref().is_some_and(|package| {
                package.version == "0.1.0" && matches!(package.name.as_str(), "app" | "helper")
            })
        }));
        println!(
            "M4_DENY_GRAPH {}",
            serde_json::json!({
                "case": name,
                "exit": expected_exit,
                "stderr_sha256": digest(&result.capture.stderr),
                "stdout_sha256": digest(&result.capture.stdout),
                "bans": parsed.bans,
                "licenses": parsed.licenses,
                "sources": parsed.sources,
                "findings": parsed.findings.len(),
                "parse_complete": parsed.parse_complete,
            })
        );
    }
    inventory_is_empty(&gateway)?;
    drop(gateway);
    std::fs::remove_dir_all(state_root)?;
    Ok(())
}
