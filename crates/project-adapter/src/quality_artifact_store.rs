//! Public native quality store adapter surface; non-macOS has no filesystem effect.
//!
//! ADR-061 qualifies only macOS ARM64/APFS, so the native store is selected on
//! exactly `macos` + `aarch64`. Every other platform — including x86-64 macOS,
//! which is not a qualified host — rejects with `UnsupportedPlatform` before a
//! reservation, a gateway start, guest output or any filesystem fallback: the
//! unsupported implementation opens nothing.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub use crate::filesystem::{NativeQualityArtifactStore, prune_expired, recover};

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub use unsupported::{NativeQualityArtifactStore, prune_expired, recover};

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod unsupported {
    use rust_engineering_application::{
        QualityArtifactChunk, QualityArtifactIndexPage, QualityArtifactInput, QualityArtifactStore,
        QualityIngest, QualityOwnerFacts, QualityReservation,
    };
    use rust_engineering_domain::{
        PruneReport, QualityArtifactDescriptor, QualityArtifactError, QualityArtifactId,
        QualityJobId, RecoveryReport,
    };
    use std::path::Path;

    fn unsupported<T>() -> Result<T, QualityArtifactError> {
        Err(QualityArtifactError::UnsupportedPlatform)
    }

    /// Uninhabited in practice: neither constructor returns one on this platform.
    pub struct NativeQualityArtifactStore(());

    impl NativeQualityArtifactStore {
        pub fn open(_state_root: &Path) -> Result<Self, QualityArtifactError> {
            unsupported()
        }

        /// Attaching to an existing store fails closed exactly like `open`, so
        /// a reader on an unsupported platform reports the platform rather
        /// than an absent store.
        pub fn attach(_state_root: &Path) -> Result<Self, QualityArtifactError> {
            unsupported()
        }

        pub fn state_root_identity(&self) -> ((i64, u64), u32) {
            ((0, 0), 0)
        }
    }

    impl QualityArtifactStore for NativeQualityArtifactStore {
        fn owner_binding(&self, _: &QualityOwnerFacts) -> Result<[u8; 32], QualityArtifactError> {
            unsupported()
        }
        fn reserve(&mut self, _: &QualityReservation) -> Result<(), QualityArtifactError> {
            unsupported()
        }
        fn release(&mut self, _: &QualityReservation) -> Result<(), QualityArtifactError> {
            unsupported()
        }
        fn ingest_member(
            &mut self,
            _: &QualityReservation,
            _: u16,
            _: u64,
            _: &mut dyn QualityArtifactInput,
        ) -> Result<QualityIngest, QualityArtifactError> {
            unsupported()
        }
        fn publish_descriptor(
            &mut self,
            _: &QualityReservation,
            _: &QualityArtifactDescriptor,
        ) -> Result<(), QualityArtifactError> {
            unsupported()
        }
        fn read_chunk(
            &mut self,
            _: [u8; 32],
            _: &QualityArtifactId,
            _: u64,
            _: u32,
        ) -> Result<QualityArtifactChunk, QualityArtifactError> {
            unsupported()
        }
        fn read_index_page(
            &mut self,
            _: [u8; 32],
            _: &QualityJobId,
            _: Option<&[u8]>,
        ) -> Result<QualityArtifactIndexPage, QualityArtifactError> {
            unsupported()
        }
        fn reconcile_recover(&mut self) -> Result<RecoveryReport, QualityArtifactError> {
            unsupported()
        }
        fn prune_expired(&mut self) -> Result<PruneReport, QualityArtifactError> {
            unsupported()
        }
    }

    pub fn recover(_state_root: &Path) -> Result<RecoveryReport, QualityArtifactError> {
        unsupported()
    }
    pub fn prune_expired(_state_root: &Path) -> Result<PruneReport, QualityArtifactError> {
        unsupported()
    }
}
