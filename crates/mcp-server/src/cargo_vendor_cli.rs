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

pub enum Invocation {
    /// The M2/M4 path: capture a `SourceBundle`-sized directory source and
    /// report the fingerprint a host approves.
    Inspect { directory: PathBuf, json: bool },
    /// ADR-078 §3: provision a large vendor capture explicitly. It writes one
    /// artifact named by its own digest and reports that digest; it downloads
    /// nothing, and no measurement ever reaches this path.
    Capture {
        directory: PathBuf,
        store: PathBuf,
        json: bool,
    },
}

fn absolute(value: OsString) -> Option<PathBuf> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || path.to_str().is_none() {
        return None;
    }
    Some(path)
}

pub fn parse(mut args: impl Iterator<Item = OsString>) -> Option<Invocation> {
    let subcommand = args.next()?.to_str()?.to_owned();
    if !matches!(subcommand.as_str(), "inspect" | "capture") {
        return None;
    }
    let (mut directory, mut store, mut json) = (None, None, false);
    while let Some(flag) = args.next() {
        match flag.to_str()? {
            "--directory" if directory.is_none() => directory = Some(absolute(args.next()?)?),
            "--into" if store.is_none() && subcommand == "capture" => {
                store = Some(absolute(args.next()?)?);
            }
            "--json" if !json => json = true,
            _ => return None,
        }
    }
    let directory = directory?;
    if subcommand == "inspect" {
        if store.is_some() {
            return None;
        }
        return Some(Invocation::Inspect { directory, json });
    }
    Some(Invocation::Capture {
        directory,
        store: store?,
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
/// Capture, then read the published artifact back through the same verifier the
/// server uses, and report `passed` only if it re-derives the declared digest.
///
/// ADR-078 §2 makes a capture whose digest is not the declared one a refusal.
/// Applying that at the moment of provisioning is what lets the receipt mean
/// "this artifact verifies" rather than "these bytes were written": the digest
/// an operator is about to configure is one this binary already recovered from
/// the artifact, incrementally, and not one it only computed while writing.
fn capture_and_read_back(
    directory: &std::path::Path,
    store: &std::path::Path,
    control: &Deadline,
) -> Result<rust_engineering_domain::vendor_capture::VendorCapture, ProjectError> {
    use rust_engineering_project::vendor_capture::{capture_artifact_name, open_verified_capture};
    let captured =
        rust_engineering_project::vendor_capture::capture_vendor_tree(directory, store, control)?;
    let name = capture_artifact_name(captured.tree_digest()).ok_or(ProjectError::Internal)?;
    let verified = open_verified_capture(&store.join(name), captured.tree_digest(), control)?;
    if verified.capture() != &captured {
        return Err(ProjectError::Rejected(OperationalErrorCode::InvalidProject));
    }
    Ok(captured)
}

/// The same receipt shape as `inspect`, so an operator reads one report.
///
/// `file_count` and `total_bytes` are the capture's own counts, and
/// `tree_fingerprint` is the value the host configures as
/// `--vendor-capture-tree-sha256`.
fn capture_report(
    result: Result<rust_engineering_domain::vendor_capture::VendorCapture, ProjectError>,
) -> Report {
    match result {
        Ok(capture) => Report {
            format_version: 1,
            status: "passed",
            error_code: None,
            message: "Captured the vendor tree into an immutable artifact named by its digest, and read it back to the same digest; approve this exact fingerprint in host configuration",
            tree_fingerprint: Some(capture.tree_digest().to_string()),
            file_count: capture.files(),
            total_bytes: usize::try_from(capture.total_bytes()).unwrap_or(usize::MAX),
            packages: vec![],
        },
        Err(error) => report(Err(error)),
    }
}

pub fn run(invocation: Invocation) -> ExitCode {
    let (directory, store, json) = match invocation {
        Invocation::Inspect { directory, json } => (directory, None, json),
        Invocation::Capture {
            directory,
            store,
            json,
        } => (directory, Some(store), json),
    };
    // ADR-078 §4 projects 512 MiB to about 3,5 s of wall clock; the capture
    // budget is the inspect budget, which is already an order of magnitude
    // above that.
    let control = Deadline(Instant::now() + Duration::from_secs(30));
    let report = match &store {
        Some(store) => capture_report(capture_and_read_back(&directory, store, &control)),
        None => report(rust_engineering_project::inspect_cargo_vendor(
            &directory, &control,
        )),
    };
    let code = u8::from(report.error_code.is_some());
    let mut bytes = if json {
        match serde_json::to_vec(&report) {
            Ok(bytes) => bytes,
            Err(_) => return ExitCode::FAILURE,
        }
    } else {
        let mut text = format!(
            "cargo-vendor {}: {}\n{}",
            if store.is_some() {
                "capture"
            } else {
                "inspect"
            },
            report.status,
            report.message
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
            // `--into` belongs to `capture`; `inspect` writes nothing.
            vec!["inspect", "--directory", VENDOR_DIR, "--into", STORE_DIR],
        ] {
            assert!(parse(args.into_iter().map(OsString::from)).is_none());
        }
    }

    #[cfg(not(windows))]
    const STORE_DIR: &str = "/private/captures";
    #[cfg(windows)]
    const STORE_DIR: &str = r"C:\private\captures";

    /// ADR-078 §3: capture is provisioning. It takes an explicit destination,
    /// accepts no network flag, and is a subcommand rather than something a
    /// measurement can reach.
    #[test]
    fn capture_requires_an_explicit_absolute_destination_and_nothing_else() {
        for args in [
            vec!["capture", "--directory", VENDOR_DIR, "--into", STORE_DIR],
            vec![
                "capture",
                "--directory",
                VENDOR_DIR,
                "--into",
                STORE_DIR,
                "--json",
            ],
        ] {
            assert!(matches!(
                parse(args.into_iter().map(OsString::from)),
                Some(Invocation::Capture { .. })
            ));
        }
        for args in [
            vec!["capture", "--directory", VENDOR_DIR],
            vec!["capture", "--into", STORE_DIR],
            vec!["capture", "--directory", VENDOR_DIR, "--into", "relative"],
            vec![
                "capture",
                "--directory",
                VENDOR_DIR,
                "--into",
                STORE_DIR,
                "--allow-network",
            ],
            vec![
                "capture",
                "--directory",
                VENDOR_DIR,
                "--into",
                STORE_DIR,
                "--into",
                STORE_DIR,
            ],
        ] {
            assert!(parse(args.into_iter().map(OsString::from)).is_none());
        }
    }
}
