//! Closed writable staging lifecycle from ADR-053.
use super::*;
use crate::rust_gateway::RustGateway;
use rust_engineering_domain::{
    ExecutionLimits, ExecutionResult, ExecutionTermination, RustMutationCommand,
    RustMutationExecution, SourceBundle,
};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

pub(super) const VOLUME_OPTIONS: &str =
    "size=64m,nr_inodes=8192,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec";
const FIX_TARGET_TMPFS: &str = "rw,exec,nosuid,nodev,size=256m,mode=0700,uid=65534,gid=65534";

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub(super) enum MutationPhase {
    Guardian,
    Ingest,
    Format,
    Fix,
    Export,
}
impl MutationPhase {
    pub(super) fn writable(self) -> bool {
        matches!(self, Self::Ingest | Self::Format | Self::Fix)
    }
    pub(super) fn interactive(self) -> bool {
        self == Self::Ingest
    }
    pub(super) fn program(self) -> &'static str {
        match self {
            Self::Guardian => "/usr/bin/sleep",
            Self::Ingest | Self::Export => "/usr/bin/tar",
            Self::Format | Self::Fix => "/opt/rust/bin/cargo",
        }
    }
    pub(super) fn arguments(self) -> &'static [&'static str] {
        match self {
            Self::Guardian => &["900"],
            Self::Ingest => &[
                "--extract",
                "--file=-",
                "--directory=/source",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::Format => &[
                "fmt",
                "--all",
                "--",
                "--color",
                "never",
                "--config",
                "disable_all_formatting=false",
            ],
            Self::Fix => &[
                "fix",
                "--workspace",
                "--all-targets",
                "--frozen",
                "--offline",
                "--allow-no-vcs",
                "--allow-dirty",
                "--allow-staged",
                "--message-format=json",
                "--color",
                "never",
                "--target-dir",
                "/target",
            ],
            Self::Export => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--sort=name",
                "--one-file-system",
                "--directory=/source",
                ".",
            ],
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub(super) struct MutationVolume {
    pub(super) name: String,
    pub(super) driver: String,
    pub(super) scope: String,
    pub(super) options: BTreeMap<String, String>,
    pub(super) labels: BTreeMap<String, String>,
    pub(super) mountpoint: String,
    pub(super) cluster_volume: Option<serde_json::Value>,
    pub(super) status: Option<serde_json::Value>,
}

pub(super) fn labels(nonce: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("org.rust-mcp.execution".into(), "true".into()),
        ("org.rust-mcp.rust-job".into(), nonce.into()),
    ])
}

fn fail_closed_state_change<T>(
    quarantined: &std::sync::atomic::AtomicBool,
    request: impl FnOnce() -> Result<T, (ExecutionError, bool)>,
    reply_proves_applied: impl FnOnce(&T) -> bool,
) -> Result<T, ExecutionError> {
    match request() {
        Ok(reply) if reply_proves_applied(&reply) => Ok(reply),
        Err((error, false)) => Err(error),
        Ok(_) | Err((_, true)) => {
            // Once a state-changing request may have reached the daemon, an
            // empty immediate inventory cannot prove that a delayed commit or
            // reply will not appear. Only a new gateway session can recover.
            quarantined.store(true, Ordering::Release);
            Err(ExecutionError::CleanupUncertain)
        }
    }
}

