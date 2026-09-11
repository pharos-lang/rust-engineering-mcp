//! Closed five-volume performance execution for M5 (ADR-073/074/075/076).
//!
//! One gateway serves the three M5 tools that run a process:
//! `rust.benchmark.run`, `rust.profile.flamegraph` and `rust.binary.bloat`.
//! `rust.benchmark.compare` runs nothing and never reaches this module.
//!
//! The shape is [`crate::security_gateway`]'s: one phase enum, one orchestrator
//! parameterized by the operation, guardian containers holding every named
//! tmpfs volume, ingest by `tar` on stdin, and a joined cleanup on every exit
//! path. Two volumes exist here that M4 did not need:
//!
//! * `/work/target` — an executable build volume shared between the phases of
//!   one operation. `cargo-bloat`'s own measurement and the product's
//!   independent `stat`/`sha256sum`/`readelf` oracle must observe the *same*
//!   file, and the profiler must execute the binary a previous phase built.
//!   A per-container `/work` tmpfs cannot carry a file across containers.
//! * `/performance` — the vendor-backed `CARGO_HOME`, ingested exactly as
//!   ADR-067 ingests `/security/cargo-home`, so `--frozen --offline` resolves
//!   against `/rust-mcp-vendor` and never against the network.
use super::*;
use crate::mutation_gateway::{
    MutationVolume, VOLUME_OPTIONS, absent, cleanup_until, cleanup_until_with_options, labels,
    mutation_control, parse_volume_with_options, query_control, remove_if_present, running,
    start_attached, start_attached_source,
};
use crate::performance_environment::{self, GovernorPaths, GuestCpuTopology};
use crate::rust_applied::{AppliedPhaseExpectation, AppliedVolumeExpectation};
use crate::rust_gateway::RustGateway;
use rust_engineering_application::vendor_capture::{BenchmarkVendor, VerifiedVendorCapture};
use rust_engineering_application::{InspectionError, ProjectError};
use rust_engineering_domain::benchmark::BenchmarkSelection;
use rust_engineering_domain::benchmark_run::HarnessDetection;
use rust_engineering_domain::bloat::{BloatOptions, BloatProfile};
use rust_engineering_domain::profile::ProfileOptions;
use rust_engineering_domain::vendor_capture::VENDOR_CAPTURE_READ_BUFFER_BYTES;
use rust_engineering_domain::{
    CargoVendorSnapshot, ExecutionFingerprint, ExecutionLimits, SourceBundle, SourceFile,
    SourceFingerprint,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

const CLEANUP: Duration = Duration::from_secs(10);
const METADATA_OUTPUT: usize = 1024 * 1024;
const PROBE_OUTPUT: usize = performance_environment::PROBE_CAPTURE_BYTES;
/// ADR-076 §7 artifact ceilings: bloat ≤ 4 MiB, muestras ≤ 32 MiB.
const BLOAT_OUTPUT: usize = 4 * 1024 * 1024;
const PROFILE_ARCHIVE_OUTPUT: usize = 33 * 1024 * 1024;
/// Exactly [`crate::criterion_dataset::MAX_CRITERION_ARCHIVE`]: an export the
/// dataset parser would refuse is stopped here, as an output-limit failure,
/// instead of being carried and then rejected.
const CRITERION_ARCHIVE_OUTPUT: usize = 32 * 1024 * 1024;
const MEASUREMENT_OUTPUT: usize = 4 * 1024;

const CONFIG_ROOT: &str = "/performance";
const VENDOR_ROOT: &str = "/rust-mcp-vendor";
const TARGET_ROOT: &str = "/work/target";
const CRITERION_ROOT: &str = "/criterion";
const PROFILE_ROOT: &str = "/profile";
const CARGO_HOME: &str = "CARGO_HOME=/performance/cargo-home";

/// The authoritative vendor selection, passed on the command line rather than
/// left to a config file.
///
/// A `CARGO_HOME` config is overridden by a `.cargo/config.toml` inside the
/// project, so a project could redirect `source.crates-io` at a directory it
/// controls and substitute the very dependency bytes the measurement is about
/// to describe — or install a `runner`, a `linker` or `rustflags`, which G2
/// forbids outright. Cargo gives `--config` the highest precedence, above the
/// environment and above every config file, so this selection cannot be
/// overridden. The two strings are literals owned by this product; nothing in
/// them comes from the caller. [`reject_project_cargo_configuration`] refuses
/// the project config as well, so this is defence in depth, not the only line.
///
/// It names the source the ingested `CARGO_HOME` config already declares
/// (`security_policy::SECURITY_CARGO_CONFIG`) instead of introducing a second
/// one. Declaring a second name for the same directory is not a redundancy but
/// a hard error — Cargo answers `source ... defines source dir /rust-mcp-vendor,
/// but that source is already defined by ...; Sources are not allowed to be
/// defined multiple times` — which is also what a project attempting to
/// redefine it would get. One override, highest precedence, fail-closed.
const VENDOR_SELECTION: [&str; 2] = [
    "--config",
    "source.crates-io.replace-with=\"rust-mcp-vendor\"",
];

fn with_vendor_selection(subcommand: &str, rest: &[&str]) -> Vec<String> {
    let mut arguments = vec![subcommand.to_owned()];
    arguments.extend(VENDOR_SELECTION.map(str::to_owned));
    arguments.extend(rest.iter().map(|value| (*value).to_owned()));
    arguments
}

/// ADR-067's rule, applied to the three measuring tools: a captured project
/// that carries its own Cargo configuration is refused before any volume
/// exists, because that file decides which sources, linker and rustflags the
/// measurement would have used.
fn reject_project_cargo_configuration(source: &SourceBundle) -> Result<(), PerformanceError> {
    if rust_engineering_domain::security::source_has_cargo_configuration(source) {
        return Err(PerformanceError::ProjectCargoConfiguration);
    }
    Ok(())
}

/// ADR-065's executable target volume options, reused verbatim: the same inode
/// density and the same 512 MiB ceiling as the qualified `/work` build tmpfs,
/// with `noexec` removed because cargo executes build scripts and the profiler
/// executes the binary it built.
pub(super) const TARGET_VOLUME_OPTIONS: &str =
    "size=512m,nr_inodes=65536,uid=65534,gid=65534,mode=0700,nosuid,nodev";

/// ADR-078's capture ceilings expressed as the tmpfs that receives the
/// authenticated tree. The remaining ownership, mode and hardening options
/// stay identical to [`VOLUME_OPTIONS`].
pub(super) const VENDOR_CAPTURE_VOLUME_OPTIONS: &str =
    "size=512m,nr_inodes=32768,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec";

/// ADR-076 §7. These are wall budgets for the whole operation, control plane
/// included; [`work_budget_ms`] takes the control reserve out before any phase
/// is given a share.
pub(super) const BENCHMARK_BUDGET_MS: u64 = 900_000;
pub(super) const PROFILE_BUDGET_MS: u64 = 300_000;
pub(super) const BLOAT_BUDGET_MS: u64 = 300_000;
/// ADR-074 §4 ceiling on the sampling window itself.
pub(super) const PROFILE_MAX_SAMPLING_MS: u64 = 60_000;

/// `fixtures/profile-helper` exit 0: it profiled, and both artifacts are on the
/// output volume.
const HELPER_EXIT_PROFILED: i32 = 0;
/// `fixtures/profile-helper` exit 3: `perf_event_open` was refused, and an
/// empty stacks file stands beside a manifest carrying the errno that refused
/// it. Every other exit leaves the volume in a state this gateway does not
/// export.
const HELPER_EXIT_PROFILER_UNAVAILABLE: i32 = 3;

// Whether the sampling phase must run under a seccomp profile that refuses
// `perf_event_open`. Always `false` outside `cfg(test)`, where it compiles to a
// constant and no shipped binary carries the switch at all.
//
// ADR-074's `denial_control` is the mandatory negative oracle for
// `rust.profile.flamegraph`, and the seccomp profile the sampling phase names
// is the only mechanism in this product that denies `perf_event_open`: no
// caller, option, project or fixture can reach it, because the whole point of
// the containment is that the guest cannot influence its own policy. Without
// this switch the oracle can only be produced by running docker by hand beside
// the product, which is what `docs/validation/M5-03-profiling-native.json`
// records and which is not a receipt of the product path. With it, the native
// qualification drives the denial through `performance_port::profile`
// unchanged — same phases, same argv, same `verify_applied` comparison — and
// the only difference is which of the two committed profiles
// `PerformancePhase::ProfileRun` selects.
#[cfg(test)]
fn perf_event_open_denied() -> bool {
    DENY_PERF_EVENT_OPEN.with(std::cell::Cell::get)
}

#[cfg(not(test))]
const fn perf_event_open_denied() -> bool {
    false
}

#[cfg(test)]
thread_local! {
    /// Test-only switch behind [`perf_event_open_denied`].
    ///
    /// Thread-local rather than global, so a selection that arms it cannot
    /// perturb any test running beside it: this gateway builds every argv,
    /// computes every fingerprint and performs every `verify_applied`
    /// comparison on the thread that called it, and spawns no thread of its
    /// own. Arm it through an RAII guard so a failed assertion still disarms
    /// it.
    pub(super) static DENY_PERF_EVENT_OPEN: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// ADR-073 §2's frozen harness parameters. The server fixes them, they travel
/// in the provenance of every dataset, and the project cannot reach them: they
/// are argv, not configuration.
pub(super) const BENCHMARK_WARM_UP_MS: u64 = 3_000;
pub(super) const BENCHMARK_MEASUREMENT_MS: u64 = 5_000;
pub(super) const BENCHMARK_SAMPLE_SIZE: u32 = 30;

/// Remaining control allowance: 84 round trips x 250 ms + 2 s startup +
/// 1 s output validation + 10 s joined cleanup, rounded up. `GovernorProbe`
/// adds one normal non-interactive phase (eight control round trips) to the
/// prior accounting. The five volumes
/// and five guardians of this gateway cost more round trips than ADR-069's
/// three-volume scan, so the reserve is larger than `SCAN_CONTROL_RESERVE_MS`.
const CONTROL_RESERVE_MS: u64 = 34_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum PerformanceKind {
    Benchmark,
    Profile,
    Bloat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) enum PerformancePhase {
    SourceGuardian,
    VendorGuardian,
    ConfigGuardian,
    TargetGuardian,
    OutputGuardian,
    SourceIngest,
    VendorIngest,
    ConfigIngest,
    Metadata,
    CpuProbe,
    KernelProbe,
    GovernorProbe,
    BenchRun,
    BenchExport,
    ProfileBuild,
    ProfileRun,
    ProfileExport,
    BloatFunctions,
    BloatCrates,
    BloatFileSize,
    BloatFileDigest,
    BloatFileHeader,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub(super) struct PerformanceMounts {
    pub(super) source: bool,
    pub(super) vendor: bool,
    pub(super) config: bool,
    pub(super) target: bool,
    pub(super) output: bool,
}

/// Writability is a strict subset of [`PerformanceMounts`]: a volume is never
/// writable in a phase that does not mount it.
type PerformancePermissions = PerformanceMounts;

#[derive(Clone, Copy)]
pub(super) enum PerformanceOperation<'a> {
    Benchmark {
        selection: &'a BenchmarkSelection,
        run_count: u8,
        /// Whether the resolved graph carries the approved harness. The frozen
        /// warmup, measurement window and sample size are Criterion's own
        /// options; a project on the default libtest harness rejects them
        /// outright (`error: Unrecognized option: 'noplot'`, exit 101), so
        /// sending them there would manufacture a failure that says nothing
        /// about the project. When the harness is not the approved one the run
        /// still happens and its logs are still reported (ADR-073 §1), just
        /// without options only Criterion understands.
        harness_parameters: bool,
    },
    Profile(&'a ProfileOptions),
    Bloat(&'a BloatOptions),
}

impl PerformanceOperation<'_> {
    pub(super) fn kind(self) -> PerformanceKind {
        match self {
            Self::Benchmark { .. } => PerformanceKind::Benchmark,
            Self::Profile(_) => PerformanceKind::Profile,
            Self::Bloat(_) => PerformanceKind::Bloat,
        }
    }
    /// Every phase this operation creates, in execution order. The list is the
    /// fingerprint input, so a phase added or removed changes the identity of
    /// every result the gateway publishes.
    pub(super) fn phases(self) -> Vec<PerformancePhase> {
        let mut phases = vec![
            PerformancePhase::SourceGuardian,
            PerformancePhase::VendorGuardian,
            PerformancePhase::ConfigGuardian,
            PerformancePhase::TargetGuardian,
            PerformancePhase::SourceIngest,
            PerformancePhase::VendorIngest,
            PerformancePhase::ConfigIngest,
            PerformancePhase::Metadata,
            PerformancePhase::CpuProbe,
            PerformancePhase::KernelProbe,
            PerformancePhase::GovernorProbe,
        ];
        match self.kind() {
            PerformanceKind::Benchmark => phases.extend([
                PerformancePhase::OutputGuardian,
                PerformancePhase::BenchRun,
                PerformancePhase::BenchExport,
            ]),
            PerformanceKind::Profile => phases.extend([
                PerformancePhase::OutputGuardian,
                PerformancePhase::ProfileBuild,
                PerformancePhase::ProfileRun,
                PerformancePhase::ProfileExport,
            ]),
            PerformanceKind::Bloat => phases.extend([
                PerformancePhase::BloatFunctions,
                PerformancePhase::BloatCrates,
                PerformancePhase::BloatFileSize,
                PerformancePhase::BloatFileDigest,
                PerformancePhase::BloatFileHeader,
            ]),
        }
        phases
    }
}

/// The guest path of the binary under analysis. Built from the closed target
/// name, never from tool output (ADR-076 §6).
///
/// Both profiles build into `release`: LTO is expressed as an environment
/// variable over the `release` profile rather than as a profile named
/// `release-lto`, because `cargo-bloat` 0.12.1 derives `CARGO_PROFILE_<NAME>_*`
/// from the profile name and `CARGO_PROFILE_RELEASE_LTO` would then be read by
/// Cargo as `profile.release.lto` and rejected (calibrated in
/// `docs/validation/M5-04-bloat-calibration.json`).
fn binary_path(target: &str) -> String {
    format!("{TARGET_ROOT}/release/{target}")
}

fn output_root(kind: PerformanceKind) -> &'static str {
    match kind {
        PerformanceKind::Benchmark => CRITERION_ROOT,
        PerformanceKind::Profile => PROFILE_ROOT,
        // Bloat publishes no volume-backed artifact; its evidence is the
        // analyzer's stdout and the product's own measurements of the file.
        PerformanceKind::Bloat => CRITERION_ROOT,
    }
}

impl PerformancePhase {
    pub(super) fn program(self) -> &'static str {
        match self {
            Self::SourceGuardian
            | Self::VendorGuardian
            | Self::ConfigGuardian
            | Self::TargetGuardian
            | Self::OutputGuardian => "/usr/bin/sleep",
            Self::SourceIngest
            | Self::VendorIngest
            | Self::ConfigIngest
            | Self::BenchExport
            | Self::ProfileExport => "/usr/bin/tar",
            Self::Metadata | Self::BenchRun | Self::ProfileBuild => "/opt/rust/bin/cargo",
            Self::CpuProbe | Self::GovernorProbe => "/usr/bin/cat",
            Self::KernelProbe => "/usr/bin/uname",
            Self::ProfileRun => "/opt/perf/bin/rust-mcp-profile-helper",
            Self::BloatFunctions | Self::BloatCrates => "/opt/perf/bin/cargo-bloat",
            Self::BloatFileSize => "/usr/bin/stat",
            Self::BloatFileDigest => "/usr/bin/sha256sum",
            Self::BloatFileHeader => "/usr/bin/readelf",
        }
    }

    pub(super) fn arguments(self, operation: PerformanceOperation<'_>) -> Vec<String> {
        let fixed: &[&str] = match self {
            Self::SourceGuardian
            | Self::VendorGuardian
            | Self::ConfigGuardian
            | Self::TargetGuardian
            | Self::OutputGuardian => &["3600"],
            Self::SourceIngest => &[
                "--extract",
                "--file=-",
                "--directory=/source",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::VendorIngest => &[
                "--extract",
                "--file=-",
                "--directory=/rust-mcp-vendor",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            Self::ConfigIngest => &[
                "--extract",
                "--file=-",
                "--directory=/performance",
                "--no-same-owner",
                "--no-same-permissions",
                "--keep-old-files",
            ],
            // No `--no-deps`: the resolved graph is what proves which harness
            // and which harness version this project actually builds against.
            Self::Metadata => &[
                "metadata",
                "--frozen",
                "--offline",
                "--format-version=1",
                "--manifest-path=/source/Cargo.toml",
            ],
            Self::CpuProbe => &["/proc/cpuinfo"],
            Self::KernelProbe => &["-s", "-r", "-m"],
            // The actual, topology-derived paths are accepted only by the
            // dedicated governor phase below. This branch intentionally never
            // manufactures an argv for a general phase invocation.
            Self::GovernorProbe => &[],
            Self::BenchExport => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--directory=/criterion",
                ".",
            ],
            Self::ProfileExport => &[
                "--create",
                "--file=-",
                "--format=ustar",
                "--no-recursion",
                "--directory=/profile",
                "stacks.txt",
                "manifest.json",
            ],
            Self::BenchRun | Self::ProfileBuild | Self::ProfileRun => &[],
            Self::BloatFunctions
            | Self::BloatCrates
            | Self::BloatFileSize
            | Self::BloatFileDigest
            | Self::BloatFileHeader => &[],
        };
        if !fixed.is_empty() {
            return fixed.iter().map(|value| (*value).to_owned()).collect();
        }
        match (self, operation) {
            (
                Self::BenchRun,
                PerformanceOperation::Benchmark {
                    selection,
                    harness_parameters,
                    ..
                },
            ) => bench_arguments(selection, harness_parameters),
            (Self::ProfileBuild, PerformanceOperation::Profile(options)) => {
                let mut arguments = with_vendor_selection(
                    "build",
                    &[
                        "--release",
                        "--frozen",
                        "--offline",
                        "--color=never",
                        "--target-dir=/work/target",
                    ],
                );
                arguments.push(format!("--bin={}", options.binary_target()));
                arguments
            }
            (Self::ProfileRun, PerformanceOperation::Profile(options)) => vec![
                "--frequency-hz".into(),
                options.frequency_hz().to_string(),
                "--duration-ms".into(),
                options
                    .duration_ms()
                    .min(PROFILE_MAX_SAMPLING_MS)
                    .to_string(),
                "--max-samples".into(),
                rust_engineering_domain::profile::PROFILE_MAX_SAMPLES.to_string(),
                "--max-depth".into(),
                rust_engineering_domain::profile::PROFILE_MAX_DEPTH.to_string(),
                "--stacks".into(),
                format!("{PROFILE_ROOT}/stacks.txt"),
                "--manifest".into(),
                format!("{PROFILE_ROOT}/manifest.json"),
                "--".into(),
                binary_path(options.binary_target()),
            ],
            (Self::BloatFunctions, PerformanceOperation::Bloat(options)) => {
                bloat_arguments(options, false)
            }
            (Self::BloatCrates, PerformanceOperation::Bloat(options)) => {
                bloat_arguments(options, true)
            }
            (Self::BloatFileSize, PerformanceOperation::Bloat(options)) => {
                vec!["--format=%s".into(), binary_path(options.binary_target())]
            }
            (Self::BloatFileDigest, PerformanceOperation::Bloat(options)) => {
                vec![binary_path(options.binary_target())]
            }
            (Self::BloatFileHeader, PerformanceOperation::Bloat(options)) => {
                vec!["-h".into(), binary_path(options.binary_target())]
            }
            // A phase is never created for an operation that does not list it;
            // an empty argv here would be a programming error, not a command.
            _ => Vec::new(),
        }
    }

    pub(super) fn interactive(self) -> bool {
        matches!(
            self,
            Self::SourceIngest | Self::VendorIngest | Self::ConfigIngest
        )
    }

    pub(super) fn mounts(self) -> PerformanceMounts {
        let all = PerformanceMounts {
            source: true,
            vendor: true,
            config: true,
            target: false,
            output: false,
        };
        match self {
            Self::SourceGuardian | Self::SourceIngest => PerformanceMounts {
                source: true,
                ..PerformanceMounts::default()
            },
            Self::VendorGuardian | Self::VendorIngest => PerformanceMounts {
                vendor: true,
                ..PerformanceMounts::default()
            },
            Self::ConfigGuardian | Self::ConfigIngest => PerformanceMounts {
                config: true,
                ..PerformanceMounts::default()
            },
            Self::TargetGuardian => PerformanceMounts {
                target: true,
                ..PerformanceMounts::default()
            },
            Self::OutputGuardian | Self::BenchExport | Self::ProfileExport => PerformanceMounts {
                output: true,
                ..PerformanceMounts::default()
            },
            Self::CpuProbe | Self::KernelProbe | Self::GovernorProbe => {
                PerformanceMounts::default()
            }
            Self::Metadata => all,
            Self::BenchRun => PerformanceMounts {
                target: true,
                output: true,
                ..all
            },
            Self::ProfileBuild | Self::BloatFunctions | Self::BloatCrates => PerformanceMounts {
                target: true,
                ..all
            },
            Self::ProfileRun => PerformanceMounts {
                target: true,
                output: true,
                ..PerformanceMounts::default()
            },
            Self::BloatFileSize | Self::BloatFileDigest | Self::BloatFileHeader => {
                PerformanceMounts {
                    target: true,
                    ..PerformanceMounts::default()
                }
            }
        }
    }

    /// `/source`, `/rust-mcp-vendor` and `/performance` are writable in their
    /// own ingest and nowhere else. `/work/target` is writable only where cargo
    /// builds, and the output volume only where its producer writes it.
    pub(super) fn permissions(self) -> PerformancePermissions {
        PerformancePermissions {
            source: self == Self::SourceIngest,
            vendor: self == Self::VendorIngest,
            config: self == Self::ConfigIngest,
            target: matches!(
                self,
                Self::BenchRun | Self::ProfileBuild | Self::BloatFunctions | Self::BloatCrates
            ),
            output: matches!(self, Self::BenchRun | Self::ProfileRun),
        }
    }

    /// ADR-074 §3: the sampling phase, and only it, runs under the quality
    /// profile plus `perf_event_open`. Every other phase keeps the ADR-064
    /// quality profile it would otherwise have.
    fn profiling_profile(self) -> bool {
        self == Self::ProfileRun && !perf_event_open_denied()
    }

    pub(super) fn seccomp_profile_name(self) -> &'static str {
        if self.profiling_profile() {
            "seccomp-rust-profile.json"
        } else {
            "seccomp-rust-quality.json"
        }
    }

    pub(super) fn seccomp_profile_json(self) -> &'static str {
        if self.profiling_profile() {
            include_str!("seccomp-rust-profile.json")
        } else {
            include_str!("seccomp-rust-quality.json")
        }
    }

    pub(super) fn environment(self, operation: PerformanceOperation<'_>) -> Vec<String> {
        let mut environment = crate::rust_gateway::environment();
        if let Some(home) = environment
            .iter_mut()
            .find(|value| value.starts_with("CARGO_HOME="))
        {
            *home = CARGO_HOME.into();
        }
        if self == Self::BenchRun {
            environment.push(format!("CRITERION_HOME={CRITERION_ROOT}"));
        }
        if self == Self::ProfileBuild {
            environment.push("RUSTFLAGS=-C force-frame-pointers=yes".into());
        }
        // The only way to reach an LTO build through `cargo-bloat` 0.12.1; see
        // [`bloat_arguments`]. The value is the product's, never the caller's.
        if matches!(self, Self::BloatFunctions | Self::BloatCrates)
            && matches!(
                operation,
                PerformanceOperation::Bloat(options)
                    if options.profile() == BloatProfile::ReleaseLto
            )
        {
            environment.push("CARGO_PROFILE_RELEASE_LTO=fat".into());
        }
        environment.sort();
        environment
    }
}

