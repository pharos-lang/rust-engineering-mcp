//! Native calibration of the M6 analyzer runtime (ADR-082/084/085).
//!
//! Never part of runtime discovery: every test here is `#[ignore]`d, demands an
//! explicit `RUST_MCP_TEST_SOCKET`, requires the admitted M6 image by digest, and
//! assumes exclusive ownership of the local Docker engine. One `#[test]` per cut
//! writes `target/m6-calibration/cut-<name>.json` and rebuilds the merged
//! `target/m6-calibration/receipt.json` from every cut written so far.
//!
//! Why these cuts and not a single happy path: every value ADR-084 marks
//! "subject to native calibration" needs a *local* oracle, because the research
//! that proposed them was rejected as evidence. So the version line and the
//! binary digest are read out of the live guest, the fixed configuration keys are
//! checked against the binary's own schema (rust-analyzer ignores unknown keys in
//! silence, so a typo is otherwise invisible), the negotiated encoding and the
//! readiness transcript come from a real `initialize`, and the process table is
//! sampled during indexing because "build scripts are disabled" is a claim about
//! what runs, not about what was configured.
//!
//! A cut that cannot be made to pass is recorded as `fail` or `not_run` with its
//! reproducible condition. None is ever weakened into a pass.
use crate::analyzer_gateway::{
    self, ANALYZER_BINARY_SHA256, ANALYZER_VERSION, APPROVED_M6_IMAGE, AnalyzerBudgets,
};
use crate::rust_gateway::{AnalyzerDocument, Phase};
use crate::*;
use rust_engineering_domain as domain;
use rust_engineering_domain::{AnalyzerQuery, SourceBundle, SourceFile};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::time::{Duration, Instant};

type Failure = Box<dyn std::error::Error>;

/// The Docker client this calibration drives. Overridable only so the same test
/// can run on a host that installed the client elsewhere; the socket and the
/// image always come from the environment contract.
const DOCKER: &str = "/Applications/Docker.app/Contents/Resources/bin/docker";
const LABEL: &str = "--filter=label=org.rust-mcp.execution=true";
const RECEIPT_SCHEMA: &str = "rust-engineering-mcp.m6-calibration.v1";
const IMAGE_TAG_AT_PROVISIONING: &str = "rust-engineering-runtime:1.98.1-arm64-m6";
/// The ADR-084 §8 total per call, and the 512 KiB log ceiling the other
/// verticals use for a bounded job.
const TOTAL_MS: u64 = domain::TOTAL_TIMEOUT_SECONDS * 1_000;
const LOG_BYTES: usize = 512 * 1024;
/// Whether a guest command line is one ADR-084 §7 admits.
///
/// The oracle is the **whole argv against a closed list**, not the program's
/// basename, because the basename cannot see the thing this cut exists to
/// refuse: rust-analyzer starts its proc-macro server as
/// `<the rust-analyzer binary> proc-macro`, whose basename is the very name the
/// entrypoint uses. Anything not spelled out below — a proc-macro server, a
/// build script, `rustfmt`, a shell, a `cargo` subcommand that builds — fails
/// the cut, which is the only way an allowlist can mean anything.
///
/// The `rustc` and `cargo` forms are the ones §7 expects rust-analyzer to run
/// while it loads a workspace: version and target probes, `locate-project`, and
/// `cargo metadata` without dependencies.
fn admitted_argv(argv: &str) -> bool {
    let mut words = argv.split_whitespace();
    let Some(program) = words.next() else {
        return false;
    };
    let arguments = words.collect::<Vec<_>>();
    match program {
        // The entrypoint of `Phase::Analyzer`: no arguments, ever.
        "/opt/analyzer/bin/rust-analyzer" => arguments.is_empty(),
        "/opt/rust/bin/rustc" => matches!(
            arguments.as_slice(),
            ["-vV"]
                | ["--print", "sysroot"]
                | ["--print", "cfg", "-O"]
                | ["-Z", "unstable-options", "--print", "target-spec-json"]
        ),
        "/opt/rust/bin/cargo" => match arguments.as_slice() {
            // The trailing arguments of `locate-project` are the caller's
            // formatting choices; the subcommand itself reads a manifest path
            // and prints it, which is what §7 admits.
            ["--version"] | ["locate-project", ..] => true,
            ["metadata", rest @ ..] => admitted_metadata(rest),
            ["rustc", "-Z", "unstable-options", "--print", ..] => true,
            _ => false,
        },
        _ => false,
    }
}

