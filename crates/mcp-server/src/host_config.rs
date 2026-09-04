//! Shared closed trusted-host flags; preserves serve semantics.
use crate::stdio;
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};
pub(crate) fn parse(mut args: impl Iterator<Item = OsString>) -> Option<stdio::HostConfig> {
    let mut config = stdio::HostConfig {
        audit: None,
        catalog: None,
        roots: Vec::new(),
        ttl_seconds: 1800,
        rust: None,
    };
    let mut ttl_seen = false;
    let mut catalog_options: [Option<PathBuf>; 4] = std::array::from_fn(|_| None);
    let mut audit_path = None;
    let mut audit_fingerprint = None;
    let mut rust_options: [Option<std::ffi::OsString>; 4] = std::array::from_fn(|_| None);
    while let Some(flag) = args.next() {
        let value = args.next()?;
        if flag == OsStr::new("--root") && config.roots.len() < 16 && value.to_str().is_some() {
            config.roots.push(PathBuf::from(value));
        } else if flag == OsStr::new("--project-ttl-secs") && !ttl_seen {
            let ttl = value
                .to_str()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|ttl| (1..=86_400).contains(ttl))?;
            config.ttl_seconds = ttl;
            ttl_seen = true;
        } else if let Some(index) = [
            "--catalog-store",
            "--catalog-trust",
            "--catalog-model-dir",
            "--catalog-index-store",
        ]
        .iter()
        .position(|name| flag == OsStr::new(name))
        {
            let path = PathBuf::from(&value);
            if catalog_options[index].is_some() || value.to_str().is_none() || !path.is_absolute() {
                return None;
            }
            catalog_options[index] = Some(path);
        } else if flag == OsStr::new("--rustsec-snapshot") && audit_path.is_none() {
            let path = PathBuf::from(&value);
            if value.to_str().is_none() || !path.is_absolute() {
                return None;
            }
            audit_path = Some(path);
        } else if flag == OsStr::new("--rustsec-sha256") && audit_fingerprint.is_none() {
            let fingerprint = value.to_str().and_then(|value| {
                value
                    .parse::<rust_engineering_domain::CatalogFingerprint>()
                    .ok()
            })?;
            audit_fingerprint = Some(fingerprint);
        } else {
            let index = [
                "--docker",
                "--docker-socket",
                "--state-root",
                "--rust-image",
            ]
            .iter()
            .position(|name| flag == OsStr::new(name))?;
            if rust_options[index].is_some() || value.to_str().is_none() {
                return None;
            }
            rust_options[index] = Some(value);
        }
    }
    if rust_options.iter().any(Option::is_some) {
        let [
            Some(executable),
            Some(socket),
            Some(state_root),
            Some(image),
        ] = rust_options
        else {
            return None;
        };
        if image != OsStr::new(rust_engineering_execution::APPROVED_RUST_IMAGE) {
            return None;
        }
        config.rust = Some(rust_engineering_execution::HostDockerConfig {
            executable: executable.into(),
            socket: socket.into(),
            state_root: state_root.into(),
            image_id: rust_engineering_execution::APPROVED_RUST_IMAGE.into(),
        });
    }
    if catalog_options.iter().any(Option::is_some) {
        let [Some(store), Some(trust), model_dir, index_store] = catalog_options else {
            return None;
        };
        if index_store.is_some() && model_dir.is_none() {
            return None;
        }
        config.catalog = Some(stdio::HostCatalogConfig {
            store,
            trust,
            model_dir,
            index_store,
        });
    }
    config.audit = match (audit_path, audit_fingerprint) {
        (None, None) => None,
        (Some(path), Some(fingerprint)) => Some(stdio::HostAuditConfig { path, fingerprint }),
        _ => return None,
    };
    Some(config)
}
