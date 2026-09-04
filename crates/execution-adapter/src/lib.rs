//! Single product boundary for external processes. No shells or caller argv.
mod applied;
mod capabilities;
pub use capabilities::{CapabilityReport, CapabilityStatus};
mod state;
mod supervisor;

use rust_engineering_application::{
    ExecutionCancellation, ExecutionError, ExecutionPort, NeverCancel,
};
use rust_engineering_domain::{
    ExecutionFingerprint, ExecutionLimits, ExecutionResult, ExecutionSpec, ExecutionTermination,
    ProbeScenario,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use state::State;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use supervisor::{Capture, Stop};

#[derive(Clone, Debug)]
pub struct HostDockerConfig {
    pub executable: PathBuf,
    pub socket: PathBuf,
    pub state_root: PathBuf,
    pub image_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
pub struct EngineIdentity {
    #[serde(rename = "ID")]
    pub id: String,
    #[serde(rename = "ServerVersion")]
    pub version: String,
    #[serde(rename = "DefaultRuntime")]
    pub default_runtime: String,
    #[serde(rename = "OSType")]
    pub os: String,
    #[serde(rename = "Architecture")]
    pub architecture: String,
    #[serde(rename = "CgroupVersion")]
    pub cgroup_version: String,
    #[serde(rename = "SecurityOptions")]
    pub security_options: Vec<String>,
    #[serde(rename = "MemoryLimit")]
    memory_limit: bool,
    #[serde(rename = "SwapLimit")]
    swap_limit: bool,
    #[serde(rename = "CpuCfsQuota")]
    cpu_quota: bool,
    #[serde(rename = "PidsLimit")]
    pids_limit: bool,
}

#[derive(Deserialize)]
struct Image {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Os")]
    os: String,
    #[serde(rename = "Architecture")]
    arch: String,
    #[serde(rename = "Config")]
    config: ImageConfig,
}
#[derive(Deserialize)]
struct ImageConfig {
    #[serde(rename = "Env")]
    env: Option<Vec<String>>,
    #[serde(rename = "Volumes")]
    volumes: Option<std::collections::BTreeMap<String, serde_json::Value>>,
    #[serde(rename = "OnBuild")]
    on_build: Option<Vec<String>>,
}
#[derive(Deserialize)]
struct Container {
    #[serde(rename = "State")]
    state: ContainerState,
}
#[derive(Deserialize)]
struct ContainerState {
    #[serde(rename = "Running")]
    running: bool,
    #[serde(rename = "Pid")]
    pid: u64,
    #[serde(rename = "ExitCode")]
    exit_code: i32,
    #[serde(rename = "Status")]
    status: String,
    #[serde(rename = "StartedAt")]
    started_at: String,
    #[serde(rename = "Error")]
    error: String,
    #[serde(rename = "OOMKilled")]
    oom_killed: bool,
}

impl ContainerState {
    fn completed(&self, cli_code: Option<i32>) -> bool {
        !self.running
            && self.pid == 0
            && self.status == "exited"
            && !self.started_at.is_empty()
            && !self.started_at.starts_with("0001-")
            && self.error.is_empty()
            && cli_code == Some(self.exit_code)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
enum Profile {
    Enforced,
    SocketControl,
    WritableControl,
}

impl Profile {
    fn permits(self, scenario: ProbeScenario) -> bool {
        self == Self::Enforced
            || (self == Self::SocketControl && scenario == ProbeScenario::Network)
            || (self == Self::WritableControl && scenario == ProbeScenario::Filesystem)
    }
}

pub struct DockerGateway {
    config: HostDockerConfig,
    state: State,
    engine: EngineIdentity,
    executable_digest: String,
    busy: Mutex<()>,
    quarantined: AtomicBool,
}

fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    let mut text = String::from("sha256:");
    for byte in hash {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

impl DockerGateway {
    pub fn new(config: HostDockerConfig) -> Result<Self, ExecutionError> {
        if !state::valid_path(&config.executable)
            || !state::valid_path(&config.socket)
            || config.image_id.len() != 71
            || !config.image_id.starts_with("sha256:")
            || !config.image_id[7..]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ExecutionError::InvalidConfiguration);
        }
        let executable_digest = digest(&state::executable_bytes(&config.executable)?);
        let state = State::new(&config.state_root)?;
        let info = Self::control_raw(
            &config,
            &state,
            &["info".into(), "--format".into(), "{{json .}}".into()],
        )?;
        if info.code != Some(0) {
            return Err(ExecutionError::Unavailable);
        }
        let engine: EngineIdentity =
            serde_json::from_slice(&info.stdout).map_err(|_| ExecutionError::Unavailable)?;
        if engine.default_runtime != "runc"
            || engine.os != "linux"
            || engine.architecture != "aarch64"
            || engine.cgroup_version != "2"
            || !engine
                .security_options
                .iter()
                .any(|s| s.starts_with("name=seccomp"))
            || !engine.memory_limit
            || !engine.swap_limit
            || !engine.cpu_quota
            || !engine.pids_limit
        {
            return Err(ExecutionError::Unavailable);
        }
        let image = Self::control_raw(
            &config,
            &state,
            &["image".into(), "inspect".into(), config.image_id.clone()],
        )?;
        let images: Vec<Image> =
            serde_json::from_slice(&image.stdout).map_err(|_| ExecutionError::Unavailable)?;
        let approved = images
            .first()
            .filter(|i| {
                images.len() == 1 && i.id == config.image_id && i.os == "linux" && i.arch == "arm64"
            })
            .ok_or(ExecutionError::InvalidConfiguration)?;
        // Docker's scratch builder supplies PATH. Only that default is accepted;
        // the generated container configuration always overrides it.
        if approved.config.env.as_ref().is_some_and(|e| {
            e.iter()
                .any(|v| v != "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
                || e.len() > 1
        }) || approved
            .config
            .volumes
            .as_ref()
            .is_some_and(|v| !v.is_empty())
            || approved
                .config
                .on_build
                .as_ref()
                .is_some_and(|v| !v.is_empty())
        {
            return Err(ExecutionError::InvalidConfiguration);
        }
        let existing = Self::control_raw(
            &config,
            &state,
            &[
                "container".into(),
                "ls".into(),
                "--all".into(),
                "--filter".into(),
                "label=org.rust-mcp.execution=true".into(),
                "--format".into(),
                "{{.ID}}".into(),
            ],
        )?;
        if existing.code != Some(0) || !existing.stdout.iter().all(u8::is_ascii_whitespace) {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(Self {
            config,
            state,
            engine,
            executable_digest,
            busy: Mutex::new(()),
            quarantined: AtomicBool::new(false),
        })
    }
    /// Covers every generated hardening argument for all closed scenarios.
    pub fn configuration_fingerprint(&self) -> Result<ExecutionFingerprint, ExecutionError> {
        let mut commands = ProbeScenario::ALL
            .iter()
            .map(|scenario| {
                self.create_arguments(
                    "<container>",
                    &ExecutionSpec {
                        scenario: *scenario,
                        limits: ExecutionLimits::default(),
                    },
                    "<seccomp-profile>",
                    Profile::Enforced,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        commands.push(self.create_arguments(
            "<container>",
            &ExecutionSpec {
                scenario: ProbeScenario::Network,
                limits: ExecutionLimits::default(),
            },
            "<seccomp-profile>",
            Profile::SocketControl,
        )?);
        commands.push(self.create_arguments(
            "<container>",
            &ExecutionSpec {
                scenario: ProbeScenario::Filesystem,
                limits: ExecutionLimits::default(),
            },
            "<seccomp-profile>",
            Profile::WritableControl,
        )?);
        configuration_identity(&self.engine, &self.executable_digest, &commands)
    }
    pub fn engine(&self) -> &EngineIdentity {
        &self.engine
    }
    pub fn image_id(&self) -> &str {
        &self.config.image_id
    }
    pub fn is_quarantined(&self) -> bool {
        self.quarantined.load(Ordering::Acquire)
    }

    fn command(
        config: &HostDockerConfig,
        state: &State,
    ) -> Result<std::process::Command, ExecutionError> {
        state.check()?;
        let mut command = std::process::Command::new(&config.executable);
        command
            .env_clear()
            .current_dir(state.path())
            .arg("--config")
            .arg(state.path())
            .arg("--host")
            .arg(format!(
                "unix://{}",
                config
                    .socket
                    .to_str()
                    .ok_or(ExecutionError::InvalidConfiguration)?
            ));
        Ok(command)
    }
    fn control_raw(
        config: &HostDockerConfig,
        state: &State,
        args: &[String],
    ) -> Result<Capture, ExecutionError> {
        let mut command = Self::command(config, state)?;
        command.args(args);
        let result = supervisor::run(command, Duration::from_secs(10), 256 * 1024, &NeverCancel)?;
        if result.stop != Stop::Exited || result.stdout_truncated || result.stderr_truncated {
            return Err(ExecutionError::Infrastructure);
        }
        Ok(result)
    }
    fn control(&self, args: &[String]) -> Result<Capture, ExecutionError> {
        Self::control_raw(&self.config, &self.state, args)
    }
    fn remove(&self, name: &str) -> Result<(), ExecutionError> {
        let result = self.control(&[
            "container".into(),
            "rm".into(),
            "--force".into(),
            name.into(),
        ]);
        if !matches!(result,Ok(c) if c.code==Some(0)) {
            self.quarantined.store(true, Ordering::Release);
            return Err(ExecutionError::CleanupUncertain);
        }
        let absent = self.control(&[
            "container".into(),
            "ls".into(),
            "--all".into(),
            "--filter".into(),
            format!("name=^/{name}$"),
            "--format".into(),
            "{{.ID}}".into(),
        ]);
        if !matches!(absent,Ok(c) if c.code==Some(0) && c.stdout.iter().all(u8::is_ascii_whitespace))
        {
            self.quarantined.store(true, Ordering::Release);
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(())
    }
    fn create_arguments(
        &self,
        name: &str,
        spec: &ExecutionSpec,
        profile: &str,
        mode: Profile,
    ) -> Result<Vec<String>, ExecutionError> {
        if !mode.permits(spec.scenario) {
            return Err(ExecutionError::Denied);
        }
        let memory = if spec.scenario == rust_engineering_domain::ProbeScenario::Pids {
            "256m"
        } else {
            "64m"
        };
        let mut args = [
            "container",
            "create",
            "--pull=never",
            "--runtime=runc",
            "--init=false",
            "--name",
            name,
            "--label",
            "org.rust-mcp.execution=true",
            "--network=none",
            "--read-only",
            "--user=65532:65532",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges=true",
            "--ipc=private",
            "--cgroupns=private",
            "--pids-limit=64",
            "--cpus=0.5",
            "--memory",
            memory,
            "--memory-swap",
            memory,
            "--shm-size=1m",
            "--log-driver=none",
            "--no-healthcheck",
            "--tmpfs",
            "/work:rw,nosuid,nodev,size=8m,mode=1777",
            "--tmpfs",
            "/tmp:rw,nosuid,nodev,noexec,size=8m,mode=1777",
            "--workdir=/work",
            "--hostname=sandbox",
            "--env=PATH=/nonexistent",
            "--env=HOME=/work",
            "--env=TMPDIR=/tmp",
            "--env=GOMAXPROCS=2",
            "--entrypoint=/mcp-probe",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        args.push(format!("--security-opt=seccomp={profile}"));
        if mode == Profile::WritableControl {
            args.retain(|arg| arg != "--read-only");
        }
        args.push(self.config.image_id.clone());
        args.push(spec.scenario.argument().to_owned());
        Ok(args)
    }
}

impl ExecutionPort for DockerGateway {
    fn execute(
        &self,
        spec: &ExecutionSpec,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<ExecutionResult, ExecutionError> {
        self.execute_profile(spec, cancel, Profile::Enforced)
    }
}
impl DockerGateway {
    fn execute_profile(
        &self,
        spec: &ExecutionSpec,
        cancel: &dyn ExecutionCancellation,
        mode: Profile,
    ) -> Result<ExecutionResult, ExecutionError> {
        let _busy = match self.busy.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => return Err(ExecutionError::Busy),
            Err(std::sync::TryLockError::Poisoned(_)) => {
                self.quarantined.store(true, Ordering::Release);
                return Err(ExecutionError::CleanupUncertain);
            }
        };
        if self.is_quarantined() {
            return Err(ExecutionError::CleanupUncertain);
        }
        if cancel.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        if digest(&state::executable_bytes(&self.config.executable)?) != self.executable_digest {
            return Err(ExecutionError::Unavailable);
        }
        let engine = self.control(&["info".into(), "--format".into(), "{{json .}}".into()])?;
        let observed: EngineIdentity =
            serde_json::from_slice(&engine.stdout).map_err(|_| ExecutionError::Unavailable)?;
        if observed != self.engine {
            return Err(ExecutionError::Unavailable);
        }
        let started = Instant::now();
        let name = format!("rust-mcp-run-{}", state::nonce()?);
        let profile = self.state.path().join(if mode == Profile::SocketControl {
            "seccomp-socket.json"
        } else {
            "seccomp.json"
        });
        let args = self.create_arguments(
            &name,
            spec,
            profile
                .to_str()
                .ok_or(ExecutionError::InvalidConfiguration)?,
            mode,
        )?;
        let created = self.control(&args);
        if !matches!(created,Ok(ref c) if c.code==Some(0)) {
            // The daemon may have accepted create even when its reply was lost.
            self.remove(&name)?;
            return Err(ExecutionError::Infrastructure);
        }
        let applied = self
            .control(&["container".into(), "inspect".into(), name.clone()])
            .and_then(|c| {
                if c.code != Some(0) {
                    return Err(ExecutionError::Infrastructure);
                }
                applied::verify_profile(&c.stdout, spec.scenario, &self.config.image_id, mode)
            });
        if let Err(error) = applied {
            self.remove(&name)?;
            return Err(error);
        }
        if cancel.is_cancelled() {
            self.remove(&name)?;
            return Err(ExecutionError::Cancelled);
        }
        let work = (|| {
            let mut command = Self::command(&self.config, &self.state)?;
            command.args(["container", "start", "--attach", &name]);
            supervisor::run(
                command,
                Duration::from_millis(spec.limits.wall_ms()),
                spec.limits.output_bytes(),
                cancel,
            )
        })();
        let outcome = match work {
            Ok(c) => c,
            Err(e) => {
                self.remove(&name)?;
                return Err(e);
            }
        };
        let mut termination = match outcome.stop {
            Stop::Exited => ExecutionTermination::Exited,
            Stop::TimedOut => ExecutionTermination::TimedOut,
            Stop::Cancelled => ExecutionTermination::Cancelled,
            Stop::OutputLimit => ExecutionTermination::OutputLimit,
        };
        let (exit_code, oom_killed) = if outcome.stop == Stop::Exited {
            let inspected = self.control(&["container".into(), "inspect".into(), name.clone()]);
            let parsed = inspected.and_then(|c| {
                serde_json::from_slice::<Vec<Container>>(&c.stdout)
                    .map_err(|_| ExecutionError::Infrastructure)
            });
            match parsed {
                Ok(c) if c.len() == 1 && c[0].state.completed(outcome.code) => {
                    (Some(c[0].state.exit_code), Some(c[0].state.oom_killed))
                }
                _ => {
                    self.remove(&name)?;
                    return Err(ExecutionError::Infrastructure);
                }
            }
        } else {
            (None, None)
        };
        self.remove(&name)?;
        let identity = serde_json::to_vec(&(
            self.configuration_fingerprint()?,
            spec.scenario,
            spec.limits,
            mode,
        ))
        .map_err(|_| ExecutionError::Infrastructure)?;
        let execution_fingerprint = digest(&identity)
            .parse()
            .map_err(|_| ExecutionError::Infrastructure)?;
        let (stdout, stdout_expanded) = bounded_text(&outcome.stdout, spec.limits.output_bytes());
        let (stderr, stderr_expanded) = bounded_text(&outcome.stderr, spec.limits.output_bytes());
        if stdout_expanded || stderr_expanded {
            termination = ExecutionTermination::OutputLimit;
        }
        Ok(ExecutionResult {
            termination,
            exit_code,
            oom_killed,
            stdout,
            stderr,
            stdout_truncated: outcome.stdout_truncated || stdout_expanded,
            stderr_truncated: outcome.stderr_truncated || stderr_expanded,
            duration_ms: outcome.duration_ms,
            total_duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            execution_fingerprint,
            platform: "linux/aarch64",
            image_id: self.config.image_id.clone(),
        })
    }
}

fn configuration_identity(
    engine: &EngineIdentity,
    executable: &str,
    commands: &[Vec<String>],
) -> Result<ExecutionFingerprint, ExecutionError> {
    let identity = serde_json::to_vec(&(
        engine,
        executable,
        commands,
        include_str!("seccomp.json"),
        include_str!("seccomp-socket.json"),
        "docker-probe-profile-v1",
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    digest(&identity)
        .parse()
        .map_err(|_| ExecutionError::Infrastructure)
}

fn bounded_text(bytes: &[u8], limit: usize) -> (String, bool) {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    let expanded = text.len() > limit;
    if expanded {
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    (text, expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn never_started_and_runtime_errors_are_not_normal_process_exits()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut state: ContainerState = serde_json::from_value(
            serde_json::json!({"OOMKilled":false,"Running":false,"Pid":0,"ExitCode":0,"Status":"created","StartedAt":"0001-01-01T00:00:00Z","Error":""}),
        )?;
        assert!(!state.completed(Some(0)));
        state.status = "exited".into();
        assert!(!state.completed(Some(0)));
        state.started_at = "2026-09-03T20:00:00Z".into();
        assert!(state.completed(Some(0)));
        state.exit_code = 7;
        assert!(state.completed(Some(7)));
        assert!(!state.completed(Some(0)));
        state.error = "runtime failed".into();
        assert!(!state.completed(Some(7)));
        Ok(())
    }
    #[test]
    fn replacement_characters_cannot_expand_the_output_budget() {
        let (text, truncated) = bounded_text(&[255; 1024], 1024);
        assert!(truncated);
        assert_eq!(text.len(), 1023);
        assert_eq!(bounded_text(b"ok", 1024), ("ok".to_owned(), false));
    }
    #[test]
    fn configuration_identity_covers_every_generated_argument()
    -> Result<(), Box<dyn std::error::Error>> {
        let engine: EngineIdentity = serde_json::from_value(
            serde_json::json!({"ID":"fixture","DefaultRuntime":"runc","ServerVersion":"29.7.2","OSType":"linux","Architecture":"aarch64","CgroupVersion":"2","SecurityOptions":["name=seccomp"],"MemoryLimit":true,"SwapLimit":true,"CpuCfsQuota":true,"PidsLimit":true}),
        )?;
        let commands = vec![vec![
            "--read-only".into(),
            "--pids-limit=64".into(),
            "--network=none".into(),
        ]];
        let base = configuration_identity(&engine, "docker-digest", &commands)
            .map_err(|e| format!("{e:?}"))?;
        for index in 0..commands[0].len() {
            let mut changed = commands.clone();
            changed[0].remove(index);
            assert_ne!(
                base,
                configuration_identity(&engine, "docker-digest", &changed)
                    .map_err(|e| format!("{e:?}"))?
            );
        }
        Ok(())
    }
}

mod rust_applied;
mod rust_gateway;
mod source_archive;
pub use rust_gateway::{APPROVED_RUST_IMAGE, RustGateway};

mod rust_calibration;

pub use rust_calibration::{RustCalibrationObservation, RustCalibrationReport};

mod cargo_diagnostics;
mod project_inspection;
mod project_metadata;
pub use project_inspection::RustProjectInspector;

mod toolchain_metadata;

mod format_output;
