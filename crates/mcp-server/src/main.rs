use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::process::ExitCode;

mod capabilities;
mod cargo_vendor_cli;
mod catalog_cli;
mod catalog_semantic;
mod catalog_sync;
mod contract_cli;
mod doctor;
mod doctor_run;
mod help;
mod host_config;
mod mutation_cli;
mod quality_artifact_cli;
mod stdio;
mod version;

const USAGE_ERROR: &str = "Unsupported invocation. Use 'rust-engineering-mcp --help'.\n";

enum Invocation {
    Catalog(catalog_cli::Invocation),
    Mutation(mutation_cli::Invocation),
    QualityArtifacts(quality_artifact_cli::Invocation),
    CargoVendor(cargo_vendor_cli::Invocation),
    Help(String),
    SecurityInventory,
    Version { json: bool },
    Doctor(doctor::Invocation),
    ServeStdio(stdio::HostConfig),
    Capabilities(capabilities::Invocation),
    Contract(contract_cli::Invocation),
    Unsupported,
}

fn invocation() -> Invocation {
    let raw_args: Vec<OsString> = env::args_os().skip(1).collect();
    // `<command> --help`/`-h`, and for commands with subcommands
    // `<command> <subcommand> --help`/`-h`, resolve to that command's help
    // section before any parser below runs; `serve --help` in particular
    // must never reach `host_config::parse` or start the server.
    if let Some(text) = help::lookup(&raw_args) {
        return Invocation::Help(text.to_string());
    }
    let mut args = raw_args.into_iter();
    let Some(command) = args.next() else {
        return Invocation::Unsupported;
    };
    if command == OsStr::new("security-runtime") {
        if args.next().as_deref() != Some(OsStr::new("inventory")) {
            return Invocation::Unsupported;
        }
        if let Some(flag) = args.next()
            && (flag != OsStr::new("--json") || args.next().is_some())
        {
            return Invocation::Unsupported;
        }
        return Invocation::SecurityInventory;
    }
    if command == OsStr::new("doctor") {
        return doctor::parse(args)
            .map(Invocation::Doctor)
            .unwrap_or(Invocation::Unsupported);
    }
    if ["version", "--version", "-V"]
        .iter()
        .any(|v| command == OsStr::new(v))
    {
        let json = match args.next() {
            None => false,
            Some(v) if v == OsStr::new("--json") => true,
            _ => return Invocation::Unsupported,
        };
        return if args.next().is_none() {
            Invocation::Version { json }
        } else {
            Invocation::Unsupported
        };
    }
    if command == OsStr::new("mutation") {
        return mutation_cli::parse(args)
            .map(Invocation::Mutation)
            .unwrap_or(Invocation::Unsupported);
    }
    if command == OsStr::new("quality-artifacts") {
        return quality_artifact_cli::parse(args)
            .map(Invocation::QualityArtifacts)
            .unwrap_or(Invocation::Unsupported);
    }
    if command == OsStr::new("cargo-vendor") {
        return cargo_vendor_cli::parse(args)
            .map(Invocation::CargoVendor)
            .unwrap_or(Invocation::Unsupported);
    }
    if command == OsStr::new("catalog") {
        return catalog_cli::parse(args)
            .map(Invocation::Catalog)
            .unwrap_or(Invocation::Unsupported);
    }
    if command == OsStr::new("capabilities") {
        return capabilities::parse(args)
            .map(Invocation::Capabilities)
            .unwrap_or(Invocation::Unsupported);
    }
    if command == OsStr::new("contract") {
        return contract_cli::parse(args)
            .map(Invocation::Contract)
            .unwrap_or(Invocation::Unsupported);
    }
    if command == OsStr::new("serve") {
        if args.next().as_deref() != Some(OsStr::new("--stdio")) {
            return Invocation::Unsupported;
        }
        return host_config::parse(args)
            .map(Invocation::ServeStdio)
            .unwrap_or(Invocation::Unsupported);
    }
    if args.next().is_some() {
        return Invocation::Unsupported;
    }

    if ["help", "--help", "-h"]
        .iter()
        .any(|value| command == OsStr::new(value))
    {
        Invocation::Help(help::full_help())
    } else {
        Invocation::Unsupported
    }
}

fn main() -> ExitCode {
    let (result, code) = match invocation() {
        Invocation::Catalog(config) => return catalog_cli::run(config),
        Invocation::Mutation(config) => return mutation_cli::run(config),
        Invocation::QualityArtifacts(config) => return quality_artifact_cli::run(config),
        Invocation::CargoVendor(config) => return cargo_vendor_cli::run(config),
        Invocation::SecurityInventory => {
            let result = serde_json::to_writer_pretty(
                io::stdout().lock(),
                &rust_engineering_execution::security_runtime_inventory(),
            );
            return if result.is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        Invocation::Help(text) => (io::stdout().lock().write_all(text.as_bytes()), 0),
        Invocation::Version { json } => return version::run(json),
        Invocation::Doctor(config) => return doctor_run::run(config),
        Invocation::Unsupported => (io::stderr().lock().write_all(USAGE_ERROR.as_bytes()), 2),
        Invocation::ServeStdio(config) => return stdio::run(config),
        Invocation::Capabilities(config) => return capabilities::run(config),
        Invocation::Contract(config) => return contract_cli::run(config),
    };

    if result.is_err() {
        ExitCode::FAILURE
    } else {
        ExitCode::from(code)
    }
}
