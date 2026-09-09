//! ADR-078: how a verified vendor capture reaches a measuring tool.
//!
//! The application never opens a path and never holds the artifact. What it
//! passes down is a *stream* the adapter already verified: ADR-078 §5 forbids a
//! path that materializes hundreds of MiB in RAM, and the measurements are the
//! reason — 2,7 MB over the interpreter floor read incrementally, against
//! 171 MB with the tree resident and 341 MB with the artifact resident.
use rust_engineering_domain::vendor_capture::VendorCapture;
use rust_engineering_domain::{CargoVendorSnapshot, SourceFingerprint};

/// Why a verified capture could not be replayed.
///
/// It is deliberately short. A capture that failed verification never becomes a
/// [`VerifiedVendorCapture`] at all, so nothing here can mean "the digest was
/// wrong": by this point the digest was already the declared one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VendorCaptureAccess {
    /// The artifact could not be read.
    Io,
    /// The artifact changed after it was verified. A capture is immutable; one
    /// that moved is refused rather than replayed.
    Mutated,
    Cancelled,
}

impl std::fmt::Display for VendorCaptureAccess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Io => "verified vendor capture could not be read",
            Self::Mutated => "verified vendor capture changed after verification",
            Self::Cancelled => "vendor capture replay was cancelled",
        })
    }
}
impl std::error::Error for VendorCaptureAccess {}

/// A capture whose digest has already been checked against the declared one,
/// and whose artifact can be replayed one buffer at a time.
///
/// `&self` with interior mutability rather than `&mut self`: the runtime port
/// receives it behind a shared reference, and the position is the adapter's
/// business, not the application's.
pub trait VerifiedVendorCapture: Send + Sync {
    /// The identity that was verified. Its tree digest is what travels in the
    /// provenance of any measurement that used this capture.
    fn capture(&self) -> &VendorCapture;
    /// Position the stream at the first artifact byte.
    fn rewind(&self) -> Result<(), VendorCaptureAccess>;
    /// Fill `buffer`. `Ok(0)` means the artifact ended, and it ends exactly at
    /// the byte count the verified capture recorded.
    fn read(&self, buffer: &mut [u8]) -> Result<usize, VendorCaptureAccess>;
}

/// Where a measuring tool resolves its offline harness from.
///
/// Both arms are host-authenticated and neither is a host directory the guest
/// can see: ADR-078 §1 refuses mounting a mutable host directory read-only,
/// because the guest's mount mode says nothing about what the host may do to
/// that directory while it is in use.
pub enum BenchmarkVendor<'a> {
    /// The `SourceBundle`-backed path every qualified M2/M4 flow uses. Its
    /// bounds and its alphabet are untouched by ADR-078.
    Snapshot(&'a CargoVendorSnapshot),
    /// ADR-078's capture: bigger, digest-addressed and immutable.
    Capture(&'a dyn VerifiedVendorCapture),
}

impl BenchmarkVendor<'_> {
    /// The fingerprint an observation must carry and a dataset's provenance
    /// quotes. One accessor, so the two arms cannot disagree about which value
    /// identifies the offline data a measurement used.
    pub fn tree_fingerprint(&self) -> &SourceFingerprint {
        match self {
            Self::Snapshot(snapshot) => &snapshot.tree_fingerprint,
            Self::Capture(capture) => capture.capture().tree_digest(),
        }
    }

    pub fn snapshot(&self) -> Option<&CargoVendorSnapshot> {
        match self {
            Self::Snapshot(snapshot) => Some(snapshot),
            Self::Capture(_) => None,
        }
    }

    pub fn capture(&self) -> Option<&dyn VerifiedVendorCapture> {
        match self {
            Self::Snapshot(_) => None,
            Self::Capture(capture) => Some(*capture),
        }
    }
}
