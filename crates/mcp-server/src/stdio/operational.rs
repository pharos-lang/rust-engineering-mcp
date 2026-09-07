//! The operational half of a tool result, which is the same for every tool.
//!
//! Each tool keeps its own `Output`, `Outcome` and `Code`: what a tool publishes
//! is its contract, and two tools do not have the same one. What they do have in
//! common is the reading: which operational codes describe an absent runtime
//! rather than a refused request, and which inspection failures are a result the
//! peer can act on rather than a protocol error. That reading lives here, so it
//! is decided once instead of once per vertical.
use super::contract::{Contract, ToolOutput};
use rmcp::model::{CallToolResult, ErrorData};
use rust_engineering_application::{ExecutionError, InspectionError, ProjectError};
use rust_engineering_domain::OperationalErrorCode;
use schemars::JsonSchema;
use serde::de::DeserializeOwned;

/// An absent approved runtime and an unqualified host are `unavailable`: the
/// request was never assessed. Everything else is `blocked`: this host refused
/// a request it understood.
pub(super) fn unavailable(code: OperationalErrorCode) -> bool {
    matches!(
        code,
        OperationalErrorCode::ToolNotInstalled | OperationalErrorCode::UnsupportedPlatform
    )
}

/// A tool result that can carry the shared operational outcomes.
pub(super) trait OperationalOutput: ToolOutput {
    /// Blocked or unavailable per [`unavailable`], with the tool's own message.
    fn operational(code: OperationalErrorCode, message: &'static str, duration_ms: u64) -> Self;
    /// Cancellation observed after joined cleanup, with no partial assessment.
    fn cancelled(duration_ms: u64) -> Self;
    /// The protocol error for a failure that is never a tool result.
    fn failed() -> ErrorData;
}

/// Map one inspection failure onto the tool's declared operational outcome.
///
/// `message` is the tool's own wording for an operational code. Uncertain
/// gateway cleanup and infrastructure failures deliberately do not become tool
/// results: a peer must not read them as an assessment of its project.
pub(super) fn encode_inspection_error<I, O>(
    contract: &Contract<I, O>,
    error: InspectionError,
    message: fn(OperationalErrorCode) -> &'static str,
    duration_ms: u64,
) -> Result<CallToolResult, ErrorData>
where
    I: DeserializeOwned + JsonSchema,
    O: OperationalOutput,
{
    let operational = |code| contract.encode(O::operational(code, message(code), duration_ms));
    match error {
        InspectionError::Project(ProjectError::Rejected(code)) => operational(code),
        InspectionError::Project(ProjectError::Cancelled)
        | InspectionError::Execution(ExecutionError::Cancelled) => {
            contract.encode(O::cancelled(duration_ms))
        }
        InspectionError::Execution(ExecutionError::Unavailable) => {
            operational(OperationalErrorCode::ToolNotInstalled)
        }
        InspectionError::Execution(
            ExecutionError::Denied | ExecutionError::Busy | ExecutionError::InvalidConfiguration,
        ) => operational(OperationalErrorCode::SandboxDenied),
        InspectionError::OutputLimit => operational(OperationalErrorCode::OutputLimitExceeded),
        InspectionError::InvalidMetadata => operational(OperationalErrorCode::InvalidProject),
        InspectionError::Execution(ExecutionError::CleanupUncertain) => {
            Err(ErrorData::internal_error(
                "Gateway cleanup could not be verified; further execution is quarantined",
                None,
            ))
        }
        InspectionError::Internal
        | InspectionError::Project(ProjectError::Internal)
        | InspectionError::Execution(ExecutionError::Infrastructure) => Err(O::failed()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_absent_runtime_or_an_unqualified_host_is_unavailable() {
        for code in [
            OperationalErrorCode::ToolNotInstalled,
            OperationalErrorCode::UnsupportedPlatform,
        ] {
            assert!(unavailable(code), "{code:?}");
        }
        for code in [
            OperationalErrorCode::ProjectNotFound,
            OperationalErrorCode::InvalidProject,
            OperationalErrorCode::LockfileUpdateRequired,
            OperationalErrorCode::CommandTimeout,
            OperationalErrorCode::SandboxDenied,
            OperationalErrorCode::NetworkDenied,
            OperationalErrorCode::OutputLimitExceeded,
        ] {
            assert!(!unavailable(code), "{code:?}");
        }
    }
}
