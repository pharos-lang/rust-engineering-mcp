//! Passive, compiled inventory. These pins describe requirements, never installation readiness.
use serde::Serialize;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityRuntimeInventory {
    format_version: u32,
    operation: &'static str,
    installation_observed: bool,
    image_id: &'static str,
    target: &'static str,
    cargo_deny: Component,
    unsafe_scanner: Component,
    nightly: &'static str,
    rust_commit: &'static str,
    sysroot_path: &'static str,
    sysroot_tree_sha256: &'static str,
    provisioning: &'static str,
}
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Component {
    version: &'static str,
    path: &'static str,
    sha256: &'static str,
}
pub fn security_runtime_inventory() -> SecurityRuntimeInventory {
    SecurityRuntimeInventory {
        format_version: 1,
        operation: "security_runtime_inventory",
        installation_observed: false,
        image_id: crate::APPROVED_M4_IMAGE,
        target: "aarch64-unknown-linux-gnu",
        cargo_deny: Component {
            version: "0.19.7",
            path: "/opt/security/bin/cargo-deny",
            sha256: "e9bcd2f489b8dd22cc3f3fc3452cfa0483cf8b9236a5e2787c54f864d4e77715",
        },
        unsafe_scanner: Component {
            version: "0.1.0",
            path: "/opt/security/bin/rust-mcp-unsafe-helper",
            sha256: "af8af1a021094003cd90023a882d707f7062cc98938bb06a4c105bf75e10120b",
        },
        nightly: "nightly-2026-09-07",
        rust_commit: crate::miri_admission::NIGHTLY_COMMIT,
        sysroot_path: crate::miri_admission::SYSROOT,
        sysroot_tree_sha256: crate::miri_admission::SYSROOT_HASH,
        provisioning: "explicit host acquisition and offline build; runtime never downloads",
    }
}
