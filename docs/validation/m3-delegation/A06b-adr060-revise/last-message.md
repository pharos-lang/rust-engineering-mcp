## Task

A06b — revisar ADR-060 y el spike según V06 y las disposiciones F01–F14.

## Result

Revisión completada. Solo se modificaron los dos archivos autorizados. ADR-061 conserva su SHA-256 original.

## Files changed

- [ADR-060-bounded-job-execution-and-mcp-tasks.md](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md)  
  SHA-256: `5355954b4c8fe17b6181470512c3cb09c27b32341f54ac2ff092f44c893143b2`
- [rmcp_tasks_spike.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/rmcp_tasks_spike.rs)  
  SHA-256: `76e5b3981c7fc9cd7dad88f8080ed4c89cd588bd4a72a37f4c79a36c39a64446`

## Tests executed

- `cargo test -p rust-engineering-mcp --test rmcp_tasks_spike --locked --offline`  
  Exit `0`: 5 passed, 0 failed, 0 ignored.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings`  
  Exit `0`: 0 warnings/errors.
- `rustfmt --edition 2024 --check crates/mcp-server/tests/rmcp_tasks_spike.rs`  
  Exit `0`: 1 file checked.
- Trailing-whitespace validation  
  Exit `0`: 2 files checked, 0 findings.

## Evidence

| Finding | Resolution |
|---|---|
| F01 | Non-delivery tracking through request ID and `AdmittedTransport`; delivered jobs are not cancelled for missing polls. [ADR lines 245–272](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:245) |
| F02 | Static advertisement is explicitly chosen, G4-gated, with overridable `initialize`/`discover` remediation and stock-client fallback. [ADR lines 186–216](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:186) |
| F03 | Child budgets now compose within 300/3,600 seconds; cleanup is separate and synchronous worst-case return is 360 seconds. [ADR lines 286–317](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:286) |
| F04 | Job permit is the ADR-030 worker permit; tool/Resource busy consequences and registry exemptions are explicit. [ADR lines 104–120](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:104) |
| F05 | Registry-owned cancellation token is seeded only from session shutdown; request token hazard is prohibited and tested. [ADR lines 251–258](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:251) |
| F06 | Container/volume reconciliation is identified as new M3-01 work with label+nonce cleanup and fail-closed quarantine. [ADR lines 340–350](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:340) |
| F07 | Spike covers five versions × both capability declarations for advertisement and task-method gating: 20 matrix cells total. [Spike lines 338–400](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/rmcp_tasks_spike.rs:338) |
| F08 | Working fixtures use 30,000 ms TTL; spike is explicitly an rmcp-pin regression guard, not product evidence. [Spike lines 1–14](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/rmcp_tasks_spike.rs:1) |
| F09 | Watchdog may signal and join only; shutdown ordering is fixed inside the current-thread `block_on`. [ADR lines 128–132](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:128) |
| F10 | `JobOwner` now matches ADR-061’s uid/state-root/granted-root binding; foreign tests are cross-ProjectRef/cross-grant. [ADR lines 59–65](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:59) |
| F11 | Expired artifact members resolve as Resource not found and project as `Unavailable` during task reads. [ADR lines 134–139](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:134) |
| F12 | Canonical job identity remains `job_<32hex>` and quota comparison now uses ADR-061’s actual figures. [ADR lines 54–65](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:54) |
| F13 | Exceed oracles added for seed/task-control deadlines, retained bytes, entry saturation and 512 KiB responses. [ADR lines 384–402](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:384) |
| F14 | Missing capability for explicit task mode is fixed `-32602`; unqualified `auto` uses structured `TASKS_REQUIRED`; busy remains `SANDBOX_DENIED`. [ADR lines 218–241](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md:218) |

## Risks

- All new numeric limits remain proposals pending fixture measurement.
- Static legacy advertisement remains gated on actual Inspector/Codex evidence.
- An asynchronous job intentionally monopolizes the existing worker permit.
- Gateway container reconciliation is new M3-01 work, not current behavior.
- The spike validates pinned SDK behavior only, not D06 product oracles.

## Decisions

- `auto` never chooses synchronous execution for a Tasks-capable peer.
- M3 reuses the existing `ready` bootstrap gate.
- Busy ordering is validation → mode selection → busy rejection → reservation.
- G4 clients are Inspector 2.5.0 and model-driven Codex CLI 0.153.0.
- If neither declares Tasks, the repository harness qualifies Tasks and stock clients qualify synchronous fallback.

## Open issues

No unresolved owner decision was found. Orchestrator acceptance, G4 client receipts, M3-01 reconciliation implementation, and M3-02 budget measurements remain pending.