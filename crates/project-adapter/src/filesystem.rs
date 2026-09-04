#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub use macos::{ProjectLease, SecureProjects, read_host_snapshot};

/// Maximum owned bytes accepted from an explicitly configured host snapshot.
pub const MAX_HOST_SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;

#[cfg(not(target_os = "macos"))]
mod unsupported {
    use rust_engineering_application::{
        OperationControl, ProjectBackend, ProjectError, ProjectIdentity, ProjectSourceBackend,
        ValidatedProject,
    };
    use rust_engineering_domain::{OperationalErrorCode, SourceBundle};
    use std::path::{Path, PathBuf};

    /// Unsupported platforms reject before examining the configured path.
    pub fn read_host_snapshot(
        _path: &Path,
        _control: &dyn OperationControl,
    ) -> Result<Vec<u8>, ProjectError> {
        Err(ProjectError::Rejected(
            OperationalErrorCode::UnsupportedPlatform,
        ))
    }

    pub struct SecureProjects;
    pub struct ProjectLease;

    impl SecureProjects {
        pub fn new(_roots: &[PathBuf]) -> Result<Self, ProjectError> {
            Ok(Self)
        }
    }

    impl ProjectSourceBackend for SecureProjects {
        fn source(
            &self,
            _lease: &ProjectLease,
            _control: &dyn OperationControl,
        ) -> Result<SourceBundle, ProjectError> {
            Err(ProjectError::Rejected(
                OperationalErrorCode::UnsupportedPlatform,
            ))
        }
    }

    impl ProjectBackend for SecureProjects {
        type Lease = ProjectLease;
        fn open(
            &self,
            _path: &str,
            _control: &dyn OperationControl,
        ) -> Result<ValidatedProject<ProjectLease>, ProjectError> {
            Err(ProjectError::Rejected(
                OperationalErrorCode::UnsupportedPlatform,
            ))
        }
        fn revalidate(
            &self,
            _lease: &ProjectLease,
            _control: &dyn OperationControl,
        ) -> Result<ProjectIdentity, ProjectError> {
            Err(ProjectError::Rejected(
                OperationalErrorCode::UnsupportedPlatform,
            ))
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub use unsupported::{ProjectLease, SecureProjects, read_host_snapshot};
