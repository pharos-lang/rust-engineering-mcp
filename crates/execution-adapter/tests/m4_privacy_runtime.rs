//! Host-only canary values are never included in the captured fixture source.
use rust_engineering_application::NeverCancel;
use rust_engineering_domain::coverage::{CoverageOptions, CoverageSelection};
use rust_engineering_domain::mutation_test::{MutationTestCommandOptions, MutationTestSelection};
use rust_engineering_domain::{ExecutionLimits, SourceBundle, SourceFile};
use rust_engineering_execution::{APPROVED_M4_IMAGE, HostDockerConfig, RustGateway};
use std::path::PathBuf;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> TestResult<T> {
    value.map_err(|error| format!("{error:?}").into())
}
struct PrivateRoot(PathBuf);
impl Drop for PrivateRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}
fn tar_diff_contains(bytes: &[u8], needle: &[u8]) -> bool {
    let mut offset = 0usize;
    while offset + 512 <= bytes.len() && bytes[offset..offset + 512].iter().any(|b| *b != 0) {
        let header = &bytes[offset..offset + 512];
        let name = String::from_utf8_lossy(&header[..100]);
        let size = String::from_utf8_lossy(&header[124..136]);
        let Ok(size) = usize::from_str_radix(size.trim_matches(['\0', ' ']), 8) else {
            return false;
        };
        let Some(body_start) = offset.checked_add(512) else {
            return false;
        };
        let Some(body_end) = body_start.checked_add(size) else {
            return false;
        };
        if name.starts_with("./diff/")
            && header[156] == b'0'
            && let Some(body) = bytes.get(body_start..body_end)
            && contains(body, needle)
        {
            return true;
        }
        let Some(next) = size
            .div_ceil(512)
            .checked_mul(512)
            .and_then(|padding| body_start.checked_add(padding))
            .filter(|next| *next <= bytes.len())
        else {
            return false;
        };
        offset = next;
    }
    false
}

#[test]
#[ignore = "explicit M4 Docker image; canaries in coverage HTML, mutation diffs and diagnostics"]
fn host_canary_cannot_enter_html_diffs_logs_or_diagnostics() -> TestResult {
    let mut entropy = [0u8; 16];
    checked(getrandom::fill(&mut entropy))?;
    let nonce = entropy
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let root =
        PrivateRoot(PathBuf::from("/private/tmp").join(format!("rust-mcp-m4-privacy-{nonce}")));
    std::fs::create_dir(&root.0)?;
    let private_path = root.0.join("private-token");
    let canary = format!("M4_HOST_ONLY_SECRET_{nonce}");
    let project_canary = "M4_AUTHORIZED_SOURCE_CANARY_d2e480";
    std::fs::write(&private_path, canary.as_bytes())?;
    let state = root.0.join("state");
    std::fs::create_dir(&state)?;
    let gateway = checked(RustGateway::new(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
            .ok_or("explicit socket required")?
            .into(),
        state_root: state,
        image_id: APPROVED_M4_IMAGE.into(),
    }))?;
    assert!(checked(gateway.calibrate(&NeverCancel))?.verified);
    // Only the inaccessible path crosses the source boundary; the token value
    // exists solely in the host file and in this test's assertion memory.
    let code = format!(
        "pub fn flag() -> bool {{ /* {project_canary} */ true }}\n#[cfg(test)] mod tests {{ #[test] fn controls() {{ let secret=std::fs::read({:?}); if let Ok(bytes)=&secret {{ println!(\"{{}}\",String::from_utf8_lossy(bytes)); }} assert!(secret.is_err()); assert!(super::flag()); }} }}\n",
        private_path.to_str().ok_or("path")?
    );
    assert!(!code.contains(&canary));
    let source = checked(SourceBundle::new(vec![
        checked(SourceFile::new(
            "Cargo.toml".into(),
            b"[package]\nname='m4_privacy'\nversion='0.1.0'\nedition='2024'\n".to_vec(),
        ))?,
        checked(SourceFile::new(
            "Cargo.lock".into(),
            b"version=4\n[[package]]\nname='m4_privacy'\nversion='0.1.0'\n".to_vec(),
        ))?,
        checked(SourceFile::new("src/lib.rs".into(), code.into_bytes()))?,
    ]))?;
    let coverage = checked(gateway.execute_coverage(
        &source,
        &CoverageOptions::try_from(CoverageSelection::default())?,
        ExecutionLimits::new_job(120_000, 256 * 1024).ok_or("limits")?,
        &NeverCancel,
    ))?;
    assert_eq!(coverage.result.exit_code, Some(0), "{:?}", coverage.result);
    let html = coverage.html.ok_or("HTML artifact absent")?;
    assert!(html.len().is_multiple_of(512));
    // Positive contamination control: M3 intentionally retains authorized source
    // in opaque HTML/diffs. This proves we inspect actual source-bearing bytes;
    // it is not a claim that arbitrary source secrets are detected or removed.
    assert!(contains(&html, project_canary.as_bytes()));
    for bytes in [
        html.as_slice(),
        coverage.json.as_deref().ok_or("JSON")?,
        coverage.lcov.as_deref().ok_or("LCOV")?,
        coverage.result.stdout.as_bytes(),
        coverage.result.stderr.as_bytes(),
    ] {
        assert!(
            !contains(bytes, canary.as_bytes()),
            "host canary leaked into coverage evidence"
        );
    }
    let mutation = checked(gateway.execute_mutation_test(
        &source,
        &MutationTestCommandOptions::try_from(MutationTestSelection {
            max_mutants: 20,
            mutant_timeout_seconds: 10,
            ..Default::default()
        })?,
        ExecutionLimits::new_job(180_000, 256 * 1024).ok_or("limits")?,
        &NeverCancel,
    ))?;
    let bundle = mutation.bundle.ok_or("mutation artifact absent")?;
    assert!(
        tar_diff_contains(&bundle, project_canary.as_bytes()),
        "the test must inspect the source canary in an actual generated diff"
    );
    for bytes in [
        bundle.as_slice(),
        mutation.result.stdout.as_bytes(),
        mutation.result.stderr.as_bytes(),
    ] {
        assert!(
            !contains(bytes, canary.as_bytes()),
            "host canary leaked into mutation evidence"
        );
    }
    assert_eq!(std::fs::read(&private_path)?, canary.as_bytes());
    assert!(!gateway.is_quarantined());
    for kind in ["container", "volume"] {
        let output =
            std::process::Command::new("/Applications/Docker.app/Contents/Resources/bin/docker")
                .env_clear()
                .args([
                    "--host",
                    &format!("unix://{}", std::env::var("RUST_MCP_TEST_SOCKET")?),
                    kind,
                    "ls",
                ])
                .args(if kind == "container" {
                    vec!["--all"]
                } else {
                    vec![]
                })
                .args(["--filter=label=org.rust-mcp.execution=true", "--quiet"])
                .output()?;
        assert!(output.status.success());
        assert!(
            output.stdout.is_empty(),
            "owned {kind} remained after cleanup"
        );
    }
    let receipt = serde_json::json!({"image_id":APPROVED_M4_IMAGE,"host_canary_not_in_captured_source":true,"host_read_attempt_denied":true,"coverage_html_bytes":html.len(),"mutation_bundle_bytes":bundle.len(),"actual_diff_present":true,"host_canary_absent_from_html_json_lcov_diff_and_process_streams":true,"authorized_source_canary_present_in_html_and_diff_bundle":true,"universal_source_secret_redaction":false,"cleanup_joined":true});
    std::fs::write(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/m4-privacy-runtime.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok(())
}
