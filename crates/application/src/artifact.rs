use rust_engineering_domain::{
    ArtifactError, ArtifactId, ArtifactMetadata, ArtifactView, ProjectRef,
};

/// Trusted nonblocking input. A zero count is EOF; counts must not exceed buffer.len().
/// The caller retains timeout/cancellation responsibilities for upstream producers.
pub trait ArtifactInput {
    /// True when the producer discarded bytes before supplying this input.
    fn truncated(&self) -> bool {
        false
    }
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, ArtifactError>;
}

/// Internal M0 port. M1 callers must separately authorize a live ProjectRef.
pub trait ArtifactStore {
    fn capture(
        &mut self,
        owner: &ProjectRef,
        input: &mut dyn ArtifactInput,
    ) -> Result<ArtifactMetadata, ArtifactError>;
    fn read<'a>(
        &'a mut self,
        owner: &ProjectRef,
        id: &ArtifactId,
    ) -> Result<ArtifactView<'a>, ArtifactError>;
    /// Remove only this owner's artifact. Missing and foreign IDs both return false.
    fn remove(&mut self, owner: &ProjectRef, id: &ArtifactId) -> Result<bool, ArtifactError>;
    /// Keep only artifacts owned by the current registry capabilities.
    fn retain_owners(&mut self, owners: &[ProjectRef]) -> Result<usize, ArtifactError>;
    fn revoke_owner(&mut self, owner: &ProjectRef) -> Result<usize, ArtifactError>;
    fn cleanup(&mut self) -> Result<usize, ArtifactError>;
}
