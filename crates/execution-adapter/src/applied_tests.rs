//! Mutation oracles for the daemon-applied configuration boundary. The real
//! Docker integration test separately verifies the shape emitted by Docker 29.
use super::verify;
use rust_engineering_application::ExecutionError;
use rust_engineering_domain::ProbeScenario;
use serde_json::{Value, json};

type TestResult = Result<(), String>;
const IMAGE: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn baseline(scenario: ProbeScenario) -> Value {
    let memory = if scenario == ProbeScenario::Pids {
        268435456
    } else {
        67108864
    };
    json!([{
        "Mounts": [],
        "Config": {
            "Volumes": null,
            "User": "65532:65532",
            "Labels": {"org.rust-mcp.execution":"true"},
            "Env": ["GOMAXPROCS=2", "HOME=/work", "PATH=/nonexistent", "TMPDIR=/tmp"],
            "Entrypoint": ["/mcp-probe"],
            "Cmd": [scenario.argument()],
            "WorkingDir": "/work",
            "Image": IMAGE
        },
        "HostConfig": {
            "ReadonlyRootfs": true,
            "Runtime":"runc","Init":false,"UsernsMode":"","CgroupParent":"","Sysctls":null,"Ulimits":[],
            "MaskedPaths":["/proc/acpi","/proc/asound","/proc/interrupts","/proc/kcore","/proc/keys","/proc/latency_stats","/proc/sched_debug","/proc/scsi","/proc/timer_list","/proc/timer_stats","/sys/devices/virtual/powercap","/sys/firmware"],
            "ReadonlyPaths":["/proc/bus","/proc/fs","/proc/irq","/proc/sys","/proc/sysrq-trigger"],
            "NetworkMode": "none",
            "PidMode": "",
            "IpcMode": "private",
            "CgroupnsMode": "private",
            "CapDrop": ["ALL"],
            "CapAdd": null,
            "VolumesFrom": null,
            "SecurityOpt": ["no-new-privileges=true", format!("seccomp={}", include_str!("seccomp.json"))],
            "PidsLimit": 64,
            "NanoCpus": 500000000,
            "Memory": memory,
            "MemorySwap": memory,
            "ShmSize": 1048576,
            "Privileged": false,
            "Binds": null,
            "Mounts": [],
            "Devices": [],
            "DeviceRequests": null,
            "PublishAllPorts": false,
            "PortBindings": {},
            "Tmpfs": {
                "/work": "rw,nosuid,nodev,size=8m,mode=1777",
                "/tmp": "rw,nosuid,nodev,noexec,size=8m,mode=1777"
            },
            "LogConfig": {"Type": "none", "Config": {}},
            "RestartPolicy": {"Name": "no", "MaximumRetryCount": 0}
        }
    }])
}

fn encode(value: &Value) -> Result<Vec<u8>, String> {
    serde_json::to_vec(value).map_err(|error| error.to_string())
}

fn replace(value: &mut Value, pointer: &str, replacement: Value) -> TestResult {
    let slot = value
        .pointer_mut(pointer)
        .ok_or_else(|| format!("fixture pointer absent: {pointer}"))?;
    *slot = replacement;
    Ok(())
}

#[test]
fn accepts_each_exact_scenario_and_its_memory_budget() -> TestResult {
    for scenario in ProbeScenario::ALL {
        let value = baseline(scenario);
        assert_eq!(
            verify(&encode(&value)?, scenario, IMAGE),
            Ok(()),
            "{scenario:?}"
        );
    }
    Ok(())
}

#[test]
fn accepts_docker_equivalent_ordering_and_nullable_empty_collections() -> TestResult {
    let mut value = baseline(ProbeScenario::Success);
    replace(
        &mut value,
        "/0/Config/Env",
        json!([
            "TMPDIR=/tmp",
            "PATH=/nonexistent",
            "HOME=/work",
            "GOMAXPROCS=2"
        ]),
    )?;
    replace(
        &mut value,
        "/0/HostConfig/SecurityOpt",
        json!([
            format!("seccomp={}", include_str!("seccomp.json")),
            "no-new-privileges"
        ]),
    )?;
    replace(&mut value, "/0/HostConfig/Binds", json!([]))?;
    replace(&mut value, "/0/HostConfig/DeviceRequests", json!([]))?;
    assert_eq!(
        verify(&encode(&value)?, ProbeScenario::Success, IMAGE),
        Ok(())
    );
    Ok(())
}

