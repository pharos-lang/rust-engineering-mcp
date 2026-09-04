# M1-09 — external review

Claude Code2.1.259; explicit claude-opus-5, effort high; safe/restricted, tools disabled,
read-only supplied packet. modelUsage confirms Opus5 (Haiku bookkeeping is not the
reviewer). No commands or edits by reviewer; scope limitations listed below.

# M1-09 Principal Review — security / contracts / evidence

**Method.** Read-only inspection of the pasted ADR-040, `domain/src/quality.rs`, `domain/src/audit.rs`, `application/src/quality.rs`, `application/src/artifact_access.rs`, `application/src/validation.rs`, `mcp-server/src/stdio/quality.rs`, the security test bodies, and the wiring diff. No tools, commands, or edits were run.

**Not verified (cite as unknown, not as pass):** `stdio/workers.rs` (pool size, `run_joined` semantics), `stdio/resources.rs` (lock order, `uri`), `stdio/contract.rs::encode`, `stdio/check.rs` precedents (annotations, diagnostics bound), `stdio/quality/schemas`, `domain/tests/quality.rs`, `mcp-server/tests/inspection_runtime/quality.rs`, `docs/validation/M1-08.md`, and the modified `protocol.rs`. I make **no gate claim**: the reported 6 application + 3 domain mock tests and the in-flight Docker suite are taken as reported status only, and core Docker execution is still pending.

**P0: none confirmed** in the reviewed slice. One finding (P1-1) becomes P0 if the unread `resources.rs` locks in the opposite order — that is the single highest-value thing to check next.

---

## P1

### P1-1 — Registry *and* Store mutexes are held for the entire ≤240 s run; possible lock-order inversion

`stdio/quality.rs::QualityTool::call` builds both guards as temporaries of one expression:

```rust
registry.lock()...?.quality_gate(..., &mut *store.lock()...?, (&WallClock, &clock), control)
```

Both live for the whole `quality_gate` call, whose worker deadline is `DEADLINE = 240s`. Consequences:

