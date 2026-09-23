//! Single source of truth for `--help` output.
//!
//! Every accepted command has exactly one `&'static str` section here. The
//! top-level help ([`full_help`]) is composed from the same sections that
//! [`lookup`] returns for `<command> --help` / `<command> -h`, so the two
//! surfaces cannot drift apart. Only the parsers in `host_config.rs`,
//! `catalog_cli.rs`, `mutation_cli.rs`, `quality_artifact_cli.rs`,
//! `cargo_vendor_cli.rs`, `doctor.rs`, `capabilities.rs`, `contract_cli.rs`
//! and `version.rs`/`main.rs` decide what is actually accepted; this module
//! only documents it.
use std::ffi::OsString;

const HEADER: &str = "Rust Engineering MCP — development server\n\nUsage: rust-engineering-mcp <COMMAND>\n\nCommands:\n";

const HELP_LINE: &str = "  help           Show this help (-h, --help)\n";

const TOOLS_FOOTER: &str = "\nAvailable tools: rust.project.open; rust.project.inspect; rust.toolchain.inspect; rust.check; rust.fmt.check; rust.clippy; rust.test; rust.dependencies.audit; rust.diagnostics.explain; rust.quality.gate; rust.catalog.status; rust.crate.search; rust.crate.inspect; rust.manifest.patch; rust.fmt.apply; rust.fix.apply; rust.dependency.add; rust.dependency.remove; rust.test.nextest; rust.coverage; rust.semver.check; rust.mutation.test; rust.deny; rust.unsafe.scan; rust.supply_chain.inspect; rust.quality.gate.v2; rust.miri; rust.benchmark.run; rust.benchmark.compare; rust.profile.flamegraph; rust.binary.bloat; rust.analyzer.symbols; rust.analyzer.references; rust.analyzer.diagnostics; rust.analyzer.actions; rust.analyzer.action.apply (explicit approved Rust runtime required except project.open, catalog.status, crate.search and crate.inspect; rust.analyzer.* additionally require the M6 analyzer runtime supplied via --rust-image, and rust.analyzer.action.apply additionally requires the --allow-analyzer-action-write grant).\n";

const QUALITY_ARTIFACTS: &str = "  quality-artifacts recover --state-root PATH [--json]\n  quality-artifacts prune --state-root PATH [--json]\n                 Reconcile quarantined/unknown-version objects, or expire artifacts past TTL (ADR-061); local only, never called by MCP tools\n";

const MUTATION: &str = "  mutation list --state-root PATH [--json]\n  mutation prune --state-root PATH --operation-id ID --plan-digest sha256:ID [--json]\n                 Inspect journals or remove one completed local receipt explicitly\n";

const CARGO_VENDOR: &str = "  cargo-vendor inspect --directory PATH [--json]\n  cargo-vendor capture --directory PATH --into PATH [--json]\n                 Fingerprint an offline vendor tree for --cargo-vendor-dir/--cargo-vendor-tree-sha256, or capture\n                 the larger Criterion tree for --vendor-capture/--vendor-capture-tree-sha256; never runs Cargo\n                 or touches the network\n";

const CATALOG: &str = "  catalog status --store PATH --trust PATH [--model-dir PATH [--index-store PATH]] [--json]\n  catalog import SNAPSHOT --store PATH --trust PATH [--model-dir PATH] [--json]\n  catalog sync --source SNAPSHOT --store PATH --trust PATH [--model-dir PATH] [--json]\n  catalog sync --url HTTPS_URL --allow-host HOST --store PATH --trust PATH [--model-dir PATH] [--json]\n  catalog rebuild-index --store PATH --trust PATH --model-dir PATH --index-store PATH [--json]\n                 --store/--trust are always required together, outside every serve --root; sync accepts\n                 exactly one of --source or --url with --allow-host (the only subcommand in this binary\n                 that makes a real network request, and only to that declared host)\n";

const VERSION: &str = "  version [--json] Show package version/build facts (-V, --version)\n";

const SECURITY_RUNTIME: &str = "  security-runtime inventory [--json]\n                 Show compiled security runtime requirements; does not inspect or install\n";

const DOCTOR: &str = "  doctor [--active] [--json] [same host flags as serve --stdio]\n  doctor [--active] [--json] --state-root PATH\n                 Diagnose configured local state; --active calibrates the approved Rust runtime\n                 (requires the full --docker/--docker-socket/--state-root/--rust-image group). The\n                 lone --state-root form (no Docker group) only computes mutation_journals.\n";

const CAPABILITIES: &str = "  capabilities [--json | --human] --docker PATH --docker-socket PATH --state-root PATH --probe-image sha256:ID\n                 Actively probe the approved local sandbox; --json is the default format\n";

const CONTRACT: &str = "  contract [--json | --human]\n                 Static spec §56 capabilities document: all 36 tool definitions, stability,\n                 canonical schema/description hashes and runtime requirements; no host access\n";

