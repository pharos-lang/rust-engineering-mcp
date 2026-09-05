//! Domain values and validated results, without protocol or I/O dependencies.
//!
//! Parsing a reference proves syntax only, never authority, existence or entropy.
//! Fingerprints are distinct values; hashing project inputs belongs to later cuts.
//!
//! ```compile_fail
//! use rust_engineering_domain::{ExecutionFingerprint, ProjectIdentityFingerprint};
//! fn execution_only(_: ExecutionFingerprint) {}
//! fn wrong_kind(identity: ProjectIdentityFingerprint) {
//!     execution_only(identity);
//! }
//! ```

mod diagnostic;
mod evidence;
mod result;
mod value;

pub use diagnostic::*;
pub use evidence::*;
pub use result::*;
pub use value::*;

// Unlike Serde's default Option handling, this requires a field to be present
// while still accepting an explicit null. Used for required-nullable contracts.
pub(crate) fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    serde::Deserialize::deserialize(deserializer)
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
mod rust_execution;
pub use rust_execution::*;

mod inspection;
pub use inspection::*;

mod toolchain;
pub use toolchain::*;

mod check;
pub use check::*;

mod format;
pub use format::{FormatObservation, ProjectFormat};

mod clippy;
pub use clippy::{ClippyOptions, ClippySelection, LintProfile, ProjectClippy};

mod test_run;
pub use test_run::{ProjectTest, TestObservation, TestOptions, TestSelection};

mod audit;
pub use audit::*;

mod explain;
pub use explain::*;

mod quality;
pub use quality::*;

mod manifest_edit;
pub use manifest_edit::*;

mod mutation;
pub use mutation::*;

mod rust_mutation;
pub use rust_mutation::*;
mod cargo_vendor;
pub use cargo_vendor::*;
