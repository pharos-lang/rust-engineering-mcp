# W31 — `--with-runtime` two-session fix: report

## Root causes found

The reported failure (`RuntimeError: rust.project.inspect did not land on a
structured refusal: passed` in `validate_negative_rows`) had **two**
independent causes in `scripts/test-m8-clients.py`'s `run()`/`run_inspector()`:

1. **Socket sharing.** `run()` computed a single `docker_socket` variable:
   a fake, never-created socket when `--with-runtime` was absent, but the
   *real* absolute `--docker-socket` when `--with-runtime` was present — and
   fed that same variable into the `docker_free` Inspector session's own
   `server_argv`. So under `--with-runtime`, the "Docker-free" session ran
   against a host that actually had a calibrated runtime, and every
   `TOOL_NOT_INSTALLED`-style refusal in the 30-row negative plan instead
   landed `passed`.
2. **Negative plan reused across modes.** Independent of (1),
   `run_inspector()` built the *same* `plan` payload (`negative_call_plan()` +
   `generic_negative_plan()`) for both `docker_free` and `runtime` modes, and
   `scripts/m8-inspector-session.mjs` executed that plan unconditionally. So
   even with the socket bug fixed, a `runtime` session would still replay the
   Docker-free refusal plan against a host that *has* a real runtime.

## Fixes applied (all three permitted files)

- `scripts/test-m8-clients.py`:
  - `run()` now derives two distinct sockets: `docker_free_socket` (always
    the never-created path, regardless of `--with-runtime`) feeds the
    `docker_free` Inspector session; `model_turn_socket` (the real socket
    under `--with-runtime`, else the same Docker-free one) feeds Codex/Claude
    Code/Gemini CLI — matching the design ("los turnos de modelo... en modo
    runtime hacen su flujo positivo con runtime"). The `runtime` Inspector
    session keeps using the real `--docker-socket` as before.
  - `run_inspector()` now only builds/validates the negative call plan
    (`negative_call_plan()`, `generic_negative_plan()`,
    `validate_negative_rows`, `validate_generic_negative_rows`,
    `generic_negative_wire_confirmed`) when `mode == DOCKER_FREE`. A
    `runtime` session that still reports any negative/generic-negative rows
    now raises explicitly ("a runtime session must not run the Docker-free
    negative plan") instead of silently mixing evidence.
  - `main()`'s CLI gate previously rejected `--preflight --with-runtime`
    outright (`--with-runtime requires --run`), which contradicted the
    explicit verification ask. Relaxed to `--with-runtime requires --run or
    --preflight`.
  - Module docstring updated to describe the two-session design accurately.
- `scripts/m8-inspector-session.mjs`: the top-of-file plan validation no
  longer demands a non-empty `negative_rows` for a `runtime` session (Python
  now sends `[]` for that mode), and the negative/generic-negative call loops
  are skipped entirely when `plan.mode === "runtime"` — that session only
  composes its own positive oracle (open → `rust.check` → Resource read →
  cancel-then-retry).
- `scripts/test-m8-clients-unit.py`: added `RunInspectorModeSeparationTests`
  (plan construction never carries negative rows for `runtime`; a completed
  `runtime` session reports empty negative evidence; a `runtime` session that
  *still* ran a negative/generic-negative row is refused),
  `WithRuntimeHostSeparationTests` (the `docker_free` Inspector session never
  sees the real socket under `--with-runtime`; model turns move to the real
  socket only under `--with-runtime`), and
  `test_preflight_with_runtime_is_accepted_by_main`.

## Verification

- **Full unit suite**: `python3 -B scripts/test-m8-clients-unit.py` →
  **106/106 passed** (98 pre-existing + 8 new), clean output (no stray
  stdout from the new CLI-gate test, which redirects `main()`'s print).
- **Regression proof**: extracted the pre-fix `scripts/test-m8-clients.py`
  from `HEAD` into a scratch copy and ran the two new socket/plan-separation
  tests against it directly — they fail exactly as expected
  (`docker_free_socket == real_socket`, i.e. the reported bug), confirming
  the new tests actually catch the regression and aren't vacuous.
- **`--preflight --with-runtime`**: now runs to completion (previously
  raised `--with-runtime requires --run` unconditionally). Against the real
  socket (`/Users/cburgosro/.docker/run/docker.sock`), `docker_socket` is
  now `satisfied: true` (it was the blocking precondition before this fix).
  Overall status was `blocked` only on `gemini_version` (host `agy` reports
  `1.2.3`, pinned `1.2.2`) — confirmed this is a pre-existing, unrelated
  environment drift by running plain `--preflight` (no `--with-runtime`),
  which is `blocked` on the exact same single precondition.
- **Bounded real Inspector `runtime` session** (open + check + resources/read
  + cancel), run directly via `run_inspector(..., M8.RUNTIME, ...)` against
  the real Docker socket, never the full `--run --with-runtime` matrix:
  - `server/discover` negotiated 2026-07-28 with `tasks_advertised: true`.
  - `tools/list` returned all 36 tools; `resources/list` was empty.
  - `rust.project.open` → **passed**.
  - `rust.check` → **passed** (a real container ran; this is the first time
    in the M8-04 matrix that `rust.check` has actually executed rather than
    refused for lack of runtime), publishing a real `rust-artifact://` URI.
  - `resources/read` on that URI → **`-32601 Unknown method`**, an
    unhandled rejection that crashed the Node session (exit 1) before the
    cancel-then-retry step could run.
  - Traced the `resources/read` failure to the checked-out
    `target/release/rust-engineering-mcp` binary being **stale**: its mtime
    (`2026-09-14 21:02`) predates commit `be0ed21` (`21:55:32`, "fix(m8):
    resources/list, resources/templates/list and prompts/list carry
    ttlMs/cacheScope"), which touched `stdio.rs`, `capability_document.rs`
    and `resources.rs` — the exact files behind resource dispatch. This is
    a build-freshness issue, not a defect in this fix or in the harness's
    own oracle; confirmed by isolating the wire exchange (protocol.jsonl
    shows `rust.check` passing, then `resources/read`'s 75-byte server
    response — the size of a bare JSON-RPC `MethodNotFound` error — with no
    further Inspector traffic after it).
  - **Action for the orchestrator**: rebuild `target/release/rust-engineering-mcp`
    from current `HEAD` before running the full `--run --with-runtime`
    matrix, or the `runtime` Inspector session's Resource-read/cancel oracle
    will fail against a stale candidate for reasons unrelated to this fix.

## Scope notes (not touched, out of the three permitted files)

- `claude_gate()`'s own docstring says "(with a real runtime) one Resource
  read", but its fixed 5-step prompt never asks for one regardless of
  `with_runtime` — a pre-existing inconsistency, unrelated to the reported
  fallo; left untouched.
- Did not rebuild the release binary or touch any Rust source — outside the
  permitted files and this delegation's scope.
