//! Explicit M5 native qualification (ADR-073/074/075/076).
//!
//! Never part of runtime discovery: every test here is `#[ignore]`d, demands an
//! explicit `RUST_MCP_TEST_SOCKET`, and assumes exclusive ownership of the local
//! Docker engine. One `#[test]` per M5 cut runs that cut's selections in order,
//! writes `target/m5-runtime/cut-<cut>.json`, and rebuilds the merged
//! `target/m5-runtime/receipt.json` from every cut written so far.
//!
//! Two facts about this file are load-bearing and are stated once, here:
//!
//! * The selections exercise the three `pub(super)` port entry points — the
//!   product path, DTO included — and drop to [`crate::performance_gateway`]
//!   only where the assertion is about a gateway-level refusal the port maps
//!   away (`ProjectCargoConfiguration`) or about a counter the DTO does not
//!   carry (`cpus_sampled`, and the guest's own CPU count).
//! * `rust.benchmark.run`'s two positive selections cannot execute: criterion
//!   0.8.2's vendored closure fits neither
//!   [`rust_engineering_domain::SOURCE_MAX_ENTRIES`] nor
//!   [`rust_engineering_domain::SOURCE_MAX_TOTAL_BYTES`] nor
//!   [`rust_engineering_domain::SOURCE_MAX_FILE_BYTES`], so no
//!   `CargoVendorSnapshot` can carry it. That condition has its own test, which
//!   measures the tree, asserts all three violations and fails the moment any
//!   of them stops holding. The M5-01 cut therefore qualifies only what the
//!   tool really does here, and no test in this file reports a qualification it
//!   did not perform.
use crate::performance_gateway::{self, PerformanceError};
use crate::performance_port;
use crate::*;
use rust_engineering_application::benchmark::BenchmarkRunOptions;
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::{InspectionError, OperationControl, ProjectError};
use rust_engineering_domain::benchmark_run::{
    BenchmarkExit, BenchmarkObservation, DatasetOmission, HarnessDetection,
};
use rust_engineering_domain::bloat::{
    APPROVED_CARGO_BLOAT_VERSION, BinaryFormat, BloatCompleteness, BloatExit, BloatObservation,
    BloatOptions, BloatProfile,
};
use rust_engineering_domain::profile::{
    PROFILE_BACKEND, ProfileBuildOutcome, ProfileCompleteness, ProfileObservation, ProfileOptions,
    ProfileStatus,
};
use rust_engineering_domain::{
    CargoVendorSnapshot, SOURCE_MAX_ENTRIES, SOURCE_MAX_FILE_BYTES, SOURCE_MAX_TOTAL_BYTES,
    SourceBundle, SourceFile,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

type Failure = Box<dyn std::error::Error>;

/// The Docker client this qualification drives. Overridable only so the same
/// test can run on a host that installed the client elsewhere; the socket and
/// the image always come from the environment contract.
const DOCKER: &str = "/Applications/Docker.app/Contents/Resources/bin/docker";
const LABEL: &str = "--filter=label=org.rust-mcp.execution=true";
const RECEIPT_SCHEMA: &str = "rust-engineering-mcp.m5-runtime.v1";

/// Log ceiling for every operation: the same 512 KiB the port applies.
const LOG_BYTES: usize = 512 * 1024;

// -- environment contract ----------------------------------------------------

fn docker() -> PathBuf {
    std::env::var_os("RUST_MCP_TEST_DOCKER").map_or_else(|| PathBuf::from(DOCKER), PathBuf::from)
}

fn socket() -> Result<PathBuf, Failure> {
    let value = std::env::var_os("RUST_MCP_TEST_SOCKET").ok_or(
        "RUST_MCP_TEST_SOCKET is required: this M5 qualification drives Docker directly and \
         takes exclusive ownership of the engine. Set it to the absolute path of the socket \
         (for example ~/.docker/run/docker.sock) and run with --test-threads=1.",
    )?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err("RUST_MCP_TEST_SOCKET must be an explicit absolute socket path".into());
    }
    Ok(path)
}

/// The image under qualification. `RUST_MCP_TEST_IMAGE` may name it explicitly,
/// but it may only name the ADR-075 digest: every other image is refused by the
/// port, so pointing this qualification at one would test nothing.
fn m5_image() -> Result<String, Failure> {
    let image = std::env::var("RUST_MCP_TEST_IMAGE")
        .unwrap_or_else(|_| crate::APPROVED_M5_IMAGE.to_owned());
    if image != crate::APPROVED_M5_IMAGE {
        return Err(format!(
            "RUST_MCP_TEST_IMAGE={image} is not the qualified M5 runtime {}",
            crate::APPROVED_M5_IMAGE
        )
        .into());
    }
    Ok(image)
}

// -- session -----------------------------------------------------------------

/// A gateway plus its state root, torn down on every exit path — return, error
/// or unwind. The gateway is dropped inside `Drop` before the directory is
/// removed, because the gateway's own teardown writes into it.
struct Session {
    gateway: Option<RustGateway>,
    root: PathBuf,
}

impl Session {
    fn open(image_id: &str) -> Result<Self, Failure> {
        let root = PathBuf::from("/private/tmp").join(format!(
            "m5-native-{}",
            state::nonce().map_err(|error| format!("nonce: {error:?}"))?
        ));
        std::fs::create_dir(&root)?;
        // The directory is owned from here on: a failure below must still take
        // it down, so the session exists before the gateway does.
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
        // The qualification does not re-run base calibration; ADR-075's
        // provisioning receipt and the M4 base calibration already carry it.
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

// -- controls ----------------------------------------------------------------

/// The uncancelled control the product passes for a normal operation.
struct Proceed;
impl OperationControl for Proceed {
    fn check(&self) -> Result<(), ProjectError> {
        Ok(())
    }
}
impl ExecutionCancellation for Proceed {
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Cancels exactly once the named program is observed running inside a
/// container this product owns, so a cancellation selection proves it cancelled
/// real work rather than racing the operation's setup.
struct CancelWhenObserved<'a> {
    gateway: &'a RustGateway,
    needle: &'static str,
    observed: AtomicBool,
    failed: AtomicBool,
    polls: AtomicU64,
}

impl<'a> CancelWhenObserved<'a> {
    fn new(gateway: &'a RustGateway, needle: &'static str) -> Self {
        Self {
            gateway,
            needle,
            observed: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            polls: AtomicU64::new(0),
        }
    }
    fn evidence(&self) -> Value {
        json!({
            "needle": self.needle,
            "observed_running": self.observed.load(Ordering::SeqCst),
            "inventory_polls": self.polls.load(Ordering::SeqCst),
            "inventory_failed": self.failed.load(Ordering::SeqCst),
        })
    }
}

impl ExecutionCancellation for CancelWhenObserved<'_> {
    fn is_cancelled(&self) -> bool {
        if self.observed.load(Ordering::SeqCst) {
            return true;
        }
        self.polls.fetch_add(1, Ordering::SeqCst);
        match self.gateway.inner.control(&[
            "container".into(),
            "ls".into(),
            LABEL.into(),
            "--no-trunc".into(),
            "--format={{.Command}}".into(),
        ]) {
            Ok(result) if result.code == Some(0) => {
                if String::from_utf8_lossy(&result.stdout).contains(self.needle) {
                    self.observed.store(true, Ordering::SeqCst);
                    return true;
                }
                false
            }
            // An inventory this test cannot read is not evidence that nothing
            // is running: stop the operation rather than keep it going blind.
            _ => {
                self.failed.store(true, Ordering::SeqCst);
                true
            }
        }
    }
}

impl OperationControl for CancelWhenObserved<'_> {
    fn check(&self) -> Result<(), ProjectError> {
        if self.observed.load(Ordering::SeqCst) {
            Err(ProjectError::Cancelled)
        } else {
            Ok(())
        }
    }
}

// -- fixtures ----------------------------------------------------------------

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn source_file(path: &str, bytes: Vec<u8>) -> Result<SourceFile, Failure> {
    Ok(SourceFile::new(path.to_owned(), bytes).map_err(|error| format!("{path}: {error:?}"))?)
}

/// Walks a fixture directory into the owned, bounded shape the product
/// ingests. `target/` and `.git/` are build and VCS state, never fixture input.
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
    let mut directories = source.directories().to_vec();
    for (path, bytes) in additions {
        files.push(source_file(&path, bytes)?);
    }
    directories.sort();
    directories.dedup();
    Ok(SourceBundle::with_directories(files, directories)
        .map_err(|error| format!("extended bundle: {error:?}"))?)
}

/// Replaces one member's bytes, keeping every other member untouched.
fn replacing(source: &SourceBundle, path: &str, bytes: Vec<u8>) -> Result<SourceBundle, Failure> {
    let mut files = source
        .files()
        .iter()
        .filter(|file| file.path() != path)
        .cloned()
        .collect::<Vec<_>>();
    files.push(source_file(path, bytes)?);
    Ok(
        SourceBundle::with_directories(files, source.directories().to_vec())
            .map_err(|error| format!("replaced bundle: {error:?}"))?,
    )
}