#[test]
fn rejects_each_changed_safety_field_independently() -> TestResult {
    let mutations = [
        ("/0/HostConfig/Runtime", json!("untrusted")),
        ("/0/HostConfig/Init", json!(true)),
        ("/0/HostConfig/MaskedPaths", json!([])),
        ("/0/HostConfig/ReadonlyPaths", json!([])),
        ("/0/HostConfig/UsernsMode", json!("host")),
        ("/0/HostConfig/CgroupParent", json!("other")),
        ("/0/HostConfig/Sysctls", json!({"kernel.msgmax":"1"})),
        (
            "/0/HostConfig/Ulimits",
            json!([{"Name":"nofile","Soft":1,"Hard":1}]),
        ),
        ("/0/Config/Labels", json!({})),
        ("/0/HostConfig/ReadonlyRootfs", json!(false)),
        ("/0/HostConfig/NetworkMode", json!("host")),
        ("/0/HostConfig/PidMode", json!("host")),
        ("/0/HostConfig/IpcMode", json!("host")),
        ("/0/HostConfig/CgroupnsMode", json!("host")),
        ("/0/HostConfig/CapDrop", json!([])),
        ("/0/HostConfig/CapDrop", json!(["NET_RAW"])),
        ("/0/HostConfig/CapDrop", json!(["ALL", "NET_RAW"])),
        ("/0/HostConfig/Privileged", json!(true)),
        ("/0/HostConfig/PidsLimit", json!(0)),
        ("/0/HostConfig/PidsLimit", json!(-1)),
        ("/0/HostConfig/PidsLimit", json!(65)),
        ("/0/HostConfig/NanoCpus", json!(0)),
        ("/0/HostConfig/NanoCpus", json!(500000001)),
        ("/0/HostConfig/Memory", json!(0)),
        ("/0/HostConfig/Memory", json!(67108865)),
        ("/0/HostConfig/MemorySwap", json!(-1)),
        ("/0/HostConfig/MemorySwap", json!(134217728)),
        ("/0/HostConfig/ShmSize", json!(67108864)),
        ("/0/HostConfig/Binds", json!(["/:/host:rw"])),
        (
            "/0/HostConfig/Mounts",
            json!([{"Type":"bind", "Source":"/", "Target":"/host"}]),
        ),
        (
            "/0/HostConfig/Devices",
            json!([{"PathOnHost":"/dev/mem", "PathInContainer":"/dev/mem", "CgroupPermissions":"rwm"}]),
        ),
        (
            "/0/HostConfig/DeviceRequests",
            json!([{"Driver":"nvidia", "Count":-1, "Capabilities":[["gpu"]]}]),
        ),
        ("/0/HostConfig/PublishAllPorts", json!(true)),
        (
            "/0/HostConfig/PortBindings",
            json!({"80/tcp":[{"HostIp":"0.0.0.0", "HostPort":"8080"}]}),
        ),
        ("/0/HostConfig/RestartPolicy/Name", json!("always")),
        ("/0/HostConfig/LogConfig/Type", json!("json-file")),
        (
            "/0/HostConfig/Tmpfs",
            json!({"/work":"rw,nosuid,nodev,size=8m,mode=1777"}),
        ),
        (
            "/0/HostConfig/Tmpfs",
            json!({"/work":"rw,nosuid,nodev,size=8m,mode=1777", "/tmp":"rw,nosuid,nodev,noexec,size=8m,mode=1777", "/extra":"rw,size=8m"}),
        ),
        (
            "/0/HostConfig/Tmpfs/~1work",
            json!("rw,nosuid,nodev,size=80m,mode=1777"),
        ),
        (
            "/0/HostConfig/Tmpfs/~1work",
            json!("rw,nodev,size=8m,mode=1777"),
        ),
        (
            "/0/HostConfig/Tmpfs/~1work",
            json!("rw,nosuid,size=8m,mode=1777"),
        ),
        (
            "/0/HostConfig/Tmpfs/~1tmp",
            json!("rw,nosuid,nodev,size=8m,mode=1777"),
        ),
        ("/0/Config/User", json!("0:0")),
        ("/0/Config/User", json!("65532:0")),
        ("/0/Config/WorkingDir", json!("/")),
        ("/0/Config/Image", json!("untrusted:latest")),
        ("/0/Config/Entrypoint", json!(["/bin/sh", "-c"])),
        ("/0/Config/Entrypoint", json!(["/mcp-probe", "success"])),
        ("/0/Config/Cmd", json!(["heartbeat"])),
        ("/0/Config/Cmd", json!(["success", "--arbitrary"])),
        (
            "/0/Config/Env",
            json!(["HOME=/work", "PATH=/nonexistent", "TMPDIR=/tmp"]),
        ),
        (
            "/0/Config/Env",
            json!(["GOMAXPROCS=2", "HOME=/work", "PATH=/usr/bin", "TMPDIR=/tmp"]),
        ),
        (
            "/0/Config/Env",
            json!([
                "GOMAXPROCS=2",
                "HOME=/work",
                "PATH=/nonexistent",
                "TMPDIR=/tmp",
                "HOST_SECRET=canary"
            ]),
        ),
        (
            "/0/Config/Env",
            json!([
                "GOMAXPROCS=2",
                "HOME=/work",
                "PATH=/nonexistent",
                "TMPDIR=/tmp",
                "HOME=/work"
            ]),
        ),
    ];
    for (pointer, changed) in mutations {
        let mut value = baseline(ProbeScenario::Success);
        replace(&mut value, pointer, changed.clone())?;
        assert_eq!(
            verify(&encode(&value)?, ProbeScenario::Success, IMAGE),
            Err(ExecutionError::InvalidConfiguration),
            "accepted mutation {pointer}={changed}"
        );
    }
    Ok(())
}

