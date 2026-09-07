M3 is now one review away from closure. Everything below comes from commands run in this session.

## Task

Close G1 (per-tool `CreateTaskResult` proof), produce the G6 upgrade/rollback receipt, run the consolidated gates to completion, and update the closure records.

## Result per item

**1. G1 — contract gap closed.** `task_materialization_requested` (`crates/mcp-server/src/stdio.rs:299`) matches four distinct message strings; only nextest proved end-to-end that a declaring peer gets a `CreateTaskResult`. Added three Docker selections in `tasks_runtime`, mirroring the nextest ones (nextest proves it via Docker, so these are Docker too). Each asserts the full envelope — `taskId` `job_…` of 36 chars, `status: working`, `ttlMs: 7200000`, `pollIntervalMs: 1000`, `createdAt == lastUpdatedAt`, non-empty `statusMessage`, and absence of `content`/`structuredContent` — then cancels inside the admission window and waits for a clean terminal `cancelled`. Mutation additionally covers `auto`, its only reachable production path (budget floor clamps to ≥300 s).

I proved the tests can fail: deleting the `"Tasks are not enabled for coverage"` arm made the coverage selection fail with `-32603` instead of creating a task. The file was restored from the pre-edit copy and its SHA-256 is in the passing full-gate inventory.

**2. G6 — receipt produced.** `docs/validation/M3-06-rollback.md` + `M3-06-rollback.json`: **10/10 selections, exit 0**, 6 new + 4 reused (named in the doc). Proved: an unknown/newer `format_version` fails closed and its exact bytes are preserved in quarantine, never rewritten to v1 or deleted; a future sibling `rust-mcp-quality-artifacts-v2/` is never read, migrated or removed across a full v1 lifecycle including both operator commands; `prune` reclaims only expired objects and never quarantines or touches an object it doesn't understand, while `recover` quarantines with `unknown_name` and keeps the bytes; a clock regression blocks `prune` with `recovery_required` (exit 1) until `recover` re-bases it; the M2 store and journals are byte-identical throughout.

One honest limitation surfaced and documented rather than papered over: a `clock-watermark.json` written by a *future* version leaves the store inoperable for `recover` too (all three entry points return `InvalidDescriptor`). The remedy is roll-forward or restoring the record this binary understands. Nothing is rewritten or deleted by the refusal.

**3. Gates — all passed on the final bytes.**

| Command | Exit | Result | Duration | Receipt SHA-256 |
| --- | ---: | --- | ---: | --- |
| `check-architecture.py` | 0 | PASS | 0.099 s (in full) | console |
| `gate.py core` | 0 | **14/14** | 408.568 s | `b31213fa…48b0bd3a` |
| `gate.py full` | 0 | **25/25** | 2,530.937 s | `124881e1…c814f3d8` |

`audit-data` — W5's reproducible blocker — **passed in 43.228 s**, unweakened. Both gates report `source_inputs_unchanged: true` over 801 inputs / 45,966,311 bytes. Stage counts: test 1,072 passed / 0 failed, doctests 1, docker-security 4/4, rust-security 20/20, m3-runtime **62/62** (nextest 19, Tasks 7, coverage 8, SemVer 18, mutation 10). The prior failed full receipt is preserved as `M3-full-gate-attempt3.json`. No stage was skipped, relaxed, or counted as a pass.

**4. Closure records** updated to what is true: `M3-matrix.md` (G1→Done, G4→Done, G5→Done, G6→Done; G2 and G8 named as the only open items), `M3-07.md` (rewritten; blocked→qualified, with counts, durations, hashes), `implementation-status.md`, plus count fixes in `README.md`, `CHANGELOG.md`, `docs/tools.md`, ADR-064/065, and the false `docs/client-configuration.md` claim that the `quality-artifacts` CLI "aún no forma parte de `main.rs`" (VF-opus item 6 — `main.rs:103` does implement it, and the CLI is now tested against the binary).

## Tests executed

| Command | Exit | Counts | Receipt |
| --- | ---: | --- | --- |
| `gate.py core --report docs/validation/M3-core-gate.json` | 0 | 14 stages, 1,072+1 | `M3-core-gate.json` |
| `gate.py full --report docs/validation/M3-full-gate.json` | 0 | 25 stages | `M3-full-gate.json` |
| m3-runtime (in full) | 0 | 62/62 | `M3-runtime.json` |
| rust-security (in full) | 0 | 20/20 | `M3-rust-security.json` |
| G6 focused pass (10 selections) | 0 | 10/10, 1 case each | `M3-06-rollback.json` |
| Negative mutation on the coverage arm | 1 | 1 failed, as intended | quoted above |

## Files changed (SHA-256)

