//! Active calibration of the exact, trusted, fixed probe configuration.
//! No result grants authority to execute project code or another image.
use crate::{DockerGateway, EngineIdentity, Profile};
use rust_engineering_application::{ExecutionError, ExecutionPort, NeverCancel, admit_execution};
use rust_engineering_domain::{
    ExecutionFingerprint, ExecutionLimits, ExecutionResult, ExecutionSpec, ExecutionTermination,
    ProbeScenario as S, SandboxCapabilities, SandboxEvidence, SandboxTier,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
pub struct CapabilityReport {
    pub status: CapabilityStatus,
    pub scope: &'static str,
    pub observed_at_unix_ms: u64,
    pub engine: EngineIdentity,
    pub image_id: String,
    pub capabilities: SandboxCapabilities,
    pub configuration_fingerprint: ExecutionFingerprint,
    pub strict_available: bool,
    pub restricted_available: bool,
    pub project_code_available: bool,
    pub observations: Vec<Observation>,
    pub invalid_evidence: Vec<S>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Verified,
    Degraded,
}
#[derive(Debug, Serialize)]
pub struct Observation {
    pub scenario: S,
    pub control: bool,
    pub limits: ExecutionLimits,
    pub memory_bytes: u64,
    pub execution: ExecutionResult,
}
#[derive(Deserialize)]
struct Record {
    pid: u32,
    #[serde(flatten)]
    event: Event,
}
#[derive(Deserialize)]
#[serde(tag = "event", content = "details", rename_all = "snake_case")]
enum Event {
    Environment {
        entries: Vec<String>,
    },
    Socket {
        family: String,
        transport: String,
        purpose: String,
        operation: String,
        result: Operation,
    },
    Write {
        path: String,
        result: Operation,
    },
    FilesystemAssertions {
        passed: bool,
        rootfs_unchanged: bool,
        unexpected_root_writes: u64,
    },
    SymlinkSwap {
        swaps: u64,
        attempts: u64,
        positive_writes: u64,
        readonly_denials: u64,
        unexpected_root_writes: u64,
        other_errors: u64,
    },
    DescendantStarted {
        child_pid: u32,
        parent_process_group: u32,
        setsid: bool,
        double_fork: bool,
    },
    Heartbeat {
        parent_pid: u32,
        process_group: u32,
    },
    Pids {
        started: u64,
        cgroups: BTreeMap<String, Reading>,
    },
    MemoryAllocated {
        bytes: u64,
    },
    Disk {
        bytes_written: u64,
        enospc: bool,
        result: Operation,
    },
    Cpu {
        before: Reading,
        after: Reading,
        #[serde(rename = "cpu.max")]
        maximum: Reading,
    },
    Cgroups(BTreeMap<String, Reading>),
    #[serde(other)]
    Other,
}
#[derive(Deserialize)]
struct Operation {
    allowed: bool,
    errno: i64,
}
#[derive(Deserialize)]
struct Reading {
    value: String,
    result: Operation,
}
fn records(result: &ExecutionResult) -> Result<Vec<Record>, ExecutionError> {
    #[derive(Deserialize)]
    struct Header {
        pid: u32,
        event: String,
    }
    result
        .stdout
        .lines()
        .map(|line| {
            let header: Header =
                serde_json::from_str(line).map_err(|_| ExecutionError::Infrastructure)?;
            if [
                "environment",
                "socket",
                "write",
                "filesystem_assertions",
                "symlink_swap",
                "descendant_started",
                "heartbeat",
                "pids",
                "memory_allocated",
                "disk",
                "cpu",
                "cgroups",
            ]
            .contains(&header.event.as_str())
            {
                serde_json::from_str(line).map_err(|_| ExecutionError::Infrastructure)
            } else {
                Ok(Record {
                    pid: header.pid,
                    event: Event::Other,
                })
            }
        })
        .collect()
}

/// Invalid guest evidence fails this probe; it is not a Docker availability error.
fn probe_records(result: &ExecutionResult) -> Vec<Record> {
    records(result).unwrap_or_default()
}

fn normal(r: &ExecutionResult) -> bool {
    r.termination == ExecutionTermination::Exited
        && r.exit_code == Some(0)
        && !r.stdout_truncated
        && !r.stderr_truncated
        && r.stderr.is_empty()
}
fn network(r: &ExecutionResult, allow: bool) -> bool {
    if !normal(r) {
        return false;
    }
    let Ok(records) = records(r) else {
        return false;
    };
    if records.len() != 10 {
        return false;
    }
    let mut seen = BTreeSet::new();
    for record in records {
        let Event::Socket {
            family,
            transport,
            purpose,
            operation,
            result,
        } = record.event
        else {
            return false;
        };
        let ip = ["ipv4", "ipv6"].contains(&family.as_str())
            && ["tcp", "udp"].contains(&transport.as_str())
            && ["dns", "loopback"].contains(&purpose.as_str());
        let local = purpose == "local"
            && ((family == "unix" && transport == "stream")
                || (family == "netlink" && transport == "raw"));
        if !(ip || local)
            || operation != "socket_only"
            || result.allowed != allow
            || result.errno != if allow { 0 } else { 1 }
        {
            return false;
        }
        seen.insert((family, transport, purpose));
    }
    seen.len() == 10
}
fn reading<'a>(map: &'a BTreeMap<String, Reading>, name: &str) -> Option<&'a str> {
    map.get(name)
        .filter(|r| r.result.allowed && r.result.errno == 0)
        .map(|r| r.value.as_str())
}
fn counter(reading: &str, name: &str) -> Option<u64> {
    reading.lines().find_map(|l| {
        let mut words = l.split_whitespace();
        (words.next()? == name)
            .then(|| words.next()?.parse().ok())
            .flatten()
    })
}
fn increased(before: &Reading, after: &Reading, name: &str) -> bool {
    before.result.allowed
        && before.result.errno == 0
        && after.result.allowed
        && after.result.errno == 0
        && matches!((counter(&before.value,name),counter(&after.value,name)),(Some(a),Some(b)) if b>a)
}