#[test]
fn rejects_capability_additions_and_implicit_or_applied_mounts() -> TestResult {
    for (pointer, replacement) in [
        ("/0/HostConfig/CapAdd", json!(["SYS_ADMIN"])),
        ("/0/HostConfig/VolumesFrom", json!(["hostdata:rw"])),
        ("/0/Config/Volumes", json!({"/host": {}})),
        (
            "/0/Mounts",
            json!([{"Type": "volume", "Name": "automatic", "Source": "/var/lib/docker/volumes/automatic/_data", "Destination": "/host", "RW": true}]),
        ),
    ] {
        let mut value = baseline(ProbeScenario::Success);
        replace(&mut value, pointer, replacement)?;
        assert_eq!(
            verify(&encode(&value)?, ProbeScenario::Success, IMAGE),
            Err(ExecutionError::InvalidConfiguration),
            "accepted {pointer}"
        );
    }
    Ok(())
}

#[test]
fn accepts_omitted_or_empty_optional_capabilities_and_mounts() -> TestResult {
    let mut value = baseline(ProbeScenario::Success);
    for (pointer, key) in [
        ("/0/HostConfig", "CapAdd"),
        ("/0/HostConfig", "VolumesFrom"),
        ("/0/Config", "Volumes"),
        ("/0", "Mounts"),
    ] {
        let object = value
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("fixture object absent: {pointer}"))?;
        assert!(object.remove(key).is_some());
    }
    assert_eq!(
        verify(&encode(&value)?, ProbeScenario::Success, IMAGE),
        Ok(())
    );
    let mut value = baseline(ProbeScenario::Success);
    replace(&mut value, "/0/HostConfig/CapAdd", json!([]))?;
    replace(&mut value, "/0/HostConfig/VolumesFrom", json!([]))?;
    replace(&mut value, "/0/Config/Volumes", json!({}))?;
    assert_eq!(
        verify(&encode(&value)?, ProbeScenario::Success, IMAGE),
        Ok(())
    );
    Ok(())
}

#[test]
fn rejects_unconfined_tampered_missing_and_duplicate_seccomp_profiles() -> TestResult {
    let trusted: Value =
        serde_json::from_str(include_str!("seccomp.json")).map_err(|error| error.to_string())?;
    let profile = format!(
        "seccomp={}",
        serde_json::to_string(&trusted).map_err(|error| error.to_string())?
    );
    let mut permissive = trusted.clone();
    replace(&mut permissive, "/defaultAction", json!("SCMP_ACT_ALLOW"))?;
    let mut extra_syscall = trusted.clone();
    let names = extra_syscall
        .pointer_mut("/syscalls/0/names")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "trusted syscall list absent".to_owned())?;
    names.push(json!("socket"));
    let options = [
        json!(["no-new-privileges=true", "seccomp=unconfined"]),
        json!(["no-new-privileges=true", "seccomp={malformed"]),
        json!([
            "no-new-privileges=true",
            format!("seccomp={}", include_str!("seccomp-socket.json"))
        ]),
        json!([profile.clone()]),
        json!(["no-new-privileges=false", profile.clone()]),
        json!(["no-new-privileges=true", profile.clone(), profile.clone()]),
        json!(["no-new-privileges=true", "no-new-privileges=true"]),
        json!(["no-new-privileges=true", profile, "label=disable"]),
        json!(["no-new-privileges=true", format!("seccomp={permissive}")]),
        json!(["no-new-privileges=true", format!("seccomp={extra_syscall}")]),
    ];
    for changed in options {
        let mut value = baseline(ProbeScenario::Success);
        replace(&mut value, "/0/HostConfig/SecurityOpt", changed.clone())?;
        assert_eq!(
            verify(&encode(&value)?, ProbeScenario::Success, IMAGE),
            Err(ExecutionError::InvalidConfiguration),
            "accepted security options {changed}"
        );
    }
    Ok(())
}

