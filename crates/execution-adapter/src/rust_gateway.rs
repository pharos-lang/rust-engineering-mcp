//! Cargo/source capability is distinct from the Go probe capability.
use super::*;
use rust_engineering_domain::{RustCommand, SourceBundle};
use std::collections::BTreeMap;

pub const APPROVED_RUST_IMAGE: &str =
    "sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909";
// These versions were verified during explicit provisioning of the immutable ID.
// Changing the approved identity requires updating and reverifying this tuple.
pub(super) const APPROVED_RUST_VERSION: &str = "1.98.1";
pub(super) const APPROVED_CARGO_VERSION: &str = "1.98.1";
#[derive(Clone, Debug)]
pub(super) enum Phase {
    Ingest,
    Run(RustCommand),
}
impl Phase {
    pub(super) fn ingesting(&self) -> bool {
        matches!(self, Self::Ingest)
    }
    pub(super) fn user(&self) -> &'static str {
        if self.ingesting() {
            "0:0"
        } else {
            "65534:65534"
        }
    }
    pub(super) fn program(&self) -> &'static str {
        match self {
            Self::Ingest => "/usr/bin/tar",
            Self::Run(RustCommand::CompilerVersion | RustCommand::Explain(_)) => {
                "/opt/rust/bin/rustc"
            }
            Self::Run(RustCommand::InstalledComponents) => "/usr/bin/cat",
            Self::Run(_) => "/opt/rust/bin/cargo",
        }
    }
    pub(super) fn arguments(&self) -> Vec<String> {
        let args: &[&str] = match self {
            Self::Ingest => &[
                "--extract",
                "--file=-",
                "--directory=/source",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::Run(RustCommand::FormatCheck) => &[
                "fmt",
                "--all",
                "--check",
                "--",
                "--color",
                "never",
                "--config",
                "disable_all_formatting=false",
            ],
            Self::Run(RustCommand::Metadata) => {
                &["metadata", "--format-version=1", "--no-deps", "--frozen"]
            }
            Self::Run(RustCommand::TestProject(_)) => &[
                "test",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--color=never",
            ],
            Self::Run(RustCommand::ClippyProject(_)) => {
                &["clippy", "--frozen", "--message-format=json", "--jobs=1"]
            }
            Self::Run(RustCommand::Check | RustCommand::CheckProject(_)) => {
                &["check", "--frozen", "--message-format=json", "--jobs=1"]
            }
            Self::Run(RustCommand::InstalledComponents) => {
                &["--", "/opt/rust/lib/rustlib/components"]
            }
            Self::Run(RustCommand::CompilerVersion | RustCommand::CargoVersion) => {
                &["--version", "--verbose"]
            }
            Self::Run(RustCommand::Explain(code)) => {
                return vec![
                    "--explain".into(),
                    code.to_string(),
                    "--color".into(),
                    "never".into(),
                ];
            }
        };
        let mut args = args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        if let Self::Run(RustCommand::CheckProject(options)) = self {
            if let Some(package) = options.package() {
                args.push(format!("--package={package}"));
            }
            if options.workspace() {
                args.push("--workspace".into());
            }
            if !options.features().is_empty() {
                args.push(format!("--features={}", options.features().join(",")));
            }
            if options.all_features() {
                args.push("--all-features".into());
            }
            if options.no_default_features() {
                args.push("--no-default-features".into());
            }
            if options.all_targets() {
                args.push("--all-targets".into());
            }
            if let Some(target) = options.target() {
                args.push(format!("--target={target}"));
            }
        }
        if let Self::Run(RustCommand::ClippyProject(options)) = self {
            if let Some(package) = options.package() {
                args.push(format!("--package={package}"));
            }
            if options.workspace() {
                args.push("--workspace".into());
            }
            if !options.features().is_empty() {
                args.push(format!("--features={}", options.features().join(",")));
            }
            if options.all_targets() {
                args.push("--all-targets".into());
            }
            use rust_engineering_domain::LintProfile;
            match options.lint_profile() {
                LintProfile::Default | LintProfile::Project => (),
                LintProfile::Strict => args.extend(["--", "-D", "warnings"].map(str::to_owned)),
                LintProfile::Pedantic => {
                    args.extend(["--", "-W", "clippy::pedantic"].map(str::to_owned))
                }
            }
        }
        if let Self::Run(RustCommand::TestProject(options)) = self {
            if let Some(package) = options.package() {
                args.push(format!("--package={package}"));
            }
            if !options.features().is_empty() {
                args.push(format!("--features={}", options.features().join(",")));
            }
            if options.all_features() {
                args.push("--all-features".into());
            }
            if let Some(target) = options.target() {
                args.push(format!("--target={target}"));
            }
            if let Some(filter) = options.test_filter() {
                args.push(filter.to_owned());
            }
            args.extend(["--", "--test-threads=1", "--color=never"].map(str::to_owned));
        }
        args
    }
}
pub(super) fn environment() -> Vec<String> {
    let mut env = [
        "PATH=/opt/rust/bin:/usr/bin:/bin",
        "HOME=/work",
        "TMPDIR=/tmp",
        "CARGO_HOME=/work/cargo",
        "CARGO_TARGET_DIR=/work/target",
        "CARGO_INCREMENTAL=0",
        "CARGO_NET_OFFLINE=true",
        "RUSTC=/opt/rust/bin/rustc",
        "RUSTDOC=/opt/rust/bin/rustdoc",
        "RUSTFMT=/opt/rust/bin/rustfmt",
    ]
    .map(str::to_owned)
    .to_vec();
    env.sort();
    env
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct Volume {
    pub(super) name: String,
    driver: String,
    scope: String,
    options: Option<BTreeMap<String, String>>,
    labels: BTreeMap<String, String>,
    pub(super) mountpoint: String,
    cluster_volume: Option<serde_json::Value>,
    status: Option<serde_json::Value>,
}
impl Volume {
    fn parse(bytes: &[u8], name: &str, nonce: &str) -> Result<Self, ExecutionError> {
        let mut volumes: Vec<Self> =
            serde_json::from_slice(bytes).map_err(|_| ExecutionError::Infrastructure)?;
        if volumes.len() != 1 {
            return Err(ExecutionError::InvalidConfiguration);
        }
        let v = volumes.pop().ok_or(ExecutionError::Infrastructure)?;
        if v.name != name
            || v.driver != "local"
            || v.scope != "local"
            || v.options.as_ref().is_some_and(|v| !v.is_empty())
            || v.labels != labels(nonce)
            || !v.mountpoint.starts_with("/var/lib/docker/volumes/")
            || !v.mountpoint.ends_with("/_data")
            || v.cluster_volume.is_some()
            || v.status.is_some()
        {
            return Err(ExecutionError::InvalidConfiguration);
        }
        Ok(v)
    }
}
fn labels(nonce: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("org.rust-mcp.execution".into(), "true".into()),
        ("org.rust-mcp.rust-job".into(), nonce.into()),
    ])
}

