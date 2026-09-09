//! Adapters from the M5 application ports to the closed performance gateway.
//!
//! Everything here is mapping: the gateway observes, this module names what it
//! observed in the domain's vocabulary. Nothing is inferred, and a field the
//! runtime could not observe stays `None` (ADR-073 §3).
//!
//! The three `impl` blocks that reach these functions belong on
//! `RustProjectInspector`, whose private `with_gateway` lives in
//! `project_inspection.rs`; that file is a separate, concurrent deliverable, so
//! the crate has no non-test caller for this module yet. Each function is
//! already shaped to its trait —
//! `rust_engineering_application::benchmark::ProjectBenchmarkPort`,
//! `::profile::ProjectProfilePort` and `::bloat::ProjectBloatPort` — so wiring
//! is one `self.with_gateway(control, |gateway| …)` per tool.
use crate::RustGateway;
use crate::bloat_json;
use crate::criterion_dataset;
use crate::performance_gateway::{
    self, APPLIED_CPU_MILLICORES, APPLIED_MEMORY_BYTES, APPLIED_PIDS, BENCHMARK_BUDGET_MS,
    BENCHMARK_MEASUREMENT_MS, BENCHMARK_SAMPLE_SIZE, BENCHMARK_WARM_UP_MS, BLOAT_BUDGET_MS,
    BloatOutput, HardwareProbe, PROFILE_BUDGET_MS, PROFILE_MAX_SAMPLING_MS, PerformanceError,
    PerformanceExecution, PerformanceKind, ProfileOutput,
};
use crate::profile_stacks::{self, FoldedProfile};
use crate::profile_svg::{self, SvgOptions};
use rust_engineering_application::benchmark::BenchmarkRunOptions;
use rust_engineering_application::security::SecurityError;
use rust_engineering_application::{ExecutionError, InspectionControl, InspectionError};
use rust_engineering_domain::benchmark::{
    APPROVED_CRITERION_VERSION, BENCHMARK_MAX_SAMPLES, BenchmarkDataset, BenchmarkHarness,
    BenchmarkIdentity, BenchmarkMeasurement, BenchmarkProvenance, BenchmarkSelection,
    HardwareProfile, MeasurementCompleteness, RawSample, ResourceQuotas, SampleUnit, SamplingMode,
    Virtualization,
};
use rust_engineering_domain::benchmark_run::{
    BenchmarkExit, BenchmarkObservation, DatasetOmission, HarnessDetection,
};
use rust_engineering_domain::bloat::{
    APPROVED_CARGO_BLOAT_VERSION, BinaryFormat, BloatAttribution, BloatCompleteness, BloatExit,
    BloatObservation, BloatOptions, MeasuredBinary,
};
use rust_engineering_domain::profile::{
    PROFILE_BACKEND, PROFILE_MAX_DEPTH, ProfileBuildOutcome, ProfileChild, ProfileCompleteness,
    ProfileCounters, ProfileFrameWeight, ProfileObservation, ProfileOptions, ProfileStatus,
};
use rust_engineering_domain::{
    CargoVendorSnapshot, ExecutionLimits, ExecutionTermination, RuntimeIdentity, SourceBundle,
};
use serde::Deserialize;
use std::collections::BTreeMap;

/// The M5 runtime qualified by ADR-075: the M4 image plus exactly
/// `/opt/perf/bin/cargo-bloat` and `/opt/perf/bin/rust-mcp-profile-helper`.
/// No other image may execute a performance measurement, because the analyzer
/// and helper versions this module reports are properties of this digest.
pub const M5_IMAGE: &str =
    "sha256:0e21c561488cb917e89e42943eb5138a7ddfd73d9de2f9cd4b9a0b516bdab820";

/// ADR-076 §5: the ranking is bounded, and the bound belongs to the product.
const TOP_FRAMES: usize = 64;
const LOG_BYTES: usize = 512 * 1024;
const PLATFORM: &str = "linux/aarch64";

/// The gateway's vocabulary, named in the one the application ports speak.
///
/// `InvalidOptions` is deliberately internal: every option reaching this module
/// was already validated by its application type, so an option the gateway
/// still refuses is a defect here, not a caller error.
/// `ProjectCargoConfiguration` is a refusal of the containment, which is what
/// `SandboxDenied` means, and what the application maps to a blocked result.
fn error(value: PerformanceError) -> SecurityError {
    match value {
        PerformanceError::Inspection(inner) => SecurityError::Inspection(inner),
        PerformanceError::Timeout => SecurityError::Timeout,
        PerformanceError::OutputLimit => SecurityError::OutputLimit,
        PerformanceError::InvalidMetadata => SecurityError::InvalidMetadata,
        PerformanceError::MissingOfflineData => SecurityError::MissingOfflineData,
        PerformanceError::InvalidOptions => SecurityError::Inspection(InspectionError::Internal),
        PerformanceError::ProjectCargoConfiguration => SecurityError::Inspection(
            InspectionError::Project(rust_engineering_application::ProjectError::Rejected(
                rust_engineering_domain::OperationalErrorCode::SandboxDenied,
            )),
        ),
    }
}

/// The gateway answered about the operation this port asked for. Cheap, and it
/// keeps a future refactor from returning one tool's evidence to another.
fn observed(execution: &PerformanceExecution, kind: PerformanceKind) -> Result<(), SecurityError> {
    if execution.kind == kind {
        Ok(())
    } else {
        Err(SecurityError::InvalidMetadata)
    }
}

fn admitted(gateway: &RustGateway) -> Result<(), SecurityError> {
    if gateway.image_id() == M5_IMAGE {
        Ok(())
    } else {
        Err(ExecutionError::Unavailable.into())
    }
}

fn limits(timeout_seconds: u64, ceiling_ms: u64) -> Result<ExecutionLimits, SecurityError> {
    let wall_ms = timeout_seconds
        .checked_mul(1000)
        .filter(|value| *value > 0 && *value <= ceiling_ms)
        .ok_or(SecurityError::Inspection(InspectionError::Internal))?;
    ExecutionLimits::new_job(wall_ms, LOG_BYTES)
        .ok_or(SecurityError::Inspection(InspectionError::Internal))
}

fn runtime_identity(
    gateway: &RustGateway,
    source: &SourceBundle,
    execution: &PerformanceExecution,
) -> Result<RuntimeIdentity, SecurityError> {
    Ok(RuntimeIdentity {
        platform: PLATFORM.into(),
        image_id: gateway.image_id().into(),
        configuration_fingerprint: gateway.configuration_fingerprint()?,
        execution_fingerprint: execution.execution_fingerprint.clone(),
        rust_version: crate::rust_gateway::APPROVED_RUST_VERSION.into(),
        cargo_version: crate::rust_gateway::APPROVED_CARGO_VERSION.into(),
        declared_toolchain: crate::project_metadata::declared_toolchain(source)?,
    })
}

fn termination(capture: &crate::supervisor::Capture) -> ExecutionTermination {
    match capture.stop {
        crate::supervisor::Stop::Exited => ExecutionTermination::Exited,
        crate::supervisor::Stop::TimedOut => ExecutionTermination::TimedOut,
        crate::supervisor::Stop::Cancelled => ExecutionTermination::Cancelled,
        crate::supervisor::Stop::OutputLimit => ExecutionTermination::OutputLimit,
    }
}

/// Wall-clock seconds, used only to stamp a dataset. A clock this process
/// cannot read is not evidence of anything, so it yields `0` rather than a
/// fabricated time; the dataset's identity is its fingerprints, not its stamp.
fn captured_at_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The ceilings the gateway itself applied. They are known exactly, so they are
/// never read back from the guest (ADR-073 §3).
pub(super) fn applied_quotas() -> ResourceQuotas {
    ResourceQuotas {
        cpu_quota_millicores: Some(APPLIED_CPU_MILLICORES),
        memory_bytes: Some(APPLIED_MEMORY_BYTES),
        pids: Some(APPLIED_PIDS),
    }
}

/// The probe results, verbatim. A probe that could not run leaves its field
/// absent, and ADR-073 §5 makes that absence block a later comparison; it is
/// never filled with a plausible value.
pub(super) fn hardware_profile(probe: &HardwareProbe) -> HardwareProfile {
    HardwareProfile {
        cpu_model: probe.cpu_model.clone(),
        cpu_cores: probe.cpu_cores,
        os_kernel: probe.os_kernel.clone(),
        arch: "aarch64".into(),
        virtualization: Virtualization::Container,
        // No governor is observable from inside the container; unknown stays
        // unknown rather than becoming "performance".
        cpu_governor: None,
        quotas: applied_quotas(),
    }
}

