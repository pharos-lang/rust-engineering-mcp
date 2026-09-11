# M1-01 workers — independent review and principal disposition

Claude Code2.1.259; explicit claude-opus-5, effort high. Read-only packet,
--safe-mode --restricted --strict-mcp-config --tools empty, no persistence.
Actual modelUsage confirms claude-opus-5; auxiliary Haiku telemetry is also present.
No reviewer tool execution, edits, commits or merges. Review scope is the worker,
transport and project.open patch; it does not certify Cargo or source transfer.

## Principal disposition

No confirmed P0 or remaining blocking regression in this unit.

- P1-A: factually unsupported batch claim. Pinned rmcp3.2.0 model.rs677 explicitly
  defines exactly Request/Response/Notification/Error. Added exhaustive matches,
  batch decoder tests and wire rejection tests in every supported mode. No batch
  dispatch bypass exists in this pinned SDK. Legacy batching support is not claimed.
- P1-B: real, pre-existing bounded bootstrap property of M0 (ADR-023/024). Documented
  the10s cooperative first-project.open cancellation window explicitly, added a
  first-call/cancellation/recovery wire fixture. Costly bootstrap rejection is
  clearly labelled a deferred prerequisite before Cargo, not implemented here.
- P1-C: pre-existing deliberate ADR-024 mapping of resource-policy admission to
  SANDBOX_DENIED; unchanged schema/status/message. A new busy public code would
  silently change the agreed vocabulary. Retained current behavior; future costly
  tool ergonomics require explicit contract design, not a foundation regression fix.
- P1-D: no demonstrated reachable panic. Poisoned registry fail-closed behavior is
  normative ADR-024. Reject suggestion to recover poisoned state without a proof of
  registry invariants; existing deep-TOML tests remain green.
- P2 send budget: independent from request budget by design. Invalid-shape responses
  originate in pinned AsyncRwTransport, whose inline framed send still traverses
  the checked writer. No claim sends<=admitted requests is made.
- P2 flush: verified pinned async_rw.rs115 uses SinkExt::send (flush per message);
  documented SDK-upgrade dependency. Partial-write timeout now has a discriminating
  test: emitted prefix can be incomplete, session fails, no complete RPC promised.
- P2 teardown: phase budgets are distinct; no aggregate OS teardown bound claimed.
  A blocked filesystem syscall can outlive worker grace; current mode spawns no
  external project processes. Cargo must enforce verified cleanup before exit.
- P2 ADR wording and future Service defaults: deferred readiness is labelled as such;
  SDK upgrade checklist requires reviewing delegation and flushing semantics.
- P2 escaping: contract.rs uses fixed Invalid tool arguments/InternalError messages,
  with no rejected value or validator payload; runtime schema/Serde tests verify it.
  New DTOs must bound full envelopes before serializer allocation.
- Cancellation tombstones are the explicit owner-selected safety tradeoff for
  rmcp3.2.0 response suppression. It remains documented and tested.

## Reviewer output (independent, not authoritative)

## Scope and limits

Reviewed only the six supplied files. Not supplied and therefore unreviewed (called out where they carry the risk): `stdio/contract.rs`, `SecureProjects`/`ProjectRegistry::open`, the pinned rmcp 3.2.0 sources, `fixtures/rust-runtime/`, and the modified docs. No commands run, nothing changed.

**No P0 confirmed from the supplied files.** P1-A below is the only candidate that would become P0, and only against a fact I cannot check without the pinned SDK.

---

## P1 findings

### P1-A — `receive`/`send` classify messages non-exhaustively; anything that is not `Request`/`Notification` bypasses admission entirely

`crates/mcp-server/src/stdio/admission.rs:160-165`

```rust
let slots = match &message {
    JsonRpcMessage::Request(_) => &self.requests,
    JsonRpcMessage::Notification(_) => &self.notifications,
    // Responses have no detached server handler. SDK handles them inline.
    _ => return Some(message),
};
```

