//! Verify the daemon's applied configuration before starting the guest.
use rust_engineering_application::ExecutionError;
use rust_engineering_domain::ProbeScenario;
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

#[cfg(test)]
fn verify(bytes: &[u8], scenario: ProbeScenario, image: &str) -> Result<(), ExecutionError> {
    verify_profile(bytes, scenario, image, crate::Profile::Enforced)
}
pub(super) fn verify_profile(
    bytes: &[u8],
    scenario: ProbeScenario,
    image: &str,
    mode: crate::Profile,
) -> Result<(), ExecutionError> {
    if !mode.permits(scenario) {
        return Err(ExecutionError::Denied);
    }
    let containers: Vec<Created> =
        serde_json::from_slice(bytes).map_err(|_| ExecutionError::Infrastructure)?;
    let c = containers
        .first()
        .filter(|_| containers.len() == 1)
        .ok_or(ExecutionError::Infrastructure)?;
    let h = &c.host_config;
    let memory = if scenario == ProbeScenario::Pids {
        268435456
    } else {
        67108864
    };
    let mut env = c.config.env.clone();
    env.sort();
    // Docker embeds the profile JSON after loading the trusted control file.
    let profile: serde_json::Value =
        serde_json::from_str(if mode == crate::Profile::SocketControl {
            include_str!("seccomp-socket.json")
        } else {
            include_str!("seccomp.json")
        })
        .map_err(|_| ExecutionError::Infrastructure)?;
    let seccomp = h
        .security_opt
        .iter()
        .filter_map(|s| s.strip_prefix("seccomp="))
        .collect::<Vec<_>>();
    let profile_ok = seccomp.len() == 1
        && serde_json::from_str::<serde_json::Value>(seccomp[0]).is_ok_and(|v| v == profile);
    let safe = h.runtime == "runc"
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
        && c.config
            .labels
            .get("org.rust-mcp.execution")
            .is_some_and(|v| v == "true")
        && c.mounts.is_empty()
        && c.config.volumes.as_ref().is_none_or(BTreeMap::is_empty)
        && h.cap_add.as_ref().is_none_or(Vec::is_empty)
        && h.volumes_from.as_ref().is_none_or(Vec::is_empty)
        && h.readonly_rootfs == (mode != crate::Profile::WritableControl)
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
        && h.pids_limit == 64
        && h.nano_cpus == 500000000
        && h.memory == memory
        && h.memory_swap == memory
        && h.shm_size == 1048576
        && !h.privileged
        && h.binds.as_ref().is_none_or(Vec::is_empty)
        && h.mounts.is_empty()
        && h.devices.is_empty()
        && h.device_requests.as_ref().is_none_or(Vec::is_empty)
        && !h.publish_all_ports
        && h.port_bindings.is_empty()
        && h.restart_policy.name == "no"
        && h.log_config.kind == "none"
        && h.tmpfs.len() == 2
        && h.tmpfs
            .get("/work")
            .is_some_and(|s| s == "rw,nosuid,nodev,size=8m,mode=1777")
        && h.tmpfs
            .get("/tmp")
            .is_some_and(|s| s == "rw,nosuid,nodev,noexec,size=8m,mode=1777")
        && c.config.user == "65532:65532"
        && c.config.working_dir == "/work"
        && c.config.image == image
        && c.config.entrypoint == ["/mcp-probe"]
        && c.config.cmd == [scenario.argument()]
        && env
            == [
                "GOMAXPROCS=2",
                "HOME=/work",
                "PATH=/nonexistent",
                "TMPDIR=/tmp",
            ];
    if safe {
        Ok(())
    } else {
        Err(ExecutionError::InvalidConfiguration)
    }
}

#[cfg(test)]
#[path = "applied_tests.rs"]
mod tests;
