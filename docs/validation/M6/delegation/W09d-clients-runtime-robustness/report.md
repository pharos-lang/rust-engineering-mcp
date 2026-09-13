# W09d — arnés de clientes robusto a la no-determinación de assists del analyzer

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`) |
| Inicio / fin (UTC) | 2026-09-13T05:39:26Z / 2026-09-13T05:50:04Z |
| Resultado | 99 unit OK, `--run` verde; Inspector reintenta actions (3×), oráculo Claude best-effort en escritura |
| Origen | attempt-5: `actions` de Claude vino vacío (assist no-determinista); ver [[analyzer-assist-readiness]] |

---

## Task

W09d: make the M6 client harness robust to rust-analyzer's non-deterministic assist availability, per the orchestrator's binding diagnosis from `--with-runtime` attempt-5.

## Result

Both changes are implemented and both foreground checks are green. No commit made, no `--with-runtime` run attempted.

## Files changed

- `scripts/m6-inspector-session.mjs` — bounded retry on the `actions_capture` row.
- `scripts/test-m6-clients.py` — best-effort runtime oracle, `write_lifecycle` receipt fields, module + function docstring debt notes.
- `scripts/test-m6-clients-unit.py` — updated/added unit tests for the new oracle semantics.

## Exact retry / best-effort logic

**Inspector (`m6-inspector-session.mjs`)**: the `actions_capture` row's call is wrapped in a loop of up to `ACTIONS_CAPTURE_RETRIES = 3` attempts, `ACTIONS_CAPTURE_RETRY_DELAY_MS = 1500` apart. It retries only when the response is the specific empty-but-complete shape (`data.actions == []` and `data.completeness.state === "complete"`); any other result (real actions, or a non-`passed` status/error) stops the loop immediately via the existing status checks. After the third empty attempt it raises `"analyzer offered no assist after 3 retries — known assist-readiness race, W09d"`. No other row's behavior changed.

**Claude Code oracle (`validate_runtime_model_flow`)**: always requires, unchanged and strict — no foreign MCP capability, all four read facts, the one `actions` call, and the `bad_file` negative, each exactly once with the planned status/error_code (this is `RUNTIME_ALWAYS_FACT_KEYS`). It captures `action_digest` from the `actions` payload but now swallows the "no actions" failure into `state["action_digest"] = None` instead of raising. It counts how many times the write project was opened: exactly 1 (never reopened) or 2 (reopened once) are legal, anything else raises. The full write cycle (`apply_preview`/`apply_commit`/`apply_receipt`/`apply_stale`, `RUNTIME_WRITE_FACT_KEYS`) is validated with the original strict logic — including the hard `write_verified_on_disk` check — **only** when both `reopened` is true and `action_digest` is not `None`; that combination sets `write_lifecycle = "performed"`. Any other combination (empty actions, or a digest offered but never applied/reopened) sets `write_lifecycle = "skipped: no applicable action offered"` without inspecting the absent write calls further — it never downgrades a real failure in the deterministic facts into a pass. The same string surfaces as `claude_code[mode]["write_lifecycle"]` in the receipt; `inspector[RUNTIME]["write_lifecycle"]` is always `"performed"` (Inspector's retry guarantees it or the gate fails hard).

## Tests

- **Unit**: `python3 -B scripts/test-m6-clients-unit.py` → **99 passed**. Added/changed:
  - (a) `test_runtime_flow_accepts_a_skipped_write_when_actions_came_back_empty` — empty `actions` response, no write calls at all → valid, `write_lifecycle` skipped, `write_verified_on_disk` is `None`, no `apply_*` keys in `facts`.
  - (b) `test_runtime_flow_still_validates_the_full_write_cycle_when_offered` — full cycle present but the receipt's `state` is corrupted to `"pending"` → still raises (`"committed operation"`); `test_runtime_transcript_normalizes_and_validates` now also asserts `write_lifecycle == "performed"`.
  - (c) `test_runtime_flow_rejects_a_missing_read` — drops the `diagnostics` call → raises `"retried or omitted diagnostics"`. `test_runtime_flow_rejects_a_foreign_capability` (pre-existing) still covers the other-capability case.
  - Updated `test_runtime_flow_rejects_a_missing_reopen_after_commit` → renamed `test_runtime_flow_treats_a_missing_reopen_as_a_skipped_write`, since this scenario (digest offered, cycle never completed) is now a valid skip, not a failure, per the diagnosis's OR condition.
- **`--run`**: `python3 -B scripts/test-m6-clients.py --run` → **passed**, attempt-7, `status: "passed"`, `with_runtime: False`. (Had to move the stale `docs/validation/M6/clients.json` — an untracked leftover from an earlier attempt-3 in this same session — aside first, since the harness refuses to overwrite an existing current receipt; removed the backup after confirming the new run succeeded.)

## Risks

- The `write_lifecycle` skip condition is intentionally lenient on *why* the model didn't write (empty actions vs. digest offered but abandoned) — both collapse to the same skip string, matching the literal `performed | "skipped: no applicable action offered"` contract given, but it means a hypothetical case where the model reopened the write project without ever obtaining a digest is also silently treated as "skipped" rather than flagged as anomalous. This matches the task's explicit OR semantics but is worth knowing about.
- I did not touch the Docker-free matrix or the fingerprint/Tasks fixes from W09b/W09c.

## Open issues

- None from this pass. `--with-runtime` still needs to be run by the orchestrator with Docker to confirm the Inspector retry and the best-effort oracle behave as intended against the real intermittent assist race.
