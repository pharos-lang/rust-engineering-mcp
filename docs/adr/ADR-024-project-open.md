# ADR-024 — Project open: bounded structural validation and capabilities

## Status

Accepted for M0-04, 2026-09-03. Refines ADR-007 and the first tool contract of
ADR-015; does not authorize execution or close M0-05/M0-06/M0-07.

## Context

`project.open` precedes the Execution Gateway. Cargo cannot be invoked in
production until that boundary exists. Cargo's complete workspace discovery,
dependency resolution and unstable manifest semantics cannot be certified by a
second parser. Canonicalizing a path before opening it does not prevent link swaps.
The implementation must expose an honest, bounded capability on the tested OS.

## Decision

The host supplies up to 16 absolute, physical roots with repeated `--root PATH`
after `serve --stdio`. No roots grants no project access. Request JSON, environment,
MCP client roots and project configuration cannot add authority. `/` is rejected.
`--project-ttl-secs` accepts 1..86400, default 1800. A process-local registry holds
at most 64 references from 128-bit OS randomness, checks collisions, expires idle
entries and revalidates directory identity and manifest fingerprint on resolution.
Unknown/expired references fail; cancellation preserves an existing lease.
Capacity exhaustion rejects new opens instead of evicting live references; this
keeps existing authority stable until TTL expiry. A poisoned registry fails
closed and requires process restart; arbitrary panic recovery does not prove
application invariants. Resource-policy denial uses the existing SANDBOX_DENIED
code; no new error vocabulary is introduced by this cut.

`rust.project.open` accepts exactly `{ "path": "absolute explicit root" }`.
It returns the existing envelope vocabulary, an opaque `project_ref`,
`workspace_root`, identity `fingerprint` and `validation: "structural"`.
This means a supported manifest graph and required target files were observed;
it does **not** certify compilation, registry resolution or Cargo's ancestor
workspace discovery. The caller selects the workspace root explicitly. Glob
members, nested/external workspace owners, unstable `cargo-features`, `replace`
and implicit named-target discovery are rejected. Deprecated `[project]`,
`dev_dependencies` and `build_dependencies` spellings are explicitly rejected,
including target-specific groups; they must never be silently ignored. Literal members/excludes/default
members (excludes apply to descendant paths), package inheritance and path dependencies are checked within host roots.
The full Cargo metadata oracle belongs to M1-01 through the Execution Gateway.
Tests may invoke Cargo on trusted, generated fixtures to cross-check this subset.

Production reads use safe rustix 1.1.4 `openat` wrappers on macOS 26+ / APFS only.
Apple flags `O_NOFOLLOW_ANY`, `O_RESOLVE_BENEATH`, `O_UNIQUE` are named locally from
XNU headers. Startup checks the kernel and discriminates accepted `.` from rejected
absolute `/` with BENEATH. XNU rejects combining NOFOLLOW with NOFOLLOW_ANY;
the latter protects every component, including the leaf. Startup also requires
EINVAL for that conflicting flag pair, proving NOFOLLOW_ANY is recognized.
UNIQUE is defense in depth; fstat independently rejects link counts other than one.
Invalid configured roots or failed macOS capability detection reject startup
with exit 1; unsupported OS stubs serve per-call unavailable without reading roots.
Root acquisition starts at a held `/` descriptor; every project read resolves the
full relative path from the original authorized root, never a descendant handle.
All components reject symlinks; regular files require one link, directories and
files must remain on the root device/APFS. Root bindings and read metadata are
rechecked. Windows/Linux adapters fail closed before filesystem I/O; no junction
support is claimed. This is filesystem authorization, not an OS process sandbox.

Root/project descriptors retain object identity. Metadata rechecks detect observed
changes, but do not provide an atomic multi-file snapshot or prove the absence of
rename-ABA attacks by a concurrent writer. A held capability identifies the same
object even if renamed; it must not become authority for a different descendant
namespace. Hardlink rejection does not prove provenance against privileged actors.
Special files are rejected by fstat **after** a nonblocking open: no content is
read, but an open itself may be observable (FIFO) or invoke a device driver.
O_UNIQUE checks link count, not VREG. The supported host boundary assumes roots
without pre-existing device nodes and excludes privileged mount/device creation
by an attacker; APFS alone does not imply MNT_NODEV. This adapter does not promise
side-effect-free opening of arbitrary privileged filesystem objects. Enforcing
that stronger policy needs a regular-only kernel open or a verified NODEV mount
boundary and remains part of platform capability work; it must never be advertised
as currently enforced.

Limits: 4096 path bytes, 64 path components, 256 KiB per manifest, 4 MiB total,
128 manifests, graph depth 32, 512 dependency entries. The pinned toml 0.9.12
parser has structural recursion limit 80 (including dotted keys); feature
`unbounded` is not enabled. Wire tests use deeply nested inputs below the byte
limit and require subsequent successful calls in the same process. Only one blocking validation worker may run, with
no queue and a 10-second cooperative deadline. The slot remains held until actual
worker completion, even after timeout. Kernel reads cannot be forcibly cancelled;
rmcp's first-request bootstrap also delays incoming cancellation processing.
These limits do not claim hard wall-time isolation or a solution for long-running
process tools. M0-05/M0-06 must supply that boundary before execution.

Identity SHA-256 hashes a versioned domain separator, root device/inode, the
length-prefixed explicit root, then sorted length-prefixed manifest paths/bytes.
Target contents, Cargo.lock, configuration and toolchain are not execution identity.
`ExecutionFingerprint` remains a separate type, generated only for a future
concrete execution specification; no fabricated execution hash is returned.

Serde/schemars types are the wire/schema source. jsonschema 0.53 validates inputs
and outputs at runtime with network/file resolver features disabled. Local fixed
schemas require no remote resolution. Operational failures use `isError: true`;
malformed arguments use SDK InvalidParams. Structured and text content agree.
Annotations declare read-only, non-destructive, non-idempotent (new reference),
closed-world. General M0-07 contracts remain a separate cut.

## Alternatives considered

- Calling `cargo metadata` now: rejected, bypasses the missing gateway.
- Pretending a custom parser fully validates Cargo: rejected; scope is explicit.
- canonicalize + ordinary file open, or NOFOLLOW on only the last component:
  rejected because link-swap containment is absent.
- Broad cross-platform support with weaker fallback: rejected; unavailable is safer.
- Root-relative descendant handles: rejected because moved descendants retain a
  capability outside the intended root namespace.

## Consequences

Initial support is deliberately narrow; macOS `/tmp` and `/var` aliases are rejected,
and the host must use physical paths. Unsupported manifest forms require future
bounded support, not shell fallbacks. Schema/parser dependencies increase the
binary dependency graph but stay in adapters. Application depends only on domain.
Reproducible tests, external security review and the item gate are required before
marking M0-04 Done; they do not close the overall milestone.

## Sources

- [XNU open(2)](https://github.com/apple-oss-distributions/xnu/blob/main/bsd/man/man2/open.2)
- [XNU resolve_beneath tests](https://github.com/apple-oss-distributions/xnu/blob/main/tests/vfs/resolve_beneath.c)
- [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html)
- Pinned rustix 1.1.4, rmcp 3.2.0 and schemars/jsonschema sources in Cargo.lock;
  implementation tests use the actual host kernel, not header availability alone.
