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
        fix_write_roots: Vec::new(),
        dependency_add_roots: Vec::new(),
        dependency_remove_roots: Vec::new(),
        cargo_vendor: None,
        audit: None,
        catalog: None,
        roots: Vec::new(),
        ttl_seconds: 1800,
        rust: None,
    };
    let mut ttl_seen = false;
    let mut catalog_options: [Option<PathBuf>; 4] = std::array::from_fn(|_| None);
    let mut vendor_path = None;
    let mut vendor_fingerprint = None;
    let mut audit_path = None;
    let mut audit_fingerprint = None;
    let mut rust_options: [Option<std::ffi::OsString>; 4] = std::array::from_fn(|_| None);
    while let Some(flag) = args.next() {
        let value = args.next()?;
        if flag == OsStr::new("--root") && config.roots.len() < 16 && value.to_str().is_some() {
            config.roots.push(PathBuf::from(value));
        } else if flag == OsStr::new("--allow-manifest-write")
            || flag == OsStr::new("--allow-fmt-write")
            || flag == OsStr::new("--allow-fix-write")
            || flag == OsStr::new("--allow-dependency-add")
            || flag == OsStr::new("--allow-dependency-remove")
        {
            let roots = if flag == OsStr::new("--allow-manifest-write") {
                &mut config.manifest_write_roots
            } else if flag == OsStr::new("--allow-fmt-write") {
                &mut config.fmt_write_roots
            } else if flag == OsStr::new("--allow-fix-write") {
                &mut config.fix_write_roots
            } else if flag == OsStr::new("--allow-dependency-add") {
                &mut config.dependency_add_roots
            } else {
                &mut config.dependency_remove_roots
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
        } else if flag == OsStr::new("--cargo-vendor-dir") && vendor_path.is_none() {
            let path = PathBuf::from(&value);
            if value.to_str().is_none() || !path.is_absolute() {
                return None;
            }
            vendor_path = Some(path);
        } else if flag == OsStr::new("--cargo-vendor-tree-sha256") && vendor_fingerprint.is_none() {
            vendor_fingerprint = Some(
                value
                    .to_str()?
                    .parse::<rust_engineering_domain::SourceFingerprint>()
                    .ok()?,
            );
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
    config.cargo_vendor = match (vendor_path, vendor_fingerprint) {
        (None, None) => None,
        (Some(directory), Some(fingerprint))
            if config.rust.is_some()
                && !config
                    .roots
                    .iter()
                    .any(|root| directory.starts_with(root) || root.starts_with(&directory)) =>
        {
            Some(stdio::HostCargoVendorConfig {
                directory,
                fingerprint,
            })
        }
        _ => return None,
    };
    let has_writes = !config.manifest_write_roots.is_empty()
        || !config.fmt_write_roots.is_empty()
        || !config.fix_write_roots.is_empty()
        || !config.dependency_add_roots.is_empty()
        || !config.dependency_remove_roots.is_empty();
    if has_writes
        && (config.rust.is_none()
            || config
                .manifest_write_roots
                .iter()
                .chain(config.fmt_write_roots.iter())
                .chain(config.fix_write_roots.iter())
                .chain(config.dependency_add_roots.iter())
                .chain(config.dependency_remove_roots.iter())
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
    // Test paths are POSIX-style; a leading slash is not absolute on Windows,
    // where a path needs a drive prefix. The same prefix is applied to every
    // path, so the containment and disjointness relationships these tests
    // assert are identical on both platforms.
    #[cfg(not(windows))]
    const PRIVATE_STATE_RUST_MCP_MUTATIONS_V1_PROJECT: &str =
        "/private/state/rust-mcp-mutations-v1/project";
    #[cfg(windows)]
    const PRIVATE_STATE_RUST_MCP_MUTATIONS_V1_PROJECT: &str =
        r"C:/private/state/rust-mcp-mutations-v1/project";
    #[cfg(not(windows))]
    const USR_LOCAL_BIN_DOCKER: &str = "/usr/local/bin/docker";
    #[cfg(windows)]
    const USR_LOCAL_BIN_DOCKER: &str = r"C:/usr/local/bin/docker";
    #[cfg(not(windows))]
    const WORK_PROJECT_VENDOR: &str = "/work/project/vendor";
    #[cfg(windows)]
    const WORK_PROJECT_VENDOR: &str = r"C:/work/project/vendor";
    #[cfg(not(windows))]
    const WORK_PROJECT_STATE: &str = "/work/project/state";
    #[cfg(windows)]
    const WORK_PROJECT_STATE: &str = r"C:/work/project/state";
    #[cfg(not(windows))]
    const TMP_DOCKER_SOCK: &str = "/tmp/docker.sock";
    #[cfg(windows)]
    const TMP_DOCKER_SOCK: &str = r"C:/tmp/docker.sock";
    #[cfg(not(windows))]
    const OTHER_PROJECT: &str = "/other/project";
    #[cfg(windows)]
    const OTHER_PROJECT: &str = r"C:/other/project";
    #[cfg(not(windows))]
    const PRIVATE_STATE: &str = "/private/state";
    #[cfg(windows)]
    const PRIVATE_STATE: &str = r"C:/private/state";
    #[cfg(not(windows))]
    const WORK_PROJECT: &str = "/work/project";
    #[cfg(windows)]
    const WORK_PROJECT: &str = r"C:/work/project";
    #[cfg(not(windows))]
    const DATA_VENDOR: &str = "/data/vendor";
    #[cfg(windows)]
    const DATA_VENDOR: &str = r"C:/data/vendor";
    #[cfg(not(windows))]
    const DATA_OTHER: &str = "/data/other";
    #[cfg(windows)]
    const DATA_OTHER: &str = r"C:/data/other";
    #[cfg(not(windows))]
    const WORK: &str = "/work";
    #[cfg(windows)]
    const WORK: &str = r"C:/work";
    #[test]
    fn mutation_journal_and_read_roots_cannot_overlap_in_either_direction() {
        for permission in [
            "--allow-manifest-write",
            "--allow-fmt-write",
            "--allow-fix-write",
            "--allow-dependency-add",
            "--allow-dependency-remove",
        ] {
            let parse_roots = |read: &str, write: &str, state: &str| {
                parse(
                    [
                        "--root",
                        read,
                        permission,
                        write,
                        "--docker",
                        USR_LOCAL_BIN_DOCKER,
                        "--docker-socket",
                        TMP_DOCKER_SOCK,
                        "--state-root",
                        state,
                        "--rust-image",
                        rust_engineering_execution::APPROVED_RUST_IMAGE,
                    ]
                    .into_iter()
                    .map(OsString::from),
                )
            };
            assert!(parse_roots(WORK_PROJECT, WORK_PROJECT, PRIVATE_STATE).is_some());
            assert!(parse_roots(WORK_PROJECT, WORK_PROJECT, WORK_PROJECT_STATE).is_none());
            assert!(
                parse_roots(
                    PRIVATE_STATE_RUST_MCP_MUTATIONS_V1_PROJECT,
                    PRIVATE_STATE_RUST_MCP_MUTATIONS_V1_PROJECT,
                    PRIVATE_STATE
                )
                .is_none()
            );
            assert!(parse_roots(WORK_PROJECT, OTHER_PROJECT, PRIVATE_STATE).is_none());
        }
    }
    #[test]
    fn vendor_data_requires_a_complete_host_pair_runtime_and_disjoint_root() {
        let fingerprint = format!("sha256:{}", "a".repeat(64));
        let base = [
            "--root",
            WORK_PROJECT,
            "--docker",
            USR_LOCAL_BIN_DOCKER,
            "--docker-socket",
            TMP_DOCKER_SOCK,
            "--state-root",
            PRIVATE_STATE,
            "--rust-image",
            rust_engineering_execution::APPROVED_RUST_IMAGE,
        ];
        let configured = |extra: &[&str]| {
            parse(
                base.iter()
                    .copied()
                    .chain(extra.iter().copied())
                    .map(OsString::from),
            )
        };
        assert!(configured(&[]).is_some());
        assert!(
            configured(&[
                "--cargo-vendor-dir",
                DATA_VENDOR,
                "--cargo-vendor-tree-sha256",
                &fingerprint
            ])
            .is_some()
        );
        for arguments in [
            vec!["--cargo-vendor-dir", DATA_VENDOR],
            vec!["--cargo-vendor-tree-sha256", &fingerprint],
            vec![
                "--cargo-vendor-dir",
                "relative",
                "--cargo-vendor-tree-sha256",
                &fingerprint,
            ],
            vec![
                "--cargo-vendor-dir",
                WORK_PROJECT_VENDOR,
                "--cargo-vendor-tree-sha256",
                &fingerprint,
            ],
            vec![
                "--cargo-vendor-dir",
                WORK,
                "--cargo-vendor-tree-sha256",
                &fingerprint,
            ],
            vec![
                "--cargo-vendor-dir",
                DATA_VENDOR,
                "--cargo-vendor-tree-sha256",
                "sha256:bad",
            ],
            vec![
                "--cargo-vendor-dir",
                DATA_VENDOR,
                "--cargo-vendor-tree-sha256",
                &fingerprint,
                "--cargo-vendor-dir",
                DATA_OTHER,
            ],
        ] {
            assert!(configured(&arguments).is_none(), "accepted {arguments:?}");
        }
        assert!(
            parse(
                [
                    "--cargo-vendor-dir",
                    DATA_VENDOR,
                    "--cargo-vendor-tree-sha256",
                    &fingerprint
                ]
                .into_iter()
                .map(OsString::from)
            )
            .is_none()
        );
    }
}