- Every other tool that touches the registry, and **every `resources/read` of any artifact URI (including other projects' logs)**, blocks for up to 4 minutes. Prior single-command tools held these for ≤ one 30 s command; M1-09 raises the hold by ~8× and makes it the common case for `standard`.
- Contention surfaces to clients as `WorkerError::Busy → ExecutionError::Busy → OperationalErrorCode::SandboxDenied` (`worker_error`, `output`), i.e. a transient queue condition is reported as a policy denial (see P2).
- **Deadlock risk:** this path takes `Registry` → `Store`. `read_artifact`/`read_artifact_without_touch` require both (`&mut self` on the registry plus `&mut impl ArtifactStore`). If the Resources read handler in `stdio/resources.rs` acquires `Store` before `Registry`, the two orders deadlock the server for the lifetime of the process. Verify the order there and in `auditing.rs`/`check.rs`/`testing.rs`; if inverted, this is P0.

Direction: assert a single documented lock order across all handlers (registry-then-store), and consider narrowing the store guard to the publication phase rather than the whole gate.

### P1-2 — Post-completion cancellation/timeout discards a published gate but leaves its staged log group and renewed lease

`stdio/quality.rs::joined_result`:

```rust
(Ok(_), Some(signal)) => Err(worker_error(signal)),
```

By that point `publish_quality` has already committed up to four log artifacts, passed the final grouped retention recheck, and called `touch_authorized_reference` — the lease *is* renewed. The client instead receives `Outcome::Cancelled { data: () }` (or `Blocked{COMMAND_TIMEOUT}` for `TimedOut`), so:

- all stage statuses and repair evidence for a completed run are lost;
- up to 4 × 256 KiB of committed logs become unreferenced (no URI ever returned) and occupy retention quota until TTL — which is precisely the pressure that makes the *next* run hit `QuotaExceeded` and drop its logs;
- the lease was extended by a run whose result was suppressed.

This also reads as a divergence from ADR-040's own sentence, "cancellation is suppressed by existing MCP policy after worker completion." The `joined_result` shape is inherited from the single-artifact tools, where the blast radius was one log; grouped publication amplifies it. Decide explicitly: either return the completed result on a post-completion signal (ADR wording), or treat late signals as a publication failure and revoke the whole new group. Silently keeping both the artifacts and the lease while discarding the result is the one option that should not stand.

### P1-3 — Outer project-snapshot `Evidence` is structurally always Fresh; capture age is unobservable

`application/src/validation.rs::publish_quality`:

```rust
Provenance::new(SourceKind::ProjectSnapshot, fp..., Some(captured.created_at),
                Some(clocks.0.now()), IntegrityStatus::Verified, false)
...
Evidence::Snapshot(SnapshotEvidence::assess(provenance, policy, clocks.0))  // policy 60 / 300
```

`observed_at` is stamped at publication and `assessed_at` is the same instant, so freshness age ≈ 0 and the 60 s/300 s policy can never fire. `created_at` (the real capture instant) is recorded but not used for freshness. Contrast `domain/src/audit.rs::normalize`, which for the RustSec snapshot explicitly rejects `observed_at > assessed_at`, `Live`, and `Unknown` — the outer evidence gets none of that scrutiny.

The pattern is inherited from `publish_validation`, where capture-to-publication was ~one command. Here the same evidence object may describe a source captured 240 s earlier and still reports Fresh, and no status or summary reflects it. Direction: use `Some(captured.created_at)` for `observed_at` (or assess against `created_at`) and choose a policy appropriate to a multi-stage run; if a stale outer snapshot is acceptable for a `passed` verdict, say so in the ADR rather than making the signal vacuous.

### P1-4 — Cross-stage `RuntimeIdentity` is never compared; a combined "passed" may span heterogeneous runtimes

ADR-040 mandates comparing source fingerprints, and `application/src/quality.rs::match_fingerprint` does exactly that — only for `SourceFingerprint`. Nothing compares `RuntimeIdentity.image_id`, `configuration_fingerprint`, or `execution_fingerprint` across stages, in the application layer or in `stdio/quality.rs::output` (whose loop also checks only `source_fingerprint`, and skips the Audit variant entirely because `QualityObservation::execution()` returns `None` for it).

The single-tool contracts each carried one runtime; the gate emits one aggregate verdict over five per-stage runtimes. A mixed or degraded executor could run Clippy under a different sandbox configuration than `check` and the aggregate still reads `passed`, with the divergence disclosed only if the client cross-checks five nested `runtime` objects itself. This is the same invariant class the ADR already establishes for source identity, and the fix is symmetric: fold runtime identity into `match_fingerprint` (infrastructure error on mismatch) and re-verify at the MCP boundary.

Related, same fix area: `AuditDetails` exposes `runtime` but no source fingerprint tying the metadata inspect to the common capture, so the audit stage's binding to the shared bundle is not client-verifiable.

### P1-5 — No pre-flight lease-headroom check; a lease that expires mid-run burns the full sandbox run and returns `PROJECT_NOT_FOUND` with no data

Per ADR, `quality_gate` revalidates without touching (`self.resolve_inner(reference, control, false)` per stage) and touches exactly once at the end. Nothing compares the lease's remaining TTL against the profile's worst case before `capture_validation`. A `standard` run is ~4×30 s commands plus calibration plus metadata plus audit; if the entry has less headroom than that, the run executes every stage, then `resolve_inner`/`touch_authorized_reference` fails and `output` emits `Outcome::Blocked { PROJECT_NOT_FOUND, data: None }` — all work discarded, all stage evidence dropped, and the client is told to re-discover only after burning the sandbox time.

Cheap fix: before capture, reject fast when `ttl_seconds - age` is below the profile's bound. This does not change the no-touch policy the ADR requires; it just fails honestly up front.

---

## Material P2

- **Artifact expiry reported as `PROJECT_NOT_FOUND`.** `validation.rs::access_error` maps `ArtifactAccessError::NotFound → Rejected(ProjectNotFound)`, and the final retention loop in `publish_quality` returns the same code when a log's lifetime lapses at the final artifact-clock instant. The project may be perfectly live; the client is directed to re-run discovery for what is an artifact-timing/infrastructure condition. Consistent with prior tools, but the grouped batch makes it far more reachable (test `batch_rolls_back_..._expiry_during_later_validation` exercises exactly this path and only asserts `is_err()`).
- **`applied_selection` is decoupled from the options actually used.** `impl From<QualityStage> for AppliedSelection` derives the label purely from the stage enum, including the literal `TestCargoDefaults30Seconds`. The application tests assert `options.timeout() == 30` and the Clippy/Check/Test option shapes, but nothing ties the emitted label to the constructed `CheckOptions`/`ClippyOptions`/`TestOptions`, and `FormatAll` asserts `--all` with no evidence from the format port at all. A future default change silently produces a lying label on every response.
- **`sha256` in `Log` is the store's unverified claim.** `artifact_access.rs::read_artifact_inner` validates `metadata.size_bytes as usize == view.content.len()` and `MAX_CONTENT`, but never recomputes the digest over `content`. The gate then publishes that hash as the client's integrity anchor for the Resource read.
- **`Truncation` has no log-omission field.** Log omission is visible only per stage via `log_unavailable_reason: RetentionCapacity`; a client reading the top-level `truncation` block sees nothing. ADR calls for "explicit omissions" at the MCP bound.
- **Deadline/revalidation aborts discard all completed stage evidence** (`data: None` on `Blocked{COMMAND_TIMEOUT}` / `Blocked{PROJECT_NOT_FOUND}`), which is ADR-sanctioned for new logs but sits in tension with "Preserve every stage's status and repair facts in the final result." Worth an explicit ADR line that abort paths carry no stage data.
- **`ExecutionError::Busy → SANDBOX_DENIED`** (`stage_issue` and `output`) presents server contention as a policy denial; clients will not retry. Amplified by P1-1.
- **`Execution.diagnostics` is unbounded at this boundary** while the schema declares `#[schemars(length(max = 128))]`; `From<CheckObservation>` copies the vector verbatim, and `encode_bounded`'s trimming only engages above the byte budget. Confirm the port enforces ≤128 (likely shared with `check.rs`); otherwise responses can violate their own advertised output schema.
- **`AuditDetails.unsupported_packages_omitted` is always initialized to 0** and only incremented by `trim_one`; the domain `AuditObservation` carries no port-side counter, so truncation performed by the audit port itself is invisible here. `normalize`'s package-accounting equality partially covers it, but the field reads as authoritative and is not.
- **`read_only(true)` annotation** on a tool whose own description says it "can execute project build scripts, proc macros and test code." Defensible for an ephemeral sandbox over a captured copy, but hosts use `readOnlyHint` for auto-approval — confirm it matches `rust.test`'s existing annotation; an inconsistency between the two is the actionable part.
- **`std::time::Instant` used directly in `crates/application/src/quality.rs`** for `duration_ms`, while the same function receives `Clock` and `RegistryClock` ports. Millisecond precision justifies it, but it makes stage durations non-deterministic under the mock suite; worth an explicit note or a ms-granularity port.
- **Minor, bounded:** `refresh` uses non-saturating `truncation.diffs_omitted += ...` (bounded by stage count); `encode_bounded` re-serializes the whole payload once per removed item (O(n²), up to several hundred passes over a ≤128 KiB document); the bootstrap override in `call` mutates `outcome`/`summary` without a following `refresh()` (coherent today only because `data` is `None`).

---

## What holds up well

Single capture is genuinely enforced and genuinely tested: `capture_validation` is called once and `profiles_use_one_capture_and_closed_options_even_after_validation_failure` asserts both `backend.captures.get() == 1` and pointer identity of the `&SourceBundle` across all six port calls. The quota path correctly reads `had_stdout`/`had_stderr` *before* clearing the streams, so an omitted non-empty log always forces `Incomplete → Blocked` and cannot yield a clean pass. Rollback attempts every staged ID even when one `remove` fails and downgrades to `Internal`, with earlier live-owner logs preserved on cancellation. There is no per-log touch: all lifetimes are rechecked at one final `clocks.1.seconds()` instant, followed by exactly one `touch_authorized_reference`, and the failed-batch tests confirm the lease is not renewed. Lifting `classify` into `AuditObservation::normalize` as shared pure domain policy is the right move, it is idempotent under the repeated `classify` calls across the three layers, and the publication-time snapshot re-assessment (`audit_fresh_at_execution_is_reassessed_after_log_publication`) closes the stale-audit-passes hole.
