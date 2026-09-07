# ADR-065 — Dedicated persistent coverage target volume

## Status

Accepted 2026-09-06 by the M3 orchestrator, as amended after W1 proved that the
pinned `cargo-llvm-cov` report command must write its profraw list and merged
profdata in the configured target directory. The independent security and final
reviews judged the amended shape contained: the target is a per-job tmpfs absent
from every exporter and every non-coverage phase, read-only for the keeper, and
destroyed at cleanup. The mount matrix is pinned by literal per-phase
expectations and negative mutations ([V-SEC](../validation/m3-delegation/V-SEC/last-message.md),
[final review](../validation/m3-delegation/VF-opus-final/last-message.md)). The
approved guest image passed the M3 runtime gate at 62/62 and the Rust security
gate at 20/20 ([runtime receipt](../validation/M3-runtime.json),
[Rust security receipt](../validation/M3-rust-security.json)).

## Context

`cargo llvm-cov --no-report` compiles and runs instrumented tests before a later
`cargo llvm-cov report` invocation consumes the binaries and `.profraw` data.
The execution gateway deliberately uses separate, single-command containers for
those phases. Q01 first placed `CARGO_LLVM_COV_TARGET_DIR` under the persistent
report volume, but that volume has the existing M3 artifact profile
`size=64m,nr_inodes=8192,uid=65534,gid=65534,mode=0700,nosuid,nodev,noexec`.
Compilation succeeded and execution then failed closed with exit 101:

```text
could not execute process /work/coverage/target/debug/deps/...
Permission denied (os error 13)
```

Executing project tests inside the sandbox is already the qualified behavior of
R2 operations under ADR-037. ADR-056 likewise grants `cargo fix` a bounded,
executable `/target` tmpfs inside one container. Coverage needs the same class of
scratch area to persist across multiple containers belonging to one job; it does
not need a new syscall, network, user, capability or host-filesystem privilege.

## Decision

Each `rust.coverage` job creates a third named local-driver tmpfs volume, named
from the job nonce and labeled with the existing closed ownership labels. It is
mounted at `/work/coverage-target` and has exactly:

```text
size=512m,nr_inodes=65536,uid=65534,gid=65534,mode=0700,nosuid,nodev
```

The 512 MiB byte ceiling matches the already-qualified executable `/work` build
ceiling. It is a ceiling, not a reservation, and bounds the instrumented binary,
incremental metadata and profraw set. The 65,536 inode ceiling preserves the
existing report profile's density of 128 inodes/MiB. W2 records the fixture
outputs and timings; a larger real project must fail within this bound rather
than silently receive more storage.

The access matrix is closed:

| Phase | `/source` | `/work/coverage` report | `/work/coverage-target` |
| --- | --- | --- | --- |
| source ingest | writable, then removed | absent | absent |
| coverage keeper | read-only | read-write, `noexec` | read-only |
| `CoverageRun` | read-only | read-write, `noexec` | read-write, executable |
| each `CoverageReport` | read-only | read-write, `noexec` | read-write, executable |
| JSON/LCOV/HTML export | read-only | read-only, `noexec` | absent |
| every non-coverage phase | unchanged | absent except its own existing artifact volume | absent |

The keeper executes only fixed `/usr/bin/sleep`; it holds both local tmpfs mounts
across the gap between single-command containers and cannot write the target.
`CoverageRun` and the three closed `CoverageReport` invocations can write it.
The pinned plugin's report command first materializes
`<crate>-profraw-list` and `<crate>.profdata` in `CARGO_LLVM_COV_TARGET_DIR`;
its fixed help exposes no redirect for that merge. The directory already contains
only this job's instrumented build, is unreachable from the host, and is destroyed
at cleanup, so report-time writes add no reachable surface.
`CARGO_LLVM_COV_TARGET_DIR` is fixed to `/work/coverage-target` for run/report only.

All coverage containers retain the ADR-064 quality seccomp profile,
`--network=none`, non-root uid/gid 65534, read-only rootfs, dropped capabilities,
and existing PID/CPU/memory/tmpfs limits. Source, report, `/tmp`, `/work`, and all
other tools' mounts are unchanged. The ordinary report volume remains `noexec`.

Creation, ownership inspection, applied-container verification and cleanup cover
the new volume. Cleanup joins/removes every container before removing source,
report and target volumes; any uncertain removal quarantines the gateway. The
configuration fingerprint includes the full option string and every phase-specific
mount argv.

Runtime negative controls prove that a planted source binary and a binary
written to the report volume cannot execute, network remains denied throughout
the coverage phases, the keeper remains read-only, exporters cannot see the
target, no non-coverage phase receives it, and no owned volume survives cleanup.
The applied-mount test uses a literal table for keeper, run, all three report and
all three export phases; it does not derive its expectation from the production
phase helper being tested.

## Alternatives considered

- **Three independent run-and-report invocations:** rejected because JSON, LCOV
  and HTML would no longer derive from one profdata set and could disagree.
- **Rebuild in each report container:** rejected for the same accounting reason,
  and because it executes project build code repeatedly.
- **Shell or multi-command entrypoint:** rejected; the gateway permits only fixed
  executable/argv pairs and never introduces `sh -c`.
- **Make the shared report profile executable:** rejected; artifacts do not need
  execution and every quality tool would inherit unnecessary privilege.
- **Reuse `/work` across containers:** impossible because it is intentionally a
  container-local tmpfs and disappears with the run container.
- **Copy binaries/profraw through the report volume:** rejected because it would
  make the artifact channel executable or require a new trusted copier/archive
  protocol with no smaller effective privilege than the dedicated volume.

## Consequences

Coverage gets one narrowly scoped persistent executable area. Its lifetime is one
job, its capacity and inode count are bounded, only the unprivileged run and three
closed report phases can write it, the keeper can only read it, and exporters
cannot see it. Report writes are limited to merge intermediates in the already
private target and disappear with that target. The security delta remains
auditable in one option string and one phase mapping: the dedicated target omits
`noexec`; no existing profile changes bytes. Its binaries, profraw, profdata and
merge intermediates are arbitrary project-controlled content. Containment never
trusts those bytes: only fixed coverage phases can reach the volume, under the
unchanged sandbox boundary, and joined cleanup destroys it.

The gateway configuration fingerprint changed. W3 reran and passed the complete
55-selection M3 runtime and 20-selection Rust security gates on the reviewed bytes.
A future tool cannot
reuse this volume without a new decision and verifier mapping.

## Sources

- `docs/validation/M3-runtime-attempt4.json`
- `docs/validation/M3-runtime-attempt5.json` (Q02 environment-blocked attempt)
- `docs/validation/M3-runtime-attempt6.json` (current Q02 implementation, same block)
- `docs/validation/M3-runtime-attempt7.json` (W1: run succeeds; three report merges fail read-only)
- `target/m3-runtime-q01-attempt4/47.log`
- `target/m3-runtime-w1/47.log`
- `docs/validation/M3-runtime.json` (historical W3, 55/55 after V-SEC; superseded
  by the current W6 receipt at 62/62)
- `docs/validation/M3-rust-security.json` (W3, 20/20 after V-SEC)
- `docs/validation/M3-03.md` (observed counts, formats and containment controls)
- `docs/adr/ADR-037-test-execution.md`
- `docs/adr/ADR-056-cargo-fix-isolated-loopback.md`
- `docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md`
- `docs/adr/ADR-064-quality-job-seccomp-profile.md`
- `crates/execution-adapter/src/{coverage_gateway,rust_gateway,rust_applied,mutation_gateway}.rs`