#[test]
fn rejects_pid_memory_budget_reused_for_other_scenarios_and_reverse() -> TestResult {
    for (scenario, wrong_memory) in [
        (ProbeScenario::Pids, 67108864),
        (ProbeScenario::Memory, 268435456),
    ] {
        for pointer in ["/0/HostConfig/Memory", "/0/HostConfig/MemorySwap"] {
            let mut value = baseline(scenario);
            replace(&mut value, pointer, json!(wrong_memory))?;
            assert_eq!(
                verify(&encode(&value)?, scenario, IMAGE),
                Err(ExecutionError::InvalidConfiguration)
            );
        }
    }
    let value = baseline(ProbeScenario::Success);
    assert_eq!(
        verify(&encode(&value)?, ProbeScenario::Exit7, IMAGE),
        Err(ExecutionError::InvalidConfiguration)
    );
    assert_eq!(
        verify(&encode(&value)?, ProbeScenario::Success, "sha256:different"),
        Err(ExecutionError::InvalidConfiguration)
    );
    Ok(())
}

#[test]
fn rejects_missing_required_configuration_fields() -> TestResult {
    let required = [
        ("/0", "Config"),
        ("/0", "HostConfig"),
        ("/0/Config", "User"),
        ("/0/Config", "Env"),
        ("/0/Config", "Entrypoint"),
        ("/0/Config", "Cmd"),
        ("/0/Config", "WorkingDir"),
        ("/0/Config", "Image"),
        ("/0/HostConfig", "ReadonlyRootfs"),
        ("/0/HostConfig", "NetworkMode"),
        ("/0/HostConfig", "PidMode"),
        ("/0/HostConfig", "IpcMode"),
        ("/0/HostConfig", "CgroupnsMode"),
        ("/0/HostConfig", "CapDrop"),
        ("/0/HostConfig", "SecurityOpt"),
        ("/0/HostConfig", "PidsLimit"),
        ("/0/HostConfig", "NanoCpus"),
        ("/0/HostConfig", "Memory"),
        ("/0/HostConfig", "MemorySwap"),
        ("/0/HostConfig", "ShmSize"),
        ("/0/HostConfig", "Privileged"),
        ("/0/HostConfig", "Tmpfs"),
        ("/0/HostConfig", "LogConfig"),
        ("/0/HostConfig", "PublishAllPorts"),
        ("/0/HostConfig", "PortBindings"),
        ("/0/HostConfig", "RestartPolicy"),
        ("/0/HostConfig/LogConfig", "Type"),
        ("/0/HostConfig/RestartPolicy", "Name"),
    ];
    for (pointer, key) in required {
        let mut value = baseline(ProbeScenario::Success);
        let object = value
            .pointer_mut(pointer)
            .and_then(Value::as_object_mut)
            .ok_or_else(|| format!("fixture object absent: {pointer}"))?;
        assert!(object.remove(key).is_some());
        assert_eq!(
            verify(&encode(&value)?, ProbeScenario::Success, IMAGE),
            Err(ExecutionError::Infrastructure),
            "accepted missing {pointer}/{key}"
        );
    }
    Ok(())
}