struct PhaseRequest<'a> {
    name: &'a str,
    nonce: &'a str,
    volume: &'a Volume,
    phase: &'a Phase,
}
fn implementation_fingerprint() -> String {
    let sources: &[&[u8]] = &[
        include_bytes!("rust_gateway.rs"),
        include_bytes!("project_inspection.rs"),
        include_bytes!("rust_applied.rs"),
        include_bytes!("rust_calibration.rs"),
        include_bytes!("source_archive.rs"),
        include_bytes!("supervisor.rs"),
        include_bytes!("state.rs"),
        include_bytes!("lib.rs"),
        include_bytes!("../../domain/src/source.rs"),
        include_bytes!("../../domain/src/rust_execution.rs"),
        include_bytes!("../../domain/src/check.rs"),
        include_bytes!("../../domain/src/clippy.rs"),
        include_bytes!("../../domain/src/test_run.rs"),
        include_bytes!("../../domain/src/explain.rs"),
        include_bytes!("../../domain/src/value.rs"),
        include_bytes!("../../../Cargo.lock"),
        include_bytes!("../../../rust-toolchain.toml"),
    ];
    let mut hash = Sha256::new();
    for source in sources {
        hash.update(source.len().to_le_bytes());
        hash.update(source);
    }
    format!(
        "sha256:{}",
        hash.finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn finish_work(
    work: Result<(Capture, Option<bool>), ExecutionError>,
    terminal_signal: Option<Stop>,
) -> Result<(Capture, Option<bool>), ExecutionError> {
    // Cleanup may succeed after a verifier/control failure. Cancellation cannot
    // turn that earlier failure into evidence of a contained timed-out process.
    let (mut outcome, oom_killed) = work?;
    if let Some(stop) = terminal_signal
        && matches!(outcome.stop, Stop::Exited | Stop::Cancelled)
    {
        outcome.stop = stop;
        outcome.code = None;
    }
    Ok((outcome, oom_killed))
}

pub(super) enum Admission<'a> {
    Project,
    Calibration(Option<&'a Mutex<Option<String>>>),
}
struct WorkBudget<'a> {
    started: Instant,
    deadline: Instant,
    limits: ExecutionLimits,
    cancel: &'a dyn ExecutionCancellation,
}
impl WorkBudget<'_> {
    fn stop(&self) -> Option<Stop> {
        if self.cancel.is_cancelled() {
            Some(Stop::Cancelled)
        } else if Instant::now() >= self.deadline {
            Some(Stop::TimedOut)
        } else {
            None
        }
    }
    fn stopped_capture(&self, stop: Stop) -> Capture {
        Capture {
            code: None,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stop,
            duration_ms: self
                .started
                .elapsed()
                .as_millis()
                .try_into()
                .unwrap_or(u64::MAX),
        }
    }
}
impl ExecutionCancellation for WorkBudget<'_> {
    fn is_cancelled(&self) -> bool {
        self.stop().is_some()
    }
}

