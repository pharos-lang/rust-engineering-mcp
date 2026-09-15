# W13 — informe del worker (Claude Sonnet 5, High; claude 2.1.268)

Invocación: `claude -p --model sonnet --effort high … < prompt-header.md`. Inicio 2026-09-14T21:23:54Z, fin 2026-09-14T21:33:17Z, exit 0, 86 turnos, 562239 ms, permission_denials: 3. Entregable: [03-formats-analysis.md](../../03-formats-analysis.md).

---

All citations check out. The document is complete and verified. Here's the final summary:

## Task
W13-formats-analysis (M8-03 prep for D12): for each of the 10 disk formats in the M8-01 census, determine version markers, fail-closed behavior on unknown versions, byte/structure changes since `v0.3.0`, non-regression state (floors/trust), recovery tooling, and existing tests — reading code only, writing one document.

## Result
Wrote `docs/validation/M8/03-formats-analysis.md`: per-format analysis (6 questions × 10 formats) with `file:line` citations, a 10×6 summary table, migration list, D12 gap list ranked by risk (S/M/L cost), and a fixture-coverage table against the plan's list.

## Key findings
- **No format changed bytes/structure between `v0.3.0` and HEAD.** 8 of 10 formats are byte-identical in their reader/writer source; the other two (host config, M2 journal) changed only additively (new M6 flag/image, new closed-enum variant). → **No formats require a 0.3.0→0.8.0 migration; per plan, no migration CLI should be added.**
- Correction to census finding F7: the M2 mutation journal *does* carry a per-record version marker — a `format: String` field with a versioned magic string (`...-journal-v2`/`-v1`), just not named `format_version`. It's already fail-closed on unknown formats and has real v1→v2 upgrade tests.
- The only genuine antirollback **floor** found in code is `SequenceFloor` for the catalog trust bundle (`bundle/floor.rs`); RustSec and vendor-tree are marked `floor_or_trust_state: true` in the census but only have per-invocation integrity pins, not monotonic floors — worth clarifying in D12.
- Biggest real gap: no test exercises an actual old-binary-vs-new-state (or reverse) rollback for any format — only in-binary "unknown format version" equivalents.

## Files changed
`docs/validation/M8/03-formats-analysis.md` (new) — the only file this worker is permitted to write.

## Counts
10/10 formats analyzed; `links-check` → 0 broken in living documents.

## Risks / Open issues
- No fixture found for "permissions revoked mid-operation" or a protocol-level "crash after commit, before MCP response" test.
- RustSec staleness degrades rather than fail-closes (asymmetric vs. other formats) — flagged as a D12 decision point, not a code defect.

No commit was made, per instructions.