const SERVE: &str = "  serve --stdio [--root PATH]... [--project-ttl-secs N]\n        [--catalog-store PATH --catalog-trust PATH [--catalog-model-dir PATH [--catalog-index-store PATH]]]\n        [--allow-manifest-write WORKSPACE_ROOT]...\n        [--allow-fmt-write WORKSPACE_ROOT]...\n        [--allow-fix-write WORKSPACE_ROOT]...\n        [--allow-analyzer-action-write WORKSPACE_ROOT]...\n        [--allow-dependency-add WORKSPACE_ROOT]...\n        [--allow-dependency-remove WORKSPACE_ROOT]...\n        [--cargo-vendor-dir PATH --cargo-vendor-tree-sha256 sha256:ID]\n        [--vendor-capture PATH --vendor-capture-tree-sha256 sha256:ID]\n        [--allow-profiling user-space-sampling]\n        [--security-policy PATH --security-policy-sha256 sha256:ID]\n        [--rustsec-snapshot PATH --rustsec-sha256 sha256:ID]\n        [--docker PATH --docker-socket PATH --state-root PATH --rust-image sha256:ID]\n                 Serve MCP with host-authorized physical roots (default: none). --root and every\n                 --allow-*-write are each repeatable up to 16 times. --docker/--docker-socket/\n                 --state-root/--rust-image is one all-or-nothing group, required by any --allow-*-write,\n                 --cargo-vendor-dir, --vendor-capture and --allow-profiling. Write grants are never\n                 active by default.\n";

/// One entry per top-level command that accepts its own `--help`/`-h`.
/// `subcommands` lists the words after which `--help`/`-h` also resolves to
/// this same `text` (e.g. `catalog status --help`); an empty slice means the
/// command has no such nested form.
pub(crate) struct Command {
    words: &'static [&'static str],
    subcommands: &'static [&'static str],
    text: &'static str,
}

pub(crate) const COMMANDS: &[Command] = &[
    Command {
        words: &["quality-artifacts"],
        subcommands: &["recover", "prune"],
        text: QUALITY_ARTIFACTS,
    },
    Command {
        words: &["mutation"],
        subcommands: &["list", "prune"],
        text: MUTATION,
    },
    Command {
        words: &["cargo-vendor"],
        subcommands: &["inspect", "capture"],
        text: CARGO_VENDOR,
    },
    Command {
        words: &["catalog"],
        subcommands: &["status", "import", "sync", "rebuild-index"],
        text: CATALOG,
    },
    Command {
        words: &["version", "--version", "-V"],
        subcommands: &[],
        text: VERSION,
    },
    Command {
        words: &["security-runtime"],
        subcommands: &["inventory"],
        text: SECURITY_RUNTIME,
    },
    Command {
        words: &["doctor"],
        subcommands: &[],
        text: DOCTOR,
    },
    Command {
        words: &["capabilities"],
        subcommands: &[],
        text: CAPABILITIES,
    },
    Command {
        words: &["contract"],
        subcommands: &[],
        text: CONTRACT,
    },
    Command {
        words: &["serve"],
        subcommands: &[],
        text: SERVE,
    },
];

/// The full top-level help, composed from [`COMMANDS`] plus `help` (which has
/// no section of its own) and the tool inventory footer.
pub(crate) fn full_help() -> String {
    let mut text = String::from(HEADER);
    for command in COMMANDS {
        text.push_str(command.text);
    }
    text.push_str(HELP_LINE);
    text.push_str(TOOLS_FOOTER);
    text
}

fn is_help_flag(value: &OsString) -> bool {
    value == "--help" || value == "-h"
}

/// Resolves `<command> --help` / `<command> -h`, and, for the commands with
/// subcommands, `<command> <subcommand> --help` / `-h`, to that command's
/// section. Returns `None` for every other invocation, which then falls
/// through to the real parser unchanged — this never rejects or accepts an
/// invocation on its own.
pub(crate) fn lookup(args: &[OsString]) -> Option<&'static str> {
    let first = args.first()?.to_str()?;
    let command = COMMANDS
        .iter()
        .find(|command| command.words.contains(&first))?;
    match args {
        [_, flag] if is_help_flag(flag) => Some(command.text),
        [_, sub, flag] if is_help_flag(flag) => {
            let sub = sub.to_str()?;
            command.subcommands.contains(&sub).then_some(command.text)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn resolves_every_documented_command_and_nested_subcommand() {
        for command in COMMANDS {
            for &word in command.words {
                for flag in ["--help", "-h"] {
                    assert_eq!(
                        lookup(&args(&[word, flag])),
                        Some(command.text),
                        "{word} {flag}"
                    );
                }
            }
            for &sub in command.subcommands {
                for flag in ["--help", "-h"] {
                    let word = command.words[0];
                    assert_eq!(
                        lookup(&args(&[word, sub, flag])),
                        Some(command.text),
                        "{word} {sub} {flag}"
                    );
                }
            }
        }
    }

    #[test]
    fn does_not_intercept_unrelated_or_incomplete_invocations() {
        for invocation in [
            vec!["serve", "--stdio"],
            vec!["serve", "--stdio", "--help"],
            vec!["catalog"],
            vec!["catalog", "status"],
            vec!["catalog", "unknown", "--help"],
            vec!["doctor"],
            vec!["unknown"],
            vec!["unknown", "--help"],
            vec!["--help"],
            vec!["-h"],
            vec!["help"],
            vec![],
        ] {
            assert!(lookup(&args(&invocation)).is_none(), "{invocation:?}");
        }
    }

    #[test]
    fn full_help_contains_every_section_and_the_tool_inventory() {
        let text = full_help();
        assert!(text.starts_with(HEADER));
        for command in COMMANDS {
            assert!(text.contains(command.text));
        }
        assert!(text.contains("rust.project.open"));
        assert!(text.ends_with(TOOLS_FOOTER));
    }
}
