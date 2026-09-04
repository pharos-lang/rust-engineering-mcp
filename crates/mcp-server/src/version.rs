//! Build facts; no runtime capability inference.
use serde::Serialize;
use std::{
    io::{self, Write},
    process::ExitCode,
};
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct Report {
    format_version: u32,
    operation: &'static str,
    package: &'static str,
    version: &'static str,
    compiled_local: bool,
    target_os: &'static str,
    target_arch: &'static str,
}
pub(crate) fn run(json: bool) -> ExitCode {
    let bytes = if json {
        serde_json::to_vec(&Report {
            format_version: 1,
            operation: "version",
            package: "rust-engineering-mcp",
            version: env!("CARGO_PKG_VERSION"),
            compiled_local: cfg!(feature = "local"),
            target_os: std::env::consts::OS,
            target_arch: std::env::consts::ARCH,
        })
    } else {
        Ok(format!("rust-engineering-mcp {}", env!("CARGO_PKG_VERSION")).into_bytes())
    };
    match bytes {
        Ok(mut bytes) => {
            bytes.push(b'\n');
            if io::stdout().lock().write_all(&bytes).is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(_) => ExitCode::FAILURE,
    }
}