/// The provenance ADR-073 §3 requires of every dataset this server emits.
///
/// `run_index` is `1` because one call publishes exactly one dataset: the
/// samples of every completed repetition, pooled per benchmark key. `run_count`
/// stays the number of repetitions requested, so a reader still sees how many
/// independent executions the pooled samples came from.
pub(super) fn provenance(
    identity: &RuntimeIdentity,
    execution: &PerformanceExecution,
    selection: &BenchmarkSelection,
    run_count: u8,
) -> BenchmarkProvenance {
    BenchmarkProvenance {
        source_fingerprint: execution.source_fingerprint.to_string(),
        harness: BenchmarkHarness::Criterion,
        harness_version: APPROVED_CRITERION_VERSION.into(),
        rust_version: identity.rust_version.clone(),
        cargo_version: identity.cargo_version.clone(),
        declared_toolchain: identity.declared_toolchain.clone(),
        image_digest: identity.image_id.clone(),
        platform: identity.platform.clone(),
        configuration_fingerprint: identity.configuration_fingerprint.to_string(),
        execution_fingerprint: identity.execution_fingerprint.to_string(),
        selection: selection.clone(),
        hardware: hardware_profile(&execution.hardware),
        run_index: 1,
        run_count,
        captured_at_unix: captured_at_unix(),
    }
}

// -- rust.benchmark.run ------------------------------------------------------

/// One benchmark key's pooled samples, in the shape the domain rebuilds a
/// measurement from. Identity comes from the first repetition that reported the
/// key; the parser already refuses two directories claiming one `full_id`, so a
/// later repetition cannot redefine it.
struct PooledMeasurement {
    identity: BenchmarkIdentity,
    sampling_mode: SamplingMode,
    samples: Vec<RawSample>,
    truncated: bool,
}

/// Pools the independent repetitions per benchmark key.
///
/// ADR-073 §2 runs each repetition into its own `CRITERION_HOME`, so the raw
/// samples arrive per run. They are concatenated, never averaged: a reader that
/// wants a per-run statistic still has every sample, and a statistic computed
/// here would be one this product could not later justify.
fn pool(runs: &[Vec<BenchmarkMeasurement>]) -> Result<Vec<BenchmarkMeasurement>, SecurityError> {
    let mut pooled: BTreeMap<String, PooledMeasurement> = BTreeMap::new();
    for measurements in runs {
        for measurement in measurements {
            let entry = pooled
                .entry(measurement.key().to_owned())
                .or_insert_with(|| PooledMeasurement {
                    identity: measurement.identity().clone(),
                    sampling_mode: measurement.sampling_mode(),
                    samples: Vec::new(),
                    truncated: false,
                });
            for sample in measurement.samples() {
                if entry.samples.len() >= BENCHMARK_MAX_SAMPLES {
                    // The pooled set is a prefix of what ran, and declares it.
                    entry.truncated = true;
                    break;
                }
                entry.samples.push(*sample);
            }
            if measurement.completeness() == MeasurementCompleteness::Truncated {
                entry.truncated = true;
            }
        }
    }
    pooled
        .into_values()
        .map(|entry| {
            let completeness = if entry.truncated {
                MeasurementCompleteness::Truncated
            } else {
                MeasurementCompleteness::Complete
            };
            BenchmarkMeasurement::new(
                entry.identity,
                entry.sampling_mode,
                entry.samples,
                BENCHMARK_WARM_UP_MS,
                BENCHMARK_MEASUREMENT_MS,
                BENCHMARK_SAMPLE_SIZE,
                completeness,
            )
            .map_err(|_| SecurityError::InvalidMetadata)
        })
        .collect()
}

/// Why this run published no dataset. Only reasons the adapter actually
/// observed are produced.
fn benchmark_omission(
    harness: &HarnessDetection,
    runs_completed: u8,
    archived: bool,
) -> Option<DatasetOmission> {
    match harness {
        HarnessDetection::Unrecognized => Some(DatasetOmission::HarnessUnrecognized),
        HarnessDetection::CriterionUnapproved { .. } => Some(DatasetOmission::HarnessUnapproved),
        HarnessDetection::Criterion { .. } if runs_completed == 0 => {
            Some(DatasetOmission::ExecutionFailed)
        }
        HarnessDetection::Criterion { .. } if !archived => Some(DatasetOmission::OutputMissing),
        HarnessDetection::Criterion { .. } => None,
    }
}

/// A parser refusal named as the omission it is. The samples are the primary
/// evidence, so a refusal is reported, never smoothed into a smaller dataset.
fn criterion_omission(failure: criterion_dataset::CriterionError) -> DatasetOmission {
    match failure {
        criterion_dataset::CriterionError::Empty
        | criterion_dataset::CriterionError::NoMeasurement => DatasetOmission::OutputMissing,
        criterion_dataset::CriterionError::TooLarge
        | criterion_dataset::CriterionError::TooManyBenchmarks => DatasetOmission::OutputTooLarge,
        criterion_dataset::CriterionError::Malformed
        | criterion_dataset::CriterionError::InvalidSample => DatasetOmission::OutputUnparsable,
    }
}

/// Decodes every exported repetition, or names the first refusal.
fn parse_runs(
    execution: &PerformanceExecution,
) -> Result<Vec<Vec<BenchmarkMeasurement>>, DatasetOmission> {
    execution
        .runs
        .iter()
        .filter(|run| !run.archive.is_empty())
        .map(|run| {
            criterion_dataset::parse_archive(
                &run.archive,
                BENCHMARK_WARM_UP_MS,
                BENCHMARK_MEASUREMENT_MS,
                BENCHMARK_SAMPLE_SIZE,
            )
            .map(|parse| parse.measurements)
            .map_err(criterion_omission)
        })
        .collect()
}

pub(super) fn benchmark(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    options: &BenchmarkRunOptions,
    control: &dyn InspectionControl,
) -> Result<BenchmarkObservation, SecurityError> {
    admitted(gateway)?;
    let selection = options.selection();
    let limits = limits(options.timeout_seconds(), BENCHMARK_BUDGET_MS)?;
    let execution = performance_gateway::execute_benchmark(
        gateway,
        source,
        vendor,
        &selection,
        options.run_count(),
        limits,
        control,
    )
    .map_err(error)?;
    observed(&execution, PerformanceKind::Benchmark)?;
    let identity = runtime_identity(gateway, source, &execution)?;
    let runs_completed = u8::try_from(
        execution
            .runs
            .iter()
            .filter(|run| run.capture.code == Some(0))
            .count(),
    )
    .unwrap_or(u8::MAX);
    let archived = execution.runs.iter().any(|run| !run.archive.is_empty());
    let last = execution
        .runs
        .last()
        .ok_or(SecurityError::InvalidMetadata)?;
    let exit = if last.capture.stop == crate::supervisor::Stop::Exited {
        last.capture
            .code
            .map_or(BenchmarkExit::Uncalibrated, BenchmarkExit::classify)
    } else {
        BenchmarkExit::Incomplete
    };
    let (dataset, omission) = match benchmark_omission(&execution.harness, runs_completed, archived)
    {
        Some(omission) => (None, Some(omission)),
        None => match parse_runs(&execution) {
            Err(omission) => (None, Some(omission)),
            Ok(parsed) => {
                let measurements = pool(&parsed)?;
                if measurements.is_empty() {
                    (None, Some(DatasetOmission::OutputMissing))
                } else {
                    let dataset = BenchmarkDataset::new(
                        SampleUnit::Nanoseconds,
                        measurements,
                        provenance(&identity, &execution, &selection, options.run_count()),
                    )
                    .map_err(|_| SecurityError::InvalidMetadata)?;
                    (Some(dataset), None)
                }
            }
        },
    };
    let observation = BenchmarkObservation {
        selection,
        harness: execution.harness.clone(),
        exit,
        exit_code: last.capture.code,
        termination: termination(&last.capture),
        dataset,
        omission,
        runs_completed,
        runs_requested: options.run_count(),
        runtime: identity,
        execution_fingerprint: execution.execution_fingerprint.clone(),
        vendor_fingerprint: execution.vendor_fingerprint.clone(),
        stdout: last.capture.stdout.clone(),
        stderr: last.capture.stderr.clone(),
        stdout_truncated: last.capture.stdout_truncated,
        stderr_truncated: last.capture.stderr_truncated,
    };
    if observation.consistent() {
        Ok(observation)
    } else {
        Err(SecurityError::InvalidMetadata)
    }
}

