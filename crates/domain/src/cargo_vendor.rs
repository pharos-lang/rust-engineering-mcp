//! Host-approved immutable Cargo directory-source data; no filesystem authority.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CargoVendorPackage {
    pub name: String,
    pub version: String,
    pub package_checksum: crate::SourceFingerprint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CargoVendorSnapshot {
    pub source: crate::SourceBundle,
    pub tree_fingerprint: crate::SourceFingerprint,
    pub packages: Vec<CargoVendorPackage>,
}