/// `cargo metadata` in the one shape ADR-084 §7 admits: the manifest's own
/// metadata, never the dependency graph.
///
/// The check is over the *set* of arguments rather than their sequence,
/// because the order cargo's own CLI happens to use is not a property this
/// calibration is measuring — rust-analyzer 1.98.1 spells it
/// `metadata --format-version 1 --no-deps --manifest-path … --filter-platform …`,
/// while §7 writes it `--no-deps --format-version 1`. What must hold is that
/// `--no-deps` is there (no dependency resolution, no `Cargo.lock` write) and
/// that every other argument is data from the closed set below: nothing here
/// can turn the call into a build.
fn admitted_metadata(arguments: &[&str]) -> bool {
    let mut no_deps = false;
    let mut format_version = false;
    let mut rest = arguments.iter().copied();
    while let Some(argument) = rest.next() {
        match argument {
            "--no-deps" => no_deps = true,
            "--offline" | "--locked" | "--frozen" => (),
            "--format-version" => format_version = rest.next() == Some("1"),
            // A path or a target triple, consumed with its flag so a bare value
            // can never be read as a flag this list does not know.
            "--manifest-path" | "--filter-platform" => {
                if rest.next().is_none() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    no_deps && format_version
}

// -- environment contract ----------------------------------------------------

fn docker() -> PathBuf {
    std::env::var_os("RUST_MCP_TEST_DOCKER").map_or_else(|| PathBuf::from(DOCKER), PathBuf::from)
}

fn socket() -> Result<PathBuf, Failure> {
    let value = std::env::var_os("RUST_MCP_TEST_SOCKET").ok_or(
        "RUST_MCP_TEST_SOCKET is required: this M6 calibration drives Docker directly and takes \
         exclusive ownership of the engine. Set it to the absolute path of the socket (for \
         example ~/.docker/run/docker.sock) and run with --test-threads=1.",
    )?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("RUST_MCP_TEST_SOCKET must be an explicit absolute socket path".into());
    }
    Ok(path)
}

/// The image under calibration. `RUST_MCP_TEST_IMAGE` may name it explicitly,
/// but only the ADR-085 digest: every other image is refused by the analyzer
/// gateway, so pointing this calibration at one would measure nothing. A
/// mismatch fails; it never skips.
fn m6_image() -> Result<String, Failure> {
    let image =
        std::env::var("RUST_MCP_TEST_IMAGE").unwrap_or_else(|_| APPROVED_M6_IMAGE.to_owned());
    if image != APPROVED_M6_IMAGE {
        return Err(format!(
            "RUST_MCP_TEST_IMAGE={image} is not the admitted M6 runtime {APPROVED_M6_IMAGE}"
        )
        .into());
    }
    Ok(image)
}

// -- session -----------------------------------------------------------------

/// A gateway plus its state root, torn down on every exit path — return, error
/// or unwind. The gateway is dropped inside `Drop` before the directory is
/// removed, because its own teardown writes into it.
struct Session {
    gateway: Option<RustGateway>,
    root: PathBuf,
}

impl Session {
    fn open(image_id: &str) -> Result<Self, Failure> {
        let root = PathBuf::from("/private/tmp").join(format!(
            "m6-native-{}",
            state::nonce().map_err(|error| format!("nonce: {error:?}"))?
        ));
        std::fs::create_dir(&root)?;
        let mut session = Self {
            gateway: None,
            root: root.clone(),
        };
        let gateway = RustGateway::new(HostDockerConfig {
            executable: docker(),
            socket: socket()?,
            state_root: root,
            image_id: image_id.to_owned(),
        })
        .map_err(|error| format!("gateway on {image_id}: {error:?}"))?;
        // The calibration does not re-run base calibration: ADR-082's
        // provisioning receipt and the M4/M5 base calibrations carry it, and M6
        // changes no toolchain binary.
        gateway.set_verified(true);
        session.gateway = Some(gateway);
        Ok(session)
    }

    fn gateway(&self) -> Result<&RustGateway, Failure> {
        Ok(self
            .gateway
            .as_ref()
            .ok_or("session gateway already taken")?)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        drop(self.gateway.take());
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// -- residue -----------------------------------------------------------------

/// Every container and volume carrying this product's labels, by name.
fn residue(gateway: &RustGateway) -> Result<Value, Failure> {
    let mut found = serde_json::Map::new();
    for (kind, all, format) in [
        ("containers", true, "--format={{.Names}}"),
        ("volumes", false, "--format={{.Name}}"),
    ] {
        let singular = kind.trim_end_matches('s');
        let mut arguments = vec![singular.to_owned(), "ls".to_owned()];
        if all {
            arguments.push("--all".to_owned());
        }
        arguments.push(LABEL.to_owned());
        arguments.push(format.to_owned());
        let result = gateway
            .inner
            .control(&arguments)
            .map_err(|error| format!("{kind} inventory: {error:?}"))?;
        if result.code != Some(0) {
            return Err(format!("{kind} inventory exited {:?}", result.code).into());
        }
        let names = String::from_utf8_lossy(&result.stdout)
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        found.insert(kind.to_owned(), json!(names));
    }
    Ok(Value::Object(found))
}

/// The residue inventory, refusing anything that survived.
fn clean(gateway: &RustGateway, at: &str) -> Result<Value, Failure> {
    let found = residue(gateway)?;
    if found != json!({"containers": [], "volumes": []}) {
        return Err(format!("{at}: labelled Docker residue survived: {found}").into());
    }
    Ok(found)
}

/// The running analyzer container this product owns, if one exists right now.
fn analyzer_container(gateway: &RustGateway) -> Option<String> {
    let result = gateway
        .inner
        .control(&[
            "container".into(),
            "ls".into(),
            LABEL.into(),
            "--format={{.Names}}".into(),
        ])
        .ok()?;
    if result.code != Some(0) {
        return None;
    }
    String::from_utf8_lossy(&result.stdout)
        .split_whitespace()
        .find(|name| name.starts_with("rust-mcp-analyzer-"))
        .map(str::to_owned)
}

// -- controls ----------------------------------------------------------------

/// The uncancelled token a normal call is given.
struct Proceed;
impl ExecutionCancellation for Proceed {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// How often a token is allowed to spend a Docker round trip. The session polls
/// its cancellation on a two millisecond loop; sampling that often would turn
/// the observation into the dominant cost of the phase it is observing.
///
/// Measured against the real binary, a session on a small fixture reaches
/// quiescent in roughly 0.3 s, and one sample costs two or three `docker`
/// invocations. The interval is therefore set just below the cost of a sample:
/// the sampler runs as continuously as the client allows, which is the most the
/// process table can be watched from outside the guest.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(25);
/// The fewest samples that make an absence mean anything. One sample proves only
/// that nothing forbidden was running at one instant.
const MIN_PROGRAM_SAMPLES: u64 = 3;

/// Samples the guest process table while the call runs, and never cancels.
///
/// Same mechanism as the gateway's own `container_top`: ownership is proved
/// before anything is read, and a container that finished between two calls is
/// simply not sampled.
struct ProgramObserver<'a> {
    gateway: &'a RustGateway,
    last: Mutex<Option<Instant>>,
    samples: AtomicU64,
    tables: Mutex<Vec<String>>,
    argv: Mutex<BTreeSet<String>>,
    programs: Mutex<BTreeSet<String>>,
    errors: AtomicU64,
}

impl<'a> ProgramObserver<'a> {
    fn new(gateway: &'a RustGateway) -> Self {
        Self {
            gateway,
            last: Mutex::new(None),
            samples: AtomicU64::new(0),
            tables: Mutex::new(Vec::new()),
            argv: Mutex::new(BTreeSet::new()),
            programs: Mutex::new(BTreeSet::new()),
            errors: AtomicU64::new(0),
        }
    }

    fn due(&self) -> bool {
        let Ok(mut last) = self.last.lock() else {
            return false;
        };
        let now = Instant::now();
        if last.is_none_or(|last| now.duration_since(last) >= SAMPLE_INTERVAL) {
            *last = Some(now);
            return true;
        }
        false
    }

    fn sample(&self) {
        if !self.due() {
            return;
        }
        let Some(name) = analyzer_container(self.gateway) else {
            return;
        };
        let Some(nonce) = name.strip_prefix("rust-mcp-analyzer-") else {
            return;
        };
        match self.gateway.container_top(&name, nonce) {
            Ok(Some(table)) => {
                self.samples.fetch_add(1, AtomicOrdering::SeqCst);
                for line in table.lines().skip(1) {
                    let fields = line.split_whitespace().collect::<Vec<_>>();
                    if fields.len() < 3 {
                        continue;
                    }
                    let command = fields[2..].join(" ");
                    let program = fields[2].rsplit('/').next().unwrap_or(fields[2]).to_owned();
                    if let Ok(mut argv) = self.argv.lock() {
                        argv.insert(command);
                    }
                    if let Ok(mut programs) = self.programs.lock() {
                        programs.insert(program);
                    }
                }
                // A handful of raw tables is evidence; every sample would be
                // the same table repeated.
                if let Ok(mut tables) = self.tables.lock()
                    && tables.len() < 8
                {
                    tables.push(table);
                }
            }
            Ok(None) => (),
            Err(_) => {
                self.errors.fetch_add(1, AtomicOrdering::SeqCst);
            }
        }
    }

    fn observed(&self) -> (BTreeSet<String>, BTreeSet<String>, u64, u64) {
        (
            self.programs
                .lock()
                .map(|set| set.clone())
                .unwrap_or_default(),
            self.argv.lock().map(|set| set.clone()).unwrap_or_default(),
            self.samples.load(AtomicOrdering::SeqCst),
            self.errors.load(AtomicOrdering::SeqCst),
        )
    }
}

impl ExecutionCancellation for ProgramObserver<'_> {
    fn is_cancelled(&self) -> bool {
        self.sample();
        false
    }
}

/// Cancels once the analyzer container is actually running, so the cancellation
/// lands inside a live session rather than racing its setup.
struct CancelWhenRunning<'a> {
    gateway: &'a RustGateway,
    last: Mutex<Option<Instant>>,
    observed: AtomicBool,
    polls: AtomicU64,
}

impl<'a> CancelWhenRunning<'a> {
    fn new(gateway: &'a RustGateway) -> Self {
        Self {
            gateway,
            last: Mutex::new(None),
            observed: AtomicBool::new(false),
            polls: AtomicU64::new(0),
        }
    }
    fn due(&self) -> bool {
        let Ok(mut last) = self.last.lock() else {
            return false;
        };
        let now = Instant::now();
        if last.is_none_or(|last| now.duration_since(last) >= SAMPLE_INTERVAL) {
            *last = Some(now);
            return true;
        }
        false
    }
    fn evidence(&self) -> Value {
        json!({
            "observed_running_analyzer": self.observed.load(AtomicOrdering::SeqCst),
            "inventory_polls": self.polls.load(AtomicOrdering::SeqCst),
        })
    }
}

impl ExecutionCancellation for CancelWhenRunning<'_> {
    fn is_cancelled(&self) -> bool {
        if self.observed.load(AtomicOrdering::SeqCst) {
            return true;
        }
        if !self.due() {
            return false;
        }
        self.polls.fetch_add(1, AtomicOrdering::SeqCst);
        if analyzer_container(self.gateway).is_some() {
            self.observed.store(true, AtomicOrdering::SeqCst);
            return true;
        }
        false
    }
}

/// Kills the guest analyzer once, from outside, and never cancels the call.
struct KillWhenRunning<'a> {
    gateway: &'a RustGateway,
    last: Mutex<Option<Instant>>,
    killed: AtomicBool,
    after: Instant,
    target: Mutex<Option<String>>,
    /// The container's own `State` after the kill, read here because the
    /// gateway removes the container before the call returns.
    guest_state: Mutex<Option<Value>>,
}

