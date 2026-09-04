# M1-01 prerequisite — workers and MCP admission

This unit prepares M1-01; it does not implement project.inspect, enable Cargo or
close M1-01. M0 remains closed and its reports remain historical.

## Checkout and environment

Started on clean main `0d6462f`, branch `ai/m1-01-workers`. Read AGENTS, complete
specification v0.3.1, implementation status, M0-12, M1 prerequisites, CI and
relevant ADRs. Actual host Rust/Cargo1.98.1 and Claude Code2.1.259 were checked.
Docker socket, pinned E5 directory and ORT directory remain present. No model,
runtime hash or toolchain substitution. The Docker images initially present were
the M0 Go probe and Terraform; neither provided Cargo. Explicit owner permission
subsequently authorized separate Rust Linux provisioning.

## Behavior and evidence

ADR-030 centralizes project.open's worker, request/session/drop cancellation,
monotonic deadline, retained admission until real closure completion and bounded
shutdown drain. The gateway is not connected and no project code was run.

Transport wraps SDK messages rather than parsing RPC. Separate request,
notification and send capacities are16. A request lease covers the SDK response
queue and transfers into the actual send future; completion cannot release a
newer request reusing that ID. Suppressed cancelled responses retain tombstones
until teardown. Exhausting capacity or duplicating a still-pending ID closes the
session. Reconnection requires reopening process-local project references.

Partial input and output/flush deadlines are10s total; idle input is unrestricted.
Both frame byte caps are1MiB. This limits wire content, not transient serializer
allocation or native RSS. Domain/application and the tool schema remain unchanged.

Tests cover cancelled/aborted/deadlined workers retaining their slot, session
drain success/failure, admission before execution, SDK cancellation during cleanup,
real SDK response suppression, cancelled tombstones, separate notification
capacity, pending/unpolled sends, stale response completion after ID reuse, frame
limits and stalled reads/writes/flushes. Wire tests exercise the real binary's
10s partial-frame deadline and64 sequential request-ID reuses, plus the existing
modern/four-legacy protocol suite.

## Gate

`python3 scripts/gate.py core --report target/M1-01-workers-core.json` passed
all10 stages on the final worker/admission implementation. Rust/Cargo1.98.1,
locked/offline, CARGO_INCREMENTAL=0. 200 normal Rust tests plus1 doctest passed;
Docker tests remained excluded from this proportional core gate. The Cargo corpus
and audit/deny passed; paste1.0.15 maintenance warning remains visible.

- [Stage report](artifacts/M1-01-workers-core.json).
- [Compact results](artifacts/M1-01-workers-results.txt).
- [Code and schema receipt](artifacts/M1-01-workers-source-receipt.json).

Independent [Opus5 High review](../reviews/M1-01-workers-claude-opus-5.md)
completed; principal disposition resolved the applicable findings. The final gate
also covers batch rejection in all modes, first-call recovery, partial output
timeouts and ID reuse after bootstrap (including error responses).
Commit `dc53c72`, local no-ff merge `50720a4`; branch retained.
Post-merge smoke: the real executable protocol suite passed23/23 tests,
Rust/Cargo1.98.1, locked/offline and CARGO_INCREMENTAL=0.
M1-01 itself remains In progress because no Cargo/source-transfer tool is enabled.