The comment reasons about `Response` only, but the arm is a wildcard. `JsonRpcMessage` in rmcp's model carries batch variants alongside `Request`/`Response`/`Notification`/`Error`. If `BatchRequest` is present in the pinned 3.2.0 enum, a single admitted frame (up to the 1 MiB cap in `budget.rs:15`) is forwarded with **no permit, no ledger entry, and no duplicate-ID check**, and the SDK dispatches every element in it. That is a direct bypass of the "16 requests" bound the patch exists to establish (ADR-030:40-42).

The mirror image is at `admission.rs:116-120`: `response_id` is `None` for any variant other than `Response`/`Error`, so a `BatchResponse` releases no ledger entry — those leases become permanent tombstones until `close()`, consuming the same 16 slots.

Evidence that this is untested either way: `protocol.rs` has no batch fixture anywhere (the wire fixtures at `protocol.rs:198-203`, `237-253`, `611-631` are all single messages), and `admission.rs:221-268` only constructs single `Request`/`Notification`/`Response` values.

**Verification, then severity:** check the pinned `rmcp::model::JsonRpcMessage` variant list and whether the server transport accepts batches on any negotiated version in `SUPPORTED_VERSIONS` (`stdio.rs:29-35` includes `2025-03-26`, where JSON-RPC batching is in-spec). If `BatchRequest` reaches `receive`, this is **P0**. Either way the fix is the same and cheap: replace the wildcard with explicit arms, and make any unclassified variant `failed.record(); return None` rather than pass-through.

### P1-B — the bootstrap cancellation gap already applies to `rust.project.open`, not only to future costly tools

`crates/mcp-server/src/stdio.rs:154`, `crates/mcp-server/src/stdio/project.rs:16`, `crates/mcp-server/src/stdio/workers.rs:97-103`

ADR-030:10-12 states the SDK awaits the first request inline before starting the receive loop, and 28-30 mitigates this by rejecting *costly* tools during bootstrap — described as future work. But the condition already holds for the shipped tool: nothing prevents `tools/call rust.project.open` from being the first request on the connection. During that window `AdmittedTransport::receive` is not being called, so `notifications/cancelled` cannot arrive, so `request.cancelled()` at `workers.rs:99` can never fire. The first request is therefore uncancellable until the `DEADLINE` arm at `workers.rs:101` expires — 10 s (`project.rs:16`), against a 12 s shutdown grace (`stdio.rs:130`).

Impact is bounded (read-only manifest reads, 10 s) but the ADR's claim that this class of problem is deferred is not accurate for the current tool surface. The protocol suite never exercises it: every fixture reaches `project_call` only after `bootstrap()` has already issued `tools/list` (`protocol.rs:255-272`, used at `407-412`, `442-443`, `484-485`). Add a first-request `tools/call` fixture with a cancellation, and either implement the bootstrap rejection now or document the 10 s uncancellable window as a known property of `project.open`.

### P1-C — transient concurrency rejection is reported as `SANDBOX_DENIED`

`crates/mcp-server/src/stdio/project.rs:286-288`

```rust
Err(WorkerError::Busy) => {
    Err(ProjectError::Rejected(OperationalErrorCode::SandboxDenied))
}
```

`Workers` has exactly one slot (`workers.rs:62`) with no queue (`workers.rs:87-89`), while the transport admits 16 concurrent requests (`admission.rs:21`). Two overlapping `project.open` calls are therefore ordinary, expected traffic, and the loser receives `status: "blocked", error_code: "SANDBOX_DENIED"` — the identical payload produced for a genuine authorization denial at `project.rs:168`, and the exact value the macOS wire test asserts as the signature of an unauthorized root (`protocol.rs:342-346`).

Two concrete harms: a client or agent cannot distinguish "retry in a moment" from "this path is permanently denied by the host" and will typically stop or escalate; and operators get no signal that the server is saturated, because overload is indistinguishable from policy. The shared message text at `project.rs:133-135` ("Project access **or server resource policy** denied the operation") shows the conflation is deliberate, but it makes a security-relevant status code non-discriminating.