fn bundle_facts(name: &str, source: &SourceBundle) -> Result<Value, Failure> {
    let archive = source_archive::encode(source).map_err(|error| format!("{name}: {error:?}"))?;
    let fingerprint = resolution_gateway::tree_fingerprint(source)
        .map_err(|error| format!("{name}: {error:?}"))?;
    Ok(json!({
        "files": source.files().len(),
        "directories": source.directories().len(),
        "bytes": source.files().iter().map(|file| file.bytes().len()).sum::<usize>(),
        "tree_fingerprint": fingerprint.to_string(),
        "archive_sha256": digest(&archive),
    }))
}

/// An empty vendor tree. Three of the four M5 fixtures resolve entirely from
/// path dependencies, so an empty directory source is the honest input for
/// them: nothing is hidden, because nothing is needed.
fn empty_vendor() -> Result<CargoVendorSnapshot, Failure> {
    let empty = SourceBundle::new(vec![]).map_err(|error| format!("{error:?}"))?;
    Ok(CargoVendorSnapshot {
        tree_fingerprint: resolution_gateway::tree_fingerprint(&empty)
            .map_err(|error| format!("{error:?}"))?,
        source: empty,
        packages: Vec::new(),
    })
}

/// What the materialized criterion vendor tree actually is, measured against
/// the bounds a [`CargoVendorSnapshot`] must satisfy.
struct VendorShape {
    files: usize,
    directories: usize,
    total_bytes: u64,
    oversized: Vec<String>,
    materialized: bool,
}

impl VendorShape {
    fn fits(&self) -> bool {
        self.materialized
            && self.oversized.is_empty()
            && self.files + self.directories <= SOURCE_MAX_ENTRIES
            && self.total_bytes <= SOURCE_MAX_TOTAL_BYTES as u64
    }
    fn facts(&self) -> Value {
        json!({
            "materialized": self.materialized,
            "files": self.files,
            "directories": self.directories,
            "entries": self.files + self.directories,
            "total_bytes": self.total_bytes,
            "files_over_source_max_file_bytes": self.oversized,
            "bounds": {
                "source_max_entries": SOURCE_MAX_ENTRIES,
                "source_max_total_bytes": SOURCE_MAX_TOTAL_BYTES,
                "source_max_file_bytes": SOURCE_MAX_FILE_BYTES,
            },
            "fits_source_bundle_bounds": self.fits(),
        })
    }
}

fn measure_vendor(root: &Path) -> Result<VendorShape, Failure> {
    let mut shape = VendorShape {
        files: 0,
        directories: 0,
        total_bytes: 0,
        oversized: Vec::new(),
        materialized: root.is_dir(),
    };
    if !shape.materialized {
        return Ok(shape);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                shape.directories += 1;
                stack.push(path);
            } else if kind.is_file() {
                shape.files += 1;
                let bytes = entry.metadata()?.len();
                shape.total_bytes = shape.total_bytes.saturating_add(bytes);
                if bytes > SOURCE_MAX_FILE_BYTES as u64 {
                    shape.oversized.push(
                        path.strip_prefix(root)?
                            .to_str()
                            .unwrap_or("<non-utf8>")
                            .to_owned(),
                    );
                }
            } else {
                return Err(format!("special vendor entry: {}", path.display()).into());
            }
        }
    }
    shape.oversized.sort();
    Ok(shape)
}

// -- receipt -----------------------------------------------------------------

fn receipt_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/m5-runtime")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `YYYY-MM-DDTHH:MM:SSZ` from a Unix second, by the civil-from-days algorithm.
/// A clock this process cannot read stamps the epoch rather than a plausible
/// time; the receipt's evidence is its fingerprints, not its stamp.
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

fn selection(name: &str, started: Instant, outcome: &str, observed: Value) -> Value {
    json!({
        "selection": name,
        "outcome": outcome,
        "duration_ms": elapsed_ms(started),
        "observed": observed,
    })
}

struct Cut {
    name: &'static str,
    started_unix: u64,
    selections: Vec<Value>,
    fixtures: serde_json::Map<String, Value>,
    residue_before: Value,
    residue_after: Value,
}

impl Cut {
    fn open(name: &'static str) -> Self {
        Self {
            name,
            started_unix: unix_now(),
            selections: Vec::new(),
            fixtures: serde_json::Map::new(),
            residue_before: Value::Null,
            residue_after: Value::Null,
        }
    }
    fn document(&self, image_id: &str) -> Value {
        json!({
            "cut": self.name,
            "image_id": image_id,
            "started_utc": utc(self.started_unix),
            "finished_utc": utc(unix_now()),
            "residue": {"before": self.residue_before, "after": self.residue_after},
            "fixtures": Value::Object(self.fixtures.clone()),
            "selections": self.selections,
        })
    }
}

/// Writes this cut's own document, then rebuilds the merged receipt from every
/// cut document present. Cuts run one at a time, so the last one to finish
/// leaves a complete `receipt.json`; each one on its own leaves a truthful
/// partial receipt naming exactly the cuts that have run.
fn publish(cut: &Cut, image_id: &str) -> Result<PathBuf, Failure> {
    let root = receipt_root();
    std::fs::create_dir_all(&root)?;
    std::fs::write(
        root.join(format!("cut-{}.json", cut.name)),
        serde_json::to_vec_pretty(&cut.document(image_id))?,
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

    let mut selections = Vec::new();
    let mut fixtures = serde_json::Map::new();
    let mut started = None::<String>;
    let mut finished = None::<String>;
    let mut before = Value::Null;
    let mut after = Value::Null;
    for document in documents.values() {
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
            {
                started = Some(value.clone());
                before = document
                    .get("residue")
                    .and_then(|residue| residue.get("before"))
                    .cloned()
                    .unwrap_or(Value::Null);
            }
        }
        if let Some(Value::String(value)) = document.get("finished_utc")
            && finished.as_ref().is_none_or(|current| value > current)
        {
            {
                finished = Some(value.clone());
                after = document
                    .get("residue")
                    .and_then(|residue| residue.get("after"))
                    .cloned()
                    .unwrap_or(Value::Null);
            }
        }
    }

    let receipt = json!({
        "schema": RECEIPT_SCHEMA,
        "started_utc": started,
        "finished_utc": finished,
        "image_id": image_id,
        "image_tag_at_provisioning": "rust-engineering-runtime:1.98.1-arm64-m5",
        "fixtures": Value::Object(fixtures),
        "residue": {"before": before, "after": after},
        "cuts": documents.values().cloned().collect::<Vec<_>>(),
        "selections": selections,
    });
    let path = root.join("receipt.json");
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    std::fs::write(&path, &bytes)?;
    println!("M5_RUNTIME_RECEIPT {} {}", digest(&bytes), path.display());
    Ok(path)
}

// -- shared assertions -------------------------------------------------------

fn benchmark_counters(observation: &BenchmarkObservation) -> Value {
    json!({
        "harness": format!("{:?}", observation.harness),
        "exit": format!("{:?}", observation.exit),
        "exit_code": observation.exit_code,
        "termination": format!("{:?}", observation.termination),
        "runs_completed": observation.runs_completed,
        "runs_requested": observation.runs_requested,
        "omission": observation.omission.map(|value| format!("{value:?}")),
        "measurements": observation.dataset.as_ref().map(|dataset| {
            dataset
                .measurements()
                .iter()
                .map(|measurement| json!({
                    "key": measurement.key(),
                    "samples": measurement.samples().len(),
                    "completeness": format!("{:?}", measurement.completeness()),
                }))
                .collect::<Vec<_>>()
        }),
        "stdout_bytes": observation.stdout.len(),
        "stderr_bytes": observation.stderr.len(),
        "execution_fingerprint": observation.execution_fingerprint.to_string(),
        "vendor_fingerprint": observation.vendor_fingerprint.to_string(),
    })
}

fn profile_counters(observation: &ProfileObservation) -> Value {
    json!({
        "build": format!("{:?}", observation.build),
        "build_exit_code": observation.build_exit_code,
        "status": format!("{:?}", observation.status),
        "completeness": format!("{:?}", observation.completeness),
        "backend": observation.backend,
        "perf_errno": observation.perf_errno,
        "child": {"exit_code": observation.child.exit_code, "signal": observation.child.signal},
        "counters": {
            "observed_duration_ms": observation.counters.observed_duration_ms,
            "samples_collected": observation.counters.samples_collected,
            "samples_lost": observation.counters.samples_lost,
            "stacks_written": observation.counters.stacks_written,
            "frames_total": observation.counters.frames_total,
            "frames_unresolved": observation.counters.frames_unresolved,
            "stacks_truncated": observation.counters.stacks_truncated,
            "modules_seen": observation.counters.modules_seen,
            "max_depth_applied": observation.counters.max_depth_applied,
        },
        "stacks_bytes": observation.stacks.len(),
        "svg_bytes": observation.svg.len(),
        "top_frames": observation.top_frames.len(),
        "execution_fingerprint": observation.execution_fingerprint.to_string(),
    })
}

