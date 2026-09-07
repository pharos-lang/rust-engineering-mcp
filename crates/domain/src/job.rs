//! Transport-neutral lifecycle values for bounded M3 quality jobs.

use serde::{Deserialize, Serialize};
use std::{error::Error, fmt, str::FromStr};

pub const TASK_RECORD_TTL_MS: u64 = 7_200_000;
pub const TASK_POLL_INTERVAL_MS: u64 = 1_000;
pub const NON_DELIVERY_DEADLINE_MS: u64 = 30_000;
pub const TASK_RESPONSE_MAX_BYTES: u64 = 512 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobContractError {
    InvalidId,
    InvalidBudget,
    InvalidDeadline,
}

impl fmt::Display for JobContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidId => "invalid job identifier",
            Self::InvalidBudget => "invalid job budget",
            Self::InvalidDeadline => "invalid job deadline",
        })
    }
}

impl Error for JobContractError {}

/// Opaque locator only. Syntax proves neither existence nor authorization.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct JobId(String);

impl JobId {
    pub fn from_random_bytes(bytes: [u8; 16]) -> Self {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(36);
        encoded.push_str("job_");
        for byte in bytes {
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Self(encoded)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for JobId {
    type Error = JobContractError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.strip_prefix("job_").is_some_and(|suffix| {
            suffix.len() == 32
                && suffix
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        }) {
            Ok(Self(value))
        } else {
            Err(JobContractError::InvalidId)
        }
    }
}

impl FromStr for JobId {
    type Err = JobContractError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.to_owned().try_into()
    }
}

impl From<JobId> for String {
    fn from(value: JobId) -> Self {
        value.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Closed to operations whose complete application vertical is qualified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    TestNextest,
    Coverage,
    SemverCheck,
    MutationTest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Admitted,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    pub fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (Self::Admitted, Self::Running)
                    | (
                        Self::Admitted | Self::Running,
                        Self::Completed | Self::Failed | Self::Cancelled
                    )
            )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPhase {
    Admission,
    Capture,
    Prepare,
    Execute,
    Collect,
    Publish,
    Cleanup,
    Terminal,
}

impl JobPhase {
    pub fn status_message(self) -> &'static str {
        match self {
            Self::Admission => "admitted",
            Self::Capture => "capturing project",
            Self::Prepare => "preparing execution",
            Self::Execute => "executing",
            Self::Collect => "collecting evidence",
            Self::Publish => "publishing result",
            Self::Cleanup => "cleaning up",
            Self::Terminal => "finished",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    #[default]
    Auto,
    Task,
    Synchronous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Milliseconds(pub u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobDeadline(Milliseconds);

impl JobDeadline {
    pub fn at(monotonic: Milliseconds) -> Self {
        Self(monotonic)
    }

    pub fn after(now: Milliseconds, duration: Milliseconds) -> Result<Self, JobContractError> {
        now.0
            .checked_add(duration.0)
            .map(|value| Self(Milliseconds(value)))
            .ok_or(JobContractError::InvalidDeadline)
    }

    pub fn monotonic(self) -> Milliseconds {
        self.0
    }

    pub fn reached(self, now: Milliseconds) -> bool {
        now >= self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobBudget {
    work: Milliseconds,
    capture_prepare: Milliseconds,
    execute: Milliseconds,
    collect_publish: Milliseconds,
    cleanup: Milliseconds,
}

impl JobBudget {
    pub fn new(
        work: Milliseconds,
        capture_prepare: Milliseconds,
        execute: Milliseconds,
        collect_publish: Milliseconds,
        cleanup: Milliseconds,
    ) -> Result<Self, JobContractError> {
        let children = capture_prepare
            .0
            .checked_add(execute.0)
            .and_then(|value| value.checked_add(collect_publish.0))
            .ok_or(JobContractError::InvalidBudget)?;
        if work.0 == 0
            || work.0 > 3_600_000
            || capture_prepare.0 == 0
            || capture_prepare.0 > 120_000
            || execute.0 == 0
            || execute.0 > 3_360_000
            || collect_publish.0 == 0
            || collect_publish.0 > 120_000
            || cleanup.0 == 0
            || cleanup.0 > 240_000
            || children > work.0
        {
            return Err(JobContractError::InvalidBudget);
        }
        Ok(Self {
            work,
            capture_prepare,
            execute,
            collect_publish,
            cleanup,
        })
    }

    pub fn asynchronous_default() -> Result<Self, JobContractError> {
        Self::new(
            Milliseconds(300_000),
            Milliseconds(60_000),
            Milliseconds(180_000),
            Milliseconds(30_000),
            Milliseconds(60_000),
        )
    }

    /// Derive the closed ADR-060 phase ceilings from a validated requested
    /// work budget. The default retains its measured 60/180/30 split; larger
    /// jobs allocate only the overhead needed to keep execute at or below its
    /// 3,360-second maximum, with both surrounding phases capped at 120 s.
    pub fn asynchronous_for_work(work: Milliseconds) -> Result<Self, JobContractError> {
        if work.0 <= 300_000 {
            return Self::asynchronous_default();
        }
        let overhead = work.0.saturating_sub(3_360_000).max(90_000);
        let capture_prepare = overhead.saturating_sub(120_000).clamp(60_000, 120_000);
        let collect_publish = overhead
            .saturating_sub(capture_prepare)
            .clamp(30_000, 120_000);
        let execute = work
            .0
            .saturating_sub(capture_prepare)
            .saturating_sub(collect_publish);
        Self::new(
            work,
            Milliseconds(capture_prepare),
            Milliseconds(execute),
            Milliseconds(collect_publish),
            Milliseconds(60_000),
        )
    }

    pub fn work(self) -> Milliseconds {
        self.work
    }

    pub fn capture_prepare(self) -> Milliseconds {
        self.capture_prepare
    }

    pub fn execute(self) -> Milliseconds {
        self.execute
    }

    pub fn collect_publish(self) -> Milliseconds {
        self.collect_publish
    }

    pub fn cleanup(self) -> Milliseconds {
        self.cleanup
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct JobOwnerBinding([u8; 32]);

impl JobOwnerBinding {
    pub fn new(digest: [u8; 32]) -> Self {
        Self(digest)
    }

    pub fn digest(&self) -> &[u8; 32] {
        &self.0
    }

    /// Derive the non-secret, domain-separated authorization binding described
    /// by ADR-060. Every length is encoded so adjacent fields cannot collide.
    pub fn derive(
        state_root_identity: (i64, u64),
        uid: u32,
        granted_root_identity: (i64, u64),
        workspace_root: &str,
    ) -> Self {
        let mut material = Vec::with_capacity(85 + workspace_root.len());
        material.extend_from_slice(b"rust-mcp/job-owner-binding/v1\0");
        material.extend_from_slice(&state_root_identity.0.to_le_bytes());
        material.extend_from_slice(&state_root_identity.1.to_le_bytes());
        material.extend_from_slice(&uid.to_le_bytes());
        material.extend_from_slice(&granted_root_identity.0.to_le_bytes());
        material.extend_from_slice(&granted_root_identity.1.to_le_bytes());
        material.extend_from_slice(&(workspace_root.len() as u64).to_le_bytes());
        material.extend_from_slice(workspace_root.as_bytes());
        Self(sha256(&material))
    }
}

// Kept private to this one security identity so the domain crate preserves its
// serde-only dependency boundary. This is the FIPS 180-4 SHA-256 compression
// function, with all arithmetic explicitly wrapping as required by the spec.
#[allow(clippy::needless_range_loop)]
fn sha256(input: &[u8]) -> [u8; 32] {
    const INITIAL: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    const ROUND: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let bit_len = (input.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity((input.len() + 72) & !63);
    padded.extend_from_slice(input);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());

    let mut state = INITIAL;
    for chunk in padded.as_chunks::<64>().0 {
        let mut words = [0_u32; 64];
        for index in 0..16 {
            let offset = index * 4;
            words[index] = u32::from_be_bytes([
                chunk[offset],
                chunk[offset + 1],
                chunk[offset + 2],
                chunk[offset + 3],
            ]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let sum1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(sum1)
                .wrapping_add(choice)
                .wrapping_add(ROUND[index])
                .wrapping_add(words[index]);
            let sum0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = sum0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }
    let mut output = [0_u8; 32];
    for (chunk, word) in output.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    output
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultRetention {
    ttl: Milliseconds,
}

impl ResultRetention {
    pub fn fixed() -> Self {
        Self {
            ttl: Milliseconds(TASK_RECORD_TTL_MS),
        }
    }

    pub fn ttl(self) -> Milliseconds {
        self.ttl
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetentionQuotas {
    per_owner_entries: usize,
    server_entries: usize,
    per_owner_bytes: u64,
    server_bytes: u64,
}

impl RetentionQuotas {
    pub fn fixed() -> Self {
        Self {
            per_owner_entries: 64,
            server_entries: 256,
            per_owner_bytes: 32 * 1024 * 1024,
            server_bytes: 128 * 1024 * 1024,
        }
    }

    pub fn per_owner_entries(self) -> usize {
        self.per_owner_entries
    }

    pub fn server_entries(self) -> usize {
        self.server_entries
    }

    pub fn per_owner_bytes(self) -> u64 {
        self.per_owner_bytes
    }

    pub fn server_bytes(self) -> u64 {
        self.server_bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobInfrastructureFailure {
    Internal,
    TimedOut,
    CleanupFailed,
    ResultUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobCompletion<T> {
    /// Ordinary tool outcomes, including `is_error == true`, are MCP-completed.
    ToolResult {
        result: T,
        is_error: bool,
    },
    InfrastructureFailure(JobInfrastructureFailure),
}
