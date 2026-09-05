//! Mutation values do not confer authority. The host grant and a live lease do.
use crate::{SourceBundle, SourceFingerprint};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationKind {
    ManifestPatch,
    FormatApply,
    FixApply,
    DependencyAdd,
    DependencyRemove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationId(String);

impl MutationId {
    pub fn new(value: String) -> Result<Self, MutationError> {
        let suffix = value.strip_prefix("mut_").ok_or(MutationError::Invalid)?;
        if suffix.len() != 32
            || !suffix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(MutationError::Invalid);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(value: String) -> Result<Self, MutationError> {
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(MutationError::Invalid);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationCandidate {
    pub kind: MutationKind,
    pub before: SourceBundle,
    pub after: SourceBundle,
    /// Exact validation/policy provenance bound into the digest; bounded by adapter.
    pub validation: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationCommit {
    pub id: MutationId,
    pub digest: SourceFingerprint,
    pub key: IdempotencyKey,
    pub candidate: MutationCandidate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationState {
    Committed,
    NoChange,
    Aborted,
    RecoveryRequired,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationFileReceipt {
    pub path: String,
    pub before: SourceFingerprint,
    pub after: SourceFingerprint,
    pub before_bytes: u64,
    pub after_bytes: u64,
    /// Hash of the effect recorded by the terminal journal state.
    pub effect_after: Option<SourceFingerprint>,
    /// Byte length paired with `effect_after`.
    pub effect_after_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationReceipt {
    pub id: MutationId,
    pub digest: SourceFingerprint,
    pub state: MutationState,
    pub files: Vec<MutationFileReceipt>,
    pub validation: String,
}

/// Bounded local-operator view of one durable replay record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MutationRecordSummary {
    pub id: MutationId,
    pub digest: SourceFingerprint,
    pub state: MutationState,
    pub stored_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationError {
    Invalid,
    PermissionDenied,
    Conflict,
    Busy,
    Expired,
    NotFound,
    LimitExceeded,
    UnsupportedPlatform,
    Cancelled,
    Io,
    RecoveryRequired,
}