Gate-tracked (the inventory diff vs W5 is exactly these — 1 added, 5 changed):
- `crates/mcp-server/tests/quality_artifact_cli.rs` `270ca891f0534ced76f673fc5acee4921580de29cf5a4b17e25581d1dde97e9f` (new)
- `crates/mcp-server/tests/inspection_runtime/tasks.rs` `87ee5289fdd7c16edc91f790e9cb998232a08b01c548e27be1dd9eb746789084`
- `crates/mcp-server/tests/inspection_runtime.rs` `f15596592a65b9843610265bc8b7bc9f618557c5bcbbe1fd6231896b69ab4e3f`
- `crates/mcp-server/tests/inspection_runtime/semver.rs` `172e8c7a0d55bd3cb436d8b71f96bb470edad51998abd89de72432fe1b3ccfcc`
- `crates/project-adapter/tests/quality_artifact_store.rs` `ea6de5c82c039605ee1c159f51a230d5dc87cf27ed87f4eae6c5d5b093c2ee8e`
- `scripts/test-m3-runtime.py` `dbcefd6f61c00968763024f2868a2fced10822ce45fe55f269e3bd44b827e877`

Docs/receipts: `M3-06-rollback.md` `2b02638f…`, `M3-06-rollback.json` `70442cbe…`, `M3-07.md` `5cad0a60…`, `M3-matrix.md` `3cd8d20a…`, `M3-runtime.json` `02b085bf…`, `M3-rust-security.json` `12be8174…`, `M3-full-gate-attempt3.json` `e7519c16…`, `implementation-status.md` `b1b6654c…`, `client-configuration.md` `b3213885…`, `tools.md` `7497e131…`, `ADR-064` `9d331a7b…`, `ADR-065` `926ceb87…`, `README.md` `3246f084…`, `CHANGELOG.md` `24403b0b…`.

**No production source changed** — every code change is a test or the runtime selection list. **All 23 tool snapshots are byte-identical**, verified by comparing their SHA-256 between the W5 and W6 gate inventories (0 diffs). Formatting: `cargo fmt --all --check` exits 0; my files were formatted with `rustfmt --edition 2024`. A post-gate re-hash of all 801 inventory entries shows **zero drift**, so the passing receipts describe the current code bytes.

## Docker hygiene

Checked before the core gate, after the full gate, and after the G6 run — all three returned empty for both inventories:
```
owned_containers=0
owned_volumes=0
```

## What remains open for closure, and what each needs

1. **G2 — ADR-064 and ADR-065 are formally `Proposed`.** Both implementations are qualified (seccomp delta and coverage target are pinned by literal expectations and negative mutations; rust-security 20/20). Needs: an explicit acceptance or disposition from the owner/orchestrator. No further testing.
2. **G8 — no independent re-review of the W6 bytes.** Both final reviews returned `Revise`; their blocking findings (VF-01 documentation, and the P2 contract gap) are now fixed and proved, but neither reviewer has seen this delta. Needs: one re-review of the six changed files and the updated closure records.
3. **G9** follows from 1 and 2. Until both are satisfied, no commit, merge, tag or release.

## Risks

- The three new materialization selections cancel inside the admission window: they prove task creation and a clean terminal `cancelled`, not a full coverage/SemVer/mutation run under Tasks. Full runs are already credited synchronously by M3-03/04/05 and, under Tasks, by nextest's restart selection. Recorded as a limit in `M3-07.md`.
- The future-watermark limitation above is a real fail-closed dead end for an operator who rolls back after a future version wrote the store.
- VF-07 stands: `RUST_MCP_TEST_TASKS_READY` can only force the advertisement to `true`, so the unadvertised path has no wire-level oracle. Recorded as an accepted gap, not fixed — it would have required a production-code change outside this package's scope.

## Decisions

- Docker variants for the three new tests, because that is how nextest proves the same property; they run in the gate under `--features test-hooks` like the existing four.
- Three separate selections rather than one, so the receipt attributes a failure to a specific tool (62/62 breaks down as nextest 19, Tasks 7, coverage 8, SemVer 18, mutation 10).
- `prepare_side` in `inspection_runtime/semver.rs` widened to `pub(super)` to reuse the existing two-root fixture instead of duplicating it.
- G6 proved at two levels — library (deterministic, injected clock) and product (the shipped binary's `quality-artifacts` subcommand) — because the second is what the G6 clause names and it also settles the CLI-wiring contradiction VF flagged.
- The passing core/full receipts kept the canonical names; the prior failed full receipt was preserved as `-attempt3.json`.

## Open issues

None found in the product during this package. Two process items (G2 acceptance, G8 re-review) are listed above. No Git command was run; no install, download, image build/pull/delete occurred.