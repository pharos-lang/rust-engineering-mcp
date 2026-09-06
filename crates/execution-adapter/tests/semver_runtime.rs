//! Docker-only M3-04 calibration and containment selections. The I04c window
//! deliberately does not execute these ignored tests.

use rust_engineering_application::{ExecutionCancellation, NeverCancel};
use rust_engineering_domain::semver_check::{SemverCommandOptions, SemverProjectSelection};
use rust_engineering_domain::{ExecutionLimits, ExecutionTermination, SourceBundle, SourceFile};
use rust_engineering_execution::{APPROVED_RUST_IMAGE, HostDockerConfig, RustGateway};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}

struct StateRoot(PathBuf);
impl StateRoot {
    fn new(label: &str) -> Result<Self> {
        let path = PathBuf::from("/private/tmp")
            .join(format!("rust-mcp-semver-{label}-{}", std::process::id()));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}
impl Drop for StateRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gateway(root: &StateRoot) -> Result<RustGateway> {
    let gateway = RustGateway::new(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
            .ok_or("RUST_MCP_TEST_SOCKET required")?
            .into(),
        state_root: root.0.clone(),
        image_id: std::env::var("RUST_MCP_TEST_IMAGE")
            .unwrap_or_else(|_| APPROVED_RUST_IMAGE.into()),
    })
    .map_err(|error| format!("{error:?}"))?;
    assert!(
        gateway
            .calibrate(&NeverCancel)
            .map_err(|error| format!("{error:?}"))?
            .verified
    );
    Ok(gateway)
}

