//! `rust-mcp-profile-helper`: a minimal sampling profiler for the hardened
//! aarch64 Linux guest image behind the `rust.profile.flamegraph` MCP tool.
//!
//! The helper launches exactly one child, profiles that child and nothing else,
//! and writes two files: collapsed stacks and a small JSON manifest.

use std::env;
use std::io::{self, Write};
use std::panic;

use rust_mcp_profile_helper::{
    Arguments, EXIT_INTERNAL_FAILURE, EXIT_INVALID_ARGUMENTS, parse_arguments,
};

#[cfg(target_os = "linux")]
mod linux;

/// The single line printed when the helper is run anywhere but its target.
pub(crate) const UNSUPPORTED: &str =
    "unsupported target: rust-mcp-profile-helper requires aarch64-unknown-linux-gnu";

fn main() {
    panic::set_hook(Box::new(|_| {
        report("internal failure: the profiler faulted");
    }));
    let code = panic::catch_unwind(run).unwrap_or(EXIT_INTERNAL_FAILURE);
    std::process::exit(code);
}

fn run() -> i32 {
    let arguments = match parse_arguments(env::args_os().skip(1)) {
        Ok(arguments) => arguments,
        Err(error) => {
            report(error.message());
            return EXIT_INVALID_ARGUMENTS;
        }
    };
    profile(&arguments)
}

#[cfg(target_os = "linux")]
fn profile(arguments: &Arguments) -> i32 {
    linux::profile(arguments)
}

#[cfg(not(target_os = "linux"))]
fn profile(_arguments: &Arguments) -> i32 {
    report(UNSUPPORTED);
    EXIT_INTERNAL_FAILURE
}

/// Writes one fixed line to stderr. Nothing derived from the profiled child or
/// from the filesystem is ever routed through here.
pub(crate) fn report(message: &str) {
    let mut stderr = io::stderr().lock();
    let _ = stderr.write_all(message.as_bytes());
    let _ = stderr.write_all(b"\n");
    let _ = stderr.flush();
}
