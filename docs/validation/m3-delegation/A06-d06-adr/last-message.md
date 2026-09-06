## Task

A06 — decidir D06 mediante ADR Proposed y spike wire de `rmcp 3.2.0`.

## Result

Completado sin commits ni cambios fuera de los dos archivos autorizados. ADR-060 estaba libre.

## Files changed

- [ADR-060-bounded-job-execution-and-mcp-tasks.md](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md)  
  SHA-256: `baed49270c3ce0d7a802f24588094c0e797bdadfd0698f87b3bccd2346c6db2d`
- [rmcp_tasks_spike.rs](/Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/rmcp_tasks_spike.rs)  
  SHA-256: `babbb187da18951734bcabc4b1e3a4756d9766d27bff8cf194d1995fa83d3ec2`

## Tests executed

- `rustfmt --edition 2024 crates/mcp-server/tests/rmcp_tasks_spike.rs && rustfmt --edition 2024 --check crates/mcp-server/tests/rmcp_tasks_spike.rs`  
  Exit `0`; 1 file.
- `cargo test -p rust-engineering-mcp --test rmcp_tasks_spike --locked --offline`  
  Exit `0`; 5 passed, 0 failed, 0 ignored.
- `cargo clippy -p rust-engineering-mcp --all-targets --locked --offline -- -D warnings`  
  Exit `0`; 0 warnings/errors.
- Trailing-whitespace validation over both deliverables  
  Exit `0`; 2 files, 0 findings.

## Evidence

The spike proves:

- Exact Tasks capability JSON across all five negotiated versions.
- `tasks/get` without client Tasks capability returns `-32021`.
- `CreateTaskResult` can be polled to the flattened terminal result.
- Cancellation acknowledges intent while status remains `working`, becoming `cancelled` only after cooperative completion.
- `TaskOptions` with no TTL emits `"ttlMs": null`.
- A `2024-11-05` peer declaring the extension can receive a task.
- Every wire operation and polling loop is bounded by deterministic two-second timeouts.

## Risks

- Every new quota and timeout in ADR-060 is explicitly provisional pending M3-01/M3-02 measurements.
- Legacy wire compatibility is demonstrated for rmcp, not guaranteed for every legacy client.
- `notifications/tasks` exists in rmcp but is not routable through its subscription API; the decision therefore uses polling.
- Crash recovery still depends on containment reconciliation; jobs are intentionally not resumed.

## Decisions

- Implement an application-neutral `JobExecutor` and owner-bound registry; do not use `rmcp::TaskManager` as product lifecycle authority.
- Advertise Tasks on all five supported versions only after product qualification, while requiring the client declaration for actual task use.
- New M3 inputs use `execution_mode: auto|task|synchronous`; existing 18 schemas remain unchanged.
- One active job, no queue; polling/cancellation never acquire the job permit.
- Fixed, non-null task TTL; uniform masking for unknown, foreign, expired, or revoked task IDs.
- Cancellation, timeout, expiry, and EOF must terminate and join cleanup before capacity is released.

## Open issues

No blocking owner conflict was found. ADR acceptance still requires orchestrator approval, independent review, and M3-01/M3-02 measurement evidence before provisional budgets become final.