# W08b — informe del worker

| Campo | Valor |
| --- | --- |
| Modelo | Claude Sonnet 5 (`claude -p --model sonnet --effort high --disallowedTools Agent Task`) |
| Inicio / fin (UTC) | 2026-09-12T21:44:54Z / 2026-09-12T23:19:42Z; 5687231 ms |
| Resultado | `subtype: success`, `is_error: False` |
| Transcript | fuera del árbol; hashes en `transcripts.sha256` |

Nueve P3 materiales de V08 cerrados: kind antes de `resolve` (`kind_of` peek),
grant antes del worker, paridad listado↔preview, saneo `is_peer_text_hazard`
(bidi/zero-width/separadores), mensaje de timeout de commit/receipt, `RESULT_LIMIT`
para el overflow de apply, vocabulario de audit documentado como abierto, y los
tres tests (dirección inversa por store, decode de `APPROVED_M6_IMAGE`, grant de
otra raíz bajo contención). Un snapshot regenerado (apply, por `RESULT_LIMIT`);
los otros 34 byte-idénticos.

## Informe literal del worker

All nine items from the W08b task are implemented and verified. Summary:

## Report

**Task:** Close the 9 material V08 P3s on the analyzer write path (`rust.analyzer.action.apply` + `rust.analyzer.actions`).

**Result:** All 9 items implemented; full targeted verification suite green.

**Files changed** (beyond pre-existing W07/W08 diffs already in the tree):
- `crates/mcp-server/src/stdio/mutation/analyzer_action.rs` — kind-of pre-check before `resolve` (1), grant check hoisted before `run_joined` (2), commit/receipt timeout wording (5), `ResultLimit` code (8), 6 new/extended tests
- `crates/application/src/mutation.rs` — new `MutationPlans::kind_of` peek (supports 1)
- `crates/application/src/analyzer.rs` — `listed_edits` now mirrors preview's no-op/`.rs`/bundle-limit checks via `apply_action_to_bundle` (3), +1 test
- `crates/mcp-server/src/stdio/analyzer.rs` / `analyzer/actions.rs` / `analyzer/tests.rs` — shared `is_peer_text_hazard` widened sanitizer (4), +3 tests
- `crates/mcp-server/src/stdio/mutation/audit.rs` — documented open `tool`/`reason`/`phase` vocabulary, no schema bump (6)
- `docs/tools.md` — ACTION_STALE-family code-mapping note (7), RESULT_LIMIT documented (8)
- `crates/project-adapter/tests/support/native_mutation.rs` — store-level foreign-kind `commit` refusal, both directions (9a)
- `crates/mcp-server/src/stdio/mutation.rs` — non-ignored `APPROVED_M6_IMAGE` decode test (9b)
- `crates/mcp-server/tests/protocol.rs` — concurrent-contention SANDBOX_DENIED test (2)
- `crates/mcp-server/tests/snapshots/analyzer-action-apply-tool.json` — regenerated for the new `RESULT_LIMIT` enum member (byte-verified against a live `tools/list`; `release-smoke.py`'s pinned hash already matched, unchanged)

**Tests executed:** fmt --check (clean), workspace check (clean), workspace clippy -D warnings (clean), domain analyzer (45 passed), application analyzer (31 passed), mcp analyzer (62 passed), mcp --bins (432 passed, 3 ignored), protocol+catalog_status+crate_inspect+crate_search (59+2+2+2 passed), analyzer_runtime --no-run (compiles), check-architecture.py (PASS), test-release-smoke.py (9 passed), docs-hygiene links-check (0 broken in living docs). The other 34 tool snapshots stayed byte-identical (asserted by the full `bootstrap()` comparison in every passing protocol test).

**Decisions:**
- Item 1: added a cheap `kind_of` peek rather than reordering around `resolve` (which has no side effects) — satisfies the literal ask and adds real defense-in-depth.
- Item 9a: verified via direct code reading + a debug probe that store-level `replay`/`receipt`/`recover` already gate on kind via `authorize_body`/`read_pair`; only `commit` lacked a *test* for the reverse direction (the code already gates it) — added that test rather than a code change.
- Item 6: chose "document as open vocabulary" over bumping `SCHEMA`, since `reason`/`tool` were already un-enumerated free strings — zero risk to the 5 pinned test/doc locations referencing `rust-mcp-mutation-event-v1`.
- Item 8: adopted `RESULT_LIMIT` (preferred option) for the apply-preview overflow path only; `LIMIT_EXCEEDED` still covers `MutationPlans`/journal budgets.

**Risks / open issues:** None outstanding. One self-caught mistake worth flagging for awareness: an early `Edit` accidentally dropped `audit.rs`'s `use` statements, and a debug-panic left in `filesystem/macos/mutation.rs` was fully reverted — both caught by the compile/test loop before this report, no trace remains in the diff.
