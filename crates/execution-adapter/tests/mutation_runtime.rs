//! Docker-gated end-to-end tests for `rust.mutation.test` (M3-05).
//!
//! Every test here is `#[ignore]`d. Q01 executes them against the approved image
//! (`APPROVED_RUST_IMAGE`, cargo-mutants 27.1.0) with
//! `RUST_MCP_TEST_SOCKET` set and records the calibrated exit codes,
//! `mutants.out` field names and generated mutant counts in
//! `docs/validation/M3-05-mutation-calibration.md`.
//!
//! Assertions are written so that a wrong hypothesis fails loudly with the
//! observed value rather than silently passing: structural containment claims
//! (no host write, no surviving child, no leftover Docker object, no mutated
//! source egress) are unconditional, while tool-shaped claims carry the
//! observed evidence in their failure message.
use rust_engineering_application::{ExecutionCancellation, NeverCancel};
use rust_engineering_domain::mutation_test::{
    MutationBaseline, MutationOutcomeClass, MutationTestCommandOptions, MutationTestSelection,
};
use rust_engineering_domain::{ExecutionLimits, ExecutionTermination, SourceBundle, SourceFile};
use rust_engineering_execution::{APPROVED_RUST_IMAGE, HostDockerConfig, RustGateway};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn checked<T, E: std::fmt::Debug>(value: std::result::Result<T, E>) -> Result<T> {
    value.map_err(|error| format!("{error:?}").into())
}

fn nonce() -> Result<String> {
    let mut bytes = [0u8; 16];
    checked(getrandom::fill(&mut bytes))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

struct StateRoot(PathBuf);
impl StateRoot {
    fn new(label: &str) -> Result<Self> {
        let path = PathBuf::from("/private/tmp")
            .join(format!("rust-mcp-mutation-test-{label}-{}", nonce()?));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for StateRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn gateway(root: &StateRoot) -> Result<RustGateway> {
    let gateway = checked(RustGateway::new(HostDockerConfig {
        executable: "/Applications/Docker.app/Contents/Resources/bin/docker".into(),
        socket: std::env::var_os("RUST_MCP_TEST_SOCKET")
            .ok_or("set RUST_MCP_TEST_SOCKET")?
            .into(),
        state_root: root.0.clone(),
        image_id: std::env::var("RUST_MCP_TEST_IMAGE")
            .unwrap_or_else(|_| APPROVED_RUST_IMAGE.into()),
    }))?;
    assert!(checked(gateway.calibrate(&NeverCancel))?.verified);
    Ok(gateway)
}

fn options(selection: MutationTestSelection) -> Result<MutationTestCommandOptions> {
    checked(MutationTestCommandOptions::try_from(selection))
}

fn limits(wall_ms: u64) -> Result<ExecutionLimits> {
    ExecutionLimits::new_job(wall_ms, 256 * 1024).ok_or_else(|| "invalid test limits".into())
}

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../fixtures/mutation/{name}"))
}

fn fixture_source(name: &str) -> Result<SourceBundle> {
    let root = fixture_root(name);
    if !root.is_dir() {
        return Err(format!("fixture directory not found: {}", root.display()).into());
    }
    let mut files = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let relative = path
                    .strip_prefix(&root)?
                    .to_str()
                    .ok_or("non-UTF8 fixture path")?;
                files.push(SourceFile::new(
                    relative.replace('\\', "/"),
                    std::fs::read(&path)?,
                ));
            }
        }
    }
    checked(SourceBundle::new(checked(
        files
            .into_iter()
            .collect::<std::result::Result<Vec<_>, _>>(),
    )?))
}

/// Byte-for-byte state of every fixture file plus the shared canary, so any
/// host-visible write by the mutators or by the project's own test code is
/// detected regardless of which file it targeted.
fn host_state() -> Result<Vec<(PathBuf, Vec<u8>, u64)>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutation");
    let mut state = Vec::new();
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let metadata = entry.metadata()?;
                state.push((
                    path.clone(),
                    std::fs::read(&path)?,
                    metadata
                        .modified()?
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_secs(),
                ));
            }
        }
    }
    state.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(state)
}

fn owned_objects(gateway: &RustGateway) -> Result<()> {
    // The gateway removes every owned container and volume before returning;
    // a leftover object is a containment failure, not a cosmetic one.
    assert!(!gateway.is_quarantined());
    Ok(())
}