// -- rust.profile.flamegraph -------------------------------------------------

/// The one manifest this adapter reads: the document
/// `fixtures/profile-helper` writes, in the version it writes today.
const HELPER_MANIFEST_SCHEMA: &str = "rust-engineering-mcp.profile-helper.v1";

/// Every key the helper writes, and the complete set this reader accepts.
///
/// The document's own key set is checked against this before it is typed,
/// because `serde` reads an absent `Option` field as `None`: without the check
/// the three `number | null` keys would silently default, which is the
/// tolerance this reader exists to remove. `deny_unknown_fields` on
/// [`HelperManifest`] then keeps the two lists from drifting apart.
const HELPER_MANIFEST_KEYS: [&str; 19] = [
    "schema",
    "status",
    "frequency_hz",
    "requested_duration_ms",
    "observed_duration_ms",
    "samples_collected",
    "samples_lost",
    "stacks_written",
    "frames_total",
    "frames_unresolved",
    "stacks_truncated",
    "max_depth",
    "modules_seen",
    "cpus_sampled",
    "descendants_reaped",
    "namespace_drained",
    "child_exit_code",
    "child_signal",
    "perf_errno",
];

/// The helper's own manifest, exactly as the helper writes it.
///
/// Every key is required and no unknown key is accepted. That is deliberately
/// the opposite of what this reader used to do: with every field
/// `#[serde(default)]` and the document read through `unwrap_or_default()`, an
/// empty, truncated or fabricated manifest deserialized into a clean-looking
/// set of counters, and the adapter published them as a measurement. The host
/// cannot re-run the sampler, so this document is its only account of what
/// happened inside a container full of the project's own code; a document that
/// is not exactly the one this helper version writes is not that account, and
/// tolerance here buys a forger far more than it buys a future helper.
///
/// A helper that gains a field is therefore a breaking change for this reader,
/// on purpose: helper and adapter ship in one image (`M5_IMAGE`) built from one
/// tree, so they are versioned together and there is no skew to tolerate.
///
/// The `Default` impl is a host-side "no helper ran" value used when the build
/// failed, never a deserialization fallback: `serde` fills nothing in.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HelperManifest {
    schema: String,
    status: String,
    frequency_hz: u32,
    requested_duration_ms: u64,
    observed_duration_ms: u64,
    samples_collected: u64,
    samples_lost: u64,
    stacks_written: u64,
    frames_total: u64,
    frames_unresolved: u64,
    stacks_truncated: u64,
    max_depth: u32,
    modules_seen: u64,
    cpus_sampled: u32,
    descendants_reaped: u64,
    namespace_drained: bool,
    child_exit_code: Option<i32>,
    child_signal: Option<i32>,
    perf_errno: Option<i32>,
}

/// Reads the helper's manifest and refuses anything that is not it.
///
/// `InvalidMetadata` is the closest existing error and the exact one: the
/// artifact bytes may be fine, it is the metadata describing them that this
/// host cannot vouch for. It is also what [`profile`] already returns for an
/// observation that contradicts itself, and what the application maps to a
/// refusal rather than to a project failure.
fn parse_helper_manifest(bytes: &[u8]) -> Result<HelperManifest, SecurityError> {
    let document: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(bytes).map_err(|_| SecurityError::InvalidMetadata)?;
    if document.len() != HELPER_MANIFEST_KEYS.len()
        || !HELPER_MANIFEST_KEYS
            .iter()
            .all(|key| document.contains_key(*key))
    {
        return Err(SecurityError::InvalidMetadata);
    }
    let manifest: HelperManifest =
        serde_json::from_slice(bytes).map_err(|_| SecurityError::InvalidMetadata)?;
    if manifest.schema != HELPER_MANIFEST_SCHEMA {
        return Err(SecurityError::InvalidMetadata);
    }
    Ok(manifest)
}

/// Whether the manifest is an account of *this* run, checked against what this
/// host itself put on the helper's argv and against the container it created.
///
/// None of this needs the artifact; it is the part a merely buggy helper fails
/// just as readily as a forged one.
fn manifest_describes_this_run(
    manifest: &HelperManifest,
    options: &ProfileOptions,
    status: ProfileStatus,
) -> bool {
    // The three numbers the gateway put on the argv, echoed back. A manifest
    // that does not repeat the request is not the answer to it.
    let echoes_the_request = manifest.frequency_hz == options.frequency_hz()
        && manifest.requested_duration_ms == options.duration_ms().min(PROFILE_MAX_SAMPLING_MS)
        && manifest.max_depth == PROFILE_MAX_DEPTH;
    // The helper empties its PID namespace before writing either artifact. When
    // it reports that it could not, it is telling us the artifacts were written
    // while another process could still rewrite them; there is then nothing
    // here to corroborate, whatever the numbers say.
    let artifacts_were_the_helper_s =
        manifest.namespace_drained && manifest.descendants_reaped <= u64::from(APPLIED_PIDS);
    // ADR-074 §3: a denial is reportable only with the errno that produced it,
    // and a denial sampled no CPU and collected nothing. The converse holds
    // too: a run that was not denied has no errno to report.
    let denial_is_coherent = if status == ProfileStatus::ProfilerUnavailable {
        manifest.perf_errno.is_some()
            && manifest.cpus_sampled == 0
            && manifest.samples_collected == 0
            && manifest.stacks_written == 0
    } else {
        manifest.perf_errno.is_none()
    };
    // Samples come from armed per-CPU events; there is no other producer.
    let samples_had_a_source = manifest.samples_collected == 0 || manifest.cpus_sampled > 0;
    echoes_the_request && artifacts_were_the_helper_s && denial_is_coherent && samples_had_a_source
}

/// The manifest counts what the helper says it wrote; [`FoldedProfile`] is what
/// this host parsed back out of the artifact. Two independent accounts of one
/// run, so they must agree exactly — the helper writes one line per distinct
/// stack and one sample per unit of count, with no rounding and no sampling of
/// its own.
///
/// This is the check a fabricated manifest cannot pass without also fabricating
/// the stacks file, and the one a helper with an off-by-one counter fails.
fn manifest_matches_the_artifact(manifest: &HelperManifest, profile: &FoldedProfile) -> bool {
    u64::try_from(profile.distinct_stacks).is_ok_and(|distinct| distinct == manifest.stacks_written)
        && profile.total_samples == manifest.samples_collected
}

fn helper_status(value: &str) -> ProfileStatus {
    match value {
        "complete" => ProfileStatus::Complete,
        "sample_limit" => ProfileStatus::SampleLimit,
        "duration_limit" => ProfileStatus::DurationLimit,
        "child_exited" => ProfileStatus::ChildExited,
        // An absent or unreadable status is not evidence that sampling worked.
        _ => ProfileStatus::ProfilerUnavailable,
    }
}

fn build_outcome(build: &crate::supervisor::Capture) -> ProfileBuildOutcome {
    if build.code == Some(0) {
        return ProfileBuildOutcome::Built;
    }
    // Cargo refusing to select the target, distinguished from a project that
    // selected fine and did not compile. Both are project outcomes.
    if String::from_utf8_lossy(&build.stderr).contains("no bin target") {
        ProfileBuildOutcome::TargetNotFound
    } else {
        ProfileBuildOutcome::CompilationFailed
    }
}

/// Completeness must agree with the counters it summarizes, in the order
/// [`ProfileObservation::consistent`] validates them.
fn profile_completeness(status: ProfileStatus, counters: &ProfileCounters) -> ProfileCompleteness {
    if status == ProfileStatus::ProfilerUnavailable {
        ProfileCompleteness::Unavailable
    } else if counters.samples_collected == 0 {
        ProfileCompleteness::NoSamples
    } else if counters.samples_lost > 0 {
        ProfileCompleteness::LostSamples
    } else if counters.stacks_truncated > 0 {
        ProfileCompleteness::Truncated
    } else {
        ProfileCompleteness::Complete
    }
}

fn frame_weights(profile: &FoldedProfile) -> Vec<ProfileFrameWeight> {
    profile_stacks::frame_weights(profile, TOP_FRAMES)
        .into_iter()
        .map(|weight| ProfileFrameWeight {
            frame: weight.frame,
            self_samples: weight.self_samples,
            total_samples: weight.total_samples,
        })
        .collect()
}

