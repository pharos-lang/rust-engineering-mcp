## Task

Completed I06’s non-Docker M3-02 implementation: negotiated asynchronous Tasks execution, D06 product tests, capability gating, validator harnesses, and documentation.

## Result

Implemented, with advertisement intentionally gated:

- All four M3 quality tools can return `CreateTaskResult` after mutual Tasks negotiation.
- Jobs use the existing ADR-030 permit and a registry-owned cancellation token.
- `tasks/get` returns the tool’s bounded terminal result.
- `tasks/cancel` becomes terminal only after observed cleanup.
- `tasks/update` remains fixed-deny.
- Owner/grant/expiry masking remains uniform.
- Async budgets derive from the validated request through the 3,600-second ceiling.
- `TASKS_ADVERTISEMENT_READY` remains `false` until G4 evidence exists.
- All 23 tool snapshots remain byte-identical.

The complete file list and SHA-256 manifest are in [last-message.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/m3-delegation/I06-tasks-implementation/last-message.md), SHA-256 `2d593c5f023659698f2b9067f2c18da7f52e2c4de58bf76bb53a7f7e17a79990`.

Key implementation files:

- [job.rs](/Users/cburgosro/Projects/rust-mcp/crates/domain/src/job.rs)
- [application job.rs](/Users/cburgosro/Projects/rust-mcp/crates/application/src/job.rs)
- [stdio.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio.rs)
- [tasks.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/tasks.rs)
- [workers.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/workers.rs)
- [test-m3-clients.py](/Users/cburgosro/Projects/rust-mcp/scripts/test-m3-clients.py)
- [test-m3-budgets.py](/Users/cburgosro/Projects/rust-mcp/scripts/test-m3-budgets.py)
- [M3-02.md](/Users/cburgosro/Projects/rust-mcp/docs/validation/M3-02.md)

## Tests executed

- `cargo fmt --all -- --check` — passed.
- `cargo check --workspace --all-targets --locked --offline` — passed.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` — passed.
- `cargo test -p rust-engineering-mcp --locked --offline tasks` — 17 selected assertions passed.
- MCP protocol tests — 43 passed.
- rmcp Tasks tests — 5 passed.
- Domain job tests — 4 passed.
- Architecture gate — passed.
- Both new harness self-checks — passed; no receipts created.
- `cargo test --workspace --locked --offline` — reached the MCP suite but exited nonzero because five existing `catalog_sync` tests cannot create local sockets in this managed sandbox (`EPERM`). MCP summary: 224 passed, 5 failed, 3 ignored. No product regression was observed.
- No Docker command was executed.

## D06 status

| Cases | Status |
|---|---|
| T01–T03 | Non-Docker complete |
| T04 | Active-child revocation pending Docker |
| T05 | Live cancellation/cleanup pending Docker |
| T06 | Request-token separation and commit race complete; real non-delivery pending |
| T07 | Permit, saturation and responsive controls complete; live contention repeat pending |
| T08 | Hostile-child EOF/quarantine pending Docker |
| T09 | Deadlines, task-control budgets and non-renewing TTL complete |
| T10 | Restart/residual cleanup pending Docker |
| T11–T14 | Non-Docker complete; live trace/suppression repetitions pending |

## Decisions and risks

- Quality results cross the application boundary as bounded, protocol-neutral JSON objects; rmcp types remain outside domain/application.
- Failed task admission is forced through the existing Busy path and cannot fall through into synchronous execution.
- The client harness drives Inspector 2.5.0 and Codex 0.153.0 app-server, records each peer’s actual Tasks declaration, requires poll/cancel when declared, and performs a durable quality Resource read.
- The sprint workflow’s evidence-first implementation and QA structure was used for the delivery.
- Client behavior and 30/30 performance samples remain unmeasured; the corresponding receipts are deliberately absent.

Advertisement may flip only after the validator completes Docker T04/T05/T08/T10, live T06/T07/T13, the 30 cold/30 warm measurement, and successful Inspector/Codex G4 qualification.