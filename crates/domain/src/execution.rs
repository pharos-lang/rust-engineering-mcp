//! Closed execution vocabulary. No command strings, host paths or runtime APIs.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxTier {
    None,
    Restricted,
    Strict,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SandboxCapabilities {
    pub filesystem_isolated: bool,
    pub network_isolated: bool,
    pub environment_isolated: bool,
    pub children_contained: bool,
    pub wall_time_limited: bool,
    pub output_limited: bool,
    pub cpu_quota: bool,
    pub memory_limited: bool,
    pub pids_limited: bool,
    pub disk_limited: bool,
}
impl SandboxCapabilities {
    pub fn satisfies(self, tier: SandboxTier) -> bool {
        if tier == SandboxTier::None {
            return false;
        }
        let base = self.filesystem_isolated
            && self.network_isolated
            && self.environment_isolated
            && self.children_contained
            && self.wall_time_limited
            && self.output_limited
            && self.pids_limited;
        base && (tier == SandboxTier::Restricted
            || (self.cpu_quota && self.memory_limited && self.disk_limited))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProbeScenario {
    Success,
    Exit7,
    Output,
    Sleep,
    Environment,
    Network,
    Filesystem,
    Descendants,
    Pids,
    Memory,
    Disk,
    Cpu,
    Cgroups,
}
impl ProbeScenario {
    pub const ALL: [Self; 13] = [
        Self::Success,
        Self::Exit7,
        Self::Output,
        Self::Sleep,
        Self::Environment,
        Self::Network,
        Self::Filesystem,
        Self::Descendants,
        Self::Pids,
        Self::Memory,
        Self::Disk,
        Self::Cpu,
        Self::Cgroups,
    ];
    pub fn argument(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Exit7 => "exit7",
            Self::Output => "output",
            Self::Sleep => "sleep",
            Self::Environment => "environment",
            Self::Network => "network",
            Self::Filesystem => "filesystem",
            Self::Descendants => "descendants",
            Self::Pids => "pids",
            Self::Memory => "memory",
            Self::Disk => "disk",
            Self::Cpu => "cpu",
            Self::Cgroups => "cgroups",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ExecutionLimits {
    wall_ms: u64,
    output_bytes: usize,
}
impl ExecutionLimits {
    /// Construct limits for the synchronous M1 command family.  Those public
    /// tools retain their calibrated 60-second ceiling.
    pub fn new(wall_ms: u64, output_bytes: usize) -> Option<Self> {
        (100..=60_000).contains(&wall_ms).then_some(())?;
        Self::new_bounded(wall_ms, output_bytes)
    }

    /// Construct limits for an ADR-060 bounded quality job.  The caller must
    /// still apply the smaller synchronous (120 s) or tool-specific budget;
    /// this constructor only raises the gateway's representable ceiling.
    pub fn new_job(wall_ms: u64, output_bytes: usize) -> Option<Self> {
        (100..=3_600_000).contains(&wall_ms).then_some(())?;
        Self::new_bounded(wall_ms, output_bytes)
    }

    fn new_bounded(wall_ms: u64, output_bytes: usize) -> Option<Self> {
        (1024..=1024 * 1024).contains(&output_bytes).then_some(())?;
        Some(Self {
            wall_ms,
            output_bytes,
        })
    }
    pub fn wall_ms(self) -> u64 {
        self.wall_ms
    }
    /// Maximum retained bytes per stream (stdout and stderr independently).
    pub fn output_bytes(self) -> usize {
        self.output_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutionLimits;

    #[test]
    fn m1_and_job_wall_time_ceilings_are_distinct_and_closed() {
        assert!(ExecutionLimits::new(60_000, 1024).is_some());
        assert!(ExecutionLimits::new(60_001, 1024).is_none());
        assert!(ExecutionLimits::new_job(3_600_000, 1024).is_some());
        assert!(ExecutionLimits::new_job(3_600_001, 1024).is_none());
        assert!(ExecutionLimits::new_job(3_600_000, 1023).is_none());
    }
}
impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            wall_ms: 10_000,
            output_bytes: 64 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionSpec {
    pub scenario: ProbeScenario,
    pub limits: ExecutionLimits,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionTermination {
    Exited,
    TimedOut,
    Cancelled,
    OutputLimit,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExecutionResult {
    pub termination: ExecutionTermination,
    pub exit_code: Option<i32>,
    pub oom_killed: Option<bool>,
    pub stdout: String,
    pub stderr: String,
    pub stdout_truncated: bool,
    pub stderr_truncated: bool,
    pub duration_ms: u64,
    pub total_duration_ms: u64,
    pub execution_fingerprint: crate::ExecutionFingerprint,
    pub platform: &'static str,
    pub image_id: String,
}

/// Evidence is scoped to one generated configuration; it is not peer authority.
#[derive(Clone, Debug, Serialize)]
pub struct SandboxEvidence {
    pub configuration_fingerprint: crate::ExecutionFingerprint,
    pub capabilities: SandboxCapabilities,
}
