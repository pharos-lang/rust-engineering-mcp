//! Trusted configuration bridge; MCP accepts no snapshot paths.
use rust_engineering_application::{DependencyAuditPort, InspectionControl, ProjectError};
use rust_engineering_catalog::RustSecSnapshot;
use rust_engineering_domain::{
    AuditDataError, AuditObservation, CatalogFingerprint, Clock, OperationalErrorCode,
    ProjectStructure, SourceBundle,
};
use std::path::PathBuf;

#[derive(Clone)]
pub struct HostAuditConfig {
    pub path: PathBuf,
    pub fingerprint: CatalogFingerprint,
}
pub(crate) struct AuditProvider(pub Option<HostAuditConfig>);
impl DependencyAuditPort for AuditProvider {
    fn audit(
        &self,
        source: &SourceBundle,
        structure: &ProjectStructure,
        clock: &dyn Clock,
        control: &dyn InspectionControl,
    ) -> Result<AuditObservation, AuditDataError> {
        let Some(config) = &self.0 else {
            return Ok(AuditObservation::unavailable());
        };
        let bytes = match rust_engineering_project::read_host_snapshot(&config.path, control)
            .map_err(snapshot_read_error)
        {
            Ok(bytes) => bytes,
            // Missing data still permits publication of the independently captured
            // project evidence; an unavailable snapshot has no advisory evidence.
            Err(AuditDataError::Unavailable) => return Ok(AuditObservation::unavailable()),
            Err(error) => return Err(error),
        };
        RustSecSnapshot::from_bytes(&bytes, &config.fingerprint, control)?
            .audit(source, structure, clock, control)
    }
}

fn snapshot_read_error(error: ProjectError) -> AuditDataError {
    match error {
        ProjectError::Cancelled => AuditDataError::Cancelled,
        ProjectError::Internal => AuditDataError::Internal,
        ProjectError::Rejected(code) => match code {
            OperationalErrorCode::ProjectNotFound | OperationalErrorCode::ToolNotInstalled => {
                AuditDataError::Unavailable
            }
            OperationalErrorCode::SandboxDenied | OperationalErrorCode::NetworkDenied => {
                AuditDataError::SandboxDenied
            }
            OperationalErrorCode::CommandTimeout => AuditDataError::Timeout,
            OperationalErrorCode::OutputLimitExceeded => AuditDataError::Budget,
            OperationalErrorCode::UnsupportedPlatform => AuditDataError::UnsupportedPlatform,
            OperationalErrorCode::InvalidProject | OperationalErrorCode::LockfileUpdateRequired => {
                AuditDataError::InvalidSnapshot
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_snapshot_errors_preserve_missing_policy_deadline_and_budget() {
        for (code, expected) in [
            (
                OperationalErrorCode::ProjectNotFound,
                AuditDataError::Unavailable,
            ),
            (
                OperationalErrorCode::SandboxDenied,
                AuditDataError::SandboxDenied,
            ),
            (
                OperationalErrorCode::NetworkDenied,
                AuditDataError::SandboxDenied,
            ),
            (
                OperationalErrorCode::CommandTimeout,
                AuditDataError::Timeout,
            ),
            (
                OperationalErrorCode::OutputLimitExceeded,
                AuditDataError::Budget,
            ),
            (
                OperationalErrorCode::UnsupportedPlatform,
                AuditDataError::UnsupportedPlatform,
            ),
            (
                OperationalErrorCode::InvalidProject,
                AuditDataError::InvalidSnapshot,
            ),
        ] {
            assert_eq!(snapshot_read_error(ProjectError::Rejected(code)), expected);
        }
        assert_eq!(
            snapshot_read_error(ProjectError::Cancelled),
            AuditDataError::Cancelled
        );
        assert_eq!(
            snapshot_read_error(ProjectError::Internal),
            AuditDataError::Internal
        );
    }
}