impl<'a> KillWhenRunning<'a> {
    /// `delay` keeps the kill away from the very first milliseconds of the
    /// session, so the conversation has really started before the server dies.
    fn new(gateway: &'a RustGateway, delay: Duration) -> Self {
        Self {
            gateway,
            last: Mutex::new(None),
            killed: AtomicBool::new(false),
            after: Instant::now() + delay,
            target: Mutex::new(None),
            guest_state: Mutex::new(None),
        }
    }
    fn evidence(&self) -> Value {
        json!({
            "killed": self.killed.load(AtomicOrdering::SeqCst),
            "target": self.target.lock().ok().and_then(|name| name.clone()),
            "guest_state": self.guest_state.lock().ok().and_then(|state| state.clone()),
        })
    }

    /// The killed container's own termination, read from the engine while the
    /// container still exists.
    ///
    /// This is the guest's verdict — `State.ExitCode == 137` for SIGKILL — and
    /// not the attached client's status, which is a different process and whose
    /// code is `None` once this side signals it. The read happens here, inside
    /// the call, because the gateway removes the container before returning.
    fn observe_guest(&self, name: &str) -> Option<Value> {
        for _ in 0..40 {
            let inspected = self
                .gateway
                .inner
                .control(&["container".into(), "inspect".into(), name.into()])
                .ok()?;
            if inspected.code != Some(0) {
                return None;
            }
            let containers: Vec<crate::Container> =
                serde_json::from_slice(&inspected.stdout).ok()?;
            let [container] = containers.as_slice() else {
                return None;
            };
            if !container.state.running {
                return Some(json!({
                    "exit_code": container.state.exit_code,
                    "oom_killed": container.state.oom_killed,
                    "status": container.state.status,
                }));
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        None
    }
}

impl ExecutionCancellation for KillWhenRunning<'_> {
    fn is_cancelled(&self) -> bool {
        if self.killed.load(AtomicOrdering::SeqCst) || Instant::now() < self.after {
            return false;
        }
        let Ok(mut last) = self.last.lock() else {
            return false;
        };
        let now = Instant::now();
        if last.is_some_and(|last| now.duration_since(last) < SAMPLE_INTERVAL) {
            return false;
        }
        *last = Some(now);
        drop(last);
        if let Some(name) = analyzer_container(self.gateway) {
            let killed = self.gateway.inner.control(&[
                "container".into(),
                "kill".into(),
                "--signal=KILL".into(),
                name.clone(),
            ]);
            if matches!(killed, Ok(ref result) if result.code == Some(0)) {
                let state = self.observe_guest(&name);
                if let Ok(mut guest) = self.guest_state.lock() {
                    *guest = state;
                }
                if let Ok(mut target) = self.target.lock() {
                    *target = Some(name);
                }
                self.killed.store(true, AtomicOrdering::SeqCst);
            }
        }
        false
    }
}

// -- fixtures ----------------------------------------------------------------

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn source_file(path: &str, bytes: Vec<u8>) -> Result<SourceFile, Failure> {
    Ok(SourceFile::new(path.to_owned(), bytes).map_err(|error| format!("{path}: {error:?}"))?)
}

/// Walks a fixture directory into the owned, bounded shape the product ingests.
/// `target/` and `.git/` are build and VCS state, never fixture input.
fn walk(
    root: &Path,
    at: &Path,
    files: &mut Vec<SourceFile>,
    directories: &mut Vec<String>,
) -> Result<(), Failure> {
    let mut entries = std::fs::read_dir(at)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_str().ok_or("non-UTF-8 fixture entry")?;
        if matches!(name, "target" | ".git") {
            continue;
        }
        let relative = path
            .strip_prefix(root)?
            .to_str()
            .ok_or("non-UTF-8 fixture path")?
            .to_owned();
        let kind = entry.file_type()?;
        if kind.is_dir() {
            directories.push(relative);
            walk(root, &path, files, directories)?;
        } else if kind.is_file() {
            files.push(source_file(&relative, std::fs::read(&path)?)?);
        } else {
            return Err(format!("special fixture entry: {relative}").into());
        }
    }
    Ok(())
}

fn fixture_bundle(name: &str) -> Result<SourceBundle, Failure> {
    let root = fixtures().join(name);
    let mut files = Vec::new();
    let mut directories = Vec::new();
    walk(&root, &root, &mut files, &mut directories)?;
    Ok(SourceBundle::with_directories(files, directories)
        .map_err(|error| format!("fixtures/{name}: {error:?}"))?)
}

fn with_files(
    source: &SourceBundle,
    additions: impl IntoIterator<Item = (String, Vec<u8>)>,
) -> Result<SourceBundle, Failure> {
    let mut files = source.files().to_vec();
    for (path, bytes) in additions {
        files.push(source_file(&path, bytes)?);
    }
    Ok(
        SourceBundle::with_directories(files, source.directories().to_vec())
            .map_err(|error| format!("extended bundle: {error:?}"))?,
    )
}

fn bundle_facts(name: &str, source: &SourceBundle) -> Result<Value, Failure> {
    let archive = source_archive::encode(source).map_err(|error| format!("{name}: {error:?}"))?;
    Ok(json!({
        "files": source.files().len(),
        "directories": source.directories().len(),
        "bytes": source.files().iter().map(|file| file.bytes().len()).sum::<usize>(),
        "archive_sha256": digest(&archive),
    }))
}

fn limits() -> Result<ExecutionLimits, Failure> {
    Ok(ExecutionLimits::new_job(TOTAL_MS, LOG_BYTES)
        .ok_or("the ADR-084 total budget is not representable")?)
}

fn symbols_query(file: &str) -> Result<AnalyzerQuery, Failure> {
    Ok(AnalyzerQuery::DocumentSymbols {
        file: domain::AnalyzerFile::new(file.to_owned())
            .map_err(|error| format!("{file}: {error:?}"))?,
    })
}

// -- receipt -----------------------------------------------------------------

fn receipt_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m6-calibration")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `YYYY-MM-DDTHH:MM:SSZ` from a Unix second, by the civil-from-days algorithm.
/// A clock this process cannot read stamps the epoch rather than a plausible
/// time; the receipt's evidence is its digests, not its stamp.
fn utc(seconds: u64) -> String {
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let rest = seconds % 86_400;
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        (rest % 3_600) / 60,
        rest % 60
    )
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The sources whose bytes this calibration is evidence *about*. A change in any
/// of them invalidates the receipt, which is the point of listing them.
fn calibrated_sources() -> Result<Value, Failure> {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut entries = serde_json::Map::new();
    for relative in [
        "src/analyzer_gateway.rs",
        "src/lsp_session.rs",
        "src/lsp_codec.rs",
        "src/analyzer_native.rs",
        "src/rust_gateway.rs",
        "src/rust_applied.rs",
        "../domain/src/analyzer.rs",
    ] {
        let path = crate_root.join(relative);
        entries.insert(relative.to_owned(), json!(digest(&std::fs::read(&path)?)));
    }
    Ok(Value::Object(entries))
}

/// When this process started, in Unix seconds.
///
/// Every cut document carries it as `run_started_at`, and the gate driver
/// refuses a receipt holding a document from an earlier run: one cut runs per
/// `cargo test` invocation, so a document stamped before the driver started is
/// a leftover being presented as today's evidence.
static PROCESS_STARTED_UNIX: std::sync::LazyLock<u64> = std::sync::LazyLock::new(unix_now);

struct Cut {
    name: &'static str,
    image: String,
    status: &'static str,
    started_unix: u64,
    started: Instant,
    selections: Vec<Value>,
    fixtures: serde_json::Map<String, Value>,
    residue_before: Value,
    residue_after: Value,
    notes: Vec<String>,
    published_verdict: bool,
}

