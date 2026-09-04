# Pre-freeze independent review, disposition pending

Explicit Claude Code2.1.259 / claude-opus-5 / high, tools disabled.
Substantive review model confirmed by modelUsage; separate CLI auxiliary Haiku4.5 call retained in raw receipt.
This review received an earlier controller packet without the subsequently completed runner, fixed transport, updated join checks and completed qualifications. Findings require principal verification; severity is the reviewer's assessment, not yet the principal disposition. No utility measurements may start before that disposition and required fixes.

## Scope and method

Review of the pasted text only — no tools, no reads, no execution. I did not verify that the on‑disk files match what was pasted, and I could not inspect `rust_gateway.rs`, the MCP server, the prompts, or the corpus. Findings that depend on unseen components are tagged **[verify]**. I accept your stated threat model (no hostile OS‑local‑writer, controlled inputs, no environment agent) and I do not treat the V8 API confinement as an OS sandbox in either direction.

---

## Critical

**C1 — Cleanup verification cannot fail on the cancellation path (false claim).**
`broker.py:192-194` computes `joined` from an acknowledgement line and `code == 0 or (cancelled and code == 1)`. On cancellation the driver returns `Err("cancelled")` and `main.rs:591` prints `{"driver_error":"cancelled", ..., "execution_joined": true}` and exits 1. `broker.py:158-164` and `186-191` scan lines only for `execution_joined is True` — unlike `_response` (`broker.py:136`), they do **not** reject a payload containing `driver_error`. So every cancelled run yields `cleanup_failed: False` and `server_cleanup_verified: True` (`broker.py:196-198`). Cancellation at the 900 s wall is the *expected* termination path, so the declared infrastructure‑failure criterion is inert exactly where it matters.

**C2 — `execution_joined` is not evidence of gateway/container teardown.**
`main.rs:566` sets the flag unconditionally after the loop breaks, and `main.rs:578` emits the literal `"execution_joined":true` without reading the atomic. The `RustGateway` held in the mutex (`main.rs:481, 519`) is never explicitly dropped or joined before that line; any container teardown in `Drop` runs at process exit, after the acknowledgement was already printed and consumed. ADR‑046:60‑61 (“joins handler, SDK server and gateway cleanup”) and `broker.py:56` are therefore stronger than the code supports for the raw arm.

**C3 — `server_cleanup_verified` ignores the driver's own `server_joined` report.**
`main.rs:458/578/591` reports `server_joined` separately, but `broker.py:192-198` never reads it. In MCP mode a failed join of the product server child is reported as verified cleanup whenever the exit‑code test passes (always true on the cancelled path per C1). `server_cleanup_verified` must require `server_joined is True` in MCP mode.

---

## High

**H1 — Participant process cwd and evidence directory sit inside the product repository.**
`participant.py:198-204` puts the app‑server cwd at `output_dir/neutral`, and the qualification artifact (`target/m1-16-controller/timeout-qualification/receipt.json`) shows this was under the repo tree. Consequences: (a) the model process's cwd has ancestors containing the protocol, ADR, reference patches, oracle expectations and hidden tests; (b) `events.jsonl` and `receipt.json` — full tool request/response bodies, including submitted candidates — live in the cwd's immediate parent. Under your threat model nothing reads them, but the app‑server itself (outside V8) walks ancestors for config layers (`M1-16-controller-notes.md:68-72`), so containment rests on one reviewed component with no second barrier. Move the neutral cwd and the output dir outside the repo, and assert at startup that no ancestor of the cwd contains the corpus, oracles or evidence.

**H2 — One malformed model tool call terminates the run; contradicts the protocol.**
`participant.py:277-284`: any shape miss (unknown name, extra key, missing required) produces a non‑retryable `-32601` **and** `interrupt()`. Protocol §*Execution budget* (`M1-16-protocol.md:209`) states invalid requests “receive retryable denials”. The broker's own `Denied` layer *is* retryable (`broker.py:511`), so the two layers have opposite semantics and the fatal one is upstream. The failure probability is arm‑dependent: arm B exposes 14 tools with the product's richer schemas (`broker.py:440-449`) versus arm A's 6 broker‑authored ones. Differential attrition on n=1 items is a direct threat to the primary comparison, and the receipt (`denied_request`) maps to no protocol failure category.