fn bench_arguments(selection: &BenchmarkSelection, harness_parameters: bool) -> Vec<String> {
    let mut arguments = with_vendor_selection(
        "bench",
        &[
            "--frozen",
            "--offline",
            "--color=never",
            "--target-dir=/work/target",
        ],
    );
    if let Some(target) = &selection.bench_target {
        arguments.push(format!("--bench={target}"));
    }
    if let Some(package) = &selection.package {
        arguments.push(format!("--package={package}"));
    }
    if !selection.features.is_empty() {
        arguments.push(format!("--features={}", selection.features.join(",")));
    }
    if selection.all_features {
        arguments.push("--all-features".into());
    }
    if selection.no_default_features {
        arguments.push("--no-default-features".into());
    }
    if !harness_parameters {
        return arguments;
    }
    arguments.push("--".into());
    // ADR-073 §2: the server freezes warmup, measurement window and sample
    // size. They are argv, not configuration the project can reach.
    arguments.extend(["--noplot", "--color", "never"].map(str::to_owned));
    arguments.extend([
        "--warm-up-time".to_owned(),
        (BENCHMARK_WARM_UP_MS / 1000).to_string(),
        "--measurement-time".to_owned(),
        (BENCHMARK_MEASUREMENT_MS / 1000).to_string(),
        "--sample-size".to_owned(),
        BENCHMARK_SAMPLE_SIZE.to_string(),
    ]);
    arguments
}

/// `cargo-bloat` invoked directly refuses any first argument but `bloat`, and
/// it refuses `--profile release-lto` outright: it pushes
/// `CARGO_PROFILE_RELEASE_LTO`, which Cargo 1.98.1 reads as `profile.release.lto`
/// and rejects. Link-time optimization is therefore requested through the
/// product-owned environment of [`PerformancePhase::environment`], and the argv
/// stays `--release` for both profiles.
fn bloat_arguments(options: &BloatOptions, crates: bool) -> Vec<String> {
    let mut arguments = with_vendor_selection(
        "bloat",
        &[
            "--release",
            "--frozen",
            "--message-format",
            "json",
            "-n",
            "0",
        ],
    );
    arguments.push(format!("--bin={}", options.binary_target()));
    arguments.push(format!("--target-dir={TARGET_ROOT}"));
    if let Some(package) = options.package() {
        arguments.push(format!("--package={package}"));
    }
    if crates {
        arguments.push("--crates".into());
    }
    arguments
}

pub(super) struct PerformanceVolumes<'a> {
    pub(super) source: &'a MutationVolume,
    pub(super) vendor: &'a MutationVolume,
    pub(super) config: &'a MutationVolume,
    pub(super) target: &'a MutationVolume,
    pub(super) output: Option<&'a MutationVolume>,
}

fn mount_arguments(
    phase: PerformancePhase,
    volumes: &PerformanceVolumes<'_>,
    kind: PerformanceKind,
) -> Vec<String> {
    let mounted = phase.mounts();
    let writable = phase.permissions();
    let mut arguments = Vec::new();
    for (mounted, writable, volume, target) in [
        (
            mounted.source,
            writable.source,
            Some(volumes.source),
            "/source",
        ),
        (
            mounted.vendor,
            writable.vendor,
            Some(volumes.vendor),
            VENDOR_ROOT,
        ),
        (
            mounted.config,
            writable.config,
            Some(volumes.config),
            CONFIG_ROOT,
        ),
        (
            mounted.target,
            writable.target,
            Some(volumes.target),
            TARGET_ROOT,
        ),
        (
            mounted.output,
            writable.output,
            volumes.output,
            output_root(kind),
        ),
    ] {
        if let (true, Some(volume)) = (mounted, volume) {
            arguments.push(format!(
                "--mount=type=volume,source={},target={target},volume-nocopy,volume-driver=local{}",
                volume.name,
                if writable { "" } else { ",readonly" }
            ));
        }
    }
    arguments
}

fn create_arguments(
    gateway: &RustGateway,
    name: &str,
    operation_id: &str,
    volumes: &PerformanceVolumes<'_>,
    phase: PerformancePhase,
    operation: PerformanceOperation<'_>,
) -> Result<Vec<String>, ExecutionError> {
    create_arguments_for_runtime(
        gateway.image_id(),
        gateway.inner.state.path(),
        name,
        operation_id,
        volumes,
        phase,
        operation,
    )
}