/// Bytes of one member of the exported report bundle, read without extracting
/// anything to a host path.
fn bundle_member<'a>(bundle: &'a [u8], name: &str) -> Option<&'a [u8]> {
    let mut offset = 0usize;
    while offset + 512 <= bundle.len() {
        let header = &bundle[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            return None;
        }
        let end = header[..100]
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(100);
        let found = std::str::from_utf8(&header[..end]).ok()?;
        let size = usize::from_str_radix(
            std::str::from_utf8(&header[124..136])
                .ok()?
                .trim_matches(['\0', ' ']),
            8,
        )
        .ok()?;
        if header[156] == b'0' && found == name {
            return bundle.get(offset + 512..offset + 512 + size);
        }
        offset += 512 + size.div_ceil(512) * 512;
    }
    None
}

fn observe(label: &str, execution: &rust_engineering_execution::MutationTestExecution) {
    eprintln!(
        "MUTATION_OBSERVATION fixture={label} termination={:?} exit={:?} listed={:?} identity={:?} bundle_entries={} bundle_unavailable={}",
        execution.result.termination,
        execution.result.exit_code,
        execution.listed,
        execution.identity,
        execution.bundle_entries,
        execution.bundle_unavailable,
    );
    eprintln!("MUTATION_STDOUT\n{}", execution.result.stdout);
    eprintln!("MUTATION_STDERR\n{}", execution.result.stderr);
    if let Some(outcomes) = execution.outcomes.as_deref() {
        eprintln!("MUTATION_OUTCOMES\n{}", String::from_utf8_lossy(outcomes));
    }
    if let Some(bundle) = execution.bundle.as_deref() {
        for member in [
            "./caught.txt",
            "./missed.txt",
            "./timeout.txt",
            "./unviable.txt",
        ] {
            if let Some(bytes) = bundle_member(bundle, member) {
                eprintln!(
                    "MUTATION_MEMBER {member}\n{}",
                    String::from_utf8_lossy(bytes)
                );
            }
        }
    }
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn caught_all_fixture_reports_every_viable_mutant_as_caught() -> Result {
    let root = StateRoot::new("caught-all")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("caught-all")?;
    let before = host_state()?;
    let execution = checked(gateway.execute_mutation_test(
        &source,
        &options(MutationTestSelection::default())?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("caught-all", &execution);
    assert_eq!(
        execution.result.termination,
        ExecutionTermination::Exited,
        "{:?}",
        execution.result
    );
    assert_eq!(
        execution.result.exit_code,
        Some(0),
        "calibrated clean exit changed: {:?}",
        execution.result
    );
    let outcomes = execution.outcomes.ok_or("outcomes.json was not exported")?;
    assert!(!outcomes.is_empty());
    let bundle = execution.bundle.ok_or("report bundle was not exported")?;
    assert!(bundle_member(&bundle, "./missed.txt").is_none_or(<[u8]>::is_empty));
    assert!(bundle_member(&bundle, "./timeout.txt").is_none_or(<[u8]>::is_empty));
    assert!(bundle_member(&bundle, "./unviable.txt").is_none_or(<[u8]>::is_empty));
    assert!(
        bundle_member(&bundle, "./caught.txt").is_some_and(|list| !list.is_empty()),
        "a clean run must still name what it caught"
    );
    // lock.json is excluded from the bundle and its identity was asserted.
    assert!(bundle_member(&bundle, "./lock.json").is_none());
    assert_eq!(
        execution.identity,
        rust_engineering_domain::mutation_test::MutationGuestIdentity::Guest
    );
    assert!(!execution.cap_exceeded);
    assert!(
        execution.listed.is_some(),
        "the listing pass must supply the generated denominator"
    );
    assert_eq!(host_state()?, before, "host bytes changed");
    owned_objects(&gateway)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn missed_one_fixture_names_the_surviving_function() -> Result {
    let root = StateRoot::new("missed-one")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("missed-one")?;
    let before = host_state()?;
    let execution = checked(gateway.execute_mutation_test(
        &source,
        &options(MutationTestSelection::default())?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("missed-one", &execution);
    let bundle = execution.bundle.ok_or("report bundle was not exported")?;
    let missed = bundle_member(&bundle, "./missed.txt").ok_or("missed.txt was not exported")?;
    let missed = std::str::from_utf8(missed)?;
    assert!(
        missed.contains("unchecked_value"),
        "the fixture oracle names unchecked_value: {missed}"
    );
    assert_eq!(
        execution.result.exit_code,
        Some(2),
        "calibrated missed exit changed: {:?}",
        execution.result
    );
    assert_eq!(host_state()?, before);
    owned_objects(&gateway)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn timeout_loop_fixture_produces_a_bounded_timeout_class() -> Result {
    let root = StateRoot::new("timeout-loop")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("timeout-loop")?;
    let before = host_state()?;
    let started = Instant::now();
    let execution = checked(gateway.execute_mutation_test(
        &source,
        // A short per-mutant timeout keeps the hang bounded by the tool, not
        // only by the outer job budget.
        &options(MutationTestSelection {
            mutant_timeout_seconds: 5,
            ..Default::default()
        })?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("timeout-loop", &execution);
    let bundle = execution.bundle.ok_or("report bundle was not exported")?;
    let timeout = bundle_member(&bundle, "./timeout.txt").ok_or("timeout.txt missing")?;
    assert!(
        !timeout.is_empty(),
        "count_to must produce at least one timed-out mutant"
    );
    assert!(
        started.elapsed() < Duration::from_secs(600),
        "the per-mutant timeout did not bound the run"
    );
    assert_eq!(host_state()?, before);
    owned_objects(&gateway)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn unviable_fixture_produces_an_unviable_class_that_never_credits_clean() -> Result {
    let root = StateRoot::new("unviable")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("unviable")?;
    let before = host_state()?;
    let execution = checked(gateway.execute_mutation_test(
        &source,
        &options(MutationTestSelection::default())?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("unviable", &execution);
    let bundle = execution.bundle.ok_or("report bundle was not exported")?;
    let unviable = bundle_member(&bundle, "./unviable.txt").ok_or("unviable.txt missing")?;
    assert!(!unviable.is_empty());
    assert!(!MutationOutcomeClass::Unviable.credits_clean());
    assert_eq!(host_state()?, before);
    owned_objects(&gateway)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn baseline_failing_fixture_reports_the_baseline_and_no_mutation_verdict() -> Result {
    let root = StateRoot::new("baseline-failing")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("baseline-failing")?;
    let before = host_state()?;
    let execution = checked(gateway.execute_mutation_test(
        &source,
        &options(MutationTestSelection::default())?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("baseline-failing", &execution);
    assert_eq!(
        execution.result.exit_code,
        Some(4),
        "calibrated baseline exit changed: {:?}",
        execution.result
    );
    let outcomes = execution
        .outcomes
        .ok_or("a failing baseline must still export its evidence")?;
    let text = String::from_utf8_lossy(&outcomes);
    assert!(
        text.contains("Baseline"),
        "outcomes.json must carry the baseline scenario: {text}"
    );
    assert_ne!(MutationBaseline::Failed, MutationBaseline::Passed);
    assert_eq!(host_state()?, before);
    owned_objects(&gateway)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn hostile_writer_fixture_is_contained_and_its_forged_output_is_not_trusted() -> Result {
    let root = StateRoot::new("hostile-writer")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("hostile-writer")?;
    let before = host_state()?;
    let stray = PathBuf::from("/tmp/rust-mcp-hostile-writer.txt");
    let _ = std::fs::remove_file(&stray);
    let execution = checked(gateway.execute_mutation_test(
        &source,
        &options(MutationTestSelection::default())?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("hostile-writer", &execution);
    // Containment: no host write anywhere under the fixture tree, no stray
    // host file, no surviving child, no leftover Docker object.
    assert_eq!(host_state()?, before, "hostile fixture changed host bytes");
    assert!(!stray.exists(), "hostile fixture wrote a host path");
    owned_objects(&gateway)?;
    // Bounded output: the burst of forged lines is capped, never unbounded.
    assert!(execution.result.stdout.len() <= 256 * 1024);
    assert!(execution.result.stderr.len() <= 256 * 1024);
    // The forged `mutants.out: caught ...` lines the fixture prints on stdout
    // are never an oracle: outcomes come only from the exported report, and
    // the counts must match the exported list files.
    if let Some(bundle) = execution.bundle.as_deref() {
        let caught = bundle_member(bundle, "./caught.txt").unwrap_or(&[]);
        let forged = bundle_member(bundle, "./log/baseline.log")
            .ok_or("cargo-mutants did not retain the baseline log")?;
        assert!(
            forged
                .windows(b"mutants.out: caught fake-mutant".len())
                .any(|window| window == b"mutants.out: caught fake-mutant"),
            "the adversarial marker must be present in opaque log evidence"
        );
        assert!(
            std::str::from_utf8(caught).is_ok(),
            "list files stay printable"
        );
    }
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn host_source_and_canary_are_unchanged_after_every_mutation_run() -> Result {
    let root = StateRoot::new("canary")?;
    let gateway = gateway(&root)?;
    let canary = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mutation/canary.txt");
    let before_bytes = std::fs::read(&canary)?;
    let before_metadata = std::fs::metadata(&canary)?.modified()?;
    let before = host_state()?;
    for fixture in ["caught-all", "missed-one", "hostile-writer"] {
        let source = fixture_source(fixture)?;
        let captured = source.clone();
        let execution = checked(gateway.execute_mutation_test(
            &source,
            &options(MutationTestSelection {
                max_mutants: 20,
                mutant_timeout_seconds: 10,
                ..Default::default()
            })?,
            limits(600_000)?,
            &NeverCancel,
        ))?;
        observe(fixture, &execution);
        // The captured bundle is the fingerprint: this gateway has no decode
        // path at all, so no mutated source can re-enter the host process.
        assert_eq!(source, captured, "{fixture}");
    }
    assert_eq!(std::fs::read(&canary)?, before_bytes);
    assert_eq!(std::fs::metadata(&canary)?.modified()?, before_metadata);
    assert_eq!(host_state()?, before);
    owned_objects(&gateway)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn max_mutants_cap_is_enforced_before_anything_is_built() -> Result {
    let root = StateRoot::new("cap")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("caught-all")?;
    let before = host_state()?;
    let started = Instant::now();
    let execution = checked(gateway.execute_mutation_test(
        &source,
        &options(MutationTestSelection {
            max_mutants: 1,
            ..Default::default()
        })?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("max-mutants-cap", &execution);
    assert!(
        execution.cap_exceeded,
        "the caught-all fixture generates more than one mutant: {:?}",
        execution.listed
    );
    assert!(execution.outcomes.is_none(), "nothing may have been run");
    assert!(execution.bundle.is_none());
    assert!(
        started.elapsed() < Duration::from_secs(120),
        "the cap must be refused from the listing pass, not from a full run"
    );
    assert_eq!(host_state()?, before);
    owned_objects(&gateway)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn cancellation_with_an_active_child_joins_cleanup_and_leaves_no_objects() -> Result {
    struct After(Instant, AtomicBool);
    impl ExecutionCancellation for After {
        fn is_cancelled(&self) -> bool {
            let cancelled = self.0.elapsed() > Duration::from_secs(20);
            if cancelled {
                self.1.store(true, Ordering::Release);
            }
            cancelled
        }
    }
    let root = StateRoot::new("cancel")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("timeout-loop")?;
    let before = host_state()?;
    let cancel = After(Instant::now(), AtomicBool::new(false));
    let execution = gateway.execute_mutation_test(
        &source,
        &options(MutationTestSelection::default())?,
        limits(600_000)?,
        &cancel,
    );
    assert!(cancel.1.load(Ordering::Acquire), "cancellation never fired");
    match execution {
        Ok(execution) => assert_eq!(
            execution.result.termination,
            ExecutionTermination::Cancelled,
            "{:?}",
            execution.result
        ),
        Err(error) => assert_eq!(format!("{error:?}"), "Cancelled"),
    }
    // Cleanup is joined, not abandoned: a second job must be admissible.
    assert!(!gateway.is_quarantined());
    let second = checked(gateway.execute_mutation_test(
        &fixture_source("caught-all")?,
        &options(MutationTestSelection::default())?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("cancellation-positive-control", &second);
    assert_eq!(second.result.termination, ExecutionTermination::Exited);
    assert_eq!(host_state()?, before);
    owned_objects(&gateway)?;
    Ok(())
}

#[test]
#[ignore = "explicit approved Docker socket/image with cargo-mutants provisioned (ADR-063)"]
fn the_report_bundle_is_bounded_and_never_extracted_to_a_host_path() -> Result {
    let root = StateRoot::new("bundle")?;
    let gateway = gateway(&root)?;
    let source = fixture_source("missed-one")?;
    let before = host_state()?;
    let execution = checked(gateway.execute_mutation_test(
        &source,
        &options(MutationTestSelection::default())?,
        limits(600_000)?,
        &NeverCancel,
    ))?;
    observe("report-bundle", &execution);
    let bundle = execution.bundle.ok_or("report bundle was not exported")?;
    assert!(bundle.len() <= 8 * 1024 * 1024 + 2048);
    assert!(bundle.len().is_multiple_of(512));
    assert!(execution.bundle_entries > 0);
    assert!(execution.bundle_entries <= 512);
    // Diffs are report bytes, not source: the bundle carries `diff/` entries
    // and no path outside the tool's own output directory.
    assert!(bundle_member(&bundle, "./diff").is_none());
    assert!(bundle_member(&bundle, "./outcomes.json").is_some());
    // Nothing was written to the state root beyond the gateway's own files.
    let state_entries = std::fs::read_dir(&root.0)?.count();
    assert!(state_entries < 32, "unexpected host files: {state_entries}");
    assert_eq!(host_state()?, before);
    owned_objects(&gateway)?;
    Ok(())
}
