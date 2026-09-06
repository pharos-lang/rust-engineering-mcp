//! Project registration policy, independent of filesystem, protocol and runtime.

use std::collections::HashMap;

use rust_engineering_domain::{OperationalErrorCode, ProjectIdentityFingerprint, ProjectRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectError {
    Rejected(OperationalErrorCode),
    Cancelled,
    Internal,
}

pub mod coverage;

/// A cooperative checkpoint for bounded read/parse operations.
pub trait OperationControl: Send + Sync {
    fn check(&self) -> Result<(), ProjectError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectIdentity {
    pub workspace_root: String,
    pub fingerprint: ProjectIdentityFingerprint,
}

pub struct ValidatedProject<L> {
    pub identity: ProjectIdentity,
    pub lease: L,
}

/// The adapter owns directory capabilities; the application cannot open paths.
pub trait ProjectBackend {
    type Lease;

    fn open(
        &self,
        path: &str,
        control: &dyn OperationControl,
    ) -> Result<ValidatedProject<Self::Lease>, ProjectError>;

    fn revalidate(
        &self,
        lease: &Self::Lease,
        control: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError>;
}

/// Optional live-owner facts required by ADR-061. Implementations derive these
/// from already-open directory capabilities, never from caller path text.
pub trait QualityProjectBackend: ProjectBackend {
    fn revalidate_quality_owner(
        &self,
        lease: &Self::Lease,
        control: &dyn OperationControl,
    ) -> Result<QualityOwnerFacts, ProjectError>;
}

pub trait ReferenceGenerator {
    fn generate(&self) -> Result<ProjectRef, ProjectError>;
}

/// Elapsed seconds from a process-local monotonic origin, never wall-clock UTC.
pub trait RegistryClock {
    fn seconds(&self) -> u64;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedProject {
    pub project_ref: ProjectRef,
    pub identity: ProjectIdentity,
}

struct Entry<L> {
    project: ValidatedProject<L>,
    last_used: u64,
}

pub struct ProjectRegistry<B: ProjectBackend, G, C> {
    backend: B,
    generator: G,
    clock: C,
    ttl_seconds: u64,
    capacity: usize,
    entries: HashMap<ProjectRef, Entry<B::Lease>>,
}

impl<B: ProjectBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn new(
        backend: B,
        generator: G,
        clock: C,
        ttl_seconds: u64,
        capacity: usize,
    ) -> Result<Self, ProjectError> {
        if ttl_seconds == 0 || ttl_seconds > 86_400 || capacity == 0 || capacity > 64 {
            return Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied));
        }
        Ok(Self {
            backend,
            generator,
            clock,
            ttl_seconds,
            capacity,
            entries: HashMap::new(),
        })
    }

    fn prune(&mut self) {
        let now = self.clock.seconds();
        self.entries.retain(|_, entry| {
            now.checked_sub(entry.last_used)
                .is_some_and(|age| age < self.ttl_seconds)
        });
    }

    pub fn open(
        &mut self,
        path: &str,
        control: &dyn OperationControl,
    ) -> Result<OpenedProject, ProjectError> {
        control.check()?;
        self.prune();
        if self.entries.len() >= self.capacity {
            return Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied));
        }
        if path.is_empty() || path.len() > 4096 || path.contains('\0') {
            return Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject));
        }
        let project = self.backend.open(path, control)?;
        // Never overwrite an existing capability even if the entropy provider fails.
        for _ in 0..4 {
            let reference = self.generator.generate()?;
            if !self.entries.contains_key(&reference) {
                control.check()?;
                let result = OpenedProject {
                    project_ref: reference.clone(),
                    identity: project.identity.clone(),
                };
                self.entries.insert(
                    reference,
                    Entry {
                        project,
                        last_used: self.clock.seconds(),
                    },
                );
                return Ok(result);
            }
        }
        Err(ProjectError::Internal)
    }

    /// Internal consumer API; no extra public MCP tool is introduced.
    pub fn resolve(
        &mut self,
        reference: &ProjectRef,
        control: &dyn OperationControl,
    ) -> Result<ProjectIdentity, ProjectError> {
        self.resolve_inner(reference, control, true)
    }

    fn resolve_inner(
        &mut self,
        reference: &ProjectRef,
        control: &dyn OperationControl,
        touch: bool,
    ) -> Result<ProjectIdentity, ProjectError> {
        control.check()?;
        self.prune();
        let Some(entry) = self.entries.get(reference) else {
            return Err(ProjectError::Rejected(
                OperationalErrorCode::ProjectNotFound,
            ));
        };
        let observed = self.backend.revalidate(&entry.project.lease, control);
        match observed {
            Ok(identity) if identity == entry.project.identity => {
                control.check()?;
                if touch && let Some(entry) = self.entries.get_mut(reference) {
                    entry.last_used = self.clock.seconds();
                }
                Ok(identity)
            }
            Ok(_) => {
                self.entries.remove(reference);
                Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject))
            }
            Err(error) => {
                if !matches!(error, ProjectError::Cancelled) {
                    self.entries.remove(reference);
                }
                Err(error)
            }
        }
    }
}

impl<B: QualityProjectBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    /// Revalidate without renewing the project lease. Artifact reads and task
    /// polling therefore cannot keep either project or artifact authority alive.
    pub fn quality_owner_facts(
        &mut self,
        reference: &ProjectRef,
        control: &dyn OperationControl,
    ) -> Result<QualityOwnerFacts, ProjectError> {
        let identity = self.resolve_inner(reference, control, false)?;
        let entry = self.entries.get(reference).ok_or(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound,
        ))?;
        let facts = self
            .backend
            .revalidate_quality_owner(&entry.project.lease, control)?;
        if facts.workspace_root != identity.workspace_root {
            return Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject));
        }
        Ok(facts)
    }
}

mod execution;
pub use execution::*;

mod catalog;
pub use catalog::*;
mod crate_search;
pub use crate_search::*;
mod crate_inspect;
pub use crate_inspect::*;
mod catalog_context;
pub use catalog_context::*;

mod semantic;
pub use semantic::*;

mod artifact;
pub use artifact::*;

mod source;
pub use source::*;

mod inspection;
pub use inspection::*;

mod toolchain;
pub use toolchain::*;

mod artifact_access;
pub use artifact_access::*;

mod quality_artifact;
pub use quality_artifact::*;

mod check;
pub use check::*;

mod validation;

mod format;
pub use format::*;

mod clippy;
pub use clippy::*;

mod test_run;
pub use test_run::*;

pub mod mutation_test;
pub mod nextest;
pub mod semver_check;

pub mod job;

mod audit;
pub use audit::*;

mod explain;
pub use explain::{DiagnosticExplainPort, explain_diagnostic};

mod quality;
pub use quality::QualityPorts;

mod manifest_edit;
pub use manifest_edit::*;

mod mutation;
pub use mutation::*;

mod rust_mutation;
pub use rust_mutation::*;

mod resolution;
pub use resolution::{
    PreparedSemanticMutation, ProjectResolutionPort, ResolutionError, SemanticPreparationError,
};