fn bloat_counters(observation: &BloatObservation) -> Value {
    json!({
        "exit": format!("{:?}", observation.exit),
        "exit_code": observation.exit_code,
        "analyzer_version": observation.analyzer_version,
        "completeness": format!("{:?}", observation.completeness),
        "measured": observation.measured.as_ref().map(|measured| json!({
            "size_bytes": measured.size_bytes,
            "sha256": measured.sha256,
            "format": format!("{:?}", measured.format),
            "analysis_build_symbols_forced": measured.analysis_build_symbols_forced,
        })),
        "attribution": observation.attribution.as_ref().map(|attribution| json!({
            "estimated": attribution.estimated,
            "reported_file_size_bytes": attribution.reported_file_size_bytes,
            "text_section_size_bytes": attribution.text_section_size_bytes,
            "functions": attribution.functions.len(),
            "functions_omitted_by_row_cap": attribution.functions_omitted_by_row_cap,
            "crates": attribution.crates.iter().map(|entry| json!({
                "name": entry.name, "size_bytes": entry.size_bytes,
            })).collect::<Vec<_>>(),
            "crates_omitted_by_row_cap": attribution.crates_omitted_by_row_cap,
        })),
        "analysis_validated": observation.analysis_validated(),
        "execution_fingerprint": observation.execution_fingerprint.to_string(),
    })
}

/// The frames of the heaviest stack in a collapsed artifact, plus its share.
fn hottest(stacks: &str) -> Result<(Vec<String>, u64, u64), Failure> {
    let mut total = 0u64;
    let mut best: Option<(Vec<String>, u64)> = None;
    for line in stacks.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (frames, count) = line
            .rsplit_once(' ')
            .ok_or_else(|| format!("collapsed line without a count: {line}"))?;
        let count = count.trim().parse::<u64>()?;
        total = total.saturating_add(count);
        if best.as_ref().is_none_or(|(_, best)| count > *best) {
            best = Some((frames.split(';').map(str::to_owned).collect(), count));
        }
    }
    let (frames, count) = best.ok_or("collapsed artifact carried no stack")?;
    Ok((frames, count, total))
}

// -- M5-01 rust.benchmark.run ------------------------------------------------

#[test]
#[ignore = "explicit M5 image, host Docker and exclusive native benchmark qualification"]
fn m5_benchmark_run_negatives_and_controls_are_qualified_natively() -> Result<(), Failure> {
    let image = m5_image()?;
    let mut cut = Cut::open("m5-01-benchmark");
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "m5-01 before")?;

    let benchmark = fixture_bundle("benchmark")?;
    let bloat = fixture_bundle("bloat")?;
    cut.fixtures
        .insert("benchmark".into(), bundle_facts("benchmark", &benchmark)?);
    cut.fixtures
        .insert("bloat".into(), bundle_facts("bloat", &bloat)?);

    // Selections 1 and 2, the two criterion positives, are not here. They
    // cannot execute at all — the harness closure does not fit the
    // offline-data contract — and that condition is qualified by its own oracle,
    // `m5_benchmark_run_positive_is_blocked_by_the_offline_data_bound`. What
    // this cut qualifies is everything `rust.benchmark.run` really does do on
    // this host: the harness it refuses to measure, the project configuration
    // it refuses to obey, and the cancellation of a live `cargo bench`.

    // -- selection 3: an unrecognised harness still reports ------------------
    let started = Instant::now();
    let options = BenchmarkRunOptions::new(None, None, Vec::new(), false, false, 1, 300)
        .map_err(|error| format!("options: {error:?}"))?;
    let observation =
        performance_port::benchmark(gateway, &bloat, &empty_vendor()?, &options, &Proceed)
            .map_err(|error| format!("unrecognised-harness: {error:?}"))?;
    assert_eq!(
        observation.harness,
        HarnessDetection::Unrecognized,
        "fixtures/bloat declares no criterion dependency"
    );
    assert!(
        observation.dataset.is_none(),
        "a project without the approved harness must publish no dataset"
    );
    assert_eq!(
        observation.omission,
        Some(DatasetOmission::HarnessUnrecognized)
    );
    assert_eq!(observation.exit, BenchmarkExit::Passed);
    assert_eq!(observation.exit_code, Some(0));
    assert_eq!(observation.runs_requested, 1);
    assert_eq!(observation.runs_completed, 1);
    assert_eq!(observation.runtime.image_id, crate::APPROVED_M5_IMAGE);
    assert!(
        !observation.stdout.is_empty() || !observation.stderr.is_empty(),
        "execution logs must still be reported when no dataset is published"
    );
    assert!(observation.consistent());
    cut.selections.push(selection(
        "unrecognised-harness",
        started,
        "passed",
        benchmark_counters(&observation),
    ));
    clean(gateway, "m5-01 after unrecognised-harness")?;

    // -- selection 4: a project Cargo configuration is refused ---------------
    let started = Instant::now();
    let before = clean(gateway, "m5-01 before project-cargo-configuration")?;
    let configured = with_files(
        &benchmark,
        [(
            ".cargo/config.toml".into(),
            b"[build]\nrustflags = [\"-C\", \"target-cpu=native\"]\n".to_vec(),
        )],
    )?;
    let refused = performance_gateway::execute_benchmark(
        gateway,
        &configured,
        &empty_vendor()?,
        &options.selection(),
        1,
        ExecutionLimits::new_job(300_000, LOG_BYTES).ok_or("limits")?,
        &Proceed,
    );
    let error = match refused {
        Ok(_) => return Err("a project Cargo configuration was not refused".into()),
        Err(error) => error,
    };
    assert_eq!(error, PerformanceError::ProjectCargoConfiguration);
    // The same refusal, in the vocabulary the tool answers in: a containment
    // refusal, never an unavailable capability.
    let mapped =
        performance_port::benchmark(gateway, &configured, &empty_vendor()?, &options, &Proceed);
    assert_eq!(
        mapped.err(),
        Some(SecurityError::Inspection(InspectionError::Project(
            ProjectError::Rejected(rust_engineering_domain::OperationalErrorCode::SandboxDenied)
        )))
    );
    let after = clean(gateway, "m5-01 after project-cargo-configuration")?;
    assert_eq!(before, after, "the refusal must create nothing at all");
    cut.selections.push(selection(
        "project-cargo-configuration-refused",
        started,
        "passed",
        json!({
            "gateway_error": format!("{error:?}"),
            "residue_before": before,
            "residue_after": after,
        }),
    ));

    // -- selection 5: cancellation mid-run -----------------------------------
    let started = Instant::now();
    let monitor = CancelWhenObserved::new(gateway, "/opt/rust/bin/cargo bench");
    let cancelled =
        performance_port::benchmark(gateway, &bloat, &empty_vendor()?, &options, &monitor);
    assert_eq!(
        cancelled.err(),
        Some(SecurityError::Inspection(InspectionError::Project(
            ProjectError::Cancelled
        ))),
        "cancelling a benchmark must be reported as a cancellation"
    );
    assert!(
        monitor.observed.load(Ordering::SeqCst),
        "cargo bench was never observed running: {}",
        monitor.evidence()
    );
    assert!(!monitor.failed.load(Ordering::SeqCst));
    let after = clean(gateway, "m5-01 after cancellation")?;
    cut.selections.push(selection(
        "cancellation-mid-run",
        started,
        "passed",
        json!({"monitor": monitor.evidence(), "residue_after": after}),
    ));

    cut.residue_after = clean(gateway, "m5-01 after")?;
    publish(&cut, &image)?;
    Ok(())
}

