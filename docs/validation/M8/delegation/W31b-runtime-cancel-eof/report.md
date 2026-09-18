# W31b — Inspector bridge: cancel + EOF, a full `runtime` session

## Re-diagnosis: the delegated premise was wrong

The prompt-header's diagnosis (`client.cancelToolCall()` doesn't exist, so
the SDK sends an unrecognized method) does not hold. The vendored bundle
(`target/m1-17-inspector/.../inspector/clients/cli/build/index.js`) defines
`InspectorClient.cancelToolCall()` as a real, documented public method: it
aborts the call's own internal `AbortController`, which (on stdio) the SDK's
own request path turns into a `notifications/cancelled` client notification —
exactly the mechanism the prompt-header asked for by another name. There is
no way to inject an external `AbortController`/`signal` into this bridge's
`callTool()`; it manages its own.

I re-ran `run_inspector(..., M8.RUNTIME, ...)` directly (bypassing `--run
--with-runtime`, per the scope restriction) against the real Docker socket
four times (`attempt-18` through `attempt-21`, since discarded) with granular
`console.error` tracing added temporarily. Every single one crashed with the
reported `ProtocolError: Unknown method (-32601)` **before ever reaching the
cancel code** — during `client.readResource()` on the published
`rust-artifact://` URI, immediately after `rust.check` itself passed. The
75-byte error response was byte-identical (same sha256) across every attempt:
a deterministic `MethodNotFound` for `resources/read` itself, not a race.

