use crate::{
    OperationControl, ProjectBackend, ProjectError, ProjectRegistry, ReferenceGenerator,
    RegistryClock,
};
use rust_engineering_domain::{OperationalErrorCode, ProjectRef, SourceBundle};

/// Capture bytes through an existing lease, never through caller-supplied paths.
pub trait ProjectSourceBackend: ProjectBackend {
    fn source(
        &self,
        lease: &Self::Lease,
        control: &dyn OperationControl,
    ) -> Result<SourceBundle, ProjectError>;
}

impl<B: ProjectSourceBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub fn source(
        &mut self,
        reference: &ProjectRef,
        control: &dyn OperationControl,
    ) -> Result<SourceBundle, ProjectError> {
        self.source_inner(reference, control, true)
    }

    pub(crate) fn source_inner(
        &mut self,
        reference: &ProjectRef,
        control: &dyn OperationControl,
        touch: bool,
    ) -> Result<SourceBundle, ProjectError> {
        self.resolve_inner(reference, control, false)?;
        let entry = self.entries.get(reference).ok_or(ProjectError::Rejected(
            OperationalErrorCode::ProjectNotFound,
        ))?;
        let result = self.backend.source(&entry.project.lease, control);
        // Revalidate even a rejected capture, except cancellation, which preserves
        // the lease. No bytes leave the registry after an observed identity change.
        if result == Err(ProjectError::Cancelled) {
            return result;
        }
        self.resolve_inner(reference, control, touch && result.is_ok())?;
        result
    }
}