impl Cut {
    fn open(name: &'static str, image: &str) -> Self {
        Self {
            name,
            image: image.to_owned(),
            status: "fail",
            started_unix: unix_now(),
            started: Instant::now(),
            selections: Vec::new(),
            fixtures: serde_json::Map::new(),
            residue_before: Value::Null,
            residue_after: Value::Null,
            notes: Vec::new(),
            published_verdict: false,
        }
    }
    fn record(&mut self, selection: &str, started: Instant, observed: Value) {
        self.selections.push(json!({
            "selection": selection,
            "duration_ms": elapsed_ms(started),
            "observed": observed,
        }));
    }
    /// Publishes this cut as passed. Anything after this point that fails still
    /// fails the selection, and [`Drop`] republishes the document as `fail`.
    fn pass(&mut self) -> Result<PathBuf, Failure> {
        self.status = "pass";
        self.published_verdict = true;
        publish(self)
    }
    /// Publishes this cut as failed, with `condition` recorded as a note.
    fn fail(&mut self, condition: &str) -> Result<PathBuf, Failure> {
        self.status = "fail";
        self.notes.push(condition.to_owned());
        self.published_verdict = true;
        publish(self)
    }
    fn document(&self) -> Value {
        json!({
            "cut": self.name,
            "status": self.status,
            "image_id": self.image,
            "run_started_at": utc(*PROCESS_STARTED_UNIX),
            "started_utc": utc(self.started_unix),
            "finished_utc": utc(unix_now()),
            "duration_ms": elapsed_ms(self.started),
            "residue": {"before": self.residue_before, "after": self.residue_after},
            "fixtures": Value::Object(self.fixtures.clone()),
            "selections": self.selections,
            "notes": self.notes,
        })
    }
}

/// A cut that ends without publishing a verdict — a failed assertion, an early
/// `?`, a panic — publishes `fail` here.
///
/// Before this guard existed, a cut that blew up left *no* document, so the
/// merged receipt simply did not mention it (or, worse, still carried a `pass`
/// from an earlier run). A document that says `fail` is the honest record of
/// what this run observed, and the gate reconciles it against the selections it
/// ran.
impl Drop for Cut {
    fn drop(&mut self) {
        if self.published_verdict {
            return;
        }
        self.status = "fail";
        self.notes.push(
            "the cut ended without reaching its own verdict; the failure is in the selection's \
             log. This document is published by the cut's drop guard so the receipt cannot omit \
             a cut that ran."
                .to_owned(),
        );
        match publish(self) {
            Ok(path) => println!("M6_CALIBRATION_CUT_FAILED {} {}", self.name, path.display()),
            Err(error) => println!("M6_CALIBRATION_CUT_UNPUBLISHED {} {error}", self.name),
        }
    }
}

/// Writes this cut's own document, then rebuilds the merged receipt from every
/// cut document present. Cuts run one at a time, so the last to finish leaves a
/// complete `receipt.json`; each one on its own leaves a truthful partial
/// receipt naming exactly the cuts that have run.
///
/// The merge is only as honest as the directory it reads. Two things keep it
/// so: the gate driver empties `target/m6-calibration` before the suite, and
/// every document carries the `run_started_at` of the process that wrote it, so
/// a document surviving from an earlier run is detectable rather than silently
/// merged with today's source digests.
fn publish(cut: &Cut) -> Result<PathBuf, Failure> {
    let image_id = cut.image.as_str();
    let root = receipt_root();
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join(format!("cut-{}.json", cut.name)),
        serde_json::to_vec_pretty(&cut.document())?,
    )?;

    let mut documents = BTreeMap::new();
    for entry in std::fs::read_dir(&root)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("cut-") || !name.ends_with(".json") {
            continue;
        }
        let document: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        documents.insert(name.to_owned(), document);
    }

    let mut statuses = serde_json::Map::new();
    let mut selections = Vec::new();
    let mut fixtures = serde_json::Map::new();
    let mut started = None::<String>;
    let mut finished = None::<String>;
    for document in documents.values() {
        if let (Some(Value::String(name)), Some(status)) =
            (document.get("cut"), document.get("status"))
        {
            statuses.insert(name.clone(), status.clone());
        }
        if let Some(Value::Array(entries)) = document.get("selections") {
            selections.extend(entries.iter().cloned());
        }
        if let Some(Value::Object(entries)) = document.get("fixtures") {
            for (key, value) in entries {
                fixtures.insert(key.clone(), value.clone());
            }
        }
        if let Some(Value::String(value)) = document.get("started_utc")
            && started.as_ref().is_none_or(|current| value < current)
        {
            started = Some(value.clone());
        }
        if let Some(Value::String(value)) = document.get("finished_utc")
            && finished.as_ref().is_none_or(|current| value > current)
        {
            finished = Some(value.clone());
        }
    }

    let receipt = json!({
        "schema": RECEIPT_SCHEMA,
        "started_utc": started,
        "finished_utc": finished,
        "image_id": image_id,
        "image_tag_at_provisioning": IMAGE_TAG_AT_PROVISIONING,
        "analyzer_version": ANALYZER_VERSION,
        "analyzer_binary_sha256": ANALYZER_BINARY_SHA256,
        "config_digest": analyzer_gateway::analyzer_config_digest()
            .map_err(|error| format!("config digest: {error:?}"))?
            .to_string(),
        "sources": calibrated_sources()?,
        "cut_status": Value::Object(statuses),
        "cuts": documents.values().cloned().collect::<Vec<_>>(),
        "selections": selections,
    });
    let path = root.join("receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    std::fs::write(&path, &bytes)?;
    println!(
        "M6_CALIBRATION_RECEIPT {} {}",
        digest(&bytes),
        path.display()
    );
    Ok(path)
}

// -- shared assertions -------------------------------------------------------

fn session_facts(execution: &domain::AnalyzerExecution) -> Value {
    json!({
        "failure": execution.failure().map(|failure| format!("{failure:?}")),
        "position_encoding": serde_json::to_value(execution.position_encoding)
            .unwrap_or(Value::Null),
        "readiness": serde_json::to_value(execution.readiness).unwrap_or(Value::Null),
        "completeness": serde_json::to_value(&execution.completeness).unwrap_or(Value::Null),
        "termination": format!("{:?}", execution.termination),
        "oom_killed": execution.oom_killed,
        "session": serde_json::to_value(&execution.session).unwrap_or(Value::Null),
        // The session's own duration and the call's are separate measurements:
        // the call also pays for the capture checks, the volume, the ingest and
        // the cleanup, and a receipt that reported one as the other would
        // describe work the session never did.
        "call": {"duration_ms": execution.call_duration_ms},
    })
}

/// The symbols of an answered document-symbol call, or an error naming what the
/// call actually produced.
fn document_symbols(
    execution: &domain::AnalyzerExecution,
) -> Result<&[domain::DocumentSymbol], Failure> {
    match execution.result() {
        Some(domain::AnalyzerResult::DocumentSymbols(symbols)) => Ok(symbols),
        other => Err(format!(
            "expected document symbols; got {:?} with failure {:?}",
            other.map(std::mem::discriminant),
            execution.failure()
        )
        .into()),
    }
}

// -- cuts --------------------------------------------------------------------

#[test]
#[ignore = "explicit M5 image and host Docker; M6 admission refusal"]
fn m6_analyzer_refuses_every_runtime_but_the_m6_image() -> Result<(), Failure> {
    // Named first so a mis-set environment fails here instead of silently
    // calibrating nothing.
    let image = m6_image()?;
    let mut cut = Cut::open("m6-00-admission", &image);
    let session = Session::open(crate::APPROVED_M5_IMAGE)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "admission before")?;
    assert_ne!(gateway.image_id(), APPROVED_M6_IMAGE);

    let source = fixture_bundle("valid-basic")?;
    cut.fixtures
        .insert("valid-basic".into(), bundle_facts("valid-basic", &source)?);
    let started = Instant::now();
    let refused =
        gateway.execute_analyzer(&source, &symbols_query("src/lib.rs")?, limits()?, &Proceed);
    let observed = json!({
        "image_id": gateway.image_id(),
        "admitted_image_id": APPROVED_M6_IMAGE,
        "error": refused.as_ref().err().map(|error| format!("{error:?}")),
    });
    assert!(
        matches!(refused, Err(ExecutionError::Unavailable)),
        "an unadmitted runtime must be refused: {observed}"
    );
    cut.record("unadmitted-image-refused", started, observed);
    cut.residue_after = clean(gateway, "admission after")?;
    cut.pass()?;
    Ok(())
}

