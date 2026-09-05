//! Facts from an isolated, host-approved offline Cargo resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutationLockDisposition {
    UpdatedExisting,
    TransientUnpublished,
}

#[derive(Clone, Debug)]
pub struct MutationResolutionObservation {
    /// Same paths/types as the input; an absent host lock is never added here.
    pub candidate: crate::SourceBundle,
    /// Identity and execution fingerprint of the independent frozen validation.
    pub runtime: crate::RuntimeIdentity,
    pub resolution_execution_fingerprint: crate::ExecutionFingerprint,
    pub dataset_fingerprint: crate::SourceFingerprint,
    pub resolved_lock_fingerprint: crate::SourceFingerprint,
    pub candidate_source_fingerprint: crate::SourceFingerprint,
    pub lock_disposition: MutationLockDisposition,
}
