# W09b — oráculo docker-free de actions/apply: usar el fingerprint real (SANDBOX_DENIED uniforme)

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`) |
| CLI | `claude` 2.1.267 |
| Inicio / fin (UTC) | 2026-09-13T05:05:21Z / 2026-09-13T05:16:39Z |
| Resultado | `subtype: success`; fingerprint corregido (96/96 unit, 5/5 docker-free SANDBOX_DENIED en Inspector y Claude Code); `--run` aún rojo por un 2º defecto de W09 (Tasks advertisement) → W09c |
| Origen | El orquestador reprodujo: `actions` con fingerprint real + socket muerto → `SANDBOX_DENIED`; con placeholder → `blocked/CONFLICT`. W09 asumió (mal) que SANDBOX_DENIED iba primero. |

---

Good, these are harness-generated evidence artifacts (already untracked before my run — `attempt-1` existed from the orchestrator's reproduction). I'm not committing anything, per instructions.

## Report

**Task:** Fix the docker-free oracle for `rust.analyzer.actions`/`rust.analyzer.action.apply` in the M6 client harness — the two write-lane rows were sending `PLACEHOLDER_FINGERPRINT`, which the host rejects as `blocked/CONFLICT` before ever reaching the (dead) runtime socket, breaking the uniform `unavailable/SANDBOX_DENIED` the docker-free matrix is meant to prove.

**Result:** Fixed and verified end-to-end. Unit tests pass (96/96). A real `--run` reached both the Inspector and stock Claude Code (Sonnet 5) docker-free sessions successfully — all five rows for both clients now answer `unavailable/SANDBOX_DENIED`, and the private Docker socket is confirmed never created (`docker_free_socket_created: false`). The run's overall exit is still red, but only because of an unrelated, pre-existing defect (see Open issues) — it is not the fingerprint bug and predates this change (reproduced identically in `attempt-1`, captured before my edits).

**Files changed:**
- `scripts/test-m6-clients.py`
- `scripts/m6-inspector-session.mjs`
- `scripts/test-m6-clients-unit.py`

**How the real fingerprint is captured and substituted (both drivers):**
- `PLACEHOLDER_FINGERPRINT` is gone. The two `CALL_PLAN` rows for `rust.analyzer.actions`/`.action.apply` no longer carry any `expected_project_fingerprint`; they're tagged `"kind": "docker_free_fingerprint"`. `check_expectation()` now refuses any row that hard-codes `expected_project_fingerprint` (top-level or nested in `action`), and `call_plan()` asserts the two fingerprint tools always carry that kind (and only those two).
- **Inspector (`m6-inspector-session.mjs`):** the initial project-open loop now records every opened root's `data.fingerprint` into `state.fingerprints`, not just `"write"`. A new `assembleArguments` branch for `kind === "docker_free_fingerprint"` reads `state.fingerprints[row.project]` (the `analyzer_actions` root) and injects it at the right spot — top-level for `actions`, nested under `action` for `apply` — leaving `action_digest: PLACEHOLDER_DIGEST` untouched (never reached: the runtime dial fails first).
- **Claude Code (`claude_prompt`/oracle):** the docker-free prompt now tells the model to keep `data.fingerprint` from every `rust.project.open`, and its per-tool instructions for `actions`/`action.apply` explicitly say to use "the fingerprint returned for the `analyzer_actions` root" as `expected_project_fingerprint`. The oracle (`opened_references`, extended to also return captured fingerprints; `validate_docker_free_model_flow`) no longer expects a fixed constant — it pulls the real fingerprint from that same transcript's own `project.open` result and builds the expected arguments dynamically (`_with_captured_fingerprint`), failing loudly if the tool call's fingerprint doesn't match what was actually captured, or if no fingerprint was captured at all.
- Confirmed live: the real Sonnet 5 transcript in `attempt-2` shows the model captured `sha256:ab5fe8251f...` from opening `analyzer-actions` and used that exact value for both `rust.analyzer.actions` and `rust.analyzer.action.apply`, both returning `SANDBOX_DENIED`.

**Tests:**
- `python3 -B scripts/test-m6-clients-unit.py` → `Ran 96 tests in 2.890s — OK` (added tests for: fingerprint-kind consistency, hard-coded-fingerprint refusal, mismatch between a call's fingerprint and the one captured from its own `project.open`, and a missing captured fingerprint).
- `python3 -B scripts/test-m6-clients.py --run` → Inspector docker-free: all 5 rows `unavailable/SANDBOX_DENIED`, discovery/tool_count correct. Claude Code docker-free: all 5 rows `unavailable/SANDBOX_DENIED` with the real captured fingerprint. `docker_free_socket_created: false`. Overall run still exits non-zero — see Open issues.

**Risks:** Low. The change only affects the docker-free plan/prompt/oracle for two tools; the three read-only docker-free rows and the entire runtime matrix are untouched (verified by the unmodified `RUNTIME_CALL_PLAN`/`runtime_call_plan()` tests still passing, and this task never touched `--with-runtime`).

**Open issues (out of scope for W09b, flagging for the orchestrator):**
- `--run` now fails past the fingerprint fix, at `validate_protocol_metadata` → `m3.protocol_summary`: `RuntimeError: unexpected modern Tasks advertisement for inspector`. `protocol.jsonl` shows both the Inspector and Claude Code sessions negotiate `tasks_advertised: true` from the server, while the M6 harness's `client_versions()` still hard-codes `"tasks": false` and calls `protocol_summary(path, False)`. This is pre-existing: `attempt-1` (captured before my edit, when the run died earlier on the CONFLICT bug) already shows `tasks_advertised: true` in its `protocol.jsonl`. It looks like the built server candidate now unconditionally advertises the Tasks capability and the M6 gate's assumption is stale — a separate defect from this ticket's scope, needs its own fix/decision before `--run` can go fully green.