If M0 mapped `Busy` the same way this is pre-existing rather than a regression — I cannot tell from the supplied post-image. Recommend a distinct outcome (retryable/busy) before a second tool shares the slot; the merge becomes far more misleading once a long Cargo worker can hold the only slot for minutes (see Cargo section).

### P1-D — a panic under the registry lock permanently disables the only tool

`crates/mcp-server/src/stdio/project.rs:279-280`, `209-214`

```rust
registry
    .lock()
    .map_err(|_| ProjectError::Internal)?
    .open(&input.path, control)
```

The guard is held across `open`, which parses attacker-controlled manifests. One panic inside `open` poisons the `Mutex` for the remaining process lifetime, so every subsequent `project.open` returns `ProjectError::Internal` → `ErrorData::internal_error` (`project.rs:210`) — permanent unavailability, with no reconnect path since the registry is process-global.

The worker side handles the panic correctly: `let _permit = permit;` is the first statement in the blocking closure (`workers.rs:92`), so unwinding releases the slot, and the `JoinError` maps to `WorkerError::Internal` (`workers.rs:102`). Only the mutex state is unrecoverable.

Contingent on a reachable panic, which is precisely what `deeply_nested_toml_is_rejected_without_aborting_the_server` (`protocol.rs:439-478`) exists to guard against — the authors already treat untrusted-TOML panics as a live threat. `ProjectRegistry::open` was not supplied, so I cannot say whether one is currently reachable. Recovering the guard via `PoisonError::into_inner` (after confirming `Registry` has no cross-call invariant broken by a mid-`open` panic) removes the permanent failure mode.

---

## P2 findings

1. **Send capacity is not derived from admitted work.** `sends` is `CAPACITY` = 16 (`admission.rs:95`), the same budget as `requests`, but responses that never took a request permit also consume it: the id-less `-32600` errors exercised at `protocol.rs:744-752` arrive as messages that fall through `receive`'s wildcard and are answered anyway. Exhaustion calls `failed.record()` (`admission.rs:137`) and kills the session. The supplied tests only ever drive these sequentially, so I have no evidence they can overlap — but the invariant "sends ≤ admitted requests" is not established by the code.

2. **The writer deadline survives across frames unless a flush lands on a frame boundary.** `CheckedWriter::expired` creates the timer on the first `poll_write` (`budget.rs:148-152`) and it is cleared only in `poll_flush`, and only when `line_bytes == 0` (`budget.rs:211-213`). If the SDK ever writes a complete frame without flushing, the next write more than 10 s later fails a healthy session. This holds today only because rmcp's sink flushes per message; it is an undocumented dependency on SDK internals.

3. **A dropped send future can leave a truncated line on stdout.** When `timeout_at` fires at `admission.rs:145-150` the inner send future is dropped, possibly mid-`poll_write`. The harness itself treats this as an error condition (`protocol.rs:87-89`, and `finish()` at `177-180` rejects trailing output), so the covered paths are clean; the unterminated-frame behaviour on the timeout path is untested.

4. **No bound on total teardown.** Worst case is 10 s frame deadline (`budget.rs:17`) + 10 s send/close deadline (`admission.rs:22`, used at `199`) + 12 s worker drain (`stdio.rs:130`) + 100 ms runtime drop (`stdio.rs:135`), sequentially, with no aggregate cap. Fine for a supervised stdio child; worth stating explicitly.

5. **ADR-030 states unimplemented behaviour in the present tense.** Lines 28-32 ("reject their execution during SDK bootstrap with a fixed structured SANDBOX_DENIED response", "Set readiness immediately after `serve_server_with_ct` returns") have no counterpart in the patch — there is no readiness concept in `stdio.rs:154-165` at all. The Status block (ADR:5) says "implementation in progress", which covers it, but a reader of the Decision section alone would conclude both exist.

