//! Observation contract for one `rust.profile.flamegraph` execution (M5-03).
//!
//! Everything here describes what the sampler observed, including what it
//! missed. Lost samples, unresolved frames and truncated stacks are first-class
//! fields: a profile that saw nothing is a valid, declared result (ADR-074 §5).
use crate::{ExecutionFingerprint, ExecutionTermination, RuntimeIdentity, SourceFingerprint};
use serde::Serialize;

/// The only sampling backend this server implements (ADR-074 §4).
pub const PROFILE_BACKEND: &str = "perf_event_software_cpu_clock";
pub const PROFILE_STACKS_FORMAT: &str = "rust-engineering-mcp.collapsed-stacks.v1";
pub const PROFILE_SVG_FORMAT: &str = "rust-engineering-mcp.flamegraph-svg.v1";

pub const PROFILE_MIN_FREQUENCY_HZ: u32 = 1;
pub const PROFILE_MAX_FREQUENCY_HZ: u32 = 999;
pub const PROFILE_DEFAULT_FREQUENCY_HZ: u32 = 99;
pub const PROFILE_MIN_DURATION_SECONDS: u64 = 1;
pub const PROFILE_MAX_DURATION_SECONDS: u64 = 60;
pub const PROFILE_DEFAULT_DURATION_SECONDS: u64 = 10;
/// The kernel's own `perf_event_max_stack` may cap this lower; the observation
/// records the depth actually applied, never the depth merely requested.
pub const PROFILE_MAX_DEPTH: u32 = 127;
pub const PROFILE_MAX_SAMPLES: u64 = 2_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileError {
    InvalidFrequency,
    InvalidDuration,
    InvalidTarget,
}

/// A validated profiling request. The peer supplies a target name and two
/// bounded numbers; it never supplies a path, an argument or a sampling mode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileOptions {
    binary_target: String,
    frequency_hz: u32,
    duration_seconds: u64,
}
impl ProfileOptions {
    pub fn new(
        binary_target: String,
        frequency_hz: u32,
        duration_seconds: u64,
    ) -> Result<Self, ProfileError> {
        if binary_target.is_empty()
            || binary_target.len() > 64
            || !binary_target
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(ProfileError::InvalidTarget);
        }
        if !(PROFILE_MIN_FREQUENCY_HZ..=PROFILE_MAX_FREQUENCY_HZ).contains(&frequency_hz) {
            return Err(ProfileError::InvalidFrequency);
        }
        if !(PROFILE_MIN_DURATION_SECONDS..=PROFILE_MAX_DURATION_SECONDS)
            .contains(&duration_seconds)
        {
            return Err(ProfileError::InvalidDuration);
        }
        Ok(Self {
            binary_target,
            frequency_hz,
            duration_seconds,
        })
    }
    pub fn binary_target(&self) -> &str {
        &self.binary_target
    }
    pub fn frequency_hz(&self) -> u32 {
        self.frequency_hz
    }
    pub fn duration_seconds(&self) -> u64 {
        self.duration_seconds
    }
    pub fn duration_ms(&self) -> u64 {
        self.duration_seconds.saturating_mul(1000)
    }
}

/// The helper's own terminal state, mapped one-to-one from its manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatus {
    Complete,
    SampleLimit,
    DurationLimit,
    ChildExited,
    ProfilerUnavailable,
}

/// What the build of the profiled target did. A project that fails to compile
/// is an observed project failure, not a profiler failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileBuildOutcome {
    Built,
    CompilationFailed,
    TargetNotFound,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProfileCounters {
    pub observed_duration_ms: u64,
    pub samples_collected: u64,
    pub samples_lost: u64,
    pub stacks_written: u64,
    pub frames_total: u64,
    pub frames_unresolved: u64,
    pub stacks_truncated: u64,
    pub modules_seen: u64,
    pub max_depth_applied: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProfileChild {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}

/// One ranked frame. `self_samples` counts stacks whose leaf is this frame;
/// `total_samples` counts stacks containing it, once per stack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct ProfileFrameWeight {
    pub frame: String,
    pub self_samples: u64,
    pub total_samples: u64,
}

/// How complete the emitted evidence is. Anything but `Complete` must be
/// visible to the caller; it is never smoothed into a clean result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileCompleteness {
    Complete,
    LostSamples,
    NoSamples,
    Truncated,
    Unavailable,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProfileObservation {
    pub options: ProfileOptions,
    pub backend: &'static str,
    pub build: ProfileBuildOutcome,
    pub build_exit_code: Option<i32>,
    pub status: ProfileStatus,
    pub counters: ProfileCounters,
    pub child: ProfileChild,
    /// Errno reported by the helper when `perf_event_open` was refused.
    pub perf_errno: Option<i32>,
    pub completeness: ProfileCompleteness,
    pub top_frames: Vec<ProfileFrameWeight>,
    pub stacks: Vec<u8>,
    pub svg: Vec<u8>,
    pub termination: ExecutionTermination,
    pub runtime: RuntimeIdentity,
    pub execution_fingerprint: ExecutionFingerprint,
    pub vendor_fingerprint: SourceFingerprint,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
}
impl ProfileObservation {
    /// Completeness must agree with the counters it summarizes.
    pub fn consistent(&self) -> bool {
        match self.completeness {
            ProfileCompleteness::Unavailable => {
                self.status == ProfileStatus::ProfilerUnavailable
                    && self.counters.samples_collected == 0
                    && self.svg.is_empty()
            }
            ProfileCompleteness::NoSamples => self.counters.samples_collected == 0,
            ProfileCompleteness::LostSamples => self.counters.samples_lost > 0,
            ProfileCompleteness::Truncated => self.counters.stacks_truncated > 0,
            ProfileCompleteness::Complete => {
                self.counters.samples_lost == 0
                    && self.counters.stacks_truncated == 0
                    && self.counters.samples_collected > 0
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)] // Fixed fixtures are malformed only by mistake; fail immediately.
mod tests {
    use super::*;

    #[test]
    fn options_reject_paths_arguments_and_out_of_range_numbers() {
        assert!(ProfileOptions::new("workload".into(), 99, 10).is_ok());
        for target in [
            "",
            "/usr/bin/workload",
            "../workload",
            "work load",
            "work;load",
            "work$load",
            &"a".repeat(65),
        ] {
            assert_eq!(
                ProfileOptions::new(target.into(), 99, 10).unwrap_err(),
                ProfileError::InvalidTarget,
                "accepted {target:?}"
            );
        }
        for frequency in [0, 1000, u32::MAX] {
            assert_eq!(
                ProfileOptions::new("w".into(), frequency, 10).unwrap_err(),
                ProfileError::InvalidFrequency
            );
        }
        for duration in [0, 61, u64::MAX] {
            assert_eq!(
                ProfileOptions::new("w".into(), 99, duration).unwrap_err(),
                ProfileError::InvalidDuration
            );
        }
        assert!(ProfileOptions::new("w".into(), 1, 1).is_ok());
        assert!(ProfileOptions::new("w".into(), 999, 60).is_ok());
    }

    #[test]
    fn duration_ms_never_overflows() {
        let options = ProfileOptions::new("w".into(), 99, 60).expect("options");
        assert_eq!(options.duration_ms(), 60_000);
    }
}
