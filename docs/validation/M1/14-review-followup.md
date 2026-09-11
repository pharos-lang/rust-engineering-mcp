# M1-14 follow-up

Claude Code2.1.259, explicit Opus5 Medium, read-only/tools disabled.
Opus5 modelUsage confirmed; CLI also reports auxiliary Haiku4.5 usage.

## F4 verdict: the design holds — no orphan execution, and the exception is genuine, not a loophole

I read `doctor_run.rs`, `doctor.rs` and ADR-045 as given. The two properties you asked me to separate really are separate here, and the code enforces the strong one.

**Cleanup/observation join is total.** `observe` (doctor_run.rs:96-100) has exactly three exits and every one of them ends in `work.await`:

- normal arm — the handle itself resolved;
- signal arm — sets `cancelled` *then* `work.await`, with no timeout wrapped around it;
- deadline arm — `work.await`, relying on `Control::check` returning `CommandTimeout` (doctor_run.rs:23-24) for cooperative unwind.

There is no `abort()`, no detach, no `select!` around the second await, so a repeated signal cannot skip the join (ADR:57 holds). The `Err(_)` arm at doctor_run.rs:104 is a `JoinError`, which only resolves after the blocking closure's frame has unwound, so Drop-based cleanup has already run by the time the handle completes. The gateway, the `CatalogProvider`, and the `Component<T>` payloads all live inside `Report`, which `render` consumes by value and drops on the runtime thread (doctor_run.rs:128, 146-159) — before the writer is spawned.

**The write exception is well-scoped.** The closure at doctor_run.rs:136 captures exactly `bytes: Vec<u8>` and touches `io::stdout()`. Both `json`/`started`/`code` are copies or already consumed. So the detached thread's entire resource footprint is heap bytes plus the process-wide stdout lock, both reclaimed by process exit. That is categorically different from detaching a container-owning gateway task, which is what ADR:61-62 forbids; the ADR text at :59-63 documents the exception at exactly the right granularity. Bounded failure is real: 5 s cap on delivery plus `shutdown_timeout(100ms)` (doctor_run.rs:143), then `main` returns.

So F4 is satisfied as claimed. What follows are residual findings, none of which reopen it.

---

### Finding 1 — a queued second signal silently discards a fully-rendered report
**Severity: Minor (diagnostic-quality loss on an already-failing run)** · **Path: `doctor_run.rs:137-141`** · **Trigger: any signal delivered while `work.await` is joining, e.g. double Ctrl-C.**

Tokio's `Signal` coalesces but does not drop notifications: a SIGINT arriving after `observe`'s select already resolved leaves the watch marked changed, so the *next* `recv()` returns immediately. The delivery select therefore fires its signal arm on the first poll, drops `output`, and exits 1 with **no bytes written at all** — even though the report is complete, encoded, and stdout is healthy.

Double-Ctrl-C on a tool that looks hung is common, so the realistic outcome of "user interrupts a slow active run" is an empty stdout rather than the interrupted report. Since no execution work is outstanding at that point, the signal arm buys nothing the 5 s bound doesn't already provide. Dropping the arm (or reaching it only after a first `biased` poll of `output`) preserves responsiveness and delivers the diagnostic. Note this narrows ADR:58 ("signal observation remains live through report delivery"), so the ADR sentence would need to change with the code.

### Finding 2 — setup failures exit 1 with no report at all
**Severity: Minor** · **Path: `doctor_run.rs:114-126`** · **Trigger: non-unix/non-windows target (`Signals::register` always returns `UnsupportedPlatform`), or a rare `signal()`/runtime-build `io::Error`.**

Both return `ExitCode::FAILURE` before any report exists, so a `--json` consumer gets empty stdout and exit 1. Every other failure path emits a `format_version: 1` report with an `Id::Diagnostic` check; `Reason::UnsupportedPlatform` and `Reason::Internal` already exist for this. This is not the documented "delivery failure may leave an incomplete response" case (ADR:62) — nothing was ever attempted. Rendering `Report::failure(active, …)` on these paths would close the contract gap.

### Finding 3 — panic inside `inspect` maps to `Internal`, never `CleanupUncertain`
**Severity: Minor, conditional** · **Path: `doctor_run.rs:104`** · **Trigger: panic in the blocking observation, most plausibly inside the active gateway path.**

Sound if and only if the gateway's container teardown is Drop-based; if any part of it is an explicit call on the happy/error path, a panic skips it and the report claims `Reason::Internal` while a container may survive. `Reason::CleanupUncertain` exists (doctor.rs:112, 470) precisely for that ambiguity and would be the honest mapping for a `JoinError` in active mode. **I could not verify this** — it depends on `rust_engineering_execution::RustProjectInspector`'s Drop impl, which is outside the files provided and I did not read anything (read-only, no tools, as instructed).

### Finding 4 — no explicit flush in the writer
**Severity: Nit** · **Path: `doctor_run.rs:136`, relies on `render` at :157.**

`write_all` on a `StdoutLock` (a `LineWriter`) only guarantees the bytes leave the buffer because `render` appends `b'\n'`. `Ok(())` from a bufferless-looking call is being treated as delivery success. Chaining `.and_then(|_| io::stdout().lock().flush())` makes the success signal independent of that coupling and surfaces late `EPIPE`/`ENOSPC` instead of swallowing them.

### Finding 5 — the *observation* phase has no bound at all (accepted, but worth stating)
**Severity: Informational** · **Path: `doctor_run.rs:98-99`.**

"Responsive failure" is guaranteed for delivery (5 s) but not for observation: both the signal and deadline arms await the blocking task indefinitely. Compounding this, registering SIGTERM and SIGHUP installs process-wide handlers, so an uncooperative hang inside the gateway is no longer terminable by SIGTERM or by terminal hangup — SIGKILL becomes the only recourse. That is the correct trade (no-orphan over responsiveness) and ADR:56-57 covers it obliquely via "deadlines are cooperative" and the SIGKILL carve-out, but the ADR nowhere says that adding SIGHUP handling *removes* the default hangup kill. One sentence in Consequences would make the operational cost explicit.

---

**On your other carried items, from the code shown:** F1 is correct — `index_source_observed` (doctor.rs:581-593) excludes `FeatureDisabled`/`DependencyUnavailable`, so an embedded index only fails when `index_store` is explicitly set, and the test at doctor/tests.rs:159-178 pins both directions including corrupt-still-fails. F2 present at doctor_run.rs:39/48/52. F3 correct: the deadline arm never sets `cancelled`, so `check()` yields `CommandTimeout` → `Reason::Deadline`, while the signal arm's `cancelled` is checked first and wins on overlap — the precedence is the right way round. F7: `duration_ms` is recomputed on the encode-failure fallback (doctor_run.rs:153), and `OutputLimitExceeded` stays distinct from `Internal` through `record_failure` (doctor.rs:241-245).
