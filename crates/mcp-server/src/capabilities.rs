use rust_engineering_domain::SandboxCapabilities;
use rust_engineering_execution::{
    CapabilityReport, CapabilityStatus, DockerGateway, HostDockerConfig,
};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

pub(crate) struct Invocation {
    pub(crate) config: HostDockerConfig,
    pub(crate) json: bool,
}

pub fn parse(args: impl Iterator<Item = OsString>) -> Option<Invocation> {
    let mut args = args;
    let (mut executable, mut socket, mut state_root, mut image_id) = (None, None, None, None);
    let mut json = None;
    while let Some(flag) = args.next() {
        if matches!(flag.to_str()?, "--json" | "--human") {
            if json.is_some() {
                return None;
            }
            json = Some(flag == "--json");
            continue;
        }
        let value = args.next()?;
        match flag.to_str()? {
            "--docker" if executable.is_none() => executable = Some(PathBuf::from(value)),
            "--docker-socket" if socket.is_none() => socket = Some(PathBuf::from(value)),
            "--state-root" if state_root.is_none() => state_root = Some(PathBuf::from(value)),
            "--probe-image" if image_id.is_none() => image_id = Some(value.into_string().ok()?),
            _ => return None,
        }
    }
    Some(Invocation {
        json: json.unwrap_or(true),
        config: HostDockerConfig {
            executable: executable?,
            socket: socket?,
            state_root: state_root?,
            image_id: image_id?,
        },
    })
}
#[derive(Serialize)]
struct Unavailable {
    status: &'static str,
    scope: &'static str,
    capabilities: SandboxCapabilities,
    strict_available: bool,
    restricted_available: bool,
    project_code_available: bool,
    reason: String,
}
pub fn run(invocation: Invocation) -> ExitCode {
    let result = DockerGateway::new(invocation.config).and_then(|g| g.probe_capabilities());
    let (bytes, code) = match result {
        Ok(report) => {
            let code = if report.strict_available { 0 } else { 1 };
            (
                if invocation.json {
                    serde_json::to_vec(&report)
                } else {
                    Ok(human_report(&report).into_bytes())
                },
                code,
            )
        }
        Err(error) => {
            let report = Unavailable {
                status: "unavailable",
                scope: "trusted_probe_image_only",
                capabilities: SandboxCapabilities::default(),
                strict_available: false,
                restricted_available: false,
                project_code_available: false,
                reason: format!("{error:?}"),
            };
            (
                if invocation.json {
                    serde_json::to_vec(&report)
                } else {
                    Ok(human_unavailable(&report).into_bytes())
                },
                1,
            )
        }
    };
    let Ok(mut bytes) = bytes else {
        return ExitCode::FAILURE;
    };
    bytes.push(b'\n');
    if io::stdout().lock().write_all(&bytes).is_err() {
        ExitCode::FAILURE
    } else {
        ExitCode::from(code)
    }
}