fn profile_observation(
    options: &ProfileOptions,
    output: &ProfileOutput,
    identity: RuntimeIdentity,
    execution: &PerformanceExecution,
) -> Result<ProfileObservation, SecurityError> {
    // A target that did not build never started a child, and the helper's own
    // vocabulary for "the child never started" is `ChildExited`. It is not a
    // profiler denial and is never reported as one: ADR-074 §3 makes a denial
    // reportable only together with the errno that produced it, and there is no
    // errno here because `perf_event_open` was never reached.
    let (manifest, status) = if output.run.is_some() {
        let manifest = parse_helper_manifest(&output.manifest)?;
        let status = helper_status(&manifest.status);
        if !manifest_describes_this_run(&manifest, options, status) {
            return Err(SecurityError::InvalidMetadata);
        }
        (manifest, status)
    } else {
        (HelperManifest::default(), ProfileStatus::ChildExited)
    };
    let counters = ProfileCounters {
        observed_duration_ms: manifest.observed_duration_ms,
        samples_collected: manifest.samples_collected,
        samples_lost: manifest.samples_lost,
        stacks_written: manifest.stacks_written,
        frames_total: manifest.frames_total,
        frames_unresolved: manifest.frames_unresolved,
        stacks_truncated: manifest.stacks_truncated,
        modules_seen: manifest.modules_seen,
        max_depth_applied: manifest.max_depth,
    };
    let completeness = profile_completeness(status, &counters);
    // A denied profiler, and a target that never ran, collected nothing; there
    // is nothing to render for either.
    let folded = (output.run.is_some() && completeness != ProfileCompleteness::Unavailable)
        .then(|| profile_stacks::parse_folded(&output.stacks).ok())
        .flatten();
    match &folded {
        Some(profile) => {
            if !manifest_matches_the_artifact(&manifest, profile) {
                return Err(SecurityError::InvalidMetadata);
            }
        }
        // A sampler that ran and was not denied wrote a stacks file this host
        // could not parse, so the counters have nothing to answer to. They are
        // refused rather than published beside an empty artifact: an
        // uncorroborated count is exactly what this reconciliation exists to
        // stop. The other two ways to reach `None` — a denial, or a target that
        // never built — are declared and have nothing to compare against.
        None => {
            if output.run.is_some() && completeness != ProfileCompleteness::Unavailable {
                return Err(SecurityError::InvalidMetadata);
            }
        }
    }
    let top_frames = folded.as_ref().map(frame_weights).unwrap_or_default();
    let title = format!(
        "{} @ {} Hz",
        options.binary_target(),
        options.frequency_hz()
    );
    let svg = folded
        .as_ref()
        .and_then(|profile| profile_svg::render(profile, &title, SvgOptions::default()).ok())
        .unwrap_or_default();
    let reported = output.run.as_ref().unwrap_or(&output.build);
    Ok(ProfileObservation {
        options: options.clone(),
        backend: PROFILE_BACKEND,
        build: build_outcome(&output.build),
        build_exit_code: output.build.code,
        status,
        counters,
        child: ProfileChild {
            exit_code: manifest.child_exit_code,
            signal: manifest.child_signal,
        },
        perf_errno: manifest.perf_errno,
        completeness,
        top_frames,
        stacks: if folded.is_some() {
            output.stacks.clone()
        } else {
            Vec::new()
        },
        svg,
        termination: termination(reported),
        runtime: identity,
        execution_fingerprint: execution.execution_fingerprint.clone(),
        vendor_fingerprint: execution.vendor_fingerprint.clone(),
        stdout: reported.stdout.clone(),
        stderr: reported.stderr.clone(),
        stdout_truncated: reported.stdout_truncated,
        stderr_truncated: reported.stderr_truncated,
    })
}

pub(super) fn profile(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    options: &ProfileOptions,
    control: &dyn InspectionControl,
) -> Result<ProfileObservation, SecurityError> {
    admitted(gateway)?;
    // The application port carries no timeout: ADR-076 §7's profiling budget is
    // the whole allowance, and the sampling window inside it is capped by the
    // gateway at ADR-074 §4's 60 s.
    let limits = limits(PROFILE_BUDGET_MS / 1000, PROFILE_BUDGET_MS)?;
    let execution =
        performance_gateway::execute_profile(gateway, source, vendor, options, limits, control)
            .map_err(error)?;
    observed(&execution, PerformanceKind::Profile)?;
    let identity = runtime_identity(gateway, source, &execution)?;
    let output = execution
        .profile
        .as_ref()
        .ok_or(SecurityError::InvalidMetadata)?;
    let observation = profile_observation(options, output, identity, &execution)?;
    if observation.consistent() {
        Ok(observation)
    } else {
        Err(SecurityError::InvalidMetadata)
    }
}

// -- rust.binary.bloat -------------------------------------------------------

