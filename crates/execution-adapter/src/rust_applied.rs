//! Verify the daemon's applied configuration before starting the guest.
use super::mutation_gateway::{MutationPhase, MutationVolume};
use super::rust_gateway::{Phase, Volume};
use rust_engineering_application::ExecutionError;
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Created {
    config: Config,
    host_config: HostConfig,
    #[serde(default)]
    mounts: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Config {
    tty: bool,
    open_stdin: bool,
    attach_stdin: bool,
    stdin_once: bool,
    user: String,
    labels: BTreeMap<String, String>,
    env: Vec<String>,
    entrypoint: Vec<String>,
    cmd: Vec<String>,
    working_dir: String,
    image: String,
    volumes: Option<BTreeMap<String, serde_json::Value>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct HostConfig {
    auto_remove: bool,
    group_add: Option<Vec<String>>,
    #[serde(rename = "UTSMode")]
    uts_mode: String,
    oom_kill_disable: Option<bool>,
    oom_score_adj: i64,
    device_cgroup_rules: Option<Vec<String>>,
    storage_opt: Option<BTreeMap<String, String>>,
    annotations: Option<BTreeMap<String, String>>,
    readonly_rootfs: bool,
    runtime: String,
    init: Option<bool>,
    masked_paths: Vec<String>,
    readonly_paths: Vec<String>,
    userns_mode: String,
    cgroup_parent: String,
    sysctls: Option<BTreeMap<String, String>>,
    ulimits: Option<Vec<serde_json::Value>>,
    network_mode: String,
    pid_mode: String,
    ipc_mode: String,
    cgroupns_mode: String,
    cap_drop: Vec<String>,
    cap_add: Option<Vec<String>>,
    volumes_from: Option<Vec<String>>,
    security_opt: Vec<String>,
    pids_limit: i64,
    nano_cpus: i64,
    memory: i64,
    memory_swap: i64,
    shm_size: i64,
    privileged: bool,
    binds: Option<Vec<String>>,
    tmpfs: BTreeMap<String, String>,
    log_config: LogConfig,
    #[serde(default)]
    mounts: Vec<serde_json::Value>,
    #[serde(default)]
    devices: Vec<serde_json::Value>,
    device_requests: Option<Vec<serde_json::Value>>,
    publish_all_ports: bool,
    port_bindings: BTreeMap<String, serde_json::Value>,
    restart_policy: Restart,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct LogConfig {
    #[serde(rename = "Type")]
    kind: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Restart {
    name: String,
}

/// Authority every applied container refuses, whichever phase it runs.
///
/// The daemon is asked for a container with no host namespaces, no added
/// groups, no OOM or cgroup tuning, no sysctls, no ulimits and the fixed masked
/// and read-only proc paths, under runc without its init. `interactive` is the
/// one shape that varies: only an ingesting or interactive phase opens stdin.
/// The two labels bind the container to this product and to one job nonce, so a
/// container created by anything else is never adopted.
fn no_host_authority(c: &Created, nonce: &str, interactive: bool) -> bool {
    let h = &c.host_config;
    !c.config.tty
        && c.config.open_stdin == interactive
        && c.config.attach_stdin == interactive
        && c.config.stdin_once == interactive
        && c.config.labels
            == BTreeMap::from([
                ("org.rust-mcp.execution".into(), "true".into()),
                ("org.rust-mcp.rust-job".into(), nonce.into()),
            ])
        && !h.auto_remove
        && h.group_add.as_ref().is_none_or(Vec::is_empty)
        && h.uts_mode.is_empty()
        && h.oom_kill_disable != Some(true)
        && h.oom_score_adj == 0
        && h.device_cgroup_rules.as_ref().is_none_or(Vec::is_empty)
        && h.storage_opt.as_ref().is_none_or(BTreeMap::is_empty)
        && h.annotations.as_ref().is_none_or(BTreeMap::is_empty)
        && h.runtime == "runc"
        && h.init != Some(true)
        && h.userns_mode.is_empty()
        && h.cgroup_parent.is_empty()
        && h.sysctls.as_ref().is_none_or(BTreeMap::is_empty)
        && h.ulimits.as_ref().is_none_or(Vec::is_empty)
        && h.masked_paths
            == [
                "/proc/acpi",
                "/proc/asound",
                "/proc/interrupts",
                "/proc/kcore",
                "/proc/keys",
                "/proc/latency_stats",
                "/proc/sched_debug",
                "/proc/scsi",
                "/proc/timer_list",
                "/proc/timer_stats",
                "/sys/devices/virtual/powercap",
                "/sys/firmware",
            ]
        && h.readonly_paths
            == [
                "/proc/bus",
                "/proc/fs",
                "/proc/irq",
                "/proc/sys",
                "/proc/sysrq-trigger",
            ]
}

/// Limits every applied container carries, whichever phase it runs.
///
/// No capabilities, devices, host binds, published ports, restart policy or
/// logging; a read-only rootfs on a private network, IPC and cgroup namespace;
/// the fixed CPU, memory, PID and shared-memory ceilings; and the two private
/// tmpfs mounts the guest writes into. The image and working directory are the
/// approved ones. Each caller still verifies its own seccomp profile, mounts,
/// extra tmpfs, guest user, entrypoint, arguments and environment.
fn applied_limits_ok(c: &Created, image: &str) -> bool {
    let h = &c.host_config;
    c.config.volumes.as_ref().is_none_or(BTreeMap::is_empty)
        && h.cap_add.as_ref().is_none_or(Vec::is_empty)
        && h.volumes_from.as_ref().is_none_or(Vec::is_empty)
        && h.readonly_rootfs
        && h.network_mode == "none"
        && h.pid_mode.is_empty()
        && h.ipc_mode == "private"
        && h.cgroupns_mode == "private"
        && h.cap_drop == ["ALL"]
        && h.security_opt.len() == 2
        && h.security_opt
            .iter()
            .any(|value| value == "no-new-privileges=true" || value == "no-new-privileges")
        && h.pids_limit == 128
        && h.nano_cpus == 1_000_000_000
        && h.memory == 1_073_741_824
        && h.memory_swap == 1_073_741_824
        && h.shm_size == 1_048_576
        && !h.privileged
        && h.binds.as_ref().is_none_or(Vec::is_empty)
        && h.devices.is_empty()
        && h.device_requests.as_ref().is_none_or(Vec::is_empty)
        && !h.publish_all_ports
        && h.port_bindings.is_empty()
        && h.restart_policy.name == "no"
        && h.log_config.kind == "none"
        && h.tmpfs
            .get("/work")
            .is_some_and(|value| value == "rw,exec,nosuid,nodev,size=512m,mode=1777")
        && h.tmpfs
            .get("/tmp")
            .is_some_and(|value| value == "rw,nosuid,nodev,noexec,size=64m,mode=1777")
        && c.config.working_dir == "/source"
        && c.config.image == image
}

/// The single applied seccomp profile must parse to exactly the phase's own.
fn applied_profile_ok(c: &Created, expected: &str) -> Result<bool, ExecutionError> {
    let profile: serde_json::Value =
        serde_json::from_str(expected).map_err(|_| ExecutionError::Infrastructure)?;
    let applied = c
        .host_config
        .security_opt
        .iter()
        .filter_map(|value| value.strip_prefix("seccomp="))
        .collect::<Vec<_>>();
    Ok(applied.len() == 1
        && serde_json::from_str::<serde_json::Value>(applied[0])
            .is_ok_and(|value| value == profile))
}

/// The daemon answered a create with exactly one container or it is not the one
/// this gateway asked for.
fn only_created(bytes: &[u8]) -> Result<Created, ExecutionError> {
    let containers: Vec<Created> =
        serde_json::from_slice(bytes).map_err(|_| ExecutionError::Infrastructure)?;
    let mut containers = containers.into_iter();
    match (containers.next(), containers.next()) {
        (Some(created), None) => Ok(created),
        _ => Err(ExecutionError::Infrastructure),
    }
}

/// Environment equality is order-insensitive; the applied order is the daemon's.
fn sorted_env(c: &Created) -> Vec<String> {
    let mut env = c.config.env.clone();
    env.sort();
    env
}

pub(super) fn verify(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    nonce: &str,
) -> Result<(), ExecutionError> {
    verify_rust(bytes, image, phase, volume, None, nonce)
}

pub(super) fn verify_nextest(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    junit: &MutationVolume,
    junit_writable: bool,
    nonce: &str,
) -> Result<(), ExecutionError> {
    verify_rust(
        bytes,
        image,
        phase,
        volume,
        Some((junit, junit_writable)),
        nonce,
    )
}

/// Coverage uses the same bounded local-driver output-volume mechanism as
/// nextest, but mounts it at the fixed guest path `/work/coverage`.
pub(super) fn verify_coverage(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    output: &MutationVolume,
    target: &MutationVolume,
    nonce: &str,
) -> Result<(), ExecutionError> {
    let output_writable = !matches!(
        phase,
        Phase::ExportCoverageJson | Phase::ExportCoverageLcov | Phase::ExportCoverageHtml
    );
    if !named_tmpfs_volume_is_exact(output, super::mutation_gateway::VOLUME_OPTIONS, nonce)
        || !named_tmpfs_volume_is_exact(
            target,
            super::coverage_gateway::COVERAGE_TARGET_VOLUME_OPTIONS,
            nonce,
        )
    {
        return Err(ExecutionError::InvalidConfiguration);
    }
    verify_rust_generic(
        bytes,
        image,
        phase,
        volume,
        Some((output, output_writable)),
        Some("/work/coverage"),
        None,
        phase
            .coverage_target_writable()
            .map(|writable| (target, writable)),
        nonce,
    )
}

fn named_tmpfs_volume_is_exact(volume: &MutationVolume, options: &str, nonce: &str) -> bool {
    volume.driver == "local"
        && volume.scope == "local"
        && volume.options
            == BTreeMap::from([
                ("device".into(), "tmpfs".into()),
                ("o".into(), options.into()),
                ("type".into(), "tmpfs".into()),
            ])
        && volume.labels == super::rust_gateway::labels(nonce)
        && volume.mountpoint.starts_with("/var/lib/docker/volumes/")
        && volume.mountpoint.ends_with("/_data")
        && volume.cluster_volume.is_none()
        && volume.status.is_none()
}

/// M3-05 uses the same bounded local-driver output-volume mechanism as nextest
/// and coverage, mounted at the fixed guest path `/mutants`. The verifier also
/// covers the extra private scratch tmpfs through `Phase::extra_tmpfs`, so a
/// daemon that dropped, resized or relaxed that mount fails closed before any
/// project code runs.
pub(super) fn verify_mutation_test(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    output: &MutationVolume,
    writable: bool,
    nonce: &str,
) -> Result<(), ExecutionError> {
    verify_rust_output(
        bytes,
        image,
        phase,
        volume,
        output,
        writable,
        super::mutation_test_gateway::MUTATION_OUTPUT_TARGET,
        nonce,
    )
}

// The concurrent M3 semver adapter calls this once its tool vertical is
// registered; I02b keeps the prepared boundary compiling without advertising it.
#[allow(dead_code)]
pub(super) fn verify_semver(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    baseline: &Volume,
    nonce: &str,
) -> Result<(), ExecutionError> {
    verify_rust_generic(
        bytes,
        image,
        phase,
        volume,
        None,
        None,
        Some(baseline),
        None,
        nonce,
    )
}

fn verify_rust(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    junit: Option<(&MutationVolume, bool)>,
    nonce: &str,
) -> Result<(), ExecutionError> {
    verify_rust_generic(
        bytes,
        image,
        phase,
        volume,
        junit,
        Some("/junit"),
        None,
        None,
        nonce,
    )
}

#[allow(clippy::too_many_arguments)] // One explicit argument per applied mount invariant.
fn verify_rust_output(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    output: &MutationVolume,
    writable: bool,
    target: &str,
    nonce: &str,
) -> Result<(), ExecutionError> {
    verify_rust_generic(
        bytes,
        image,
        phase,
        volume,
        Some((output, writable)),
        Some(target),
        None,
        None,
        nonce,
    )
}

#[allow(clippy::too_many_arguments)] // One explicit argument per applied mount invariant.
fn verify_rust_generic(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    junit: Option<(&MutationVolume, bool)>,
    output_target: Option<&str>,
    baseline: Option<&Volume>,
    coverage_target: Option<(&MutationVolume, bool)>,
    nonce: &str,
) -> Result<(), ExecutionError> {
    let c = only_created(bytes)?;
    let c = &c;
    let h = &c.host_config;
    let profile_ok = applied_profile_ok(c, phase.seccomp_profile_json())?;
    let safe = no_host_authority(c, nonce, phase.ingesting())
        && mounts_ok(
            c,
            phase,
            volume,
            junit,
            output_target,
            baseline,
            coverage_target,
        )?
        && applied_limits_ok(c, image)
        && profile_ok
        && h.tmpfs.len() == 2 + usize::from(phase.extra_tmpfs().is_some())
        // The mutation phases add exactly one further private tmpfs, with the
        // exact ADR-053 staging profile, for the writable copy of the tree.
        && phase.extra_tmpfs().is_none_or(|(path, options)| {
            h.tmpfs.get(path).is_some_and(|applied| applied == options)
        })
        && c.config.user == phase.user()
        && c.config.entrypoint == [phase.program()]
        && c.config.cmd == phase.arguments()
        && sorted_env(c) == phase.environment();
    if safe {
        Ok(())
    } else {
        Err(ExecutionError::InvalidConfiguration)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct MountOptions {
    no_copy: bool,
    #[serde(default)]
    labels: BTreeMap<String, String>,
    driver_config: Driver,
    #[serde(default)]
    subpath: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct Driver {
    name: String,
    #[serde(default)]
    options: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct RequestedMount {
    #[serde(rename = "Type")]
    kind: String,
    source: String,
    target: String,
    #[serde(default)]
    read_only: bool,
    volume_options: MountOptions,
}
#[derive(Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
struct AppliedMount {
    #[serde(rename = "Type")]
    kind: String,
    name: String,
    source: String,
    destination: String,
    driver: String,
    mode: String,
    #[serde(rename = "RW")]
    rw: bool,
    propagation: String,
}
fn mounts_ok(
    c: &Created,
    phase: &Phase,
    volume: &Volume,
    junit: Option<(&MutationVolume, bool)>,
    output_target: Option<&str>,
    baseline: Option<&Volume>,
    coverage_target: Option<(&MutationVolume, bool)>,
) -> Result<bool, ExecutionError> {
    let expected = usize::from(junit.is_some())
        + usize::from(baseline.is_some())
        + usize::from(coverage_target.is_some())
        + 1;
    if c.mounts.len() != expected || c.host_config.mounts.len() != expected {
        return Ok(false);
    }
    let applied = c
        .mounts
        .iter()
        .cloned()
        .map(serde_json::from_value::<AppliedMount>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    let requested = c
        .host_config
        .mounts
        .iter()
        .cloned()
        .map(serde_json::from_value::<RequestedMount>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    let source_target = if matches!(phase, Phase::IngestBaseline) {
        "/baseline"
    } else {
        "/source"
    };
    let a = applied
        .iter()
        .find(|mount| mount.destination == source_target)
        .ok_or(ExecutionError::InvalidConfiguration)?;
    let r = requested
        .iter()
        .find(|mount| mount.target == source_target)
        .ok_or(ExecutionError::InvalidConfiguration)?;
    let source_ok = a.kind == "volume"
        && a.name == volume.name
        && a.source == volume.mountpoint
        && a.destination == source_target
        && a.driver == "local"
        && a.mode == "z"
        && a.propagation.is_empty()
        && a.rw == phase.ingesting()
        && r.kind == "volume"
        && r.source == volume.name
        && r.target == source_target
        && r.read_only != phase.ingesting()
        && r.volume_options.no_copy
        && r.volume_options.subpath.is_empty()
        && r.volume_options.labels.is_empty()
        && r.volume_options.driver_config.name == "local"
        && r.volume_options.driver_config.options.is_empty();
    if !source_ok {
        return Ok(false);
    }
    if let Some((junit, writable)) = junit {
        let target = output_target.ok_or(ExecutionError::InvalidConfiguration)?;
        let a = applied
            .iter()
            .find(|mount| mount.destination == target)
            .ok_or(ExecutionError::InvalidConfiguration)?;
        let r = requested
            .iter()
            .find(|mount| mount.target == target)
            .ok_or(ExecutionError::InvalidConfiguration)?;
        let junit_ok = a.kind == "volume"
            && a.name == junit.name
            && a.source == junit.mountpoint
            && a.destination == target
            && a.driver == "local"
            && a.mode == "z"
            && a.propagation.is_empty()
            && a.rw == writable
            && r.kind == "volume"
            && r.source == junit.name
            && r.target == target
            && r.read_only != writable
            && r.volume_options.no_copy
            && r.volume_options.subpath.is_empty()
            && r.volume_options.labels.is_empty()
            && r.volume_options.driver_config.name == "local"
            && r.volume_options.driver_config.options.is_empty();
        if !junit_ok {
            return Ok(false);
        }
    }
    if let Some((target, writable)) = coverage_target {
        let destination = super::coverage_gateway::COVERAGE_TARGET_PATH;
        let a = applied
            .iter()
            .find(|mount| mount.destination == destination)
            .ok_or(ExecutionError::InvalidConfiguration)?;
        let r = requested
            .iter()
            .find(|mount| mount.target == destination)
            .ok_or(ExecutionError::InvalidConfiguration)?;
        let target_ok = a.kind == "volume"
            && a.name == target.name
            && a.source == target.mountpoint
            && a.destination == destination
            && a.driver == "local"
            && a.mode == "z"
            && a.propagation.is_empty()
            && a.rw == writable
            && r.kind == "volume"
            && r.source == target.name
            && r.target == destination
            && r.read_only != writable
            && r.volume_options.no_copy
            && r.volume_options.subpath.is_empty()
            && r.volume_options.labels.is_empty()
            && r.volume_options.driver_config.name == "local"
            && r.volume_options.driver_config.options.is_empty();
        if !target_ok {
            return Ok(false);
        }
    }
    let Some(baseline) = baseline else {
        return Ok(true);
    };
    let a = applied
        .iter()
        .find(|mount| mount.destination == "/baseline")
        .ok_or(ExecutionError::InvalidConfiguration)?;
    let r = requested
        .iter()
        .find(|mount| mount.target == "/baseline")
        .ok_or(ExecutionError::InvalidConfiguration)?;
    Ok(a.kind == "volume"
        && a.name == baseline.name
        && a.source == baseline.mountpoint
        && a.destination == "/baseline"
        && a.driver == "local"
        && a.mode == "z"
        && a.propagation.is_empty()
        && !a.rw
        && r.kind == "volume"
        && r.source == baseline.name
        && r.target == "/baseline"
        && r.read_only
        && r.volume_options.no_copy
        && r.volume_options.subpath.is_empty()
        && r.volume_options.labels.is_empty()
        && r.volume_options.driver_config.name == "local"
        && r.volume_options.driver_config.options.is_empty())
}

/// Verify every security-sensitive field for an ADR-053 phase. This stays
/// separate from the M1 verifier because the volume driver options and source
/// access matrix are intentionally different.
pub(super) fn verify_mutation(
    bytes: &[u8],
    image: &str,
    phase: MutationPhase,
    volume: &MutationVolume,
    nonce: &str,
) -> Result<(), ExecutionError> {
    let c = only_created(bytes)?;
    let c = &c;
    let h = &c.host_config;
    let profile_ok = applied_profile_ok(
        c,
        if phase == MutationPhase::Fix {
            include_str!("seccomp-rust-fix.json")
        } else {
            include_str!("seccomp-rust.json")
        },
    )?;
    let safe = no_host_authority(c, nonce, phase.interactive())
        && mutation_mounts_ok(c, phase, volume)?
        && applied_limits_ok(c, image)
        && profile_ok
        && h.tmpfs.len() == if phase == MutationPhase::Fix { 3 } else { 2 }
        && (phase != MutationPhase::Fix
            || h.tmpfs.get("/target").is_some_and(|value| {
                value == "rw,exec,nosuid,nodev,size=256m,mode=0700,uid=65534,gid=65534"
            }))
        && c.config.user == "65534:65534"
        && c.config.entrypoint == [phase.program()]
        && c.config.cmd == phase.arguments()
        && sorted_env(c) == super::rust_gateway::environment();
    if safe {
        Ok(())
    } else {
        Err(ExecutionError::InvalidConfiguration)
    }
}

fn mutation_mounts_ok(
    c: &Created,
    phase: MutationPhase,
    volume: &MutationVolume,
) -> Result<bool, ExecutionError> {
    if c.mounts.len() != 1 || c.host_config.mounts.len() != 1 {
        return Ok(false);
    }
    let applied: AppliedMount = serde_json::from_value(c.mounts[0].clone())
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    let requested: RequestedMount = serde_json::from_value(c.host_config.mounts[0].clone())
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    Ok(applied.kind == "volume"
        && applied.name == volume.name
        && applied.source == volume.mountpoint
        && applied.destination == "/source"
        && applied.driver == "local"
        && applied.mode == "z"
        && applied.propagation.is_empty()
        && applied.rw == phase.writable()
        && requested.kind == "volume"
        && requested.source == volume.name
        && requested.target == "/source"
        && requested.read_only != phase.writable()
        && requested.volume_options.no_copy
        && requested.volume_options.subpath.is_empty()
        && requested.volume_options.labels.is_empty()
        && requested.volume_options.driver_config.name == "local"
        && requested.volume_options.driver_config.options.is_empty())
}

fn resolution_mounts_ok(
    c: &Created,
    phase: super::resolution_gateway::ResolutionPhase,
    source: &MutationVolume,
    vendor: &MutationVolume,
) -> Result<bool, ExecutionError> {
    let mut expected = Vec::new();
    if phase.source_mounted() {
        expected.push(("/source", source, phase.source_writable()));
    }
    if phase.vendor_mounted() {
        expected.push(("/rust-mcp-vendor", vendor, phase.vendor_writable()));
    }
    if c.mounts.len() != expected.len() || c.host_config.mounts.len() != expected.len() {
        return Ok(false);
    }
    let applied = c
        .mounts
        .iter()
        .cloned()
        .map(serde_json::from_value::<AppliedMount>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    let requested = c
        .host_config
        .mounts
        .iter()
        .cloned()
        .map(serde_json::from_value::<RequestedMount>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    for (target, volume, writable) in expected {
        let Some(a) = applied.iter().find(|mount| mount.destination == target) else {
            return Ok(false);
        };
        let Some(r) = requested.iter().find(|mount| mount.target == target) else {
            return Ok(false);
        };
        if a.kind != "volume"
            || a.name != volume.name
            || a.source != volume.mountpoint
            || a.driver != "local"
            || a.mode != "z"
            || !a.propagation.is_empty()
            || a.rw != writable
            || r.kind != "volume"
            || r.source != volume.name
            || r.read_only == writable
            || !r.volume_options.no_copy
            || !r.volume_options.subpath.is_empty()
            || !r.volume_options.labels.is_empty()
            || r.volume_options.driver_config.name != "local"
            || !r.volume_options.driver_config.options.is_empty()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn verify_resolution(
    bytes: &[u8],
    image: &str,
    phase: super::resolution_gateway::ResolutionPhase,
    source: &MutationVolume,
    vendor: &MutationVolume,
    nonce: &str,
) -> Result<(), ExecutionError> {
    let c = only_created(bytes)?;
    let c = &c;
    let h = &c.host_config;
    let profile_ok = applied_profile_ok(c, include_str!("seccomp-rust.json"))?;
    let safe = no_host_authority(c, nonce, phase.interactive())
        && resolution_mounts_ok(c, phase, source, vendor)?
        && applied_limits_ok(c, image)
        && profile_ok
        && h.tmpfs.len() == 2
        && c.config.user == "65534:65534"
        && c.config.entrypoint == [phase.program()]
        && c.config.cmd == phase.arguments()
        && sorted_env(c) == super::rust_gateway::environment();
    if safe {
        Ok(())
    } else {
        Err(ExecutionError::InvalidConfiguration)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolution_gateway::ResolutionPhase;
    use rust_engineering_domain::RustCommand;
    use serde_json::{Value, json};

    #[test]
    fn fix_profile_only_adds_the_qualified_loopback_syscalls()
    -> Result<(), Box<dyn std::error::Error>> {
        let base: Value = serde_json::from_str(include_str!("seccomp-rust.json"))?;
        let fix: Value = serde_json::from_str(include_str!("seccomp-rust-fix.json"))?;
        for key in ["defaultAction", "defaultErrnoRet", "archMap"] {
            assert_eq!(fix[key], base[key], "changed {key}");
        }
        let base_rules = base["syscalls"].as_array().ok_or("base rules")?;
        let fix_rules = fix["syscalls"].as_array().ok_or("fix rules")?;
        assert_eq!(&fix_rules[..base_rules.len()], base_rules);
        assert_eq!(fix_rules.len(), base_rules.len() + 2);
        assert_eq!(fix_rules[base_rules.len()]["names"], json!(["socket"]));
        assert_eq!(
            fix_rules[base_rules.len()]["args"],
            json!([
                {"index":0,"value":2,"op":"SCMP_CMP_EQ"},
                {"index":1,"value":15,"valueTwo":1,"op":"SCMP_CMP_MASKED_EQ"},
                {"index":2,"value":0,"op":"SCMP_CMP_EQ"}
            ])
        );
        assert_eq!(
            fix_rules[base_rules.len() + 1]["names"],
            json!([
                "bind",
                "connect",
                "listen",
                "accept4",
                "getsockname",
                "setsockopt",
                "shutdown"
            ])
        );
        Ok(())
    }

    #[test]
    fn quality_profile_only_adds_af_unix_stream_socketpair()
    -> Result<(), Box<dyn std::error::Error>> {
        let base_bytes = include_bytes!("seccomp-rust.json");
        let base: Value = serde_json::from_slice(base_bytes)?;
        let quality: Value = serde_json::from_str(include_str!("seccomp-rust-quality.json"))?;
        for key in ["defaultAction", "defaultErrnoRet", "archMap"] {
            assert_eq!(quality[key], base[key], "changed {key}");
        }
        let base_rules = base["syscalls"].as_array().ok_or("base rules")?;
        let quality_rules = quality["syscalls"].as_array().ok_or("quality rules")?;
        assert_eq!(&quality_rules[..base_rules.len()], base_rules);
        assert_eq!(quality_rules.len(), base_rules.len() + 1);
        assert_eq!(
            quality_rules[base_rules.len()]["names"],
            json!(["socketpair"])
        );
        assert_eq!(
            quality_rules[base_rules.len()]["args"],
            json!([
                {"index":0,"value":1,"op":"SCMP_CMP_EQ"},
                {"index":1,"value":15,"valueTwo":1,"op":"SCMP_CMP_MASKED_EQ"},
                {"index":2,"value":0,"op":"SCMP_CMP_EQ"}
            ])
        );
        assert!(!String::from_utf8_lossy(base_bytes).contains("\"valueTwo\": 1"));
        Ok(())
    }
    fn fixture(phase: &Phase) -> Result<(Value, Volume), Box<dyn std::error::Error>> {
        // Recorded real Docker29.7.2 mount shapes, with this unit's explicit
        // exec tmpfs, ownership labels and fixed M1-04 formatter applied.
        // Historical receipt stays unchanged; live gateway tests cover creation.
        let receipt: Value = serde_json::from_str(include_str!(
            "../../../docs/validation/artifacts/M1-01-runtime-volume-feasibility.json"
        ))?;
        let event = receipt["events"]
            .as_array()
            .ok_or("events")?
            .iter()
            .find(|e| {
                e["args"][0] == "container"
                    && e["args"][1] == "inspect"
                    && e["args"][2].as_str().is_some_and(|n| {
                        n.ends_with(if phase.ingesting() {
                            "-ingest"
                        } else {
                            "-check"
                        })
                    })
            })
            .ok_or("inspect")?;
        let mut c: Value = serde_json::from_str(event["stdout"].as_str().ok_or("stdout")?)?;
        c[0]["Config"]["Env"]
            .as_array_mut()
            .ok_or("env")?
            .extend([json!("RUSTFMT=/opt/rust/bin/rustfmt")]);
        let env = c[0]["Config"]["Env"].as_array_mut().ok_or("env")?;
        let cargo_home = env
            .iter_mut()
            .find(|value| value.as_str() == Some("CARGO_HOME=/work/cargo"))
            .ok_or("historical cargo home")?;
        *cargo_home = json!("CARGO_HOME=/opt/rust");
        c[0]["Config"]["Image"] = json!(super::super::APPROVED_RUST_IMAGE);
        c[0]["Config"]["Labels"] =
            json!({"org.rust-mcp.execution":"true","org.rust-mcp.rust-job":"fixture"});
        c[0]["HostConfig"]["Tmpfs"]["/work"] = json!("rw,exec,nosuid,nodev,size=512m,mode=1777");
        let volume: Volume = serde_json::from_value(
            json!({"Name":c[0]["Mounts"][0]["Name"],"Mountpoint":c[0]["Mounts"][0]["Source"],"Driver":"local","Scope":"local","Options":null,"Labels":{}}),
        )?;
        Ok((c, volume))
    }

    fn coverage_fixture(
        phase: &Phase,
    ) -> Result<(Value, Volume, MutationVolume, MutationVolume), Box<dyn std::error::Error>> {
        let (mut document, source) = fixture(phase)?;
        document[0]["Config"]["User"] = json!(phase.user());
        document[0]["Config"]["Env"] = json!(phase.environment());
        document[0]["Config"]["Entrypoint"] = json!([phase.program()]);
        document[0]["Config"]["Cmd"] = json!(phase.arguments());
        let profile: Value = serde_json::from_str(phase.seccomp_profile_json())?;
        document[0]["HostConfig"]["SecurityOpt"] =
            json!(["no-new-privileges=true", format!("seccomp={profile}")]);
        let named = |name: &str, options: &str| MutationVolume {
            name: name.into(),
            driver: "local".into(),
            scope: "local".into(),
            options: BTreeMap::from([
                ("device".into(), "tmpfs".into()),
                ("o".into(), options.into()),
                ("type".into(), "tmpfs".into()),
            ]),
            labels: super::super::rust_gateway::labels("fixture"),
            mountpoint: format!("/var/lib/docker/volumes/{name}/_data"),
            cluster_volume: None,
            status: None,
        };
        let output = named(
            "coverage-output",
            super::super::mutation_gateway::VOLUME_OPTIONS,
        );
        let target = named(
            "coverage-target",
            super::super::coverage_gateway::COVERAGE_TARGET_VOLUME_OPTIONS,
        );
        let original_applied = document[0]["Mounts"][0].clone();
        let original_requested = document[0]["HostConfig"]["Mounts"][0].clone();
        let mut add_mount = |volume: &MutationVolume,
                             destination: &str,
                             writable: bool|
         -> Result<(), Box<dyn std::error::Error>> {
            let mut applied = original_applied.clone();
            applied["Name"] = json!(volume.name);
            applied["Source"] = json!(volume.mountpoint);
            applied["Destination"] = json!(destination);
            applied["RW"] = json!(writable);
            document[0]["Mounts"]
                .as_array_mut()
                .ok_or("applied mounts")?
                .push(applied);
            let mut requested = original_requested.clone();
            requested["Source"] = json!(volume.name);
            requested["Target"] = json!(destination);
            requested["ReadOnly"] = json!(!writable);
            document[0]["HostConfig"]["Mounts"]
                .as_array_mut()
                .ok_or("requested mounts")?
                .push(requested);
            Ok(())
        };
        let output_writable = !matches!(
            phase,
            Phase::ExportCoverageJson | Phase::ExportCoverageLcov | Phase::ExportCoverageHtml
        );
        add_mount(&output, "/work/coverage", output_writable)?;
        if let Some(writable) = phase.coverage_target_writable() {
            add_mount(
                &target,
                super::super::coverage_gateway::COVERAGE_TARGET_PATH,
                writable,
            )?;
        }
        Ok((document, source, output, target))
    }
    #[test]
    fn real_mount_shapes_are_accepted_for_both_phases() -> Result<(), Box<dyn std::error::Error>> {
        for phase in [Phase::Ingest, Phase::Run(RustCommand::Check)] {
            let (c, v) = fixture(&phase)?;
            assert_eq!(
                verify(
                    &serde_json::to_vec(&c)?,
                    super::super::APPROVED_RUST_IMAGE,
                    &phase,
                    &v,
                    "fixture"
                ),
                Ok(())
            );
        }
        Ok(())
    }

    #[test]
    fn coverage_mounts_enforce_the_adr065_access_matrix_and_exact_options()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::coverage::{CoverageReportFormat, CoverageSelection};
        // Literal expectations are the oracle. They deliberately do not call
        // `coverage_target_writable` to derive either side of the assertion.
        let phases = [
            (Phase::GuardCoverageVolumes, Some(false)),
            (
                Phase::Run(RustCommand::CoverageRun(
                    CoverageSelection::default().try_into()?,
                )),
                Some(true),
            ),
            (
                Phase::Run(RustCommand::CoverageReport(CoverageReportFormat::Json)),
                Some(true),
            ),
            (
                Phase::Run(RustCommand::CoverageReport(CoverageReportFormat::Lcov)),
                Some(true),
            ),
            (
                Phase::Run(RustCommand::CoverageReport(CoverageReportFormat::Html)),
                Some(true),
            ),
            (Phase::ExportCoverageJson, None),
            (Phase::ExportCoverageLcov, None),
            (Phase::ExportCoverageHtml, None),
        ];
        for (phase, expected) in phases {
            assert_eq!(phase.coverage_target_writable(), expected, "{phase:?}");
            let (document, source, output, target) = coverage_fixture(&phase)?;
            assert_eq!(
                verify_coverage(
                    &serde_json::to_vec(&document)?,
                    super::super::APPROVED_RUST_IMAGE,
                    &phase,
                    &source,
                    &output,
                    &target,
                    "fixture",
                ),
                Ok(()),
                "{phase:?}"
            );
            for (pointer, changed) in [
                ("/0/HostConfig/NetworkMode", json!("host")),
                ("/0/Mounts/0/RW", json!(true)),
                ("/0/HostConfig/Mounts/0/ReadOnly", json!(false)),
                (
                    "/0/Mounts/1/RW",
                    json!(!document[0]["Mounts"][1]["RW"].as_bool().ok_or("rw")?),
                ),
            ] {
                let mut invalid = document.clone();
                *invalid.pointer_mut(pointer).ok_or("coverage mutation")? = changed;
                assert!(
                    verify_coverage(
                        &serde_json::to_vec(&invalid)?,
                        super::super::APPROVED_RUST_IMAGE,
                        &phase,
                        &source,
                        &output,
                        &target,
                        "fixture",
                    )
                    .is_err(),
                    "{phase:?} accepted {pointer}"
                );
            }
            if phase.coverage_target_writable().is_some() {
                let mut invalid = document.clone();
                let target_rw = invalid[0]["Mounts"][2]["RW"].as_bool().ok_or("target rw")?;
                invalid[0]["Mounts"][2]["RW"] = json!(!target_rw);
                assert!(
                    verify_coverage(
                        &serde_json::to_vec(&invalid)?,
                        super::super::APPROVED_RUST_IMAGE,
                        &phase,
                        &source,
                        &output,
                        &target,
                        "fixture",
                    )
                    .is_err()
                );
            }
            let mut wrong_target = target.clone();
            wrong_target.options.insert(
                "o".into(),
                "size=512m,nr_inodes=65536,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec"
                    .into(),
            );
            assert!(
                verify_coverage(
                    &serde_json::to_vec(&document)?,
                    super::super::APPROVED_RUST_IMAGE,
                    &phase,
                    &source,
                    &output,
                    &wrong_target,
                    "fixture",
                )
                .is_err()
            );
        }
        Ok(())
    }

    #[test]
    fn coverage_target_is_absent_from_every_non_coverage_phase()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::mutation_test::MutationTestSelection;
        use rust_engineering_domain::nextest::NextestSelection;
        use rust_engineering_domain::semver_check::SemverProjectSelection;
        use rust_engineering_domain::{CheckSelection, ClippySelection, TestSelection};
        let phases = [
            Phase::Ingest,
            Phase::IngestBaseline,
            Phase::GuardNextestOutput,
            Phase::GuardMutationOutput,
            Phase::ExportNextest,
            Phase::ExportMutationOutcomes,
            Phase::ExportMutationBundle,
            Phase::ExportMutationLock,
            Phase::ListMutants(MutationTestSelection::default().try_into()?),
            Phase::Run(RustCommand::Metadata),
            Phase::Run(RustCommand::FormatCheck),
            Phase::Run(RustCommand::TestProject(
                TestSelection::default().try_into()?,
            )),
            Phase::Run(RustCommand::Check),
            Phase::Run(RustCommand::CheckProject(
                CheckSelection::default().try_into()?,
            )),
            Phase::Run(RustCommand::ClippyProject(
                ClippySelection::default().try_into()?,
            )),
            Phase::Run(RustCommand::TestNextest(
                NextestSelection::default().try_into()?,
            )),
            Phase::Run(RustCommand::SemverCheck(
                SemverProjectSelection::default().try_into()?,
            )),
            Phase::Run(RustCommand::MutationTest(
                MutationTestSelection::default().try_into()?,
            )),
            Phase::Run(RustCommand::CompilerVersion),
            Phase::Run(RustCommand::Explain("E0502".parse()?)),
            Phase::Run(RustCommand::CargoVersion),
            Phase::Run(RustCommand::LlvmCovVersion),
            Phase::Run(RustCommand::InstalledComponents),
            Phase::Run(RustCommand::SemverChecksVersion),
            Phase::Run(RustCommand::MutantsVersion),
        ];
        for phase in phases {
            assert_eq!(phase.coverage_target_writable(), None, "{phase:?}");
        }
        Ok(())
    }
    #[test]
    fn semver_baseline_ingest_is_writable_only_at_the_dedicated_mount()
    -> Result<(), Box<dyn std::error::Error>> {
        let phase = Phase::IngestBaseline;
        let (mut document, baseline) = fixture(&Phase::Ingest)?;
        document[0]["Config"]["Cmd"] = json!(phase.arguments());
        document[0]["Mounts"][0]["Destination"] = json!("/baseline");
        document[0]["HostConfig"]["Mounts"][0]["Target"] = json!("/baseline");
        assert_eq!(
            verify(
                &serde_json::to_vec(&document)?,
                super::super::APPROVED_RUST_IMAGE,
                &phase,
                &baseline,
                "fixture"
            ),
            Ok(())
        );
        document[0]["HostConfig"]["Mounts"][0]["ReadOnly"] = json!(true);
        assert!(
            verify(
                &serde_json::to_vec(&document)?,
                super::super::APPROVED_RUST_IMAGE,
                &phase,
                &baseline,
                "fixture"
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn semver_requires_exactly_two_distinct_read_only_source_volumes()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::semver_check::SemverProjectSelection;
        let phase = Phase::Run(RustCommand::SemverCheck(
            SemverProjectSelection::default().try_into()?,
        ));
        let (mut document, source) = fixture(&Phase::Run(RustCommand::Check))?;
        document[0]["Config"]["Cmd"] = json!(phase.arguments());
        document[0]["Config"]["Env"] = json!(phase.environment());
        let profile: Value = serde_json::from_str(include_str!("seccomp-rust-quality.json"))?;
        document[0]["HostConfig"]["SecurityOpt"] =
            json!(["no-new-privileges=true", format!("seccomp={profile}")]);
        document[0]["Mounts"][0]["RW"] = json!(false);
        document[0]["HostConfig"]["Mounts"][0]["ReadOnly"] = json!(true);
        let baseline: Volume = serde_json::from_value(json!({
            "Name":"baseline-volume",
            "Mountpoint":"/var/lib/docker/volumes/baseline-volume/_data",
            "Driver":"local",
            "Scope":"local",
            "Options":null,
            "Labels":{},
            "ClusterVolume":null,
            "Status":null
        }))?;
        let mut applied = document[0]["Mounts"][0].clone();
        applied["Name"] = json!(baseline.name);
        applied["Source"] = json!(baseline.mountpoint);
        applied["Destination"] = json!("/baseline");
        applied["RW"] = json!(false);
        document[0]["Mounts"]
            .as_array_mut()
            .ok_or("mounts")?
            .push(applied);
        let mut requested = document[0]["HostConfig"]["Mounts"][0].clone();
        requested["Source"] = json!(baseline.name);
        requested["Target"] = json!("/baseline");
        requested["ReadOnly"] = json!(true);
        document[0]["HostConfig"]["Mounts"]
            .as_array_mut()
            .ok_or("mount requests")?
            .push(requested);
        assert_eq!(
            verify_semver(
                &serde_json::to_vec(&document)?,
                super::super::APPROVED_RUST_IMAGE,
                &phase,
                &source,
                &baseline,
                "fixture"
            ),
            Ok(())
        );
        document[0]["Mounts"][1]["RW"] = json!(true);
        assert!(
            verify_semver(
                &serde_json::to_vec(&document)?,
                super::super::APPROVED_RUST_IMAGE,
                &phase,
                &source,
                &baseline,
                "fixture"
            )
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn mutations_cannot_add_authority_or_weaken_applied_limits()
    -> Result<(), Box<dyn std::error::Error>> {
        for phase in [Phase::Ingest, Phase::Run(RustCommand::Check)] {
            let (base, v) = fixture(&phase)?;
            let mutations = [
                ("/0/Config/Tty", json!(true)),
                ("/0/Config/OpenStdin", json!(!phase.ingesting())),
                ("/0/HostConfig/AutoRemove", json!(true)),
                ("/0/HostConfig/GroupAdd", json!(["0"])),
                ("/0/HostConfig/UTSMode", json!("host")),
                ("/0/HostConfig/OomKillDisable", json!(true)),
                ("/0/HostConfig/OomScoreAdj", json!(-1000)),
                ("/0/HostConfig/DeviceCgroupRules", json!(["a *:* rwm"])),
                ("/0/Config/User", json!("0:0")),
                ("/0/Config/Env", json!(["HOST_SECRET=sentinel"])),
                ("/0/Config/Cmd", json!(["--help"])),
                ("/0/Config/Labels/org.rust-mcp.rust-job", json!("other")),
                ("/0/HostConfig/ReadonlyRootfs", json!(false)),
                ("/0/HostConfig/NetworkMode", json!("host")),
                ("/0/HostConfig/Privileged", json!(true)),
                ("/0/HostConfig/Memory", json!(0)),
                ("/0/HostConfig/NanoCpus", json!(0)),
                ("/0/HostConfig/PidsLimit", json!(-1)),
                ("/0/HostConfig/SecurityOpt", json!(["seccomp=unconfined"])),
                ("/0/HostConfig/Mounts/0/Type", json!("bind")),
                ("/0/HostConfig/Mounts/0/Source", json!("/")),
                ("/0/HostConfig/Mounts/0/VolumeOptions/NoCopy", json!(false)),
                ("/0/Mounts/0/Type", json!("bind")),
                ("/0/Mounts/0/Source", json!("/other")),
                ("/0/Mounts/0/RW", json!(!phase.ingesting())),
            ];
            for (path, value) in mutations {
                if phase.ingesting() && path == "/0/Config/User" {
                    continue;
                }
                let mut c = base.clone();
                *c.pointer_mut(path).ok_or("mutation path")? = value;
                assert!(
                    verify(
                        &serde_json::to_vec(&c)?,
                        super::super::APPROVED_RUST_IMAGE,
                        &phase,
                        &v,
                        "fixture"
                    )
                    .is_err(),
                    "{phase:?}: {path}"
                );
            }
            for (key, value) in [
                ("Subpath", json!("outside")),
                (
                    "DriverConfig",
                    json!({"Name":"local","Options":{"device":"/","o":"bind","type":"none"}}),
                ),
            ] {
                let mut c = base.clone();
                c[0]["HostConfig"]["Mounts"][0]["VolumeOptions"][key] = value;
                assert!(
                    verify(
                        &serde_json::to_vec(&c)?,
                        super::super::APPROVED_RUST_IMAGE,
                        &phase,
                        &v,
                        "fixture"
                    )
                    .is_err()
                );
            }
        }
        Ok(())
    }

    fn mutation_fixture(
        phase: MutationPhase,
    ) -> Result<(Value, MutationVolume), Box<dyn std::error::Error>> {
        let (mut value, base) = fixture(&Phase::Run(RustCommand::Check))?;
        value[0]["Config"]["OpenStdin"] = json!(phase.interactive());
        value[0]["Config"]["AttachStdin"] = json!(phase.interactive());
        value[0]["Config"]["StdinOnce"] = json!(phase.interactive());
        value[0]["Config"]["Entrypoint"] = json!([phase.program()]);
        value[0]["Config"]["Cmd"] = json!(phase.arguments());
        value[0]["Mounts"][0]["RW"] = json!(phase.writable());
        value[0]["HostConfig"]["Mounts"][0]["ReadOnly"] = json!(!phase.writable());
        if phase == MutationPhase::Fix {
            let profile: Value = serde_json::from_str(include_str!("seccomp-rust-fix.json"))?;
            value[0]["HostConfig"]["SecurityOpt"] =
                json!(["no-new-privileges=true", format!("seccomp={profile}")]);
            value[0]["HostConfig"]["Tmpfs"]["/target"] =
                json!("rw,exec,nosuid,nodev,size=256m,mode=0700,uid=65534,gid=65534");
        }
        let volume = MutationVolume {
            name: base.name,
            driver: "local".into(),
            scope: "local".into(),
            options: BTreeMap::from([
                ("device".into(), "tmpfs".into()),
                (
                    "o".into(),
                    "size=64m,nr_inodes=8192,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec"
                        .into(),
                ),
                ("type".into(), "tmpfs".into()),
            ]),
            labels: BTreeMap::from([
                ("org.rust-mcp.execution".into(), "true".into()),
                ("org.rust-mcp.rust-job".into(), "fixture".into()),
            ]),
            mountpoint: base.mountpoint,
            cluster_volume: None,
            status: None,
        };
        Ok((value, volume))
    }

    fn resolution_fixture(
        phase: ResolutionPhase,
    ) -> Result<(Value, MutationVolume, MutationVolume), Box<dyn std::error::Error>> {
        let (mut value, base) = fixture(&Phase::Run(RustCommand::Check))?;
        value[0]["Config"]["OpenStdin"] = json!(phase.interactive());
        value[0]["Config"]["AttachStdin"] = json!(phase.interactive());
        value[0]["Config"]["StdinOnce"] = json!(phase.interactive());
        value[0]["Config"]["Entrypoint"] = json!([phase.program()]);
        value[0]["Config"]["Cmd"] = json!(phase.arguments());
        let source = MutationVolume {
            name: "source-volume".into(),
            driver: "local".into(),
            scope: "local".into(),
            options: BTreeMap::new(),
            labels: BTreeMap::new(),
            mountpoint: "/var/lib/docker/volumes/source-volume/_data".into(),
            cluster_volume: None,
            status: None,
        };
        let vendor = MutationVolume {
            name: "vendor-volume".into(),
            mountpoint: "/var/lib/docker/volumes/vendor-volume/_data".into(),
            ..source.clone()
        };
        let original_applied = value[0]["Mounts"][0].clone();
        let original_requested = value[0]["HostConfig"]["Mounts"][0].clone();
        let mut applied = Vec::new();
        let mut requested = Vec::new();
        for (target, volume, writable, mounted) in [
            (
                "/source",
                &source,
                phase.source_writable(),
                phase.source_mounted(),
            ),
            (
                "/rust-mcp-vendor",
                &vendor,
                phase.vendor_writable(),
                phase.vendor_mounted(),
            ),
        ] {
            if !mounted {
                continue;
            }
            let mut a = original_applied.clone();
            a["Name"] = json!(volume.name);
            a["Source"] = json!(volume.mountpoint);
            a["Destination"] = json!(target);
            a["RW"] = json!(writable);
            applied.push(a);
            let mut r = original_requested.clone();
            r["Source"] = json!(volume.name);
            r["Target"] = json!(target);
            r["ReadOnly"] = json!(!writable);
            requested.push(r);
        }
        value[0]["Mounts"] = Value::Array(applied);
        value[0]["HostConfig"]["Mounts"] = Value::Array(requested);
        // The generic fixture labels are already the exact product labels.
        let _ = base;
        Ok((value, source, vendor))
    }

    fn mutation_security_changes(phase: MutationPhase) -> Vec<(&'static str, Value)> {
        vec![
            ("/0/Config/Tty", json!(true)),
            ("/0/Config/OpenStdin", json!(!phase.interactive())),
            ("/0/Config/AttachStdin", json!(!phase.interactive())),
            ("/0/Config/StdinOnce", json!(!phase.interactive())),
            ("/0/Config/User", json!("0:0")),
            ("/0/Config/Env", json!(["HOST_SECRET=sentinel"])),
            ("/0/Config/Entrypoint", json!(["/usr/bin/id"])),
            ("/0/Config/Cmd", json!(["--help"])),
            ("/0/Config/WorkingDir", json!("/work")),
            ("/0/Config/Image", json!("sha256:unapproved")),
            ("/0/Config/Labels/org.rust-mcp.rust-job", json!("other")),
            ("/0/Config/Volumes", json!({"/host":{}})),
            ("/0/HostConfig/AutoRemove", json!(true)),
            ("/0/HostConfig/GroupAdd", json!(["0"])),
            ("/0/HostConfig/UTSMode", json!("host")),
            ("/0/HostConfig/OomKillDisable", json!(true)),
            ("/0/HostConfig/OomScoreAdj", json!(-1000)),
            ("/0/HostConfig/DeviceCgroupRules", json!(["a *:* rwm"])),
            ("/0/HostConfig/StorageOpt", json!({"size":"2g"})),
            ("/0/HostConfig/Annotations", json!({"unsafe":"true"})),
            ("/0/HostConfig/Runtime", json!("other")),
            ("/0/HostConfig/Init", json!(true)),
            ("/0/HostConfig/UsernsMode", json!("host")),
            ("/0/HostConfig/CgroupParent", json!("other")),
            ("/0/HostConfig/Sysctls", json!({"kernel.domainname":"x"})),
            ("/0/HostConfig/Ulimits", json!([{}])),
            ("/0/HostConfig/NetworkMode", json!("host")),
            ("/0/HostConfig/PidMode", json!("host")),
            ("/0/HostConfig/IpcMode", json!("host")),
            ("/0/HostConfig/CgroupnsMode", json!("host")),
            ("/0/HostConfig/CapDrop", json!([])),
            ("/0/HostConfig/CapAdd", json!(["SYS_ADMIN"])),
            ("/0/HostConfig/VolumesFrom", json!(["other"])),
            ("/0/HostConfig/SecurityOpt", json!(["seccomp=unconfined"])),
            ("/0/HostConfig/ReadonlyRootfs", json!(false)),
            ("/0/HostConfig/PidsLimit", json!(-1)),
            ("/0/HostConfig/NanoCpus", json!(0)),
            ("/0/HostConfig/Memory", json!(0)),
            ("/0/HostConfig/MemorySwap", json!(-1)),
            ("/0/HostConfig/ShmSize", json!(64 * 1024 * 1024)),
            ("/0/HostConfig/Privileged", json!(true)),
            ("/0/HostConfig/Binds", json!(["/:/host"])),
            ("/0/HostConfig/Devices", json!([{}])),
            ("/0/HostConfig/DeviceRequests", json!([{}])),
            ("/0/HostConfig/PublishAllPorts", json!(true)),
            ("/0/HostConfig/PortBindings", json!({"80/tcp":[{}]})),
            ("/0/HostConfig/RestartPolicy/Name", json!("always")),
            ("/0/HostConfig/LogConfig/Type", json!("json-file")),
            ("/0/HostConfig/Tmpfs/~1work", json!("rw,size=1g")),
            ("/0/HostConfig/Tmpfs/~1target", json!("rw,size=1g")),
            ("/0/HostConfig/MaskedPaths", json!([])),
            ("/0/HostConfig/ReadonlyPaths", json!([])),
            ("/0/HostConfig/Mounts/0/Type", json!("bind")),
            ("/0/HostConfig/Mounts/0/Source", json!("other")),
            ("/0/HostConfig/Mounts/0/Target", json!("/other")),
            ("/0/HostConfig/Mounts/0/ReadOnly", json!(phase.writable())),
            ("/0/HostConfig/Mounts/0/VolumeOptions/NoCopy", json!(false)),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/Labels",
                json!({"unsafe":"true"}),
            ),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/Subpath",
                json!("other"),
            ),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/DriverConfig/Name",
                json!("other"),
            ),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/DriverConfig/Options",
                json!({"type":"bind"}),
            ),
            ("/0/Mounts/0/Type", json!("bind")),
            ("/0/Mounts/0/Name", json!("other")),
            ("/0/Mounts/0/Source", json!("/other")),
            ("/0/Mounts/0/Destination", json!("/other")),
            ("/0/Mounts/0/Driver", json!("other")),
            ("/0/Mounts/0/Mode", json!("rw")),
            ("/0/Mounts/0/RW", json!(!phase.writable())),
            ("/0/Mounts/0/Propagation", json!("rshared")),
        ]
    }

    fn set_fixture_value(
        document: &mut Value,
        pointer: &str,
        changed: Value,
    ) -> Result<(), String> {
        if let Some(slot) = document.pointer_mut(pointer) {
            *slot = changed;
            return Ok(());
        }
        let (parent, key) = pointer
            .rsplit_once('/')
            .ok_or_else(|| format!("mutation path {pointer}"))?;
        document
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("mutation path {pointer}"))?
            .insert(key.to_owned(), changed);
        Ok(())
    }

    #[test]
    fn mutation_phases_enforce_access_argv_and_security_configuration()
    -> Result<(), Box<dyn std::error::Error>> {
        for phase in [
            MutationPhase::Guardian,
            MutationPhase::Ingest,
            MutationPhase::Format,
            MutationPhase::Fix,
            MutationPhase::Export,
        ] {
            let (base, volume) = mutation_fixture(phase)?;
            assert_eq!(
                verify_mutation(
                    &serde_json::to_vec(&base)?,
                    super::super::APPROVED_RUST_IMAGE,
                    phase,
                    &volume,
                    "fixture",
                ),
                Ok(())
            );
            for (path, changed) in mutation_security_changes(phase) {
                let mut invalid = base.clone();
                set_fixture_value(&mut invalid, path, changed)?;
                assert!(
                    verify_mutation(
                        &serde_json::to_vec(&invalid)?,
                        super::super::APPROVED_RUST_IMAGE,
                        phase,
                        &volume,
                        "fixture",
                    )
                    .is_err(),
                    "{phase:?} accepted {path}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn resolution_phases_enforce_two_mount_access_and_security()
    -> Result<(), Box<dyn std::error::Error>> {
        for phase in [
            ResolutionPhase::SourceGuardian,
            ResolutionPhase::VendorGuardian,
            ResolutionPhase::SourceIngest,
            ResolutionPhase::VendorIngest,
            ResolutionPhase::Resolve,
            ResolutionPhase::Frozen,
            ResolutionPhase::Export,
        ] {
            let (base, source, vendor) = resolution_fixture(phase)?;
            assert_eq!(
                verify_resolution(
                    &serde_json::to_vec(&base)?,
                    super::super::APPROVED_RUST_IMAGE,
                    phase,
                    &source,
                    &vendor,
                    "fixture",
                ),
                Ok(()),
                "{phase:?}"
            );
            for (path, changed) in [
                ("/0/Config/Cmd", json!(["metadata"])),
                ("/0/HostConfig/NetworkMode", json!("host")),
                ("/0/HostConfig/SecurityOpt", json!(["seccomp=unconfined"])),
                ("/0/HostConfig/Tmpfs/~1work", json!("rw,size=2g")),
            ] {
                let mut invalid = base.clone();
                set_fixture_value(&mut invalid, path, changed)?;
                assert!(
                    verify_resolution(
                        &serde_json::to_vec(&invalid)?,
                        super::super::APPROVED_RUST_IMAGE,
                        phase,
                        &source,
                        &vendor,
                        "fixture",
                    )
                    .is_err(),
                    "{phase:?} accepted {path}"
                );
            }
            if matches!(phase, ResolutionPhase::Resolve | ResolutionPhase::Frozen) {
                for (index, writable) in [(0, phase.source_writable()), (1, false)] {
                    let mut invalid = base.clone();
                    invalid[0]["Mounts"][index]["RW"] = json!(!writable);
                    assert!(
                        verify_resolution(
                            &serde_json::to_vec(&invalid)?,
                            super::super::APPROVED_RUST_IMAGE,
                            phase,
                            &source,
                            &vendor,
                            "fixture",
                        )
                        .is_err()
                    );
                }
            }
        }
        Ok(())
    }
}
