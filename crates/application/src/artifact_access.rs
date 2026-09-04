//! Live project authorization around the bounded artifact storage port.
use crate::{
    ArtifactStore, OperationControl, ProjectBackend, ProjectError, ProjectRegistry,
    ReferenceGenerator, RegistryClock,
};
use rust_engineering_domain::{ArtifactError, ArtifactId, ArtifactMetadata, ProjectRef};

const MAX_CONTENT: usize = 256 * 1024;

#[derive(Debug)]
pub struct AuthorizedArtifact {
    pub metadata: ArtifactMetadata,
    pub content: Vec<u8>,
    pub retention_remaining_seconds: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactAccessError {
    NotFound,
    Cancelled,
    Internal,
}
impl From<ProjectError> for ArtifactAccessError {
    fn from(error: ProjectError) -> Self {
        match error {
            ProjectError::Cancelled => Self::Cancelled,
            ProjectError::Internal => Self::Internal,
            ProjectError::Rejected(_) => Self::NotFound,
        }
    }
}
impl From<ArtifactError> for ArtifactAccessError {
    fn from(error: ArtifactError) -> Self {
        match error {
            ArtifactError::NotFound => Self::NotFound,
            _ => Self::Internal,
        }
    }
}
fn retention(metadata: &ArtifactMetadata, now: u64) -> Result<u64, ArtifactAccessError> {
    if now < metadata.created_seconds || metadata.expires_seconds <= metadata.created_seconds {
        return Err(ArtifactAccessError::Internal);
    }
    metadata
        .expires_seconds
        .checked_sub(now)
        .filter(|remaining| *remaining > 0)
        .ok_or(ArtifactAccessError::NotFound)
}
impl<B: ProjectBackend, G: ReferenceGenerator, C: RegistryClock> ProjectRegistry<B, G, C> {
    pub(crate) fn reap_artifacts(
        &mut self,
        store: &mut impl ArtifactStore,
    ) -> Result<(), ArtifactError> {
        self.prune();
        let owners: Vec<_> = self.entries.keys().cloned().collect();
        store.retain_owners(&owners)?;
        Ok(())
    }
    pub fn read_artifact(
        &mut self,
        reference: &ProjectRef,
        id: &ArtifactId,
        store: &mut impl ArtifactStore,
        artifact_clock: &impl RegistryClock,
        control: &dyn OperationControl,
    ) -> Result<AuthorizedArtifact, ArtifactAccessError> {
        self.read_artifact_with_touch(reference, id, store, artifact_clock, control, true)
    }
    pub(crate) fn read_artifact_without_touch(
        &mut self,
        reference: &ProjectRef,
        id: &ArtifactId,
        store: &mut impl ArtifactStore,
        artifact_clock: &impl RegistryClock,
        control: &dyn OperationControl,
    ) -> Result<AuthorizedArtifact, ArtifactAccessError> {
        self.read_artifact_with_touch(reference, id, store, artifact_clock, control, false)
    }
    fn read_artifact_with_touch(
        &mut self,
        reference: &ProjectRef,
        id: &ArtifactId,
        store: &mut impl ArtifactStore,
        artifact_clock: &impl RegistryClock,
        control: &dyn OperationControl,
        touch: bool,
    ) -> Result<AuthorizedArtifact, ArtifactAccessError> {
        self.reap_artifacts(store)?;
        let result = self.read_artifact_inner(reference, id, store, artifact_clock, control, touch);
        if result.is_err() {
            // Revalidation may have retired a capability; do not keep its logs.
            self.reap_artifacts(store)?;
        }
        result
    }
    fn read_artifact_inner(
        &mut self,
        reference: &ProjectRef,
        id: &ArtifactId,
        store: &mut impl ArtifactStore,
        artifact_clock: &impl RegistryClock,
        control: &dyn OperationControl,
        touch: bool,
    ) -> Result<AuthorizedArtifact, ArtifactAccessError> {
        self.resolve_inner(reference, control, false)?;
        let view = store.read(reference, id)?;
        if &view.metadata.owner != reference || &view.metadata.id != id {
            return Err(ArtifactAccessError::NotFound);
        }
        retention(view.metadata, artifact_clock.seconds())?;
        if view.content.len() > MAX_CONTENT
            || view.metadata.size_bytes as usize != view.content.len()
        {
            return Err(ArtifactAccessError::Internal);
        }
        let metadata = view.metadata.clone();
        let content = view.content.to_vec();
        self.resolve_inner(reference, control, false)?;
        let current = store.read(reference, id)?;
        if &current.metadata.owner != reference || &current.metadata.id != id {
            return Err(ArtifactAccessError::NotFound);
        }
        if current.metadata != &metadata || current.content != content {
            return Err(ArtifactAccessError::Internal);
        }
        control.check()?;
        let retention_remaining_seconds = retention(&metadata, artifact_clock.seconds())?;
        if touch {
            self.touch_authorized_reference(reference)?;
        }
        Ok(AuthorizedArtifact {
            metadata,
            content,
            retention_remaining_seconds,
        })
    }
    /// No I/O after final grouped retention checks; failed touch never renews a lease.
    pub(crate) fn touch_authorized_reference(
        &mut self,
        reference: &ProjectRef,
    ) -> Result<(), ArtifactAccessError> {
        self.prune();
        let now = self.clock.seconds();
        let entry = self
            .entries
            .get_mut(reference)
            .ok_or(ArtifactAccessError::NotFound)?;
        if !now
            .checked_sub(entry.last_used)
            .is_some_and(|age| age < self.ttl_seconds)
        {
            self.entries.remove(reference);
            return Err(ArtifactAccessError::NotFound);
        }
        entry.last_used = now;
        Ok(())
    }
}