/// The M5-01 positive is unreachable, and this pins exactly why.
///
/// `rust.benchmark.run` resolves its harness offline from a
/// `CargoVendorSnapshot`, which is a `SourceBundle`, and criterion 0.8.2's
/// closure does not fit a `SourceBundle`. The bounds were not widened: they
/// belong to the offline-data contract qualified in M2/M4 and shared by every
/// flow that carries host data into the guest.
///
/// This oracle passes while that is true and **fails the moment it stops being
/// true**, which is the only reason it may pass at all: a blocker that can
/// disappear silently is not recorded, it is forgotten. It executes nothing and
/// needs no container — there is no snapshot to run anything against — so it
/// measures the tree the product would have had to ingest and checks it against
/// the product's own constants.
#[test]
#[ignore = "explicit M5 image; records the M5-01 blocking condition"]
fn m5_benchmark_run_positive_is_blocked_by_the_offline_data_bound() -> Result<(), Failure> {
    let image = m5_image()?;
    let mut cut = Cut::open("m5-01-benchmark-blocked");
    let vendor_root = fixtures().join("criterion-vendor/vendor");
    let shape = measure_vendor(&vendor_root)?;
    cut.fixtures
        .insert("criterion_vendor".into(), shape.facts());

    if !shape.materialized {
        return Err(format!(
            "the criterion vendor tree is absent at {}; run \
             fixtures/criterion-vendor/materialize.py. Its absence is a host condition, not the \
             blocking condition this oracle exists to record",
            vendor_root.display()
        )
        .into());
    }

    // Each violation is asserted on its own, because they are not
    // interchangeable: the per-file bound is the one that cannot be pruned
    // away, and it is what makes every proposed workaround fail.
    let started = Instant::now();
    let over_entries = shape.files + shape.directories > SOURCE_MAX_ENTRIES;
    let over_total = shape.total_bytes > SOURCE_MAX_TOTAL_BYTES as u64;
    let over_per_file = !shape.oversized.is_empty();
    if !(over_entries && over_total && over_per_file) {
        return Err(format!(
            "the criterion closure no longer breaks all three bounds (entries {over_entries}, \
             total {over_total}, per-file {over_per_file}); the M5-01 positive must now be \
             implemented and qualified instead of recorded as blocked. Observed: {} files, {} \
             directories, {} bytes, {} file(s) over {SOURCE_MAX_FILE_BYTES}; bounds are \
             {SOURCE_MAX_ENTRIES} entries and {SOURCE_MAX_TOTAL_BYTES} bytes",
            shape.files,
            shape.directories,
            shape.total_bytes,
            shape.oversized.len(),
        )
        .into());
    }
    assert!(
        !shape.fits(),
        "a closure that breaks three bounds cannot be admissible"
    );

    for (name, run_count) in [("positive-run-count-1", 1u8), ("pooled-run-count-2", 2u8)] {
        cut.selections.push(selection(
            name,
            started,
            "blocked",
            json!({
                "run_count": run_count,
                "reason": "criterion vendor closure exceeds SourceBundle bounds",
                "over_entries": over_entries,
                "over_total_bytes": over_total,
                "over_per_file_bytes": over_per_file,
                "vendor": shape.facts(),
            }),
        ));
    }
    publish(&cut, &image)?;
    Ok(())
}

// -- M5-03 rust.profile.flamegraph -------------------------------------------

/// The profiled program that leaves a descendant behind, and the only fixture
/// source in this file long enough to deserve being written as one.
///
/// It is project code, compiled inside the guest at run time from the bundle
/// below, so it needs no change to the admitted image; the profiled binary is
/// exec'd by absolute path, so `argv[0]` is that path and the program can
/// re-exec itself without `/proc`, which the sampling container does not mount.
const FORK_SOURCE: &str = r##"//! Containment oracle: a profiled program that leaves a descendant behind.
//!
//! Nothing else in this fixture forks, so before this target existed the
//! helper's drain had nothing to reap in any recorded run: the child was
//! already reaped when the drain ran, `kill(-1)` reached an empty namespace and
//! `descendants_reaped` was `0` in every manifest. This target gives it work.
//!
//! Three processes, two re-execs of this one binary:
//!
//! * the *root* is what the profiler started. It re-execs itself as the relay,
//!   waits for it, and is then the hot workload the profiler measures;
//! * the *relay* re-execs this binary once more as the orphan and exits
//!   immediately, without waiting for it;
//! * the *orphan* outlives the relay and is reparented by the kernel onto pid 1
//!   of the sampling container's PID namespace, which is the profiler helper.
//!
//! The orphan outliving its parent is structural rather than a matter of
//! timing: `Command::spawn` returns only once the spawned process has reached
//! `exec`, the relay exits on the next statement, and the orphan holds a loop
//! that outlasts any sampling window. The root waits for the relay, so past
//! that point the orphan exists and is already the helper's own child.
//!
//! While it lives, the orphan goes after the artifact the helper has not
//! written yet. In the default shape the open carries no `O_CREAT`, so the
//! orphan can only ever corrupt a file that is already there and can never
//! create one: if the drain does its job, every one of those opens fails and
//! both artifacts are exactly what the helper wrote. In the `--create` shape
//! the orphan creates the file first instead, and the helper's own `O_EXCL`
//! then refuses to write over it.

#[path = "main.rs"]
mod workload;

use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// The root's busy-loop budget: the fixture's own default, so the sampled
/// stack is the same known hot frame.
const BUDGET: Duration = Duration::from_millis(2000);

/// How long the orphan keeps going. Far past any sampling window this fixture
/// is used with, so what ends it is the helper's drain -- or, if the drain ever
/// failed, pid 1 exiting and taking the whole namespace with it.
const ORPHAN_BUDGET: Duration = Duration::from_secs(600);

/// Interval between two attempts, and between two looks for the artifact.
const POLL: Duration = Duration::from_millis(1);

/// The artifact the orphan goes after. The helper writes this one first, so a
/// pre-created copy is also the first thing its `O_EXCL` meets.
const ARTIFACT: &str = "/profile/stacks.txt";

/// What the orphan writes if it ever gets the chance. It is not a collapsed
/// stack, so an artifact carrying it cannot reconcile with the manifest.
const TAMPER: &[u8] = b"# a descendant wrote this after the drain\n";

const RELAY: &str = "--relay";
const ORPHAN: &str = "--orphan";
const CREATE: &str = "--create";
const KEEP: &str = "--no-create";

/// Exit codes. Anything but `0` says the descendant was never created, which is
/// a failure of this fixture and not an observation about the drain.
const EXIT_SPAWN_FAILED: i32 = 10;
const EXIT_RELAY_FAILED: i32 = 11;
const EXIT_NO_ARTIFACT: i32 = 12;
const EXIT_UNKNOWN_ROLE: i32 = 13;

/// Re-execs this binary in another role. The caller decides whether to wait.
///
/// The three descriptors are closed rather than inherited on purpose: the
/// orphan outlives everything else here, and a long-lived process holding the
/// parent's stdout open is a process that can keep whoever reads that stream
/// waiting long after the profiled program is gone.
fn respawn(role: &str, shape: &str) -> Option<Child> {
    let program = std::env::args_os().next()?;
    Command::new(program)
        .arg(role)
        .arg(shape)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
}

/// The process the profiler started.
fn root(create: bool) -> i32 {
    let shape = if create { CREATE } else { KEEP };
    let Some(mut relay) = respawn(RELAY, shape) else {
        return EXIT_SPAWN_FAILED;
    };
    // The synchronization point: the relay spawns the orphan and returns at
    // once, so a relay that has exited is a relay whose child has exec'd and
    // has already been reparented onto pid 1.
    match relay.wait() {
        Ok(status) if status.success() => {}
        _ => return EXIT_RELAY_FAILED,
    }
    // In the pre-creating shape the file has to be on the volume before this
    // process does anything else, so the helper's own write can only ever come
    // second. That is an ordering, not a race: the helper does not write until
    // this process exits or its window closes, and this process does not
    // proceed until the file is there.
    if create && !await_artifact() {
        return EXIT_NO_ARTIFACT;
    }
    println!("{:016x}", workload::known_hot_frame(BUDGET));
    0
}

/// The middle process. It exists only to die, which is what orphans its child.
fn relay(shape: &str) -> i32 {
    // Deliberately never waited for. Dropping a `Child` neither kills nor
    // reaps, so the orphan survives this process by the whole of its budget.
    if respawn(ORPHAN, shape).is_some() {
        0
    } else {
        EXIT_SPAWN_FAILED
    }
}

/// The descendant the drain has to find.
fn orphan(create: bool) -> i32 {
    let deadline = Instant::now() + ORPHAN_BUDGET;
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(create);
    while Instant::now() < deadline {
        if let Ok(mut file) = options.open(ARTIFACT) {
            let _ = file.write_all(TAMPER);
        }
        std::thread::sleep(POLL);
    }
    0
}

/// Blocks until the orphan has created the artifact.
fn await_artifact() -> bool {
    let deadline = Instant::now() + ORPHAN_BUDGET;
    while Instant::now() < deadline {
        if std::fs::metadata(ARTIFACT).is_ok() {
            return true;
        }
        std::thread::sleep(POLL);
    }
    false
}

/// Routes the three roles. The profiler passes the program no argv at all, so
/// the argv-less case is the root by construction and `create` can only come
/// from which of the two binary targets was built.
pub fn dispatch(create: bool) -> i32 {
    let mut argv = std::env::args().skip(1);
    let role = argv.next();
    let creates = argv.next().as_deref() == Some(CREATE);
    match role.as_deref() {
        None => root(create),
        Some(value) if value == RELAY => relay(if creates { CREATE } else { KEEP }),
        Some(value) if value == ORPHAN => orphan(creates),
        Some(_) => EXIT_UNKNOWN_ROLE,
    }
}