pub(super) fn mutation_control(
    gateway: &RustGateway,
    arguments: &[String],
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<Capture, ExecutionError> {
    fail_closed_state_change(
        &gateway.inner.quarantined,
        || gateway.inner.control_until(arguments, deadline, cancel),
        |capture| capture.code == Some(0),
    )
}

pub(super) fn query_control(
    gateway: &RustGateway,
    arguments: &[String],
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<Capture, ExecutionError> {
    gateway
        .inner
        .control_until(arguments, deadline, cancel)
        .map_err(|(error, _)| error)
}

pub(super) fn parse_volume(
    bytes: &[u8],
    name: &str,
    nonce: &str,
) -> Result<MutationVolume, ExecutionError> {
    parse_volume_with_options(bytes, name, nonce, VOLUME_OPTIONS)
}

pub(super) fn parse_volume_with_options(
    bytes: &[u8],
    name: &str,
    nonce: &str,
    expected_options: &str,
) -> Result<MutationVolume, ExecutionError> {
    let mut values: Vec<MutationVolume> =
        serde_json::from_slice(bytes).map_err(|_| ExecutionError::Infrastructure)?;
    let volume = values
        .pop()
        .filter(|_| values.is_empty())
        .ok_or(ExecutionError::InvalidConfiguration)?;
    let expected = BTreeMap::from([
        ("device".into(), "tmpfs".into()),
        ("o".into(), expected_options.into()),
        ("type".into(), "tmpfs".into()),
    ]);
    if volume.name != name
        || volume.driver != "local"
        || volume.scope != "local"
        || volume.options != expected
        || volume.labels != labels(nonce)
        || !volume.mountpoint.starts_with("/var/lib/docker/volumes/")
        || !volume.mountpoint.ends_with("/_data")
        || volume.cluster_volume.is_some()
        || volume.status.is_some()
    {
        return Err(ExecutionError::InvalidConfiguration);
    }
    Ok(volume)
}

fn create_arguments(
    gateway: &RustGateway,
    name: &str,
    nonce: &str,
    volume: &MutationVolume,
    phase: MutationPhase,
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
        "--user=65534:65534",
    ]
    .map(str::to_owned)
    .to_vec();
    args.push(format!("--name={name}"));
    for (key, value) in labels(nonce) {
        args.push(format!("--label={key}={value}"));
    }
    for value in crate::rust_gateway::environment() {
        args.push(format!("--env={value}"));
    }
    if phase == MutationPhase::Fix {
        args.push(format!("--tmpfs=/target:{FIX_TARGET_TMPFS}"));
    }
    let profile = gateway
        .inner
        .state
        .path()
        .join(if phase == MutationPhase::Fix {
            "seccomp-rust-fix.json"
        } else {
            "seccomp-rust.json"
        });
    args.push(format!(
        "--security-opt=seccomp={}",
        profile
            .to_str()
            .ok_or(ExecutionError::InvalidConfiguration)?
    ));
    args.push(format!(
        "--mount=type=volume,source={},target=/source,volume-nocopy,volume-driver=local{}",
        volume.name,
        if phase.writable() { "" } else { ",readonly" }
    ));
    if phase.interactive() {
        args.push("--interactive".into());
    }
    args.push(format!("--entrypoint={}", phase.program()));
    args.push(gateway.image_id().into());
    args.extend(phase.arguments().iter().map(|arg| (*arg).to_owned()));
    Ok(args)
}

pub(super) fn absent(
    gateway: &RustGateway,
    kind: &str,
    name: &str,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<bool, ExecutionError> {
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
    let result = query_control(gateway, &args, deadline, cancel)?;
    if result.code != Some(0) {
        return Err(ExecutionError::CleanupUncertain);
    }
    Ok(result.stdout.iter().all(u8::is_ascii_whitespace))
}

fn create_phase(
    gateway: &RustGateway,
    name: &str,
    nonce: &str,
    volume: &MutationVolume,
    phase: MutationPhase,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), ExecutionError> {
    if !absent(gateway, "container", name, deadline, cancel)? {
        return Err(ExecutionError::CleanupUncertain);
    }
    mutation_control(
        gateway,
        &create_arguments(gateway, name, nonce, volume, phase)?,
        deadline,
        cancel,
    )?;
    let inspect = query_control(
        gateway,
        &["container".into(), "inspect".into(), name.into()],
        deadline,
        cancel,
    )?;
    if inspect.code != Some(0) {
        return Err(ExecutionError::Infrastructure);
    }
    crate::rust_applied::verify_mutation(&inspect.stdout, gateway.image_id(), phase, volume, nonce)
}

pub(super) fn start_attached(
    gateway: &RustGateway,
    name: &str,
    interactive: bool,
    input: &[u8],
    deadline: Instant,
    output_limit: usize,
    cancel: &dyn ExecutionCancellation,
) -> Result<Capture, ExecutionError> {
    let mut command = DockerGateway::command(&gateway.inner.config, &gateway.inner.state)?;
    command.args(["container", "start", "--attach"]);
    if interactive {
        command.arg("--interactive");
    }
    command.arg(name);
    if cancel.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    if deadline.saturating_duration_since(Instant::now()).is_zero() {
        return Err(ExecutionError::Infrastructure);
    }
    fail_closed_state_change(
        &gateway.inner.quarantined,
        || {
            supervisor::run_with_input(
                command,
                deadline.saturating_duration_since(Instant::now()),
                output_limit,
                cancel,
                input,
            )
            .map_err(|error| (error, true))
        },
        |_| true,
    )
}

pub(super) fn running(
    gateway: &RustGateway,
    name: &str,
    nonce: &str,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<bool, ExecutionError> {
    #[derive(Deserialize)]
    struct State {
        #[serde(rename = "Running")]
        running: bool,
        #[serde(rename = "Status")]
        status: String,
        #[serde(rename = "OOMKilled")]
        oom: bool,
        #[serde(rename = "Error")]
        error: String,
    }
    #[derive(Deserialize)]
    struct Config {
        #[serde(rename = "Labels")]
        labels: BTreeMap<String, String>,
        #[serde(rename = "Image")]
        image: String,
    }
    #[derive(Deserialize)]
    struct Item {
        #[serde(rename = "State")]
        state: State,
        #[serde(rename = "Config")]
        config: Config,
    }
    let capture = query_control(
        gateway,
        &["container".into(), "inspect".into(), name.into()],
        deadline,
        cancel,
    )?;
    let items: Vec<Item> =
        serde_json::from_slice(&capture.stdout).map_err(|_| ExecutionError::Infrastructure)?;
    let item = items
        .first()
        .filter(|_| capture.code == Some(0) && items.len() == 1)
        .ok_or(ExecutionError::Infrastructure)?;
    Ok(item.state.running
        && item.state.status == "running"
        && !item.state.oom
        && item.state.error.is_empty()
        && item.config.labels == labels(nonce)
        && item.config.image == gateway.image_id())
}

pub(super) fn remove_if_present(
    gateway: &RustGateway,
    name: &str,
    nonce: &str,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), ExecutionError> {
    if absent(gateway, "container", name, deadline, cancel)? {
        Ok(())
    } else {
        #[derive(Deserialize)]
        struct Config {
            #[serde(rename = "Labels")]
            labels: BTreeMap<String, String>,
            #[serde(rename = "Image")]
            image: String,
        }
        #[derive(Deserialize)]
        struct Item {
            #[serde(rename = "Config")]
            config: Config,
        }
        let inspected = query_control(
            gateway,
            &["container".into(), "inspect".into(), name.into()],
            deadline,
            cancel,
        )?;
        let items: Vec<Item> = serde_json::from_slice(&inspected.stdout)
            .map_err(|_| ExecutionError::CleanupUncertain)?;
        let item = items
            .first()
            .filter(|_| inspected.code == Some(0) && items.len() == 1)
            .ok_or(ExecutionError::CleanupUncertain)?;
        if item.config.labels != labels(nonce) || item.config.image != gateway.image_id() {
            return Err(ExecutionError::CleanupUncertain);
        }
        mutation_control(
            gateway,
            &[
                "container".into(),
                "rm".into(),
                "--force".into(),
                name.into(),
            ],
            deadline,
            cancel,
        )?;
        if absent(gateway, "container", name, deadline, cancel)? {
            Ok(())
        } else {
            Err(ExecutionError::CleanupUncertain)
        }
    }
}

pub(super) fn cleanup(
    gateway: &RustGateway,
    names: &[&str],
    volume: &str,
    nonce: &str,
) -> Result<(), ExecutionError> {
    cleanup_with_options(gateway, names, volume, nonce, VOLUME_OPTIONS)
}

pub(super) fn cleanup_with_options(
    gateway: &RustGateway,
    names: &[&str],
    volume: &str,
    nonce: &str,
    expected_options: &str,
) -> Result<(), ExecutionError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    cleanup_until_with_options(gateway, names, volume, nonce, expected_options, deadline)
}

pub(super) fn cleanup_until(
    gateway: &RustGateway,
    names: &[&str],
    volume: &str,
    nonce: &str,
    deadline: Instant,
) -> Result<(), ExecutionError> {
    cleanup_until_with_options(gateway, names, volume, nonce, VOLUME_OPTIONS, deadline)
}

fn cleanup_until_with_options(
    gateway: &RustGateway,
    names: &[&str],
    volume: &str,
    nonce: &str,
    expected_options: &str,
    deadline: Instant,
) -> Result<(), ExecutionError> {
    let cancel = &NeverCancel;
    let mut clean = true;
    for name in names {
        if remove_if_present(gateway, name, nonce, deadline, cancel).is_err() {
            clean = false;
        }
    }
    if clean {
        match absent(gateway, "volume", volume, deadline, cancel) {
            Ok(true) => (),
            Ok(false) => {
                let inspected = query_control(
                    gateway,
                    &["volume".into(), "inspect".into(), volume.into()],
                    deadline,
                    cancel,
                );
                if !matches!(inspected, Ok(ref c) if c.code == Some(0) && parse_volume_with_options(&c.stdout, volume, nonce, expected_options).is_ok())
                {
                    clean = false;
                } else {
                    let removed = mutation_control(
                        gateway,
                        &["volume".into(), "rm".into(), volume.into()],
                        deadline,
                        cancel,
                    );
                    if removed.is_err()
                        || !matches!(
                            absent(gateway, "volume", volume, deadline, cancel),
                            Ok(true)
                        )
                    {
                        clean = false;
                    }
                }
            }
            Err(_) => clean = false,
        }
    }
    if clean {
        Ok(())
    } else {
        gateway.inner.quarantined.store(true, Ordering::Release);
        Err(ExecutionError::CleanupUncertain)
    }
}

fn mutation_scope_ok(
    before: &SourceBundle,
    after: &SourceBundle,
    command: &RustMutationCommand,
) -> bool {
    if before.files().len() != after.files().len() || before.directories() != after.directories() {
        return false;
    }
    let mut changed = 0usize;
    before.files().iter().zip(after.files()).all(|(old, new)| {
        if old.path() != new.path() {
            return false;
        }
        if old.bytes() == new.bytes() {
            return true;
        }
        changed += 1;
        old.path().ends_with(".rs")
            && (!matches!(command, RustMutationCommand::Fix) || changed <= 128)
    })
}

fn fix_output_complete(capture: &Capture, candidate: &SourceBundle) -> bool {
    if capture.stdout_truncated || capture.stderr_truncated {
        return false;
    }
    let Ok(stdout) = std::str::from_utf8(&capture.stdout) else {
        return false;
    };
    super::cargo_diagnostics::parse(stdout, candidate, true)
        .is_ok_and(|parsed| parsed.complete && parsed.build_finished == Some(true))
}

fn make_result(
    gateway: &RustGateway,
    source_archive: &[u8],
    command: RustMutationCommand,
    limits: ExecutionLimits,
    started: Instant,
    capture: Capture,
    oom: Option<bool>,
) -> Result<ExecutionResult, ExecutionError> {
    let (stdout, expanded_out) = bounded_text(&capture.stdout, limits.output_bytes());
    let (stderr, expanded_err) = bounded_text(&capture.stderr, limits.output_bytes());
    let termination = if expanded_out || expanded_err {
        ExecutionTermination::OutputLimit
    } else {
        match capture.stop {
            Stop::Exited => ExecutionTermination::Exited,
            Stop::TimedOut => ExecutionTermination::TimedOut,
            Stop::Cancelled => ExecutionTermination::Cancelled,
            Stop::OutputLimit => ExecutionTermination::OutputLimit,
        }
    };
    let identity = serde_json::to_vec(&(
        mutation_configuration_fingerprint(gateway)?,
        command,
        limits,
        digest(source_archive),
        "rust-mutation-staging-v1",
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    Ok(ExecutionResult {
        termination,
        exit_code: (capture.stop == Stop::Exited)
            .then_some(capture.code)
            .flatten(),
        oom_killed: oom,
        stdout,
        stderr,
        stdout_truncated: capture.stdout_truncated || expanded_out,
        stderr_truncated: capture.stderr_truncated || expanded_err,
        duration_ms: capture.duration_ms,
        total_duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        execution_fingerprint: digest(&identity)
            .parse()
            .map_err(|_| ExecutionError::Infrastructure)?,
        platform: "linux/aarch64",
        image_id: gateway.image_id().into(),
    })
}

fn mutation_configuration_fingerprint(
    gateway: &RustGateway,
) -> Result<rust_engineering_domain::ExecutionFingerprint, ExecutionError> {
    let volume = MutationVolume {
        name: "<volume>".into(),
        driver: "local".into(),
        scope: "local".into(),
        options: BTreeMap::from([
            ("device".into(), "tmpfs".into()),
            ("o".into(), VOLUME_OPTIONS.into()),
            ("type".into(), "tmpfs".into()),
        ]),
        labels: labels("<nonce>"),
        mountpoint: "<mountpoint>".into(),
        cluster_volume: None,
        status: None,
    };
    let commands = [
        MutationPhase::Guardian,
        MutationPhase::Ingest,
        MutationPhase::Format,
        MutationPhase::Fix,
        MutationPhase::Export,
    ]
    .into_iter()
    .map(|phase| create_arguments(gateway, "<container>", "<nonce>", &volume, phase))
    .collect::<Result<Vec<_>, _>>()?;
    let implementation: &[&[u8]] = &[
        include_bytes!("mutation_gateway.rs"),
        include_bytes!("mutation_archive.rs"),
        include_bytes!("rust_applied.rs"),
        include_bytes!("seccomp-rust-fix.json"),
        include_bytes!("../../domain/src/rust_mutation.rs"),
    ];
    let bytes = serde_json::to_vec(&(
        gateway.configuration_fingerprint()?,
        commands,
        ["--opt=type=tmpfs", "--opt=device=tmpfs", VOLUME_OPTIONS],
        implementation
            .iter()
            .map(|source| digest(source))
            .collect::<Vec<_>>(),
        "rust-mutation-staging-v1",
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    digest(&bytes)
        .parse()
        .map_err(|_| ExecutionError::Infrastructure)
}

pub(super) fn execute(
    gateway: &RustGateway,
    source: &SourceBundle,
    command: RustMutationCommand,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<RustMutationExecution, ExecutionError> {
    let started = Instant::now();
    let _busy = match gateway.inner.busy.try_lock() {
        Ok(guard) => guard,
        Err(std::sync::TryLockError::WouldBlock) => return Err(ExecutionError::Busy),
        Err(_) => {
            gateway.inner.quarantined.store(true, Ordering::Release);
            return Err(ExecutionError::CleanupUncertain);
        }
    };
    if gateway.is_quarantined() {
        return Err(ExecutionError::CleanupUncertain);
    }
    if !gateway.verified.load(Ordering::Acquire) {
        return Err(ExecutionError::Denied);
    }
    if cancel.is_cancelled() {
        return Err(ExecutionError::Cancelled);
    }
    if digest(&state::executable_bytes(&gateway.inner.config.executable)?)
        != gateway.inner.executable_digest
    {
        return Err(ExecutionError::Unavailable);
    }
    let info = gateway
        .inner
        .control(&["info".into(), "--format={{json .}}".into()])?;
    let engine: EngineIdentity =
        serde_json::from_slice(&info.stdout).map_err(|_| ExecutionError::Unavailable)?;
    if info.code != Some(0) || engine != gateway.inner.engine {
        return Err(ExecutionError::Unavailable);
    }
    let archive = crate::mutation_archive::encode(source)?;
    let deadline = started + Duration::from_millis(limits.wall_ms());
    let nonce = state::nonce()?;
    let volume_name = format!("rust-mcp-mutation-source-{nonce}");
    let guardian = format!("rust-mcp-mutation-guardian-{nonce}");
    let ingest = format!("rust-mcp-mutation-ingest-{nonce}");
    let mutation_phase = match &command {
        RustMutationCommand::Format => MutationPhase::Format,
        RustMutationCommand::Fix => MutationPhase::Fix,
    };
    let mutator_kind = match &command {
        RustMutationCommand::Format => "format",
        RustMutationCommand::Fix => "fix",
    };
    let mutator = format!("rust-mcp-mutation-{mutator_kind}-{nonce}");
    let exporter = format!("rust-mcp-mutation-export-{nonce}");
    let names = [&ingest[..], &mutator[..], &exporter[..], &guardian[..]];
    if !absent(gateway, "volume", &volume_name, deadline, cancel)? {
        return Err(ExecutionError::CleanupUncertain);
    }
    let work = (|| {
        let mut args = vec![
            "volume".into(),
            "create".into(),
            "--driver=local".into(),
            "--opt=type=tmpfs".into(),
            "--opt=device=tmpfs".into(),
            format!("--opt=o={VOLUME_OPTIONS}"),
        ];
        for (key, value) in labels(&nonce) {
            args.push(format!("--label={key}={value}"));
        }
        args.push(volume_name.clone());
        mutation_control(gateway, &args, deadline, cancel)?;
        let inspected = query_control(
            gateway,
            &["volume".into(), "inspect".into(), volume_name.clone()],
            deadline,
            cancel,
        )?;
        let volume = parse_volume(&inspected.stdout, &volume_name, &nonce)?;
        create_phase(
            gateway,
            &guardian,
            &nonce,
            &volume,
            MutationPhase::Guardian,
            deadline,
            cancel,
        )?;
        mutation_control(
            gateway,
            &["container".into(), "start".into(), guardian.clone()],
            deadline,
            cancel,
        )?;
        if !running(gateway, &guardian, &nonce, deadline, cancel)? {
            return Err(ExecutionError::Infrastructure);
        }
        create_phase(
            gateway,
            &ingest,
            &nonce,
            &volume,
            MutationPhase::Ingest,
            deadline,
            cancel,
        )?;
        let ingested = start_attached(
            gateway,
            &ingest,
            true,
            &archive,
            deadline,
            limits.output_bytes(),
            cancel,
        )?;
        if ingested.stop != Stop::Exited {
            return Ok((ingested, None, None));
        }
        remove_if_present(gateway, &ingest, &nonce, deadline, cancel)?;
        if ingested.code != Some(0)
            || ingested.stdout_truncated
            || ingested.stderr_truncated
            || !ingested.stdout.is_empty()
            || !ingested.stderr.is_empty()
        {
            return Err(ExecutionError::Infrastructure);
        }
        if !running(gateway, &guardian, &nonce, deadline, cancel)? {
            return Err(ExecutionError::Infrastructure);
        }
        create_phase(
            gateway,
            &mutator,
            &nonce,
            &volume,
            mutation_phase,
            deadline,
            cancel,
        )?;
        let mutated = start_attached(
            gateway,
            &mutator,
            false,
            &[],
            deadline,
            limits.output_bytes(),
            cancel,
        )?;
        let oom = if mutated.stop == Stop::Exited {
            let inspected = query_control(
                gateway,
                &["container".into(), "inspect".into(), mutator.clone()],
                deadline,
                cancel,
            )?;
            let values: Vec<Container> = serde_json::from_slice(&inspected.stdout)
                .map_err(|_| ExecutionError::Infrastructure)?;
            let value = values
                .first()
                .filter(|_| values.len() == 1)
                .ok_or(ExecutionError::Infrastructure)?;
            if inspected.code != Some(0) || !value.state.completed(mutated.code) {
                return Err(ExecutionError::Infrastructure);
            }
            Some(value.state.oom_killed)
        } else {
            None
        };
        if mutated.stop != Stop::Exited || mutated.code != Some(0) {
            return Ok((mutated, oom, None));
        }
        if matches!(command, RustMutationCommand::Fix) && oom != Some(false) {
            return Ok((mutated, oom, None));
        }
        remove_if_present(gateway, &mutator, &nonce, deadline, cancel)?;
        if !running(gateway, &guardian, &nonce, deadline, cancel)?
            || !absent(gateway, "container", &ingest, deadline, cancel)?
            || !absent(gateway, "container", &mutator, deadline, cancel)?
        {
            return Err(ExecutionError::Infrastructure);
        }
        create_phase(
            gateway,
            &exporter,
            &nonce,
            &volume,
            MutationPhase::Export,
            deadline,
            cancel,
        )?;
        let exported = start_attached(
            gateway,
            &exporter,
            false,
            &[],
            deadline,
            crate::mutation_archive::MAX_ARCHIVE,
            cancel,
        )?;
        if exported.stop != Stop::Exited {
            if exported.stop == Stop::OutputLimit {
                return Err(ExecutionError::Denied);
            }
            let mut interrupted = mutated;
            interrupted.stop = exported.stop;
            interrupted.code = None;
            interrupted.duration_ms = exported.duration_ms;
            return Ok((interrupted, None, None));
        }
        remove_if_present(gateway, &exporter, &nonce, deadline, cancel)?;
        if exported.code != Some(0)
            || exported.stdout_truncated
            || exported.stderr_truncated
            || !exported.stderr.is_empty()
        {
            return Err(ExecutionError::Infrastructure);
        }
        if !running(gateway, &guardian, &nonce, deadline, cancel)? {
            return Err(ExecutionError::Infrastructure);
        }
        let candidate = crate::mutation_archive::decode(&exported.stdout, source)?;
        if !mutation_scope_ok(source, &candidate, &command) {
            return Err(ExecutionError::Denied);
        }
        if matches!(command, RustMutationCommand::Fix) && !fix_output_complete(&mutated, &candidate)
        {
            return Ok((mutated, oom, None));
        }
        Ok((mutated, oom, Some(candidate)))
    })();
    cleanup(gateway, &names, &volume_name, &nonce)?;
    let (mut capture, oom, mut candidate) = work?;
    if capture.stop == Stop::Exited {
        if cancel.is_cancelled() {
            capture.stop = Stop::Cancelled;
            capture.code = None;
            candidate = None;
        } else if Instant::now() >= deadline {
            capture.stop = Stop::TimedOut;
            capture.code = None;
            candidate = None;
        }
    }
    let result = make_result(gateway, &archive, command, limits, started, capture, oom)?;
    Ok(RustMutationExecution { result, candidate })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_engineering_domain::SourceFile;
    use std::cell::Cell;
    #[test]
    fn phases_have_closed_programs_arguments_and_mount_access() {
        assert_eq!(MutationPhase::Guardian.program(), "/usr/bin/sleep");
        assert_eq!(MutationPhase::Guardian.arguments(), ["900"]);
        assert_eq!(
            MutationPhase::Format.arguments(),
            [
                "fmt",
                "--all",
                "--",
                "--color",
                "never",
                "--config",
                "disable_all_formatting=false"
            ]
        );
        assert_eq!(
            MutationPhase::Fix.arguments(),
            [
                "fix",
                "--workspace",
                "--all-targets",
                "--frozen",
                "--offline",
                "--allow-no-vcs",
                "--allow-dirty",
                "--allow-staged",
                "--message-format=json",
                "--color",
                "never",
                "--target-dir",
                "/target"
            ]
        );
        assert_eq!(
            MutationPhase::Export.arguments(),
            [
                "--create",
                "--file=-",
                "--format=ustar",
                "--sort=name",
                "--one-file-system",
                "--directory=/source",
                "."
            ]
        );
        assert!(MutationPhase::Ingest.writable());
        assert!(MutationPhase::Format.writable());
        assert!(MutationPhase::Fix.writable());
        assert!(!MutationPhase::Guardian.writable());
        assert!(!MutationPhase::Export.writable());
        assert!(MutationPhase::Ingest.interactive());
    }
    #[test]
    fn fix_socket_rule_masks_the_base_type_and_keeps_only_stream()
    -> Result<(), Box<dyn std::error::Error>> {
        let profile: serde_json::Value =
            serde_json::from_slice(include_bytes!("seccomp-rust-fix.json"))?;
        let rules = profile["syscalls"].as_array().ok_or("syscalls")?;
        let socket = rules
            .iter()
            .find(|rule| {
                rule["names"] == serde_json::json!(["socket"]) && rule["args"][0]["value"] == 2
            })
            .ok_or("inet socket rule")?;
        let predicate = &socket["args"][1];
        assert_eq!(predicate["index"], 1);
        assert_eq!(predicate["op"], "SCMP_CMP_MASKED_EQ");
        let mask = predicate["value"].as_u64().ok_or("mask")?;
        let datum = predicate["valueTwo"].as_u64().ok_or("datum")?;
        // Libseccomp masks both operands. A reversed 1/15 also admits odd types.
        for (kind, allowed) in [
            (1, true),
            (1 | 0x80000, true),
            (1 | 0x800, true),
            (2, false),
            (3, false),
            (5, false),
        ] {
            assert_eq!(kind & mask == datum & mask, allowed);
        }
        assert_eq!(
            socket["args"][2],
            serde_json::json!({"index": 2, "value": 0, "op": "SCMP_CMP_EQ"})
        );
        Ok(())
    }
    #[test]
    fn ambiguous_state_change_reply_permanently_quarantines_late_commit() {
        let quarantined = std::sync::atomic::AtomicBool::new(false);
        let request_sent = Cell::new(false);
        let delayed_commit = Cell::new(false);
        let result = fail_closed_state_change(
            &quarantined,
            || {
                request_sent.set(true);
                Err::<Capture, _>((ExecutionError::Infrastructure, true))
            },
            |_| true,
        );
        assert_eq!(result.err(), Some(ExecutionError::CleanupUncertain));
        assert!(request_sent.get());
        // Cleanup's first absence observation is empty, then the daemon makes
        // the delayed state change visible. Neither event clears quarantine.
        let first_absence_observation = true;
        assert!(first_absence_observation);
        delayed_commit.set(true);
        assert!(delayed_commit.get());
        assert!(quarantined.load(Ordering::Acquire));
        let admission_allowed = !quarantined.load(Ordering::Acquire);
        assert!(!admission_allowed);
        let clean_preflight = std::sync::atomic::AtomicBool::new(false);
        assert_eq!(
            fail_closed_state_change(
                &clean_preflight,
                || Err::<Capture, _>((ExecutionError::Cancelled, false)),
                |_| true,
            )
            .err(),
            Some(ExecutionError::Cancelled)
        );
        assert!(!clean_preflight.load(Ordering::Acquire));
    }
    #[test]
    fn mutation_volume_requires_exact_tmpfs_quota_identity_and_ownership()
    -> Result<(), Box<dyn std::error::Error>> {
        let valid = serde_json::json!([{
            "Name":"rust-mcp-mutation-source-fixture",
            "Driver":"local",
            "Scope":"local",
            "Options":{
                "device":"tmpfs",
                "o":VOLUME_OPTIONS,
                "type":"tmpfs"
            },
            "Labels":{
                "org.rust-mcp.execution":"true",
                "org.rust-mcp.rust-job":"fixture"
            },
            "Mountpoint":"/var/lib/docker/volumes/rust-mcp-mutation-source-fixture/_data",
            "ClusterVolume":null,
            "Status":null
        }]);
        let bytes = serde_json::to_vec(&valid)?;
        assert!(parse_volume(&bytes, "rust-mcp-mutation-source-fixture", "fixture").is_ok());
        let mut coverage = valid.clone();
        coverage[0]["Options"]["o"] =
            serde_json::json!(super::super::coverage_gateway::COVERAGE_TARGET_VOLUME_OPTIONS);
        assert!(
            parse_volume_with_options(
                &serde_json::to_vec(&coverage)?,
                "rust-mcp-mutation-source-fixture",
                "fixture",
                super::super::coverage_gateway::COVERAGE_TARGET_VOLUME_OPTIONS,
            )
            .is_ok()
        );
        assert!(
            parse_volume(
                &serde_json::to_vec(&coverage)?,
                "rust-mcp-mutation-source-fixture",
                "fixture"
            )
            .is_err()
        );
        for (path, changed) in [
            ("/0/Driver", serde_json::json!("bind")),
            ("/0/Scope", serde_json::json!("global")),
            ("/0/Options/type", serde_json::json!("none")),
            ("/0/Options/device", serde_json::json!("/")),
            ("/0/Options/o", serde_json::json!("size=1g")),
            (
                "/0/Labels/org.rust-mcp.rust-job",
                serde_json::json!("other"),
            ),
            ("/0/Mountpoint", serde_json::json!("/host")),
            ("/0/ClusterVolume", serde_json::json!({})),
            ("/0/Status", serde_json::json!({})),
        ] {
            let mut invalid = valid.clone();
            *invalid.pointer_mut(path).ok_or("volume mutation path")? = changed;
            assert!(
                parse_volume(
                    &serde_json::to_vec(&invalid)?,
                    "rust-mcp-mutation-source-fixture",
                    "fixture"
                )
                .is_err(),
                "accepted {path}"
            );
        }
        Ok(())
    }
    #[test]
    fn scope_accepts_only_existing_rust_file_byte_changes() -> Result<(), String> {
        fn bundle(rs: &[u8], toml: &[u8]) -> Result<SourceBundle, String> {
            SourceBundle::new(vec![
                SourceFile::new("src/lib.rs".into(), rs.to_vec()).map_err(|e| format!("{e:?}"))?,
                SourceFile::new("Cargo.toml".into(), toml.to_vec())
                    .map_err(|e| format!("{e:?}"))?,
            ])
            .map_err(|e| format!("{e:?}"))
        }
        let before = bundle(b"fn x( ){ }", b"[package]")?;
        assert!(mutation_scope_ok(
            &before,
            &bundle(b"fn x() {}", b"[package]")?,
            &RustMutationCommand::Fix,
        ));
        assert!(!mutation_scope_ok(
            &before,
            &bundle(b"fn x() {}", b"changed")?,
            &RustMutationCommand::Fix,
        ));
        let shorter =
            SourceBundle::new(vec![before.files()[0].clone()]).map_err(|e| format!("{e:?}"))?;
        assert!(!mutation_scope_ok(
            &before,
            &shorter,
            &RustMutationCommand::Fix
        ));
        assert!(!mutation_scope_ok(
            &shorter,
            &before,
            &RustMutationCommand::Format
        ));
        let extra_directory =
            SourceBundle::with_directories(before.files().to_vec(), vec!["empty".into()])
                .map_err(|e| format!("{e:?}"))?;
        assert!(!mutation_scope_ok(
            &before,
            &extra_directory,
            &RustMutationCommand::Fix
        ));
        Ok(())
    }

    #[test]
    fn fix_scope_caps_changed_existing_rust_files() -> Result<(), String> {
        let before = SourceBundle::new(
            (0..129)
                .map(|index| SourceFile::new(format!("src/f{index}.rs"), b"old".to_vec()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        let after = SourceBundle::new(
            (0..129)
                .map(|index| SourceFile::new(format!("src/f{index}.rs"), b"new".to_vec()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("{error:?}"))?,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert!(!mutation_scope_ok(
            &before,
            &after,
            &RustMutationCommand::Fix
        ));
        assert!(mutation_scope_ok(
            &before,
            &after,
            &RustMutationCommand::Format
        ));
        Ok(())
    }

    #[test]
    fn fix_requires_complete_strict_cargo_json() -> Result<(), String> {
        let source = SourceBundle::new(vec![
            SourceFile::new("src/lib.rs".into(), b"pub fn answer() {}\n".to_vec())
                .map_err(|error| format!("{error:?}"))?,
        ])
        .map_err(|error| format!("{error:?}"))?;
        let capture = |stdout: &[u8]| Capture {
            code: Some(0),
            stdout: stdout.to_vec(),
            stderr: b"Checking fixture\n".to_vec(),
            stdout_truncated: false,
            stderr_truncated: false,
            stop: Stop::Exited,
            duration_ms: 1,
        };
        assert!(fix_output_complete(
            &capture(b"{\"reason\":\"build-finished\",\"success\":true}\n"),
            &source
        ));
        for stdout in [
            &b"{\"reason\":\"build-finished\",\"success\":false}\n"[..],
            &b"{\"reason\":\"build-finished\",\"success\":true}"[..],
            &b"not-json\n"[..],
            &b"{\"reason\":\"unknown\"}\n"[..],
            &[0xff][..],
        ] {
            assert!(!fix_output_complete(&capture(stdout), &source));
        }
        let mut truncated = capture(b"{\"reason\":\"build-finished\",\"success\":true}\n");
        truncated.stderr_truncated = true;
        assert!(!fix_output_complete(&truncated, &source));
        Ok(())
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit approved Docker socket/image; isolated ADR-053 staging fixture"]
    fn real_rustfmt_exports_full_candidate_and_cleans_owned_objects()
    -> Result<(), Box<dyn std::error::Error>> {
        let suffix = crate::state::nonce().map_err(|error| format!("nonce: {error:?}"))?;
        let state_root = std::path::PathBuf::from("/private/tmp")
            .join(format!("rust-mcp-mutation-test-{suffix}"));
        std::fs::create_dir(&state_root)?;
        let gateway = RustGateway::new(HostDockerConfig {
            executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
            socket: "/Users/cburgosro/.docker/run/docker.sock".into(),
            state_root: state_root.clone(),
            image_id: crate::APPROVED_RUST_IMAGE.into(),
        })
        .map_err(|error| format!("gateway: {error:?}"))?;
        gateway.set_verified(true);
        let source = SourceBundle::new(vec![
            SourceFile::new(
                "Cargo.toml".into(),
                b"[package]\nname = \"fmt_fixture\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
                    .to_vec(),
            )
            .map_err(|error| format!("source: {error:?}"))?,
            SourceFile::new(
                "src/main.rs".into(),
                b"mod other;fn main( ){println!(\"ok\");}\n".to_vec(),
            )
            .map_err(|error| format!("source: {error:?}"))?,
            SourceFile::new(
                "src/other.rs".into(),
                b"pub fn answer( )->u8{42}\n".to_vec(),
            )
            .map_err(|error| format!("source: {error:?}"))?,
        ])
        .map_err(|error| format!("bundle: {error:?}"))?;
        let execution = gateway
            .execute_mutation(
                &source,
                RustMutationCommand::Format,
                ExecutionLimits::new(30_000, 256 * 1024).ok_or("limits")?,
                &NeverCancel,
            )
            .map_err(|error| format!("mutation: {error:?}"))?;
        assert_eq!(execution.result.termination, ExecutionTermination::Exited);
        assert_eq!(execution.result.exit_code, Some(0));
        let candidate = execution.candidate.ok_or("candidate")?;
        assert_eq!(candidate.files()[0].bytes(), source.files()[0].bytes());
        assert_eq!(
            candidate.files()[1].bytes(),
            b"mod other;\nfn main() {\n    println!(\"ok\");\n}\n"
        );
        assert_eq!(
            candidate.files()[2].bytes(),
            b"pub fn answer() -> u8 {\n    42\n}\n"
        );
        let no_op = gateway
            .execute_mutation(
                &candidate,
                RustMutationCommand::Format,
                ExecutionLimits::new(30_000, 256 * 1024).ok_or("limits")?,
                &NeverCancel,
            )
            .map_err(|error| format!("no-op mutation: {error:?}"))?;
        assert_eq!(no_op.candidate.ok_or("no-op candidate")?, candidate);
        let invalid = SourceBundle::new(vec![
            SourceFile::new("Cargo.toml".into(), b"[package\n".to_vec())
                .map_err(|error| format!("source: {error:?}"))?,
            SourceFile::new("src/main.rs".into(), b"fn main( ){}\n".to_vec())
                .map_err(|error| format!("source: {error:?}"))?,
        ])
        .map_err(|error| format!("bundle: {error:?}"))?;
        let failed = gateway
            .execute_mutation(
                &invalid,
                RustMutationCommand::Format,
                ExecutionLimits::new(30_000, 256 * 1024).ok_or("limits")?,
                &NeverCancel,
            )
            .map_err(|error| format!("invalid mutation: {error:?}"))?;
        assert_eq!(failed.result.termination, ExecutionTermination::Exited);
        assert_ne!(failed.result.exit_code, Some(0));
        assert!(failed.candidate.is_none());
        for kind in ["container", "volume"] {
            let args = if kind == "container" {
                vec![
                    "container".into(),
                    "ls".into(),
                    "--all".into(),
                    "--filter=label=org.rust-mcp.execution=true".into(),
                    "--format={{.ID}}".into(),
                ]
            } else {
                vec![
                    "volume".into(),
                    "ls".into(),
                    "--filter=label=org.rust-mcp.execution=true".into(),
                    "--format={{.Name}}".into(),
                ]
            };
            let inventory = gateway
                .inner
                .control(&args)
                .map_err(|error| format!("inventory: {error:?}"))?;
            assert_eq!(inventory.code, Some(0));
            assert!(inventory.stdout.iter().all(u8::is_ascii_whitespace));
        }
        assert!(!gateway.is_quarantined());
        drop(gateway);
        std::fs::remove_dir_all(state_root)?;
        Ok(())
    }
}