fn memory_enforced(memory: &ExecutionResult, ram: &[Record], cgroups_ok: bool) -> bool {
    cgroups_ok
        && memory.termination == ExecutionTermination::Exited
        && memory.exit_code == Some(137)
        && memory.oom_killed == Some(true)
        && ram
            .iter()
            .any(|r| matches!(r.event,Event::MemoryAllocated{bytes} if bytes>0 && bytes<67108864))
}

impl DockerGateway {
    /// Explicit host operation: active adversarial fixtures, bounded and local.
    pub fn probe_capabilities(&self) -> Result<CapabilityReport, ExecutionError> {
        let mut observations = Vec::new();
        let mut run = |scenario: S,
                       profile: Profile,
                       wall: u64,
                       bytes: usize|
         -> Result<ExecutionResult, ExecutionError> {
            let spec = ExecutionSpec {
                scenario,
                limits: ExecutionLimits::new(wall, bytes)
                    .ok_or(ExecutionError::InvalidConfiguration)?,
            };
            let execution = if profile == Profile::Enforced {
                self.execute(&spec, &NeverCancel)?
            } else {
                self.execute_profile(&spec, &NeverCancel, profile)?
            };
            observations.push(Observation {
                scenario,
                control: profile != Profile::Enforced,
                limits: spec.limits,
                memory_bytes: if scenario == S::Pids {
                    268435456
                } else {
                    67108864
                },
                execution: execution.clone(),
            });
            Ok(execution)
        };
        let environment = run(S::Environment, Profile::Enforced, 10000, 65536)?;
        let net = run(S::Network, Profile::Enforced, 10000, 65536)?;
        // Socket creation only; this control never connects or sends traffic.
        let net_control = run(S::Network, Profile::SocketControl, 10000, 65536)?;
        let filesystem = run(S::Filesystem, Profile::Enforced, 10000, 65536)?;
        let fs_control = run(S::Filesystem, Profile::WritableControl, 10000, 65536)?;
        let descendants = run(S::Descendants, Profile::Enforced, 1500, 65536)?;
        let output = run(S::Output, Profile::Enforced, 10000, 4096)?;
        let cgroups = run(S::Cgroups, Profile::Enforced, 10000, 65536)?;
        let pids = run(S::Pids, Profile::Enforced, 10000, 65536)?;
        let memory = run(S::Memory, Profile::Enforced, 10000, 65536)?;
        let disk = run(S::Disk, Profile::Enforced, 10000, 65536)?;
        let cpu = run(S::Cpu, Profile::Enforced, 10000, 65536)?;
        let env = probe_records(&environment);
        let fs = probe_records(&filesystem);
        let fs_ctrl = probe_records(&fs_control);
        let children = probe_records(&descendants);
        let groups = probe_records(&cgroups);
        let pid_records = probe_records(&pids);
        let ram = probe_records(&memory);
        let disks = probe_records(&disk);
        let cpus = probe_records(&cpu);
        let env_ok=normal(&environment) && env.len()==1 && env.iter().any(|r|matches!(&r.event,Event::Environment{entries} if entries==&["GOMAXPROCS=2","HOME=/work","HOSTNAME=sandbox","PATH=/nonexistent","TMPDIR=/tmp"]));
        let fs_ok=normal(&filesystem) && fs.iter().any(|r|matches!(r.event,Event::FilesystemAssertions{passed:true,rootfs_unchanged:true,unexpected_root_writes:0}))
            && fs.iter().any(|r|matches!(r.event,Event::SymlinkSwap{swaps:256,attempts:512,positive_writes,readonly_denials,unexpected_root_writes:0,other_errors:0} if positive_writes>0 && readonly_denials>0))
            && !fs_control.stdout_truncated && !fs_control.stderr_truncated
            && fs_control.termination==ExecutionTermination::Exited && fs_control.exit_code==Some(1)
            && fs_ctrl.iter().any(|r|matches!(&r.event,Event::Write{path,result} if path=="/rootfs-canary" && result.allowed && result.errno==0));
        let child_ok=descendants.termination==ExecutionTermination::TimedOut && children.iter().any(|r| {
            if let Event::DescendantStarted{child_pid,parent_process_group,setsid:true,double_fork:true}=r.event {
                children.iter().any(|h| h.pid==child_pid && matches!(h.event,Event::Heartbeat{parent_pid:1,process_group} if process_group==child_pid && process_group!=parent_process_group))
            }else{false}
        });
        let cgroups_ok=normal(&cgroups) && groups.iter().any(|r|matches!(&r.event,Event::Cgroups(g) if reading(g,"memory.max")==Some("67108864") && reading(g,"pids.max")==Some("64") && reading(g,"cpu.max")==Some("50000 100000")));
        let caps=SandboxCapabilities {
            filesystem_isolated:fs_ok,
            network_isolated:network(&net,false) && network(&net_control,true),
            environment_isolated:env_ok,
            children_contained:child_ok,
            wall_time_limited:child_ok && descendants.duration_ms>=1500 && descendants.duration_ms<15000,
            output_limited:output.termination==ExecutionTermination::OutputLimit && (output.stdout_truncated || output.stderr_truncated) && output.stdout.len()<=4096 && output.stderr.len()<=4096,
            cpu_quota:cgroups_ok && normal(&cpu) && cpus.iter().any(|r|matches!(&r.event,Event::Cpu{before,after,maximum} if maximum.result.allowed && maximum.value=="50000 100000" && increased(before,after,"nr_throttled") && increased(before,after,"throttled_usec"))),
            memory_limited: memory_enforced(&memory, &ram, cgroups_ok),
            pids_limited:cgroups_ok && normal(&pids) && pid_records.iter().any(|r|matches!(&r.event,Event::Pids{started,cgroups} if *started>0 && *started<80 && reading(cgroups,"pids.max")==Some("64") && reading(cgroups,"pids.events").and_then(|s|counter(s,"max")).is_some_and(|n|n>0))),
            disk_limited:normal(&disk) && disks.iter().any(|r|matches!(&r.event,Event::Disk{bytes_written,enospc:true,result} if *bytes_written>0 && *bytes_written<=8388608 && !result.allowed && result.errno==28)),
        };
        let invalid_evidence = observations
            .iter()
            .filter(|o| o.scenario != S::Output && records(&o.execution).is_err())
            .map(|o| o.scenario)
            .collect();
        let configuration_fingerprint = self.configuration_fingerprint()?;
        let evidence = SandboxEvidence {
            configuration_fingerprint: configuration_fingerprint.clone(),
            capabilities: caps,
        };
        let strict = admit_execution(
            SandboxTier::Strict,
            false,
            false,
            &evidence,
            &configuration_fingerprint,
        )
        .is_ok();
        let restricted = admit_execution(
            SandboxTier::Restricted,
            false,
            false,
            &evidence,
            &configuration_fingerprint,
        )
        .is_ok();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ExecutionError::Infrastructure)?
            .as_millis()
            .try_into()
            .map_err(|_| ExecutionError::Infrastructure)?;
        Ok(CapabilityReport {
            status: if strict {
                CapabilityStatus::Verified
            } else {
                CapabilityStatus::Degraded
            },
            scope: "trusted_probe_image_only",
            observed_at_unix_ms: now,
            engine: self.engine().clone(),
            image_id: self.image_id().to_owned(),
            capabilities: caps,
            configuration_fingerprint,
            strict_available: strict,
            restricted_available: restricted,
            project_code_available: false,
            observations,
            invalid_evidence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    fn result(stdout: String) -> Result<ExecutionResult, Box<dyn std::error::Error>> {
        Ok(ExecutionResult {
            termination: ExecutionTermination::Exited,
            exit_code: Some(0),
            oom_killed: Some(false),
            stdout,
            stderr: String::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            duration_ms: 1,
            total_duration_ms: 2,
            execution_fingerprint: format!("sha256:{}", "a".repeat(64)).parse()?,
            platform: "linux/aarch64",
            image_id: "fixture".into(),
        })
    }
    fn sockets(allowed: bool) -> String {
        let mut text = String::new();
        for family in ["ipv4", "ipv6"] {
            for transport in ["tcp", "udp"] {
                for purpose in ["dns", "loopback"] {
                    text.push_str(&serde_json::json!({"pid":1,"event":"socket","details":{"family":family,"transport":transport,"purpose":purpose,"operation":"socket_only","result":{"allowed":allowed,"errno":if allowed {0}else{1}}}}).to_string());
                    text.push('\n');
                }
            }
        }
        for (family, transport) in [("unix", "stream"), ("netlink", "raw")] {
            text.push_str(&serde_json::json!({"pid":1,"event":"socket","details":{"family":family,"transport":transport,"purpose":"local","operation":"socket_only","result":{"allowed":allowed,"errno":if allowed{0}else{1}}}}).to_string());
            text.push('\n');
        }
        text
    }
    #[test]
    fn socket_oracle_requires_all_distinct_cases_and_correct_control() -> TestResult {
        let denied = result(sockets(false))?;
        let allowed = result(sockets(true))?;
        assert!(network(&denied, false));
        assert!(network(&allowed, true));
        assert!(!network(&denied, true));
        assert!(!network(&allowed, false));
        let line = denied.stdout.lines().next().ok_or("no record")?;
        assert!(!network(&result(format!("{line}\n").repeat(8))?, false));
        assert!(!network(
            &result(denied.stdout.lines().skip(1).collect::<Vec<_>>().join("\n"))?,
            false
        ));
        assert!(!network(&result("not json".into())?, false));
        Ok(())
    }
    #[test]
    fn ignored_details_do_not_break_known_evidence_but_bad_known_events_fail() -> TestResult {
        let r = result(
            r#"{"event":"rootfs_canary","pid":1,"details":{"world_writable":true}}"#.into(),
        )?;
        assert!(matches!(
            records(&r).map_err(|e| format!("{e:?}"))?[0].event,
            Event::Other
        ));
        assert!(
            records(&result(
                r#"{"event":"socket","pid":1,"details":{}}"#.into()
            )?)
            .is_err()
        );
        Ok(())
    }
    #[test]
    fn exit137_without_oom_or_allocation_evidence_never_proves_memory_limit() -> TestResult {
        let mut r =
            result(r#"{"event":"memory_allocated","pid":1,"details":{"bytes":8388608}}"#.into())?;
        r.exit_code = Some(137);
        let evidence = records(&r).map_err(|e| format!("{e:?}"))?;
        assert!(!memory_enforced(&r, &evidence, true));
        r.oom_killed = Some(true);
        assert!(memory_enforced(&r, &evidence, true));
        assert!(!memory_enforced(&r, &evidence, false));
        assert!(!memory_enforced(&r, &[], true));
        Ok(())
    }
    #[test]
    fn malformed_guest_evidence_is_a_failed_probe_not_infrastructure() -> TestResult {
        let evidence = result(r#"{"event":"memory_allocated","pid":1,"details":{}"#.into())?;
        assert!(probe_records(&evidence).is_empty());
        assert!(!memory_enforced(&evidence, &probe_records(&evidence), true));
        Ok(())
    }
    #[test]
    fn quota_without_actual_throttling_is_not_enforcement() {
        let reading = |value: &str| Reading {
            value: value.into(),
            result: Operation {
                allowed: true,
                errno: 0,
            },
        };
        assert!(!increased(
            &reading("nr_throttled 0"),
            &reading("nr_throttled 0"),
            "nr_throttled"
        ));
        assert!(increased(
            &reading("nr_throttled 0"),
            &reading("nr_throttled 1"),
            "nr_throttled"
        ));
        assert!(!increased(
            &reading("broken"),
            &reading("nr_throttled 1"),
            "nr_throttled"
        ));
    }
}