fn create_arguments_for_runtime(
    image_id: &str,
    state_path: &std::path::Path,
    name: &str,
    operation_id: &str,
    volumes: &PerformanceVolumes<'_>,
    phase: PerformancePhase,
    operation: PerformanceOperation<'_>,
) -> Result<Vec<String>, ExecutionError> {
    let mut arguments = [
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
    arguments.push(format!("--name={name}"));
    for (key, value) in labels(operation_id) {
        arguments.push(format!("--label={key}={value}"));
    }
    for value in phase.environment(operation) {
        arguments.push(format!("--env={value}"));
    }
    let profile = state_path.join(phase.seccomp_profile_name());
    arguments.push(format!(
        "--security-opt=seccomp={}",
        profile
            .to_str()
            .ok_or(ExecutionError::InvalidConfiguration)?
    ));
    arguments.extend(mount_arguments(phase, volumes, operation.kind()));
    if phase.interactive() {
        arguments.push("--interactive".into());
    }
    arguments.push(format!("--entrypoint={}", phase.program()));
    arguments.push(image_id.into());
    arguments.extend(phase.arguments(operation));
    Ok(arguments)
}

/// Builds the only dynamic performance argv. Each path came from an unsigned
/// CPU ID parsed from the guest kernel's `/proc/cpuinfo`; no peer or project text
/// can reach this command.
fn governor_arguments_for_runtime(
    image_id: &str,
    state_path: &std::path::Path,
    name: &str,
    operation_id: &str,
    volumes: &PerformanceVolumes<'_>,
    operation: PerformanceOperation<'_>,
    paths: &GovernorPaths,
) -> Result<Vec<String>, ExecutionError> {
    if paths.as_slice().is_empty() {
        return Err(ExecutionError::InvalidConfiguration);
    }
    let mut arguments = create_arguments_for_runtime(
        image_id,
        state_path,
        name,
        operation_id,
        volumes,
        PerformancePhase::GovernorProbe,
        operation,
    )?;
    arguments.extend(paths.as_slice().iter().cloned());
    Ok(arguments)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PerformanceError {
    Inspection(InspectionError),
    Timeout,
    OutputLimit,
    InvalidMetadata,
    InvalidOptions,
    MissingOfflineData,
    /// The captured project carries `.cargo/config.toml` or `.cargo/config`.
    /// Distinct on purpose: the tool reports the real reason instead of a
    /// generic refusal (ADR-076 §3, and G2 on linker/runner/rustflags).
    ProjectCargoConfiguration,
}
impl From<InspectionError> for PerformanceError {
    fn from(value: InspectionError) -> Self {
        Self::Inspection(value)
    }
}
impl From<ExecutionError> for PerformanceError {
    fn from(value: ExecutionError) -> Self {
        Self::Inspection(if value == ExecutionError::Cancelled {
            InspectionError::Project(ProjectError::Cancelled)
        } else {
            InspectionError::Execution(value)
        })
    }
}
impl From<ProjectError> for PerformanceError {
    fn from(value: ProjectError) -> Self {
        Self::Inspection(InspectionError::Project(value))
    }
}

fn budget_error(
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), PerformanceError> {
    if cancel.is_cancelled() {
        Err(ExecutionError::Cancelled.into())
    } else if Instant::now() >= deadline {
        Err(PerformanceError::Timeout)
    } else {
        Ok(())
    }
}

fn phase_result<T>(
    result: Result<T, ExecutionError>,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<T, PerformanceError> {
    match result {
        Ok(value) => {
            budget_error(deadline, cancel)?;
            Ok(value)
        }
        Err(ExecutionError::CleanupUncertain) => Err(ExecutionError::CleanupUncertain.into()),
        Err(_) if cancel.is_cancelled() => Err(ExecutionError::Cancelled.into()),
        Err(_) if Instant::now() >= deadline => Err(PerformanceError::Timeout),
        Err(error) => Err(error.into()),
    }
}

/// The wall budget minus the control-plane reserve. Phases divide what is left;
/// the reserve is never lent to a phase, so a run that consumed its whole share
/// still leaves enough to join and remove the tree.
fn work_budget_ms(remaining_ms: u64) -> Result<u64, PerformanceError> {
    remaining_ms
        .checked_sub(CONTROL_RESERVE_MS)
        .filter(|value| *value > 0)
        .ok_or(PerformanceError::Timeout)
}

/// One phase's share of the work budget. `shares` is the number of executing
/// phases still to run, so an early phase can never spend a later phase's time.
fn share_ms(work_ms: u64, shares: u64) -> Result<u64, PerformanceError> {
    if shares == 0 {
        return Err(PerformanceError::Timeout);
    }
    let share = work_ms / shares;
    if share == 0 {
        return Err(PerformanceError::Timeout);
    }
    Ok(share)
}

fn remaining_ms(deadline: Instant) -> Result<u64, PerformanceError> {
    u64::try_from(
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis(),
    )
    .map_err(|_| PerformanceError::Timeout)
}

fn phase_deadline(
    deadline: Instant,
    shares: u64,
    cancel: &dyn ExecutionCancellation,
) -> Result<Instant, PerformanceError> {
    budget_error(deadline, cancel)?;
    let share = share_ms(work_budget_ms(remaining_ms(deadline)?)?, shares)?;
    Instant::now()
        .checked_add(Duration::from_millis(share))
        .filter(|value| *value <= deadline)
        .ok_or(PerformanceError::Timeout)
}

// -- applied configuration ---------------------------------------------------

/// Everything the daemon actually applied must equal what this phase asked for.
fn verify_applied(
    bytes: &[u8],
    image: &str,
    phase: PerformancePhase,
    volumes: &PerformanceVolumes<'_>,
    operation: PerformanceOperation<'_>,
    operation_id: &str,
) -> Result<(), ExecutionError> {
    let command = phase.arguments(operation);
    verify_applied_with_command(
        bytes,
        image,
        phase,
        volumes,
        operation,
        operation_id,
        &command,
    )
}

fn verify_applied_with_command(
    bytes: &[u8],
    image: &str,
    phase: PerformancePhase,
    volumes: &PerformanceVolumes<'_>,
    operation: PerformanceOperation<'_>,
    operation_id: &str,
    command: &[String],
) -> Result<(), ExecutionError> {
    let environment = phase.environment(operation);
    let mounts = applied_mount_expectations(phase, volumes, operation.kind())?;
    crate::rust_applied::verify_phase(
        bytes,
        &AppliedPhaseExpectation {
            image,
            operation_id,
            interactive: phase.interactive(),
            user: "65534:65534",
            program: phase.program(),
            command,
            environment: &environment,
            seccomp_profile: phase.seccomp_profile_json(),
            mounts,
        },
    )
}

fn applied_mount_expectations<'a>(
    phase: PerformancePhase,
    volumes: &PerformanceVolumes<'a>,
    kind: PerformanceKind,
) -> Result<Vec<AppliedVolumeExpectation<'a>>, ExecutionError> {
    let mounted = phase.mounts();
    let writable = phase.permissions();
    let mut expected = Vec::new();
    for (mounted, writable, volume, target) in [
        (
            mounted.source,
            writable.source,
            Some(volumes.source),
            "/source",
        ),
        (
            mounted.vendor,
            writable.vendor,
            Some(volumes.vendor),
            VENDOR_ROOT,
        ),
        (
            mounted.config,
            writable.config,
            Some(volumes.config),
            CONFIG_ROOT,
        ),
        (
            mounted.target,
            writable.target,
            Some(volumes.target),
            TARGET_ROOT,
        ),
        (
            mounted.output,
            writable.output,
            volumes.output,
            output_root(kind),
        ),
    ] {
        if mounted {
            expected.push(AppliedVolumeExpectation {
                volume: volume.ok_or(ExecutionError::InvalidConfiguration)?,
                target,
                writable,
            });
        }
    }
    Ok(expected)
}

fn create_governor_phase(
    gateway: &RustGateway,
    request: &PhaseRequest<'_, '_>,
    paths: &GovernorPaths,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), PerformanceError> {
    let deadline = request.deadline;
    budget_error(deadline, cancel)?;
    phase_result(gateway.approved_runtime(cancel), deadline, cancel)?;
    if !phase_result(
        absent(gateway, "container", request.name, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    let arguments = governor_arguments_for_runtime(
        gateway.image_id(),
        gateway.inner.state.path(),
        request.name,
        request.operation_id,
        request.volumes,
        request.operation,
        paths,
    )?;
    phase_result(
        mutation_control(gateway, &arguments, deadline, cancel),
        deadline,
        cancel,
    )?;
    let inspected = phase_result(
        query_control(
            gateway,
            &["container".into(), "inspect".into(), request.name.into()],
            deadline,
            cancel,
        ),
        deadline,
        cancel,
    )?;
    if inspected.code != Some(0) {
        return Err(ExecutionError::Infrastructure.into());
    }
    verify_applied_with_command(
        &inspected.stdout,
        gateway.image_id(),
        PerformancePhase::GovernorProbe,
        request.volumes,
        request.operation,
        request.operation_id,
        paths.as_slice(),
    )?;
    Ok(())
}

// -- phase lifecycle ---------------------------------------------------------

struct PhaseRequest<'a, 'v> {
    name: &'a str,
    operation_id: &'a str,
    volumes: &'a PerformanceVolumes<'v>,
    phase: PerformancePhase,
    operation: PerformanceOperation<'a>,
    deadline: Instant,
    output_limit: usize,
}

fn create_volume(
    gateway: &RustGateway,
    name: &str,
    operation_id: &str,
    options: &str,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<MutationVolume, PerformanceError> {
    budget_error(deadline, cancel)?;
    if !phase_result(
        absent(gateway, "volume", name, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    let mut arguments = vec![
        "volume".into(),
        "create".into(),
        "--driver=local".into(),
        "--opt=type=tmpfs".into(),
        "--opt=device=tmpfs".into(),
        format!("--opt=o={options}"),
    ];
    for (key, value) in labels(operation_id) {
        arguments.push(format!("--label={key}={value}"));
    }
    arguments.push(name.into());
    phase_result(
        mutation_control(gateway, &arguments, deadline, cancel),
        deadline,
        cancel,
    )?;
    let inspected = phase_result(
        query_control(
            gateway,
            &["volume".into(), "inspect".into(), name.into()],
            deadline,
            cancel,
        ),
        deadline,
        cancel,
    )?;
    if inspected.code != Some(0) {
        return Err(ExecutionError::Infrastructure.into());
    }
    parse_volume_with_options(&inspected.stdout, name, operation_id, options).map_err(Into::into)
}

fn create_phase(
    gateway: &RustGateway,
    request: &PhaseRequest<'_, '_>,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), PerformanceError> {
    let deadline = request.deadline;
    budget_error(deadline, cancel)?;
    phase_result(gateway.approved_runtime(cancel), deadline, cancel)?;
    if !phase_result(
        absent(gateway, "container", request.name, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    let arguments = create_arguments(
        gateway,
        request.name,
        request.operation_id,
        request.volumes,
        request.phase,
        request.operation,
    )?;
    phase_result(
        mutation_control(gateway, &arguments, deadline, cancel),
        deadline,
        cancel,
    )?;
    let inspected = phase_result(
        query_control(
            gateway,
            &["container".into(), "inspect".into(), request.name.into()],
            deadline,
            cancel,
        ),
        deadline,
        cancel,
    )?;
    if inspected.code != Some(0) {
        return Err(ExecutionError::Infrastructure.into());
    }
    verify_applied(
        &inspected.stdout,
        gateway.image_id(),
        request.phase,
        request.volumes,
        request.operation,
        request.operation_id,
    )?;
    Ok(())
}

fn validate_capture_completion(capture: &Capture) -> Result<(), PerformanceError> {
    match capture.stop {
        Stop::Cancelled => return Err(ExecutionError::Cancelled.into()),
        Stop::TimedOut => return Err(PerformanceError::Timeout),
        Stop::OutputLimit => return Err(PerformanceError::OutputLimit),
        Stop::Exited => {}
    }
    if capture.stdout_truncated || capture.stderr_truncated {
        return Err(PerformanceError::OutputLimit);
    }
    Ok(())
}

fn validate_completed_container(
    inspected: &Capture,
    capture: &Capture,
) -> Result<(), PerformanceError> {
    let containers: Vec<Container> =
        serde_json::from_slice(&inspected.stdout).map_err(|_| ExecutionError::Infrastructure)?;
    let container = containers
        .first()
        .filter(|_| containers.len() == 1)
        .ok_or(ExecutionError::Infrastructure)?;
    if inspected.code != Some(0)
        || !container.state.completed(capture.code)
        || container.state.oom_killed
    {
        return Err(ExecutionError::Infrastructure.into());
    }
    Ok(())
}

fn finish_phase(
    gateway: &RustGateway,
    name: &str,
    operation_id: &str,
    capture: &Capture,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), PerformanceError> {
    validate_capture_completion(capture)?;
    let inspected = phase_result(
        query_control(
            gateway,
            &["container".into(), "inspect".into(), name.into()],
            deadline,
            cancel,
        ),
        deadline,
        cancel,
    )?;
    validate_completed_container(&inspected, capture)?;
    phase_result(
        remove_if_present(gateway, name, operation_id, deadline, cancel),
        deadline,
        cancel,
    )?;
    if !phase_result(
        absent(gateway, "container", name, deadline, cancel),
        deadline,
        cancel,
    )? {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    budget_error(deadline, cancel)
}

fn start_guardian(
    gateway: &RustGateway,
    request: &PhaseRequest<'_, '_>,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), PerformanceError> {
    create_phase(gateway, request, cancel)?;
    phase_result(
        mutation_control(
            gateway,
            &["container".into(), "start".into(), request.name.into()],
            request.deadline,
            cancel,
        ),
        request.deadline,
        cancel,
    )?;
    if !phase_result(
        running(
            gateway,
            request.name,
            request.operation_id,
            request.deadline,
            cancel,
        ),
        request.deadline,
        cancel,
    )? {
        return Err(ExecutionError::Infrastructure.into());
    }
    budget_error(request.deadline, cancel)
}

fn revalidate(
    gateway: &RustGateway,
    guardians: &[&str],
    removed: &[&str],
    operation_id: &str,
    deadline: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), PerformanceError> {
    for guardian in guardians {
        if !phase_result(
            running(gateway, guardian, operation_id, deadline, cancel),
            deadline,
            cancel,
        )? {
            return Err(ExecutionError::Infrastructure.into());
        }
    }
    for name in removed {
        if !phase_result(
            absent(gateway, "container", name, deadline, cancel),
            deadline,
            cancel,
        )? {
            return Err(ExecutionError::CleanupUncertain.into());
        }
    }
    budget_error(deadline, cancel)
}

/// What the vendor ingest phase writes into the guest's `tar`.
enum VendorIngest<'a> {
    /// A `SourceBundle`-backed vendor, or the source and config archives:
    /// small, owned and bounded by ADR-031, exactly as before ADR-078.
    Bytes(Vec<u8>),
    /// ADR-078's capture, streamed from the artifact the host verified. The
    /// guest still receives a read-only volume built from these bytes, never
    /// the host directory they were captured from (ADR-078 §1, §6).
    Capture(&'a dyn VerifiedVendorCapture),
}

impl VendorIngest<'_> {
    /// What stands for these bytes inside the execution fingerprint.
    ///
    /// An owned archive is hashed whole, so every pre-ADR-078 flow keeps the
    /// fingerprint it had. A capture is never resident, so what is hashed is
    /// the artifact digest the host already computed over exactly those bytes:
    /// the same commitment, reached incrementally.
    fn commitment(&self) -> &[u8] {
        match self {
            Self::Bytes(bytes) => bytes,
            Self::Capture(capture) => capture.capture().artifact_digest().as_str().as_bytes(),
        }
    }
}

/// Pulls a verified capture into the supervisor one ADR-078 read buffer at a
/// time. Its whole state is that buffer.
struct CaptureSource<'a> {
    capture: &'a dyn VerifiedVendorCapture,
    buffer: Vec<u8>,
    filled: usize,
    position: usize,
    ended: bool,
}
impl<'a> CaptureSource<'a> {
    fn new(capture: &'a dyn VerifiedVendorCapture) -> Self {
        Self {
            capture,
            buffer: vec![0; VENDOR_CAPTURE_READ_BUFFER_BYTES],
            filled: 0,
            position: 0,
            ended: false,
        }
    }
}
impl crate::supervisor::InputSource for CaptureSource<'_> {
    fn fill(&mut self) -> std::io::Result<&[u8]> {
        if self.position == self.filled && !self.ended {
            self.filled = self
                .capture
                .read(&mut self.buffer)
                .map_err(std::io::Error::other)?;
            self.position = 0;
            if self.filled == 0 {
                self.ended = true;
            }
        }
        Ok(&self.buffer[self.position..self.filled])
    }
    fn consume(&mut self, count: usize) {
        self.position = self.position.saturating_add(count).min(self.filled);
    }
}

fn ingest(
    gateway: &RustGateway,
    request: &PhaseRequest<'_, '_>,
    archive: &VendorIngest<'_>,
    cancel: &dyn ExecutionCancellation,
) -> Result<(), PerformanceError> {
    create_phase(gateway, request, cancel)?;
    let capture = phase_result(
        match archive {
            VendorIngest::Bytes(bytes) => start_attached(
                gateway,
                request.name,
                true,
                bytes,
                request.deadline,
                request.output_limit,
                cancel,
            ),
            // ADR-078 §5: the capture is streamed into `tar` one 64 KiB buffer
            // at a time. Nothing here ever holds the artifact.
            VendorIngest::Capture(capture) => {
                capture.rewind().map_err(|_| ExecutionError::Denied)?;
                let mut source = CaptureSource::new(*capture);
                start_attached_source(
                    gateway,
                    request.name,
                    &mut source,
                    capture.capture().artifact_bytes(),
                    request.deadline,
                    request.output_limit,
                    cancel,
                )
            }
        },
        request.deadline,
        cancel,
    )?;
    finish_phase(
        gateway,
        request.name,
        request.operation_id,
        &capture,
        request.deadline,
        cancel,
    )?;
    if capture.code != Some(0) || !capture.stdout.is_empty() || !capture.stderr.is_empty() {
        return Err(ExecutionError::Infrastructure.into());
    }
    Ok(())
}

/// Create, run to completion and remove one non-interactive phase. `deadline`
/// is the phase's own share; `outer` is the operation deadline that bounds the
/// control calls around it.
fn run_phase(
    gateway: &RustGateway,
    request: &PhaseRequest<'_, '_>,
    outer: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<Capture, PerformanceError> {
    create_phase(gateway, request, cancel)?;
    let capture = phase_result(
        start_attached(
            gateway,
            request.name,
            false,
            &[],
            request.deadline,
            request.output_limit,
            cancel,
        ),
        outer,
        cancel,
    )?;
    finish_phase(
        gateway,
        request.name,
        request.operation_id,
        &capture,
        outer,
        cancel,
    )?;
    Ok(capture)
}

/// Runs the one closed, topology-derived CPUFreq observation. It deliberately
/// shares the normal lifecycle and containment verification with every other
/// phase; only its already-validated argv differs from `PerformancePhase`'s
/// static command table.
fn run_governor_probe(
    gateway: &RustGateway,
    request: &PhaseRequest<'_, '_>,
    paths: &GovernorPaths,
    outer: Instant,
    cancel: &dyn ExecutionCancellation,
) -> Result<Capture, PerformanceError> {
    create_governor_phase(gateway, request, paths, cancel)?;
    let capture = phase_result(
        start_attached(
            gateway,
            request.name,
            false,
            &[],
            request.deadline,
            request.output_limit,
            cancel,
        ),
        outer,
        cancel,
    )?;
    finish_phase(
        gateway,
        request.name,
        request.operation_id,
        &capture,
        request.deadline,
        cancel,
    )?;
    Ok(capture)
}

// -- archives ----------------------------------------------------------------

fn config_archive() -> Result<Vec<u8>, PerformanceError> {
    let file = SourceFile::new(
        "cargo-home/config.toml".into(),
        crate::security_policy::SECURITY_CARGO_CONFIG.to_vec(),
    )
    .map_err(|_| ExecutionError::InvalidConfiguration)?;
    let bundle = SourceBundle::new(vec![file]).map_err(|_| ExecutionError::InvalidConfiguration)?;
    crate::mutation_archive::encode(&bundle).map_err(Into::into)
}

/// Reads the two fixed regular-file members of the profiling exporter tar.
/// Links, devices, extra members and guest-chosen names are rejected.
fn decode_profile_tar(bytes: &[u8], max_len: usize) -> Option<(Vec<u8>, Vec<u8>)> {
    let mut offset = 0usize;
    let mut stacks: Option<Vec<u8>> = None;
    let mut manifest: Option<Vec<u8>> = None;
    while offset + 512 <= bytes.len() {
        let header = &bytes[offset..offset + 512];
        if header.iter().all(|byte| *byte == 0) {
            break;
        }
        let size = octal(&header[124..136])?;
        let typeflag = header[156];
        let padded = size.div_ceil(512) * 512;
        offset += 512;
        if offset + padded > bytes.len() || size > max_len {
            return None;
        }
        let content = &bytes[offset..offset + size];
        match typeflag {
            b'0' | 0 => {
                let name = header[..100]
                    .split(|byte| *byte == 0)
                    .next()
                    .and_then(|value| std::str::from_utf8(value).ok())?;
                let slot = match name {
                    "stacks.txt" => &mut stacks,
                    "manifest.json" => &mut manifest,
                    _ => return None,
                };
                if slot.is_some() {
                    return None;
                }
                *slot = Some(content.to_vec());
            }
            b'5' => (),
            _ => return None,
        }
        offset += padded;
    }
    stacks.zip(manifest)
}

fn octal(field: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(field).ok()?;
    let trimmed = text.trim_matches(|c: char| c == '\0' || c.is_ascii_whitespace());
    if trimmed.is_empty() {
        return Some(0);
    }
    usize::from_str_radix(trimmed, 8).ok()
}

// -- harness detection -------------------------------------------------------

#[derive(Deserialize)]
struct MetadataPackage {
    name: String,
    version: String,
}
#[derive(Deserialize)]
struct MetadataDocument {
    packages: Vec<MetadataPackage>,
}

/// ADR-073 §1: the harness is the exact package Cargo resolved, at its exact
/// version. A target name never implies a harness, and an unapproved version is
/// never treated as a degraded measurement.
pub(super) fn detect_harness(metadata: &[u8]) -> Result<HarnessDetection, PerformanceError> {
    let document: MetadataDocument =
        serde_json::from_slice(metadata).map_err(|_| PerformanceError::InvalidMetadata)?;
    let Some(package) = document
        .packages
        .iter()
        .find(|package| package.name == "criterion")
    else {
        return Ok(HarnessDetection::Unrecognized);
    };
    if package.version == rust_engineering_domain::benchmark::APPROVED_CRITERION_VERSION {
        Ok(HarnessDetection::Criterion {
            version: package.version.clone(),
        })
    } else {
        Ok(HarnessDetection::CriterionUnapproved {
            version: package.version.clone(),
        })
    }
}

// -- option validation -------------------------------------------------------

/// A leading `-` is refused even though cargo never accepts it in a target
/// name: the value is interpolated into a single `--bench=<name>` token, so it
/// could not become a flag, and refusing it keeps that true without depending
/// on the interpolation staying that way.
fn valid_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// A cargo feature may name a dependency's feature with `dep/feature`; nothing
/// else is accepted, so no argument, flag or path can be smuggled through.
fn valid_feature(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && !value.starts_with('-')
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'/'
        })
}

pub(super) fn validate_benchmark(
    selection: &BenchmarkSelection,
    run_count: u8,
) -> Result<(), PerformanceError> {
    if !(1..=3).contains(&run_count) {
        return Err(PerformanceError::InvalidOptions);
    }
    selection
        .validate()
        .map_err(|_| PerformanceError::InvalidOptions)?;
    let named = selection
        .bench_target
        .iter()
        .chain(selection.package.iter())
        .all(|value| valid_name(value));
    if !named
        || !selection.features.iter().all(|value| valid_feature(value))
        || !valid_name(&selection.profile)
    {
        return Err(PerformanceError::InvalidOptions);
    }
    Ok(())
}

// -- fingerprint -------------------------------------------------------------

/// What one phase's process reported, reduced to what the fingerprint binds.
/// [`Capture`] is deliberately not `Clone`, and the raw bytes are evidence the
/// port owns, so only the identity travels here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct CaptureIdentity {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn capture_identity(capture: &Capture) -> CaptureIdentity {
    CaptureIdentity {
        code: capture.code,
        stdout: digest(&capture.stdout),
        stderr: digest(&capture.stderr),
    }
}

struct PerformanceFingerprintInputs<'a, 'v> {
    operation: PerformanceOperation<'a>,
    volumes: &'a PerformanceVolumes<'v>,
    source_archive: &'a [u8],
    vendor_archive: &'a [u8],
    config_archive: &'a [u8],
    metadata: &'a [u8],
    governor_paths: Option<&'a GovernorPaths>,
    vendor_fingerprint: &'a SourceFingerprint,
    limits: ExecutionLimits,
    captures: &'a [CaptureIdentity],
    artifacts: &'a [String],
}

fn fingerprint_volume(name: &str, mountpoint: &str, options: &str) -> MutationVolume {
    MutationVolume {
        name: name.into(),
        driver: "local".into(),
        scope: "local".into(),
        options: BTreeMap::from([
            ("device".into(), "tmpfs".into()),
            ("o".into(), options.into()),
            ("type".into(), "tmpfs".into()),
        ]),
        labels: labels("<operation_id>"),
        mountpoint: mountpoint.into(),
        cluster_volume: None,
        status: None,
    }
}

fn execution_fingerprint_for_runtime(
    configuration_fingerprint: &ExecutionFingerprint,
    image_id: &str,
    state_path: &std::path::Path,
    inputs: PerformanceFingerprintInputs<'_, '_>,
) -> Result<ExecutionFingerprint, PerformanceError> {
    let mut commands = Vec::new();
    for phase in inputs.operation.phases() {
        if phase == PerformancePhase::GovernorProbe {
            if let Some(paths) = inputs.governor_paths {
                commands.push(governor_arguments_for_runtime(
                    image_id,
                    state_path,
                    "<container>",
                    "<operation_id>",
                    inputs.volumes,
                    inputs.operation,
                    paths,
                )?);
            }
        } else {
            commands.push(create_arguments_for_runtime(
                image_id,
                state_path,
                "<container>",
                "<operation_id>",
                inputs.volumes,
                phase,
                inputs.operation,
            )?);
        }
    }
    let volume_options = [
        inputs.volumes.source.options.get("o"),
        inputs.volumes.vendor.options.get("o"),
        inputs.volumes.config.options.get("o"),
        inputs.volumes.target.options.get("o"),
        inputs
            .volumes
            .output
            .and_then(|volume| volume.options.get("o")),
    ];
    let bytes = serde_json::to_vec(&(
        configuration_fingerprint,
        commands,
        ["--opt=type=tmpfs", "--opt=device=tmpfs"],
        volume_options,
        inputs.operation.kind(),
        digest(inputs.source_archive),
        digest(inputs.vendor_archive),
        digest(inputs.config_archive),
        digest(inputs.metadata),
        inputs.vendor_fingerprint,
        inputs.limits,
        inputs.captures,
        inputs.artifacts,
        (
            digest(include_bytes!("performance_gateway.rs")),
            digest(include_bytes!("performance_environment.rs")),
            digest(include_bytes!("rust_applied.rs")),
            digest(include_bytes!("mutation_gateway.rs")),
            digest(include_bytes!("security_policy.rs")),
            digest(include_bytes!("profile_stacks.rs")),
            digest(include_bytes!("profile_svg.rs")),
            digest(include_bytes!("seccomp-rust-quality.json")),
            digest(include_bytes!("seccomp-rust-profile.json")),
        ),
    ))
    .map_err(|_| ExecutionError::Infrastructure)?;
    digest(&bytes)
        .parse()
        .map_err(|_| ExecutionError::Infrastructure.into())
}

// -- results -----------------------------------------------------------------

/// One independent benchmark repetition (ADR-073 §2): its own `CRITERION_HOME`
/// volume, its own capture and its own exported archive.
pub(super) struct BenchmarkRunOutput {
    pub(super) capture: Capture,
    pub(super) archive: Vec<u8>,
}

pub(super) struct ProfileOutput {
    pub(super) build: Capture,
    /// Absent when the build failed: no sampler ran, and none is reported as
    /// though it had.
    pub(super) run: Option<Capture>,
    pub(super) stacks: Vec<u8>,
    pub(super) manifest: Vec<u8>,
}

pub(super) struct BloatOutput {
    pub(super) functions: Capture,
    pub(super) crates: Capture,
    pub(super) size: Capture,
    pub(super) digest: Capture,
    pub(super) header: Capture,
}

/// What the guest reported about the measuring host. An absent field is
/// UNKNOWN and blocks comparison (ADR-073 §3); it is never defaulted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct HardwareProbe {
    pub(super) cpu_model: Option<String>,
    pub(super) cpu_cores: Option<u16>,
    pub(super) os_kernel: Option<String>,
    /// A uniform value observed through CPUFreq links for every logical CPU the
    /// guest exposed. `None` is unknown or heterogeneous; it is never a CPU0
    /// default and never describes the physical host.
    pub(super) cpu_governor: Option<String>,
}

/// The ceilings this gateway itself applied. It knows them exactly, so they are
/// never inferred from the guest.
pub(super) const APPLIED_CPU_MILLICORES: u32 = 1_000;
pub(super) const APPLIED_MEMORY_BYTES: u64 = 1_073_741_824;
pub(super) const APPLIED_PIDS: u32 = 128;

pub(super) struct PerformanceExecution {
    pub(super) kind: PerformanceKind,
    pub(super) harness: HarnessDetection,
    pub(super) hardware: HardwareProbe,
    pub(super) runs: Vec<BenchmarkRunOutput>,
    pub(super) profile: Option<ProfileOutput>,
    pub(super) bloat: Option<BloatOutput>,
    pub(super) execution_fingerprint: ExecutionFingerprint,
    pub(super) source_fingerprint: SourceFingerprint,
    pub(super) vendor_fingerprint: SourceFingerprint,
}

fn parse_uname(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty() && line.len() <= 512 && !line.chars().any(char::is_control))
        .then(|| line.to_owned())
}

// -- entry points ------------------------------------------------------------

pub(super) fn execute_benchmark(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: BenchmarkVendor<'_>,
    selection: &BenchmarkSelection,
    run_count: u8,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<PerformanceExecution, PerformanceError> {
    validate_benchmark(selection, run_count)?;
    execute_operation(
        gateway,
        source,
        vendor,
        PerformanceOperation::Benchmark {
            selection,
            run_count,
            harness_parameters: true,
        },
        limits,
        cancel,
    )
}

pub(super) fn execute_profile(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    options: &ProfileOptions,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<PerformanceExecution, PerformanceError> {
    execute_operation(
        gateway,
        source,
        BenchmarkVendor::Snapshot(vendor),
        PerformanceOperation::Profile(options),
        limits,
        cancel,
    )
}

pub(super) fn execute_bloat(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
    options: &BloatOptions,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<PerformanceExecution, PerformanceError> {
    execute_operation(
        gateway,
        source,
        BenchmarkVendor::Snapshot(vendor),
        PerformanceOperation::Bloat(options),
        limits,
        cancel,
    )
}

fn validate_vendor(vendor: &BenchmarkVendor<'_>) -> Result<(), PerformanceError> {
    let Some(snapshot) = vendor.snapshot() else {
        // A capture arrives already verified: `open_verified_capture` re-derived
        // its tree digest from the artifact, incrementally, and refused it if it
        // was not the declared one. Re-reading 512 MiB here to reach the same
        // answer would be the residency ADR-078 §5 forbids.
        return Ok(());
    };
    if crate::resolution_gateway::tree_fingerprint(&snapshot.source)
        .map_err(|_| PerformanceError::MissingOfflineData)?
        != snapshot.tree_fingerprint
    {
        return Err(PerformanceError::MissingOfflineData);
    }
    Ok(())
}

fn bytes_fingerprint(bytes: &[u8]) -> Result<SourceFingerprint, PerformanceError> {
    digest(bytes)
        .parse()
        .map_err(|_| ExecutionError::Infrastructure.into())
}

struct Names {
    operation_id: String,
    source_volume: String,
    vendor_volume: String,
    config_volume: String,
    target_volume: String,
    output_volumes: Vec<String>,
    containers: Vec<String>,
    guardians: Vec<String>,
    output_guardians: Vec<String>,
}

fn names(operation_id: String, outputs: usize) -> Names {
    let prefix = "rust-mcp-performance";
    let output_volumes = (0..outputs)
        .map(|index| format!("{prefix}-output-{index}-{operation_id}"))
        .collect::<Vec<_>>();
    let output_guardians = (0..outputs)
        .map(|index| format!("{prefix}-output-guardian-{index}-{operation_id}"))
        .collect::<Vec<_>>();
    let guardians = [
        format!("{prefix}-source-guardian-{operation_id}"),
        format!("{prefix}-vendor-guardian-{operation_id}"),
        format!("{prefix}-config-guardian-{operation_id}"),
        format!("{prefix}-target-guardian-{operation_id}"),
    ]
    .to_vec();
    let containers = [
        format!("{prefix}-source-ingest-{operation_id}"),
        format!("{prefix}-vendor-ingest-{operation_id}"),
        format!("{prefix}-config-ingest-{operation_id}"),
        format!("{prefix}-metadata-{operation_id}"),
        format!("{prefix}-cpu-{operation_id}"),
        format!("{prefix}-kernel-{operation_id}"),
        format!("{prefix}-work-0-{operation_id}"),
        format!("{prefix}-work-1-{operation_id}"),
        format!("{prefix}-work-2-{operation_id}"),
        format!("{prefix}-work-3-{operation_id}"),
        format!("{prefix}-work-4-{operation_id}"),
        format!("{prefix}-work-5-{operation_id}"),
        format!("{prefix}-governor-{operation_id}"),
    ]
    .to_vec();
    Names {
        source_volume: format!("{prefix}-source-{operation_id}"),
        vendor_volume: format!("{prefix}-vendor-{operation_id}"),
        config_volume: format!("{prefix}-config-{operation_id}"),
        target_volume: format!("{prefix}-target-{operation_id}"),
        output_volumes,
        containers,
        guardians,
        output_guardians,
        operation_id,
    }
}

struct Work {
    harness: HarnessDetection,
    hardware: HardwareProbe,
    metadata: Vec<u8>,
    governor_paths: Option<GovernorPaths>,
    runs: Vec<BenchmarkRunOutput>,
    profile: Option<ProfileOutput>,
    bloat: Option<BloatOutput>,
    captures: Vec<CaptureIdentity>,
    artifacts: Vec<String>,
}

#[expect(
    clippy::too_many_lines,
    reason = "One orchestrator serves all three operations; splitting it would \
              duplicate the guardian, ingest and cleanup ordering per tool."
)]
fn execute_operation(
    gateway: &RustGateway,
    source: &SourceBundle,
    vendor: BenchmarkVendor<'_>,
    operation: PerformanceOperation<'_>,
    limits: ExecutionLimits,
    cancel: &dyn ExecutionCancellation,
) -> Result<PerformanceExecution, PerformanceError> {
    let kind = operation.kind();
    let started = Instant::now();
    let deadline = started + Duration::from_millis(limits.wall_ms());
    let _busy = gateway.hold_busy()?;
    if gateway.is_quarantined() {
        return Err(ExecutionError::CleanupUncertain.into());
    }
    if gateway.calibrating.load(Ordering::Acquire) || !gateway.verified.load(Ordering::Acquire) {
        return Err(ExecutionError::Denied.into());
    }
    phase_result(gateway.approved_runtime(cancel), deadline, cancel)?;
    reject_project_cargo_configuration(source)?;
    budget_error(deadline, cancel)?;
    validate_vendor(&vendor)?;
    let source_archive = crate::source_archive::encode(source)?;
    budget_error(deadline, cancel)?;
    let vendor_options = match &vendor {
        BenchmarkVendor::Capture(_) => VENDOR_CAPTURE_VOLUME_OPTIONS,
        BenchmarkVendor::Snapshot(_) => VOLUME_OPTIONS,
    };
    let vendor_archive = match &vendor {
        BenchmarkVendor::Snapshot(snapshot) => {
            VendorIngest::Bytes(crate::source_archive::encode(&snapshot.source)?)
        }
        BenchmarkVendor::Capture(capture) => VendorIngest::Capture(*capture),
    };
    budget_error(deadline, cancel)?;
    let config = config_archive()?;
    budget_error(deadline, cancel)?;

    let outputs = match operation {
        PerformanceOperation::Benchmark { run_count, .. } => usize::from(run_count),
        PerformanceOperation::Profile(_) => 1,
        PerformanceOperation::Bloat(_) => 0,
    };
    let names = names(state::nonce()?, outputs);
    let all_containers = names
        .containers
        .iter()
        .chain(names.guardians.iter())
        .chain(names.output_guardians.iter())
        .map(String::as_str)
        .collect::<Vec<_>>();

    for volume in [
        &names.source_volume,
        &names.vendor_volume,
        &names.config_volume,
        &names.target_volume,
    ]
    .into_iter()
    .chain(names.output_volumes.iter())
    {
        if !phase_result(
            absent(gateway, "volume", volume, deadline, cancel),
            deadline,
            cancel,
        )? {
            return Err(ExecutionError::CleanupUncertain.into());
        }
    }

    let work = (|| -> Result<Work, PerformanceError> {
        let source_volume = create_volume(
            gateway,
            &names.source_volume,
            &names.operation_id,
            VOLUME_OPTIONS,
            deadline,
            cancel,
        )?;
        let vendor_volume = create_volume(
            gateway,
            &names.vendor_volume,
            &names.operation_id,
            vendor_options,
            deadline,
            cancel,
        )?;
        let config_volume = create_volume(
            gateway,
            &names.config_volume,
            &names.operation_id,
            VOLUME_OPTIONS,
            deadline,
            cancel,
        )?;
        let target_volume = create_volume(
            gateway,
            &names.target_volume,
            &names.operation_id,
            TARGET_VOLUME_OPTIONS,
            deadline,
            cancel,
        )?;
        let mut output_volumes = Vec::new();
        for name in &names.output_volumes {
            output_volumes.push(create_volume(
                gateway,
                name,
                &names.operation_id,
                VOLUME_OPTIONS,
                deadline,
                cancel,
            )?);
        }
        let base = PerformanceVolumes {
            source: &source_volume,
            vendor: &vendor_volume,
            config: &config_volume,
            target: &target_volume,
            output: None,
        };

        for (name, phase) in [
            (&names.guardians[0], PerformancePhase::SourceGuardian),
            (&names.guardians[1], PerformancePhase::VendorGuardian),
            (&names.guardians[2], PerformancePhase::ConfigGuardian),
            (&names.guardians[3], PerformancePhase::TargetGuardian),
        ] {
            start_guardian(
                gateway,
                &PhaseRequest {
                    name,
                    operation_id: &names.operation_id,
                    volumes: &base,
                    phase,
                    operation,
                    deadline,
                    output_limit: limits.output_bytes(),
                },
                cancel,
            )?;
        }
        let mut guardians = names
            .guardians
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        for (index, volume) in output_volumes.iter().enumerate() {
            let volumes = PerformanceVolumes {
                output: Some(volume),
                ..base
            };
            start_guardian(
                gateway,
                &PhaseRequest {
                    name: &names.output_guardians[index],
                    operation_id: &names.operation_id,
                    volumes: &volumes,
                    phase: PerformancePhase::OutputGuardian,
                    operation,
                    deadline,
                    output_limit: limits.output_bytes(),
                },
                cancel,
            )?;
            guardians.push(&names.output_guardians[index]);
        }

        let mut removed: Vec<&str> = Vec::new();
        for (index, (phase, archive)) in [
            (
                PerformancePhase::SourceIngest,
                &VendorIngest::Bytes(source_archive.clone()),
            ),
            (PerformancePhase::VendorIngest, &vendor_archive),
            (
                PerformancePhase::ConfigIngest,
                &VendorIngest::Bytes(config.clone()),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            ingest(
                gateway,
                &PhaseRequest {
                    name: &names.containers[index],
                    operation_id: &names.operation_id,
                    volumes: &base,
                    phase,
                    operation,
                    deadline,
                    output_limit: limits.output_bytes(),
                },
                archive,
                cancel,
            )?;
            removed.push(&names.containers[index]);
            revalidate(
                gateway,
                &guardians,
                &removed,
                &names.operation_id,
                deadline,
                cancel,
            )?;
        }

        // Reading phases: metadata identity and guest hardware probes. They
        // execute no project code and share one small budget slice.
        let identity_deadline = phase_deadline(deadline, 8, cancel)?;
        let metadata_capture = run_phase(
            gateway,
            &PhaseRequest {
                name: &names.containers[3],
                operation_id: &names.operation_id,
                volumes: &base,
                phase: PerformancePhase::Metadata,
                operation,
                deadline: identity_deadline,
                output_limit: METADATA_OUTPUT,
            },
            deadline,
            cancel,
        )?;
        removed.push(&names.containers[3]);
        if metadata_capture.code != Some(0) {
            return Err(
                if crate::resolution_gateway::missing_offline_data(&metadata_capture) {
                    PerformanceError::MissingOfflineData
                } else {
                    PerformanceError::InvalidMetadata
                },
            );
        }
        let harness = detect_harness(&metadata_capture.stdout)?;
        let harness_parameters = harness.measurable();
        revalidate(
            gateway,
            &guardians,
            &removed,
            &names.operation_id,
            deadline,
            cancel,
        )?;

        let mut hardware = HardwareProbe::default();
        let mut governor_paths = None;
        let probe_deadline = phase_deadline(deadline, 8, cancel)?;
        let cpu = run_phase(
            gateway,
            &PhaseRequest {
                name: &names.containers[4],
                operation_id: &names.operation_id,
                volumes: &base,
                phase: PerformancePhase::CpuProbe,
                operation,
                deadline: probe_deadline,
                output_limit: PROBE_OUTPUT,
            },
            deadline,
            cancel,
        )?;
        removed.push(&names.containers[4]);
        if cpu.code == Some(0) {
            let topology: GuestCpuTopology = performance_environment::parse_cpuinfo(&cpu.stdout);
            hardware.cpu_model = topology.cpu_model.clone();
            hardware.cpu_cores = topology
                .cpu_ids_complete
                .then(|| u16::try_from(topology.logical_cpu_ids.len()).ok())
                .flatten()
                .filter(|count| *count > 0);
            governor_paths = performance_environment::governor_paths(&topology);
        }
        let kernel = run_phase(
            gateway,
            &PhaseRequest {
                name: &names.containers[5],
                operation_id: &names.operation_id,
                volumes: &base,
                phase: PerformancePhase::KernelProbe,
                operation,
                deadline: probe_deadline,
                output_limit: PROBE_OUTPUT,
            },
            deadline,
            cancel,
        )?;
        removed.push(&names.containers[5]);
        if kernel.code == Some(0) {
            hardware.os_kernel = parse_uname(&kernel.stdout);
        }
        let governor = if let Some(paths) = governor_paths.as_ref() {
            let capture = run_governor_probe(
                gateway,
                &PhaseRequest {
                    name: &names.containers[12],
                    operation_id: &names.operation_id,
                    volumes: &base,
                    phase: PerformancePhase::GovernorProbe,
                    operation,
                    deadline: probe_deadline,
                    output_limit: PROBE_OUTPUT,
                },
                paths,
                deadline,
                cancel,
            )?;
            removed.push(&names.containers[12]);
            if capture.code == Some(0) {
                hardware.cpu_governor =
                    performance_environment::parse_uniform_governor(&capture.stdout, paths.len());
            }
            Some(capture)
        } else {
            None
        };
        revalidate(
            gateway,
            &guardians,
            &removed,
            &names.operation_id,
            deadline,
            cancel,
        )?;

        let mut identities = vec![
            capture_identity(&metadata_capture),
            capture_identity(&cpu),
            capture_identity(&kernel),
        ];
        if let Some(capture) = &governor {
            identities.push(capture_identity(capture));
        }
        let mut work = Work {
            harness,
            hardware,
            metadata: metadata_capture.stdout,
            governor_paths,
            runs: Vec::new(),
            profile: None,
            bloat: None,
            captures: identities,
            artifacts: Vec::new(),
        };
        match operation {
            PerformanceOperation::Benchmark {
                selection,
                run_count,
                ..
            } => {
                // The harness is known only after `Metadata`; the run argv is
                // rebuilt from what was actually resolved, never from what the
                // caller assumed.
                let operation = PerformanceOperation::Benchmark {
                    selection,
                    run_count,
                    harness_parameters,
                };
                for (index, output) in output_volumes
                    .iter()
                    .enumerate()
                    .take(usize::from(run_count))
                {
                    let volumes = PerformanceVolumes {
                        output: Some(output),
                        ..base
                    };
                    let run_deadline = phase_deadline(deadline, u64::from(run_count) * 2, cancel)?;
                    let capture = run_phase(
                        gateway,
                        &PhaseRequest {
                            name: &names.containers[6 + index * 2],
                            operation_id: &names.operation_id,
                            volumes: &volumes,
                            phase: PerformancePhase::BenchRun,
                            operation,
                            deadline: run_deadline,
                            output_limit: limits.output_bytes(),
                        },
                        deadline,
                        cancel,
                    )?;
                    removed.push(&names.containers[6 + index * 2]);
                    let exported = run_phase(
                        gateway,
                        &PhaseRequest {
                            name: &names.containers[7 + index * 2],
                            operation_id: &names.operation_id,
                            volumes: &volumes,
                            phase: PerformancePhase::BenchExport,
                            operation,
                            deadline: phase_deadline(deadline, 8, cancel)?,
                            output_limit: CRITERION_ARCHIVE_OUTPUT,
                        },
                        deadline,
                        cancel,
                    )?;
                    removed.push(&names.containers[7 + index * 2]);
                    revalidate(
                        gateway,
                        &guardians,
                        &removed,
                        &names.operation_id,
                        deadline,
                        cancel,
                    )?;
                    work.artifacts.push(digest(&exported.stdout));
                    work.captures.push(capture_identity(&capture));
                    work.runs.push(BenchmarkRunOutput {
                        capture,
                        archive: if exported.code == Some(0) {
                            exported.stdout
                        } else {
                            Vec::new()
                        },
                    });
                }
            }
            PerformanceOperation::Profile(options) => {
                let volumes = PerformanceVolumes {
                    output: output_volumes.first(),
                    ..base
                };
                let build = run_phase(
                    gateway,
                    &PhaseRequest {
                        name: &names.containers[6],
                        operation_id: &names.operation_id,
                        volumes: &volumes,
                        phase: PerformancePhase::ProfileBuild,
                        operation,
                        deadline: phase_deadline(deadline, 3, cancel)?,
                        output_limit: limits.output_bytes(),
                    },
                    deadline,
                    cancel,
                )?;
                removed.push(&names.containers[6]);
                work.captures.push(capture_identity(&build));
                let run = if build.code == Some(0) {
                    let sampling = options
                        .duration_ms()
                        .min(PROFILE_MAX_SAMPLING_MS)
                        .saturating_add(15_000);
                    let bound = Instant::now()
                        .checked_add(Duration::from_millis(sampling))
                        .filter(|value| *value <= deadline)
                        .ok_or(PerformanceError::Timeout)?;
                    let run = run_phase(
                        gateway,
                        &PhaseRequest {
                            name: &names.containers[7],
                            operation_id: &names.operation_id,
                            volumes: &volumes,
                            phase: PerformancePhase::ProfileRun,
                            operation,
                            deadline: bound,
                            output_limit: limits.output_bytes(),
                        },
                        deadline,
                        cancel,
                    )?;
                    removed.push(&names.containers[7]);
                    Some(run)
                } else {
                    None
                };
                // A refused `perf_event_open` writes its artifacts too, and the
                // manifest is the only place the errno appears; exporting only
                // exit 0 made ADR-074 §3's denial unreportable by construction,
                // because the application refuses a denial that arrives without
                // the errno that produced it.
                let exported = if run.as_ref().is_some_and(|run| {
                    matches!(
                        run.code,
                        Some(HELPER_EXIT_PROFILED | HELPER_EXIT_PROFILER_UNAVAILABLE)
                    )
                }) {
                    let exported = run_phase(
                        gateway,
                        &PhaseRequest {
                            name: &names.containers[8],
                            operation_id: &names.operation_id,
                            volumes: &volumes,
                            phase: PerformancePhase::ProfileExport,
                            operation,
                            deadline: phase_deadline(deadline, 2, cancel)?,
                            output_limit: PROFILE_ARCHIVE_OUTPUT,
                        },
                        deadline,
                        cancel,
                    )?;
                    removed.push(&names.containers[8]);
                    Some(exported)
                } else {
                    None
                };
                revalidate(
                    gateway,
                    &guardians,
                    &removed,
                    &names.operation_id,
                    deadline,
                    cancel,
                )?;
                let (stacks, manifest) = exported
                    .as_ref()
                    .filter(|exported| exported.code == Some(0))
                    .and_then(|exported| {
                        decode_profile_tar(&exported.stdout, PROFILE_ARCHIVE_OUTPUT)
                    })
                    .unwrap_or_default();
                if let Some(exported) = &exported {
                    work.artifacts.push(digest(&exported.stdout));
                }
                if let Some(run) = &run {
                    work.captures.push(capture_identity(run));
                }
                work.profile = Some(ProfileOutput {
                    build,
                    run,
                    stacks,
                    manifest,
                });
            }
            PerformanceOperation::Bloat(_) => {
                let functions = run_phase(
                    gateway,
                    &PhaseRequest {
                        name: &names.containers[6],
                        operation_id: &names.operation_id,
                        volumes: &base,
                        phase: PerformancePhase::BloatFunctions,
                        operation,
                        deadline: phase_deadline(deadline, 2, cancel)?,
                        output_limit: BLOAT_OUTPUT,
                    },
                    deadline,
                    cancel,
                )?;
                removed.push(&names.containers[6]);
                let crates = run_phase(
                    gateway,
                    &PhaseRequest {
                        name: &names.containers[7],
                        operation_id: &names.operation_id,
                        volumes: &base,
                        phase: PerformancePhase::BloatCrates,
                        operation,
                        deadline: phase_deadline(deadline, 4, cancel)?,
                        output_limit: BLOAT_OUTPUT,
                    },
                    deadline,
                    cancel,
                )?;
                removed.push(&names.containers[7]);
                let mut measurements = Vec::new();
                for (index, phase) in [
                    PerformancePhase::BloatFileSize,
                    PerformancePhase::BloatFileDigest,
                    PerformancePhase::BloatFileHeader,
                ]
                .into_iter()
                .enumerate()
                {
                    let capture = run_phase(
                        gateway,
                        &PhaseRequest {
                            name: &names.containers[8 + index],
                            operation_id: &names.operation_id,
                            volumes: &base,
                            phase,
                            operation,
                            deadline: phase_deadline(deadline, 6, cancel)?,
                            output_limit: MEASUREMENT_OUTPUT,
                        },
                        deadline,
                        cancel,
                    )?;
                    removed.push(&names.containers[8 + index]);
                    measurements.push(capture);
                }
                revalidate(
                    gateway,
                    &guardians,
                    &removed,
                    &names.operation_id,
                    deadline,
                    cancel,
                )?;
                let mut measurements = measurements.into_iter();
                let (Some(size), Some(file_digest), Some(header)) = (
                    measurements.next(),
                    measurements.next(),
                    measurements.next(),
                ) else {
                    return Err(ExecutionError::Infrastructure.into());
                };
                work.captures.extend([
                    capture_identity(&functions),
                    capture_identity(&crates),
                    capture_identity(&size),
                    capture_identity(&file_digest),
                    capture_identity(&header),
                ]);
                work.bloat = Some(BloatOutput {
                    functions,
                    crates,
                    size,
                    digest: file_digest,
                    header,
                });
            }
        }
        Ok(work)
    })();

    let cleanup_deadline = Instant::now() + CLEANUP;
    let source_cleanup = cleanup_until(
        gateway,
        &all_containers,
        &names.source_volume,
        &names.operation_id,
        cleanup_deadline,
    );
    let vendor_cleanup = cleanup_until_with_options(
        gateway,
        &[],
        &names.vendor_volume,
        &names.operation_id,
        vendor_options,
        cleanup_deadline,
    );
    let config_cleanup = cleanup_until(
        gateway,
        &[],
        &names.config_volume,
        &names.operation_id,
        cleanup_deadline,
    );
    let target_cleanup = cleanup_target_until(
        gateway,
        &names.target_volume,
        &names.operation_id,
        cleanup_deadline,
    );
    let mut output_cleanup = Ok(());
    for volume in &names.output_volumes {
        let result = cleanup_until(gateway, &[], volume, &names.operation_id, cleanup_deadline);
        if result.is_err() {
            output_cleanup = result;
        }
    }
    source_cleanup?;
    vendor_cleanup?;
    config_cleanup?;
    target_cleanup?;
    output_cleanup?;
    let work = work?;
    budget_error(deadline, cancel)?;

    let source_volume = fingerprint_volume("<source-volume>", "<source>", VOLUME_OPTIONS);
    let vendor_volume = fingerprint_volume("<vendor-volume>", "<vendor>", vendor_options);
    let config_volume = fingerprint_volume("<config-volume>", "<config>", VOLUME_OPTIONS);
    let target_volume = fingerprint_volume("<target-volume>", "<target>", TARGET_VOLUME_OPTIONS);
    let output_volume = fingerprint_volume("<output-volume>", "<output>", VOLUME_OPTIONS);
    let volumes = PerformanceVolumes {
        source: &source_volume,
        vendor: &vendor_volume,
        config: &config_volume,
        target: &target_volume,
        output: Some(&output_volume),
    };
    let execution_fingerprint = execution_fingerprint_for_runtime(
        &gateway.configuration_fingerprint()?,
        gateway.image_id(),
        gateway.inner.state.path(),
        PerformanceFingerprintInputs {
            operation,
            volumes: &volumes,
            source_archive: &source_archive,
            vendor_archive: vendor_archive.commitment(),
            config_archive: &config,
            metadata: &work.metadata,
            governor_paths: work.governor_paths.as_ref(),
            vendor_fingerprint: vendor.tree_fingerprint(),
            limits,
            captures: &work.captures,
            artifacts: &work.artifacts,
        },
    )?;
    Ok(PerformanceExecution {
        kind,
        harness: work.harness,
        hardware: work.hardware,
        runs: work.runs,
        profile: work.profile,
        bloat: work.bloat,
        execution_fingerprint,
        source_fingerprint: bytes_fingerprint(&source_archive)?,
        vendor_fingerprint: vendor.tree_fingerprint().clone(),
    })
}

/// [`cleanup_until`] validates a volume against [`VOLUME_OPTIONS`]; the
/// executable target volume carries [`TARGET_VOLUME_OPTIONS`] instead, so its
/// removal is driven here over the same primitives and with the same
/// fail-closed rule: an uncertain removal quarantines the gateway.
fn cleanup_target_until(
    gateway: &RustGateway,
    volume: &str,
    operation_id: &str,
    deadline: Instant,
) -> Result<(), ExecutionError> {
    let cancel = &NeverCancel;
    let clean = match absent(gateway, "volume", volume, deadline, cancel) {
        Ok(true) => true,
        Ok(false) => {
            let inspected = query_control(
                gateway,
                &["volume".into(), "inspect".into(), volume.into()],
                deadline,
                cancel,
            );
            if matches!(inspected, Ok(ref capture) if capture.code == Some(0)
                && parse_volume_with_options(&capture.stdout, volume, operation_id, TARGET_VOLUME_OPTIONS).is_ok())
            {
                mutation_control(
                    gateway,
                    &["volume".into(), "rm".into(), volume.into()],
                    deadline,
                    cancel,
                )
                .is_ok()
                    && matches!(
                        absent(gateway, "volume", volume, deadline, cancel),
                        Ok(true)
                    )
            } else {
                false
            }
        }
        Err(_) => false,
    };
    if clean {
        Ok(())
    } else {
        gateway.inner.quarantined.store(true, Ordering::Release);
        Err(ExecutionError::CleanupUncertain)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(name: &str, options: &str) -> MutationVolume {
        MutationVolume {
            name: name.into(),
            driver: "local".into(),
            scope: "local".into(),
            options: BTreeMap::from([
                ("device".into(), "tmpfs".into()),
                ("o".into(), options.into()),
                ("type".into(), "tmpfs".into()),
            ]),
            labels: labels("fixture"),
            mountpoint: format!("/var/lib/docker/volumes/{name}/_data"),
            cluster_volume: None,
            status: None,
        }
    }

    fn selection() -> BenchmarkSelection {
        BenchmarkSelection {
            package: None,
            bench_target: None,
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
            profile: "bench".into(),
        }
    }

    fn profile_options() -> Result<ProfileOptions, String> {
        ProfileOptions::new("workload".into(), 99, 10).map_err(|error| format!("{error:?}"))
    }

    fn bloat_options(profile: BloatProfile) -> Result<BloatOptions, String> {
        BloatOptions::new("workload".into(), None, profile).map_err(|error| format!("{error:?}"))
    }

    fn capture(code: Option<i32>) -> Capture {
        Capture {
            code,
            stdout: Vec::new(),
            stderr: Vec::new(),
            stdout_truncated: false,
            stderr_truncated: false,
            stop: Stop::Exited,
            duration_ms: 1,
        }
    }

    fn applied_fixture(
        phase: PerformancePhase,
        volumes: &PerformanceVolumes<'_>,
        operation: PerformanceOperation<'_>,
        command: &[String],
    ) -> Result<serde_json::Value, String> {
        let mounts = applied_mount_expectations(phase, volumes, operation.kind())
            .map_err(|error| format!("{error:?}"))?;
        let applied = mounts
            .iter()
            .map(|mount| {
                serde_json::json!({
                    "Type": "volume",
                    "Name": mount.volume.name,
                    "Source": mount.volume.mountpoint,
                    "Destination": mount.target,
                    "Driver": "local",
                    "Mode": "z",
                    "RW": mount.writable,
                    "Propagation": ""
                })
            })
            .collect::<Vec<_>>();
        let requested = mounts
            .iter()
            .map(|mount| {
                serde_json::json!({
                    "Type": "volume",
                    "Source": mount.volume.name,
                    "Target": mount.target,
                    "ReadOnly": !mount.writable,
                    "VolumeOptions": {
                        "NoCopy": true,
                        "Labels": {},
                        "Subpath": "",
                        "DriverConfig": {"Name": "local", "Options": {}}
                    }
                })
            })
            .collect::<Vec<_>>();
        let config = serde_json::json!({
            "Tty": false,
            "OpenStdin": phase.interactive(),
            "AttachStdin": phase.interactive(),
            "StdinOnce": phase.interactive(),
            "User": "65534:65534",
            "Labels": labels("fixture"),
            "Env": phase.environment(operation),
            "Entrypoint": [phase.program()],
            "Cmd": command,
            "WorkingDir": "/source",
            "Image": crate::APPROVED_M4_IMAGE,
            "Volumes": {}
        });
        let host_chunks = [
            serde_json::json!({
                "AutoRemove": false,
                "GroupAdd": [],
                "UTSMode": "",
                "OomKillDisable": false,
                "OomScoreAdj": 0,
                "DeviceCgroupRules": [],
                "StorageOpt": {},
                "Annotations": {},
                "ReadonlyRootfs": true,
                "Runtime": "runc",
                "Init": false
            }),
            serde_json::json!({
                "MaskedPaths": [
                    "/proc/acpi", "/proc/asound", "/proc/interrupts", "/proc/kcore",
                    "/proc/keys", "/proc/latency_stats", "/proc/sched_debug", "/proc/scsi",
                    "/proc/timer_list", "/proc/timer_stats",
                    "/sys/devices/virtual/powercap", "/sys/firmware"
                ],
                "ReadonlyPaths": [
                    "/proc/bus", "/proc/fs", "/proc/irq", "/proc/sys", "/proc/sysrq-trigger"
                ],
                "UsernsMode": "",
                "CgroupParent": "",
                "Sysctls": {},
                "Ulimits": []
            }),
            serde_json::json!({
                "NetworkMode": "none",
                "PidMode": "",
                "IpcMode": "private",
                "CgroupnsMode": "private",
                "CapDrop": ["ALL"],
                "CapAdd": [],
                "VolumesFrom": [],
                "SecurityOpt": [
                    "no-new-privileges=true",
                    format!("seccomp={}", phase.seccomp_profile_json())
                ]
            }),
            serde_json::json!({
                "PidsLimit": 128,
                "NanoCpus": 1_000_000_000i64,
                "Memory": 1_073_741_824i64,
                "MemorySwap": 1_073_741_824i64,
                "ShmSize": 1_048_576i64,
                "Privileged": false,
                "Binds": []
            }),
            serde_json::json!({
                "Tmpfs": {
                    "/work": "rw,exec,nosuid,nodev,size=512m,mode=1777",
                    "/tmp": "rw,nosuid,nodev,noexec,size=64m,mode=1777"
                },
                "LogConfig": {"Type": "none"},
                "Mounts": requested,
                "Devices": [],
                "DeviceRequests": [],
                "PublishAllPorts": false,
                "PortBindings": {},
                "RestartPolicy": {"Name": "no"}
            }),
        ];
        let mut host = serde_json::Map::new();
        for chunk in host_chunks {
            let serde_json::Value::Object(chunk) = chunk else {
                return Err("HostConfig fixture object".to_owned());
            };
            host.extend(chunk);
        }
        Ok(serde_json::json!([{
            "Config": config,
            "HostConfig": host,
            "Mounts": applied
        }]))
    }

    fn set_fixture_value(
        document: &mut serde_json::Value,
        pointer: &str,
        changed: serde_json::Value,
    ) -> Result<(), String> {
        if let Some(slot) = document.pointer_mut(pointer) {
            *slot = changed;
            return Ok(());
        }
        let (parent, key) = pointer
            .rsplit_once('/')
            .ok_or_else(|| format!("mutation path {pointer}"))?;
        document
            .pointer_mut(parent)
            .and_then(serde_json::Value::as_object_mut)
            .ok_or_else(|| format!("mutation path {pointer}"))?
            .insert(key.to_owned(), changed);
        Ok(())
    }

    #[test]
    fn benchmark_argv_is_closed_and_carries_the_frozen_harness_parameters() {
        let base = PerformancePhase::BenchRun.arguments(PerformanceOperation::Benchmark {
            selection: &selection(),
            run_count: 3,
            harness_parameters: true,
        });
        assert_eq!(
            base,
            [
                "bench",
                "--config",
                "source.crates-io.replace-with=\"rust-mcp-vendor\"",
                "--frozen",
                "--offline",
                "--color=never",
                "--target-dir=/work/target",
                "--",
                "--noplot",
                "--color",
                "never",
                "--warm-up-time",
                "3",
                "--measurement-time",
                "5",
                "--sample-size",
                "30",
            ]
        );
        let mut selected = selection();
        selected.bench_target = Some("throughput".into());
        selected.package = Some("member".into());
        selected.features = vec!["std".into(), "extra".into()];
        selected.all_features = true;
        selected.no_default_features = true;
        let full = PerformancePhase::BenchRun.arguments(PerformanceOperation::Benchmark {
            selection: &selected,
            run_count: 1,
            harness_parameters: true,
        });
        assert_eq!(
            &full[7..12],
            [
                "--bench=throughput",
                "--package=member",
                "--features=std,extra",
                "--all-features",
                "--no-default-features",
            ]
        );
        assert_eq!(full[12], "--");
        assert_eq!(
            PerformancePhase::BenchExport.arguments(PerformanceOperation::Benchmark {
                selection: &selection(),
                run_count: 1,
                harness_parameters: true,
            }),
            [
                "--create",
                "--file=-",
                "--format=ustar",
                "--directory=/criterion",
                ".",
            ]
        );
    }

    #[test]
    fn profile_argv_never_carries_a_peer_path_or_argument() -> Result<(), String> {
        let options = profile_options()?;
        let operation = PerformanceOperation::Profile(&options);
        assert_eq!(
            PerformancePhase::ProfileBuild.arguments(operation),
            [
                "build",
                "--config",
                "source.crates-io.replace-with=\"rust-mcp-vendor\"",
                "--release",
                "--frozen",
                "--offline",
                "--color=never",
                "--target-dir=/work/target",
                "--bin=workload",
            ]
        );
        assert_eq!(
            PerformancePhase::ProfileRun.arguments(operation),
            [
                "--frequency-hz",
                "99",
                "--duration-ms",
                "10000",
                "--max-samples",
                "2000000",
                "--max-depth",
                "127",
                "--stacks",
                "/profile/stacks.txt",
                "--manifest",
                "/profile/manifest.json",
                "--",
                "/work/target/release/workload",
            ]
        );
        assert_eq!(
            PerformancePhase::ProfileExport.arguments(operation),
            [
                "--create",
                "--file=-",
                "--format=ustar",
                "--no-recursion",
                "--directory=/profile",
                "stacks.txt",
                "manifest.json",
            ]
        );
        // The sampling window is capped by the product, never by the request.
        let long = ProfileOptions::new("workload".into(), 999, 60)
            .map_err(|error| format!("{error:?}"))?;
        let argv = PerformancePhase::ProfileRun.arguments(PerformanceOperation::Profile(&long));
        assert_eq!(argv[3], PROFILE_MAX_SAMPLING_MS.to_string());
        Ok(())
    }

    #[test]
    fn bloat_argv_starts_with_the_subcommand_and_measures_a_gateway_built_path()
    -> Result<(), String> {
        let release = bloat_options(BloatProfile::Release)?;
        let operation = PerformanceOperation::Bloat(&release);
        assert_eq!(
            PerformancePhase::BloatFunctions.arguments(operation),
            [
                "bloat",
                "--config",
                "source.crates-io.replace-with=\"rust-mcp-vendor\"",
                "--release",
                "--frozen",
                "--message-format",
                "json",
                "-n",
                "0",
                "--bin=workload",
                "--target-dir=/work/target",
            ]
        );
        let crates = PerformancePhase::BloatCrates.arguments(operation);
        assert_eq!(crates.last().map(String::as_str), Some("--crates"));
        assert_eq!(crates.len(), 12);
        assert_eq!(
            PerformancePhase::BloatFileSize.arguments(operation),
            ["--format=%s", "/work/target/release/workload"]
        );
        assert_eq!(
            PerformancePhase::BloatFileDigest.arguments(operation),
            ["/work/target/release/workload"]
        );
        assert_eq!(
            PerformancePhase::BloatFileHeader.arguments(operation),
            ["-h", "/work/target/release/workload"]
        );
        // ADR-076 §6 keeps two profiles, but `cargo-bloat` 0.12.1 cannot be
        // given `release-lto` as a profile name: the argv is identical and the
        // difference travels in the product-owned environment instead. Both
        // profiles therefore build into `release`, so the oracle path is too.
        let lto = bloat_options(BloatProfile::ReleaseLto)?;
        let operation = PerformanceOperation::Bloat(&lto);
        assert_eq!(
            PerformancePhase::BloatFunctions.arguments(operation),
            PerformancePhase::BloatFunctions.arguments(PerformanceOperation::Bloat(&release))
        );
        assert!(
            !PerformancePhase::BloatFunctions
                .arguments(operation)
                .iter()
                .any(|value| value.contains("release-lto"))
        );
        assert_eq!(
            PerformancePhase::BloatFileSize.arguments(operation)[1],
            "/work/target/release/workload"
        );
        assert!(
            PerformancePhase::BloatFunctions
                .environment(operation)
                .contains(&"CARGO_PROFILE_RELEASE_LTO=fat".to_owned())
        );
        assert!(
            PerformancePhase::BloatCrates
                .environment(operation)
                .contains(&"CARGO_PROFILE_RELEASE_LTO=fat".to_owned())
        );
        assert!(
            !PerformancePhase::BloatFileSize
                .environment(operation)
                .contains(&"CARGO_PROFILE_RELEASE_LTO=fat".to_owned())
        );
        assert!(
            !PerformancePhase::BloatFunctions
                .environment(PerformanceOperation::Bloat(&release))
                .contains(&"CARGO_PROFILE_RELEASE_LTO=fat".to_owned())
        );
        let packaged = BloatOptions::new(
            "workload".into(),
            Some("member".into()),
            BloatProfile::Release,
        )
        .map_err(|error| format!("{error:?}"))?;
        let argv = PerformancePhase::BloatCrates.arguments(PerformanceOperation::Bloat(&packaged));
        assert_eq!(argv[11], "--package=member");
        assert_eq!(argv[12], "--crates");
        Ok(())
    }

    #[test]
    fn metadata_resolves_the_whole_graph_and_probes_read_only_facts() {
        let operation = PerformanceOperation::Benchmark {
            selection: &selection(),
            run_count: 1,
            harness_parameters: true,
        };
        let metadata = PerformancePhase::Metadata.arguments(operation);
        assert_eq!(
            metadata,
            [
                "metadata",
                "--frozen",
                "--offline",
                "--format-version=1",
                "--manifest-path=/source/Cargo.toml",
            ]
        );
        assert!(!metadata.iter().any(|value| value == "--no-deps"));
        assert_eq!(PerformancePhase::CpuProbe.program(), "/usr/bin/cat");
        assert_eq!(
            PerformancePhase::CpuProbe.arguments(operation),
            ["/proc/cpuinfo"]
        );
        assert_eq!(PerformancePhase::GovernorProbe.program(), "/usr/bin/cat");
        assert!(
            PerformancePhase::GovernorProbe
                .arguments(operation)
                .is_empty()
        );
        assert_eq!(PerformancePhase::KernelProbe.program(), "/usr/bin/uname");
        assert_eq!(
            PerformancePhase::KernelProbe.arguments(operation),
            ["-s", "-r", "-m"]
        );
        assert_eq!(PerformancePhase::BloatFileSize.program(), "/usr/bin/stat");
        assert_eq!(
            PerformancePhase::BloatFileDigest.program(),
            "/usr/bin/sha256sum"
        );
        assert_eq!(
            PerformancePhase::BloatFileHeader.program(),
            "/usr/bin/readelf"
        );
    }

    const ALL_PHASES: [PerformancePhase; 22] = [
        PerformancePhase::SourceGuardian,
        PerformancePhase::VendorGuardian,
        PerformancePhase::ConfigGuardian,
        PerformancePhase::TargetGuardian,
        PerformancePhase::OutputGuardian,
        PerformancePhase::SourceIngest,
        PerformancePhase::VendorIngest,
        PerformancePhase::ConfigIngest,
        PerformancePhase::Metadata,
        PerformancePhase::CpuProbe,
        PerformancePhase::KernelProbe,
        PerformancePhase::GovernorProbe,
        PerformancePhase::BenchRun,
        PerformancePhase::BenchExport,
        PerformancePhase::ProfileBuild,
        PerformancePhase::ProfileRun,
        PerformancePhase::ProfileExport,
        PerformancePhase::BloatFunctions,
        PerformancePhase::BloatCrates,
        PerformancePhase::BloatFileSize,
        PerformancePhase::BloatFileDigest,
        PerformancePhase::BloatFileHeader,
    ];

    #[test]
    fn exactly_one_phase_selects_the_profiling_seccomp_profile() {
        let profiling = ALL_PHASES
            .iter()
            .filter(|phase| phase.seccomp_profile_name() == "seccomp-rust-profile.json")
            .copied()
            .collect::<Vec<_>>();
        assert_eq!(profiling, [PerformancePhase::ProfileRun]);
        for phase in ALL_PHASES {
            let expected = if phase == PerformancePhase::ProfileRun {
                (
                    "seccomp-rust-profile.json",
                    include_str!("seccomp-rust-profile.json"),
                )
            } else {
                (
                    "seccomp-rust-quality.json",
                    include_str!("seccomp-rust-quality.json"),
                )
            };
            assert_eq!(phase.seccomp_profile_name(), expected.0, "{phase:?}");
            assert_eq!(phase.seccomp_profile_json(), expected.1, "{phase:?}");
        }
    }

    /// The test-only switch the native denial control arms. It moves exactly
    /// one phase to the other committed profile and changes nothing else: the
    /// phase list, the argv and the profile bytes are the shipped ones, so the
    /// denial it produces is the product's own refusal path and not a mock.
    #[test]
    fn the_denial_switch_moves_only_the_sampling_phase_to_the_quality_profile() {
        assert!(!perf_event_open_denied());
        DENY_PERF_EVENT_OPEN.with(|switch| switch.set(true));
        assert_eq!(
            PerformancePhase::ProfileRun.seccomp_profile_name(),
            "seccomp-rust-quality.json"
        );
        assert_eq!(
            PerformancePhase::ProfileRun.seccomp_profile_json(),
            include_str!("seccomp-rust-quality.json")
        );
        assert!(
            ALL_PHASES
                .iter()
                .all(|phase| phase.seccomp_profile_name() == "seccomp-rust-quality.json")
        );
        // Only the profile moves: the phase still runs the helper with the argv
        // it always had.
        assert_eq!(
            PerformancePhase::ProfileRun.program(),
            "/opt/perf/bin/rust-mcp-profile-helper"
        );
        DENY_PERF_EVENT_OPEN.with(|switch| switch.set(false));
        assert_eq!(
            PerformancePhase::ProfileRun.seccomp_profile_name(),
            "seccomp-rust-profile.json"
        );
    }

    #[test]
    fn profiling_profile_only_adds_perf_event_open() -> Result<(), Box<dyn std::error::Error>> {
        let quality: serde_json::Value =
            serde_json::from_str(include_str!("seccomp-rust-quality.json"))?;
        let profiling: serde_json::Value =
            serde_json::from_str(include_str!("seccomp-rust-profile.json"))?;
        for key in ["defaultAction", "defaultErrnoRet", "archMap"] {
            assert_eq!(profiling[key], quality[key], "changed {key}");
        }
        let quality_rules = quality["syscalls"].as_array().ok_or("quality rules")?;
        let profiling_rules = profiling["syscalls"].as_array().ok_or("profiling rules")?;
        assert_eq!(&profiling_rules[..quality_rules.len()], quality_rules);
        assert_eq!(profiling_rules.len(), quality_rules.len() + 1);
        assert_eq!(
            profiling_rules[quality_rules.len()],
            serde_json::json!({"names":["perf_event_open"],"action":"SCMP_ACT_ALLOW"})
        );
        Ok(())
    }

    #[test]
    fn source_is_writable_only_in_its_ingest_and_scratch_only_where_it_is_produced() {
        for phase in ALL_PHASES {
            let mounted = phase.mounts();
            let writable = phase.permissions();
            assert_eq!(
                writable.source,
                phase == PerformancePhase::SourceIngest,
                "/source writable in {phase:?}"
            );
            assert_eq!(
                writable.vendor,
                phase == PerformancePhase::VendorIngest,
                "/rust-mcp-vendor writable in {phase:?}"
            );
            assert_eq!(
                writable.config,
                phase == PerformancePhase::ConfigIngest,
                "/performance writable in {phase:?}"
            );
            assert_eq!(
                writable.output,
                matches!(
                    phase,
                    PerformancePhase::BenchRun | PerformancePhase::ProfileRun
                ),
                "output writable in {phase:?}"
            );
            for (mounted, writable, label) in [
                (mounted.source, writable.source, "source"),
                (mounted.vendor, writable.vendor, "vendor"),
                (mounted.config, writable.config, "config"),
                (mounted.target, writable.target, "target"),
                (mounted.output, writable.output, "output"),
            ] {
                assert!(!writable || mounted, "{label} writable unmounted {phase:?}");
            }
        }
        assert!(!PerformancePhase::BloatFileSize.permissions().target);
        assert!(!PerformancePhase::ProfileRun.permissions().target);
        assert!(PerformancePhase::ProfileBuild.permissions().target);
    }

    #[test]
    fn output_and_target_volumes_are_absent_from_every_phase_that_must_not_see_them() {
        for phase in [
            PerformancePhase::SourceGuardian,
            PerformancePhase::VendorGuardian,
            PerformancePhase::ConfigGuardian,
            PerformancePhase::SourceIngest,
            PerformancePhase::VendorIngest,
            PerformancePhase::ConfigIngest,
            PerformancePhase::Metadata,
            PerformancePhase::CpuProbe,
            PerformancePhase::KernelProbe,
            PerformancePhase::GovernorProbe,
        ] {
            assert!(!phase.mounts().output, "output mounted in {phase:?}");
            assert!(!phase.mounts().target, "target mounted in {phase:?}");
        }
        for phase in [
            PerformancePhase::CpuProbe,
            PerformancePhase::KernelProbe,
            PerformancePhase::GovernorProbe,
        ] {
            assert_eq!(phase.mounts(), PerformanceMounts::default(), "{phase:?}");
        }
    }

    #[test]
    fn mount_arguments_are_exact_ordered_and_read_only_outside_the_producing_phase() {
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let output = volume("output", VOLUME_OPTIONS);
        let volumes = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: Some(&output),
        };
        assert_eq!(
            mount_arguments(
                PerformancePhase::BenchRun,
                &volumes,
                PerformanceKind::Benchmark
            ),
            [
                "--mount=type=volume,source=source,target=/source,volume-nocopy,volume-driver=local,readonly",
                "--mount=type=volume,source=vendor,target=/rust-mcp-vendor,volume-nocopy,volume-driver=local,readonly",
                "--mount=type=volume,source=config,target=/performance,volume-nocopy,volume-driver=local,readonly",
                "--mount=type=volume,source=target,target=/work/target,volume-nocopy,volume-driver=local",
                "--mount=type=volume,source=output,target=/criterion,volume-nocopy,volume-driver=local",
            ]
        );
        assert_eq!(
            mount_arguments(
                PerformancePhase::ProfileRun,
                &volumes,
                PerformanceKind::Profile
            ),
            [
                "--mount=type=volume,source=target,target=/work/target,volume-nocopy,volume-driver=local,readonly",
                "--mount=type=volume,source=output,target=/profile,volume-nocopy,volume-driver=local",
            ]
        );
        assert_eq!(
            mount_arguments(
                PerformancePhase::BloatFileDigest,
                &volumes,
                PerformanceKind::Bloat
            ),
            [
                "--mount=type=volume,source=target,target=/work/target,volume-nocopy,volume-driver=local,readonly",
            ]
        );
        assert_eq!(
            mount_arguments(
                PerformancePhase::SourceIngest,
                &volumes,
                PerformanceKind::Bloat
            ),
            ["--mount=type=volume,source=source,target=/source,volume-nocopy,volume-driver=local"]
        );
        assert!(
            mount_arguments(PerformancePhase::CpuProbe, &volumes, PerformanceKind::Bloat)
                .is_empty()
        );
        let without_output = PerformanceVolumes {
            output: None,
            ..volumes
        };
        assert!(
            mount_arguments(
                PerformancePhase::OutputGuardian,
                &without_output,
                PerformanceKind::Profile
            )
            .is_empty()
        );
    }

    #[test]
    fn environment_redirects_cargo_home_and_scopes_criterion_and_rustflags() -> Result<(), String> {
        let base = [
            "CARGO_HOME=/performance/cargo-home",
            "CARGO_INCREMENTAL=0",
            "CARGO_NET_OFFLINE=true",
            "CARGO_TARGET_DIR=/work/target",
            "HOME=/work",
            "PATH=/opt/rust/bin:/usr/bin:/bin",
            "RUSTC=/opt/rust/bin/rustc",
            "RUSTDOC=/opt/rust/bin/rustdoc",
            "RUSTFMT=/opt/rust/bin/rustfmt",
            "TMPDIR=/tmp",
        ];
        let selected = selection();
        let profile = profile_options()?;
        let bloat = bloat_options(BloatProfile::Release)?;
        for operation in [
            PerformanceOperation::Benchmark {
                selection: &selected,
                run_count: 3,
                harness_parameters: true,
            },
            PerformanceOperation::Profile(&profile),
            PerformanceOperation::Bloat(&bloat),
        ] {
            for phase in ALL_PHASES {
                let environment = phase.environment(operation);
                let criterion = environment
                    .iter()
                    .any(|value| value == "CRITERION_HOME=/criterion");
                let rustflags = environment
                    .iter()
                    .any(|value| value == "RUSTFLAGS=-C force-frame-pointers=yes");
                assert_eq!(
                    criterion,
                    phase == PerformancePhase::BenchRun,
                    "CRITERION_HOME in {phase:?}"
                );
                assert_eq!(
                    rustflags,
                    phase == PerformancePhase::ProfileBuild,
                    "RUSTFLAGS in {phase:?}"
                );
                assert!(
                    environment
                        .iter()
                        .all(|value| value != "CARGO_HOME=/opt/rust"),
                    "host CARGO_HOME leaked into {phase:?}"
                );
                if criterion || rustflags {
                    assert_eq!(environment.len(), base.len() + 1, "{phase:?}");
                } else {
                    assert_eq!(environment, base, "environment for {phase:?}");
                }
            }
        }
        Ok(())
    }

    #[test]
    fn complete_container_arguments_cover_every_phase_without_a_runtime() -> Result<(), String> {
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let output = volume("output", VOLUME_OPTIONS);
        let volumes = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: Some(&output),
        };
        let selected = selection();
        let profile = profile_options()?;
        let bloat = bloat_options(BloatProfile::Release)?;
        for operation in [
            PerformanceOperation::Benchmark {
                selection: &selected,
                run_count: 3,
                harness_parameters: true,
            },
            PerformanceOperation::Profile(&profile),
            PerformanceOperation::Bloat(&bloat),
        ] {
            for phase in operation.phases() {
                let arguments = create_arguments_for_runtime(
                    crate::APPROVED_M4_IMAGE,
                    std::path::Path::new("/state"),
                    "container",
                    "fixture",
                    &volumes,
                    phase,
                    operation,
                )
                .map_err(|error| format!("{error:?}"))?;
                assert!(arguments.contains(&"--name=container".to_owned()));
                assert!(arguments.contains(&"--user=65534:65534".to_owned()));
                assert!(arguments.contains(&"--network=none".to_owned()));
                assert!(arguments.contains(&"--read-only".to_owned()));
                assert!(arguments.contains(&"--cap-drop=ALL".to_owned()));
                assert!(arguments.contains(&"--security-opt=no-new-privileges=true".to_owned()));
                assert!(!arguments.iter().any(|value| value.starts_with("--cap-add")));
                assert!(!arguments.iter().any(|value| value.contains("privileged")));
                assert!(arguments.contains(&"--label=org.rust-mcp.execution=true".to_owned()));
                assert!(arguments.contains(&"--label=org.rust-mcp.rust-job=fixture".to_owned()));
                assert!(arguments.contains(&format!("--entrypoint={}", phase.program())));
                assert!(arguments.contains(&crate::APPROVED_M4_IMAGE.to_owned()));
                assert_eq!(
                    arguments.contains(&"--interactive".to_owned()),
                    phase.interactive(),
                    "{phase:?}"
                );
                let seccomp = std::path::Path::new("/state").join(phase.seccomp_profile_name());
                assert!(
                    arguments.contains(&format!("--security-opt=seccomp={}", seccomp.display()))
                );
                for argument in phase.arguments(operation) {
                    assert!(arguments.contains(&argument), "{phase:?}: {argument}");
                }
                let source_mount = arguments
                    .iter()
                    .find(|value| value.contains("target=/source,"));
                assert_eq!(
                    source_mount.is_some_and(|value| !value.ends_with(",readonly")),
                    phase == PerformancePhase::SourceIngest,
                    "/source writability for {phase:?}"
                );
            }
        }
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn container_arguments_reject_a_non_utf8_profile_path() -> Result<(), String> {
        use std::os::unix::ffi::OsStrExt;
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let volumes = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: None,
        };
        let bloat = bloat_options(BloatProfile::Release)?;
        assert_eq!(
            create_arguments_for_runtime(
                crate::APPROVED_M4_IMAGE,
                std::path::Path::new(std::ffi::OsStr::from_bytes(b"/state/\xff")),
                "container",
                "fixture",
                &volumes,
                PerformancePhase::BloatFunctions,
                PerformanceOperation::Bloat(&bloat),
            ),
            Err(ExecutionError::InvalidConfiguration)
        );
        Ok(())
    }

    #[test]
    fn budget_reserves_control_headroom_before_dividing_it() {
        assert_eq!(
            work_budget_ms(CONTROL_RESERVE_MS),
            Err(PerformanceError::Timeout)
        );
        assert_eq!(work_budget_ms(CONTROL_RESERVE_MS + 1), Ok(1));
        assert_eq!(work_budget_ms(BENCHMARK_BUDGET_MS), Ok(866_000));
        assert_eq!(work_budget_ms(PROFILE_BUDGET_MS), Ok(266_000));
        assert_eq!(work_budget_ms(BLOAT_BUDGET_MS), Ok(266_000));
        assert_eq!(work_budget_ms(0), Err(PerformanceError::Timeout));
        assert_eq!(share_ms(866_000, 6), Ok(144_333));
        assert_eq!(share_ms(866_000, 1), Ok(866_000));
        assert_eq!(share_ms(10, 0), Err(PerformanceError::Timeout));
        assert_eq!(share_ms(3, 4), Err(PerformanceError::Timeout));
        // The three budgets are the ADR-076 §7 ceilings, sampling included.
        assert_eq!(BENCHMARK_BUDGET_MS, 900_000);
        assert_eq!(PROFILE_BUDGET_MS, 300_000);
        assert_eq!(BLOAT_BUDGET_MS, 300_000);
        assert_eq!(PROFILE_MAX_SAMPLING_MS, 60_000);
    }

    #[test]
    fn vendor_capture_volume_matches_adr_078_and_changes_the_fingerprint() -> Result<(), String> {
        assert_eq!(
            VENDOR_CAPTURE_VOLUME_OPTIONS,
            "size=512m,nr_inodes=32768,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec"
        );
        assert_eq!(
            rust_engineering_domain::vendor_capture::VENDOR_CAPTURE_MAX_TOTAL_BYTES / (1024 * 1024),
            512
        );
        assert_eq!(
            rust_engineering_domain::vendor_capture::VENDOR_CAPTURE_MAX_ENTRIES,
            32_768
        );

        let source = fingerprint_volume("<source-volume>", "<source>", VOLUME_OPTIONS);
        let small_vendor = fingerprint_volume("<vendor-volume>", "<vendor>", VOLUME_OPTIONS);
        let captured_vendor =
            fingerprint_volume("<vendor-volume>", "<vendor>", VENDOR_CAPTURE_VOLUME_OPTIONS);
        let config = fingerprint_volume("<config-volume>", "<config>", VOLUME_OPTIONS);
        let target = fingerprint_volume("<target-volume>", "<target>", TARGET_VOLUME_OPTIONS);
        assert_eq!(
            captured_vendor.options.get("o").map(String::as_str),
            Some(VENDOR_CAPTURE_VOLUME_OPTIONS)
        );

        let configuration: ExecutionFingerprint = format!("sha256:{}", "a".repeat(64))
            .parse()
            .map_err(|error| format!("{error:?}"))?;
        let vendor_fingerprint: SourceFingerprint = format!("sha256:{}", "b".repeat(64))
            .parse()
            .map_err(|error| format!("{error:?}"))?;
        let limits = ExecutionLimits::new_job(BLOAT_BUDGET_MS, 512 * 1024)
            .ok_or_else(|| "invalid fixture limits".to_owned())?;
        let bloat = bloat_options(BloatProfile::Release)?;
        let operation = PerformanceOperation::Bloat(&bloat);
        let fingerprint = |vendor| {
            let volumes = PerformanceVolumes {
                source: &source,
                vendor,
                config: &config,
                target: &target,
                output: None,
            };
            execution_fingerprint_for_runtime(
                &configuration,
                crate::APPROVED_M4_IMAGE,
                std::path::Path::new("/state"),
                PerformanceFingerprintInputs {
                    operation,
                    volumes: &volumes,
                    source_archive: b"source",
                    vendor_archive: b"vendor",
                    config_archive: b"config",
                    metadata: b"metadata",
                    governor_paths: None,
                    vendor_fingerprint: &vendor_fingerprint,
                    limits,
                    captures: &[],
                    artifacts: &[],
                },
            )
            .map_err(|error| format!("{error:?}"))
        };
        assert_ne!(fingerprint(&small_vendor)?, fingerprint(&captured_vendor)?);
        Ok(())
    }

    #[test]
    fn governor_probe_uses_only_closed_paths_and_binds_its_effective_argv() -> Result<(), String> {
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let volumes = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: None,
        };
        let selected = selection();
        let operation = PerformanceOperation::Benchmark {
            selection: &selected,
            run_count: 3,
            harness_parameters: true,
        };
        let topology = performance_environment::parse_cpuinfo(b"processor: 7\nprocessor: 1\n");
        let paths = performance_environment::governor_paths(&topology)
            .ok_or_else(|| "fixture topology is not observable".to_owned())?;
        let argv = governor_arguments_for_runtime(
            crate::APPROVED_M4_IMAGE,
            std::path::Path::new("/state"),
            "container",
            "fixture",
            &volumes,
            operation,
            &paths,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert!(argv.contains(&"--entrypoint=/usr/bin/cat".to_owned()));
        assert!(argv.ends_with(paths.as_slice()));
        assert!(
            mount_arguments(PerformancePhase::GovernorProbe, &volumes, operation.kind()).is_empty()
        );

        let configuration: ExecutionFingerprint = format!("sha256:{}", "a".repeat(64))
            .parse()
            .map_err(|error| format!("{error:?}"))?;
        let vendor_fingerprint: SourceFingerprint = format!("sha256:{}", "b".repeat(64))
            .parse()
            .map_err(|error| format!("{error:?}"))?;
        let limits = ExecutionLimits::new_job(BENCHMARK_BUDGET_MS, 512 * 1024)
            .ok_or_else(|| "invalid fixture limits".to_owned())?;
        let fingerprint = |governor_paths: Option<&GovernorPaths>| {
            execution_fingerprint_for_runtime(
                &configuration,
                crate::APPROVED_M4_IMAGE,
                std::path::Path::new("/state"),
                PerformanceFingerprintInputs {
                    operation,
                    volumes: &volumes,
                    source_archive: b"source",
                    vendor_archive: b"vendor",
                    config_archive: b"config",
                    metadata: b"metadata",
                    governor_paths,
                    vendor_fingerprint: &vendor_fingerprint,
                    limits,
                    captures: &[],
                    artifacts: &[],
                },
            )
            .map_err(|error| format!("{error:?}"))
        };
        assert_ne!(fingerprint(None)?, fingerprint(Some(&paths))?);
        Ok(())
    }

    #[test]
    fn phase_deadline_never_outlives_the_operation_deadline() -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_millis(BLOAT_BUDGET_MS);
        let share = phase_deadline(deadline, 4, &NeverCancel).map_err(|e| format!("{e:?}"))?;
        assert!(share <= deadline);
        assert!(share > Instant::now());
        let exhausted = Instant::now() + Duration::from_millis(CONTROL_RESERVE_MS);
        assert_eq!(
            phase_deadline(exhausted, 1, &NeverCancel),
            Err(PerformanceError::Timeout)
        );
        Ok(())
    }

    #[test]
    fn harness_detection_reads_the_resolved_graph_and_never_a_target_name() -> Result<(), String> {
        let document = |packages: &str| format!("{{\"packages\":[{packages}]}}");
        assert_eq!(
            detect_harness(document("{\"name\":\"criterion\",\"version\":\"0.8.2\"}").as_bytes()),
            Ok(HarnessDetection::Criterion {
                version: "0.8.2".into()
            })
        );
        assert_eq!(
            detect_harness(document("{\"name\":\"criterion\",\"version\":\"0.5.1\"}").as_bytes()),
            Ok(HarnessDetection::CriterionUnapproved {
                version: "0.5.1".into()
            })
        );
        assert_eq!(
            detect_harness(document("{\"name\":\"benchmarks\",\"version\":\"0.8.2\"}").as_bytes()),
            Ok(HarnessDetection::Unrecognized)
        );
        assert_eq!(
            detect_harness(document("").as_bytes()),
            Ok(HarnessDetection::Unrecognized)
        );
        assert_eq!(
            detect_harness(b"not json"),
            Err(PerformanceError::InvalidMetadata)
        );
        Ok(())
    }

    #[test]
    fn options_that_could_reach_argv_as_a_flag_or_a_path_are_refused() -> Result<(), String> {
        assert_eq!(validate_benchmark(&selection(), 3), Ok(()));
        for count in [0, 4, u8::MAX] {
            assert_eq!(
                validate_benchmark(&selection(), count),
                Err(PerformanceError::InvalidOptions)
            );
        }
        for target in ["../evil", "--all-features", "a b", "a;b", ""] {
            let mut selected = selection();
            selected.bench_target = Some(target.into());
            assert_eq!(
                validate_benchmark(&selected, 1),
                Err(PerformanceError::InvalidOptions),
                "accepted {target:?}"
            );
        }
        for feature in ["-Zbad", "a,b", "a b", ""] {
            let mut selected = selection();
            selected.features = vec![feature.into()];
            assert_eq!(
                validate_benchmark(&selected, 1),
                Err(PerformanceError::InvalidOptions),
                "accepted {feature:?}"
            );
        }
        let mut selected = selection();
        selected.features = vec!["std".into(), "dep/feature".into()];
        assert_eq!(validate_benchmark(&selected, 1), Ok(()));
        Ok(())
    }

    #[test]
    fn hardware_probes_leave_an_unobservable_field_unknown() {
        let intel = b"processor\t: 0\nmodel name\t: Fixture CPU\nprocessor\t: 1\nmodel name\t: Fixture CPU\n";
        assert_eq!(
            performance_environment::parse_cpuinfo(intel).logical_cpu_ids,
            [0, 1]
        );
        let arm = b"processor\t: 0\nCPU implementer\t: 0x61\nCPU architecture: 8\nCPU variant\t: 0x0\nCPU part\t: 0x000\nCPU revision\t: 0\n";
        assert_eq!(
            performance_environment::parse_cpuinfo(arm)
                .cpu_model
                .as_deref(),
            Some("CPU implementer=0x61 CPU part=0x000 CPU revision=0 CPU variant=0x0")
        );
        // Nothing observable: unknown stays unknown, never a plausible value.
        assert_eq!(
            performance_environment::parse_cpuinfo(b""),
            GuestCpuTopology::default()
        );
        assert_eq!(
            performance_environment::parse_cpuinfo(b"processor\t: 0\n").cpu_model,
            None
        );
        assert_eq!(
            parse_uname(b"Linux 6.6.0 aarch64\n"),
            Some("Linux 6.6.0 aarch64".into())
        );
        assert_eq!(parse_uname(b""), None);
        assert_eq!(parse_uname(&[0xff, 0xfe]), None);
    }

    #[test]
    fn fingerprint_inputs_bind_image_argv_limits_source_and_seccomp() -> Result<(), String> {
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let output = volume("output", VOLUME_OPTIONS);
        let volumes = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: Some(&output),
        };
        let configuration: ExecutionFingerprint = format!("sha256:{}", "a".repeat(64))
            .parse()
            .map_err(|error| format!("{error:?}"))?;
        let vendor_fingerprint: SourceFingerprint = format!("sha256:{}", "b".repeat(64))
            .parse()
            .map_err(|error| format!("{error:?}"))?;
        let limits = ExecutionLimits::new_job(BLOAT_BUDGET_MS, 512 * 1024)
            .ok_or_else(|| "invalid fixture limits".to_owned())?;
        let selected = selection();
        let bloat = bloat_options(BloatProfile::Release)?;
        let captures = [capture_identity(&capture(Some(0)))];
        let artifacts = [digest(b"artifact")];
        let fingerprint = |operation,
                           image,
                           state: &str,
                           limits,
                           source_archive: &[u8],
                           captures: &[CaptureIdentity]| {
            execution_fingerprint_for_runtime(
                &configuration,
                image,
                std::path::Path::new(state),
                PerformanceFingerprintInputs {
                    operation,
                    volumes: &volumes,
                    source_archive,
                    vendor_archive: b"vendor",
                    config_archive: b"config",
                    metadata: b"metadata",
                    governor_paths: None,
                    vendor_fingerprint: &vendor_fingerprint,
                    limits,
                    captures,
                    artifacts: &artifacts,
                },
            )
            .map_err(|error| format!("{error:?}"))
        };
        let bloat_operation = PerformanceOperation::Bloat(&bloat);
        let base = fingerprint(
            bloat_operation,
            crate::APPROVED_M4_IMAGE,
            "/state",
            limits,
            b"source",
            &captures,
        )?;
        assert_eq!(
            base,
            fingerprint(
                bloat_operation,
                crate::APPROVED_M4_IMAGE,
                "/state",
                limits,
                b"source",
                &captures
            )?
        );
        // image
        assert_ne!(
            base,
            fingerprint(
                bloat_operation,
                crate::APPROVED_RUST_IMAGE,
                "/state",
                limits,
                b"source",
                &captures
            )?
        );
        // argv, through the operation that generates it
        let lto = bloat_options(BloatProfile::ReleaseLto)?;
        assert_ne!(
            base,
            fingerprint(
                PerformanceOperation::Bloat(&lto),
                crate::APPROVED_M4_IMAGE,
                "/state",
                limits,
                b"source",
                &captures
            )?
        );
        // limits
        let other_limits = ExecutionLimits::new_job(BLOAT_BUDGET_MS, 256 * 1024)
            .ok_or_else(|| "invalid fixture limits".to_owned())?;
        assert_ne!(
            base,
            fingerprint(
                bloat_operation,
                crate::APPROVED_M4_IMAGE,
                "/state",
                other_limits,
                b"source",
                &captures
            )?
        );
        // source digest
        assert_ne!(
            base,
            fingerprint(
                bloat_operation,
                crate::APPROVED_M4_IMAGE,
                "/state",
                limits,
                b"changed",
                &captures
            )?
        );
        // seccomp profile selection, through the phase list of another operation
        let profile = profile_options()?;
        assert_ne!(
            base,
            fingerprint(
                PerformanceOperation::Profile(&profile),
                crate::APPROVED_M4_IMAGE,
                "/state",
                limits,
                b"source",
                &captures
            )?
        );
        // the state path holds the seccomp profile, so it is bound too
        assert_ne!(
            base,
            fingerprint(
                bloat_operation,
                crate::APPROVED_M4_IMAGE,
                "/other-state",
                limits,
                b"source",
                &captures
            )?
        );
        // observed output
        let other = [capture_identity(&capture(Some(1)))];
        assert_ne!(
            base,
            fingerprint(
                bloat_operation,
                crate::APPROVED_M4_IMAGE,
                "/state",
                limits,
                b"source",
                &other
            )?
        );
        // run count is part of the benchmark identity
        let one = fingerprint(
            PerformanceOperation::Benchmark {
                selection: &selected,
                run_count: 1,
                harness_parameters: true,
            },
            crate::APPROVED_M4_IMAGE,
            "/state",
            limits,
            b"source",
            &captures,
        )?;
        let three = fingerprint(
            PerformanceOperation::Benchmark {
                selection: &selected,
                run_count: 3,
                harness_parameters: true,
            },
            crate::APPROVED_M4_IMAGE,
            "/state",
            limits,
            b"source",
            &captures,
        )?;
        assert_eq!(
            one, three,
            "run_count changes phases only through repetition"
        );
        Ok(())
    }

    #[test]
    fn phase_error_mapping_preserves_cleanup_and_separates_deadline_from_cancel() {
        let future = Instant::now() + Duration::from_secs(1);
        let past = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            phase_result::<()>(Err(ExecutionError::Infrastructure), past, &NeverCancel),
            Err(PerformanceError::Timeout)
        );
        assert_eq!(
            phase_result::<()>(Err(ExecutionError::CleanupUncertain), past, &NeverCancel),
            Err(PerformanceError::Inspection(InspectionError::Execution(
                ExecutionError::CleanupUncertain
            )))
        );
        struct Cancel;
        impl ExecutionCancellation for Cancel {
            fn is_cancelled(&self) -> bool {
                true
            }
        }
        assert_eq!(
            phase_result::<()>(Err(ExecutionError::Infrastructure), future, &Cancel),
            Err(PerformanceError::Inspection(InspectionError::Project(
                ProjectError::Cancelled
            )))
        );
        assert_eq!(phase_result::<u8>(Ok(7), future, &NeverCancel), Ok(7));
        assert_eq!(
            phase_result::<u8>(Ok(7), past, &NeverCancel),
            Err(PerformanceError::Timeout)
        );
        assert_eq!(
            phase_result::<()>(Err(ExecutionError::Cancelled), future, &NeverCancel),
            Err(PerformanceError::Inspection(InspectionError::Project(
                ProjectError::Cancelled
            )))
        );
    }

    #[test]
    fn capture_and_container_completion_are_validated_before_publication()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut cancelled = capture(None);
        cancelled.stop = Stop::Cancelled;
        assert_eq!(
            validate_capture_completion(&cancelled),
            Err(PerformanceError::Inspection(InspectionError::Project(
                ProjectError::Cancelled
            )))
        );
        let mut timed_out = capture(None);
        timed_out.stop = Stop::TimedOut;
        assert_eq!(
            validate_capture_completion(&timed_out),
            Err(PerformanceError::Timeout)
        );
        let mut limited = capture(None);
        limited.stop = Stop::OutputLimit;
        assert_eq!(
            validate_capture_completion(&limited),
            Err(PerformanceError::OutputLimit)
        );
        let mut truncated = capture(Some(0));
        truncated.stdout_truncated = true;
        assert_eq!(
            validate_capture_completion(&truncated),
            Err(PerformanceError::OutputLimit)
        );
        let completed = capture(Some(7));
        assert_eq!(validate_capture_completion(&completed), Ok(()));
        let mut inspected = capture(Some(0));
        inspected.stdout = serde_json::to_vec(&serde_json::json!([{
            "State": {
                "Running": false,
                "Pid": 0,
                "ExitCode": 7,
                "Status": "exited",
                "StartedAt": "2026-09-08T12:00:00Z",
                "Error": "",
                "OOMKilled": false
            }
        }]))?;
        assert_eq!(validate_completed_container(&inspected, &completed), Ok(()));
        inspected.stdout = serde_json::to_vec(&serde_json::json!([{
            "State": {
                "Running": false,
                "Pid": 0,
                "ExitCode": 7,
                "Status": "exited",
                "StartedAt": "2026-09-08T12:00:00Z",
                "Error": "",
                "OOMKilled": true
            }
        }]))?;
        assert!(validate_completed_container(&inspected, &completed).is_err());
        inspected.stdout = b"[]".to_vec();
        assert!(validate_completed_container(&inspected, &completed).is_err());
        Ok(())
    }

    #[test]
    fn the_profiling_export_accepts_exactly_its_two_fixed_members() -> Result<(), String> {
        let entry = |name: &str, body: &[u8]| -> Vec<u8> {
            let mut header = vec![0u8; 512];
            header[..name.len()].copy_from_slice(name.as_bytes());
            let size = format!("{:011o}\0", body.len());
            header[124..124 + size.len()].copy_from_slice(size.as_bytes());
            header[156] = b'0';
            let mut block = header;
            block.extend_from_slice(body);
            block.resize(512 + body.len().div_ceil(512) * 512, 0);
            block
        };
        let mut archive = entry("stacks.txt", b"main;work 3\n");
        archive.extend(entry("manifest.json", b"{}"));
        archive.extend([0u8; 1024]);
        assert_eq!(
            decode_profile_tar(&archive, 1024),
            Some((b"main;work 3\n".to_vec(), b"{}".to_vec()))
        );
        let mut foreign = entry("stacks.txt", b"x 1\n");
        foreign.extend(entry("passwd", b"root"));
        assert_eq!(decode_profile_tar(&foreign, 1024), None);
        let mut duplicated = entry("stacks.txt", b"x 1\n");
        duplicated.extend(entry("stacks.txt", b"y 1\n"));
        assert_eq!(decode_profile_tar(&duplicated, 1024), None);
        assert_eq!(
            decode_profile_tar(&entry("stacks.txt", b"x 1\n"), 1024),
            None
        );
        assert_eq!(decode_profile_tar(&archive, 1), None);
        Ok(())
    }

    #[test]
    fn the_ingested_cargo_home_is_the_vendor_backed_one_and_nothing_else() -> Result<(), String> {
        let archive = config_archive().map_err(|error| format!("{error:?}"))?;
        assert!(
            archive
                .windows("cargo-home/config.toml".len())
                .any(|window| window == b"cargo-home/config.toml")
        );
        assert!(
            archive
                .windows(VENDOR_ROOT.len())
                .any(|window| window == VENDOR_ROOT.as_bytes())
        );
        assert_eq!(
            PerformancePhase::ConfigIngest.arguments(PerformanceOperation::Benchmark {
                selection: &selection(),
                run_count: 1,
                harness_parameters: true,
            })[2],
            "--directory=/performance"
        );
        Ok(())
    }

    #[test]
    fn every_m5_phase_accepts_the_complete_shared_applied_matrix() -> Result<(), String> {
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let output = volume("output", VOLUME_OPTIONS);
        let with_output = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: Some(&output),
        };
        let without_output = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: None,
        };
        let selected = selection();
        let profile = profile_options()?;
        let bloat = bloat_options(BloatProfile::Release)?;
        for (operation, volumes) in [
            (
                PerformanceOperation::Benchmark {
                    selection: &selected,
                    run_count: 3,
                    harness_parameters: true,
                },
                &with_output,
            ),
            (PerformanceOperation::Profile(&profile), &with_output),
            (PerformanceOperation::Bloat(&bloat), &without_output),
        ] {
            for phase in operation.phases() {
                let command = if phase == PerformancePhase::GovernorProbe {
                    vec![
                        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor".into(),
                        "/sys/devices/system/cpu/cpu1/cpufreq/scaling_governor".into(),
                    ]
                } else {
                    phase.arguments(operation)
                };
                let applied = applied_fixture(phase, volumes, operation, &command)?;
                assert_eq!(
                    verify_applied_with_command(
                        &serde_json::to_vec(&applied).map_err(|error| error.to_string())?,
                        crate::APPROVED_M4_IMAGE,
                        phase,
                        volumes,
                        operation,
                        "fixture",
                        &command,
                    ),
                    Ok(()),
                    "{phase:?}"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn every_m5_phase_rejects_drift_in_its_applied_configuration() -> Result<(), String> {
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let output = volume("output", VOLUME_OPTIONS);
        let with_output = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: Some(&output),
        };
        let without_output = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: None,
        };
        let selected = selection();
        let profile = profile_options()?;
        let bloat = bloat_options(BloatProfile::Release)?;
        for (operation, volumes) in [
            (
                PerformanceOperation::Benchmark {
                    selection: &selected,
                    run_count: 3,
                    harness_parameters: true,
                },
                &with_output,
            ),
            (PerformanceOperation::Profile(&profile), &with_output),
            (PerformanceOperation::Bloat(&bloat), &without_output),
        ] {
            for phase in operation.phases() {
                let command = if phase == PerformancePhase::GovernorProbe {
                    vec![
                        "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor".into(),
                        "/sys/devices/system/cpu/cpu1/cpufreq/scaling_governor".into(),
                    ]
                } else {
                    phase.arguments(operation)
                };
                let applied = applied_fixture(phase, volumes, operation, &command)?;
                let check = |value: &serde_json::Value| {
                    verify_applied_with_command(
                        &serde_json::to_vec(value).unwrap_or_default(),
                        crate::APPROVED_M4_IMAGE,
                        phase,
                        volumes,
                        operation,
                        "fixture",
                        &command,
                    )
                };
                for (pointer, changed) in [
                    (
                        "/0/Config/AttachStdin",
                        serde_json::json!(!phase.interactive()),
                    ),
                    (
                        "/0/Config/StdinOnce",
                        serde_json::json!(!phase.interactive()),
                    ),
                    (
                        "/0/Config/Entrypoint",
                        serde_json::json!(["/usr/bin/false"]),
                    ),
                    ("/0/Config/Env", serde_json::json!(["HOST_SECRET=sentinel"])),
                    (
                        "/0/HostConfig/SecurityOpt",
                        serde_json::json!(["no-new-privileges=true", "seccomp=unconfined"]),
                    ),
                ] {
                    let mut invalid = applied.clone();
                    set_fixture_value(&mut invalid, pointer, changed)?;
                    assert_eq!(
                        check(&invalid),
                        Err(ExecutionError::InvalidConfiguration),
                        "{phase:?} accepted {pointer}"
                    );
                }
                let mut wrong_command = applied.clone();
                wrong_command[0]["Config"]["Cmd"]
                    .as_array_mut()
                    .ok_or_else(|| "Cmd array".to_owned())?
                    .push(serde_json::json!("--untrusted"));
                assert_eq!(
                    check(&wrong_command),
                    Err(ExecutionError::InvalidConfiguration),
                    "{phase:?} accepted argv drift"
                );

                let mut wrong_access = applied.clone();
                if wrong_access[0]["Mounts"]
                    .as_array()
                    .is_some_and(Vec::is_empty)
                {
                    wrong_access[0]["Mounts"] = serde_json::json!([{
                        "Type": "volume", "Name": "source", "Source": source.mountpoint,
                        "Destination": "/source", "Driver": "local", "Mode": "z",
                        "RW": false, "Propagation": ""
                    }]);
                } else {
                    let writable = wrong_access[0]["Mounts"][0]["RW"]
                        .as_bool()
                        .ok_or_else(|| "RW bool".to_owned())?;
                    wrong_access[0]["Mounts"][0]["RW"] = serde_json::json!(!writable);
                }
                assert_eq!(
                    check(&wrong_access),
                    Err(ExecutionError::InvalidConfiguration),
                    "{phase:?} accepted applied mount drift"
                );
            }
        }
        Ok(())
    }

    #[test]
    fn m5_rejects_every_authority_field_from_the_shared_matrix() -> Result<(), String> {
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let output = volume("output", VOLUME_OPTIONS);
        let volumes = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: Some(&output),
        };
        let profile = profile_options()?;
        let operation = PerformanceOperation::Profile(&profile);
        let phase = PerformancePhase::ProfileRun;
        let command = phase.arguments(operation);
        let applied = applied_fixture(phase, &volumes, operation, &command)?;
        let check = |value: &serde_json::Value| {
            verify_applied_with_command(
                &serde_json::to_vec(value).unwrap_or_default(),
                crate::APPROVED_M4_IMAGE,
                phase,
                &volumes,
                operation,
                "fixture",
                &command,
            )
        };
        assert_eq!(check(&applied), Ok(()));
        for (pointer, changed) in [
            ("/0/Config/AttachStdin", serde_json::json!(true)),
            ("/0/Config/StdinOnce", serde_json::json!(true)),
            ("/0/Config/Volumes", serde_json::json!({"/host": {}})),
            ("/0/HostConfig/AutoRemove", serde_json::json!(true)),
            ("/0/HostConfig/GroupAdd", serde_json::json!(["0"])),
            ("/0/HostConfig/UTSMode", serde_json::json!("host")),
            ("/0/HostConfig/OomKillDisable", serde_json::json!(true)),
            ("/0/HostConfig/OomScoreAdj", serde_json::json!(-1000)),
            (
                "/0/HostConfig/DeviceCgroupRules",
                serde_json::json!(["a *:* rwm"]),
            ),
            (
                "/0/HostConfig/StorageOpt",
                serde_json::json!({"size": "unbounded"}),
            ),
            (
                "/0/HostConfig/Annotations",
                serde_json::json!({"host": "trusted"}),
            ),
            ("/0/HostConfig/Init", serde_json::json!(true)),
            ("/0/HostConfig/MaskedPaths", serde_json::json!([])),
            ("/0/HostConfig/ReadonlyPaths", serde_json::json!([])),
            ("/0/HostConfig/UsernsMode", serde_json::json!("host")),
            ("/0/HostConfig/CgroupParent", serde_json::json!("root")),
            (
                "/0/HostConfig/Sysctls",
                serde_json::json!({"kernel.core_pattern": "/host"}),
            ),
            (
                "/0/HostConfig/Ulimits",
                serde_json::json!([{"Name": "nofile", "Soft": 1, "Hard": 1}]),
            ),
            ("/0/HostConfig/PidMode", serde_json::json!("host")),
            ("/0/HostConfig/VolumesFrom", serde_json::json!(["other:rw"])),
            (
                "/0/HostConfig/LogConfig/Type",
                serde_json::json!("json-file"),
            ),
            (
                "/0/HostConfig/Devices",
                serde_json::json!([{"PathOnHost": "/dev/mem"}]),
            ),
            (
                "/0/HostConfig/DeviceRequests",
                serde_json::json!([{"Driver": "nvidia"}]),
            ),
            ("/0/HostConfig/PublishAllPorts", serde_json::json!(true)),
            (
                "/0/HostConfig/PortBindings",
                serde_json::json!({"22/tcp": [{"HostPort": "22"}]}),
            ),
            (
                "/0/HostConfig/RestartPolicy/Name",
                serde_json::json!("always"),
            ),
        ] {
            let mut invalid = applied.clone();
            set_fixture_value(&mut invalid, pointer, changed)?;
            assert_eq!(
                check(&invalid),
                Err(ExecutionError::InvalidConfiguration),
                "accepted {pointer}"
            );
        }
        let mut missing_command = applied.clone();
        missing_command[0]["Config"]
            .as_object_mut()
            .ok_or_else(|| "Config object".to_owned())?
            .remove("Cmd");
        assert_eq!(
            check(&missing_command),
            Err(ExecutionError::Infrastructure),
            "a missing Cmd is not Docker's explicit null argv"
        );
        Ok(())
    }

    #[test]
    fn m5_mounts_match_both_docker_views_and_the_phase_access_mode() -> Result<(), String> {
        let source = volume("source", VOLUME_OPTIONS);
        let vendor = volume("vendor", VOLUME_OPTIONS);
        let config = volume("config", VOLUME_OPTIONS);
        let target = volume("target", TARGET_VOLUME_OPTIONS);
        let output = volume("output", VOLUME_OPTIONS);
        let volumes = PerformanceVolumes {
            source: &source,
            vendor: &vendor,
            config: &config,
            target: &target,
            output: Some(&output),
        };
        let profile = profile_options()?;
        let operation = PerformanceOperation::Profile(&profile);
        let phase = PerformancePhase::ProfileRun;
        let command = phase.arguments(operation);
        let applied = applied_fixture(phase, &volumes, operation, &command)?;
        let check = |value: &serde_json::Value| {
            verify_applied_with_command(
                &serde_json::to_vec(value).unwrap_or_default(),
                crate::APPROVED_M4_IMAGE,
                phase,
                &volumes,
                operation,
                "fixture",
                &command,
            )
        };
        for (pointer, changed) in [
            ("/0/Mounts/0/Type", serde_json::json!("bind")),
            ("/0/Mounts/0/Name", serde_json::json!("other")),
            ("/0/Mounts/0/Source", serde_json::json!("/host")),
            ("/0/Mounts/0/Destination", serde_json::json!("/other")),
            ("/0/Mounts/0/Driver", serde_json::json!("other")),
            ("/0/Mounts/0/Mode", serde_json::json!("rw")),
            ("/0/Mounts/0/RW", serde_json::json!(true)),
            ("/0/Mounts/0/Propagation", serde_json::json!("rshared")),
            ("/0/HostConfig/Mounts/0/Type", serde_json::json!("bind")),
            ("/0/HostConfig/Mounts/0/Source", serde_json::json!("other")),
            ("/0/HostConfig/Mounts/0/Target", serde_json::json!("/other")),
            ("/0/HostConfig/Mounts/0/ReadOnly", serde_json::json!(false)),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/NoCopy",
                serde_json::json!(false),
            ),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/Labels",
                serde_json::json!({"host": "trusted"}),
            ),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/Subpath",
                serde_json::json!("outside"),
            ),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/DriverConfig/Name",
                serde_json::json!("bind"),
            ),
            (
                "/0/HostConfig/Mounts/0/VolumeOptions/DriverConfig/Options",
                serde_json::json!({"device": "/"}),
            ),
            ("/0/Mounts/0/Unknown", serde_json::json!(true)),
            ("/0/HostConfig/Mounts/0/Unknown", serde_json::json!(true)),
        ] {
            let mut invalid = applied.clone();
            set_fixture_value(&mut invalid, pointer, changed)?;
            assert_eq!(
                check(&invalid),
                Err(ExecutionError::InvalidConfiguration),
                "accepted {pointer}"
            );
        }
        let mut omitted = applied.clone();
        omitted[0]["Mounts"] = serde_json::json!([]);
        assert_eq!(check(&omitted), Err(ExecutionError::InvalidConfiguration));
        let mut extra = applied.clone();
        let duplicate = extra[0]["Mounts"][0].clone();
        extra[0]["Mounts"]
            .as_array_mut()
            .ok_or_else(|| "Mounts array".to_owned())?
            .push(duplicate);
        assert_eq!(check(&extra), Err(ExecutionError::InvalidConfiguration));
        Ok(())
    }

    #[test]
    fn every_operation_lists_exactly_the_phases_it_creates() -> Result<(), String> {
        let selected = selection();
        let profile = profile_options()?;
        let bloat = bloat_options(BloatProfile::Release)?;
        let benchmark = PerformanceOperation::Benchmark {
            selection: &selected,
            run_count: 3,
            harness_parameters: true,
        }
        .phases();
        assert!(benchmark.contains(&PerformancePhase::BenchRun));
        assert!(benchmark.contains(&PerformancePhase::BenchExport));
        assert!(!benchmark.contains(&PerformancePhase::ProfileRun));
        assert!(!benchmark.contains(&PerformancePhase::BloatFunctions));
        let profiling = PerformanceOperation::Profile(&profile).phases();
        assert!(profiling.contains(&PerformancePhase::ProfileRun));
        assert!(!profiling.contains(&PerformancePhase::BenchRun));
        let bloating = PerformanceOperation::Bloat(&bloat).phases();
        assert!(bloating.contains(&PerformancePhase::BloatFileDigest));
        assert!(!bloating.contains(&PerformancePhase::OutputGuardian));
        for phases in [benchmark, profiling, bloating] {
            assert!(phases.contains(&PerformancePhase::SourceGuardian));
            assert!(phases.contains(&PerformancePhase::ConfigIngest));
            assert!(phases.contains(&PerformancePhase::CpuProbe));
            assert!(phases.contains(&PerformancePhase::KernelProbe));
            assert!(phases.contains(&PerformancePhase::GovernorProbe));
        }
        Ok(())
    }

    #[test]
    fn every_cargo_phase_selects_the_vendor_tree_on_the_command_line() -> Result<(), String> {
        let selected = selection();
        let profile = profile_options()?;
        let bloat = bloat_options(BloatProfile::Release)?;
        for (phase, operation) in [
            (
                PerformancePhase::BenchRun,
                PerformanceOperation::Benchmark {
                    selection: &selected,
                    run_count: 3,
                    harness_parameters: true,
                },
            ),
            (
                PerformancePhase::ProfileBuild,
                PerformanceOperation::Profile(&profile),
            ),
            (
                PerformancePhase::BloatFunctions,
                PerformanceOperation::Bloat(&bloat),
            ),
            (
                PerformancePhase::BloatCrates,
                PerformanceOperation::Bloat(&bloat),
            ),
        ] {
            let arguments = phase.arguments(operation);
            assert_eq!(&arguments[1..3], VENDOR_SELECTION, "{phase:?}");
        }
        // A phase that runs no cargo never carries the selection.
        for phase in [
            PerformancePhase::Metadata,
            PerformancePhase::ProfileRun,
            PerformancePhase::BloatFileSize,
            PerformancePhase::BenchExport,
        ] {
            assert!(
                !phase
                    .arguments(PerformanceOperation::Bloat(&bloat))
                    .iter()
                    .any(|value| value == "--config"),
                "{phase:?}"
            );
        }
        Ok(())
    }

    #[test]
    fn a_project_supplied_cargo_configuration_is_refused_before_any_volume() -> Result<(), String> {
        for path in [
            ".cargo/config.toml",
            ".cargo/config",
            "member/.cargo/config.toml",
        ] {
            let file = SourceFile::new(path.into(), b"[source.crates-io]\n".to_vec())
                .map_err(|error| format!("{error:?}"))?;
            let source = SourceBundle::new(vec![file]).map_err(|error| format!("{error:?}"))?;
            assert_eq!(
                reject_project_cargo_configuration(&source),
                Err(PerformanceError::ProjectCargoConfiguration),
                "accepted {path}"
            );
        }
        let file = SourceFile::new("Cargo.toml".into(), b"[package]\n".to_vec())
            .map_err(|error| format!("{error:?}"))?;
        let source = SourceBundle::new(vec![file]).map_err(|error| format!("{error:?}"))?;
        assert_eq!(reject_project_cargo_configuration(&source), Ok(()));
        Ok(())
    }

    #[test]
    fn applied_quotas_are_the_ones_this_gateway_asked_for() {
        assert_eq!(APPLIED_CPU_MILLICORES, 1_000);
        assert_eq!(APPLIED_MEMORY_BYTES, 1_073_741_824);
        assert_eq!(APPLIED_PIDS, 128);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod capture_ingest_tests {
    //! What the vendor ingest does with ADR-078's capture, and what it still
    //! does with everything else.
    use super::*;
    use crate::supervisor::InputSource;
    use rust_engineering_application::vendor_capture::{
        VendorCaptureAccess, VerifiedVendorCapture,
    };
    use rust_engineering_domain::vendor_capture::{
        CaptureHasher, VendorCapture, VendorCaptureBuilder, VendorCaptureError,
    };
    use std::sync::Mutex;

    struct Toy(u64);
    impl CaptureHasher for Toy {
        fn update(&mut self, bytes: &[u8]) {
            for byte in bytes {
                self.0 = (self.0 ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
        fn finish(self) -> Result<SourceFingerprint, VendorCaptureError> {
            format!("sha256:{}", format!("{:016x}", self.0).repeat(4))
                .parse()
                .map_err(|_| VendorCaptureError::Invalid)
        }
    }

    /// A capture that already passed verification, standing in for the one the
    /// project adapter hands over.
    struct Verified {
        capture: VendorCapture,
        artifact: Vec<u8>,
        position: Mutex<usize>,
    }
    impl VerifiedVendorCapture for Verified {
        fn capture(&self) -> &VendorCapture {
            &self.capture
        }
        fn rewind(&self) -> Result<(), VendorCaptureAccess> {
            *self.position.lock().map_err(|_| VendorCaptureAccess::Io)? = 0;
            Ok(())
        }
        fn read(&self, buffer: &mut [u8]) -> Result<usize, VendorCaptureAccess> {
            let mut position = self.position.lock().map_err(|_| VendorCaptureAccess::Io)?;
            let take = buffer.len().min(self.artifact.len() - *position);
            buffer[..take].copy_from_slice(&self.artifact[*position..*position + take]);
            *position += take;
            Ok(take)
        }
    }

    /// One package, one source directory and one file large enough that the
    /// stream is several read buffers rather than one.
    fn verified() -> Verified {
        let body = vec![b'x'; 200 * 1024];
        let mut builder = VendorCaptureBuilder::new(Toy(0), Toy(0));
        let mut artifact = Vec::new();
        for directory in ["criterion-0.8.2", "criterion-0.8.2/src"] {
            artifact.extend_from_slice(builder.directory(directory).unwrap().as_slice());
        }
        artifact.extend_from_slice(
            builder
                .begin_file("criterion-0.8.2/src/lib.rs", body.len() as u64)
                .unwrap()
                .as_slice(),
        );
        builder.chunk(&body).unwrap();
        artifact.extend_from_slice(&body);
        artifact.extend_from_slice(builder.end_file().unwrap().as_slice());
        let (capture, trailer) = builder.finish().unwrap();
        artifact.extend_from_slice(trailer.as_slice());
        assert_eq!(capture.artifact_bytes() as usize, artifact.len());
        Verified {
            capture,
            artifact,
            position: Mutex::new(0),
        }
    }

    #[test]
    fn the_capture_reaches_the_ingest_whole_without_ever_being_resident() {
        let verified = verified();
        let mut source = CaptureSource::new(&verified);
        let mut delivered = Vec::new();
        loop {
            let chunk = source.fill().expect("fill");
            if chunk.is_empty() {
                break;
            }
            assert!(
                chunk.len() <= VENDOR_CAPTURE_READ_BUFFER_BYTES,
                "the source never holds more than one ADR-078 read buffer"
            );
            let take = chunk.len().min(8192);
            delivered.extend_from_slice(&chunk[..take]);
            source.consume(take);
        }
        assert_eq!(delivered, verified.artifact);
        assert_eq!(delivered.len() as u64, verified.capture.artifact_bytes());
        assert_eq!(
            source.buffer.len(),
            VENDOR_CAPTURE_READ_BUFFER_BYTES,
            "and its whole state is that buffer"
        );
    }

    #[test]
    fn rewinding_replays_the_same_artifact() {
        let verified = verified();
        let mut first = Vec::new();
        let mut source = CaptureSource::new(&verified);
        while let Ok(chunk) = source.fill() {
            if chunk.is_empty() {
                break;
            }
            let take = chunk.len();
            first.extend_from_slice(chunk);
            source.consume(take);
        }
        verified.rewind().expect("rewind");
        let mut second = Vec::new();
        let mut source = CaptureSource::new(&verified);
        while let Ok(chunk) = source.fill() {
            if chunk.is_empty() {
                break;
            }
            let take = chunk.len();
            second.extend_from_slice(chunk);
            source.consume(take);
        }
        assert_eq!(first, second);
    }

    #[test]
    fn the_fingerprint_commitment_is_the_bytes_for_a_snapshot_and_the_digest_for_a_capture() {
        // Nothing changes for the flows that were already qualified: an owned
        // archive still commits to its own bytes.
        let bytes = VendorIngest::Bytes(vec![1, 2, 3]);
        assert_eq!(bytes.commitment(), &[1, 2, 3]);
        let verified = verified();
        let capture = VendorIngest::Capture(&verified);
        assert_eq!(
            capture.commitment(),
            verified.capture.artifact_digest().as_str().as_bytes()
        );
        assert_ne!(capture.commitment(), verified.artifact.as_slice());
    }
}
