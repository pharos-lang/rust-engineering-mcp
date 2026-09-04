# M1-16 trusted driver cleanup acknowledgement fix

Date: 2026-09-04. Research-only changes; no production crate changes,
model runs, installs, freeze or milestone completion in this worker task.
The initial fix below had no Docker calls; the parent subsequently granted an
exclusive Docker slot for the follow-up documented at the end.

## Task and result

Preserve gateway failures when cancellation races completion, and distinguish joined
handlers/processes from cleanup certainty. Implemented and locally validated.

`Cancel.cleanup_uncertain` is sticky. Typed `ExecutionError::CleanupUncertain` from
gateway initialization, calibration or execute produces `gateway_cleanup_uncertain`.
Blocking worker JoinError/panic and poisoned gateway state mark uncertainty. A
failed child wait marks uncertainty and leaves `server_joined=false`; an unsuccessful
exit marks uncertainty even though `server_joined=true` correctly records the reap.
Server stderr-worker panic and MCP transport shutdown/request failures are also
conservative. MCP internal errors are conservative because production maps cleanup
uncertainty and worker failures into internal errors, without a distinct public
machine-readable cleanup code. Other MCP error responses retain their previous
retryable representation.

Cancellation only replaces an otherwise successful outcome. It cannot overwrite
an already returned gateway/child/transport failure at either cancellation check.
The error acknowledgement is emitted after teardown attempts, with the observed
atomic flags rather than an unconditional cleanup claim.

## Private acknowledgement contract for broker/runner

Both terminal success (`closed=true`, exit0) and terminal error
(`driver_error`, `success=false`, exit1) objects include all three booleans:

- `execution_joined`: driver completed its awaited request/worker path.
- `server_joined`: the owned MCP server process was successfully waited/reaped;
  false for raw mode. This does not itself imply successful exit or cleanup.
- `cleanup_uncertain`: observed cleanup uncertainty, worker failure or server /
  transport failure. Sticky true is never cleared by later success or cancellation.

Consumers must require explicit `cleanup_uncertain is False`,
`execution_joined is True`, and `server_joined is True` in MCP mode. Missing fields
fail closed. Exit1 is acceptable only for expected cancellation whose final
`driver_error` is exactly `cancelled`; all other error labels remain infrastructure
failure even if external cancellation was requested. Independent per-run Docker
container/volume absence remains a controller responsibility before freeze.

Plain cancellation after verified synchronous gateway cleanup may acknowledge
joined with `cleanup_uncertain=false`. A panicked task may be joined while cleanup
is uncertain; those separate facts must not be conflated.

## Actual production source evidence (read-only)

- `crates/execution-adapter/src/rust_gateway.rs:315`: RustGateway definition.
- `rust_gateway.rs:584-642`: cleanup synchronously removes owned containers and
  volume, checks absence, and quarantines/returns CleanupUncertain on failure.
- `rust_gateway.rs:852-853`: execute_observed calls `self.cleanup(...) ?` before
  `finish_work(work, terminal_signal) ?`. The gateway is not waiting for its Drop
  to tear down a successful/cancelled execution. Review C2's Drop premise is false.
- `crates/mcp-server/src/stdio/check.rs:386-390`: cleanup uncertainty becomes MCP
  internal_error; corresponding mappings exist for other execution tools.

## Files changed

- `target/m1-16-driver/src/main.rs`
- `target/m1-16-driver/src/tests.rs`
- this receipt

## Tests executed and evidence

Rust 1.98.1, CARGO_INCREMENTAL=0, locked/offline, shared target directory preserved.

```text
cargo +1.98.1 fmt --manifest-path target/m1-16-driver/Cargo.toml -- --check
cargo +1.98.1 test --manifest-path target/m1-16-driver/Cargo.toml --locked --offline --target-dir <LOCAL_HOME>/Projects/rust-mcp/target
cargo +1.98.1 clippy --manifest-path target/m1-16-driver/Cargo.toml --locked --offline --target-dir <LOCAL_HOME>/Projects/rust-mcp/target --all-targets -- -D warnings
cargo +1.98.1 build --manifest-path target/m1-16-driver/Cargo.toml --locked --offline --target-dir <LOCAL_HOME>/Projects/rust-mcp/target --bin m1-16-trusted-driver
```

All passed: 11 driver tests and 1 existing research-bundle test. Four new tests
discriminate cancellation plus cleanup/infrastructure error, sticky uncertainty,
clean cancellation, an actual panicked Tokio blocking worker, failed child wait,
unsuccessful child exit and successful child exit.

Three actual executable IPC smoke cases with empty environment and raw initialization
(no gateway creation/execution) passed: explicit close => exit0/closed;
SIGTERM => exit1/cancelled; stdin EOF => exit1/cancelled. Each terminal ack carried
execution_joined=true, server_joined=false, cleanup_uncertain=false. All processes
were waited with a five-second timeout and exited normally.