This is the *exact* failure W31's own report already diagnosed and fixed by
rebuilding: `target/release/rust-engineering-mcp` (mtime 2026-09-14 21:02)
predates commit `be0ed21` (2026-09-14 21:55:32, "fix(m8): resources/list,
resources/templates/list and prompts/list carry ttlMs/cacheScope"), which
touched `stdio.rs`/`capability_document.rs`/`resources.rs` — the exact
resource-dispatch files. The binary regressed to stale again after W31's
session ended (no Rust source changed since `be0ed21`). I could not rebuild
it myself: `cargo build --release --bin rust-engineering-mcp` requires
interactive approval this non-interactive worker session never received (I
did not retry it in a loop, per instructions). **Action for the
orchestrator: rebuild the release binary from current `HEAD` before the next
`--run --with-runtime` matrix**, or `resources/read` will keep failing for a
reason outside these three files' scope.

## What I actually fixed (all in the permitted files)

To verify the cancel/EOF logic for real despite the stale binary, I ran a
**scratch-only** copy of the session script (`target/scratch-w31b/`, never
part of the diff, deleted afterward) with `resources/read` stubbed out, and
iterated against the live server + Docker socket until the real fix below
was solid. `target/release/rust-engineering-mcp` itself was never modified.

### `scripts/m8-inspector-session.mjs`

1. **Cancel (G4), delay before abort.** `cancelToolCall()` was already
   correct, but the original code called it *synchronously in the same tick*
   as `client.callTool(...)`. Tracing the vendored SDK's `Client.request()`
   path shows the whole chain from `callTool()` down to
   `_transport.send()` is unbroken by any `await` — so an abort fired before
   the event loop yields can beat the request off the process entirely: no
   `tools/call` and no `notifications/cancelled` ever reach the wire, and
   there is nothing for the proxy to observe (this is what every one of my
   scratch runs against the live server initially showed: cancel "worked"
   locally, but `protocol.jsonl` carried zero evidence of it). Added a
   `300ms` wait between issuing the call and cancelling it, so the request
   is genuinely in flight first.
2. **Cancel, retry with backoff.** The first real run against the live
   Docker socket exposed a second, real bug the delegated diagnosis never
   mentioned: an immediate retry right after the local rejection lands on
   `blocked`/`SANDBOX_DENIED`
   (`crates/mcp-server/src/stdio/check.rs`'s `ExecutionError::Busy` mapping)
   — the local promise rejects the instant the abort fires, well before the
   server has actually finished tearing down the cancelled container's
   sandbox slot. Changed the retry into a bounded poll (up to 10s, 500ms
   between attempts) so the oracle asks "did cleanup join within a
   reasonable bound", not "was it already done the instant the local promise
   settled".
3. **EOF mid-call (G5), new.** After the cancel-then-retry oracle: fire a
   third `rust.check` (not awaited), read `client.baseTransport.pid` for the
   report, then call `client.baseTransport.close()` **without awaiting it**
   — the transport's own `close()` starts with `stdin.end()`, i.e. exactly
   the EOF an abruptly-vanishing client leaves behind. Immediately (no wait
   for the old process to exit) construct a brand-new `InspectorClient`
   against the same `serverConfig` (same argv, same `--state-root`), connect
   it, and require `tools/list` to return the full inventory — proof the
   server is still healthy and accepting fresh connections, never proof
   about the torn-down connection itself. Both clients are disconnected
   before the script's own exit.

### `scripts/test-m8-clients.py`

- `runtime_cancellation_wire_confirmed(observation)`: new — scans
  `protocol.jsonl` for a client-direction `notifications/cancelled` row from
  the `inspector` client. `run_inspector()` now raises if a `runtime`
  session's own cancel oracle never produced one — the delegated ask's "make
  sure the proxy records it" requirement; the existing proxy already logs
  any dict message's `method`, notification or request alike, so no proxy
  change was needed.
- `assert_no_orphan_server(state, timeout=10.0)`: new — polls
  `pgrep -f <state-root>` (a unique-per-attempt path, safe as a `pgrep -f`
  needle) until no match or the bound elapses; `FileNotFoundError` (no
  `pgrep`) is treated as "cannot check", not a failure. `run_inspector()`
  calls it after a `runtime` session completes and raises if anything still
  matches — the "or at least no orphan process" half of the EOF ask.
- `run_inspector()`'s `mode == RUNTIME` branch now also requires
  `outcome["eof_new_session_ok"] is True`, folding `eof_prior_pid` into the
  report for evidence.
- Module docstring updated to describe the cancel-wire-confirmation and
  EOF/orphan-check additions under `--run --with-runtime`.

### `scripts/test-m8-clients-unit.py`

Added, all passing: `RuntimeCancellationWireConfirmationTests` (4 cases:
confirmed, not confirmed, missing observation file, a server-direction
notification doesn't count), `OrphanServerCheckTests` (no match, a match is
reported, missing `pgrep` binary is treated as unable-to-check, not a
failure), and three new cases on `RunInspectorModeSeparationTests`: a
`runtime` session whose cancellation never reached the wire is refused, one
with no fresh post-EOF session is refused, one that leaves an orphan process
is refused. Extended `_run_to_completion`'s fake outcome with
`eof_new_session_ok`/`eof_prior_pid` so the two pre-existing tests
(`test_a_completed_runtime_session_reports_no_negative_evidence` and the two
"still ran a negative row" tests) keep passing against the new validation.

## Verification

- **Full unit suite**: `python3 -B scripts/test-m8-clients-unit.py` →
  **116/116 passed** (106 pre-existing + 10 new), clean output.
- **`py_compile`** on both Python files, clean.
- **`--preflight`** (no `--with-runtime`) still runs to completion.
- **Bounded real `runtime` Inspector session**, Inspector + real Docker
  socket (`/Users/cburgosro/.docker/run/docker.sock`), driven directly via
  `run_inspector(..., M8.RUNTIME, ...)` (never `--run --with-runtime`), using
  the **scratch** session copy described above (`resources/read` stubbed to
  isolate the cancel/EOF logic from the unrelated stale-binary failure; the
  three permitted files' own diff never stubs it — the shipped script still
  requires a real Resource read, as it did before):
  - `server/discover` negotiated 2026-07-28, `tasks_advertised: true`;
    `tools/list` returned all 36 tools; `resources/list` empty.
  - `rust.project.open` → passed; first `rust.check` → passed.
  - **G4 cancel**: second `rust.check` sent, then a genuine
    `notifications/cancelled` client row on the wire (confirmed via
    `protocol.jsonl`, not merely inferred), the local call rejected. The
    immediate retry landed `blocked`/`SANDBOX_DENIED` once (the container
    teardown race described above, live and reproduced), then the polled
    retry landed `passed` on the next attempt — `cancel_ok: true`,
    `cancellation_wire_confirmed: true`.
  - **G5 EOF**: a third `rust.check` sent and abandoned, the transport
    closed without waiting (server's own response to that abandoned call
    later arrived tagged `status: "cancelled"`, observed on the wire — the
    server itself detected the EOF). A brand-new session against the same
    server argv negotiated cleanly and listed all 36 tools right after —
    `eof_new_session_ok: true`. No `rust-engineering-mcp` process remained
    for that session's `--state-root` — `eof_no_orphan_process: true`.
  - Full session `exit_code: 0`, no crash, no uncaught exception.
  - Debug artifacts from this investigation (`attempt-18`..`attempt-23`
    under `docs/validation/M8/clients/`, `target/scratch-w31b/`) were
    deleted; `attempt-17` (pre-existing, referenced by the prompt-header) was
    left untouched.

## Scope notes

- Did not touch `target/release/rust-engineering-mcp` or any Rust source —
  outside the three permitted files. The resources/read staleness is a
  build-freshness issue for the orchestrator to rebuild before the next full
  matrix, not a defect in this fix.
- No commit made, per instructions.