**H3 — Native‑tool suppression is a denylist with no positive inventory assertion.**
`participant.py:25-26, 242` check a hardcoded list of feature names. A build that adds a capability not in `DISABLED` passes every guard silently, and `participant.py:20` hardcodes the CLI path with no version or binary‑hash check at runtime (the manifest requirement lives only in prose, `M1-16-controller-notes.md:76`). Combined with `HOME`/`CODEX_HOME` in the child env (`participant.py:59`) and `sandbox:'read-only'` (`participant.py:254`) — which permits reads, as your own notes warn (`:53-56`) — an unenumerated native tool would neither round‑trip to the controller nor appear in `events.jsonl`, so the ADR's “all unadmitted app‑server requests fail closed” (ADR‑046:29) covers only the server→client direction. Assert the effective tool inventory equals the admitted set, and abort on any feature key present in effective config but absent from a frozen expected set.

**H4 — Legitimate large output aborts the run at three independent layers.**
Raw execution retains 256 KiB *per stream* (`main.rs:523`), so one stage can approach 512 KiB pre‑escaping and a `quality` bundle carries four (`broker.py:470-479`). Caps: `broker.py:507` (`MAX_REPLY` = 524288, raises `BrokerError` → fatal), `participant.py:292` (same number, raises → `interrupt()`), `main.rs:312` (1 MiB → `driver_error` → `driver_failed` → `cancel_and_join`). A verbose but ordinary compile is therefore likely to end an arm‑A run as infrastructure failure, and `raw_validate('quality')` — the declared analog of `rust.quality.gate` — may be unusable in practice. Truncate deterministically with a recorded truncation flag under one policy applied identically to both arms.

**H5 — Protocol rules are disclosed asymmetrically through tool descriptions.**
`broker.py:430` tells arm A “Six validation requests total” and “strict clippy”; arm B receives the product's verbatim descriptions (`broker.py:446`), which do not state the six‑cycle cap, the mandatory `lint_profile='strict'` (`broker.py:569`) or the ≤30 s test timeout (`broker.py:567`). Arm B must spend calls discovering rules arm A is handed. Iteration count is the primary metric, so this directly biases it. Put every budget and protocol constraint in the shared prompt, verbatim and identical, and keep descriptions free of them.

**H6 — Run evidence does not bind the prompt, the tool declarations or the binaries.**
`participant.py:202-203` builds the report with no `prompt_sha256`; `validate_tools` (`:39-53`) persists nothing; the receipt has no CLI version/hash, no config hash, no driver binary hash. `broker.py:35` executes `target/debug/m1-16-trusted-driver` with no integrity check. Worse, the entire controller (`participant.py`, `broker.py`, `main.rs`) and the catalog projection live under `target/` — a disposable, git‑ignored build directory absent from the repository. The freeze prerequisite “freezes exact source commit, controller/broker binaries/scripts” (`M1-16-protocol.md:174-177`) is unenforceable from the artifacts as they stand, and `cargo clean` destroys both the code and the receipts.

**H7 — The broker issues a semantic claim it elsewhere disclaims, to one arm only.**
`broker.py:482-492` parses `Cargo.lock`/`Cargo.toml` in Python and returns `lock_audit_fact.std_only_locked_closure` to arm A. `M1-16-protocol.md:116` states the broker “does not parse Rust or claim those semantic properties”; this is the same class of claim by a different parser, and only arm A sees it (arm B gets `rust.dependencies.audit`). Also `third_party_locked_packages` (`broker.py:490`) is `0` whenever `std_only` is true and `None` otherwise — it carries no information and reads as if it did. Either drop the fact or produce it identically in both arms outside participant feedback (as protocol:120 already does for scoring).

**H8 — [verify] Arm B may operate on the live workspace while arm A operates on a snapshot bundle.**
`broker.py:399` binds the MCP server's `--root` to `workspace.root`, and `main.rs:379-386` launches it with `current_dir(&init.root)`, whereas arm A ships `workspace.snapshot()` per execution (`broker.py:477`). If the server compiles against the live root: (a) arm B accumulates a build cache across validations and arm A does not, biasing elapsed/gateway time and possibly iteration behaviour; (b) any cargo path that rewrites `Cargo.lock` trips `immutable_file_changed` (`broker.py:357-358`, re‑checked before every MCP call at `:571`) and ends the arm‑B run as infrastructure failure, while arm A is structurally immune. Determine the server's execution model before freeze; if it is live‑root, this is outcome‑determining.