#[test]
fn rejects_malformed_wrong_typed_and_duplicate_container_inspection() -> TestResult {
    for bytes in [
        b"".as_slice(),
        b"{",
        b"null",
        b"{}",
        b"[]",
        b"[null]",
        b"[{},{}]",
    ] {
        assert_eq!(
            verify(bytes, ProbeScenario::Success, IMAGE),
            Err(ExecutionError::Infrastructure)
        );
    }
    let valid = baseline(ProbeScenario::Success);
    let container = valid
        .get(0)
        .ok_or_else(|| "baseline container absent".to_owned())?;
    let duplicated = json!([container, container]);
    assert_eq!(
        verify(&encode(&duplicated)?, ProbeScenario::Success, IMAGE),
        Err(ExecutionError::Infrastructure)
    );
    for (pointer, replacement) in [
        ("/0/Config/User", json!(65532)),
        ("/0/Config/Env", json!(null)),
        ("/0/Config/Entrypoint", json!("/mcp-probe")),
        ("/0/HostConfig/PidsLimit", json!(64.5)),
        ("/0/HostConfig/Memory", json!(18446744073709551615_u64)),
        ("/0/HostConfig/ReadonlyRootfs", json!("true")),
        ("/0/HostConfig/SecurityOpt", json!([null])),
        ("/0/HostConfig/Tmpfs", json!([])),
    ] {
        let mut value = baseline(ProbeScenario::Success);
        replace(&mut value, pointer, replacement)?;
        assert_eq!(
            verify(&encode(&value)?, ProbeScenario::Success, IMAGE),
            Err(ExecutionError::Infrastructure),
            "accepted wrong type {pointer}"
        );
    }
    let text = serde_json::to_string(&valid).map_err(|error| error.to_string())?;
    let duplicate_field = text.replace(
        "\"ReadonlyRootfs\":true",
        "\"ReadonlyRootfs\":true,\"ReadonlyRootfs\":false",
    );
    assert_ne!(text, duplicate_field);
    assert_eq!(
        verify(duplicate_field.as_bytes(), ProbeScenario::Success, IMAGE),
        Err(ExecutionError::Infrastructure)
    );
    Ok(())
}

#[test]
fn control_profiles_are_bound_to_scenarios_and_preserve_other_guarantees() -> TestResult {
    use crate::Profile;
    let mut network = baseline(ProbeScenario::Network);
    replace(
        &mut network,
        "/0/HostConfig/SecurityOpt",
        json!([
            "no-new-privileges=true",
            format!("seccomp={}", include_str!("seccomp-socket.json"))
        ]),
    )?;
    assert!(
        super::verify_profile(
            &encode(&network)?,
            ProbeScenario::Network,
            IMAGE,
            Profile::SocketControl
        )
        .is_ok()
    );
    assert!(verify(&encode(&network)?, ProbeScenario::Network, IMAGE).is_err());
    replace(&mut network, "/0/HostConfig/ReadonlyRootfs", json!(false))?;
    assert!(
        super::verify_profile(
            &encode(&network)?,
            ProbeScenario::Network,
            IMAGE,
            Profile::SocketControl
        )
        .is_err()
    );
    let mut fs = baseline(ProbeScenario::Filesystem);
    replace(&mut fs, "/0/HostConfig/ReadonlyRootfs", json!(false))?;
    assert!(
        super::verify_profile(
            &encode(&fs)?,
            ProbeScenario::Filesystem,
            IMAGE,
            Profile::WritableControl
        )
        .is_ok()
    );
    assert!(verify(&encode(&fs)?, ProbeScenario::Filesystem, IMAGE).is_err());
    replace(&mut fs, "/0/HostConfig/CapAdd", json!(["SYS_ADMIN"]))?;
    assert!(
        super::verify_profile(
            &encode(&fs)?,
            ProbeScenario::Filesystem,
            IMAGE,
            Profile::WritableControl
        )
        .is_err()
    );
    for scenario in ProbeScenario::ALL {
        for (mode, allowed) in [
            (Profile::SocketControl, ProbeScenario::Network),
            (Profile::WritableControl, ProbeScenario::Filesystem),
        ] {
            if scenario != allowed {
                assert_eq!(
                    super::verify_profile(&[], scenario, IMAGE, mode),
                    Err(ExecutionError::Denied)
                );
            }
        }
    }
    Ok(())
}

#[test]
fn socket_control_profile_only_adds_socket_and_preserves_every_other_rule() -> TestResult {
    let strict: Value =
        serde_json::from_str(include_str!("seccomp.json")).map_err(|e| e.to_string())?;
    let mut control: Value =
        serde_json::from_str(include_str!("seccomp-socket.json")).map_err(|e| e.to_string())?;
    let names = control
        .pointer_mut("/syscalls/0/names")
        .and_then(Value::as_array_mut)
        .ok_or("missing syscall names")?;
    assert_eq!(names.pop(), Some(json!("socket")));
    assert_eq!(control, strict);
    Ok(())
}