fn main() {
    std::process::exit(dispatch(false));
}
"##;

/// The pre-creating shape of the same descendant, as its own binary target
/// because the gateway gives the profiled program no argv to select one with.
const PRECREATE_SOURCE: &str = r##"//! The same descendant, in the shape that gets to the artifact first.
//!
//! Identical to `rust-mcp-profile-fork` in every respect but one: the orphan is
//! allowed to *create* `/profile/stacks.txt`, and the root does not start its
//! workload until the file is on the volume. The helper then opens its own
//! output `O_WRONLY|O_CREAT|O_EXCL` onto a path it did not write, refuses to
//! overwrite it, and exits 4 with nothing exported.

#[path = "fork.rs"]
mod fork;

fn main() {
    std::process::exit(fork::dispatch(true));
}
"##;

/// The profile fixture, plus four binary targets this qualification needs and
/// the product cannot reach otherwise: the gateway passes the profiled child no
/// argv at all, so `rust-mcp-profile-workload --zero` is unreachable through
/// `rust.profile.flamegraph`, and so is any other shape of the same program.
/// Every added target calls the fixture's own `known_hot_frame` through
/// `#[path]`, so the measured code is the fixture's.
fn profile_bundle() -> Result<SourceBundle, Failure> {
    let base = fixture_bundle("profile-workload")?;
    let manifest = base
        .files()
        .iter()
        .find(|file| file.path() == "Cargo.toml")
        .ok_or("profile fixture has no Cargo.toml")?
        .bytes()
        .to_vec();
    let mut manifest = String::from_utf8(manifest)?;
    manifest.push_str(
        "\n[[bin]]\nname = \"rust-mcp-profile-zero\"\npath = \"src/zero.rs\"\n\
         \n[[bin]]\nname = \"rust-mcp-profile-hold\"\npath = \"src/hold.rs\"\n\
         \n[[bin]]\nname = \"rust-mcp-profile-fork\"\npath = \"src/fork.rs\"\n\
         \n[[bin]]\nname = \"rust-mcp-profile-precreate\"\npath = \"src/precreate.rs\"\n",
    );
    let base = replacing(&base, "Cargo.toml", manifest.into_bytes())?;
    with_files(
        &base,
        [
            (
                "src/zero.rs".into(),
                b"//! Zero-sample control: the fixture's `--zero` path, reached without argv.\n\
                  #[path = \"main.rs\"]\n\
                  mod workload;\n\
                  fn main() {\n    \
                      println!(\"{:016x}\", workload::known_hot_frame(std::time::Duration::ZERO));\n\
                  }\n"
                    .to_vec(),
            ),
            (
                "src/hold.rs".into(),
                b"//! Long-running variant, so a cancellation selection can observe the sampler.\n\
                  #[path = \"main.rs\"]\n\
                  mod workload;\n\
                  fn main() {\n    \
                      println!(\n        \"{:016x}\",\n        \
                      workload::known_hot_frame(std::time::Duration::from_secs(30))\n    );\n\
                  }\n"
                    .to_vec(),
            ),
            ("src/fork.rs".into(), FORK_SOURCE.as_bytes().to_vec()),
            (
                "src/precreate.rs".into(),
                PRECREATE_SOURCE.as_bytes().to_vec(),
            ),
        ],
    )
}

/// Arms [`performance_gateway::DENY_PERF_EVENT_OPEN`] for one selection and
/// disarms it however that selection ends.
///
/// This is the one test-only hook in the M5 qualification, and it exists
/// because ADR-074's `denial_control` cannot be produced any other way: the
/// seccomp profile the sampling phase names is the only mechanism in the
/// product that refuses `perf_event_open`, and nothing a caller, an option or a
/// project can express reaches it — that is the whole point of the containment.
/// The alternative was to leave the mandatory negative oracle as a hand-run
/// docker session recorded in `docs/validation/M5-03-profiling-native.json`,
/// which is not a receipt of the product path. With the switch armed the
/// selection below still goes through `performance_port::profile` and every
/// phase, argv and `verify_applied` comparison it always performs; the only
/// difference is that the sampling phase names the committed quality profile,
/// under which `perf_event_open` returns EPERM.
struct DeniedPerfEventOpen;

impl DeniedPerfEventOpen {
    fn arm() -> Self {
        performance_gateway::DENY_PERF_EVENT_OPEN.with(|switch| switch.set(true));
        Self
    }
}

impl Drop for DeniedPerfEventOpen {
    fn drop(&mut self) {
        performance_gateway::DENY_PERF_EVENT_OPEN.with(|switch| switch.set(false));
    }
}

/// `defaultErrnoRet` of both committed seccomp profiles: a syscall the profile
/// does not allow returns EPERM rather than killing the process.
const SECCOMP_EPERM: i32 = 1;

