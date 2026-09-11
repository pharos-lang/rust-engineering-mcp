# M2 D06 Cargo fix qualification

Status: **exact_fixture_qualified_not_accepted**. This is runtime qualification evidence, not an ADR or production implementation.

## Qualified invocation

`/opt/rust/bin/cargo fix --workspace --all-targets --frozen --offline --allow-no-vcs --allow-dirty --allow-staged --message-format=json --color never --target-dir /target`

The initially proposed literal `--default-features` spelling was rejected by Cargo 1.98.1. Default features are selected by omitting both `--no-default-features` and `--all-features`.

The approved cargo 1.98.1 (797e8a9bc 2026-08-05) image failed under the unchanged `seccomp-rust.json` profile because Cargo binds a TCP listener for locking. The positive run used the recorded private experimental profile, `--network=none`, a read-only container root, a bounded writable source tmpfs, and an isolated executable `/target` tmpfs.

## Results

- Positive fixture exit: `0`; changed paths: `['src/lib.rs']`.
- Existing `Cargo.lock` remained `sha256:ce788ff0bd5ddf871cfcc1c2151a210aeb045fc6c4b6422ca50f57bf7b6effee`.
- Missing lock exit: `101`; generated lock: `False`.
- Cancellation exit: `137`; OOM: `False`.
- Control timeout cleanup exit: `137`.
- Final owned-object inventory: `{'containers': [], 'volumes': []}`.

The experimental profile admits AF_INET stream sockets with protocol 0 and `bind`, `connect`, `listen`, `accept4`, `getsockname`, `setsockopt`, and `shutdown`. The runs directly discriminate the initial socket denial and that `setsockopt` and `shutdown` are required. The remaining operations were qualified as a group and were not individually minimized. The result qualifies this exact command and fixture only. It does not prove behavior for dependency-bearing workspaces, build scripts other than the fixed cancellation fixture, proc macros, every compiler diagnostic, or all Docker platforms. `--network=none` records namespace isolation and external-interface removal; it does not deny loopback TCP.

## Official sources

- [Cargo fix command](https://doc.rust-lang.org/nightly/cargo/commands/cargo-fix.html) documents target, feature, VCS, frozen, and offline behavior.
- [Cargo fix implementation at the runtime commit](https://github.com/rust-lang/cargo/blob/797e8a9bca276c1c9f9f738d2a20f484fa4eea9d/src/cargo/ops/fix/mod.rs) shows the TCP lock client and bounded iterative rustc/rustfix execution used by Cargo 1.98.1.
