# M1-03 — Opus5 Medium focused read-only follow-up

Claude Code2.1.259; explicit claude-opus-5/medium, no tools.

## Follow-up review — M1-03 fixes only (read-only, no tools run)

Scope: the four addressed P1s plus regression check on the new code as pasted. I did not execute anything; all statements below are from the supplied sources.

---

## Prior P1s — status

| Finding | Status | Evidence |
|---|---|---|
| Orphan artifact quota | **Partially resolved** | `reap_artifacts` (`artifact_access.rs`) prunes then `retain_owners`, called before capture, before each read, and on every error path in `check`. Reclaims *dead* owners only — see P1 below. |
| Stream starvation | **Resolved** | `LOG_STREAM = (MAX_STREAM-128)/2` gives both streams an equal 131 008 B floor; overhead is 69 B worst case (15+16+19+19) so the log is ≤ 262 085 B < 256 KiB. `ArtifactInput::truncated` is fed the merged gateway∨gateway-app flag and surfaces in `Log.truncated` and the Resource `_meta`. Markers use the merged flag, so gateway-only truncation stays visible. |
| Hardcoded versions | **Resolved with a caveat** | `rust_version`/`cargo_version` remain `APPROVED_*` constants, but they now sit alongside `image_id` + `configuration_fingerprint` + `execution_fingerprint` and are gated by calibration. They are an *assertion inherited from calibration*, not an observation of this run. That is defensible; it just isn't stated anywhere in ADR-034. |
| Unauthenticated diagnostics | **Resolved** | Tool description carries "Project code may write the diagnostic stream; normalization does not authenticate its origin", ADR repeats it, and `frozen_lock_error` is fenced by exit 101 + empty stdout + `!truncated` + exact first-line match, with a negative test per relaxation. |

---

## P1 — live-owner artifact accumulation has no reclamation path

`reap_artifacts` only reclaims artifacts whose owner is *no longer registered*. For a live project, retained logs are released only by TTL (3600 s). With `ArtifactLimits::default()` at 1 MiB/owner and 64 items/owner, a project that iterates — the expected agent loop — reaches the owner cap and then every subsequent `rust.check` fails at `artifacts.capture(...)` with `ArtifactError::QuotaExceeded`, which `artifact_error` maps to `InspectionError::OutputLimit` → `OUTPUT_LIMIT_EXCEEDED`. 64 checks in an hour on one project is ordinary usage; the tool then stays unusable for that `project_ref` for up to an hour with no operator-visible cause.

Two secondary problems compound it:

- The code is wrong for the condition. This is retention capacity, not output size, and §4.5 reserves `OUTPUT_LIMIT_EXCEEDED` for inability to deliver a safe partial artifact — here a safe partial (diagnostics, no log link) exists.
- All diagnostics are discarded on this path even though the check itself completed.

Suggested fix, consistent with the ADR's own rule: on `QuotaExceeded` during check capture, reclaim the *same owner's* oldest logs and retry once. Successful publication superseding an owner's own earlier artifact does not violate "failed artifact publication does not silently evict existing content" — that clause protects against a *failed* publication evicting, and it says nothing about a project reclaiming its own prior log. Alternatively cap by construction at N logs per project.

## P2 — a completed check discarded by late interruption leaks its artifact

`joined_result` maps `(Ok(_), Some(signal))` to `worker_error(signal)`, so a check that captured the log, passed final authorization and produced a `ProjectCheck` is turned into `Blocked{CommandTimeout}` / `Cancelled` with `data: None`. The artifact was published under a still-registered owner, so `reap_artifacts` will never reclaim it and no URI ever reaches the client. It is unreferenceable retained content charged against the owner budget for the full TTL — a direct feeder for the P1 above, and reachable whenever the 120 s worker budget expires after the 30 s Cargo phase or a client cancels late.

ADR-034 provides exactly the right primitive ("owner-bound removal of a single artifact to rollback newly captured logs after failed final authorization") but only wires it to *failed* authorization in `check`. The discard-after-success case needs the same rollback, which means the removal has to happen where the `Joined` is unwrapped, not inside `ProjectRegistry::check`.

## P2 — Resource interruption and cancellation report −32603, contradicting the stated −32000 rule

In `resources.rs::read_uri`:

```rust
Err(ArtifactAccessError::Internal | ArtifactAccessError::Cancelled) => return Err(internal()),
Ok(_) if joined.interrupted.is_some() => return Err(internal()),
```

`worker_error` correctly emits −32000 for `Busy | Cancelled | TimedOut` at *admission*, but both post-admission interruption paths collapse to `internal_error` (−32603). The review clarification in the ADR says "Resource admission Busy/interruption uses fixed JSON-RPC server error −32000; internal errors remain −32603". A client-initiated cancellation and a deadline hit are being reported as server faults, so the retryability signal is lost and the two conditions are indistinguishable from a real internal failure. `ArtifactAccessError::Cancelled` in particular is never an internal condition — it is produced by `control.check()?`. Route both through the −32000 message used by `worker_error`, or narrow the ADR clause to admission-only and say so explicitly.

