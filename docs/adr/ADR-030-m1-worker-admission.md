# ADR-030 — M1 bounded workers and transport admission

## Status

Accepted; worker/transport prerequisite implemented and reviewed. M1-01 prerequisite, not completion
of project.inspect or authorization to execute Cargo.

## Context

rmcp 3.2.0 owns protocol parsing and dispatch. Its modern bootstrap awaits the
first request inline before starting the receive loop. A blocking worker alone
cannot make cancellation notifications observable during that first request.
The existing project.open worker has one slot but no reusable session shutdown
control. Transport framing has a byte cap, without partial-frame/write deadlines.

## Decision

Keep the current-thread rmcp runtime and SDK protocol implementation. Share a
bounded worker admission boundary across project operations: one active worker,
no waiting queue, permit held until the actual blocking closure exits. Propagate
request cancellation, session shutdown and monotonic deadlines into the worker.
Dropping a request signals cancellation; it never implies that a blocking worker
or an external process has terminated. Shutdown cancels workers and waits for
their actual completion with a bounded grace period, reporting failure otherwise.
Cargo workers must additionally await verified Execution Gateway cleanup before
their permit is released. ADR-032 implements that composition with run_joined: retain and await the real blocking JoinHandle; interruption never erases a cleanup error. A persistent panic latch is set before releasing the permit, including when the waiting handler was dropped. Session EOF signals cancellation when the SDK transport finishes delivering buffered messages. I/O failure signals it immediately.

Shutdown grace remains12s without explicit Rust configuration and becomes240s with it, covering the current bounded Docker control, observer and up to twelve cleanup controls with pipe-drain margins. Exceeding grace fails the session and is never proof of cleanup; kernel/daemon stalls remain an explicit limit.

Implemented by ADR-032 for costly inspection: reject execution during SDK bootstrap with a
fixed structured SANDBOX_DENIED response. Discovery or tools/list establishes the
receive loop; clients can retry with a new request ID. Set readiness immediately
after serve_server_with_ct returns and before yielding on the current-thread
runtime. Do not replace SDK negotiation with serve_directly or intercept JSON.
Existing bounded structural project.open remains compatible with M0: when it is
itself the first modern request, peer cancellation is not observed during its
inline execution (up to the10s cooperative deadline). This known M0 limitation
is preserved explicitly; the new worker does not resolve SDK bootstrap reception.

Bound partial input frames and response writes/flushes by absolute deadlines.
Idle time between complete input frames is unrestricted. Progress within a frame
does not renew its deadline. Retain the 1MiB input cap and apply a 1MiB output frame
cap. Fail the session on transport deadline, overflow or I/O failure and signal
session cancellation. These byte guards do not parse JSON or fabricate RPC errors.
Typed SDK transport admission uses separate limits of16 requests,16 notifications
and16 pending sends. Request leases span dispatch, the SDK response queue and
actual send completion. Duplicate outstanding request IDs reject the session.
The pinned SDK suppresses cancelled responses without a transport callback; those
request leases conservatively remain until session teardown. After16 retained
cancellations, a further request closes the session; reconnect/reopen is required.
This explicit resource policy avoids recycling capacity while suppressed response
producers could still be blocked in the SDK queue. Notifications have independent
capacity, so active requests do not prevent cancellation delivery. Overload fails
the session; receiving never waits for an admission permit. SDK-owned allocation
and native RSS are not inferred from these bounded object counts.

## Alternatives considered

- spawn_blocking alone: does not start the SDK receive loop during bootstrap.
- serve_directly: bypasses supported modern/legacy negotiation and metadata checks.
- A second JSON-RPC parser: violates the SDK ownership boundary.
- Release admission when the async waiter disappears: admits more work while the
  previous blocking closure or process tree still runs.
- Unbounded request queues: permit memory growth and delay cancellation.

## Consequences

Costly first-call behavior must be documented and protocol-tested when those tools
are introduced. Filesystem kernel reads remain cooperatively cancellable; no hard
kernel I/O termination is claimed. Cargo needs the separately approved runtime,
source transfer, containment calibration and cleanup tests. M0 remains closed.

## Sources

- Pinned rmcp3.2.0 service/server.rs bootstrap and service.rs dispatch sources.
- https://docs.rs/tokio/1.53.1/tokio/task/fn.spawn_blocking.html
- ADR-023, ADR-024 and docs/m1-prerequisites.md.
