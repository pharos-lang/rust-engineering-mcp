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

pub(super) fn verify(
    bytes: &[u8],
    image: &str,
    phase: &Phase,
    volume: &Volume,
    nonce: &str,
) -> Result<(), ExecutionError> {
    let containers: Vec<Created> =
        serde_json::from_slice(bytes).map_err(|_| ExecutionError::Infrastructure)?;
    let c = containers
        .first()
        .filter(|_| containers.len() == 1)
        .ok_or(ExecutionError::Infrastructure)?;
    let h = &c.host_config;
    let mut env = c.config.env.clone();
    env.sort();
    let profile: serde_json::Value = serde_json::from_str(include_str!("seccomp-rust.json"))
        .map_err(|_| ExecutionError::Infrastructure)?;
    let seccomp = h
        .security_opt
        .iter()
        .filter_map(|s| s.strip_prefix("seccomp="))
        .collect::<Vec<_>>();
    let profile_ok = seccomp.len() == 1
        && serde_json::from_str::<serde_json::Value>(seccomp[0]).is_ok_and(|v| v == profile);
    let safe = !c.config.tty
        && c.config.open_stdin == phase.ingesting()
        && c.config.attach_stdin == phase.ingesting()
        && c.config.stdin_once == phase.ingesting()
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
        && c.config.labels
            == BTreeMap::from([
                ("org.rust-mcp.execution".into(), "true".into()),
                ("org.rust-mcp.rust-job".into(), nonce.into()),
            ])
        && mounts_ok(c, phase, volume)?
        && c.config.volumes.as_ref().is_none_or(BTreeMap::is_empty)
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
            .any(|s| s == "no-new-privileges=true" || s == "no-new-privileges")
        && profile_ok
        && h.pids_limit == 128
        && h.nano_cpus == 1000000000
        && h.memory == 1073741824
        && h.memory_swap == 1073741824
        && h.shm_size == 1048576
        && !h.privileged
        && h.binds.as_ref().is_none_or(Vec::is_empty)
        && h.devices.is_empty()
        && h.device_requests.as_ref().is_none_or(Vec::is_empty)
        && !h.publish_all_ports
        && h.port_bindings.is_empty()
        && h.restart_policy.name == "no"
        && h.log_config.kind == "none"
        && h.tmpfs.len() == 2
        && h.tmpfs
            .get("/work")
            .is_some_and(|s| s == "rw,exec,nosuid,nodev,size=512m,mode=1777")
        && h.tmpfs
            .get("/tmp")
            .is_some_and(|s| s == "rw,nosuid,nodev,noexec,size=64m,mode=1777")
        && c.config.user == phase.user()
        && c.config.working_dir == "/source"
        && c.config.image == image
        && c.config.entrypoint == [phase.program()]
        && c.config.cmd == phase.arguments()
        && env == super::rust_gateway::environment();
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
fn mounts_ok(c: &Created, phase: &Phase, volume: &Volume) -> Result<bool, ExecutionError> {
    if c.mounts.len() != 1 || c.host_config.mounts.len() != 1 {
        return Ok(false);
    }
    let a: AppliedMount = serde_json::from_value(c.mounts[0].clone())
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    let r: RequestedMount = serde_json::from_value(c.host_config.mounts[0].clone())
        .map_err(|_| ExecutionError::InvalidConfiguration)?;
    Ok(a.kind == "volume"
        && a.name == volume.name
        && a.source == volume.mountpoint
        && a.destination == "/source"
        && a.driver == "local"
        && a.mode == "z"
        && a.propagation.is_empty()
        && a.rw == phase.ingesting()
        && r.kind == "volume"
        && r.source == volume.name
        && r.target == "/source"
        && r.read_only != phase.ingesting()
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
    let containers: Vec<Created> =
        serde_json::from_slice(bytes).map_err(|_| ExecutionError::Infrastructure)?;
    let c = containers
        .first()
        .filter(|_| containers.len() == 1)
        .ok_or(ExecutionError::Infrastructure)?;
    let h = &c.host_config;
    let mut env = c.config.env.clone();
    env.sort();
    let expected_profile = if phase == MutationPhase::Fix {
        include_str!("seccomp-rust-fix.json")
    } else {
        include_str!("seccomp-rust.json")
    };
    let profile: serde_json::Value =
        serde_json::from_str(expected_profile).map_err(|_| ExecutionError::Infrastructure)?;
    let seccomp = h
        .security_opt
        .iter()
        .filter_map(|value| value.strip_prefix("seccomp="))
        .collect::<Vec<_>>();
    let profile_ok = seccomp.len() == 1
        && serde_json::from_str::<serde_json::Value>(seccomp[0])
            .is_ok_and(|value| value == profile);
    let expected_labels = BTreeMap::from([
        ("org.rust-mcp.execution".into(), "true".into()),
        ("org.rust-mcp.rust-job".into(), nonce.into()),
    ]);
    let safe = !c.config.tty
        && c.config.open_stdin == phase.interactive()
        && c.config.attach_stdin == phase.interactive()
        && c.config.stdin_once == phase.interactive()
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
        && c.config.labels == expected_labels
        && mutation_mounts_ok(c, phase, volume)?
        && c.config.volumes.as_ref().is_none_or(BTreeMap::is_empty)
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
        && profile_ok
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
        && h.tmpfs.len() == if phase == MutationPhase::Fix { 3 } else { 2 }
        && h.tmpfs
            .get("/work")
            .is_some_and(|value| value == "rw,exec,nosuid,nodev,size=512m,mode=1777")
        && h.tmpfs
            .get("/tmp")
            .is_some_and(|value| value == "rw,nosuid,nodev,noexec,size=64m,mode=1777")
        && (phase != MutationPhase::Fix
            || h.tmpfs.get("/target").is_some_and(|value| {
                value == "rw,exec,nosuid,nodev,size=256m,mode=0700,uid=65534,gid=65534"
            }))
        && c.config.user == "65534:65534"
        && c.config.working_dir == "/source"
        && c.config.image == image
        && c.config.entrypoint == [phase.program()]
        && c.config.cmd == phase.arguments()
        && env == super::rust_gateway::environment();
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
    let containers: Vec<Created> =
        serde_json::from_slice(bytes).map_err(|_| ExecutionError::Infrastructure)?;
    let c = containers
        .first()
        .filter(|_| containers.len() == 1)
        .ok_or(ExecutionError::Infrastructure)?;
    let h = &c.host_config;
    let mut env = c.config.env.clone();
    env.sort();
    let profile: serde_json::Value = serde_json::from_str(include_str!("seccomp-rust.json"))
        .map_err(|_| ExecutionError::Infrastructure)?;
    let seccomp = h
        .security_opt
        .iter()
        .filter_map(|value| value.strip_prefix("seccomp="))
        .collect::<Vec<_>>();
    let profile_ok = seccomp.len() == 1
        && serde_json::from_str::<serde_json::Value>(seccomp[0])
            .is_ok_and(|value| value == profile);
    let expected_labels = BTreeMap::from([
        ("org.rust-mcp.execution".into(), "true".into()),
        ("org.rust-mcp.rust-job".into(), nonce.into()),
    ]);
    let safe = !c.config.tty
        && c.config.open_stdin == phase.interactive()
        && c.config.attach_stdin == phase.interactive()
        && c.config.stdin_once == phase.interactive()
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
        && c.config.labels == expected_labels
        && resolution_mounts_ok(c, phase, source, vendor)?
        && c.config.volumes.as_ref().is_none_or(BTreeMap::is_empty)
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
        && profile_ok
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
        && h.tmpfs.len() == 2
        && h.tmpfs
            .get("/work")
            .is_some_and(|value| value == "rw,exec,nosuid,nodev,size=512m,mode=1777")
        && h.tmpfs
            .get("/tmp")
            .is_some_and(|value| value == "rw,nosuid,nodev,noexec,size=64m,mode=1777")
        && c.config.user == "65534:65534"
        && c.config.working_dir == "/source"
        && c.config.image == image
        && c.config.entrypoint == [phase.program()]
        && c.config.cmd == phase.arguments()
        && env == super::rust_gateway::environment();
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
            .push(json!("RUSTFMT=/opt/rust/bin/rustfmt"));
        c[0]["Config"]["Labels"] =
            json!({"org.rust-mcp.execution":"true","org.rust-mcp.rust-job":"fixture"});
        c[0]["HostConfig"]["Tmpfs"]["/work"] = json!("rw,exec,nosuid,nodev,size=512m,mode=1777");
        let volume: Volume = serde_json::from_value(
            json!({"Name":c[0]["Mounts"][0]["Name"],"Mountpoint":c[0]["Mounts"][0]["Source"],"Driver":"local","Scope":"local","Options":null,"Labels":{}}),
        )?;
        Ok((c, volume))
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