SHA256:

```text
c5934024ec5f3af00b2a87f2a20b75b3541ea0788d372c2c34c3f899f7600d65  target/m1-16-driver/src/main.rs
183b9161bdaa05fbfe5abbf8670bfe6717ccf910ea304ddd4f1758e158de3388  target/m1-16-driver/src/tests.rs
1a323b03fc4cfb2685793c1a8f9133aecadf251151f6ed1322dadd6b7d18dd61  target/debug/m1-16-trusted-driver
```

## Risks, decisions and open issues

The initial stage did not repeat real Docker cancellation qualification. Parent must
integrate consumer changes, preserve source copies under docs/research, refresh
hash-bound configuration and arrange serialized Docker requalification plus
per-run absence checks before freeze. MCP internal-error classification can mark
uncertainty conservatively even when the internal error did not involve cleanup;
it intentionally cannot produce false verified-cleanup evidence.

Principal review/disposition and freeze authority remain with the parent.

## Follow-up: actual MCP shutdown failure, diagnosed and corrected

Parent requalification exposed a real server exit1 after active MCP cancellation.
Failed receipts are retained unmodified:

- `target/m1-16-driver-qualification/run-1788542464267348000/cancel-mcp.json`
- `target/m1-16-driver-qualification/run-1788542661030854000/cancel-mcp.json`

The second receipt includes a captured private driver terminal acknowledgement:
execution_joined=true, server_joined=true, cleanup_uncertain=true,
driver_error=child_failed, server_exit={code:1, signal:null, success:false}.
The server stderr is `ERROR MCP stdio session failed`. This was an ordinary
failure exit, not a signal. The broker's former combined execution_joined=false
field did not represent the driver's actual join claim.

Two distinct SDK behaviours explain the correction:

1. Pinned rmcp3.2.0 `src/service.rs:1454-1469` completes the local pending request
   with ServiceError::Cancelled after sending our cancellation notification.
   That exact error, only when driver cancellation is active, is now classified
   as `cancelled`. Unexpected cancellation remains uncertain; internal errors
   remain uncertain even during cancellation.
2. Pinned rmcp3.2.0 `src/service.rs:1735-1755` drains late handler responses with
   transport.send during server shutdown. Client SDK cancellation drops its
   stdout reader while the product can still write that final response. The
   product CheckedWriter records the broken pipe as an I/O failure and exits1.
   The driver now retains a cloned stdout descriptor across SDK close, then drains
   it concurrently with child.wait and the stderr worker. This uses no unsafe
   code or new dependency, retains constant8KiB buffer memory, and reports
   shutdown output beyond1MiB as a failure after draining to EOF.

The pipe is retained solely for orderly teardown: the SDK remains the only
protocol reader while active. No nonzero exit was whitelisted. `server_exit`
is now an orthogonal private terminal diagnostic {code, signal, success}, null
for raw mode. All previous cleanup-failure tests remain in place.

An actual MCP cancellation after this fix passed in
`target/m1-16-driver-qualification/run-1788542787247260000/cancel-mcp.json`:
driver exit1/cancelled, server code0/signalnull/successtrue, both joins true,
cleanup_uncertain=false, and no owned Docker containers or volumes.

Final source and binary after adding the regression tests:

```text
073890ec59c7697451df09e3b21793a37f512554fa1cf6eb834c91e2f6e86a08  target/m1-16-driver/src/main.rs
510f8d074332f3f53dcd4f1ab5b3c8efda10b54cd31c4c119cf59dbd824453b2  target/m1-16-driver/src/tests.rs
7da80655d7908d41ab0f9b6ebc0b78eaab692821a19990b3a005a5e11f8598a4  target/debug/m1-16-trusted-driver
```

The earlier hashes above are historical evidence of the initial fix, superseded
by these final identities. The same fmt/test/clippy/build commands pass with13
driver tests plus1 existing bundle test. New discriminating tests cover expected
versus unexpected SDK cancellation, preserved internal-error uncertainty, exit
status diagnostics, and a real /bin/cat pipe writing256KiB after the SDK-equivalent
reader has been dropped. The retained endpoint drains to EOF and the child exits0;
the1MiB drain threshold is checked separately. This is a trusted OS fixture, never
execution of participant/project Rust on the host.

Final exact-binary requalification is retained at
`target/m1-16-driver-qualification/run-1788542887633558000/`:
both `cancel-raw.json` and `cancel-mcp.json` passed after observing an actual
running Cargo test, with explicit container/volume absence and captured private
terminal acknowledgements. MCP server exit0 is required and observed. IPv4/IPv6
outbound deny controls also passed. The qualification script was executed in
memory with a diagnostic broker wrapper that retained terminal acknowledgements;
the final run executes its unchanged both-arm --cancel-only body. No script source
or production source was modified. The worker released the Docker slot to parent
immediately after this successful run.
