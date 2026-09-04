//! Filesystem and manifest adapters. No Cargo or other child process is spawned.

pub mod catalog_store;
mod filesystem;
mod manifest;

use std::path::{Path, PathBuf};
use std::time::Instant;

use rust_engineering_application::{ProjectError, ReferenceGenerator, RegistryClock};
use rust_engineering_domain::ProjectRef;

pub use filesystem::{MAX_HOST_SNAPSHOT_BYTES, ProjectLease, SecureProjects, read_host_snapshot};

/// All manifest access is mediated by the capability adapter.
pub trait ManifestIo {
    fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>, ProjectError>;
    fn is_file(&self, path: &Path) -> Result<bool, ProjectError>;
}

/// Sorted exact bytes observed by the structural validator, not Cargo metadata.
pub struct ManifestGraph {
    pub manifests: Vec<(PathBuf, Vec<u8>)>,
}

pub struct OsReferences;

impl ReferenceGenerator for OsReferences {
    fn generate(&self) -> Result<ProjectRef, ProjectError> {
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| ProjectError::Internal)?;
        let mut reference = String::from("prj_");
        for byte in entropy {
            use std::fmt::Write;
            write!(&mut reference, "{byte:02x}").map_err(|_| ProjectError::Internal)?;
        }
        reference.parse().map_err(|_| ProjectError::Internal)
    }
}

pub struct MonotonicClock(Instant);

impl Default for MonotonicClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}

impl RegistryClock for MonotonicClock {
    fn seconds(&self) -> u64 {
        self.0.elapsed().as_secs()
    }
}