fn source(root: &Path) -> Result<SourceBundle> {
    let mut stack = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            if entry.path().is_dir() {
                stack.push(entry.path());
            } else {
                let relative = entry
                    .path()
                    .strip_prefix(root)?
                    .to_str()
                    .ok_or("non UTF-8 fixture")?
                    .replace('\\', "/");
                files.push(checked(SourceFile::new(
                    relative,
                    std::fs::read(entry.path())?,
                ))?);
            }
        }
    }
    checked(SourceBundle::new(files))
}
fn pair(name: &str) -> Result<(SourceBundle, SourceBundle)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/semver/{name}"));
    Ok((
        source(&root.join("baseline"))?,
        source(&root.join("candidate"))?,
    ))
}
fn run(
    name: &str,
    selection: SemverProjectSelection,
) -> Result<rust_engineering_domain::ExecutionResult> {
    let root = StateRoot::new(name)?;
    let gateway = gateway(&root)?;
    let (baseline, candidate) = pair(name)?;
    let options = SemverCommandOptions::try_from(selection)?;
    let result = gateway
        .execute_semver(
            &baseline,
            &candidate,
            &options,
            ExecutionLimits::new_job(120_000, 512 * 1024).ok_or("invalid limits")?,
            &NeverCancel,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert!(!gateway.is_quarantined());
    eprintln!(
        "SEMVER_OBSERVATION fixture={name} termination={:?} exit={:?}\n--- stdout ---\n{}\n--- stderr ---\n{}\n--- end ---",
        result.termination, result.exit_code, result.stdout, result.stderr,
    );
    Ok(result)
}

macro_rules! exited_case {
    ($name:ident, $fixture:literal, $exit:expr) => {
        #[test]
        #[ignore = "requires approved Docker semver gateway; pending M3-04 calibration"]
        fn $name() -> Result {
            let result = run($fixture, SemverProjectSelection::default())?;
            assert_eq!(
                result.termination,
                ExecutionTermination::Exited,
                "{result:?}"
            );
            assert_eq!(result.exit_code, Some($exit), "{result:?}");
            Ok(())
        }
    };
}

#[test]
#[ignore = "requires approved Docker semver gateway for M3-02 budget measurement"]
fn identical_records_cold_and_warm_sync_samples() -> Result {
    let (baseline, candidate) = pair("identical")?;
    let options = SemverCommandOptions::try_from(SemverProjectSelection::default())?;
    let samples = std::env::var("RUST_MCP_M3_BUDGET_SAMPLES")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(5);
    if !(1..=100).contains(&samples) {
        return Err("RUST_MCP_M3_BUDGET_SAMPLES must be between 1 and 100".into());
    }
    let mut cold_ms = Vec::with_capacity(samples);
    let mut warm_ms = Vec::with_capacity(samples);
    let mut cold_command_ms = Vec::with_capacity(samples);
    let mut warm_command_ms = Vec::with_capacity(samples);
    let mut cold_gateway_ms = Vec::with_capacity(samples);
    let mut warm_gateway_ms = Vec::with_capacity(samples);
    for sample in 0..samples {
        let root = StateRoot::new(&format!("timing-{sample}"))?;
        let gateway = gateway(&root)?;
        let limits = ExecutionLimits::new_job(60_000, 512 * 1024).ok_or("invalid limits")?;
        let started = Instant::now();
        let cold = gateway
            .execute_semver(&baseline, &candidate, &options, limits, &NeverCancel)
            .map_err(|error| format!("{error:?}"))?;
        cold_ms.push(u64::try_from(started.elapsed().as_millis())?);
        cold_command_ms.push(cold.duration_ms);
        cold_gateway_ms.push(cold.total_duration_ms);
        assert_eq!(cold.termination, ExecutionTermination::Exited);
        assert_eq!(cold.exit_code, Some(0));

        let started = Instant::now();
        let warm = gateway
            .execute_semver(&baseline, &candidate, &options, limits, &NeverCancel)
            .map_err(|error| format!("{error:?}"))?;
        warm_ms.push(u64::try_from(started.elapsed().as_millis())?);
        warm_command_ms.push(warm.duration_ms);
        warm_gateway_ms.push(warm.total_duration_ms);
        assert_eq!(warm.termination, ExecutionTermination::Exited);
        assert_eq!(warm.exit_code, Some(0));
        assert!(!gateway.is_quarantined());
    }
    println!(
        "M3_SEMVER_SYNC_TIMINGS {{\"cold_ms\":{cold_ms:?},\"warm_ms\":{warm_ms:?},\"cold_command_ms\":{cold_command_ms:?},\"warm_command_ms\":{warm_command_ms:?},\"cold_gateway_ms\":{cold_gateway_ms:?},\"warm_gateway_ms\":{warm_gateway_ms:?},\"samples_each\":{samples}}}"
    );
    Ok(())
}

exited_case!(identical_libraries_exit_zero, "identical", 0);
exited_case!(
    removed_public_function_is_a_deny_level_break,
    "removed-pub-fn",
    100
);
exited_case!(
    trait_method_with_default_is_compatible,
    "trait-default-added",
    0
);
exited_case!(
    trait_method_without_default_is_a_break,
    "trait-required-added",
    100
);
exited_case!(
    non_exhaustive_enum_variant_addition_is_compatible,
    "enum-non-exhaustive",
    0
);
exited_case!(
    exhaustive_enum_variant_addition_is_a_break,
    "enum-exhaustive",
    100
);

#[test]
#[ignore = "requires approved Docker semver gateway; pending M3-04 calibration"]
fn feature_gated_removal_uses_the_identical_selection_on_both_sides() -> Result {
    let result = run(
        "feature-gated-removal",
        SemverProjectSelection {
            features: vec!["extra".into()],
            ..Default::default()
        },
    )?;
    assert_eq!(
        result.termination,
        ExecutionTermination::Exited,
        "{result:?}"
    );
    assert_eq!(result.exit_code, Some(100), "{result:?}");
    Ok(())
}

#[test]
#[ignore = "requires approved Docker semver gateway; pending M3-04 calibration"]
fn feature_gated_removal_without_feature_has_no_signal() -> Result {
    let result = run(
        "feature-gated-removal",
        SemverProjectSelection {
            no_default_features: true,
            ..Default::default()
        },
    )?;
    assert_eq!(
        result.termination,
        ExecutionTermination::Exited,
        "{result:?}"
    );
    assert_eq!(result.exit_code, Some(0), "{result:?}");
    Ok(())
}

exited_case!(
    no_lib_gateway_exit_is_calibrated_before_application_maps_unavailable,
    "no-lib",
    101
);
exited_case!(
    broken_baseline_is_incomplete_not_a_compatibility_pass,
    "broken-baseline",
    101
);
exited_case!(
    warn_level_only_findings_are_surfaced_under_exit_zero,
    "warn-only",
    0
);
exited_case!(
    exit_100_with_zero_parsed_findings_is_blocked,
    "removed-pub-fn",
    100
);

#[test]
#[ignore = "requires approved Docker semver gateway; pending M3-04 calibration"]
fn planted_git_directory_is_never_discovered() -> Result {
    let root = StateRoot::new("git")?;
    let gateway = gateway(&root)?;
    let options = SemverCommandOptions::try_from(SemverProjectSelection::default())?;
    let (baseline, candidate) = pair("removed-pub-fn")?;
    let clean = gateway
        .execute_semver(
            &baseline,
            &candidate,
            &options,
            ExecutionLimits::new_job(120_000, 512 * 1024).ok_or("limits")?,
            &NeverCancel,
        )
        .map_err(|error| format!("{error:?}"))?;
    for side in ["baseline", "candidate"] {
        let (mut baseline, mut candidate) = pair("removed-pub-fn")?;
        let selected = if side == "baseline" {
            &mut baseline
        } else {
            &mut candidate
        };
        let mut files = selected.files().to_vec();
        files.push(checked(SourceFile::new(
            ".git/config".into(),
            b"[remote \"origin\"]\nurl=https://example.invalid/repo\n".to_vec(),
        ))?);
        *selected = checked(SourceBundle::new(files))?;
        let result = gateway
            .execute_semver(
                &baseline,
                &candidate,
                &options,
                ExecutionLimits::new_job(120_000, 512 * 1024).ok_or("limits")?,
                &NeverCancel,
            )
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!(result.termination, clean.termination, "{side}: {result:?}");
        assert_eq!(result.exit_code, clean.exit_code, "{side}: {result:?}");
        assert!(
            !result.stderr.contains("example.invalid"),
            "{side}: {}",
            result.stderr
        );
    }
    Ok(())
}

#[test]
#[ignore = "requires approved Docker semver gateway; pending M3-04 calibration"]
fn registry_dependent_baseline_fails_recognizably_without_hanging() -> Result {
    let result = run("registry-required", SemverProjectSelection::default())?;
    assert!(result.total_duration_ms < 120_000);
    assert_ne!(result.exit_code, Some(0), "{result:?}");
    Ok(())
}

struct CancelAfter {
    started: Instant,
    cancelled: AtomicBool,
}
impl ExecutionCancellation for CancelAfter {
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire) || self.started.elapsed() >= Duration::from_secs(2)
    }
}

