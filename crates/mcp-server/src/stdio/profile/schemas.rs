//! Wire types for `rust.profile.flamegraph`.
//!
//! Every enum this tool publishes is declared here and nowhere else, so a
//! variant added elsewhere cannot widen this frozen schema (ADR-076 §2).
//!
//! The sampled evidence itself never reaches these types: the sanitized SVG and
//! the collapsed stacks are artifacts (ADR-074 §5). What travels here is the
//! sampler's identity, its parameters, and everything it saw *and missed*.
use schemars::JsonSchema;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactCompleteness {
    Complete,
    Truncated,
    Partial,
    Invalid,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTermination {
    Exited,
    TimedOut,
    Cancelled,
    OutputLimit,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeIdentity {
    #[schemars(length(max = 128))]
    pub platform: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub image_id: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub configuration_fingerprint: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub execution_fingerprint: String,
    #[schemars(length(max = 128))]
    pub rust_version: String,
    #[schemars(length(max = 128))]
    pub cargo_version: String,
    #[schemars(length(max = 128))]
    pub declared_toolchain: Option<String>,
}

/// What the build of the profiled target did. A project that does not compile
/// is an observed project failure, never a profiler failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BuildOutcome {
    Built,
    CompilationFailed,
    TargetNotFound,
}

/// The sampler's own terminal state, mapped one-to-one from its manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfilerStatus {
    Complete,
    SampleLimit,
    DurationLimit,
    ChildExited,
    ProfilerUnavailable,
}

/// How complete the emitted evidence is. Anything but `complete` is visible to
/// the caller and is never smoothed into a clean result. `no_samples` is a
/// valid, declared observation, not a failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    LostSamples,
    NoSamples,
    Truncated,
    Unavailable,
}

/// One ranked frame. `self_samples` counts stacks whose leaf is this frame;
/// `total_samples` counts stacks containing it, once per stack. A frame the
/// sampler could not resolve is reported as `[unknown]` and counted.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FrameWeight {
    /// Sanitized to a closed alphabet and bounded by the stack parser; never
    /// a filesystem path or a module name.
    #[schemars(length(min = 1, max = 200))]
    pub frame: String,
    pub self_samples: u64,
    pub total_samples: u64,
}

/// What the sampler was asked for and what it observed. Requested and observed
/// duration are separate fields: a shortfall is reported, never smoothed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    #[schemars(length(min = 1, max = 128))]
    pub backend: String,
    #[schemars(length(min = 1, max = 64), regex(pattern = "^[A-Za-z0-9_-]{1,64}$"))]
    pub binary_target: String,
    pub frequency_hz: u32,
    pub requested_duration_seconds: u64,
    pub observed_duration_ms: u64,
    pub build: BuildOutcome,
    pub build_exit_code: Option<i32>,
    pub status: ProfilerStatus,
    pub samples_collected: u64,
    pub samples_lost: u64,
    pub stacks_written: u64,
    pub frames_total: u64,
    pub frames_unresolved: u64,
    pub stacks_truncated: u64,
    pub modules_seen: u64,
    /// The stack depth the kernel actually applied, never the depth requested.
    pub max_depth_applied: u32,
    pub child_exit_code: Option<i32>,
    pub child_signal: Option<i32>,
    /// The errno the sampler reported when `perf_event_open` was refused.
    pub perf_errno: Option<i32>,
    pub completeness: Completeness,
    #[schemars(length(max = 1024))]
    pub top_frames: Vec<FrameWeight>,
    pub top_frames_omitted: u32,
    pub termination: ExecutionTermination,
    pub runtime: RuntimeIdentity,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub execution_fingerprint: String,
    #[schemars(regex(pattern = "^sha256:[0-9a-f]{64}$"))]
    pub vendor_fingerprint: String,
    /// Sampler logs are evidence, not a measurement; only the fact that they
    /// were cut travels here.
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    /// False whenever this response describes less than the run produced.
    pub complete: bool,
}
