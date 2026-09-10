//! ADR-078: the host side of the offline vendor capture.
//!
//! Three things live here and nothing else: the SHA-256 the domain's framing is
//! fed to, the two entry points a host uses (capture a tree, verify an
//! artifact), and the replayable handle a runtime receives. Everything about
//! *what* is admitted is in `rust_engineering_domain::vendor_capture`;
//! everything about *how* the filesystem is touched is in the macOS module.
use rust_engineering_application::vendor_capture::{VendorCaptureAccess, VerifiedVendorCapture};
use rust_engineering_application::{OperationControl, ProjectError};
use rust_engineering_domain::vendor_capture::{CaptureHasher, VendorCapture, VendorCaptureError};
use rust_engineering_domain::{OperationalErrorCode, SourceFingerprint};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

/// SHA-256 behind the domain's framing-only hasher. The domain decides what is
/// hashed and in what order; this decides nothing.
#[derive(Default)]
pub struct Sha256Hasher(Sha256);

impl CaptureHasher for Sha256Hasher {
    fn update(&mut self, bytes: &[u8]) {
        self.0.update(bytes);
    }
    fn finish(self) -> Result<SourceFingerprint, VendorCaptureError> {
        let mut encoded = String::from("sha256:");
        for byte in self.0.finalize() {
            use std::fmt::Write;
            write!(&mut encoded, "{byte:02x}").map_err(|_| VendorCaptureError::Invalid)?;
        }
        encoded.parse().map_err(|_| VendorCaptureError::Invalid)
    }
}

/// One mapping, so every site reports the same refusal for the same cause.
pub fn capture_error(error: VendorCaptureError) -> ProjectError {
    ProjectError::Rejected(match error {
        VendorCaptureError::Limits => OperationalErrorCode::OutputLimitExceeded,
        // A link, a moved tree and a wrong digest are all refusals of host
        // data, not limits: they say the capture is not what it claimed to be.
        VendorCaptureError::Invalid
        | VendorCaptureError::Link
        | VendorCaptureError::Mutated
        | VendorCaptureError::Digest => OperationalErrorCode::InvalidProject,
    })
}

/// Capture `directory` into `store` and return the identity of the artifact.
///
/// ADR-078 §3: this is provisioning. It is reached from the explicit CLI and
/// never as a side effect of a measurement.
pub fn capture_vendor_tree(
    directory: &Path,
    store: &Path,
    control: &dyn OperationControl,
) -> Result<VendorCapture, ProjectError> {
    #[cfg(target_os = "macos")]
    {
        crate::filesystem::capture_vendor_tree(directory, store, control)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (directory, store, control);
        Err(ProjectError::Rejected(
            OperationalErrorCode::UnsupportedPlatform,
        ))
    }
}

/// An artifact whose digest was checked against the declared one, held open at
/// the descriptor the verification read.
///
/// ADR-078 §2 is why this is a type and not a `(VendorCapture, PathBuf)`: if a
/// consumer reopened the path it would be using an artifact nobody verified.
pub struct NativeVendorCapture {
    capture: VendorCapture,
    artifact: Mutex<std::fs::File>,
}

impl NativeVendorCapture {
    /// The verified identity. Inherent as well as on the trait, so a host tool
    /// can read it without importing the runtime port.
    pub fn capture(&self) -> &VendorCapture {
        &self.capture
    }
}

impl VerifiedVendorCapture for NativeVendorCapture {
    fn capture(&self) -> &VendorCapture {
        &self.capture
    }

    fn rewind(&self) -> Result<(), VendorCaptureAccess> {
        use std::io::Seek;
        let mut artifact = self.artifact.lock().map_err(|_| VendorCaptureAccess::Io)?;
        artifact
            .seek(std::io::SeekFrom::Start(0))
            .map(|_| ())
            .map_err(|_| VendorCaptureAccess::Io)
    }

    fn read(&self, buffer: &mut [u8]) -> Result<usize, VendorCaptureAccess> {
        let mut artifact = self.artifact.lock().map_err(|_| VendorCaptureAccess::Io)?;
        artifact.read(buffer).map_err(|_| VendorCaptureAccess::Io)
    }
}

/// Open the artifact `declared` names, re-derive its identity incrementally and
/// refuse it if the digest is not the declared one.
pub fn open_verified_capture(
    artifact: &Path,
    declared: &SourceFingerprint,
    control: &dyn OperationControl,
) -> Result<NativeVendorCapture, ProjectError> {
    #[cfg(target_os = "macos")]
    {
        let (capture, file) =
            crate::filesystem::verify_vendor_capture(artifact, declared, control)?;
        Ok(NativeVendorCapture {
            capture,
            artifact: Mutex::new(file),
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (artifact, declared, control);
        Err(ProjectError::Rejected(
            OperationalErrorCode::UnsupportedPlatform,
        ))
    }
}

/// The artifact file name a capture is published under: the 64 hex digits of
/// its tree digest, so an existing capture is found by digest (ADR-078 §2).
pub fn capture_artifact_name(digest: &SourceFingerprint) -> Option<String> {
    digest
        .as_str()
        .strip_prefix("sha256:")
        .filter(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
}