---

## Medium

**M1 — Cancellation mid‑handler discards a committed state change.** `participant.py:291-298` and `broker.py:504-506` both discard a *successful* result if `cancel` was set during the handler, after `workspace.submit` already wrote the artifact and appended the candidate (`broker.py:530`, `broker.py:366-381`). The broker receipt then lists a candidate the participant log records as failed — precisely at the first/final‑candidate boundary the oracle protocol depends on (`M1-16-protocol.md:105-107`).

**M2 — Candidate indices diverge between artifacts and receipt.** The archive filename uses `len(workspace.candidates)+1` (`broker.py:372`) while the receipt entry uses `len(self.submissions)+1` (`broker.py:531`), and `submit_selection` returns yet a third number (`broker.py:549` vs `:543`). Both submit tools are exposed for both task types (`broker.py:419-427`), so a repair run can consume selection budget and desynchronise the indices.

**M3 — Cleanup exceptions destroy the receipt.** `Transport.close()` calls `self.processes()` with `check=True, timeout=2` at `participant.py:142` and `:164`; a `ps` failure raises out of the `finally` block at `participant.py:328`, so `receipt.json` (`:335-338`) is never written and the original failure is lost. `participant.py:165` also iterates `self.observed` unguarded while the monitor thread may still mutate it (`:115`).

**M4 — Cross‑run carryover is possible.** Freshness is checked only via `receipt.json` (`participant.py:199`), `neutral.mkdir(exist_ok=True)` (`:200`) reuses an existing directory, and `events.jsonl` is opened append‑only (`:222`). A run that died before writing its receipt leaves state that the next run silently inherits — contrary to `M1-16-protocol.md:180`.

**M5 — `allow_project_code=False` degrades a run instead of failing setup.** `broker.py:454-455` raises `Denied` (retryable), so a misconfigured consent flag produces a full‑length run with zero validations that is indistinguishable from model behaviour. This should be a construction‑time `BrokerError`.

**M6 — Backpressure is converted into terminal transport failure.** `participant.py:62, 86-87`: a 32‑slot queue with a 0.2 s put timeout fails the run when the main thread is blocked in a long synchronous handler. Bound the queue by bytes against the existing 16 MiB budget instead of by slots.

**M7 — The oracle's file surface is reachable from the participant‑mode driver.** `main.rs:189-199` admits `tests/behavior.rs` in the same mode and binary used for participant runs; only the broker's `FILES` tuple (`broker.py:26`) keeps hidden tests out. Add an explicit oracle mode that participant‑mode init rejects.

**M8 — Provider‑side prompt caching couples pair members.** Pair members run serially on the same task (`M1-16-protocol.md:75`) and the receipt already reports `cachedInputTokens`. Position‑in‑pair may systematically affect latency and cached‑token share. Order is counterbalanced 6/6, but record position explicitly and treat time as position‑confounded.

**M9 — Arm A cannot satisfy the selection acceptance criterion as configured.** Acceptance requires `snapshot_fingerprint` and `provenance.source_id` (`M1-16-protocol.md:126`), and `:147-151` records that the baseline projection has no real fingerprint. `broker.py:402-404` validates only the projection's size. Assert the projection carries every acceptance field at construction so a gap fails as setup, not as an arm‑A task failure.

**M10 — MCP server enumeration is regex over one file.** `participant.py:182-187` matches `[mcp_servers.NAME]` lines only, missing quoted/dotted keys, inline tables, and project/managed layers your notes flag as separate (`M1-16-controller-notes.md:68-72`). The effective‑config check (`:247`) fails closed today, but the override list should not be presented as the control.

**M11 — Watchdog interrupt can deadlock the receipt path.** `participant.py:212` writes to the child's stdin under the shared lock (`:122-123`); if the pipe is full the watchdog blocks, and `watcher.join()` at `:326` has no timeout, so no receipt is written.