fn measured_size(capture: &crate::supervisor::Capture) -> Option<u64> {
    if capture.code != Some(0) {
        return None;
    }
    std::str::from_utf8(&capture.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

/// `sha256sum` prints `<64 hex>  <path>`. Only the digest is kept, and only
/// when it is exactly the shape the product expects.
fn measured_digest(capture: &crate::supervisor::Capture) -> Option<String> {
    if capture.code != Some(0) {
        return None;
    }
    let text = std::str::from_utf8(&capture.stdout).ok()?;
    let hex = text.split_whitespace().next()?;
    (hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| format!("sha256:{hex}"))
}

/// `readelf -h` classifies the container format. Mach-O, PE and WASM are not
/// produced by this Linux guest and are not qualified by the ELF positive; a
/// file `readelf` does not recognise stays `Unknown` rather than being guessed.
fn measured_format(capture: &crate::supervisor::Capture) -> BinaryFormat {
    if capture.code != Some(0) {
        return BinaryFormat::Unknown;
    }
    let text = String::from_utf8_lossy(&capture.stdout);
    if !text.contains("ELF Header") {
        return BinaryFormat::Unknown;
    }
    let field = |name: &str| {
        text.lines()
            .find_map(|line| line.trim().strip_prefix(name))
            .map(|value| value.trim().to_owned())
            .unwrap_or_default()
    };
    if field("Class:").contains("ELF64") && field("Machine:").contains("AArch64") {
        BinaryFormat::Elf64Aarch64
    } else {
        BinaryFormat::OtherElf
    }
}

/// The product's own measurement of the file, plus the one claim about it that
/// is a property of the analyzer rather than of the file: `cargo-bloat 0.12.1`
/// forces `CARGO_PROFILE_<PROFILE>_STRIP=false` on every build it performs, so
/// what we measured is an analysis build and never what a stripping project
/// would ship. It is always declared, and never as `false`.
fn measured_binary(output: &BloatOutput) -> Option<MeasuredBinary> {
    Some(MeasuredBinary {
        size_bytes: measured_size(&output.size)?,
        sha256: measured_digest(&output.digest)?,
        format: measured_format(&output.header),
        analysis_build_symbols_forced: true,
    })
}

fn attribution(output: &BloatOutput) -> Option<BloatAttribution> {
    let functions = bloat_json::parse_functions(&output.functions.stdout).ok()?;
    let crates = bloat_json::parse_crates(&output.crates.stdout).ok()?;
    // Two views of one file. If the analyzer does not agree with itself about
    // the file it looked at, neither view describes a file we can name.
    if functions.file_size_bytes != crates.file_size_bytes
        || functions.text_section_size_bytes != crates.text_section_size_bytes
    {
        return None;
    }
    Some(BloatAttribution {
        estimated: true,
        reported_file_size_bytes: Some(functions.file_size_bytes),
        text_section_size_bytes: Some(functions.text_section_size_bytes),
        functions: functions.functions,
        crates: crates.crates,
        functions_omitted: functions.omitted,
        crates_omitted: crates.omitted,
    })
}

/// ADR-076 §6, in order: no measurement of our own is `Unavailable`; a format
/// the ELF positive does not qualify is `UnsupportedFormat`; an attribution
/// whose file size disagrees with ours describes another file and is
/// `SizeMismatch`; a ranking the report capped is `Truncated`.
fn bloat_completeness(
    measured: Option<&MeasuredBinary>,
    attribution: Option<&BloatAttribution>,
) -> BloatCompleteness {
    let Some(measured) = measured else {
        return BloatCompleteness::Unavailable;
    };
    if !matches!(
        measured.format,
        BinaryFormat::Elf64Aarch64 | BinaryFormat::OtherElf
    ) {
        return BloatCompleteness::UnsupportedFormat;
    }
    let Some(attribution) = attribution else {
        return BloatCompleteness::Unavailable;
    };
    if attribution.reported_file_size_bytes != Some(measured.size_bytes) {
        return BloatCompleteness::SizeMismatch;
    }
    if attribution.functions_omitted > 0 || attribution.crates_omitted > 0 {
        return BloatCompleteness::Truncated;
    }
    BloatCompleteness::Complete
}

pub(super) fn bloat(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    options: &BloatOptions,
    control: &dyn InspectionControl,
) -> Result<BloatObservation, SecurityError> {
    admitted(gateway)?;
    let limits = limits(BLOAT_BUDGET_MS / 1000, BLOAT_BUDGET_MS)?;
    let execution =
        performance_gateway::execute_bloat(gateway, source, vendor, options, limits, control)
            .map_err(error)?;
    observed(&execution, PerformanceKind::Bloat)?;
    let identity = runtime_identity(gateway, source, &execution)?;
    let output = execution
        .bloat
        .as_ref()
        .ok_or(SecurityError::InvalidMetadata)?;
    let measured = measured_binary(output);
    let attribution = attribution(output);
    let completeness = bloat_completeness(measured.as_ref(), attribution.as_ref());
    let observation = BloatObservation {
        options: options.clone(),
        analyzer_version: APPROVED_CARGO_BLOAT_VERSION.into(),
        exit: if output.functions.stop == crate::supervisor::Stop::Exited {
            output
                .functions
                .code
                .map_or(BloatExit::Uncalibrated, BloatExit::classify)
        } else {
            BloatExit::Incomplete
        },
        exit_code: output.functions.code,
        termination: termination(&output.functions),
        measured,
        attribution,
        completeness,
        report: output.functions.stdout.clone(),
        runtime: identity,
        execution_fingerprint: execution.execution_fingerprint.clone(),
        vendor_fingerprint: execution.vendor_fingerprint.clone(),
        stdout: output.functions.stdout.clone(),
        stderr: output.functions.stderr.clone(),
        stdout_truncated: output.functions.stdout_truncated,
        stderr_truncated: output.functions.stderr_truncated,
    };
    if observation.consistent() {
        Ok(observation)
    } else {
        Err(SecurityError::InvalidMetadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::supervisor::{Capture, Stop};

    fn capture(code: Option<i32>, stdout: &[u8]) -> Capture {
        Capture {
            code,
            stdout: stdout.to_vec(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stop: Stop::Exited,
            duration_ms: 1,
        }
    }

    fn fingerprint(value: char) -> Result<rust_engineering_domain::SourceFingerprint, String> {
        format!("sha256:{}", value.to_string().repeat(64))
            .parse()
            .map_err(|error| format!("{error:?}"))
    }

    fn identity() -> Result<RuntimeIdentity, String> {
        Ok(RuntimeIdentity {
            platform: PLATFORM.into(),
            image_id: M5_IMAGE.into(),
            configuration_fingerprint: format!("sha256:{}", "a".repeat(64))
                .parse()
                .map_err(|error| format!("{error:?}"))?,
            execution_fingerprint: format!("sha256:{}", "b".repeat(64))
                .parse()
                .map_err(|error| format!("{error:?}"))?,
            rust_version: "1.98.1".into(),
            cargo_version: "1.98.1".into(),
            declared_toolchain: None,
        })
    }

    fn execution() -> Result<PerformanceExecution, String> {
        let identity = identity()?;
        Ok(PerformanceExecution {
            kind: PerformanceKind::Profile,
            harness: HarnessDetection::Unrecognized,
            hardware: HardwareProbe::default(),
            runs: Vec::new(),
            profile: None,
            bloat: None,
            execution_fingerprint: identity.execution_fingerprint.clone(),
            source_fingerprint: fingerprint('c')?,
            vendor_fingerprint: fingerprint('d')?,
        })
    }

    fn measurement(
        key: &str,
        samples: usize,
        completeness: MeasurementCompleteness,
    ) -> Result<BenchmarkMeasurement, String> {
        let identity = BenchmarkIdentity::new(
            "group".into(),
            Some("function".into()),
            None,
            key.into(),
            key.replace('/', "_"),
        )
        .map_err(|error| format!("{error:?}"))?;
        let samples = (0..samples)
            .map(|index| {
                RawSample::new(1, 1_000.0 + index as f64).map_err(|error| format!("{error:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        BenchmarkMeasurement::new(
            identity,
            SamplingMode::Flat,
            samples,
            BENCHMARK_WARM_UP_MS,
            BENCHMARK_MEASUREMENT_MS,
            BENCHMARK_SAMPLE_SIZE,
            completeness,
        )
        .map_err(|error| format!("{error:?}"))
    }

    #[test]
    fn only_the_qualified_m5_runtime_is_admitted() {
        assert_eq!(M5_IMAGE.len(), 71);
        assert!(M5_IMAGE.starts_with("sha256:"));
        assert_ne!(M5_IMAGE, crate::APPROVED_M4_IMAGE);
        assert_eq!(M5_IMAGE, crate::APPROVED_M5_IMAGE);
    }

    #[test]
    fn a_timeout_over_its_adr_ceiling_is_refused_before_any_container() {
        assert!(limits(900, BENCHMARK_BUDGET_MS).is_ok());
        assert!(matches!(
            limits(901, BENCHMARK_BUDGET_MS),
            Err(SecurityError::Inspection(InspectionError::Internal))
        ));
        assert!(limits(300, PROFILE_BUDGET_MS).is_ok());
        assert!(limits(301, PROFILE_BUDGET_MS).is_err());
        assert!(limits(300, BLOAT_BUDGET_MS).is_ok());
        assert!(limits(u64::MAX, BLOAT_BUDGET_MS).is_err());
        assert!(limits(0, BLOAT_BUDGET_MS).is_err());
    }

    #[test]
    fn gateway_failures_keep_their_meaning_in_the_application_vocabulary() {
        assert_eq!(error(PerformanceError::Timeout), SecurityError::Timeout);
        assert_eq!(
            error(PerformanceError::OutputLimit),
            SecurityError::OutputLimit
        );
        assert_eq!(
            error(PerformanceError::MissingOfflineData),
            SecurityError::MissingOfflineData
        );
        assert_eq!(
            error(PerformanceError::InvalidMetadata),
            SecurityError::InvalidMetadata
        );
        assert_eq!(
            error(PerformanceError::Inspection(InspectionError::Execution(
                ExecutionError::CleanupUncertain
            ))),
            SecurityError::Inspection(InspectionError::Execution(ExecutionError::CleanupUncertain))
        );
        assert_eq!(
            error(PerformanceError::InvalidOptions),
            SecurityError::Inspection(InspectionError::Internal)
        );
        // A project Cargo configuration is a containment refusal, so the tool
        // reports it as blocked and not as an unavailable capability.
        assert_eq!(
            error(PerformanceError::ProjectCargoConfiguration),
            SecurityError::Inspection(InspectionError::Project(
                rust_engineering_application::ProjectError::Rejected(
                    rust_engineering_domain::OperationalErrorCode::SandboxDenied
                )
            ))
        );
    }

    #[test]
    fn quotas_come_from_the_limits_the_gateway_applied() {
        assert_eq!(
            applied_quotas(),
            ResourceQuotas {
                cpu_quota_millicores: Some(1_000),
                memory_bytes: Some(1_073_741_824),
                pids: Some(128),
            }
        );
    }

    #[test]
    fn an_unobserved_hardware_field_stays_unknown_and_is_never_defaulted() {
        let empty = hardware_profile(&HardwareProbe::default());
        assert_eq!(empty.cpu_model, None);
        assert_eq!(empty.cpu_cores, None);
        assert_eq!(empty.os_kernel, None);
        assert_eq!(empty.cpu_governor, None);
        assert_eq!(empty.arch, "aarch64");
        assert_eq!(empty.virtualization, Virtualization::Container);
        assert_eq!(empty.validate(), Ok(()));
        let observed = hardware_profile(&HardwareProbe {
            cpu_model: Some("Fixture CPU".into()),
            cpu_cores: Some(4),
            os_kernel: Some("Linux 7.0.12-linuxkit aarch64".into()),
        });
        assert_eq!(observed.cpu_model.as_deref(), Some("Fixture CPU"));
        assert_eq!(observed.cpu_cores, Some(4));
        assert_eq!(
            observed.os_kernel.as_deref(),
            Some("Linux 7.0.12-linuxkit aarch64")
        );
    }

    #[test]
    fn provenance_carries_the_frozen_method_and_this_executions_identity() -> Result<(), String> {
        let identity = identity()?;
        let execution = execution()?;
        let selection = BenchmarkSelection {
            package: None,
            bench_target: Some("throughput".into()),
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            profile: "bench".into(),
        };
        let provenance = provenance(&identity, &execution, &selection, 3);
        assert_eq!(provenance.harness, BenchmarkHarness::Criterion);
        assert_eq!(provenance.harness_version, APPROVED_CRITERION_VERSION);
        assert_eq!(provenance.image_digest, M5_IMAGE);
        assert_eq!(provenance.platform, PLATFORM);
        assert_eq!(
            provenance.execution_fingerprint,
            identity.execution_fingerprint.to_string()
        );
        assert_eq!(
            provenance.source_fingerprint,
            execution.source_fingerprint.to_string()
        );
        assert_eq!(provenance.run_index, 1);
        assert_eq!(provenance.run_count, 3);
        assert_eq!(
            provenance.hardware.virtualization,
            Virtualization::Container
        );
        assert_eq!(provenance.hardware.quotas, applied_quotas());
        assert_eq!(provenance.validate(), Ok(()));
        Ok(())
    }

    #[test]
    fn the_omission_names_what_the_adapter_actually_observed() {
        assert_eq!(
            benchmark_omission(&HarnessDetection::Unrecognized, 3, true),
            Some(DatasetOmission::HarnessUnrecognized)
        );
        assert_eq!(
            benchmark_omission(
                &HarnessDetection::CriterionUnapproved {
                    version: "0.5.1".into()
                },
                3,
                true
            ),
            Some(DatasetOmission::HarnessUnapproved)
        );
        let approved = HarnessDetection::Criterion {
            version: APPROVED_CRITERION_VERSION.into(),
        };
        assert_eq!(
            benchmark_omission(&approved, 0, true),
            Some(DatasetOmission::ExecutionFailed)
        );
        assert_eq!(
            benchmark_omission(&approved, 3, false),
            Some(DatasetOmission::OutputMissing)
        );
        assert_eq!(benchmark_omission(&approved, 3, true), None);
        assert_eq!(
            criterion_omission(criterion_dataset::CriterionError::Empty),
            DatasetOmission::OutputMissing
        );
        assert_eq!(
            criterion_omission(criterion_dataset::CriterionError::NoMeasurement),
            DatasetOmission::OutputMissing
        );
        assert_eq!(
            criterion_omission(criterion_dataset::CriterionError::TooLarge),
            DatasetOmission::OutputTooLarge
        );
        assert_eq!(
            criterion_omission(criterion_dataset::CriterionError::TooManyBenchmarks),
            DatasetOmission::OutputTooLarge
        );
        assert_eq!(
            criterion_omission(criterion_dataset::CriterionError::Malformed),
            DatasetOmission::OutputUnparsable
        );
        assert_eq!(
            criterion_omission(criterion_dataset::CriterionError::InvalidSample),
            DatasetOmission::OutputUnparsable
        );
    }

    #[test]
    fn independent_repetitions_pool_per_benchmark_key_and_declare_truncation() -> Result<(), String>
    {
        let runs = vec![
            vec![
                measurement("bench/one", 30, MeasurementCompleteness::Complete)?,
                measurement("bench/two", 30, MeasurementCompleteness::Complete)?,
            ],
            vec![measurement(
                "bench/one",
                30,
                MeasurementCompleteness::Complete,
            )?],
        ];
        let pooled = pool(&runs).map_err(|error| format!("{error:?}"))?;
        assert_eq!(pooled.len(), 2);
        let one = pooled
            .iter()
            .find(|measurement| measurement.key() == "bench/one")
            .ok_or("bench/one")?;
        assert_eq!(one.samples().len(), 60);
        assert_eq!(one.completeness(), MeasurementCompleteness::Complete);
        assert_eq!(one.sample_size_requested(), BENCHMARK_SAMPLE_SIZE);
        assert_eq!(one.warm_up_ms(), BENCHMARK_WARM_UP_MS);
        assert_eq!(one.measurement_ms(), BENCHMARK_MEASUREMENT_MS);
        let two = pooled
            .iter()
            .find(|measurement| measurement.key() == "bench/two")
            .ok_or("bench/two")?;
        assert_eq!(two.samples().len(), 30);
        // A truncated repetition truncates the pooled set; it never disappears.
        let runs = vec![vec![measurement(
            "bench/one",
            30,
            MeasurementCompleteness::Truncated,
        )?]];
        let pooled = pool(&runs).map_err(|error| format!("{error:?}"))?;
        assert_eq!(
            pooled.first().map(BenchmarkMeasurement::completeness),
            Some(MeasurementCompleteness::Truncated)
        );
        assert!(pool(&[]).map_err(|error| format!("{error:?}"))?.is_empty());
        Ok(())
    }

    /// The manifest `fixtures/profile-helper` writes for [`sampled_output`],
    /// as an ordered key/value list so a test can bend exactly one key.
    fn helper_fields() -> Vec<(String, String)> {
        [
            ("schema", format!("\"{HELPER_MANIFEST_SCHEMA}\"")),
            ("status", "\"complete\"".to_owned()),
            ("frequency_hz", "99".to_owned()),
            ("requested_duration_ms", "10000".to_owned()),
            ("observed_duration_ms", "9987".to_owned()),
            ("samples_collected", "4".to_owned()),
            ("samples_lost", "0".to_owned()),
            ("stacks_written", "2".to_owned()),
            ("frames_total", "4".to_owned()),
            ("frames_unresolved", "0".to_owned()),
            ("stacks_truncated", "0".to_owned()),
            ("max_depth", "127".to_owned()),
            ("modules_seen", "1".to_owned()),
            ("cpus_sampled", "4".to_owned()),
            ("descendants_reaped", "0".to_owned()),
            ("namespace_drained", "true".to_owned()),
            ("child_exit_code", "0".to_owned()),
            ("child_signal", "null".to_owned()),
            ("perf_errno", "null".to_owned()),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
    }

    fn render_fields(fields: &[(String, String)]) -> Vec<u8> {
        let body = fields
            .iter()
            .map(|(key, value)| format!("\"{key}\":{value}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{{{body}}}\n").into_bytes()
    }

    /// The helper's manifest with `edits` applied: a key the document already
    /// carries is replaced, a key it does not is appended (which is how the
    /// unknown-field refusal is driven), and `None` removes it.
    fn manifest_bytes(edits: &[(&str, Option<&str>)]) -> Vec<u8> {
        let mut fields = helper_fields();
        for (key, value) in edits {
            match value {
                Some(value) => {
                    if let Some(field) = fields.iter_mut().find(|field| field.0 == *key) {
                        field.1 = (*value).to_owned();
                    } else {
                        fields.push(((*key).to_owned(), (*value).to_owned()));
                    }
                }
                None => fields.retain(|field| field.0 != *key),
            }
        }
        render_fields(&fields)
    }

    /// A run of the sampler that collected four samples over two stacks. The
    /// stacks artifact and the manifest agree, which is what makes it the base
    /// every refusal below departs from by exactly one key.
    fn sampled_output(manifest: Vec<u8>) -> ProfileOutput {
        ProfileOutput {
            build: capture(Some(0), b""),
            run: Some(capture(Some(0), b"")),
            stacks: b"main;idle 1\nmain;work 3\n".to_vec(),
            manifest,
        }
    }

    fn profile_options() -> Result<ProfileOptions, String> {
        ProfileOptions::new("workload".into(), 99, 10).map_err(|error| format!("{error:?}"))
    }

    #[test]
    fn the_helper_manifest_is_read_field_by_field() -> Result<(), String> {
        let parsed = parse_helper_manifest(&manifest_bytes(&[]))
            .map_err(|error| format!("the base manifest was refused: {error:?}"))?;
        assert_eq!(parsed.schema, HELPER_MANIFEST_SCHEMA);
        assert_eq!(parsed.status, "complete");
        assert_eq!(parsed.frequency_hz, 99);
        assert_eq!(parsed.requested_duration_ms, 10_000);
        assert_eq!(parsed.observed_duration_ms, 9_987);
        assert_eq!(parsed.samples_collected, 4);
        assert_eq!(parsed.stacks_written, 2);
        assert_eq!(parsed.frames_unresolved, 0);
        assert_eq!(parsed.max_depth, 127);
        assert_eq!(parsed.modules_seen, 1);
        assert_eq!(parsed.cpus_sampled, 4);
        assert_eq!(parsed.descendants_reaped, 0);
        assert!(parsed.namespace_drained);
        assert_eq!(parsed.child_exit_code, Some(0));
        assert_eq!(parsed.child_signal, None);
        assert_eq!(parsed.perf_errno, None);
        Ok(())
    }

    #[test]
    fn the_manifest_reader_refuses_every_document_but_the_helper_s() {
        // A schema this adapter does not know how to read, including the one
        // the manual smoke record misspells.
        for schema in [
            "\"rust-engineering-mcp.profile-manifest.v1\"",
            "\"rust-engineering-mcp.profile-helper.v2\"",
            "\"\"",
            "null",
        ] {
            assert!(
                parse_helper_manifest(&manifest_bytes(&[("schema", Some(schema))])).is_err(),
                "accepted schema {schema}"
            );
        }
        // A key the helper does not write. Tolerating it is what let a forged
        // document pass as a measurement, so it is now a refusal.
        assert!(
            parse_helper_manifest(&manifest_bytes(&[("stacks_forged", Some("1"))])).is_err(),
            "accepted an unknown key"
        );
        // Every key the helper always writes is required: an absent one used to
        // default to a clean zero, and the three `number | null` keys still
        // would if presence were left to `serde`.
        for key in HELPER_MANIFEST_KEYS {
            assert!(
                parse_helper_manifest(&manifest_bytes(&[(key, None)])).is_err(),
                "accepted a manifest with no {key}"
            );
        }
        // The two documents the old reader turned into a clean set of counters.
        assert!(parse_helper_manifest(b"").is_err());
        assert!(parse_helper_manifest(b"{}").is_err());
    }

    #[test]
    fn a_manifest_that_contradicts_the_artifact_is_refused() -> Result<(), String> {
        let options = profile_options()?;
        let execution = execution()?;
        let identity = identity()?;
        let observe = |manifest: Vec<u8>| {
            profile_observation(
                &options,
                &sampled_output(manifest),
                identity.clone(),
                &execution,
            )
        };
        // The base agrees with the two-stack, four-sample artifact.
        assert!(observe(manifest_bytes(&[])).is_ok());
        // Both cross-checks, in both directions. These catch a helper that
        // merely counts wrong just as readily as a fabricated manifest.
        for (key, value) in [
            ("stacks_written", "1"),
            ("stacks_written", "3"),
            ("samples_collected", "3"),
            ("samples_collected", "5"),
            ("samples_collected", "812"),
        ] {
            assert!(
                observe(manifest_bytes(&[(key, Some(value))])).is_err(),
                "published a manifest claiming {key} = {value} over a 2-stack, \
                 4-sample artifact"
            );
        }
        // The request this host itself put on the helper's argv, echoed back.
        for (key, value) in [
            ("frequency_hz", "98"),
            ("requested_duration_ms", "4000"),
            ("max_depth", "128"),
        ] {
            assert!(
                observe(manifest_bytes(&[(key, Some(value))])).is_err(),
                "published a manifest whose {key} is not the one requested"
            );
        }
        // A namespace the helper could not empty means the artifacts were
        // written while something else could still rewrite them.
        assert!(
            observe(manifest_bytes(&[("namespace_drained", Some("false"))])).is_err(),
            "published a run whose helper declared an undrained namespace"
        );
        // More descendants than the container's own pid limit allows is not a
        // count of this run's processes.
        assert!(
            observe(manifest_bytes(&[("descendants_reaped", Some("129"))])).is_err(),
            "published a descendant count the pid limit forbids"
        );
        assert!(observe(manifest_bytes(&[("descendants_reaped", Some("128"))])).is_ok());
        // Samples without an armed event, and an errno on a run nothing denied.
        assert!(observe(manifest_bytes(&[("cpus_sampled", Some("0"))])).is_err());
        assert!(observe(manifest_bytes(&[("perf_errno", Some("1"))])).is_err());
        // A stacks artifact this host cannot parse leaves the counters with
        // nothing to answer to, so they are refused rather than published
        // beside an empty graph.
        let unparseable = ProfileOutput {
            stacks: b"main;work not-a-count\n".to_vec(),
            ..sampled_output(manifest_bytes(&[]))
        };
        assert!(
            profile_observation(&options, &unparseable, identity.clone(), &execution).is_err(),
            "published counters over a stacks artifact that did not parse"
        );
        Ok(())
    }

    #[test]
    fn a_denial_is_reportable_only_with_the_errno_that_produced_it() -> Result<(), String> {
        let options = profile_options()?;
        let execution = execution()?;
        let denial = |edits: &[(&str, Option<&str>)]| {
            let mut fields = vec![
                ("status", Some("\"profiler_unavailable\"")),
                ("observed_duration_ms", Some("0")),
                ("samples_collected", Some("0")),
                ("stacks_written", Some("0")),
                ("frames_total", Some("0")),
                ("modules_seen", Some("0")),
                ("cpus_sampled", Some("0")),
                ("child_exit_code", Some("null")),
                ("child_signal", Some("9")),
                ("perf_errno", Some("1")),
            ];
            fields.extend_from_slice(edits);
            ProfileOutput {
                // The helper writes an empty stacks file on this path; the
                // guest's sampler never armed an event.
                stacks: Vec::new(),
                ..sampled_output(manifest_bytes(&fields))
            }
        };
        let observed = profile_observation(&options, &denial(&[]), identity()?, &execution)
            .map_err(|error| format!("a well-formed denial was refused: {error:?}"))?;
        assert_eq!(observed.status, ProfileStatus::ProfilerUnavailable);
        assert_eq!(observed.completeness, ProfileCompleteness::Unavailable);
        assert_eq!(observed.perf_errno, Some(1));
        assert!(observed.svg.is_empty());
        assert!(observed.stacks.is_empty());
        assert!(observed.top_frames.is_empty());
        assert!(observed.consistent());

        // ADR-074 §3: without the errno there is no denial to report, and a
        // denial that claims to have sampled something is not one either.
        for edit in [
            ("perf_errno", Some("null")),
            ("cpus_sampled", Some("4")),
            ("samples_collected", Some("2")),
            ("stacks_written", Some("2")),
        ] {
            assert!(
                profile_observation(&options, &denial(&[edit]), identity()?, &execution).is_err(),
                "published a denial with {} = {:?}",
                edit.0,
                edit.1
            );
        }
        Ok(())
    }

    #[test]
    fn the_helper_status_vocabulary_is_closed() {
        assert_eq!(helper_status("complete"), ProfileStatus::Complete);
        assert_eq!(helper_status("sample_limit"), ProfileStatus::SampleLimit);
        assert_eq!(
            helper_status("duration_limit"),
            ProfileStatus::DurationLimit
        );
        assert_eq!(helper_status("child_exited"), ProfileStatus::ChildExited);
        assert_eq!(
            helper_status("profiler_unavailable"),
            ProfileStatus::ProfilerUnavailable
        );
        // An unknown or absent status never becomes a successful profile.
        assert_eq!(helper_status(""), ProfileStatus::ProfilerUnavailable);
        assert_eq!(helper_status("ok"), ProfileStatus::ProfilerUnavailable);
    }

    #[test]
    fn completeness_declares_every_loss_the_counters_report() {
        let counters = |collected, lost, truncated| ProfileCounters {
            samples_collected: collected,
            samples_lost: lost,
            stacks_truncated: truncated,
            ..ProfileCounters::default()
        };
        assert_eq!(
            profile_completeness(ProfileStatus::ProfilerUnavailable, &counters(0, 0, 0)),
            ProfileCompleteness::Unavailable
        );
        assert_eq!(
            profile_completeness(ProfileStatus::ChildExited, &counters(0, 0, 0)),
            ProfileCompleteness::NoSamples
        );
        assert_eq!(
            profile_completeness(ProfileStatus::Complete, &counters(10, 2, 0)),
            ProfileCompleteness::LostSamples
        );
        assert_eq!(
            profile_completeness(ProfileStatus::Complete, &counters(10, 0, 3)),
            ProfileCompleteness::Truncated
        );
        assert_eq!(
            profile_completeness(ProfileStatus::Complete, &counters(10, 0, 0)),
            ProfileCompleteness::Complete
        );
    }

    #[test]
    fn a_sampled_profile_publishes_a_graph_of_the_stacks_it_parsed() -> Result<(), String> {
        let options = profile_options()?;
        let execution = execution()?;
        let sampled = sampled_output(manifest_bytes(&[]));
        let observed = profile_observation(&options, &sampled, identity()?, &execution)
            .map_err(|error| format!("the base profile was refused: {error:?}"))?;
        assert_eq!(observed.completeness, ProfileCompleteness::Complete);
        assert_eq!(observed.counters.samples_collected, 4);
        assert_eq!(observed.counters.max_depth_applied, 127);
        assert_eq!(observed.child.exit_code, Some(0));
        assert!(!observed.svg.is_empty());
        assert!(!observed.stacks.is_empty());
        assert!(observed.top_frames.len() <= TOP_FRAMES);
        assert!(observed.consistent());
        assert_eq!(observed.backend, PROFILE_BACKEND);
        let svg = String::from_utf8_lossy(&observed.svg);
        for forbidden in ["<script", "xlink:href", "href=", "<foreignObject", "<image"] {
            assert!(!svg.contains(forbidden), "svg carried {forbidden}");
        }
        // A build that never ran reports the build's own capture, and nothing
        // claims a profile was taken.
        let unbuilt = ProfileOutput {
            build: capture(Some(101), b""),
            run: None,
            stacks: Vec::new(),
            manifest: Vec::new(),
        };
        // No helper ran, so there is no manifest to read and none is demanded:
        // the empty document is a fact about the build, not a forgery.
        let observed = profile_observation(&options, &unbuilt, identity()?, &execution)
            .map_err(|error| format!("an unbuilt target was refused: {error:?}"))?;
        assert_eq!(observed.build, ProfileBuildOutcome::CompilationFailed);
        assert_eq!(observed.build_exit_code, Some(101));
        // Never a profiler denial: there is no errno, because the sampler was
        // never reached. The application refuses a denial without one, and this
        // is a project failure, not a denial.
        assert_eq!(observed.status, ProfileStatus::ChildExited);
        assert_eq!(observed.completeness, ProfileCompleteness::NoSamples);
        assert_eq!(observed.perf_errno, None);
        assert!(observed.svg.is_empty());
        assert!(observed.stacks.is_empty());
        assert!(observed.consistent());
        Ok(())
    }

    #[test]
    fn a_failed_build_is_a_project_outcome_not_a_profiler_outcome() {
        assert_eq!(
            build_outcome(&capture(Some(0), b"")),
            ProfileBuildOutcome::Built
        );
        let mut missing = capture(Some(101), b"");
        missing.stderr = b"error: no bin target named `ghost`".to_vec();
        assert_eq!(build_outcome(&missing), ProfileBuildOutcome::TargetNotFound);
        let mut broken = capture(Some(101), b"");
        broken.stderr = b"error[E0432]: unresolved import".to_vec();
        assert_eq!(
            build_outcome(&broken),
            ProfileBuildOutcome::CompilationFailed
        );
    }

    #[test]
    fn frame_ranking_is_bounded_and_carries_self_and_total_weight() -> Result<(), String> {
        let folded = profile_stacks::parse_folded(b"a;b 3\na;c 1\n")
            .map_err(|error| format!("{error:?}"))?;
        let weights = frame_weights(&folded);
        assert!(weights.len() <= TOP_FRAMES);
        let top = weights.first().ok_or("empty ranking")?;
        assert_eq!(top.frame, "a");
        assert_eq!(top.total_samples, 4);
        assert_eq!(top.self_samples, 0);
        Ok(())
    }

    #[test]
    fn the_products_own_measurements_are_the_oracle_against_the_analyzer() {
        assert_eq!(measured_size(&capture(Some(0), b"4096\n")), Some(4096));
        assert_eq!(measured_size(&capture(Some(1), b"4096\n")), None);
        assert_eq!(measured_size(&capture(Some(0), b"not a number")), None);
        let hex = "a".repeat(64);
        assert_eq!(
            measured_digest(&capture(
                Some(0),
                format!("{hex}  /work/target/release/workload\n").as_bytes()
            )),
            Some(format!("sha256:{hex}"))
        );
        assert_eq!(measured_digest(&capture(Some(0), b"short  /path\n")), None);
        assert_eq!(measured_digest(&capture(Some(1), b"")), None);
        let elf = b"ELF Header:\n  Class:                             ELF64\n  Machine:                           AArch64\n";
        assert_eq!(
            measured_format(&capture(Some(0), elf)),
            BinaryFormat::Elf64Aarch64
        );
        let other = b"ELF Header:\n  Class:                             ELF32\n  Machine:                           ARM\n";
        assert_eq!(
            measured_format(&capture(Some(0), other)),
            BinaryFormat::OtherElf
        );
        assert_eq!(
            measured_format(&capture(Some(1), b"readelf: Error: Not an ELF file\n")),
            BinaryFormat::Unknown
        );
        assert_eq!(
            measured_format(&capture(Some(0), b"")),
            BinaryFormat::Unknown
        );
    }

    fn bloat_output(size: &[u8], functions: &[u8], crates: &[u8]) -> BloatOutput {
        BloatOutput {
            functions: capture(Some(0), functions),
            crates: capture(Some(0), crates),
            size: capture(Some(0), size),
            digest: capture(Some(0), format!("{}  /x\n", "b".repeat(64)).as_bytes()),
            header: capture(
                Some(0),
                b"ELF Header:\n  Class: ELF64\n  Machine: AArch64\n",
            ),
        }
    }

    const FUNCTIONS: &[u8] = br#"{"file-size":4096,"text-section-size":2048,
        "functions":[{"crate":"fixture","name":"main","size":128}]}"#;
    const CRATES: &[u8] = br#"{"file-size":4096,"text-section-size":2048,
        "crates":[{"name":"fixture","size":512}]}"#;

    #[test]
    fn the_exact_size_and_the_estimated_attribution_are_never_merged() {
        let output = bloat_output(b"4096\n", FUNCTIONS, CRATES);
        let measured = measured_binary(&output);
        let attributed = attribution(&output);
        assert_eq!(
            measured.as_ref().map(|measured| measured.size_bytes),
            Some(4096)
        );
        // Always an analysis build: the analyzer forces the symbol table on.
        assert!(
            measured
                .as_ref()
                .is_some_and(|measured| measured.analysis_build_symbols_forced)
        );
        assert!(
            attributed
                .as_ref()
                .is_some_and(|attribution| attribution.estimated)
        );
        assert_eq!(
            attributed
                .as_ref()
                .and_then(|attribution| attribution.reported_file_size_bytes),
            Some(4096)
        );
        assert_eq!(
            bloat_completeness(measured.as_ref(), attributed.as_ref()),
            BloatCompleteness::Complete
        );
        // The analyzer's own file size disagreeing with ours is never Complete.
        let mismatched = bloat_output(b"4095\n", FUNCTIONS, CRATES);
        assert_eq!(
            bloat_completeness(
                measured_binary(&mismatched).as_ref(),
                attribution(&mismatched).as_ref()
            ),
            BloatCompleteness::SizeMismatch
        );
        // The two views disagreeing with each other yields no attribution.
        const OTHER_CRATES: &[u8] = br#"{"file-size":9999,"text-section-size":2048,
            "crates":[{"name":"fixture","size":512}]}"#;
        let inconsistent = bloat_output(b"4096\n", FUNCTIONS, OTHER_CRATES);
        assert_eq!(attribution(&inconsistent), None);
        assert_eq!(
            bloat_completeness(
                measured_binary(&inconsistent).as_ref(),
                attribution(&inconsistent).as_ref()
            ),
            BloatCompleteness::Unavailable
        );
    }

    #[test]
    fn a_binary_the_product_cannot_measure_is_never_reported_as_measured() {
        let mut output = bloat_output(b"4096\n", FUNCTIONS, CRATES);
        output.size = capture(Some(1), b"");
        assert_eq!(measured_binary(&output), None);
        assert_eq!(
            bloat_completeness(None, attribution(&output).as_ref()),
            BloatCompleteness::Unavailable
        );
        let mut foreign = bloat_output(b"4096\n", FUNCTIONS, CRATES);
        foreign.header = capture(Some(1), b"readelf: Error: Not an ELF file\n");
        assert_eq!(
            bloat_completeness(
                measured_binary(&foreign).as_ref(),
                attribution(&foreign).as_ref()
            ),
            BloatCompleteness::UnsupportedFormat
        );
        // A refused analyzer leaves the exact measurement standing on its own.
        let mut refused = bloat_output(b"4096\n", FUNCTIONS, CRATES);
        refused.functions = capture(Some(1), b"");
        refused.crates = capture(Some(1), b"");
        assert_eq!(attribution(&refused), None);
        assert_eq!(
            bloat_completeness(
                measured_binary(&refused).as_ref(),
                attribution(&refused).as_ref()
            ),
            BloatCompleteness::Unavailable
        );
    }
}