fn human_capabilities(capabilities: &SandboxCapabilities) -> String {
    [
        ("Filesystem isolated", capabilities.filesystem_isolated),
        ("Network isolated", capabilities.network_isolated),
        ("Environment isolated", capabilities.environment_isolated),
        ("Children contained", capabilities.children_contained),
        ("Wall time limited", capabilities.wall_time_limited),
        ("Output limited", capabilities.output_limited),
        ("CPU quota", capabilities.cpu_quota),
        ("Memory limited", capabilities.memory_limited),
        ("PIDs limited", capabilities.pids_limited),
        ("Disk limited", capabilities.disk_limited),
    ]
    .map(|(name, available)| format!("  {name}: {available}"))
    .join("\n")
}
fn human_report(report: &CapabilityReport) -> String {
    let status = match report.status {
        CapabilityStatus::Verified => "verified",
        CapabilityStatus::Degraded => "degraded",
    };
    format!(
        "Sandbox capabilities: {status}\nScope: {}\nObserved at (Unix ms): {}\nConfiguration: {}\nStrict available: {}\nRestricted available: {}\nProject code available: {}\n{}\nObservations: {}\nInvalid evidence: {}",
        report.scope,
        report.observed_at_unix_ms,
        report.configuration_fingerprint,
        report.strict_available,
        report.restricted_available,
        report.project_code_available,
        human_capabilities(&report.capabilities),
        report.observations.len(),
        report.invalid_evidence.len()
    )
}
fn human_unavailable(report: &Unavailable) -> String {
    format!(
        "Sandbox capabilities: {}\nScope: {}\nReason: {}\nStrict available: {}\nRestricted available: {}\nProject code available: {}\n{}",
        report.status,
        report.scope,
        report.reason,
        report.strict_available,
        report.restricted_available,
        report.project_code_available,
        human_capabilities(&report.capabilities)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn format_flags_are_optional_unique_and_do_not_replace_configuration()
    -> Result<(), Box<dyn std::error::Error>> {
        let base = [
            "--docker",
            "/fixed/docker",
            "--docker-socket",
            "/fixed/socket",
            "--state-root",
            "/fixed/state",
            "--probe-image",
            "invalid-for-parser-only",
        ];
        let parse_values = |values: Vec<&str>| parse(values.into_iter().map(OsString::from));
        assert!(parse_values(base.to_vec()).is_some_and(|v| v.json));
        for (flag, json) in [("--json", true), ("--human", false)] {
            let mut args = vec![flag];
            args.extend(base);
            let invocation = parse_values(args).ok_or("complete fixture")?;
            assert_eq!(invocation.json, json);
            assert_eq!(invocation.config.executable, PathBuf::from("/fixed/docker"));
        }
        for flags in [
            vec!["--json", "--json"],
            vec!["--human", "--human"],
            vec!["--json", "--human"],
            vec!["--human", "--json"],
            vec!["--unknown"],
        ] {
            let mut args = base.to_vec();
            args.extend(flags);
            assert!(parse_values(args).is_none());
        }
        for args in [
            vec![],
            vec!["--human"],
            vec!["--json"],
            vec!["--docker", "/fixed/docker"],
        ] {
            assert!(parse_values(args).is_none());
        }
        let mut duplicate = base.to_vec();
        duplicate.extend(["--docker", "/other"]);
        assert!(parse_values(duplicate).is_none());
        Ok(())
    }
    #[test]
    fn human_summary_uses_the_same_fixed_report() -> Result<(), Box<dyn std::error::Error>> {
        let report = CapabilityReport {
            status: CapabilityStatus::Degraded,
            scope: "trusted_probe_image_only",
            observed_at_unix_ms: 42,
            engine: serde_json::from_value(
                serde_json::json!({"ID":"fixture","ServerVersion":"1","DefaultRuntime":"runc","OSType":"linux","Architecture":"aarch64","CgroupVersion":"2","SecurityOptions":[],"MemoryLimit":true,"SwapLimit":true,"CpuCfsQuota":true,"PidsLimit":true}),
            )?,
            image_id: format!("sha256:{}", "1".repeat(64)),
            capabilities: SandboxCapabilities {
                network_isolated: true,
                ..Default::default()
            },
            configuration_fingerprint: format!("sha256:{}", "2".repeat(64)).parse()?,
            strict_available: false,
            restricted_available: false,
            project_code_available: false,
            observations: vec![],
            invalid_evidence: vec![rust_engineering_domain::ProbeScenario::Network],
        };
        let human = human_report(&report);
        assert!(human.starts_with("Sandbox capabilities: degraded\nScope: trusted_probe_image_only\nObserved at (Unix ms): 42\n"));
        assert!(human.contains("Network isolated: true"));
        assert!(human.contains("Filesystem isolated: false"));
        assert!(human.ends_with("Observations: 0\nInvalid evidence: 1"));
        assert!(human.contains("Project code available: false"));
        Ok(())
    }
}