#[test]
#[ignore = "explicit M5 image, host Docker and exclusive native profiling qualification"]
fn m5_profile_flamegraph_is_qualified_natively() -> Result<(), Failure> {
    let image = m5_image()?;
    let mut cut = Cut::open("m5-03-profile");
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "m5-03 before")?;

    let source = profile_bundle()?;
    let vendor = empty_vendor()?;
    cut.fixtures.insert(
        "profile_workload".into(),
        bundle_facts("profile-workload", &source)?,
    );

    // -- selection 6: the positive -------------------------------------------
    let started = Instant::now();
    let options = ProfileOptions::new("rust-mcp-profile-workload".into(), 99, 4)
        .map_err(|error| format!("options: {error:?}"))?;
    let observation = performance_port::profile(gateway, &source, &vendor, &options, &Proceed)
        .map_err(|error| format!("profile positive: {error:?}"))?;
    assert_eq!(observation.build, ProfileBuildOutcome::Built);
    assert_eq!(observation.build_exit_code, Some(0));
    assert_eq!(
        observation.status,
        ProfileStatus::Complete,
        "counters: {}",
        profile_counters(&observation)
    );
    assert_eq!(observation.backend, PROFILE_BACKEND);
    assert!(observation.counters.samples_collected > 0);
    assert!(
        observation.counters.samples_collected >= 50,
        "99 Hz over a 2 s workload should collect far more than 50 samples, got {}",
        observation.counters.samples_collected
    );
    assert_eq!(observation.counters.samples_lost, 0);
    assert_eq!(observation.counters.stacks_truncated, 0);
    assert_eq!(observation.completeness, ProfileCompleteness::Complete);
    assert_eq!(observation.perf_errno, None);
    assert!(observation.consistent());
    assert!(!observation.top_frames.is_empty());

    let stacks = String::from_utf8(observation.stacks.clone())?;
    let (frames, hot, total) = hottest(&stacks)?;
    assert!(
        frames.len() >= 4,
        "the hottest stack is shorter than the known chain: {frames:?}"
    );
    let tail = &frames[frames.len() - 4..];
    for (index, expected) in ["main", "level_one", "level_two", "known_hot_frame"]
        .into_iter()
        .enumerate()
    {
        assert!(
            tail[index].contains(expected),
            "the hottest stack does not end with the known chain: tail {tail:?}"
        );
    }
    assert!(
        hot * 4 >= total * 3,
        "the known stack carried {hot} of {total} samples, not the large majority"
    );
    let mut all_frames = 0usize;
    for line in stacks.lines() {
        let Some((frames, _)) = line.rsplit_once(' ') else {
            continue;
        };
        for frame in frames.split(';') {
            all_frames += 1;
            assert!(
                !frame.contains('/'),
                "an emitted frame carried a path separator: {frame}"
            );
        }
    }

    let svg = String::from_utf8(observation.svg.clone())?;
    assert!(!svg.is_empty());
    assert!(svg.starts_with("<svg"), "svg did not start with <svg");
    for forbidden in [
        "<script",
        "href",
        "xlink:",
        "<foreignObject",
        "<image",
        "<use",
        "javascript:",
        "data:",
        "<!ENTITY",
        "<!DOCTYPE",
    ] {
        assert!(!svg.contains(forbidden), "the svg carried {forbidden}");
    }
    assert_eq!(
        svg.matches("http").count(),
        1,
        "the only http occurrence permitted is the SVG namespace"
    );
    let mut positive = profile_counters(&observation);
    if let Some(object) = positive.as_object_mut() {
        object.insert(
            "hottest_stack".into(),
            json!({
                "frames": frames,
                "samples": hot,
                "total_samples": total,
                "distinct_frames_emitted": all_frames,
            }),
        );
    }
    cut.selections
        .push(selection("profile-positive", started, "passed", positive));
    clean(gateway, "m5-03 after positive")?;

    // -- selection 6b: the counters the DTO does not carry --------------------
    //
    // `cpus_sampled` is the helper's own field and the port deliberately does
    // not read it (ADR-074 §5 lets the helper gain fields ahead of the reader).
    // It is still evidence, so it is read here from the raw manifest and
    // compared with the CPU count the same execution probed from the guest.
    let started = Instant::now();
    let execution = performance_gateway::execute_profile(
        gateway,
        &source,
        &vendor,
        &options,
        ExecutionLimits::new_job(300_000, LOG_BYTES).ok_or("limits")?,
        &Proceed,
    )
    .map_err(|error| format!("profile counters: {error:?}"))?;
    let output = execution.profile.as_ref().ok_or("no profile output")?;
    let manifest: Value = serde_json::from_slice(&output.manifest)?;
    let cpus_sampled = manifest
        .get("cpus_sampled")
        .and_then(Value::as_u64)
        .ok_or("the helper manifest declared no cpus_sampled")?;
    let cpu_cores = execution
        .hardware
        .cpu_cores
        .ok_or("the CPU probe reported no core count")?;
    assert!(cpus_sampled > 0, "the helper sampled no CPU at all");
    assert_eq!(
        cpus_sampled,
        u64::from(cpu_cores),
        "the helper must fan out over every online CPU the guest reports"
    );
    assert!(execution.hardware.cpu_model.is_some());
    assert!(execution.hardware.os_kernel.is_some());
    // The same integrity claim on the positive path: the helper is pid 1 of its
    // own namespace and emptied it before rendering or writing anything, so no
    // descendant of the profiled binary was alive across either write.
    assert_eq!(
        manifest.get("namespace_drained").and_then(Value::as_bool),
        Some(true),
        "the helper could not empty its PID namespace before writing"
    );
    let descendants_reaped = manifest
        .get("descendants_reaped")
        .and_then(Value::as_u64)
        .ok_or("the helper manifest declared no descendants_reaped")?;
    cut.selections.push(selection(
        "profile-cpus-sampled",
        started,
        "passed",
        json!({
            "cpus_sampled": cpus_sampled,
            "guest_cpu_cores": cpu_cores,
            "namespace_drained": true,
            "descendants_reaped": descendants_reaped,
            "cpu_model": execution.hardware.cpu_model,
            "os_kernel": execution.hardware.os_kernel,
            "manifest_status": manifest.get("status"),
            "manifest_sha256": digest(&output.manifest),
        }),
    ));
    drop(execution);
    clean(gateway, "m5-03 after cpus-sampled")?;

    // -- selection 7: the zero-sample control --------------------------------
    let started = Instant::now();
    let zero = ProfileOptions::new("rust-mcp-profile-zero".into(), 99, 4)
        .map_err(|error| format!("options: {error:?}"))?;
    let observation = performance_port::profile(gateway, &source, &vendor, &zero, &Proceed)
        .map_err(|error| format!("zero-sample control: {error:?}"))?;
    assert_eq!(observation.build, ProfileBuildOutcome::Built);
    assert_eq!(observation.counters.samples_collected, 0);
    assert_eq!(observation.counters.samples_lost, 0);
    assert!(
        observation.stacks.is_empty(),
        "zero samples must publish an empty stacks artifact"
    );
    assert!(observation.top_frames.is_empty());
    assert_eq!(observation.completeness, ProfileCompleteness::NoSamples);
    assert_ne!(
        observation.status,
        ProfileStatus::ProfilerUnavailable,
        "zero samples is not a profiler denial"
    );
    assert_eq!(observation.perf_errno, None);
    assert!(observation.consistent());
    cut.selections.push(selection(
        "zero-sample-control",
        started,
        "passed",
        profile_counters(&observation),
    ));
    clean(gateway, "m5-03 after zero-sample control")?;

    // -- selection 8: cancellation during profiling --------------------------
    let started = Instant::now();
    let hold = ProfileOptions::new("rust-mcp-profile-hold".into(), 99, 20)
        .map_err(|error| format!("options: {error:?}"))?;
    let monitor = CancelWhenObserved::new(gateway, "/opt/perf/bin/rust-mcp-profile-helper");
    let cancelled = performance_port::profile(gateway, &source, &vendor, &hold, &monitor);
    assert_eq!(
        cancelled.err(),
        Some(SecurityError::Inspection(InspectionError::Project(
            ProjectError::Cancelled
        ))),
        "cancelling a profile must be reported as a cancellation"
    );
    assert!(
        monitor.observed.load(Ordering::SeqCst),
        "the profiling helper was never observed running: {}",
        monitor.evidence()
    );
    assert!(!monitor.failed.load(Ordering::SeqCst));
    let after = clean(gateway, "m5-03 after cancellation")?;
    cut.selections.push(selection(
        "cancellation-during-profiling",
        started,
        "passed",
        json!({"monitor": monitor.evidence(), "residue_after": after}),
    ));

    // -- selection 9: the denial control -------------------------------------
    //
    // ADR-074 §3's mandatory negative: `perf_event_open` refused, reported as
    // data with the errno that refused it, and never as a profile. The sampling
    // phase is moved to the committed quality profile — the only committed
    // mechanism that denies the syscall — and everything else is the product
    // path, port included. See [`DeniedPerfEventOpen`] for why this needs a
    // hook at all.
    let started = Instant::now();
    let denied = {
        let _seccomp = DeniedPerfEventOpen::arm();
        performance_port::profile(gateway, &source, &vendor, &options, &Proceed)
    };
    let observation = denied.map_err(|error| format!("denial control: {error:?}"))?;
    assert_eq!(observation.build, ProfileBuildOutcome::Built);
    assert_eq!(
        observation.status,
        ProfileStatus::ProfilerUnavailable,
        "a refused perf_event_open must be reported as a denial: {}",
        profile_counters(&observation)
    );
    assert_eq!(observation.completeness, ProfileCompleteness::Unavailable);
    assert_eq!(
        observation.perf_errno,
        Some(SECCOMP_EPERM),
        "the denial must carry the errno that produced it"
    );
    assert_eq!(observation.counters.samples_collected, 0);
    assert_eq!(observation.counters.stacks_written, 0);
    assert!(
        observation.stacks.is_empty(),
        "a denied profiler published stacks"
    );
    assert!(
        observation.svg.is_empty(),
        "a denied profiler published a graph"
    );
    assert!(observation.top_frames.is_empty());
    assert!(observation.consistent());
    // The application's own gate, which is what makes this the oracle: a denial
    // without its errno, or with a graph, is refused there.
    rust_engineering_application::profile::validate_profile_observation(
        &observation,
        &options,
        &vendor,
    )
    .map_err(|error| format!("the published denial did not validate: {error:?}"))?;
    let stderr = String::from_utf8_lossy(&observation.stderr).into_owned();
    assert!(
        stderr.contains("profiler unavailable"),
        "the helper did not announce the refusal: {stderr:?}"
    );

    // The helper's own exit code and manifest, which the DTO does not carry and
    // which `docs/validation/M5-03-profiling-native.json` declares for this
    // case.
    let execution = {
        let _seccomp = DeniedPerfEventOpen::arm();
        performance_gateway::execute_profile(
            gateway,
            &source,
            &vendor,
            &options,
            ExecutionLimits::new_job(300_000, LOG_BYTES).ok_or("limits")?,
            &Proceed,
        )
    }
    .map_err(|error| format!("denial manifest: {error:?}"))?;
    let output = execution.profile.as_ref().ok_or("no profile output")?;
    let run = output.run.as_ref().ok_or("the sampler never ran")?;
    assert_eq!(
        run.code,
        Some(3),
        "a refused perf_event_open must exit 3, not {:?}",
        run.code
    );
    let manifest: Value = serde_json::from_slice(&output.manifest)?;
    assert_eq!(
        manifest.get("status").and_then(Value::as_str),
        Some("profiler_unavailable")
    );
    assert_eq!(
        manifest.get("cpus_sampled").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        manifest.get("perf_errno").and_then(Value::as_i64),
        Some(i64::from(SECCOMP_EPERM))
    );
    assert!(
        output.stacks.is_empty(),
        "the denial path wrote a non-empty stacks artifact"
    );
    // The helper's own artifact-integrity claim: it emptied its PID namespace
    // before either file was written, so nothing of the workload could have
    // rewritten them. The port refuses a manifest that says otherwise.
    assert_eq!(
        manifest.get("namespace_drained").and_then(Value::as_bool),
        Some(true),
        "the helper could not empty its PID namespace before writing"
    );
    let mut denial = profile_counters(&observation);
    if let Some(object) = denial.as_object_mut() {
        object.insert("helper_exit".into(), json!(run.code));
        object.insert("helper_manifest".into(), manifest);
        object.insert("manifest_sha256".into(), json!(digest(&output.manifest)));
        object.insert(
            "seccomp".into(),
            json!("seccomp-rust-quality.json (perf_event_open not allowed)"),
        );
    }
    drop(execution);
    cut.selections
        .push(selection("denial-control", started, "passed", denial));
    clean(gateway, "m5-03 after denial control")?;

    // -- selection 10: the PID-namespace drain, observed reaping -------------
    //
    // Until this selection every profiling manifest on this tree reported
    // `descendants_reaped: 0`. `rust-mcp-profile-workload` is straight-line, so
    // its child was always already reaped when the drain ran: `kill(-1)`
    // reached an empty namespace and the reap loop returned on its first
    // `ECHILD`. The drain had therefore never been observed reaping anything,
    // and it is the last barrier between a surviving descendant and the two
    // artifacts the export phase tars — a descendant alive across `emit` could
    // rewrite both files after the helper created them.
    //
    // `rust-mcp-profile-fork` leaves a grandchild parented onto the helper
    // itself, so here the drain has work to do; and the run must still be
    // accepted, because an artifact written after a real reap is still the
    // helper's own.
    let started = Instant::now();
    let forking = ProfileOptions::new("rust-mcp-profile-fork".into(), 99, 4)
        .map_err(|error| format!("options: {error:?}"))?;
    let observation = performance_port::profile(gateway, &source, &vendor, &forking, &Proceed)
        .map_err(|error| format!("descendant drain: {error:?}"))?;
    assert_eq!(observation.build, ProfileBuildOutcome::Built);
    assert_eq!(
        observation.status,
        ProfileStatus::Complete,
        "counters: {}",
        profile_counters(&observation)
    );
    // The fixture returns non-zero from every path on which the descendant was
    // not created, so this is the profiled program's own account of the double
    // fork having happened at all.
    assert_eq!(
        observation.child.exit_code,
        Some(0),
        "the profiled program did not complete its double fork: {:?}",
        observation.child
    );
    assert_eq!(observation.child.signal, None);
    assert!(observation.counters.samples_collected > 0);
    assert_eq!(observation.perf_errno, None);
    assert!(observation.consistent());
    assert!(!observation.top_frames.is_empty());
    // The application's own gate over the published observation, the same one
    // the denial control uses.
    rust_engineering_application::profile::validate_profile_observation(
        &observation,
        &forking,
        &vendor,
    )
    .map_err(|error| format!("the drained observation did not validate: {error:?}"))?;
    let stacks = String::from_utf8(observation.stacks.clone())?;
    let (frames, hot, total) = hottest(&stacks)?;
    assert!(
        frames.iter().any(|frame| frame.contains("known_hot_frame")),
        "the hottest stack is not the fixture's own hot frame: {frames:?}"
    );
    // What the descendant writes if it ever reaches the artifact. Finding it
    // here would mean the file was rewritten after the helper published it.
    assert!(
        !stacks.contains("a descendant wrote this"),
        "a descendant reached the artifact the helper published"
    );

    // The drain's own two counters, which the DTO does not carry, plus the
    // reconciliation the port performs — recomputed here over the bytes of one
    // execution, so that the reap and the reconciliation are observed on the
    // same run rather than on two.
    let execution = performance_gateway::execute_profile(
        gateway,
        &source,
        &vendor,
        &forking,
        ExecutionLimits::new_job(300_000, LOG_BYTES).ok_or("limits")?,
        &Proceed,
    )
    .map_err(|error| format!("descendant drain manifest: {error:?}"))?;
    let output = execution.profile.as_ref().ok_or("no profile output")?;
    let run = output.run.as_ref().ok_or("the sampler never ran")?;
    assert_eq!(
        run.code,
        Some(0),
        "the helper did not profile the forking target: {:?}",
        run.code
    );
    let manifest: Value = serde_json::from_slice(&output.manifest)?;
    let descendants_reaped = manifest
        .get("descendants_reaped")
        .and_then(Value::as_u64)
        .ok_or("the helper manifest declared no descendants_reaped")?;
    assert!(
        descendants_reaped > 0,
        "the profiled program left a descendant behind and the drain reaped nothing: {manifest}"
    );
    assert_eq!(
        manifest.get("namespace_drained").and_then(Value::as_bool),
        Some(true),
        "the helper could not empty its PID namespace before writing"
    );
    assert_eq!(
        manifest.get("child_exit_code").and_then(Value::as_i64),
        Some(0),
        "the profiled program did not complete its double fork: {manifest}"
    );
    // `manifest_matches_the_artifact`, recomputed: one line per distinct stack
    // and one sample per unit of count, with no rounding on either side.
    let drained_stacks = String::from_utf8(output.stacks.clone())?;
    let (_, _, artifact_samples) = hottest(&drained_stacks)?;
    let artifact_stacks = drained_stacks
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(
        manifest.get("stacks_written").and_then(Value::as_u64),
        u64::try_from(artifact_stacks).ok(),
        "the manifest and the artifact disagree on how many stacks were written"
    );
    assert_eq!(
        manifest.get("samples_collected").and_then(Value::as_u64),
        Some(artifact_samples),
        "the manifest and the artifact disagree on how many samples were collected"
    );
    assert!(
        !drained_stacks.contains("a descendant wrote this"),
        "a descendant reached the artifact the helper published"
    );
    let mut drained = profile_counters(&observation);
    if let Some(object) = drained.as_object_mut() {
        object.insert(
            "hottest_stack".into(),
            json!({"frames": frames, "samples": hot, "total_samples": total}),
        );
        object.insert("helper_exit".into(), json!(run.code));
        object.insert("descendants_reaped".into(), json!(descendants_reaped));
        object.insert("namespace_drained".into(), json!(true));
        object.insert(
            "reconciled_artifact".into(),
            json!({
                "distinct_stacks": artifact_stacks,
                "total_samples": artifact_samples,
            }),
        );
        object.insert("helper_manifest".into(), manifest);
        object.insert("manifest_sha256".into(), json!(digest(&output.manifest)));
    }
    drop(execution);
    cut.selections.push(selection(
        "profile-descendant-drained",
        started,
        "passed",
        drained,
    ));
    clean(gateway, "m5-03 after descendant drain")?;

    // -- selection 11: a descendant that got to the artifact first ------------
    //
    // The other half of the containment claim, and the one the `O_EXCL` in the
    // helper's `emit` exists for. Here the grandchild creates
    // `/profile/stacks.txt` before the profiled program starts its workload, so
    // the helper opens its own output onto a path it did not write. It refuses
    // to overwrite it, reports an internal failure rather than a denial and
    // exits 4; the gateway exports nothing on that exit, so the host has no
    // manifest to vouch for and refuses the operation instead of publishing an
    // artifact somebody else authored.
    //
    // The ordering is structural rather than a race: the profiled program does
    // not start its budget until the file is on the volume, and the helper does
    // not write until that program exits or its sampling window closes.
    let started = Instant::now();
    let precreated = ProfileOptions::new("rust-mcp-profile-precreate".into(), 99, 4)
        .map_err(|error| format!("options: {error:?}"))?;
    let refused = performance_port::profile(gateway, &source, &vendor, &precreated, &Proceed);
    assert_eq!(
        refused.err(),
        Some(SecurityError::InvalidMetadata),
        "an artifact a descendant created first must refuse the run, not be published"
    );
    let execution = performance_gateway::execute_profile(
        gateway,
        &source,
        &vendor,
        &precreated,
        ExecutionLimits::new_job(300_000, LOG_BYTES).ok_or("limits")?,
        &Proceed,
    )
    .map_err(|error| format!("pre-created artifact: {error:?}"))?;
    let output = execution.profile.as_ref().ok_or("no profile output")?;
    assert_eq!(
        output.build.code,
        Some(0),
        "the pre-creating target did not build"
    );
    let run = output.run.as_ref().ok_or("the sampler never ran")?;
    assert_eq!(
        run.code,
        Some(4),
        "a pre-created output must be an internal failure, not {:?}",
        run.code
    );
    assert!(
        output.stacks.is_empty(),
        "the gateway exported a stacks artifact from a refused run"
    );
    assert!(
        output.manifest.is_empty(),
        "the gateway exported a manifest from a refused run"
    );
    let stderr = String::from_utf8_lossy(&run.stderr).into_owned();
    assert!(
        stderr.contains("the profile outputs could not be written"),
        "the helper did not announce the refusal: {stderr:?}"
    );
    let helper_exit = run.code;
    let stderr_bytes = run.stderr.len();
    drop(execution);
    cut.selections.push(selection(
        "profile-precreated-artifact-refused",
        started,
        "passed",
        json!({
            "port_error": "InvalidMetadata",
            "helper_exit": helper_exit,
            "exported_stacks_bytes": 0,
            "exported_manifest_bytes": 0,
            "helper_stderr_bytes": stderr_bytes,
            "helper_stderr": stderr,
        }),
    ));
    clean(gateway, "m5-03 after pre-created artifact")?;

    cut.residue_after = clean(gateway, "m5-03 after")?;
    publish(&cut, &image)?;
    Ok(())
}