#[test]
#[ignore = "explicit M6 image and host Docker; reads the guest's own identity and config schema"]
fn m6_analyzer_version_and_config_schema_match_the_receipt() -> Result<(), Failure> {
    let image = m6_image()?;
    let mut cut = Cut::open("m6-01-identity", &image);
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "identity before")?;

    // 1. The version line, read out of the live guest.
    let started = Instant::now();
    let (bytes, code) = analyzer_gateway::probe(
        gateway,
        &Phase::AnalyzerDocument(AnalyzerDocument::Version),
        limits()?,
        &Proceed,
    )
    .map_err(|error| format!("version probe: {error:?}"))?;
    let guest_version = String::from_utf8(bytes)?.trim().to_owned();
    assert_eq!(code, Some(0));
    assert_eq!(
        guest_version, ANALYZER_VERSION,
        "the pinned version line must be the guest's own"
    );
    cut.record(
        "guest-version-line",
        started,
        json!({"guest": guest_version, "pinned": ANALYZER_VERSION}),
    );

    // 2. The binary digest, from the guest's installed inventory.
    let started = Instant::now();
    let (bytes, code) = analyzer_gateway::probe(
        gateway,
        &Phase::AnalyzerDocument(AnalyzerDocument::Installed),
        limits()?,
        &Proceed,
    )
    .map_err(|error| format!("installed probe: {error:?}"))?;
    assert_eq!(code, Some(0));
    let installed: Value = serde_json::from_slice(&bytes)?;
    let guest_binary = installed["components"]
        .as_array()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component["name"] == json!("rust-analyzer"))
        })
        .and_then(|component| component["sha256"].as_str())
        .ok_or("the guest inventory has no rust-analyzer sha256")?
        .to_owned();
    assert_eq!(
        format!("sha256:{guest_binary}"),
        ANALYZER_BINARY_SHA256,
        "the pinned binary digest must be the guest's own"
    );

    // 3. Both must equal the provisioning receipt that built the image.
    let provisioning: Value = serde_json::from_slice(&std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/validation/M6/provisioning.json"),
    )?)?;
    assert_eq!(
        provisioning["rust_analyzer_version"],
        json!(ANALYZER_VERSION)
    );
    assert_eq!(provisioning["image_id"], json!(APPROVED_M6_IMAGE));
    let receipt_binary = provisioning["installed"]["components"]
        .as_array()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component["name"] == json!("rust-analyzer"))
        })
        .and_then(|component| component["sha256"].as_str())
        .ok_or("the provisioning receipt has no rust-analyzer sha256")?;
    assert_eq!(receipt_binary, guest_binary);
    cut.record(
        "guest-binary-digest",
        started,
        json!({
            "guest": format!("sha256:{guest_binary}"),
            "pinned": ANALYZER_BINARY_SHA256,
            "provisioning_receipt": format!("sha256:{receipt_binary}"),
        }),
    );

    // 4. Every fixed configuration key must exist in the binary's own schema.
    //    rust-analyzer ignores unknown keys in silence, so this is the only
    //    thing standing between a typo and a default quietly staying in force.
    let started = Instant::now();
    let (bytes, code) =
        analyzer_gateway::probe(gateway, &Phase::AnalyzerConfigSchema, limits()?, &Proceed)
            .map_err(|error| format!("config schema probe: {error:?}"))?;
    assert_eq!(code, Some(0), "the binary must print its own schema");
    let schema_sha256 = digest(&bytes);
    let archive = receipt_root();
    std::fs::create_dir_all(&archive)?;
    std::fs::write(archive.join("config-schema.json"), &bytes)?;
    let schema: Value = serde_json::from_slice(&bytes)?;
    let mut schema_keys = BTreeSet::new();
    collect_keys(&schema, &mut schema_keys);
    assert!(
        !schema_keys.is_empty(),
        "the printed schema has no object keys to check against"
    );
    let mut expected = Vec::new();
    flatten_keys(
        &crate::lsp_codec::initialization_options(),
        String::new(),
        &mut expected,
    );
    let missing = expected
        .iter()
        .filter(|key| {
            !schema_keys
                .iter()
                .any(|known| known == *key || known.ends_with(&format!(".{key}")))
        })
        .cloned()
        .collect::<Vec<_>>();
    cut.record(
        "fixed-configuration-keys-exist",
        started,
        json!({
            "config_schema_sha256": schema_sha256,
            "config_schema_bytes": bytes.len(),
            "schema_keys": schema_keys.len(),
            "checked_keys": expected,
            "missing_keys": missing,
        }),
    );
    // This verdict is published either way. A key the real binary does not know
    // is not a test bug to route around: rust-analyzer ignores it in silence, so
    // the configured value never takes effect and the binary's default stays in
    // force. Recording the condition is the whole point of the cut, so the cut
    // document is written before the failure is raised.
    let condition = (!missing.is_empty()).then(|| {
        format!(
            "these fixed initializationOptions keys do not exist in the schema printed by \
             {ANALYZER_VERSION} (config-schema.json sha256 {schema_sha256}, {} keys): {missing:?}. \
             Each one leaves the binary's default in force with no error. Reproduce with: \
             cargo test -p rust-engineering-execution --lib \
             analyzer_native::m6_analyzer_version_and_config_schema_match_the_receipt -- \
             --exact --ignored --nocapture --test-threads=1",
            schema_keys.len()
        )
    });
    if let Some(condition) = condition {
        cut.residue_after = clean(gateway, "identity after")?;
        cut.fail(&condition)?;
        return Err(condition.into());
    }

    cut.residue_after = clean(gateway, "identity after")?;
    cut.pass()?;
    Ok(())
}

/// Every key of a nested configuration object, in dotted form.
fn flatten_keys(value: &Value, prefix: String, out: &mut Vec<String>) {
    match value {
        Value::Object(entries) => {
            for (key, nested) in entries {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_keys(nested, path, out);
            }
        }
        // A non-object value is a leaf: that dotted path is the configuration
        // key the server must recognise.
        _ => out.push(prefix),
    }
}

/// Every string used as an object key anywhere in a JSON document.
fn collect_keys(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Object(entries) => {
            for (key, nested) in entries {
                out.insert(key.clone());
                collect_keys(nested, out);
            }
        }
        Value::Array(entries) => {
            for nested in entries {
                collect_keys(nested, out);
            }
        }
        _ => (),
    }
}

