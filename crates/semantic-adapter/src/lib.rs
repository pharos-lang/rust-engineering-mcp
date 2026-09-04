//! Verified offline semantic adapter. Core builds validate bundles; `local` adds
//! real fastembed CPU inference and memory-only LanceDB, without download features.
mod model;
pub use model::{E5_FILES, E5_REVISION, VerifiedE5Bundle};
#[cfg(feature = "local")]
mod embedding;
#[cfg(feature = "local")]
pub use embedding::{LocalEmbeddingProvider, OfflineRuntime};
#[cfg(feature = "local")]
mod index;
#[cfg(feature = "local")]
pub use index::LanceMemoryIndex;
