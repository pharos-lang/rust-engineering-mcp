# ADR-033 — Installed runtime toolchain observation

## Status

Accepted and implemented for M1-02. Core and actual Rust/MCP gates, independent
review and evidence recorded in validation/M1-02.md.

## Context

Specification23.3 requires rustc/Cargo versions, channel, host triple and installed
targets/components. M1-01 provides a calibrated approved runtime and a live project
lease. The Linux image has no rustup. Compiler supported targets do not establish
installed standard libraries. Distribution metadata alone is not installation evidence.

## Decision

Expose rust.toolchain.inspect with a closed project_ref-only input. Use the same
registry, admitted joined worker, RustProjectInspector instance, calibration latch
and quarantine as project.inspect. Capture the project and its fixed optional
selector, observe the approved runtime, then revalidate ProjectRef before publication.
No host toolchain discovery, provisioning, network or arbitrary program/argv/path.

Execute existing CompilerVersion and CargoVersion commands and add closed
InstalledComponents: /usr/bin/cat -- /opt/rust/lib/rustlib/components. The exact
path is written by the verified distribution installer. Parse bounded verbose
version records and installer component lines; verify versions/host/components
against the immutable approved image. Normalize explicit rustfmt-preview and
clippy-preview names. Derive installed targets only from rust-std-<triple> entries,
not rustc --print target-list. Unknown, duplicate, inconsistent or incomplete
records fail closed; never emit partial successful inventory.

The observation reports stable channel from the verified release, guest host
triple and installed inventory. It is explicitly Linux ARM64 even on a macOS host.
Each command retains its execution fingerprint; shared image/configuration and
captured source digest remain distinct. Evidence carries ProjectSnapshot
provenance/freshness and latest_known as in ADR-032. Installed inventory is not a
promise that every component was executed; real rustc/Cargo version commands are.

Bound each command output16KiB, identifiers128bytes and inventory32entries. Full
MCP result budget64KiB includes text plus structuredContent. Joined request
budget120s includes lazy calibration; each inventory execution30s. Cancellation,
TTL/identity invalidation and cleanup uncertainty cannot publish a partial result.
Adding a command changes the gateway configuration fingerprint and requires real
recalibration. Keep M1-01 output compatible; use a separate toolchain observation DTO.

## Alternatives considered

- rustup show/list: runtime has no rustup and must not install one implicitly.
- rustc --print target-list: lists supported targets, not installed components.
- infer installation from sources.json: manifest is availability, not observation.
- create another gateway per tool: splits admission/calibration/quarantine ownership.
- query host PATH or filesystem: changes the explicitly approved execution authority.

## Consequences

No new distribution, downloads or image rebuild expected. The installed component
file must be observed in actual Docker tests before marking Done. Existing image
identity and tools remain unchanged. Native cross-platform support remains pending.

## Sources

- Specification23.3; ADR-031/032.
- Official local Rust1.98.1 distribution archives: components and install.sh.
- https://doc.rust-lang.org/rustc/command-line-arguments/print-options.html#target-list