fn slow_source(version: &str) -> Result<SourceBundle> {
    let files = vec![
        checked(SourceFile::new("Cargo.toml".into(), format!("[package]\nname='semver-slow'\nversion='{version}'\nedition='2024'\nbuild='build.rs'\n").into_bytes()))?,
        checked(SourceFile::new(
            "Cargo.lock".into(),
            format!(
                "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"semver-slow\"\nversion = \"{version}\"\n"
            )
            .into_bytes(),
        ))?,
        checked(SourceFile::new("build.rs".into(), b"fn main(){std::thread::sleep(std::time::Duration::from_secs(30));}".to_vec()))?,
        checked(SourceFile::new("src/lib.rs".into(), b"pub fn api() {}".to_vec()))?,
    ];
    checked(SourceBundle::new(files))
}

#[test]
#[ignore = "requires approved Docker semver gateway; pending M3-04 calibration"]
fn cancellation_or_eof_with_active_child_is_joined_before_return() -> Result {
    let root = StateRoot::new("cancel")?;
    let gateway = gateway(&root)?;
    let baseline = slow_source("1.0.0")?;
    let candidate = slow_source("1.0.0")?;
    let options = SemverCommandOptions::try_from(SemverProjectSelection::default())?;
    let cancel = CancelAfter {
        started: Instant::now(),
        cancelled: AtomicBool::new(false),
    };
    let result = gateway
        .execute_semver(
            &baseline,
            &candidate,
            &options,
            ExecutionLimits::new_job(120_000, 512 * 1024).ok_or("limits")?,
            &cancel,
        )
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(
        result.termination,
        ExecutionTermination::Cancelled,
        "{result:?}"
    );
    assert!(!gateway.is_quarantined());
    Ok(())
}

#[test]
#[ignore = "requires approved Docker semver gateway; pending M3-04 calibration"]
fn baseline_and_candidate_roots_are_immutable_after_run() -> Result {
    let before = pair("removed-pub-fn")?;
    let _ = run("removed-pub-fn", SemverProjectSelection::default())?;
    let after = pair("removed-pub-fn")?;
    assert_eq!(before, after);
    Ok(())
}
