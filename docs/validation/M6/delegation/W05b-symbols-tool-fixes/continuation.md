# W05c — Continuation of W05b (the W05b session ended without a report)

Model requested: Claude Sonnet 5 (`claude -p --model sonnet --effort high`). Same ownership and constraints as `prompt-header.md` in this directory (read it and the V05 disposition first). You may not spawn subagents. Do not commit. Do not wait on background tasks: run every command in the foreground and finish with the report as your last message.

The previous session applied most items but ended while "waiting for a background cargo check". Orchestrator audit of the tree (2026-09-12 12:10 UTC):

| Item | State in tree | What you do |
| --- | --- | --- |
| 1 bootstrap doc + test | present (`tools.md`, `bootstrap_refusal_is_blocked_sandbox_denied_with_no_data`) | verify only |
| 2 `RESULT_LIMIT` → `unavailable` | present | verify the test exists |
| 3 effective limits | present (`wire_limits(total)`) | verify the test exists and that the gateway budgets agree |
| 4 peer-text bounds | `MAX_PEER_NAME_CHARS`/`MAX_PEER_DETAIL_CHARS`, `OversizedEntry`, `detail_truncated` present | verify codec tests (oversized name, control char, long detail) and schema `maxLength` exist; add what is missing |
| 5 quarantine doc | present | verify |
| 6 digest before `with_gateway` | present | verify |
| 7 input schema pattern | present in code | — |
| 8 trim margin | `encode_bounded_within` + test budget | verify the worst-case 512-symbol test exists |
| 9 README | present | verify |
| **10 snapshot + pins** | **NOT regenerated**: `tests/snapshots/analyzer-symbols-tool.json` and `scripts/release-smoke.py` predate the schema changes of items 4 and 7 | regenerate the snapshot from the real `tools/list` (the same way W05 did), update the hash in `release-smoke.py`/`test-release-smoke.py`, assert the other 31 are byte-identical |
| 11 native test handshake | the test uses the 2026-07-28 `_meta` convention of `tests/inspection_runtime.rs` (no `initialize`) — the orchestrator's earlier verdict (H) assumed a missing `initialize`; if `inspection_runtime.rs` really passes with that convention, the cause of `Disconnected` is something else | read `tests/inspection_runtime.rs` and `tests/protocol.rs` (`first_project_call_preserves_bounded_bootstrap_behavior`, `modern()`); make the analyzer test follow **exactly** the same first-frame sequence as `inspection_runtime.rs` (whatever it is); explain in the report what differed. Do not run it (no Docker for you) |

Then run the full targeted verification list of `prompt-header.md` and report with the mandatory headings (Task / Result / Files changed / Tests executed (counts) / Evidence (new snapshot hash) / Risks / Decisions / Open issues).

## Orchestrator's test run on this tree (12:20 UTC)

`cargo fmt --all -- --check` fails (an unformatted test in `stdio/analyzer/tests.rs`); `cargo test -p rust-engineering-mcp analyzer` → 14 passed, 1 failed; `--test protocol` → 17 passed, **30 failed** (stale snapshot after the schema changes); codec 48/48; domain 35/35; `test-release-smoke.py` OK. Failing tests seen: . Fix all of it.