pub struct RustGateway {
    inner: DockerGateway,
    verified: AtomicBool,
    pub(super) calibrating: AtomicBool,
}
impl RustGateway {
    pub fn new(config: HostDockerConfig) -> Result<Self, ExecutionError> {
        if config.image_id != APPROVED_RUST_IMAGE {
            return Err(ExecutionError::InvalidConfiguration);
        }
        let inner = DockerGateway::new(config)?;
        let existing = inner.control(&[
            "volume".into(),
            "ls".into(),
            "--filter=label=org.rust-mcp.execution=true".into(),
            "--format={{.Name}}".into(),
        ])?;
        if existing.code != Some(0) || !existing.stdout.iter().all(u8::is_ascii_whitespace) {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(Self {
            inner,
            verified: AtomicBool::new(false),
            calibrating: AtomicBool::new(false),
        })
    }
    pub(super) fn set_verified(&self, verified: bool) {
        self.verified.store(verified, Ordering::Release);
    }
    pub(super) fn detached_observation(
        &self,
        name: &str,
    ) -> Result<Option<String>, ExecutionError> {
        if self.absent("container", name)? {
            return Ok(None);
        }
        let suffix = name
            .strip_prefix("rust-mcp-cargo-")
            .ok_or(ExecutionError::Infrastructure)?;
        if suffix.len() != 32 || !suffix.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ExecutionError::Infrastructure);
        }
        if let Err(error) = self.owned_container(name, suffix) {
            if self.absent("container", name)? {
                return Ok(None);
            }
            return Err(error);
        }
        let top = self.inner.control(&[
            "container".into(),
            "top".into(),
            name.into(),
            "-eo".into(),
            "pid,ppid,pgid,sid,args".into(),
        ])?;
        // Completion can race observation. It cannot authorize a capability.
        if top.code != Some(0) {
            return Ok(None);
        }
        let top = String::from_utf8(top.stdout).map_err(|_| ExecutionError::Infrastructure)?;
        let mut sessions = std::collections::BTreeSet::new();
        for line in top.lines().skip(1) {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            if fields.len() >= 5 && fields[4].ends_with("/build-script-build") {
                let pid = fields[0]
                    .parse::<u64>()
                    .map_err(|_| ExecutionError::Infrastructure)?;
                let sid = fields[3]
                    .parse::<u64>()
                    .map_err(|_| ExecutionError::Infrastructure)?;
                if pid == 0 || sid == 0 {
                    return Err(ExecutionError::Infrastructure);
                }
                sessions.insert(sid);
            }
        }
        Ok((sessions.len() >= 2).then_some(top))
    }
    pub fn configuration_fingerprint(&self) -> Result<ExecutionFingerprint, ExecutionError> {
        let volume = Volume {
            name: "<volume>".into(),
            mountpoint: "<mountpoint>".into(),
            driver: "local".into(),
            scope: "local".into(),
            options: None,
            labels: labels("<nonce>"),
            cluster_volume: None,
            status: None,
        };
        let mut commands = Vec::new();
        for phase in [
            Phase::Ingest,
            Phase::Run(RustCommand::Metadata),
            Phase::Run(RustCommand::FormatCheck),
            Phase::Run(RustCommand::TestProject(
                rust_engineering_domain::TestSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::ClippyProject(
                rust_engineering_domain::ClippySelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::Check),
            Phase::Run(RustCommand::CompilerVersion),
            Phase::Run(RustCommand::Explain(
                "E0502"
                    .parse()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
            Phase::Run(RustCommand::CargoVersion),
            Phase::Run(RustCommand::InstalledComponents),
            Phase::Run(RustCommand::CheckProject(
                rust_engineering_domain::CheckSelection::default()
                    .try_into()
                    .map_err(|_| ExecutionError::Infrastructure)?,
            )),
        ] {
            let mut args = self.arguments("<container>", "<nonce>", &volume, &phase)?;
            for arg in &mut args {
                if arg.starts_with("--security-opt=seccomp=") {
                    *arg = "--security-opt=seccomp=<profile>".into();
                }
            }
            commands.push(args);
        }
        let bytes = serde_json::to_vec(&(
            self.image_id(),
            &self.inner.engine,
            &self.inner.executable_digest,
            commands,
            include_str!("seccomp-rust.json"),
            // Receipts identify the actual verifier, archive/source limits and
            // supervisor implementation, not only a manually maintained label.
            implementation_fingerprint(),
            "rust-source-profile-v1",
        ))
        .map_err(|_| ExecutionError::Infrastructure)?;
        digest(&bytes)
            .parse()
            .map_err(|_| ExecutionError::Infrastructure)
    }
    pub fn image_id(&self) -> &str {
        self.inner.image_id()
    }
    pub fn is_quarantined(&self) -> bool {
        self.inner.is_quarantined()
    }
    fn arguments(
        &self,
        name: &str,
        nonce: &str,
        volume: &Volume,
        phase: &Phase,
    ) -> Result<Vec<String>, ExecutionError> {
        let mut args = [
            "container",
            "create",
            "--pull=never",
            "--runtime=runc",
            "--init=false",
            "--network=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges=true",
            "--ipc=private",
            "--cgroupns=private",
            "--pids-limit=128",
            "--cpus=1",
            "--memory=1g",
            "--memory-swap=1g",
            "--shm-size=1m",
            "--log-driver=none",
            "--no-healthcheck",
            "--tmpfs=/work:rw,exec,nosuid,nodev,size=512m,mode=1777",
            "--tmpfs=/tmp:rw,nosuid,nodev,noexec,size=64m,mode=1777",
            "--workdir=/source",
            "--hostname=sandbox",
        ]
        .map(str::to_owned)
        .to_vec();
        args.push(format!("--name={name}"));
        args.push(format!("--user={}", phase.user()));
        for (k, v) in labels(nonce) {
            args.push(format!("--label={k}={v}"));
        }
        for env in environment() {
            args.push(format!("--env={env}"));
        }
        let profile = self.inner.state.path().join("seccomp-rust.json");
        args.push(format!(
            "--security-opt=seccomp={}",
            profile
                .to_str()
                .ok_or(ExecutionError::InvalidConfiguration)?
        ));
        args.push(format!(
            "--mount=type=volume,source={},target=/source,volume-nocopy,volume-driver=local{}",
            volume.name,
            if phase.ingesting() { "" } else { ",readonly" }
        ));
        if phase.ingesting() {
            args.push("--interactive".into());
        }
        args.push(format!("--entrypoint={}", phase.program()));
        args.push(self.inner.config.image_id.clone());
        args.extend(phase.arguments());
        Ok(args)
    }
    fn absent(&self, kind: &str, name: &str) -> Result<bool, ExecutionError> {
        let args = if kind == "volume" {
            vec![
                "volume".into(),
                "ls".into(),
                format!("--filter=name=^{name}$"),
                "--format={{.Name}}".into(),
            ]
        } else {
            vec![
                "container".into(),
                "ls".into(),
                "--all".into(),
                format!("--filter=name=^/{name}$"),
                "--format={{.ID}}".into(),
            ]
        };
        let c = self.inner.control(&args)?;
        if c.code != Some(0) {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(c.stdout.iter().all(u8::is_ascii_whitespace))
    }
    fn owned_container(&self, name: &str, nonce: &str) -> Result<(), ExecutionError> {
        #[derive(Deserialize)]
        struct Owned {
            #[serde(rename = "Config")]
            config: OwnedConfig,
        }
        #[derive(Deserialize)]
        struct OwnedConfig {
            #[serde(rename = "Labels")]
            labels: BTreeMap<String, String>,
            #[serde(rename = "Image")]
            image: String,
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        let values: Vec<Owned> = serde_json::from_slice(&inspect.stdout)
            .map_err(|_| ExecutionError::CleanupUncertain)?;
        if inspect.code != Some(0)
            || values.len() != 1
            || values[0].config.labels != labels(nonce)
            || values[0].config.image != self.image_id()
        {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(())
    }
    fn owned_volume(&self, name: &str, nonce: &str) -> Result<(), ExecutionError> {
        let inspect = self
            .inner
            .control(&["volume".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) || Volume::parse(&inspect.stdout, name, nonce).is_err() {
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(())
    }
    fn cleanup(
        &self,
        ingest: &str,
        run: &str,
        volume: &str,
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        self.cleanup_inner(ingest, run, volume, nonce).map_err(|_| {
            self.inner.quarantined.store(true, Ordering::Release);
            ExecutionError::CleanupUncertain
        })
    }
    fn cleanup_inner(
        &self,
        ingest: &str,
        run: &str,
        volume: &str,
        nonce: &str,
    ) -> Result<(), ExecutionError> {
        // Attempt all removals even if one fails; never remove a volume before its writers.
        let mut clean = true;
        for name in [ingest, run] {
            match self.absent("container", name) {
                Ok(true) => (),
                Ok(false) => {
                    if self.owned_container(name, nonce).is_err()
                        || self.inner.remove(name).is_err()
                    {
                        clean = false;
                    }
                }
                Err(_) => clean = false,
            }
        }
        if clean {
            match self.absent("volume", volume) {
                Ok(true) => (),
                Ok(false) => {
                    self.owned_volume(volume, nonce)?;
                    match self
                        .inner
                        .control(&["volume".into(), "rm".into(), volume.into()])
                    {
                        Ok(c) if c.code == Some(0) => (),
                        _ => clean = false,
                    }
                    if !matches!(self.absent("volume", volume), Ok(true)) {
                        clean = false;
                    }
                }
                Err(_) => clean = false,
            }
        }
        if !clean {
            self.inner.quarantined.store(true, Ordering::Release);
            return Err(ExecutionError::CleanupUncertain);
        }
        Ok(())
    }
    fn phase(
        &self,
        request: PhaseRequest<'_>,
        input: &[u8],
        budget: &WorkBudget<'_>,
    ) -> Result<(Capture, Option<bool>), ExecutionError> {
        let PhaseRequest {
            name,
            nonce,
            volume,
            phase,
        } = request;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        if !self.absent("container", name)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let created = self
            .inner
            .control(&self.arguments(name, nonce, volume, phase)?)?;
        if created.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let inspect = self
            .inner
            .control(&["container".into(), "inspect".into(), name.into()])?;
        if inspect.code != Some(0) {
            return Err(ExecutionError::Infrastructure);
        }
        super::rust_applied::verify(&inspect.stdout, self.image_id(), phase, volume, nonce)?;
        if let Some(stop) = budget.stop() {
            return Ok((budget.stopped_capture(stop), None));
        }
        let mut command = DockerGateway::command(&self.inner.config, &self.inner.state)?;
        command.args(["container", "start", "--attach"]);
        if phase.ingesting() {
            command.arg("--interactive");
        }
        command.arg(name);
        let outcome = supervisor::run_with_input(
            command,
            budget.deadline.saturating_duration_since(Instant::now()),
            budget.limits.output_bytes(),
            budget,
            input,
        )?;
        let mut oom_killed = None;
        if outcome.stop == Stop::Exited {
            let inspected =
                self.inner
                    .control(&["container".into(), "inspect".into(), name.into()])?;
            let containers: Vec<Container> = serde_json::from_slice(&inspected.stdout)
                .map_err(|_| ExecutionError::Infrastructure)?;
            if inspected.code != Some(0)
                || containers.len() != 1
                || !containers[0].state.completed(outcome.code)
            {
                return Err(ExecutionError::Infrastructure);
            }
            oom_killed = Some(containers[0].state.oom_killed);
        }
        Ok((outcome, oom_killed))
    }
    pub fn execute(
        &self,
        source: &SourceBundle,
        command: RustCommand,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<ExecutionResult, ExecutionError> {
        if !self.verified.load(Ordering::Acquire) {
            return Err(ExecutionError::Denied);
        }
        self.execute_observed(source, command, limits, cancel, Admission::Project)
    }
    pub(super) fn execute_calibration(
        &self,
        source: &SourceBundle,
        command: RustCommand,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<ExecutionResult, ExecutionError> {
        self.execute_observed(
            source,
            command,
            limits,
            cancel,
            Admission::Calibration(None),
        )
    }
    pub(super) fn execute_observed(
        &self,
        source: &SourceBundle,
        command: RustCommand,
        limits: ExecutionLimits,
        cancel: &dyn ExecutionCancellation,
        admission: Admission<'_>,
    ) -> Result<ExecutionResult, ExecutionError> {
        let started = Instant::now();
        let _busy = match self.inner.busy.try_lock() {
            Ok(g) => g,
            Err(std::sync::TryLockError::WouldBlock) => return Err(ExecutionError::Busy),
            Err(_) => {
                self.inner.quarantined.store(true, Ordering::Release);
                return Err(ExecutionError::CleanupUncertain);
            }
        };
        if self.is_quarantined() {
            return Err(ExecutionError::CleanupUncertain);
        }
        // Recheck under the job lock: calibration may have revoked an earlier
        // observation while this caller was entering the gateway.
        if matches!(admission, Admission::Project)
            && (self.calibrating.load(Ordering::Acquire) || !self.verified.load(Ordering::Acquire))
        {
            return Err(ExecutionError::Denied);
        }
        if cancel.is_cancelled() {
            return Err(ExecutionError::Cancelled);
        }
        if digest(&state::executable_bytes(&self.inner.config.executable)?)
            != self.inner.executable_digest
        {
            return Err(ExecutionError::Unavailable);
        }
        let current = self
            .inner
            .control(&["info".into(), "--format={{json .}}".into()])?;
        let engine: EngineIdentity =
            serde_json::from_slice(&current.stdout).map_err(|_| ExecutionError::Unavailable)?;
        if current.code != Some(0) || engine != self.inner.engine {
            return Err(ExecutionError::Unavailable);
        }
        let archive = super::source_archive::encode(source)?;
        let budget = WorkBudget {
            started,
            deadline: started + Duration::from_millis(limits.wall_ms()),
            limits,
            cancel,
        };
        let nonce = state::nonce()?;
        let volume = format!("rust-mcp-source-{nonce}");
        let ingest = format!("rust-mcp-ingest-{nonce}");
        let run = format!("rust-mcp-cargo-{nonce}");
        let admission_scope = match admission {
            Admission::Project => "project",
            Admission::Calibration(_) => "calibration",
        };
        if let Admission::Calibration(Some(observer)) = admission {
            *observer
                .lock()
                .map_err(|_| ExecutionError::Infrastructure)? = Some(run.clone());
        }
        if !self.absent("volume", &volume)? {
            return Err(ExecutionError::CleanupUncertain);
        }
        let work = (|| {
            if let Some(stop) = budget.stop() {
                return Ok((budget.stopped_capture(stop), None));
            }
            let mut args = vec!["volume".into(), "create".into(), "--driver=local".into()];
            for (k, v) in labels(&nonce) {
                args.push(format!("--label={k}={v}"));
            }
            args.push(volume.clone());
            if self.inner.control(&args)?.code != Some(0) {
                return Err(ExecutionError::Infrastructure);
            }
            let inspect =
                self.inner
                    .control(&["volume".into(), "inspect".into(), volume.clone()])?;
            if inspect.code != Some(0) {
                return Err(ExecutionError::Infrastructure);
            }
            let v = Volume::parse(&inspect.stdout, &volume, &nonce)?;
            let (ingested, ingest_oom) = self.phase(
                PhaseRequest {
                    name: &ingest,
                    nonce: &nonce,
                    volume: &v,
                    phase: &Phase::Ingest,
                },
                &archive,
                &budget,
            )?;
            if ingested.stop != Stop::Exited {
                return Ok((ingested, ingest_oom));
            }
            if ingested.code != Some(0) {
                return Err(ExecutionError::Infrastructure);
            }
            self.inner.remove(&ingest)?;
            // No source writer remains while untrusted code executes.
            self.phase(
                PhaseRequest {
                    name: &run,
                    nonce: &nonce,
                    volume: &v,
                    phase: &Phase::Run(command.clone()),
                },
                &[],
                &budget,
            )
        })();
        let terminal_signal = budget.stop();
        self.cleanup(&ingest, &run, &volume, &nonce)?;
        let (outcome, oom_killed) = finish_work(work, terminal_signal)?;
        let (stdout, expanded_out) = bounded_text(&outcome.stdout, limits.output_bytes());
        let (stderr, expanded_err) = bounded_text(&outcome.stderr, limits.output_bytes());
        let termination = if expanded_out || expanded_err {
            ExecutionTermination::OutputLimit
        } else {
            match outcome.stop {
                Stop::Exited => ExecutionTermination::Exited,
                Stop::Cancelled => ExecutionTermination::Cancelled,
                Stop::TimedOut => ExecutionTermination::TimedOut,
                Stop::OutputLimit => ExecutionTermination::OutputLimit,
            }
        };
        let identity = serde_json::to_vec(&(
            self.configuration_fingerprint()?,
            command,
            limits,
            digest(&archive),
            admission_scope,
            "rust-source-profile-v1",
        ))
        .map_err(|_| ExecutionError::Infrastructure)?;
        Ok(ExecutionResult {
            termination,
            exit_code: if outcome.stop == Stop::Exited {
                outcome.code
            } else {
                None
            },
            oom_killed,
            stdout,
            stderr,
            stdout_truncated: outcome.stdout_truncated || expanded_out,
            stderr_truncated: outcome.stderr_truncated || expanded_err,
            duration_ms: outcome.duration_ms,
            total_duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
            execution_fingerprint: digest(&identity)
                .parse()
                .map_err(|_| ExecutionError::Infrastructure)?,
            platform: "linux/aarch64",
            image_id: self.image_id().into(),
        })
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use rust_engineering_domain::SourceFile;
    #[test]
    fn explain_command_accepts_only_validated_code_as_one_separate_argument()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::DiagnosticCode;
        for code in ["E0502", "E0000", "E9999"] {
            let phase = Phase::Run(RustCommand::Explain(code.parse()?));
            assert_eq!(phase.program(), "/opt/rust/bin/rustc");
            assert_eq!(phase.arguments(), ["--explain", code, "--color", "never"]);
            assert_eq!(phase.user(), "65534:65534");
            assert!(!phase.ingesting());
        }
        for invalid in [
            "E0502 --help",
            "E0502;id",
            "$(id)",
            "E0502\n",
            "--help",
            "e0502",
            "E０５０２",
        ] {
            assert!(invalid.parse::<DiagnosticCode>().is_err(), "{invalid:?}");
        }
        assert_ne!(
            serde_json::to_vec(&RustCommand::Explain("E0502".parse()?))?,
            serde_json::to_vec(&RustCommand::Explain("E9999".parse()?))?
        );
        Ok(())
    }
    #[test]
    fn test_command_seals_harness_args_and_filter_position()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::TestSelection;
        let options = TestSelection {
            package: Some("app".into()),
            test_filter: Some("tests::works".into()),
            features: vec!["extra".into()],
            target: Some("aarch64-unknown-linux-gnu".into()),
            timeout: 60,
            ..Default::default()
        }
        .try_into()?;
        let phase = Phase::Run(RustCommand::TestProject(options));
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            phase.arguments(),
            [
                "test",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--color=never",
                "--package=app",
                "--features=extra",
                "--target=aarch64-unknown-linux-gnu",
                "tests::works",
                "--",
                "--test-threads=1",
                "--color=never"
            ]
        );
        let all = Phase::Run(RustCommand::TestProject(
            TestSelection {
                all_features: true,
                ..Default::default()
            }
            .try_into()?,
        ));
        assert_eq!(
            all.arguments(),
            [
                "test",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--color=never",
                "--all-features",
                "--",
                "--test-threads=1",
                "--color=never"
            ]
        );
        Ok(())
    }
    #[test]
    fn clippy_profiles_and_selections_have_closed_argv() -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::{ClippySelection, LintProfile};
        for (lint_profile, suffix) in [
            (LintProfile::Default, vec![]),
            (LintProfile::Project, vec![]),
            (LintProfile::Strict, vec!["--", "-D", "warnings"]),
            (LintProfile::Pedantic, vec!["--", "-W", "clippy::pedantic"]),
        ] {
            let options = ClippySelection {
                package: Some("app".into()),
                features: vec!["serde/derive".into()],
                all_targets: true,
                lint_profile,
                ..Default::default()
            }
            .try_into()?;
            let phase = Phase::Run(RustCommand::ClippyProject(options));
            let mut expected = vec![
                "clippy",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--package=app",
                "--features=serde/derive",
                "--all-targets",
            ];
            expected.extend(suffix);
            assert_eq!(phase.arguments(), expected);
            assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        }
        let workspace = Phase::Run(RustCommand::ClippyProject(
            ClippySelection {
                workspace: true,
                ..Default::default()
            }
            .try_into()?,
        ));
        assert_eq!(
            workspace.arguments(),
            [
                "clippy",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--workspace"
            ]
        );
        Ok(())
    }
    #[test]
    fn formatting_command_is_closed_and_cannot_write_source() {
        let phase = Phase::Run(RustCommand::FormatCheck);
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            phase.arguments(),
            [
                "fmt",
                "--all",
                "--check",
                "--",
                "--color",
                "never",
                "--config",
                "disable_all_formatting=false"
            ]
        );
        assert!(environment().contains(&"RUSTFMT=/opt/rust/bin/rustfmt".into()));
    }
    #[test]
    fn check_arguments_are_closed_separate_and_fingerprintable()
    -> Result<(), Box<dyn std::error::Error>> {
        use rust_engineering_domain::CheckSelection;
        let options = CheckSelection {
            package: Some("member".into()),
            features: vec!["z".into(), "dep/feature".into()],
            no_default_features: true,
            all_targets: true,
            target: Some("aarch64-unknown-linux-gnu".into()),
            ..Default::default()
        }
        .try_into()?;
        let phase = Phase::Run(RustCommand::CheckProject(options));
        assert_eq!(phase.program(), "/opt/rust/bin/cargo");
        assert_eq!(
            phase.arguments(),
            [
                "check",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--package=member",
                "--features=dep/feature,z",
                "--no-default-features",
                "--all-targets",
                "--target=aarch64-unknown-linux-gnu"
            ]
        );
        let workspace = Phase::Run(RustCommand::CheckProject(
            CheckSelection {
                workspace: true,
                all_features: true,
                ..Default::default()
            }
            .try_into()?,
        ));
        assert_eq!(
            workspace.arguments(),
            [
                "check",
                "--frozen",
                "--message-format=json",
                "--jobs=1",
                "--workspace",
                "--all-features"
            ]
        );
        assert_ne!(
            serde_json::to_vec(&RustCommand::Check)?,
            serde_json::to_vec(&RustCommand::CheckProject(
                CheckSelection::default().try_into()?
            ))?
        );
        assert!(environment().contains(&"CARGO_NET_OFFLINE=true".into()));
        Ok(())
    }
    #[test]
    fn installed_components_command_is_fixed_and_has_no_peer_arguments() {
        let phase = Phase::Run(RustCommand::InstalledComponents);
        assert_eq!(phase.program(), "/usr/bin/cat");
        assert_eq!(
            phase.arguments(),
            ["--", "/opt/rust/lib/rustlib/components"]
        );
        assert!(!phase.ingesting());
        assert_eq!(phase.user(), "65534:65534");
    }
    #[test]
    fn cancellation_and_deadlines_never_mask_harness_or_cleanup_errors() {
        for error in [
            ExecutionError::InvalidConfiguration,
            ExecutionError::Unavailable,
            ExecutionError::Denied,
            ExecutionError::Busy,
            ExecutionError::Cancelled,
            ExecutionError::Infrastructure,
            ExecutionError::CleanupUncertain,
        ] {
            for signal in [None, Some(Stop::TimedOut), Some(Stop::Cancelled)] {
                assert_eq!(finish_work(Err(error), signal).err(), Some(error));
            }
        }
    }
    #[test]
    fn completion_preserves_overflow_and_observed_oom() -> Result<(), ExecutionError> {
        let capture = Capture {
            code: None,
            stdout: vec![],
            stderr: vec![],
            stdout_truncated: true,
            stderr_truncated: false,
            stop: Stop::OutputLimit,
            duration_ms: 1,
        };
        let (capture, oom) = finish_work(Ok((capture, Some(true))), Some(Stop::Cancelled))?;
        assert_eq!(capture.stop, Stop::OutputLimit);
        assert_eq!(oom, Some(true));
        Ok(())
    }
    #[test]
    #[ignore = "Requires explicit local Docker socket and approved Rust image"]
    fn benign_source_transfer_compiles_with_empty_directory() -> Result<(), String> {
        let socket = std::env::var_os("RUST_MCP_TEST_SOCKET").ok_or("explicit socket required")?;
        let root = PathBuf::from("/private/tmp").join(format!(
            "rust-mcp-rust-test-{}",
            state::nonce().map_err(|e| format!("{e:?}"))?
        ));
        std::fs::create_dir(&root).map_err(|e| e.to_string())?;
        struct Root(PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _root = Root(root.clone());
        let gateway = RustGateway::new(HostDockerConfig {
            executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
            socket: socket.into(),
            state_root: root,
            image_id: APPROVED_RUST_IMAGE.into(),
        })
        .map_err(|e| format!("create: {e:?}"))?;
        let files = [
            (
                "Cargo.toml",
                "[package]\nname='m1_transfer'\nversion='0.1.0'\nedition='2024'\n",
            ),
            (
                "Cargo.lock",
                "version = 4\n[[package]]\nname = 'm1_transfer'\nversion = '0.1.0'\n",
            ),
            ("src/lib.rs", "pub fn answer() -> u8 { 42 }\n"),
            (
                "build.rs",
                "fn main() { assert!(std::path::Path::new(\"empty\").is_dir()); assert!(std::path::Path::new(&\"a\".repeat(100)).is_dir()); }\n",
            ),
        ]
        .into_iter()
        .map(|(p, b)| SourceFile::new(p.into(), b.as_bytes().to_vec()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("{e:?}"))?;
        let source = SourceBundle::with_directories(files, vec!["empty".into(), "a".repeat(100)])
            .map_err(|e| format!("{e:?}"))?;
        let limits = ExecutionLimits::new(30_000, 256 * 1024).ok_or("limits")?;
        assert!(matches!(
            gateway.execute(&source, RustCommand::Check, limits, &NeverCancel),
            Err(ExecutionError::Denied)
        ));
        let result = gateway
            .execute_calibration(&source, RustCommand::Check, limits, &NeverCancel)
            .map_err(|e| format!("execute: {e:?}"))?;
        assert_eq!(
            result.termination,
            ExecutionTermination::Exited,
            "{}",
            result.stderr
        );
        assert_eq!(result.exit_code, Some(0), "{}", result.stderr);
        assert!(result.stdout.contains("build-finished"));
        assert!(!gateway.is_quarantined());
        println!(
            "{}",
            serde_json::to_string(&result).map_err(|e| e.to_string())?
        );
        Ok(())
    }
}

#[cfg(all(test, target_os = "macos"))]
mod test_runtime;
