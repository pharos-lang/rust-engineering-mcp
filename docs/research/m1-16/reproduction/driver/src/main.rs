//! Trusted experiment controller IPC. This is an MCP SDK client, not an MCP server.
use rmcp::{
    RoleClient, ServiceExt,
    model::*,
    service::{PeerRequestOptions, RunningService},
};
use rust_engineering_application::{ExecutionCancellation, ExecutionError};
use rust_engineering_domain::{
    ClippyOptions, ClippySelection, ExecutionLimits, LintProfile, RustCommand, SourceBundle,
    SourceFile, TestOptions, TestSelection,
};
use rust_engineering_execution::{APPROVED_RUST_IMAGE, HostDockerConfig, RustGateway};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::PathBuf,
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::{Notify, mpsc},
};
const LIMIT: usize = 1024 * 1024;
const DOCKER: &str = "/Applications/Docker.app/Contents/Resources/bin/docker";
const TOOLS: [&str; 13] = [
    "rust.project.open",
    "rust.project.inspect",
    "rust.toolchain.inspect",
    "rust.check",
    "rust.fmt.check",
    "rust.clippy",
    "rust.test",
    "rust.dependencies.audit",
    "rust.diagnostics.explain",
    "rust.quality.gate",
    "rust.catalog.status",
    "rust.crate.search",
    "rust.crate.inspect",
];
type Result<T> = std::result::Result<T, &'static str>;
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Raw,
    Mcp,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Init {
    mode: Mode,
    server_binary: PathBuf,
    root: PathBuf,
    state_root: PathBuf,
    docker_socket: PathBuf,
    catalog_store: Option<PathBuf>,
    catalog_trust: Option<PathBuf>,
    model_dir: Option<PathBuf>,
    index_store: Option<PathBuf>,
    rustsec_path: Option<PathBuf>,
    rustsec_sha256: Option<String>,
    stderr_path: Option<PathBuf>,
}
impl Init {
    fn validate(&self) -> Result<()> {
        for p in [
            &self.server_binary,
            &self.root,
            &self.state_root,
            &self.docker_socket,
        ]
        .into_iter()
        .chain(
            [
                self.catalog_store.as_ref(),
                self.catalog_trust.as_ref(),
                self.model_dir.as_ref(),
                self.index_store.as_ref(),
                self.rustsec_path.as_ref(),
                self.stderr_path.as_ref(),
            ]
            .into_iter()
            .flatten(),
        ) {
            if !p.is_absolute() || p.to_str().is_none_or(|s| s.chars().any(char::is_control)) {
                return Err("invalid_host_path");
            }
        }
        if self.catalog_store.is_some() != self.catalog_trust.is_some()
            || (self.model_dir.is_some() && self.catalog_store.is_none())
            || (self.index_store.is_some() && self.model_dir.is_none())
            || self.rustsec_path.is_some() != self.rustsec_sha256.is_some()
        {
            return Err("incomplete_host_configuration");
        }
        if self.rustsec_sha256.as_ref().is_some_and(|s| {
            s.parse::<rust_engineering_domain::CatalogFingerprint>()
                .is_err()
        }) {
            return Err("invalid_fingerprint");
        }
        Ok(())
    }
    fn host(&self) -> HostDockerConfig {
        HostDockerConfig {
            executable: DOCKER.into(),
            socket: self.docker_socket.clone(),
            state_root: self.state_root.clone(),
            image_id: APPROVED_RUST_IMAGE.into(),
        }
    }
    fn flags(&self) -> Vec<std::ffi::OsString> {
        let mut args = vec![
            "serve".into(),
            "--stdio".into(),
            "--root".into(),
            self.root.as_os_str().into(),
            "--docker".into(),
            DOCKER.into(),
            "--docker-socket".into(),
            self.docker_socket.as_os_str().into(),
            "--state-root".into(),
            self.state_root.as_os_str().into(),
            "--rust-image".into(),
            APPROVED_RUST_IMAGE.into(),
        ];
        for (flag, path) in [
            ("--catalog-store", self.catalog_store.as_ref()),
            ("--catalog-trust", self.catalog_trust.as_ref()),
            ("--catalog-model-dir", self.model_dir.as_ref()),
            ("--catalog-index-store", self.index_store.as_ref()),
            ("--rustsec-snapshot", self.rustsec_path.as_ref()),
        ] {
            if let Some(path) = path {
                args.push(flag.into());
                args.push(path.as_os_str().into());
            }
        }
        if let Some(hash) = &self.rustsec_sha256 {
            args.push("--rustsec-sha256".into());
            args.push(hash.into());
        }
        args
    }
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Tools {},
    Call {
        name: String,
        arguments: serde_json::Map<String, Value>,
    },
    Resource {
        uri: String,
    },
    Execute {
        files: Vec<File>,
        command: Command,
        code: Option<String>,
    },
    Close {},
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    text: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Command {
    Check,
    Fmt,
    Clippy,
    Test,
    Metadata,
    Explain,
}
fn source(files: Vec<File>) -> Result<SourceBundle> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for f in files {
        if ![
            "Cargo.toml",
            "Cargo.lock",
            "src/lib.rs",
            "tests/behavior.rs",
        ]
        .contains(&f.path.as_str())
            || !seen.insert(f.path.clone())
        {
            return Err("denied_source_path");
        }
        result.push(SourceFile::new(f.path, f.text.into_bytes()).map_err(|_| "invalid_source")?);
    }
    if !["Cargo.toml", "Cargo.lock", "src/lib.rs"]
        .iter()
        .all(|name| seen.contains(*name))
    {
        return Err("missing_source_file");
    }
    SourceBundle::new(result).map_err(|_| "invalid_source")
}
fn command(value: Command, code: Option<String>) -> Result<RustCommand> {
    if !matches!(value, Command::Explain) && code.is_some() {
        return Err("unexpected_diagnostic_code");
    }
    Ok(match value {
        Command::Check => RustCommand::Check,
        Command::Fmt => RustCommand::FormatCheck,
        Command::Metadata => RustCommand::Metadata,
        Command::Explain => RustCommand::Explain(
            code.ok_or("missing_diagnostic_code")?
                .parse()
                .map_err(|_| "invalid_diagnostic_code")?,
        ),
        Command::Clippy => RustCommand::ClippyProject(
            ClippyOptions::try_from(ClippySelection {
                lint_profile: LintProfile::Strict,
                ..Default::default()
            })
            .map_err(|_| "invalid_fixed_command")?,
        ),
        Command::Test => RustCommand::TestProject(
            TestOptions::try_from(TestSelection {
                timeout: 30,
                ..Default::default()
            })
            .map_err(|_| "invalid_fixed_command")?,
        ),
    })
}
#[derive(Default)]
struct Cancel {
    flag: AtomicBool,
    execution_joined: AtomicBool,
    server_joined: AtomicBool,
    cleanup_uncertain: AtomicBool,
    server_exit_status: Mutex<Option<Value>>,
    notify: Notify,
}
impl Cancel {
    fn uncertain(&self, error: &'static str) -> &'static str {
        self.cleanup_uncertain.store(true, Ordering::SeqCst);
        error
    }
    fn gateway_error(&self, error: ExecutionError, stage: &'static str) -> &'static str {
        match error {
            ExecutionError::CleanupUncertain => self.uncertain("gateway_cleanup_uncertain"),
            ExecutionError::Cancelled => "cancelled",
            _ => stage,
        }
    }
    fn after_cancellation<T>(&self, result: Result<T>) -> Result<T> {
        // A completed join does not erase an execution/cleanup error, even if
        // cancellation raced with the return of the blocking gateway call.
        result.and_then(|value| {
            if self.is_cancelled() {
                Err("cancelled")
            } else {
                Ok(value)
            }
        })
    }
    fn worker_result<T>(
        &self,
        result: std::result::Result<Result<T>, tokio::task::JoinError>,
    ) -> Result<T> {
        result.map_err(|_| self.uncertain("gateway_worker_panicked"))?
    }
    fn server_exit(&self, result: std::io::Result<std::process::ExitStatus>) -> Result<()> {
        use std::os::unix::process::ExitStatusExt;
        let status = result.map_err(|_| self.uncertain("child_join"))?;
        *self
            .server_exit_status
            .lock()
            .map_err(|_| self.uncertain("exit_status_poisoned"))? = Some(
            json!({"code":status.code(), "signal":status.signal(), "success":status.success()}),
        );
        self.server_joined.store(true, Ordering::SeqCst);
        if !status.success() {
            return Err(self.uncertain("child_failed"));
        }
        Ok(())
    }
    fn acknowledgement(&self, error: Option<&str>) -> Value {
        let mut value = json!({
            "execution_joined": self.execution_joined.load(Ordering::SeqCst),
            "server_joined": self.server_joined.load(Ordering::SeqCst),
            "cleanup_uncertain": self.cleanup_uncertain.load(Ordering::SeqCst),
            "server_exit": self.server_exit_status.lock().ok().and_then(|status| status.clone()),
        });
        if let Some(error) = error {
            value["driver_error"] = json!(error);
            value["success"] = json!(false);
        } else {
            value["closed"] = json!(true);
        }
        value
    }
    fn mcp_failure(&self, error: rmcp::service::ServiceError) -> Result<Value> {
        match error {
            rmcp::service::ServiceError::Cancelled { .. } if self.is_cancelled() => {
                Err("cancelled")
            }
            rmcp::service::ServiceError::McpError(error) => {
                // The public protocol maps cleanup failures and worker panics
                // to internal errors. Never infer verified cleanup from them.
                if error.code == ErrorCode::INTERNAL_ERROR {
                    return Err(self.uncertain("mcp_internal_error"));
                }
                Ok(json!({"mcp_error":error}))
            }
            _ => Err(self.uncertain("mcp_request_failed")),
        }
    }
    fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
    async fn wait(&self) {
        loop {
            let notified = self.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }
}
impl ExecutionCancellation for Cancel {
    fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}
async fn line(reader: &mut (impl AsyncBufReadExt + Unpin)) -> Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    loop {
        let buffer = reader.fill_buf().await.map_err(|_| "input_io")?;
        if buffer.is_empty() {
            return if bytes.is_empty() {
                Ok(None)
            } else {
                Err("unterminated_line")
            };
        }
        let end = buffer.iter().position(|v| *v == b'\n').map(|n| n + 1);
        let count = end.unwrap_or(buffer.len());
        if bytes.len() + count > LIMIT {
            return Err("input_limit");
        };
        bytes.extend_from_slice(&buffer[..count]);
        reader.consume(count);
        if end.is_some() {
            return Ok(Some(bytes));
        }
    }
}
async fn input_task(tx: mpsc::Sender<Result<Vec<u8>>>, cancel: Arc<Cancel>) {
    let mut stdin = BufReader::new(tokio::io::stdin());
    loop {
        match line(&mut stdin).await {
            Ok(Some(v)) => {
                if tx.send(Ok(v)).await.is_err() {
                    return;
                }
            }
            Ok(None) => {
                cancel.cancel();
                return;
            }
            Err(e) => {
                cancel.cancel();
                let _ = tx.send(Err(e)).await;
                return;
            }
        }
    }
}
async fn output(value: Value) -> Result<()> {
    let mut bytes = serde_json::to_vec(&value).map_err(|_| "serialization")?;
    if bytes.len() + 1 > LIMIT {
        return Err("output_limit");
    };
    bytes.push(b'\n');
    let mut out = tokio::io::stdout();
    tokio::time::timeout(Duration::from_secs(5), async {
        out.write_all(&bytes).await?;
        out.flush().await
    })
    .await
    .map_err(|_| "output_timeout")?
    .map_err(|_| "output_io")
}
#[derive(Serialize)]
struct StderrStats {
    bytes: u64,
    retained_bytes: usize,
    truncated: bool,
}
async fn capture_stderr(
    mut reader: tokio::process::ChildStderr,
    mut log: Option<std::fs::File>,
) -> Result<StderrStats> {
    use std::io::Write;
    let mut buffer = [0_u8; 8192];
    let mut retained = Vec::new();
    let mut count = 0_u64;
    loop {
        let n = reader.read(&mut buffer).await.map_err(|_| "stderr_io")?;
        if n == 0 {
            break;
        };
        count = count.saturating_add(n as u64);
        let take = n.min((256 * 1024_usize).saturating_sub(retained.len()));
        retained.extend_from_slice(&buffer[..take]);
    }
    if let Some(file) = log.as_mut() {
        file.write_all(&retained)
            .and_then(|_| file.flush())
            .map_err(|_| "stderr_log_write")?;
    }
    Ok(StderrStats {
        bytes: count,
        retained_bytes: retained.len(),
        truncated: count > retained.len() as u64,
    })
}
async fn drain_stdout(mut reader: impl AsyncReadExt + Unpin) -> Result<u64> {
    let mut buffer = [0_u8; 8192];
    let mut bytes = 0_u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .await
            .map_err(|_| "shutdown_stdout_io")?;
        if count == 0 {
            break;
        }
        bytes = bytes.saturating_add(count as u64);
    }
    if bytes > LIMIT as u64 {
        return Err("shutdown_stdout_limit");
    }
    Ok(bytes)
}
fn retain_stdout(stdout: &tokio::process::ChildStdout) -> Result<tokio::process::ChildStdout> {
    use std::os::fd::AsFd;
    let drain_fd = stdout
        .as_fd()
        .try_clone_to_owned()
        .map_err(|_| "server_stdout_clone")?;
    tokio::process::ChildStdout::from_std(std::process::ChildStdout::from(drain_fd))
        .map_err(|_| "server_stdout_clone")
}
struct Mcp {
    service: Option<RunningService<RoleClient, ()>>,
    child: tokio::process::Child,
    stderr: tokio::task::JoinHandle<Result<StderrStats>>,
    stdout_drain: tokio::process::ChildStdout,
}
impl Mcp {
    async fn new(init: &Init, cancel: &Cancel) -> Result<Self> {
        use std::os::unix::fs::OpenOptionsExt;
        let log = init
            .stderr_path
            .as_ref()
            .map(|path| {
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(path)
                    .map_err(|_| "stderr_log_create")
            })
            .transpose()?;
        let mut child = tokio::process::Command::new(&init.server_binary)
            .args(init.flags())
            .env_clear()
            .current_dir(&init.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "server_spawn")?;
        let stderr = child.stderr.take().ok_or("server_stderr")?;
        let stderr = tokio::spawn(capture_stderr(stderr, log));
        let stdout = child.stdout.take().ok_or("server_stdout")?;
        // rmcp 3.2.0 drains late handler replies during server shutdown, even
        // after a cancelled request's local client response has completed.
        // Retain the read endpoint while the SDK closes stdin; otherwise the
        // product correctly treats the resulting broken pipe as an I/O failure.
        let stdout_drain = retain_stdout(&stdout)?;
        let stdin = child.stdin.take().ok_or("server_stdin")?;
        let result = tokio::select! {value=tokio::time::timeout(Duration::from_secs(30),().serve((stdout,stdin)))=>value.map_err(|_|"initialize_timeout").and_then(|v|v.map_err(|_|"initialize_failed")),_=cancel.wait()=>Err("cancelled")};
        match result {
            Ok(service) => Ok(Self {
                service: Some(service),
                child,
                stderr,
                stdout_drain,
            }),
            Err(e) => {
                let (child_status, drained, stderr_status) =
                    tokio::join!(child.wait(), drain_stdout(stdout_drain), stderr);
                let child_result = cancel.server_exit(child_status);
                let stderr_result = stderr_status.map_err(|_| cancel.uncertain("stderr_worker"));
                child_result?;
                drained?;
                stderr_result??;
                Err(e)
            }
        }
    }
    async fn request(&self, input: Input, cancel: &Cancel) -> Result<Value> {
        let request = match input {
            Input::Tools {} => ClientRequest::ListToolsRequest(Default::default()),
            Input::Call { name, arguments } => {
                if !TOOLS.contains(&name.as_str()) {
                    return Err("denied_tool");
                };
                ClientRequest::CallToolRequest(Request::new(
                    CallToolRequestParams::new(name).with_arguments(arguments),
                ))
            }
            Input::Resource { uri } => {
                if uri.len() > 4096 || !valid_resource(&uri) || uri.chars().any(char::is_control) {
                    return Err("denied_resource");
                };
                ClientRequest::ReadResourceRequest(Request::new(ReadResourceRequestParams::new(
                    uri,
                )))
            }
            _ => return Err("wrong_mode"),
        };
        let peer = self.service.as_ref().ok_or("service_closed")?.peer();
        let handle = peer
            .send_cancellable_request(
                request,
                PeerRequestOptions::with_timeout(Duration::from_secs(900)),
            )
            .await
            .map_err(|_| cancel.uncertain("mcp_send"))?;
        let id = handle.id.clone();
        let response = handle.await_response();
        tokio::pin!(response);
        let result = tokio::select! {value=&mut response=>value,_=cancel.wait()=>{let _=peer.notify_cancelled(CancelledNotificationParam::new(Some(id),Some("trusted controller cancellation".into()))).await;response.await}};
        match result {
            Ok(value) => serde_json::to_value(value).map_err(|_| "serialization"),
            Err(error) => cancel.mcp_failure(error),
        }
    }
    async fn close(mut self, control: &Cancel) -> Result<StderrStats> {
        let transport = if let Some(service) = self.service.take() {
            service
                .cancel()
                .await
                .map(|_| ())
                .map_err(|_| control.uncertain("mcp_shutdown"))
        } else {
            Ok(())
        };
        // Even a transport shutdown error must not skip the owned product process join.
        let (child_status, drained, stderr_status) = tokio::join!(
            self.child.wait(),
            drain_stdout(self.stdout_drain),
            self.stderr
        );
        let child = control.server_exit(child_status);
        let stats = stderr_status.map_err(|_| control.uncertain("stderr_worker"));
        child?;
        transport?;
        drained?;
        stats?
    }
}
async fn run(cancel: Arc<Cancel>) -> Result<()> {
    let (tx, mut rx) = mpsc::channel(1);
    let reader = tokio::spawn(input_task(tx, Arc::clone(&cancel)));
    let first = tokio::select! {value=rx.recv()=>value.ok_or("missing_init")??,_=cancel.wait()=>return Err("cancelled")};
    let init: Init = serde_json::from_slice(&first).map_err(|_| "invalid_init")?;
    init.validate()?;
    if cancel.is_cancelled() {
        return Err("cancelled");
    }
    let mut mcp = match init.mode {
        Mode::Mcp => Some(Mcp::new(&init, &cancel).await?),
        Mode::Raw => None,
    };
    let gateway: Arc<Mutex<Option<RustGateway>>> = Arc::new(Mutex::new(None));
    let server_pid = mcp.as_ref().and_then(|client| client.child.id());
    let negotiated_protocol = mcp
        .as_ref()
        .and_then(|client| client.service.as_ref())
        .and_then(|service| service.peer().peer_info())
        .map(|info| info.protocol_version.clone());
    let mut outcome = output(json!({"ready":true,"ipc_version":1,"server_pid":server_pid,"negotiated_protocol":negotiated_protocol})).await;
    while outcome.is_ok() && !cancel.is_cancelled() {
        let bytes = tokio::select! {v=rx.recv()=>v,_=cancel.wait()=>None};
        let Some(bytes) = bytes else { break };
        let value = bytes.and_then(|bytes| {
            serde_json::from_slice::<Input>(&bytes).map_err(|_| "invalid_request")
        });
        let result = match value {
            Ok(Input::Close {}) => break,
            Ok(Input::Execute {
                files,
                command: cmd,
                code,
            }) if matches!(init.mode, Mode::Raw) => {
                match source(files).and_then(|src| command(cmd, code).map(|cmd| (src, cmd))) {
                    Err(e) => Err(e),
                    Ok((source, command)) => {
                        let host = init.host();
                        let state = Arc::clone(&gateway);
                        let control = Arc::clone(&cancel);
                        let worker = tokio::task::spawn_blocking(move || {
                            if control.is_cancelled() {
                                return Err("cancelled");
                            };
                            let mut guard = state
                                .lock()
                                .map_err(|_| control.uncertain("gateway_poisoned"))?;
                            if guard.is_none() {
                                let value = RustGateway::new(host).map_err(|e| {
                                    control.gateway_error(e, "gateway_initialization")
                                })?;
                                value
                                    .calibrate(control.as_ref())
                                    .map_err(|e| control.gateway_error(e, "gateway_calibration"))?;
                                *guard = Some(value);
                            }
                            let gateway = guard.as_ref().ok_or("gateway_missing")?;
                            let limits =
                                ExecutionLimits::new(30_000, 256 * 1024).ok_or("invalid_limits")?;
                            let result = gateway
                                .execute(&source, command, limits, control.as_ref())
                                .map_err(|e| {
                                    control.gateway_error(e, "gateway_execution_or_cleanup")
                                })?;
                            serde_json::to_value(result).map_err(|_| "serialization")
                        })
                        .await;
                        cancel.worker_result(worker)
                    }
                }
            }
            Ok(request) => {
                if let Some(client) = &mcp {
                    client.request(request, &cancel).await
                } else {
                    Err("wrong_mode")
                }
            }
            Err(e) => Err(e),
        };
        if cancel.is_cancelled() {
            outcome = cancel.after_cancellation(result.map(|_| ()));
            break;
        }
        match result {
            Ok(value) => outcome = output(value).await,
            Err(error) => {
                outcome = output(json!({"driver_error":error})).await;
                if matches!(
                    error,
                    "mcp_request_failed"
                        | "mcp_send"
                        | "mcp_internal_error"
                        | "gateway_execution_or_cleanup"
                        | "gateway_cleanup_uncertain"
                        | "gateway_poisoned"
                        | "gateway_calibration"
                        | "gateway_worker_panicked"
                ) {
                    cancel.cancel();
                    outcome = Err(error);
                }
            }
        }
    }
    reader.abort();
    cancel.execution_joined.store(true, Ordering::SeqCst);
    let mut stderr = None;
    if let Some(client) = mcp.take() {
        match client.close(&cancel).await {
            Ok(stats) => stderr = Some(stats),
            Err(error) => outcome = Err(error),
        }
    }
    cancel.after_cancellation(outcome)?;
    let mut acknowledgement = cancel.acknowledgement(None);
    acknowledgement["stderr"] = json!(stderr);
    output(acknowledgement).await
}
fn main() {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(v) => v,
        Err(_) => {
            eprintln!("driver runtime unavailable");
            std::process::exit(1)
        }
    };
    let result=runtime.block_on(async{let cancel=Arc::new(Cancel::default());let mut interrupt=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).map_err(|_|"signal_registration")?;let mut terminate=tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).map_err(|_|"signal_registration")?;let control=Arc::clone(&cancel);let signals=tokio::spawn(async move{loop{tokio::select!{_=interrupt.recv()=>control.cancel(),_=terminate.recv()=>control.cancel()}}});let result=run(Arc::clone(&cancel)).await;signals.abort();if let Err(error)=result{let _=output(cancel.acknowledgement(Some(error))).await;}result});
    runtime.shutdown_timeout(Duration::from_millis(100));
    if result.is_err() {
        std::process::exit(1)
    }
}
fn valid_resource(uri: &str) -> bool {
    let Some((project, artifact)) = uri
        .strip_prefix("rust-artifact://")
        .and_then(|s| s.split_once('/'))
    else {
        return false;
    };
    project
        .parse::<rust_engineering_domain::ProjectRef>()
        .is_ok()
        && artifact
            .parse::<rust_engineering_domain::ArtifactId>()
            .is_ok()
}
#[cfg(test)]
mod tests;