#[test]
#[ignore = "explicit M6 image, host Docker and one real rust-analyzer session"]
fn m6_document_symbols_on_valid_basic_negotiate_utf8_and_reach_quiescent() -> Result<(), Failure> {
    let image = m6_image()?;
    let mut cut = Cut::open("m6-02-document-symbols", &image);
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "symbols before")?;

    let source = fixture_bundle("valid-basic")?;
    cut.fixtures
        .insert("valid-basic".into(), bundle_facts("valid-basic", &source)?);
    let started = Instant::now();
    let execution = gateway
        .execute_analyzer(&source, &symbols_query("src/lib.rs")?, limits()?, &Proceed)
        .map_err(|error| format!("analyzer session: {error:?}"))?;
    let facts = session_facts(&execution);

    assert_eq!(
        execution.position_encoding,
        Some(domain::PositionEncoding::Utf8),
        "ADR-084 §5 requires utf-8 to be the negotiated encoding: {facts}"
    );
    let domain::AnalyzerReadiness::Quiescent { elapsed_ms, health } = execution.readiness else {
        return Err(format!("the server never reached the readiness oracle: {facts}").into());
    };
    // The fixed configuration must leave the server *healthy* on a valid
    // project. Until 2026-09-12 it did not: `cargo.autoreload=false` made every
    // session quiescent with `health: warning`, which degraded every answer to
    // `incomplete` and made `complete` unreachable. The key is gone (ADR-084 §3,
    // amended); this is the oracle that says so, and a warning here is a new
    // finding rather than something to map away.
    assert_eq!(
        health,
        domain::ServerHealth::Ok,
        "a valid project must reach quiescent healthy; a warning degrades every answer to \
         incomplete and its message is never published, so the cause would be invisible: {facts}"
    );
    assert_eq!(
        execution.completeness.state(),
        domain::CompletenessState::Complete,
        "an answered, unwarned, unlimited call is exhaustive: {facts}"
    );
    let symbols = document_symbols(&execution)?;
    assert!(!symbols.is_empty(), "valid-basic has symbols: {facts}");

    // The positions are 1-based Unicode scalars, so each selection range must
    // cut exactly the symbol's own name out of the captured bytes. That is the
    // oracle: an off-by-one or a byte/scalar confusion cannot survive it.
    let bytes = source
        .files()
        .iter()
        .find(|file| file.path() == "src/lib.rs")
        .map(|file| file.bytes().to_vec())
        .ok_or("the fixture has no src/lib.rs")?;
    let index = domain::LineIndex::new(&bytes).map_err(|error| format!("{error:?}"))?;
    let text = String::from_utf8(bytes)?;
    let mut named = Vec::new();
    for symbol in symbols {
        let start = index
            .byte_offset_from_position(symbol.selection_range().start())
            .map_err(|error| format!("{:?}: {error:?}", symbol.name()))?;
        let end = index
            .byte_offset_from_position(symbol.selection_range().end())
            .map_err(|error| format!("{:?}: {error:?}", symbol.name()))?;
        let sliced = text
            .get(start..end)
            .ok_or("selection range outside the file")?;
        assert_eq!(
            sliced,
            symbol.name().as_str(),
            "the selection range must cut the symbol's own name out of the captured bytes"
        );
        named.push(json!({
            "name": symbol.name().as_str(),
            "kind": format!("{:?}", symbol.kind()),
            "depth": symbol.depth(),
            "selection_start": serde_json::to_value(symbol.selection_range().start())
                .unwrap_or(Value::Null),
        }));
    }
    assert_eq!(
        execution.session.stop,
        domain::SessionStop::Exited,
        "the shutdown handshake must complete: {facts}"
    );
    assert_eq!(execution.session.exit_code, Some(0));
    assert_eq!(execution.session.server_requests_refused, 0);
    assert_eq!(execution.session.fault, None);
    cut.record(
        "document-symbols-valid-basic",
        started,
        json!({
            "quiescent_after_ms": elapsed_ms,
            "health": format!("{health:?}"),
            "symbols": named,
            "facts": facts,
        }),
    );
    cut.residue_after = clean(gateway, "symbols after")?;
    cut.pass()?;
    Ok(())
}

#[test]
#[ignore = "explicit M6 image, host Docker; samples the guest process table during initialize"]
fn m6_initialize_spawns_only_the_expected_guest_programs() -> Result<(), Failure> {
    let image = m6_image()?;
    let mut cut = Cut::open("m6-03-guest-programs", &image);
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "programs before")?;

    // A fixture with a real `build.rs`, so "no build script ran" is a statement
    // about something that could have run.
    let source = fixture_bundle("build-script")?;
    cut.fixtures.insert(
        "build-script".into(),
        bundle_facts("build-script", &source)?,
    );
    let observer = ProgramObserver::new(gateway);
    let started = Instant::now();
    let execution = gateway
        .execute_analyzer(&source, &symbols_query("src/lib.rs")?, limits()?, &observer)
        .map_err(|error| format!("analyzer session: {error:?}"))?;
    let (programs, argv, samples, errors) = observer.observed();
    let observed = json!({
        "samples": samples,
        "sample_interval_ms": SAMPLE_INTERVAL.as_millis(),
        "sample_errors": errors,
        "programs": programs.iter().collect::<Vec<_>>(),
        "argv": argv.iter().collect::<Vec<_>>(),
        "facts": session_facts(&execution),
    });
    // What this cut establishes, and what it does not. `docker container top` is
    // sampled from outside the guest, one invocation at a time, so it observes
    // instants and not an interval: a child that lives for tens of milliseconds
    // can pass between two samples. Every distinct command line observed is
    // recorded above, so the strength of the absence is visible rather than
    // implied. The deterministic in-band oracle for the same property — with
    // build scripts disabled, `textDocument/diagnostic` on this fixture cannot
    // resolve its `include!(concat!(env!("OUT_DIR"), …))` — needs a diagnostics
    // cut, which does not exist yet: it is assigned to W07 (M6-03), the package
    // that owns that query.
    cut.notes.push(format!(
        "Process absence is evidence over {samples} sampled instants of a session that reached \
         quiescent, not over the whole interval: `docker container top` is sampled from the host \
         at best every {} ms while a guest child may live for tens of milliseconds. The \
         deterministic in-band oracle (build-script output unresolvable in \
         `textDocument/diagnostic`) is not part of this cut and is assigned to W07 (M6-03), which \
         owns that query.",
        SAMPLE_INTERVAL.as_millis()
    ));
    assert!(
        samples >= MIN_PROGRAM_SAMPLES && !programs.is_empty(),
        "the process table was observed {samples} times, below the {MIN_PROGRAM_SAMPLES} this cut \
         needs for an absence to mean anything: {observed}"
    );
    // The whole command line, against the closed list. A basename check would
    // pass a running proc-macro server, which is spawned as
    // `<the rust-analyzer binary> proc-macro`.
    let unexpected = argv
        .iter()
        .filter(|command| !admitted_argv(command))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "ADR-084 §7 admits only the closed argv list inside the guest; {unexpected:?} is outside \
         it and any such program is a P1 finding: {observed}"
    );
    cut.record("initialize-guest-programs", started, observed);
    cut.residue_after = clean(gateway, "programs after")?;
    cut.pass()?;
    Ok(())
}

#[test]
#[ignore = "explicit M6 image and host Docker; a hostile workspace configuration"]
fn m6_hostile_rust_analyzer_toml_is_rejected_before_any_container() -> Result<(), Failure> {
    let image = m6_image()?;
    let mut cut = Cut::open("m6-04-hostile-ratoml", &image);
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "ratoml before")?;

    let hostile = b"[cargo.buildScripts]\nenable = true\n\
                    [check]\noverrideCommand = [\"sh\", \"-c\", \"id\"]\n"
        .to_vec();
    let source = with_files(
        &fixture_bundle("valid-basic")?,
        [("rust-analyzer.toml".to_owned(), hostile)],
    )?;
    cut.fixtures
        .insert("hostile-ratoml".into(), bundle_facts("hostile", &source)?);
    cut.notes.push(
        "The hardened capture refuses the same names before a bundle can exist; that half is \
         covered by crates/project-adapter/tests/source.rs, which this crate cannot reach \
         without a new dependency. This cut is the gateway's own refusal of a bundle it was \
         handed."
            .to_owned(),
    );
    let started = Instant::now();
    let execution = gateway
        .execute_analyzer(&source, &symbols_query("src/lib.rs")?, limits()?, &Proceed)
        .map_err(|error| format!("analyzer session: {error:?}"))?;
    let facts = session_facts(&execution);
    assert_eq!(
        execution.failure(),
        Some(domain::AnalyzerFailure::UnsupportedProjectConfig),
        "a bundle carrying a rust-analyzer.toml must be refused: {facts}"
    );
    assert_eq!(
        execution.session.stop,
        domain::SessionStop::NotStarted,
        "no session may be opened for it: {facts}"
    );
    assert!(execution.result().is_none());
    // What the inventory proves is that **nothing survived**: it is taken after
    // the call, so it cannot distinguish "never created" from "created and
    // cleaned up". That the refusal happens before any object exists is a
    // property of the gateway's own order — `refusal()` returns above the volume
    // block — and `SessionStop::NotStarted` above is the observable half of it.
    cut.residue_after = clean(gateway, "ratoml after")?;
    cut.record("hostile-ratoml-refused", started, facts);
    cut.pass()?;
    Ok(())
}