6. **Tombstone policy — accepted risk, recording the operational consequence only.** 16 cancellations close the session (`admission.rs:166-194` + retained ledger entries), forfeiting every registered `project_ref` (capacity 64, `project.rs:254`). Agents that cancel on user interrupt reach 16 quickly. Owner-chosen; noted, not re-litigated.

7. **`AdmittedService` forwards three trait methods.** `handle_request`, `handle_notification`, `get_info`, `supported_protocol_versions` (`admission.rs:45-75`). Any defaulted `Service<RoleServer>` method added by a future rmcp release silently reverts to the SDK default instead of `EngineeringServer`'s override, with no compile error. Add a pin-upgrade checklist item.

8. **Input and output caps are both 1 MiB with no escaping headroom** (`budget.rs:15`, applied to both directions). A ~1 MiB accepted request whose error path echoed any input substring could exceed the output cap after JSON escaping and kill the session. The adapter's own error text is fixed and payload-free (`stdio.rs:85-90`, `164`), but `Contract::decode`'s `-32602` message was not supplied and is the one path I could not clear.

---

## Future Cargo requirements (not defects in this patch)

- **ADR-030:26 is unenforced by the API.** `Workers::run` releases the slot when the closure returns (`workers.rs:90-96`), and the bound `F: FnOnce(&Control) -> Result<T, E>` permits returning while a child process tree is still alive. "Await verified Execution Gateway cleanup before the permit is released" must be implemented *inside* the closure; nothing in the type system will catch its omission.
- **`runtime.shutdown_timeout(Duration::from_millis(100))` (`stdio.rs:135`) is unsafe for process-spawning workers.** After a failed 12 s drain (`stdio.rs:130` returning `false`), the process exits 100 ms later with the blocking thread still live. Harmless for filesystem reads; with Cargo this abandons a process tree. A kill-tree step must precede exit.
- **One global slot shared across all operations** (`workers.rs:62`) means a long `cargo check` makes every concurrent `project.open` return `SANDBOX_DENIED` (P1-C). Resolve the status-code conflation before adding a second, slow tool class.
- **Bootstrap rejection (ADR:28) and readiness (ADR:31) are prerequisites, not present.** P1-B shows the underlying gap is already live at 10 s scale.
- **Cooperative cancellation granularity is unverified.** The 12 s grace assumes `Registry::open` calls `OperationControl::check` frequently. A single blocking syscall that never returns — e.g. a FIFO or a stalled network mount under an authorized root — defeats it. Whether `SecureProjects` rejects non-regular files was not reviewable here; confirm before Cargo widens the syscall surface.

---

## Verified correct in the supplied code

- Worker admission tracks the blocking closure, not the async waiter: permit moved into `spawn_blocking` (`workers.rs:90-96`), `drop_guard` on `local` signals cancellation on waiter drop/abort (`workers.rs:79`), and the timeout arm does not release the slot (`workers.rs:101`, proven by `workers.rs:258-297`). The abort case is covered at `workers.rs:198-201`.
- Pre-acquisition and post-acquisition cancellation checks (`workers.rs:86`, `93`) mean no admitted-but-dead request ever invokes work (`workers.rs:218-255`).
- Ledger removal happens **before** the write (`admission.rs:125-131`, comment at `122-124`), so a completing send can never free a newer entry with the same ID; `admission.rs:342-362` proves it.
- Reader distinguishes idle time from partial-frame time correctly (`budget.rs:106-113`), does not renew on progress, and is proven end-to-end against a live writer at `protocol.rs:206-223`.
- Failure state is shared bidirectionally and cancels the SDK session (`budget.rs:26-30`), and a post-failure clean EOF cannot be mistaken for success because of `clean_close && !failed.occurred()` (`stdio.rs:166`).
- No `unsafe`, no panicking unwraps in the adapter paths, `RUST_LOG` deliberately ignored (`stdio.rs:95-103`), and SDK errors are never formatted into output (`stdio.rs:163-164`) — the `SECRET` assertions throughout `protocol.rs` back this.