// -- M5-04 rust.binary.bloat -------------------------------------------------

#[test]
#[ignore = "explicit M5 image, host Docker and exclusive native bloat qualification"]
fn m5_binary_bloat_is_qualified_natively() -> Result<(), Failure> {
    let image = m5_image()?;
    let mut cut = Cut::open("m5-04-bloat");
    let session = Session::open(&image)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "m5-04 before")?;

    let source = fixture_bundle("bloat")?;
    let vendor = empty_vendor()?;
    cut.fixtures
        .insert("bloat".into(), bundle_facts("bloat", &source)?);

    let mut sizes = BTreeMap::new();
    for (name, profile) in [
        ("release-positive", BloatProfile::Release),
        ("release-lto", BloatProfile::ReleaseLto),
    ] {
        let started = Instant::now();
        let options = BloatOptions::new("rust-mcp-bloat-fixture".into(), None, profile)
            .map_err(|error| format!("options: {error:?}"))?;
        let observation = performance_port::bloat(gateway, &source, &vendor, &options, &Proceed)
            .map_err(|error| format!("{name}: {error:?}"))?;
        assert_eq!(observation.exit, BloatExit::Passed, "{name}");
        assert_eq!(observation.exit_code, Some(0), "{name}");
        assert_eq!(observation.analyzer_version, APPROVED_CARGO_BLOAT_VERSION);
        let measured = observation
            .measured
            .as_ref()
            .ok_or_else(|| format!("{name}: the product measured no binary"))?;
        let attribution = observation
            .attribution
            .as_ref()
            .ok_or_else(|| format!("{name}: the analyzer attributed nothing"))?;
        assert_eq!(
            attribution.reported_file_size_bytes,
            Some(measured.size_bytes),
            "{name}: the analyzer's file-size must equal the product's own measurement"
        );
        assert!(measured.analysis_build_symbols_forced, "{name}");
        assert_eq!(measured.format, BinaryFormat::Elf64Aarch64, "{name}");
        assert!(attribution.estimated, "{name}");
        assert!(!attribution.functions.is_empty(), "{name}");
        for expected in ["rust_mcp_bloat_fixture", "bloat_inner"] {
            assert!(
                attribution
                    .crates
                    .iter()
                    .any(|entry| entry.name == expected),
                "{name}: {expected} is absent from the per-crate attribution"
            );
        }
        // The fixture links 634 attributable functions and the product's ranking
        // is bounded at 256, so this binary is exactly the case ADR-079 §1
        // separates: the cap acted, the report says how many rows it dropped,
        // and the measurement is still valid. Completeness is validity only, so
        // it is `Complete`; what must be exact is the file, and that is asserted
        // above -- the product's own measurement equals the analyzer's reported
        // size, byte for byte. Before ADR-079 this asserted `Truncated`, which
        // is what made the tool's success path unreachable for any binary that
        // links `std`.
        assert_eq!(
            observation.completeness,
            BloatCompleteness::Complete,
            "{name}: {}",
            bloat_counters(&observation)
        );
        assert!(
            attribution.functions_omitted_by_row_cap > 0,
            "{name}: a capped ranking must say how many rows the cap dropped"
        );
        assert!(
            observation.analysis_validated(),
            "{name}: a capped ranking over an exactly measured file is a validated analysis"
        );
        assert_eq!(
            attribution.reported_file_size_bytes,
            Some(measured.size_bytes),
            "{name}: the analyzer and the product must name the same file"
        );
        assert!(observation.consistent(), "{name}");
        sizes.insert(name, measured.size_bytes);
        cut.selections.push(selection(
            name,
            started,
            "passed",
            bloat_counters(&observation),
        ));
        clean(gateway, &format!("m5-04 after {name}"))?;
    }
    let release = *sizes.get("release-positive").ok_or("release size")?;
    let lto = *sizes.get("release-lto").ok_or("lto size")?;
    assert!(
        lto < release,
        "link-time optimization must produce a strictly smaller binary: {lto} vs {release}"
    );

    // -- selection 11: a missing target is an observed failure ---------------
    let started = Instant::now();
    let options = BloatOptions::new("rust-mcp-bloat-absent".into(), None, BloatProfile::Release)
        .map_err(|error| format!("options: {error:?}"))?;
    let observation = performance_port::bloat(gateway, &source, &vendor, &options, &Proceed)
        .map_err(|error| format!("missing-target: {error:?}"))?;
    assert_eq!(
        observation.exit,
        BloatExit::AnalysisFailed,
        "a missing binary target is an observed analysis failure, not infrastructure"
    );
    assert_eq!(observation.exit_code, Some(1));
    assert_eq!(observation.measured, None);
    assert_eq!(observation.attribution, None);
    assert_eq!(observation.completeness, BloatCompleteness::Unavailable);
    assert!(observation.consistent());
    cut.selections.push(selection(
        "missing-binary-target",
        started,
        "passed",
        bloat_counters(&observation),
    ));

    cut.residue_after = clean(gateway, "m5-04 after")?;
    publish(&cut, &image)?;
    Ok(())
}

