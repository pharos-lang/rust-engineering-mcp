//! Bounded journaled publication for host-authorized local mutations.
#[cfg(not(target_os = "macos"))]
use rust_engineering_application::OperationControl;
#[cfg(not(target_os = "macos"))]
use rust_engineering_domain::MutationKind;
#[cfg(not(target_os = "macos"))]
use rust_engineering_domain::{
    IdempotencyKey, MutationCommit, MutationId, MutationReceipt, MutationRecordSummary,
};
use rust_engineering_domain::{MutationCandidate, MutationError, SourceFingerprint};
use sha2::{Digest, Sha256};
#[cfg(not(target_os = "macos"))]
use std::path::{Path, PathBuf};

const MAX_VALIDATION_BYTES: usize = 64 * 1024;

fn field(hash: &mut Sha256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn bundle(hash: &mut Sha256, value: &rust_engineering_domain::SourceBundle) {
    hash.update((value.directories().len() as u64).to_le_bytes());
    for directory in value.directories() {
        field(hash, directory.as_bytes());
    }
    hash.update((value.files().len() as u64).to_le_bytes());
    for file in value.files() {
        field(hash, file.path().as_bytes());
        field(hash, file.bytes());
    }
}

/// Exact domain-separated digest used by preview and commit.
pub fn mutation_digest(candidate: &MutationCandidate) -> Result<SourceFingerprint, MutationError> {
    if candidate.validation.len() > MAX_VALIDATION_BYTES {
        return Err(MutationError::LimitExceeded);
    }
    let mut hash = Sha256::new();
    hash.update(b"rust-engineering-mcp/mutation-candidate/v1\0");
    let kind = match candidate.kind {
        rust_engineering_domain::MutationKind::ManifestPatch => b"manifest_patch".as_slice(),
        rust_engineering_domain::MutationKind::FormatApply => b"format_apply".as_slice(),
        rust_engineering_domain::MutationKind::FixApply => b"fix_apply".as_slice(),
        rust_engineering_domain::MutationKind::DependencyAdd => b"dependency_add".as_slice(),
        rust_engineering_domain::MutationKind::DependencyRemove => b"dependency_remove".as_slice(),
    };
    field(&mut hash, kind);
    bundle(&mut hash, &candidate.before);
    bundle(&mut hash, &candidate.after);
    field(&mut hash, candidate.validation.as_bytes());
    let mut encoded = String::from("sha256:");
    for byte in hash.finalize() {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").map_err(|_| MutationError::Io)?;
    }
    encoded.parse().map_err(|_| MutationError::Io)
}

#[cfg(target_os = "macos")]
pub use crate::filesystem::NativeMutationStore;

#[cfg(not(target_os = "macos"))]
pub struct NativeMutationStore;

#[cfg(not(target_os = "macos"))]
impl NativeMutationStore {
    pub fn open(_state_path: &Path, _write_roots: &[PathBuf]) -> Result<Self, MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
    pub fn open_for_kind(
        _state_path: &Path,
        _write_roots: &[PathBuf],
        _kind: MutationKind,
    ) -> Result<Self, MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
    pub fn commit(
        &self,
        _lease: &crate::ProjectLease,
        _request: &MutationCommit,
        _control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
    pub fn replay(
        &self,
        _lease: &crate::ProjectLease,
        _id: &MutationId,
        _digest: &SourceFingerprint,
        _key: &IdempotencyKey,
        _control: &dyn OperationControl,
    ) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
    pub fn authorize(&self, _lease: &crate::ProjectLease) -> Result<(), MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
    pub fn receipt(
        &self,
        _lease: &crate::ProjectLease,
        _id: &MutationId,
    ) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
    pub fn recover(
        &self,
        _lease: &crate::ProjectLease,
        _id: &MutationId,
    ) -> Result<MutationReceipt, MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
    pub fn list_records(&self) -> Result<Vec<MutationRecordSummary>, MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
    pub fn prune_record(
        &self,
        _id: &MutationId,
        _digest: &SourceFingerprint,
    ) -> Result<(), MutationError> {
        Err(MutationError::UnsupportedPlatform)
    }
}