**M12 — Cumulative driver transport budget is arm‑asymmetric.** `broker.py:98-100` never resets `self.total`; 16 MiB across up to 64 MCP responses is reachable in arm B and much less so in arm A's bounded raw stages, terminating the richer arm as infrastructure failure.

**M13 — Cleanup deadlines disagree across layers.** The driver may join for 300 s (`broker.py:59`) while the participant's post‑cancel loop allows 30 s (`participant.py:267-268`), so a long but correct driver join returns into an immediate `turn_cleanup_timeout` and is misclassified.

**M14 — [verify] Run content may persist outside the evidence closure.** `CODEX_HOME` is passed through (`participant.py:59`) and `ephemeral: True` is set (`:252`). Confirm that ephemeral suppresses session/rollout persistence; otherwise prompts, model output and submitted candidates are written to an unmanaged location.

---

## Low

- **L1** `participant.py:329-330` and `:336-337` overwrite `status`, collapsing task‑vs‑infrastructure failure (`M1-16-protocol.md:134`) and destroying the outcome on receipt overflow. Add orthogonal flags.
- **L2** `broker.py:577` reports `min(64, self.calls)` — the receipt cannot show an overflow that occurred.
- **L3** No `stop_reason`: `interrupt()` (`participant.py:210`) sets `admission_stopped` for wall, token, denial and handler failure alike.
- **L4** The 30 000 threshold reads `total.outputTokens` (`participant.py:307`); the qualification receipt shows `reasoningOutputTokens` as a separate field. Fix and record the definition — arms may reason differently.
- **L5** Messages consumed during `rpc` (`participant.py:230-232`) are never written to `events.jsonl`; the pre‑turn phase has no event record.
- **L6** `broker.py:372-380` writes the archive before `_check`/`os.replace` and appends only on success; a mid‑sequence failure leaves an orphan file whose `O_EXCL` collision surfaces as an unwrapped `OSError`.
- **L7** Broker denials are wrapped in `success: True` (`participant.py:294`).
- **L8** The string RPC id `'interrupt'` (`participant.py:212`) is untested against the app‑server's id typing; a rejected interrupt fails silently.
- **L9** ADR‑046:38 says “13 unchanged MCP tools”, but names are rewritten dot→underscore (`broker.py:441`) and a 14th broker‑authored `resource_read` is added (`:448`). Record the reversible mapping as evidence, as your notes require.
- **L10** Warm Docker/cargo caches favour the second pair member; order is counterbalanced, but timing comparisons should carry the caveat.

---

## Claims to correct before freeze

1. **`README.md:41-43`** — “Twelve fake‑transport tests pass” plus one timeout qualification. The qualification receipt shows `tool_calls: []`: **no end‑to‑end dynamic‑tool round trip against the real app‑server has occurred**, so `dynamicTools` acceptance on this build remains untested. Your own notes (`M1-16-controller-notes.md:129-135`) require that smoke and require stopping if dynamic tools are rejected. ADR‑046:79 (“an actual agent workflow through the SDK”) is ahead of the evidence.
2. **`README.md:9`** — “never … falsely reports cleanup complete” is not supported at the driver layer (C1–C3).
3. **ADR‑046:60-61** — “joins … gateway cleanup” is not evidenced for the raw arm (C2).
4. **ADR‑046:25 vs `participant.py:25/180`** — `features.code_mode=false` while `features.code_mode_host=true`. State precisely which feature delivers the callbacks, because the entire V8 confinement argument is bound to that mechanism, and the qualification run did not exercise it.
5. **`M1-16-protocol.md:61`** — “identical byte limits” is not implemented as equal information budget; the only shared cap is `MAX_REPLY`, and per‑stage raw truncation differs from whatever the MCP server applies internally (**[verify]**).

---

## Verify before freeze (inferred, not confirmable from the pasted material)

- Whether the MCP server compiles against the live workspace root or a copy (H8).
- Whether `rust.project.open` performs compilation — if so it is an uncounted execution channel available only to arm B (`M1-16-protocol.md:203-205`).
- Whether `ephemeral` suppresses on‑disk session persistence under the real `CODEX_HOME` (M14).
- Whether any cargo path in either arm rewrites `Cargo.lock` (H8b).
- That the two arms' prompts differ only by a declared tool‑name substitution — with both hashes recorded (H5, H6).
