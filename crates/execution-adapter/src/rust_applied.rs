//! Verify the daemon's applied configuration before starting the guest.
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::RustCommand;
    use serde_json::{Value, json};
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
}
