//! Explicit inspection of host-supplied Cargo data; no Cargo or network execution.
use rust_engineering_application::{OperationControl, ProjectError};
use rust_engineering_domain::{CargoVendorSnapshot, OperationalErrorCode};
use serde::Serialize;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::{Duration, Instant},
};

pub struct Invocation {
    directory: PathBuf,
    json: bool,
}
pub fn parse(mut args: impl Iterator<Item = OsString>) -> Option<Invocation> {
    if args.next()?.to_str()? != "inspect" {
        return None;
    }
    let (mut directory, mut json) = (None, false);
    while let Some(flag) = args.next() {
        match flag.to_str()? {
            "--directory" if directory.is_none() => {
                let path = PathBuf::from(args.next()?);
                if !path.is_absolute() || path.to_str().is_none() {
                    return None;
                }
                directory = Some(path);
            }
            "--json" if !json => json = true,
            _ => return None,
        }
    }
    Some(Invocation {
        directory: directory?,
        json,
    })
}
struct Deadline(Instant);
impl OperationControl for Deadline {
    fn check(&self) -> Result<(), ProjectError> {
        if Instant::now() >= self.0 {
            Err(ProjectError::Rejected(OperationalErrorCode::CommandTimeout))
        } else {
            Ok(())
        }
    }
}
#[derive(Serialize)]
struct Package {
    name: String,
    version: String,
    package_checksum: String,
}
#[derive(Serialize)]
struct Report {
    format_version: u32,
    status: &'static str,
    error_code: Option<&'static str>,
    message: &'static str,
    tree_fingerprint: Option<String>,
    file_count: usize,
    total_bytes: usize,
    packages: Vec<Package>,
}
fn report(result: Result<CargoVendorSnapshot, ProjectError>) -> Report {
    match result {
        Ok(snapshot) => Report {
            format_version: 1,
            status: "passed",
            error_code: None,
            message: "Captured directory source and verified all file checksums; approve this exact fingerprint in host configuration",
            tree_fingerprint: Some(snapshot.tree_fingerprint.to_string()),
            file_count: snapshot.source.files().len(),
            total_bytes: snapshot
                .source
                .files()
                .iter()
                .map(|file| file.bytes().len())
                .sum(),
            packages: snapshot
                .packages
                .into_iter()
                .map(|package| Package {
                    name: package.name,
                    version: package.version,
                    package_checksum: package.package_checksum.to_string(),
                })
                .collect(),
        },
        Err(error) => Report {
            format_version: 1,
            status: "blocked",
            error_code: Some(match error {
                ProjectError::Cancelled => "cancelled",
                ProjectError::Rejected(OperationalErrorCode::UnsupportedPlatform) => {
                    "unsupported_platform"
                }
                ProjectError::Rejected(OperationalErrorCode::OutputLimitExceeded) => {
                    "limit_exceeded"
                }
                ProjectError::Rejected(OperationalErrorCode::CommandTimeout) => "command_timeout",
                ProjectError::Rejected(OperationalErrorCode::SandboxDenied) => "permission_denied",
                ProjectError::Rejected(_) => "invalid_cargo_data",
                ProjectError::Internal => "io",
            }),
            message: "Cargo data was not approved; verify the directory, integrity and native capture limits before configuring it",
            tree_fingerprint: None,
            file_count: 0,
            total_bytes: 0,
            packages: vec![],
        },
    }
}
pub fn run(invocation: Invocation) -> ExitCode {
    let result = rust_engineering_project::inspect_cargo_vendor(
        &invocation.directory,
        &Deadline(Instant::now() + Duration::from_secs(30)),
    );
    let report = report(result);
    let code = u8::from(report.error_code.is_some());
    let mut bytes = if invocation.json {
        match serde_json::to_vec(&report) {
            Ok(bytes) => bytes,
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        let mut text = format!(
            "cargo-vendor inspect: {}\n{}",
            report.status, report.message
        );
        if let Some(hash) = &report.tree_fingerprint {
            use std::fmt::Write;
            if write!(
                text,
                "\n{hash}\n{} packages, {} files, {} bytes",
                report.packages.len(),
                report.file_count,
                report.total_bytes
            )
            .is_err()
            {
                return ExitCode::FAILURE;
            }
        }
        text.into_bytes()
    };
    bytes.push(b'\n');
    if bytes.len() > 512 * 1024 {
        return ExitCode::FAILURE;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::FAILURE,
    };
    let result = runtime.block_on(async {
        let output = tokio::task::spawn_blocking(move || {
            let mut out = io::stdout().lock();
            out.write_all(&bytes)?;
            out.flush()
        });
        match tokio::time::timeout(Duration::from_secs(5), output).await {
            Ok(Ok(Ok(()))) => ExitCode::from(code),
            _ => ExitCode::FAILURE,
        }
    });
    runtime.shutdown_timeout(Duration::from_millis(100));
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    /// The parser requires an absolute directory, and a leading slash is not
    /// absolute on Windows, where a path needs a drive prefix.
    #[cfg(not(windows))]
    const VENDOR_DIR: &str = "/private/vendor";
    #[cfg(windows)]
    const VENDOR_DIR: &str = r"C:\private\vendor";
    #[test]
    fn closed_read_only_cli_rejects_implicit_provisioning_and_relative_roots() {
        for args in [
            vec!["inspect", "--directory", VENDOR_DIR, "--json"],
            vec!["inspect", "--directory", VENDOR_DIR],
        ] {
            assert!(parse(args.into_iter().map(OsString::from)).is_some());
        }
        for args in [
            vec!["inspect"],
            vec!["sync", "--directory", VENDOR_DIR],
            vec!["inspect", "--directory", "relative"],
            vec!["inspect", "--directory", VENDOR_DIR, "--allow-network"],
            vec!["inspect", "--directory", VENDOR_DIR, "--json", "--json"],
        ] {
            assert!(parse(args.into_iter().map(OsString::from)).is_none());
        }
    }
}
