//! Admit only graphs whose compiler/test diagnostic producer is not project-native code.
use crate::security_metadata::PreparedSecurityMetadata;
use rust_engineering_application::security::SecurityError;
use rust_engineering_domain::security::SecuritySource;
use rust_engineering_domain::{CargoVendorSnapshot, SourceBundle};

pub(super) const CONFIG: &[u8] =
    include_bytes!("../../../fixtures/m4-runtime-oracles/miri-classification/nextest.toml");
pub(super) const NIGHTLY: &str = "/opt/rust-nightly-2026-09-07/bin";
pub(super) const SYSROOT: &str = "/opt/miri-sysroot/2026-09-07/aarch64-unknown-linux-gnu";
pub(super) const NIGHTLY_COMMIT: &str = "5a2be9f5f075d31e3ca5526b5b029881ce441253";
pub(super) const SYSROOT_HASH: &str =
    "sha256:68324d8d8b2dcb55616ff2e53c7c91f4db78ada40ceafe189f933899d8a1f136";
pub(super) fn environment() -> Vec<String> {
    let mut values = vec![
        format!("PATH={NIGHTLY}:/opt/rust/bin:/usr/bin:/bin"),
        "HOME=/work".into(),
        "TMPDIR=/tmp".into(),
        "CARGO_HOME=/security/cargo-home".into(),
        "CARGO_TARGET_DIR=/work/target".into(),
        "CARGO_NET_OFFLINE=true".into(),
        "CARGO_INCREMENTAL=0".into(),
        format!("MIRI={NIGHTLY}/miri"),
        format!("MIRI_SYSROOT={SYSROOT}"),
        "MIRIFLAGS=--error-format=json -Zmiri-isolation-error=abort -Zmiri-backtrace=0".into(),
        format!("RUSTC={NIGHTLY}/rustc"),
        format!("CARGO={NIGHTLY}/cargo"),
    ];
    values.sort();
    values
}
fn invalid() -> SecurityError {
    SecurityError::InvalidMetadata
}
fn incompatible() -> SecurityError {
    SecurityError::ClassificationIntegrityUnsupported
}

pub(super) fn validate(
    metadata: &PreparedSecurityMetadata,
    source: &SourceBundle,
    vendor: &CargoVendorSnapshot,
) -> Result<(), SecurityError> {
    let value: serde_json::Value =
        serde_json::from_slice(&metadata.derived).map_err(|_| invalid())?;
    let packages = value
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(invalid)?;
    if packages.len() != metadata.packages.len() || metadata.package_roots.len() != packages.len() {
        return Err(invalid());
    }
    for (index, package) in packages.iter().enumerate() {
        for target in package
            .get("targets")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(invalid)?
        {
            for key in ["kind", "crate_types"] {
                let kinds = target
                    .get(key)
                    .and_then(serde_json::Value::as_array)
                    .ok_or_else(invalid)?;
                if kinds.is_empty() {
                    return Err(invalid());
                }
                for kind in kinds {
                    match kind.as_str() {
                        Some("proc-macro" | "custom-build") => return Err(incompatible()),
                        Some(_) => {}
                        None => return Err(invalid()),
                    }
                }
            }
        }
        let bundle = match metadata.packages[index].source {
            SecuritySource::Workspace => source,
            SecuritySource::CratesIo => &vendor.source,
            SecuritySource::Unverified => return Err(incompatible()),
        };
        let path = format!("{}Cargo.toml", metadata.package_roots[index]);
        let file = bundle
            .files()
            .iter()
            .find(|f| f.path() == path)
            .ok_or_else(invalid)?;
        validate_manifest(file.bytes())?;
    }
    Ok(())
}
fn validate_manifest(bytes: &[u8]) -> Result<(), SecurityError> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let value: toml::Value = toml::from_str(text).map_err(|_| invalid())?;
    for key in ["lib", "bin", "test", "bench", "example"] {
        let Some(value) = value.get(key) else {
            continue;
        };
        let entries = if let Some(array) = value.as_array() {
            array.iter().collect::<Vec<_>>()
        } else {
            vec![value]
        };
        for entry in entries {
            let entry = entry.as_table().ok_or_else(invalid)?;
            if entry.get("harness").and_then(toml::Value::as_bool) == Some(false)
                || ["proc-macro", "proc_macro"]
                    .iter()
                    .any(|key| entry.get(*key).and_then(toml::Value::as_bool) == Some(true))
            {
                return Err(incompatible());
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_harness_and_proc_macro_aliases_cannot_enter_the_diagnostic_oracle() {
        for source in [
            "[lib]\nharness=false",
            "[[test]]\nname='evil'\nharness=false",
            "[lib]\nproc-macro=true",
            "[lib]\nproc_macro=true",
        ] {
            assert_eq!(validate_manifest(source.as_bytes()), Err(incompatible()));
        }
        assert_eq!(
            validate_manifest(b"[package]\nname='clean'\nversion='0.1.0'\n[lib]\nharness=true\n"),
            Ok(())
        );
    }
    #[test]
    fn env_and_wrapper_only_mute_interpreted_execution_and_never_disable_isolation() {
        let environment = environment();
        assert!(environment.iter().any(|v| v
            == "MIRIFLAGS=--error-format=json -Zmiri-isolation-error=abort -Zmiri-backtrace=0"));
        assert!(
            environment
                .iter()
                .any(|v| v == &format!("MIRI_SYSROOT={SYSROOT}"))
        );
        let config = std::str::from_utf8(CONFIG).unwrap_or_default();
        assert!(config.contains("run-wrapper = \"miri-muted\""));
        assert!(!config.contains("list-wrapper"));
        assert!(config.contains("-Zmiri-mute-stdout-stderr"));
        assert!(!config.contains("disable-isolation"));
    }
}