/// The initialize budget this cut uses to reach the never-ready path.
///
/// The brief asked for a one second budget against a project too large to be
/// quiescent inside it. No fixture in this repository is: measured against the
/// real binary under the fixed configuration of ADR-084 §3 (as amended
/// 2026-09-12), every fixture reaches quiescent between 320 ms and 580 ms —
/// the upper end is `build-script` while the process table is being sampled,
/// which inflates it — with the `initialize` *response* arriving inside 60 ms
/// in all of them. The budget is therefore tightened instead of the project
/// being inflated: the path under test is "the readiness oracle did not arrive
/// inside the budget", and 150 ms is below every measured quiescent time while
/// leaving the response a margin of more than two.
const NEVER_READY_INITIALIZE: Duration = Duration::from_millis(150);

#[test]
#[ignore = "explicit M6 image and host Docker; an initialize budget below the measured readiness"]
fn m6_never_ready_times_out_with_joined_cleanup() -> Result<(), Failure> {
    let image = m6_image()?;
    let mut cut = Cut::open("m6-05-never-ready", &image);
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "never-ready before")?;

    let source = fixture_bundle("workspace")?;
    cut.fixtures
        .insert("workspace".into(), bundle_facts("workspace", &source)?);
    cut.notes.push(format!(
        "Initialize budget {} ms, below the 320-580 ms this repository's fixtures measurably need \
         to reach quiescent against {ANALYZER_VERSION} under the fixed configuration; no \
         available fixture stays non-quiescent for a full second.",
        NEVER_READY_INITIALIZE.as_millis()
    ));
    let budgets = AnalyzerBudgets::with_initialize(limits()?, NEVER_READY_INITIALIZE)
        .ok_or("the never-ready initialize budget must be representable")?;
    let started = Instant::now();
    let execution = analyzer_gateway::execute_bounded(
        gateway,
        &source,
        &symbols_query("core/src/lib.rs")?,
        budgets,
        &Proceed,
    )
    .map_err(|error| format!("analyzer session: {error:?}"))?;
    let facts = session_facts(&execution);
    assert_eq!(
        execution.failure(),
        Some(domain::AnalyzerFailure::NotReady),
        "an unreachable readiness oracle is ANALYZER_NOT_READY, never partial data: {facts}"
    );
    assert!(execution.result().is_none(), "no data without readiness");
    assert!(matches!(
        execution.readiness,
        domain::AnalyzerReadiness::NotReady { .. }
    ));
    assert_eq!(
        execution.completeness.state(),
        domain::CompletenessState::Incomplete
    );
    assert!(
        execution
            .completeness
            .reasons()
            .contains(&domain::IncompleteReason::AnalyzerNotReady)
    );
    cut.residue_after = clean(gateway, "never-ready after")?;
    assert!(
        !gateway.is_quarantined(),
        "a clean timeout is not a quarantine"
    );
    // The single-flight lock really was released: another call can start.
    let (bytes, _) = analyzer_gateway::probe(
        gateway,
        &Phase::AnalyzerDocument(AnalyzerDocument::Version),
        limits()?,
        &Proceed,
    )
    .map_err(|error| format!("the gateway stayed busy after a timeout: {error:?}"))?;
    assert!(!bytes.is_empty());
    cut.record("never-ready-one-second", started, facts);
    cut.residue_after = clean(gateway, "never-ready after probe")?;
    cut.pass()?;
    Ok(())
}

#[test]
#[ignore = "explicit M6 image and host Docker; cancellation inside a live session"]
fn m6_cancellation_during_initialize_kills_and_joins() -> Result<(), Failure> {
    let image = m6_image()?;
    let mut cut = Cut::open("m6-06-cancellation", &image);
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "cancel before")?;

    let source = fixture_bundle("valid-basic")?;
    cut.fixtures
        .insert("valid-basic".into(), bundle_facts("valid-basic", &source)?);
    let cancel = CancelWhenRunning::new(gateway);
    let started = Instant::now();
    let execution = gateway
        .execute_analyzer(&source, &symbols_query("src/lib.rs")?, limits()?, &cancel)
        .map_err(|error| format!("analyzer session: {error:?}"))?;
    let facts = session_facts(&execution);
    assert_eq!(
        execution.failure(),
        Some(domain::AnalyzerFailure::Cancelled),
        "a cancelled session is cancelled, never a timeout: {facts}"
    );
    assert_eq!(execution.termination, ExecutionTermination::Cancelled);
    assert!(execution.result().is_none());
    cut.residue_after = clean(gateway, "cancel after")?;
    assert!(
        !gateway.is_quarantined(),
        "a cancellation with verified cleanup is not a quarantine"
    );

    // The gateway is still usable: the next call answers normally.
    let again = Instant::now();
    let second = gateway
        .execute_analyzer(&source, &symbols_query("src/lib.rs")?, limits()?, &Proceed)
        .map_err(|error| format!("the gateway did not survive a cancellation: {error:?}"))?;
    let second_facts = session_facts(&second);
    assert!(
        !document_symbols(&second)?.is_empty(),
        "the second call must answer: {second_facts}"
    );
    cut.record(
        "cancelled-session",
        started,
        json!({"cancel": cancel.evidence(), "facts": facts}),
    );
    cut.record("second-call-after-cancellation", again, second_facts);
    cut.residue_after = clean(gateway, "cancel after second call")?;
    cut.pass()?;
    Ok(())
}

#[test]
#[ignore = "explicit M6 image and host Docker; kills the guest analyzer mid-session"]
fn m6_analyzer_crash_mid_session_is_reported_not_masked() -> Result<(), Failure> {
    let image = m6_image()?;
    let mut cut = Cut::open("m6-07-crash", &image);
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "crash before")?;

    let source = fixture_bundle("valid-basic")?;
    cut.fixtures
        .insert("valid-basic".into(), bundle_facts("valid-basic", &source)?);
    // Killed at the first instant the guest analyzer is observed running, which
    // is inside the live conversation: measured against the real binary the whole
    // session lasts under a second and reaches quiescent at ~270 ms, so a delayed
    // kill either misses the session entirely or races the answer it is supposed
    // to prevent. Killing on first sight is the only timing that reliably lands
    // while the server is still owed something.
    let killer = KillWhenRunning::new(gateway, Duration::ZERO);
    cut.notes.push(
        "The kill lands during indexing rather than after the readiness oracle: the measured \
         post-quiescent window of a small fixture is a few hundred milliseconds, and a later kill \
         would race the answer. The property under test — a dead server is reported, never masked \
         as an empty result — is the same either way."
            .to_owned(),
    );
    let started = Instant::now();
    let execution = gateway
        .execute_analyzer(&source, &symbols_query("src/lib.rs")?, limits()?, &killer)
        .map_err(|error| format!("analyzer session: {error:?}"))?;
    let facts = session_facts(&execution);
    let evidence = killer.evidence();
    assert_eq!(
        evidence["killed"],
        json!(true),
        "the guest analyzer was never killed, so this cut asserts nothing: {facts}"
    );
    assert_eq!(
        execution.failure(),
        Some(domain::AnalyzerFailure::Crashed),
        "a dead server is ANALYZER_CRASHED, never an empty answer: {facts}"
    );
    assert!(execution.result().is_none());
    // The crash evidence is the *container's* own termination, read from the
    // engine before cleanup removed it: SIGKILL is 137. The attached client's
    // exit code is not evidence of anything here — this side signals that
    // client on the way out, and a signalled process has no code of its own.
    assert_eq!(
        evidence["guest_state"]["exit_code"],
        json!(137),
        "the killed guest must report SIGKILL's 137 before cleanup: {evidence}"
    );
    // Which half of the dead pipe this side saw first is a read/write race, and
    // both readings are the same fact. What must never happen is a stop that
    // claims a handshake or an orderly exit.
    assert!(
        matches!(
            execution.session.stop,
            domain::SessionStop::Eof | domain::SessionStop::Killed
        ),
        "a killed server ends the session by EOF or by this side's kill: {facts}"
    );
    cut.record(
        "killed-analyzer",
        started,
        json!({"kill": evidence, "facts": facts}),
    );
    cut.residue_after = clean(gateway, "crash after")?;
    cut.pass()?;
    Ok(())
}

