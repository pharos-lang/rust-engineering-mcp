# ADR-064 — Quality job seccomp profile

## Status

Accepted 2026-09-06 by the M3 orchestrator. The independent security review
verified that the quality profile differs from the base profile by exactly one
rule: an AF_UNIX anonymous stream `socketpair`, with creation flags masked out;
`socket`, `bind`, `connect`, `listen` and `accept` remain absent. It also verified
that the applied container is checked against the phase-declared profile, so a
wider profile fails closed ([V-SEC](../validation/m3-delegation/V-SEC/last-message.md)).
The approved guest image passed the M3 runtime gate at 62/62 and the Rust
security gate at 20/20 ([runtime receipt](../validation/M3-runtime.json),
[Rust security receipt](../validation/M3-rust-security.json)).

## Context

The approved M3 image contains `cargo-nextest` 0.9.143, but its first real run
failed with exit 101 before JUnit publication. The bounded stderr identified
Tokio 1.53.1 `signal/unix.rs:71`: signal initialization calls
`mio::net::UnixStream::pair()` and panics when the syscall is denied.

The pinned local sources show the complete chain. Tokio
`src/signal/unix.rs:71` calls `UnixStream::pair`; Mio 1.2.1
`src/sys/unix/uds/stream.rs:23-24` selects `SOCK_STREAM`; and Mio
`src/sys/unix/uds/mod.rs:101-104` adds `SOCK_NONBLOCK | SOCK_CLOEXEC` and calls
`socketpair(AF_UNIX, flags, 0, ...)`. The M1 profile permits only
`socketpair(AF_UNIX, SOCK_SEQPACKET, 0)`, comparing the low four type bits and
therefore deliberately ignoring those two creation flags.

## Decision

Add `seccomp-rust-quality.json`. Its bytes are the complete unchanged M1
`seccomp-rust.json` rule sequence followed by exactly one rule:

```text
socketpair(
  arg0 == AF_UNIX (1),
  arg1 & 0x0f == SOCK_STREAM (1),
  arg2 == 0
)
```

The profile does not allow `socket`, `AF_INET`, `AF_INET6`, `AF_NETLINK`,
`bind`, `connect`, `listen` or `accept`. Docker `--network=none`, the read-only
root filesystem, capability drop, namespace settings and all other containment
settings remain unchanged.

Only `Phase::Run(RustCommand::TestNextest(_))` selects the quality profile in
M3-01. Ingest, fixed-path JUnit export, M1 commands and all M2 commands retain
their existing profiles byte-for-byte. Any later quality plugin must justify
and record its profile selection independently. Both base and quality profile
bytes are included in the Rust gateway configuration fingerprint.

The selected profile SHA-256 is
`c288305c9fdba791926a9154fe47a1f53a30d61501cd9d059a78467866ccf938`.
The unchanged M1 profile SHA-256 is
`f9d31acb22989dc6ac37c02d4c73acfbbb3b74b5e08beff9983f3a811fd4e56d`.

## Alternatives considered

- Allow general Unix sockets: rejected; Nextest needs an anonymous stream pair,
  not pathname sockets or server operations.
- Reuse the M2 cargo-fix profile: rejected; it enables IPv4 loopback socket and
  connection operations that Nextest does not require.
- Disable Tokio signal handling or patch the guest binary: rejected; it would
  diverge from the provisioned immutable binary and invalidate its provenance.
- Weaken or disable seccomp: rejected; this would violate the execution gateway
  containment contract.

## Consequences

Nextest can initialize Tokio's local wakeup channel while arbitrary project test
code remains unable to create IPv4, IPv6 or pathname Unix sockets. Runtime
qualification includes positive nextest execution and negative AF_INET, AF_INET6,
connect and Unix bind controls. Because the shared Rust gateway verifier and
configuration fingerprint changed, both the complete M3 runtime gate and the
existing twenty-case Rust security gate were rerun and passed. Their receipts are
`docs/validation/M3-01-runtime.json` and
`docs/validation/M3-01-rust-security.json`.

Any further denied syscall is a new security decision. It must stop this
qualification and return to the orchestrator with the exact syscall and source
evidence.

## Sources

- `/Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.53.1/src/signal/unix.rs:71`
- `/Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/mio-1.2.1/src/sys/unix/uds/stream.rs:23`
- `/Users/cburgosro/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/mio-1.2.1/src/sys/unix/uds/mod.rs:101`
- `crates/execution-adapter/src/seccomp-rust.json`
- `crates/execution-adapter/src/seccomp-rust-quality.json`
- `docs/adr/ADR-056-cargo-fix-isolated-loopback.md`
- `docs/validation/M3-01-runtime-attempt1.json`