// -- image admission ---------------------------------------------------------

#[test]
#[ignore = "explicit M4 image and host Docker; M5 admission refusal"]
fn m5_tools_refuse_every_runtime_but_the_qualified_one() -> Result<(), Failure> {
    // Named only so a mis-set environment cannot silently skip this cut.
    m5_image()?;
    let mut cut = Cut::open("m5-00-admission");
    let session = Session::open(crate::APPROVED_M4_IMAGE)?;
    let gateway = session.gateway()?;
    cut.residue_before = clean(gateway, "admission before")?;
    assert_ne!(gateway.image_id(), crate::APPROVED_M5_IMAGE);

    let source = fixture_bundle("bloat")?;
    let vendor = empty_vendor()?;
    let unavailable = SecurityError::Inspection(InspectionError::Execution(
        rust_engineering_application::ExecutionError::Unavailable,
    ));

    let started = Instant::now();
    let options = BenchmarkRunOptions::new(None, None, Vec::new(), false, false, 1, 300)
        .map_err(|error| format!("options: {error:?}"))?;
    assert_eq!(
        performance_port::benchmark(gateway, &source, &vendor, &options, &Proceed).err(),
        Some(unavailable)
    );
    assert_eq!(
        performance_port::profile(
            gateway,
            &source,
            &vendor,
            &ProfileOptions::new("rust-mcp-bloat-fixture".into(), 99, 4)
                .map_err(|error| format!("{error:?}"))?,
            &Proceed,
        )
        .err(),
        Some(unavailable)
    );
    assert_eq!(
        performance_port::bloat(
            gateway,
            &source,
            &vendor,
            &BloatOptions::new("rust-mcp-bloat-fixture".into(), None, BloatProfile::Release)
                .map_err(|error| format!("{error:?}"))?,
            &Proceed,
        )
        .err(),
        Some(unavailable)
    );
    let after = clean(gateway, "admission after")?;
    cut.selections.push(selection(
        "unqualified-image-refused",
        started,
        "passed",
        json!({
            "image_id": gateway.image_id(),
            "qualified_image_id": crate::APPROVED_M5_IMAGE,
            "error": "Inspection(Execution(Unavailable))",
            "residue_after": after,
        }),
    ));
    cut.residue_after = after;
    publish(&cut, crate::APPROVED_M4_IMAGE)?;
    Ok(())
}

// -- unit checks that need no Docker -----------------------------------------

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
    fn the_hottest_stack_is_the_one_with_the_most_samples() -> Result<(), Failure> {
        let (frames, hot, total) = hottest("a;b 3\na;c;d 11\n")?;
        assert_eq!(frames, ["a", "c", "d"]);
        assert_eq!(hot, 11);
        assert_eq!(total, 14);
        assert!(hottest("").is_err());
        Ok(())
    }

    #[test]
    fn a_vendor_tree_over_any_source_bound_never_fits() {
        let shape = VendorShape {
            files: 10,
            directories: 2,
            total_bytes: 1024,
            oversized: Vec::new(),
            materialized: true,
        };
        assert!(shape.fits());
        assert!(
            !VendorShape {
                materialized: false,
                ..shape_of(&shape)
            }
            .fits()
        );
        assert!(
            !VendorShape {
                files: SOURCE_MAX_ENTRIES,
                ..shape_of(&shape)
            }
            .fits()
        );
        assert!(
            !VendorShape {
                total_bytes: SOURCE_MAX_TOTAL_BYTES as u64 + 1,
                ..shape_of(&shape)
            }
            .fits()
        );
        assert!(
            !VendorShape {
                oversized: vec!["big".into()],
                ..shape_of(&shape)
            }
            .fits()
        );
    }

    fn shape_of(shape: &VendorShape) -> VendorShape {
        VendorShape {
            files: shape.files,
            directories: shape.directories,
            total_bytes: shape.total_bytes,
            oversized: shape.oversized.clone(),
            materialized: shape.materialized,
        }
    }
}
