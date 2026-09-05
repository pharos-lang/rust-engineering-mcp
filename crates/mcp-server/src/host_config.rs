//! Shared closed trusted-host flags; preserves serve semantics.
use crate::stdio;
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};
pub(crate) fn parse(mut args: impl Iterator<Item = OsString>) -> Option<stdio::HostConfig> {
    let mut config = stdio::HostConfig {
        manifest_write_roots: Vec::new(),
        fmt_write_roots: Vec::new(),
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
        } else if flag == OsStr::new("--allow-manifest-write")
            || flag == OsStr::new("--allow-fmt-write")
        {
            let roots = if flag == OsStr::new("--allow-manifest-write") {
                &mut config.manifest_write_roots
            } else {
                &mut config.fmt_write_roots
            };
            let path = PathBuf::from(&value);
            if value.to_str().is_none()
                || !path.is_absolute()
                || roots.contains(&path)
                || roots.len() >= 16
            {
                return None;
            }
            roots.push(path);
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
    let has_writes = !config.manifest_write_roots.is_empty() || !config.fmt_write_roots.is_empty();
    if has_writes
        && (config.rust.is_none()
            || config
                .manifest_write_roots
                .iter()
                .chain(config.fmt_write_roots.iter())
                .any(|root| !config.roots.iter().any(|read| root.starts_with(read))))
    {
        return None;
    }
    if has_writes {
        let runtime = config.rust.as_ref()?;
        let journal = runtime.state_root.join("rust-mcp-mutations-v1");
        if config
            .roots
            .iter()
            .any(|read| journal.starts_with(read) || read.starts_with(&journal))
        {
            return None;
        }
    }
    Some(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mutation_journal_and_read_roots_cannot_overlap_in_either_direction() {
        let parse_roots = |read: &str, write: &str, state: &str| {
            parse(
                [
                    "--root",
                    read,
                    "--allow-manifest-write",
                    write,
                    "--docker",
                    "/usr/local/bin/docker",
                    "--docker-socket",
                    "/tmp/docker.sock",
                    "--state-root",
                    state,
                    "--rust-image",
                    rust_engineering_execution::APPROVED_RUST_IMAGE,
                ]
                .into_iter()
                .map(OsString::from),
            )
        };
        assert!(parse_roots("/work/project", "/work/project", "/private/state").is_some());
        assert!(parse_roots("/work/project", "/work/project", "/work/project/state").is_none());
        assert!(
            parse_roots(
                "/private/state/rust-mcp-mutations-v1/project",
                "/private/state/rust-mcp-mutations-v1/project",
                "/private/state"
            )
            .is_none()
        );
        assert!(parse_roots("/work/project", "/other/project", "/private/state").is_none());
    }
}