## P2 — `validation_complete` can stay true while the retained log is cut at half the old threshold

The balanced-budget fix halved the per-stream log allowance to ~128 KiB, but `bounded_stream`'s cut is applied in `application::check` *after* the inspector computed `validation_complete`, and only `observation.stdout_truncated` / `stderr_truncated` are updated. So a run with 200 KiB of stdout now emits `validation_complete: true` together with `truncation.stdout_truncated: true` and `log.truncated: true`.

I believe the behaviour is right — the criterion is completeness of the *diagnostic parse*, which happened against the full stdout inside the inspector, and the abridged log is fully signalled by the section markers plus the artifact flag. But ADR-034 currently states flatly that "Partial evidence caused by termination/output/diagnostic budgets never claims complete validation", and the log budget is an output budget. Under the halved threshold this combination is now common rather than rare, so the ADR needs one sentence distinguishing *evidence* completeness (parse + terminal build-finished + exit agreement) from *transport* completeness (log bytes, dropped diagnostics), otherwise the contract reads as violated by its own implementation.

## P3 — smaller items

- `access_error` maps `ArtifactAccessError::NotFound` to `PROJECT_NOT_FOUND`. At the end of `check` the common cause is a project lease that expired during the run, so this is usually accurate, but an artifact-side `NotFound` (retention race in `retention()`) would also be reported as a missing project. Cosmetic, no authorization impact.
- The `observation.stdout.len() > MAX_STREAM` early return in `check` yields `OUTPUT_LIMIT_EXCEEDED` with no artifact at all, even though `bounded_stream` immediately below could produce a safe partial. It is unreachable given `ExecutionLimits::new(30_000, 256*1024)`, so this is a defensive branch — but it is a defensive branch whose behaviour contradicts §4.5. Truncating instead of erroring would make the invariant failure-safe in the direction the spec wants.
- `encode_bounded` changes the summary when it downgrades `Passed` → `Failed`, but leaves "Cargo check reported compilation failure" in place when it forces `validation_complete = false` on an already-`Failed` result. The claim stays true, yet the pairing of a compile-failure summary with `validation_complete: false` is exactly the mixed signal the five-state contract is trying to avoid. Consider the incomplete summary in both branches.
- `crates/mcp-server/src/stdio/check.rs` holds the registry guard *and* the store guard for the entire `check` call — i.e. across the 30 s Cargo phase. `resources.rs` takes the same order (registry → store), so there is no deadlock, and if the worker pool admits one operation at a time the contention is invisible because `Resources::read` gets `Busy` at admission. If the pool ever admits concurrently, a resource read would block on `Mutex::lock()` inside its worker and its 10 s deadline could not fire, since `control` is only polled cooperatively. Worth an assertion or a comment recording the single-admission dependency rather than leaving it implicit.
- `retain_owners` computes `before` prior to `expire(now)`, so its returned count includes TTL expiries as well as owner reclamations. Return value is unused by `reap_artifacts`; only affects metrics if it is ever surfaced.

---

## Confirmed correct — no action

- Two-clock discipline: `ArtifactClock` is `Clone` over a shared `Instant`, so the store, `retention()` and the Resource read all measure from one monotonic origin; `retention()` rejects `now < created_seconds` and a non-advancing TTL as `Internal` rather than granting access.
- `read_artifact_inner` renews the project lease only after every fallible check (identity, retention, double-read equality, `control.check()`), and never renews artifact retention. Failed reads extend nothing.
- Bootstrap: `Resources::read_uri` checks `ready` after `parse` and before any registry/store access; `CheckTool::call` short-circuits to `Blocked{SandboxDenied}` with `data: None`. No artifact I/O before admission in either path.
- `parse` rejects on exact length before `strip_prefix`/`split_once`, so no normalization, escaping, query, fragment or trailing-slash surface exists.
- `encode`'s `MAX_RESPONSE` of 512 KiB is comfortably above the worst case (256 KiB → 349 528 B base64, which needs no JSON escaping) and below the 1 MiB frame.
- `encode_bounded`'s `MAX_RESULT / 4` loop bound is sound: structured content plus the re-escaped text fallback is at most 3× the measured `Output`, since worst-case re-escaping of an already-serialized JSON payload is 2× (`"` → `\"`), and control characters are already expanded in the measured bytes.
- Rollback ordering in `check`: `remove` is owner-scoped and returns `Ok(false)` harmlessly when `reap_artifacts` inside `read_artifact` already reclaimed the log after owner retirement; a hard `remove` failure escalates to `Internal`, preserving cleanup-outranks-outcome.
- I did not re-raise the partial-fallback path in `encode_bounded`; with diagnostics emptied, `Data` is bounded well under 8 KiB as you established, so it is dead code rather than a defect.