#[test]
#[ignore = "explicit M6 image and host Docker; a real answer above the frame bound"]
fn m6_frame_limit_from_a_real_peer_kills_the_session() -> Result<(), Failure> {
    let image = m6_image()?;
    let mut cut = Cut::open("m6-08-frame-limit", &image);
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "frame before")?;

    // One `documentSymbol` entry costs far more JSON than the source line that
    // declares it, so a file of many tiny items produces an answer above the
    // 1 MiB frame bound while staying inside the capture's own file limit.
    let count = 12_000usize;
    let mut generated = String::new();
    for index in 0..count {
        generated.push_str(&format!("pub fn symbol_{index:06}() {{}}\n"));
    }
    let source = SourceBundle::with_directories(
        vec![
            source_file(
                "Cargo.toml",
                b"[package]\nname = \"fixture-frame-limit\"\nversion = \"0.1.0\"\n\
                  edition = \"2024\"\nrust-version = \"1.98.1\"\npublish = false\n\n[workspace]\n"
                    .to_vec(),
            )?,
            source_file("src/lib.rs", generated.into_bytes())?,
        ],
        vec!["src".to_owned()],
    )
    .map_err(|error| format!("generated bundle: {error:?}"))?;
    cut.fixtures
        .insert("generated".into(), bundle_facts("generated", &source)?);

    let started = Instant::now();
    let execution = gateway
        .execute_analyzer(&source, &symbols_query("src/lib.rs")?, limits()?, &Proceed)
        .map_err(|error| format!("analyzer session: {error:?}"))?;
    let facts = session_facts(&execution);
    assert_eq!(
        execution.failure(),
        Some(domain::AnalyzerFailure::FrameTooLarge),
        "an answer above the frame bound must kill the session, not be truncated into data, and \
         the failure must name the bound rather than framing in general. If the real answer for \
         {count} symbols is under 1 MiB this cut is not achievable with the real binary and must \
         be recorded as not_run, never as a pass: {facts}"
    );
    assert!(execution.result().is_none());
    assert_eq!(execution.session.stop, domain::SessionStop::Killed);
    // The number the peer declared is the measurement: without it the cut only
    // says "some header was refused".
    let declared = execution
        .session
        .declared_frame_bytes
        .ok_or("the refused Content-Length was not recorded")?;
    assert!(
        declared > domain::MAX_FRAME_BYTES as u64,
        "the refusal must be the bound being exceeded, not a malformed header: {declared} bytes \
         against a bound of {}",
        domain::MAX_FRAME_BYTES
    );
    cut.record(
        "oversized-real-answer",
        started,
        json!({
            "declared_content_length": declared,
            "max_frame_bytes": domain::MAX_FRAME_BYTES,
            "facts": facts,
        }),
    );
    cut.residue_after = clean(gateway, "frame after")?;
    cut.pass()?;
    Ok(())
}

#[cfg(test)]
mod unit {
    use super::*;

    #[test]
    fn the_receipt_stamps_a_real_utc_instant() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc(1_767_225_599), "2025-12-31T23:59:59Z");
        assert_eq!(utc(1_767_225_600), "2026-01-01T00:00:00Z");
        // A leap day, so the civil conversion is not merely 365-day arithmetic.
        assert_eq!(utc(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn configuration_keys_flatten_to_the_dotted_paths_the_server_knows() {
        let mut keys = Vec::new();
        flatten_keys(
            &crate::lsp_codec::initialization_options(),
            String::new(),
            &mut keys,
        );
        for expected in [
            "cargo.buildScripts.enable",
            "cargo.noDeps",
            "cargo.sysroot",
            "cargo.targetDir",
            "procMacro.enable",
            "checkOnSave",
            "files.watcher",
            "cachePriming.enable",
            "numThreads",
            "lru.capacity",
            "linkedProjects",
            "diagnostics.experimental.enable",
            "workspace.symbol.search.scope",
            "workspace.symbol.search.kind",
            "workspace.symbol.search.limit",
            "references.excludeImports",
            "references.excludeTests",
        ] {
            assert!(keys.iter().any(|key| key == expected), "{expected} missing");
        }
        assert_eq!(keys.len(), 17, "every fixed key is checked, and only those");
        // Removed by the 2026-09-12 amendment to ADR-084 §3, each on this
        // calibration's own evidence: one key the real binary's schema does not
        // have, one whose value kept the server in `health: warning`.
        for gone in ["cargo.sysrootQueryMetadata", "cargo.autoreload"] {
            assert!(!keys.iter().any(|key| key == gone), "{gone} came back");
        }
    }

    #[test]
    fn the_guest_argv_allowlist_admits_the_expected_forms_and_nothing_else() {
        for admitted in [
            "/opt/analyzer/bin/rust-analyzer",
            "/opt/rust/bin/rustc -vV",
            "/opt/rust/bin/rustc --print sysroot",
            "/opt/rust/bin/rustc --print cfg -O",
            "/opt/rust/bin/rustc -Z unstable-options --print target-spec-json",
            "/opt/rust/bin/cargo --version",
            "/opt/rust/bin/cargo locate-project --workspace --message-format json",
            "/opt/rust/bin/cargo metadata --no-deps --format-version 1 --manifest-path \
             /source/Cargo.toml",
            // The three forms rust-analyzer 1.98.1 really used, read from the
            // guest process table during the 2026-09-12 calibration.
            "/opt/rust/bin/cargo metadata --format-version 1 --no-deps --manifest-path \
             /opt/rust/lib/rustlib/src/rust/library/Cargo.toml --filter-platform \
             aarch64-unknown-linux-gnu",
            "/opt/rust/bin/cargo rustc -Z unstable-options --print cfg --target \
             aarch64-unknown-linux-gnu -- -O",
            "/opt/rust/bin/cargo rustc -Z unstable-options --print target-spec-json --target \
             aarch64-unknown-linux-gnu -- -Z unstable-options",
            "/opt/rust/bin/cargo rustc -Z unstable-options --print cfg",
        ] {
            assert!(admitted_argv(admitted), "{admitted} must be admitted");
        }
        for refused in [
            // The proc-macro server: same basename as the entrypoint, which is
            // exactly why the oracle is the whole argv.
            "/opt/analyzer/bin/rust-analyzer proc-macro",
            "/opt/analyzer/bin/rust-analyzer diagnostics /source",
            "rust-analyzer",
            "/source/target/debug/build/fixture-abc123/build-script-build",
            "/opt/rust/bin/rustfmt --edition 2024 /source/src/lib.rs",
            "/bin/sh -c id",
            "/opt/rust/bin/cargo build",
            // Without `--no-deps` the call resolves the dependency graph, which
            // is the thing §7 does not admit.
            "/opt/rust/bin/cargo metadata --format-version 1",
            "/opt/rust/bin/cargo metadata --no-deps",
            "/opt/rust/bin/cargo metadata --no-deps --format-version 1 --features hostile",
            "/opt/rust/bin/cargo metadata --no-deps --format-version 2",
            "/opt/rust/bin/cargo metadata --no-deps --format-version 1 --manifest-path",
            "/opt/rust/bin/rustc --crate-name fixture /source/src/lib.rs",
            "/opt/rust/bin/rustc",
            "",
        ] {
            assert!(!admitted_argv(refused), "{refused} must be refused");
        }
    }

    #[test]
    fn schema_keys_are_collected_from_any_nesting() {
        let schema = json!({
            "properties": {
                "rust-analyzer.cargo.noDeps": {"type": "boolean"},
                "nested": [{"rust-analyzer.numThreads": {"type": "integer"}}],
            }
        });
        let mut keys = BTreeSet::new();
        collect_keys(&schema, &mut keys);
        assert!(keys.contains("rust-analyzer.cargo.noDeps"));
        assert!(keys.contains("rust-analyzer.numThreads"));
        assert!(keys.contains("properties"));
    }

    #[test]
    fn a_dotted_key_matches_a_prefixed_schema_key() {
        // The schema may name keys with a `rust-analyzer.` prefix; a suffix
        // match accepts that without accepting an unrelated key that merely
        // ends in the same word.
        let known = "rust-analyzer.cargo.noDeps";
        assert!(known.ends_with(".cargo.noDeps"));
        assert!(!known.ends_with(".noDeps.cargo"));
        assert!(!"rust-analyzer.cargo.noDepsExtra".ends_with(".cargo.noDeps"));
    }
}
