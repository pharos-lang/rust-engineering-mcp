use std::env;
use std::ffi::OsStr;
use std::io::{self, Write};
use std::process::ExitCode;

mod capabilities;
mod cargo_vendor_cli;
mod catalog_cli;
mod catalog_semantic;
mod catalog_sync;
mod doctor;
mod doctor_run;
mod host_config;
mod mutation_cli;
mod stdio;
mod version;

const HELP: &str = "Rust Engineering MCP — development server

Usage: rust-engineering-mcp <COMMAND>

Commands:
  mutation list --state-root PATH [--json]
  cargo-vendor inspect --directory PATH [--json]
  mutation prune --state-root PATH --operation-id ID --plan-digest sha256:ID [--json]
                 Inspect journals or remove one completed local receipt explicitly
  catalog status --store PATH --trust PATH [--json]
  catalog import SNAPSHOT --store PATH --trust PATH [--json]
  catalog sync --source SNAPSHOT --store PATH --trust PATH [--json]
  catalog sync --url HTTPS_URL --allow-host HOST --store PATH --trust PATH [--json]
                 Import explicitly supplied signed local mirror snapshots
  catalog rebuild-index --store PATH --trust PATH --index-store PATH --model-dir PATH [--json]
                 Rebuild native Lance objects using the verified installed E5 model
  help           Show this help (-h, --help)
  version [--json] Show package version/build facts (-V, --version)
  doctor [--active] [--json] [same host flags as serve]
                 Diagnose configured local state; --active calibrates approved Rust runtime
  capabilities [--json | --human] --docker PATH --docker-socket PATH --state-root PATH --probe-image sha256:ID
                 Actively probe the approved local sandbox; JSON output
  serve --stdio [--root PATH]... [--project-ttl-secs N]
        [--catalog-store PATH --catalog-trust PATH [--catalog-model-dir PATH] [--catalog-index-store PATH]]
        [--allow-manifest-write WORKSPACE_ROOT]...
        [--allow-fmt-write WORKSPACE_ROOT]...
        [--allow-fix-write WORKSPACE_ROOT]...
        [--allow-dependency-add WORKSPACE_ROOT]...
        [--allow-dependency-remove WORKSPACE_ROOT]...
        [--cargo-vendor-dir PATH --cargo-vendor-tree-sha256 sha256:ID]
        [--rustsec-snapshot PATH --rustsec-sha256 sha256:ID]
        [--docker PATH --docker-socket PATH --state-root PATH --rust-image sha256:ID]
                 Serve MCP with host-authorized physical roots (default: none)

Available tools: rust.project.open; rust.project.inspect; rust.toolchain.inspect; rust.check; rust.fmt.check; rust.clippy; rust.test; rust.dependencies.audit; rust.diagnostics.explain; rust.quality.gate; rust.catalog.status; rust.crate.search; rust.crate.inspect; rust.manifest.patch; rust.fmt.apply; rust.fix.apply; rust.dependency.add; rust.dependency.remove (explicit approved Rust runtime required except project.open, catalog.status, crate.search and crate.inspect).
";

const USAGE_ERROR: &str = "Unsupported invocation. Use 'rust-engineering-mcp --help'.\n";

enum Invocation {
    Catalog(catalog_cli::Invocation),
    Mutation(mutation_cli::Invocation),
    CargoVendor(cargo_vendor_cli::Invocation),
    Help,
    Version { json: bool },
    Doctor(doctor::Invocation),
    ServeStdio(stdio::HostConfig),
    Capabilities(capabilities::Invocation),
    Unsupported,
}

fn invocation() -> Invocation {
    let mut args = env::args_os().skip(1);
    let Some(command) = args.next() else {
        return Invocation::Unsupported;
    };
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
        Invocation::Help
    } else {
        Invocation::Unsupported
    }
}

fn main() -> ExitCode {
    let (result, code) = match invocation() {
        Invocation::Catalog(config) => return catalog_cli::run(config),
        Invocation::Mutation(config) => return mutation_cli::run(config),
        Invocation::CargoVendor(config) => return cargo_vendor_cli::run(config),
        Invocation::Help => (io::stdout().lock().write_all(HELP.as_bytes()), 0),
        Invocation::Version { json } => return version::run(json),
        Invocation::Doctor(config) => return doctor_run::run(config),
        Invocation::Unsupported => (io::stderr().lock().write_all(USAGE_ERROR.as_bytes()), 2),
        Invocation::ServeStdio(config) => return stdio::run(config),
        Invocation::Capabilities(config) => return capabilities::run(config),
    };

    if result.is_err() {
        ExitCode::FAILURE
    } else {
        ExitCode::from(code)
    }
}
