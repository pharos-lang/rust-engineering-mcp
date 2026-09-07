use crate::{ProjectLease, mutation_store::NativeMutationStore};
use rust_engineering_application::{MutationPublisher, OperationControl};
use rust_engineering_domain::{
    IdempotencyKey, MutationCommit, MutationError, MutationId, MutationReceipt, SourceFingerprint,
};

pub fn mutation_bytes_digest(
    bytes: &[u8],
) -> Result<rust_engineering_domain::SourceFingerprint, MutationError> {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let mut result = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        write!(result, "{byte:02x}").map_err(|_| MutationError::Io)?;
    }
    result.parse().map_err(|_| MutationError::Io)
}

impl MutationPublisher<ProjectLease> for NativeMutationStore {
    fn authorize(&self, lease: &ProjectLease) -> Result<(), MutationError> {
        self.authorize(lease)
    }
    fn commit(
        &self,
        lease: &ProjectLease,
        request: &MutationCommit,
        control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        self.commit(lease, request, control)
    }
    fn replay(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
        digest: &SourceFingerprint,
        key: &IdempotencyKey,
        control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        self.replay(lease, id, digest, key, control)
    }
    fn receipt(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
    ) -> Result<MutationReceipt, MutationError> {
        self.receipt(lease, id)
    }
    fn recover(
        &self,
        lease: &ProjectLease,
        id: &MutationId,
    ) -> Result<MutationReceipt, MutationError> {
        self.recover(lease, id)
    }
}
