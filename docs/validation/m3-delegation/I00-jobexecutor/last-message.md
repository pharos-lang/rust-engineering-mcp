## Task

I00 — ADR-060 core interfaces and dormant MCP task plumbing on `ai/m3-quality`.

## Result

Implemented:

- Neutral domain job identifiers, lifecycle, phases, budgets, deadlines, owner binding, completion, execution modes, TTL and retention quotas.
- Owner-bound `JobExecutor`, in-memory registry, watchdog, delivery tracking, cancellation/join semantics, authority revalidation and the shared ports required by I01/I17.
- Stable `ProjectNextestPort`, validated nextest options and bounded typed observations/artifact references.
- Dormant `tasks/get`, `tasks/cancel`, and `tasks/update` handlers using rmcp 3.2.0 signatures.
- `rust.test.nextest` closed DTO/schema skeleton with `execution_mode`, 512-KiB response bound and conservative outcome mapping.
- ADR-030 worker-permit reuse and delivery marking only after successful send completion.
- Watchdog and `shutdown_and_join` lifecycle plumbing.
- Tasks capability and the new tool remain deliberately unadvertised/unregistered.
- No commit created.

No rmcp API gap was found.

## Files changed

- [domain/src/job.rs](/Users/cburgosro/Projects/rust-mcp/crates/domain/src/job.rs) — `31c880e0f9212439f943e4fa6c8a222b7e917f79acbe59b371d9a875741e8a96`
- [domain/src/lib.rs](/Users/cburgosro/Projects/rust-mcp/crates/domain/src/lib.rs) — `a694c219a7dbf41842e9b81a516195b5dbda0e3bf0d56d95b92dbc1400c8aa9b`
- [domain/tests/job.rs](/Users/cburgosro/Projects/rust-mcp/crates/domain/tests/job.rs) — `74a82866bf86cc459165164c209d9082bf395e1e88aa26b6cc9a812d2b176b18`
- [application/src/job.rs](/Users/cburgosro/Projects/rust-mcp/crates/application/src/job.rs) — `d0422c2c857b3a87e801c9fb8730005f4a392e7e8b1d95448a42b099d44248b9`
- [application/src/nextest.rs](/Users/cburgosro/Projects/rust-mcp/crates/application/src/nextest.rs) — `4d4208943164186490ce31bd148c3dec8dc3c85b838f636f6ce8606681759084`
- [application/src/lib.rs](/Users/cburgosro/Projects/rust-mcp/crates/application/src/lib.rs) — `d0cdbc0d3272434219f42823d5c04c0277557934d053e7b2bd7a6f13a269e999`
- [application/tests/job.rs](/Users/cburgosro/Projects/rust-mcp/crates/application/tests/job.rs) — `efc00248bd6ad234c98376b23620cf471ef444c4ed2d5cb796d76d9b0de91b3c`
- [stdio/tasks.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/tasks.rs) — `920d818373b163d2db5d6130a03b811d9edb1b5fea050b257a38106f5bd19614`
- [stdio/tasks/tests.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/tasks/tests.rs) — `bafe3ae8d1871196ffeffbda5d829bc21bbc51c2bee3bc73c6b0353fe1942d48`
- [stdio/nextest.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/nextest.rs) — `28823dbf60fc107ce5c995cf1dcb20853515a62218e08e2b9a37aeda363fb382`
- [stdio.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio.rs) — `e1707c3af1dfd28b88005aea33cf23379fcb2365d40239be7cf0d8c3b4a1a6b2`
- [stdio/admission.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/admission.rs) — `7ce91a8a90f6e8d01ec4708c0f30bd36409b4bebc787944a96cbdd20928ccadd`
- [stdio/workers.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/workers.rs) — `fbf39b185df5d3eff050b4c744397d85ba26265e9e22dcc351c798b0237a9a90`
- [protocol.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/protocol.rs) — `af79e75c4fd27bee68f5c8a6960f55fc3dfe98cf8b0b7d6d9b904caaaf8b6aed`

The shared `lib.rs` hashes include concurrent workers’ module declarations; I00 only added its permitted `pub mod` lines.

## Tests executed

- `cargo check -p rust-engineering-domain --all-targets --locked --offline` — exit 0.
- `cargo test -p rust-engineering-domain --locked --offline` — exit 0; 62 passed.
- `cargo clippy -p rust-engineering-domain --test job --locked --offline -- -D warnings` — exit 0.
- `cargo clippy -p rust-engineering-domain --all-targets --locked --offline -- -D warnings` — exit 101; blocked by five I01-owned lints in `domain/src/nextest.rs` and `domain/tests/nextest.rs`.
- `cargo check -p rust-engineering-application --all-targets --locked --offline` — exit 0.
- `cargo clippy -p rust-engineering-application --all-targets --locked --offline -- -D warnings` — exit 0.
- `cargo test -p rust-engineering-application --locked --offline` — exit 0; 140 passed.
- `cargo check -p rust-engineering-mcp --all-targets --locked --offline` — exit 0.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings` — exit 0.
- `cargo test -p rust-engineering-mcp --locked --offline` — exit 101; 194 passed, five existing `catalog_sync` tests failed because the sandbox denied local socket creation, three ignored.
- Supplemental `cargo test -q -p rust-engineering-mcp --locked --offline -- --skip catalog_sync::tests` — exit 0; 273 passed, 32 ignored, six filtered.
- Full protocol suite — exit 0; 39 passed.
- Nextest contract tests — exit 0; four passed.
- Task projection tests — exit 0; six passed.
- `python3 -B scripts/check-architecture.py` — exit 0.
- `git diff --check` — exit 0.
- Existing snapshot directory diff — empty.

## Evidence

The new tests cover D06-T03, T05, T06, T07, T09, T11, T12 and T14. Existing admission regression tests cover T13. The protocol test proves `tasks/get` remains method-not-found across all five supported versions while capability advertisement is gated.

The schema fingerprint is fixed at `fa15c0fe20d34fb4f0d9e3d769931998ceade709c07719938f2285af595c53c9`.

## Risks

- This is intentionally dormant infrastructure: no production executor instance exists yet, and neither Tasks nor `rust.test.nextest` is advertised.
- The exact full MCP test command requires an environment that permits its local TLS/socket fixtures.
- The repository-wide domain Clippy gate remains red solely in concurrent I01-owned nextest tests.

## Decisions

- `JobKind` contains only `TestNextest`; unimplemented future kinds were not made constructible.
- Terminal publication and permit release require independent cleanup confirmation from `JobSignal`.
- Raw artifact streams are erased from retained task results after publication; only bounded typed facts and artifact references remain.
- Overall duration belongs to the registry-safe `NextestTaskResult`, preserving the already-consumed I01 port signature.
- Capability advertisement remains hard-gated pending G4.

## Open issues / specialist needs

- I01 must remove two `expect` uses and constant assertions from each of its domain nextest test areas to clear the domain all-target Clippy gate.
- The integration package must supply concrete clock, OS-random ID, signal, authority and executor construction; connect I01/I17 publication; register `rust.test.nextest`; and only then run G4 and flip Tasks advertisement.
- Rerun the exact MCP package test in a host environment permitting the five local `catalog_sync` socket fixtures.