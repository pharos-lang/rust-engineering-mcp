## Task

Bump the checkout to `0.3.0-dev`, re-qualify, and repair the M3 status board on `main`.

## Result

Both defects fixed, both gates re-run green on the new bytes, two commits pushed to `origin/main` (`b4a4213..e78a3b8`).

## The version change and where it propagated

`Cargo.toml` `0.2.0-dev` → `0.3.0-dev`. `Cargo.lock` regenerated with `cargo update --workspace --offline` — the diff is **exactly the eight `version` lines of our own packages** (16 lines changed, nothing else); no third-party version resolved, nothing went online.

**Nothing in code or fixtures pinned the literal.** The CLI, `doctor`, the protocol handshake and the catalog user agent all derive it from `env!("CARGO_PKG_VERSION")` (`crates/mcp-server/src/version.rs`, `stdio.rs`, `catalog_sync.rs`, `tests/cli.rs`, `tests/doctor.rs`, `tests/protocol.rs`). The rebuilt binary answers `rust-engineering-mcp 0.3.0-dev`.

Two **live** documents had been left behind and now agree: `docs/compatibility.md` claimed the M2 joint qualification covered "el checkout `0.2.0-dev`" 334 lines after its own header says the checkout is `0.3.0-dev` — it now attributes that qualification to the bytes that carried the old version while naming the current one; `docs/tools.md` now says `0.3.0-dev`.

Deliberately untouched: M2/M3 receipts, `docs/validation/m3-delegation/` transcripts, `docs/reviews/` packages, the CHANGELOG's historical `## 0.2.0-dev — Unreleased` heading, ADR-052's statement about stores populated on 0.2.0-dev checkouts, and `docs/prompts/`.

## Status-board repair

- `## M3 — Quality` moved from the end of the file (wedged between the M1-13 and M1-14 notes) to follow M0 and M1. Sections now read **M0, M1, M2, M3**.
- Decision table closed and completed with **ADR-064** (quality seccomp profile) and **ADR-065** (coverage target volume, as amended), both Accepted 2026-09-06 by the M3 orchestrator, each with its link and the evidence its own ADR cites (runtime 62/62, rust-security 20/20, V-SEC, plus the final review for ADR-065).
- New `## M2 — Safe Mutation / 0.2.x` section built from the notes already in the file and the receipts they link. Every link verified to resolve.
- No historical note deleted or reworded; the M0/M1/M2 trail is intact beneath `## Riesgos activos`, and removing the misplaced block left M1-13 followed by M1-14.

**Numbers verified against receipts:** 19/19 + 7 Tasks + 18/18 + 10/10 + 8/8 = runtime 62/62 ✓; security 20/20 ✓; core 14/14 ✓; full 25/25 over 810 inputs ✓; G6 rollback against `{"selections": 10, "passed": 10}` ✓; 1,105 tests + 1 doctest ✓. **One disagreed:** "budgets 30/30" read as a pass count beside "lifecycle 4/4", but `M3-02-budgets.json` records `samples_each: 30` over three operations — corrected to 30 cold and 30 warm samples per operation.

## Gates

| Command | Exit / result | Duration | Receipt SHA-256 |
|---|---|---:|---|
| fmt, check, clippy `-D warnings`, test, check-architecture | 0 / 1,106 Rust results, 0 failed | — | console |
| `gate.py core` | 0 / **14 of 14** | 435.767 s | `cf37ad509cad5c6feec76510bcc505c0cfcc6dd7ff693559eff6ae693938ccd4` |
| `gate.py full` | 0 / **25 of 25** | 2,456.504 s | `a5261457fc99640d62f421a5395d8d795a7124871db21f90e926d32259b36259` |
| `m3-runtime` stage | 0 / **62 of 62** | 1,229.210 s | `0c2984052db8a8849be32f024002f86b381c3ee69679fb098fe50901f7cb3e0a` |
| `rust-security` stage | 0 / **20 of 20** | 529.250 s | `d395c712d23ac9a55c8b8af31adc3d50d1a371338b73f200648314738b1ac8fc` |

Core `16:01:20 → 16:08:36`, full `16:08:36 → 16:49:32`; both `source_inputs_unchanged: true`. Host unchanged (cargo/rustc 1.98.1, macOS ARM64), guest image still `sha256:384a1742`. Runtime and security receipts copied from `target/` to their canonical names as previous packages did.

**Why the old receipts were invalid, measured:** the inventory is still 810 inputs and differs from the previous qualification in **exactly two entries, `Cargo.toml` and `Cargo.lock`**. The canonical `source_inputs` hash moves `0af0ed9b…` → `791856ed…`, and it is **identical in the core and the full** — both re-qualified the same bytes.

## Receipts preserved

Copied before anything overwrote them, byte-identical to what M3-07 already cites by hash:

- `M3-core-gate-v0.2.0-dev.json` — `7b59e852…`
- `M3-full-gate-v0.2.0-dev.json` — `035bfad6…`
- `M3-runtime-v0.2.0-dev.json` — `5cfab6c5…`
- `M3-rust-security-v0.2.0-dev.json` — `4ad812a0…`

## Docker hygiene

After the core gate, after the full gate, and again after the commits:

```
containers=0     # docker ps -aq --filter label=org.rust-mcp.execution=true | wc -l
volumes=0        # docker volume ls -q --filter label=org.rust-mcp.execution=true | wc -l
```

The `--format` listings were empty on all six queries.

## Files changed (SHA-256)

```
77be7241…  Cargo.toml                                  d67b10b3…  Cargo.lock
9d723401…  docs/compatibility.md                       99e0235b…  docs/tools.md
b50095a8…  docs/implementation-status.md               c930154d…  docs/validation/M3-07.md
05527a89…  docs/validation/M3-integration.json         cf37ad50…  docs/validation/M3-core-gate.json
a5261457…  docs/validation/M3-full-gate.json           0c298405…  docs/validation/M3-runtime.json
d395c712…  docs/validation/M3-rust-security.json     + the four -v0.2.0-dev receipts above
```

Commits: `340c47e` (status board), `e78a3b8` (version bump + re-qualification). `M3-integration.json` was **extended, not replaced** — 279 insertions, 0 deletions.

## Risks

- The full gate passing does not prove the rmcp legacy-batch defect absent, only that it did not fire — same standing risk W7 recorded.
- The Linux `cli.rs:131` failure still has no confirmed root cause. Unaffected by this change, still needs a Linux reproduction before anyone touches the oracle.
- `main` is now ahead of the last CI-verified tree by two commits; only one touches buildable bytes (the version), and CI has not yet reported on them.

## Decisions

- Used `cargo update --workspace --offline` rather than deleting and regenerating the lock: it is the minimal offline form and the diff proves it resolved nothing new.
- Corrected `docs/compatibility.md` and `docs/tools.md` even though they were not among the eight documents named — they are live contract documents whose "0.2.0-dev" now reads as the current checkout, which would be false. `compatibility.md` was reworded rather than flipped, because the M2 qualification genuinely happened on 0.2.0-dev bytes.
- Put the status-board commit first so the record files in the second commit describe work that already landed.

## Open issues

- `docs/validation/m3-delegation/W8-postmerge/` and `W9-version-and-status/` are untracked. W8 left its own transcripts uncommitted; committing delegation transcripts was not in this package's scope, so I left both. They will be lost if the working tree is cleaned.
- `Claude outputs/` is untracked and pre-existing; not mine, not touched.
- Unchanged from W8: the 33 CodeQL alerts still want dismissing in the Security tab, and the Docker `nonce` label rename still needs its own cut and gate.