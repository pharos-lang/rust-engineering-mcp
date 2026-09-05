//! Closed guest mutation capabilities, distinct from M1 read-only Rust commands.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RustMutationCommand {
    Format,
}

#[derive(Clone, Debug)]
pub struct RustMutationExecution {
    pub result: crate::ExecutionResult,
    /// Present only after successful complete export, scope checks and cleanup.
    pub candidate: Option<crate::SourceBundle>,
}

/// A complete candidate that passed an independent read-only postcondition check.
#[derive(Clone, Debug)]
pub struct RustMutationObservation {
    pub candidate: crate::SourceBundle,
    pub runtime: crate::RuntimeIdentity,
    pub mutation_execution_fingerprint: crate::ExecutionFingerprint,
    pub candidate_source_fingerprint: crate::SourceFingerprint,
}
