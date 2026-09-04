//! Admission is independent of Docker, processes, protocol and filesystem.
use rust_engineering_domain::{
    ExecutionFingerprint, ExecutionResult, ExecutionSpec, SandboxEvidence, SandboxTier,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionError {
    InvalidConfiguration,
    Unavailable,
    Denied,
    Busy,
    Cancelled,
    Infrastructure,
    CleanupUncertain,
}

pub trait ExecutionCancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}
pub struct NeverCancel;
impl ExecutionCancellation for NeverCancel {
    fn is_cancelled(&self) -> bool {
        false
    }
}

pub trait ExecutionPort {
    fn execute(
        &self,
        spec: &ExecutionSpec,
        cancel: &dyn ExecutionCancellation,
    ) -> Result<ExecutionResult, ExecutionError>;
}

/// Used by future tool adapters after selecting a concrete, verified configuration.
/// Calibration itself is an explicit host operation over trusted fixtures only.
pub fn admit_execution(
    tier: SandboxTier,
    executes_project_code: bool,
    allow_project_code: bool,
    evidence: &SandboxEvidence,
    expected_configuration: &ExecutionFingerprint,
) -> Result<(), ExecutionError> {
    if &evidence.configuration_fingerprint != expected_configuration {
        return Err(ExecutionError::Denied);
    }
    if executes_project_code && (!allow_project_code || tier != SandboxTier::Strict) {
        return Err(ExecutionError::Denied);
    }
    if tier == SandboxTier::None || !evidence.capabilities.satisfies(tier) {
        return Err(ExecutionError::Denied);
    }
    Ok(())
}
