## Task

Sanitize the M3 evidence tree, commit `ai/m3-quality` in reviewable commits, open a PR against `main`, supervise the checks, and merge when green.

## Result

**Delivered up to the merge. The PR is open, complete, and green on every platform gate; the merge is blocked by two things I am not authorized to resolve.** PR: **https://github.com/pharos-lang/rust-engineering-mcp/pull/14** (branch tip `e2ec7da`).

## Sanitization report

`docs/validation/m3-clients/attempt-{7..12}/codex-home/` held **real credential material**: `auth.json` with live OAuth `id_token`/`access_token`/`refresh_token`/`account_id`, the Codex session databases (`*.sqlite`, `-wal`, `-shm`), `installation_id`, and per-run scratch — **112 files across 6 directories**, all removed. They were **never tracked**, and `git log --all --diff-filter=A` confirms no such file has ever existed in this repository's history. Every real artifact was kept (Inspector JSON/stdout/stderr, `protocol.jsonl`, `codex-model-events.jsonl`, artifact-store state, `receipt.json`). SHA-256 of all 112 removed files is in `docs/validation/M3-integration.json`.

`scripts/test-m3-clients.py` now points the client home at a temp directory outside the repo and destroys it with the gate, and asserts the produced evidence directory contains no `auth.json`, `installation_id` or `*.sqlite*` — demoting the attempt so no current receipt is published if one appears. `.gitignore` blocks credential-shaped paths (verified to hide **no** currently tracked file; the tracked M1 `catalog.sqlite` fixture stays visible). Final sweep: **0** credential-shaped stageable paths.

The 59 raw delegation transcripts (42,399,935 bytes) stay on disk, recorded with path/size/SHA-256 in `docs/validation/M3-delegation-log-inventory.json` alongside the 356 committed files across 58 packages.

## Commits — 16 on the branch

Sanitization → ADRs → provisioning → domain/application → execution adapter → project adapter → MCP tools 19–22 → fixtures → gate scripts → documentation → evidence, then `origin/main` merge, 4 defect-fix commits, and the receipt.

## Check matrix (final, on `e2ec7da`)

| Check | Result |
|---|---|
| portable / x86_64-unknown-linux-gnu | **pass** (required) |
| portable / aarch64-apple-darwin | **pass** (required) |
| portable / x86_64-pc-windows-msvc | **pass** (required) |
| supply chain | **pass** (required) |
| SonarCloud | **fail** (required) |
| CodeQL actions / python / rust | pass |

## Merge: not performed

1. **`.github/CODEOWNERS` requires @cburgosro9303 to review every change**; `mergeStateStatus: BLOCKED`, `reviewDecision: REVIEW_REQUIRED`. The author cannot self-approve.
2. **SonarCloud** fails three new-code conditions: `new_security_rating` E (all 6 issues are false positives — 4 path-injection reports where the path comes from `tempfile.TemporaryDirectory`, 2 on a test that deliberately sets `0o666`/`0o777` to assert the validator **rejects** them), `new_coverage` 54.3 % vs 80 %, `new_duplicated_lines_density` 4.0 % vs 3 %.

I did not add scanner exclusions or touch thresholds to force it green — `sonar-project.properties` already records your stance: *"Do not turn unavailable host paths into fake coverage on the portable Linux scanner."*

## Scope finding you should know about

**`origin/main` never received M2** — it was merged on local `main` only. This PR therefore delivers **M2 + M3 together** (8 M2 commits + the M3 work), and CI compiled M2 for Linux/Windows for the first time. That found **8 real portability defects**, all fixed on the branch, none worked around: the fail-closed artifact-store stub was missing `attach`; `tests/cli.rs` imported `PermissionsExt` unconditionally; a task-deadline test used a 100 ms margin against `spawn_blocking` latency; M2's `semantic_delta` and `cargo_vendor::validate` were dead off macOS (39 `dead_code` errors); a redundant `AtomicBool` import; `UnsupportedPlatform` made `--state-root` refuse to start instead of falling back to Stage 0 as documented; and five parser tests asserted absoluteness with POSIX literals that aren't absolute on Windows.

I reproduced the portable-CI lint failures locally via `cargo clippy --target x86_64-unknown-linux-gnu` rather than iterating through CI.

## Risks

- The M2 code in this PR was qualified only on macOS; its portable behaviour is now compile- and lint-clean but exercised only by CI's test run.
- SonarCloud's coverage condition will keep failing for any macOS-only work measured on the Linux scanner.

## Decisions

- Excluded all `stdout.txt` transcripts uniformly (including one 328-byte nested file) since its denial evidence is preserved in the committed `stderr.txt`.
- Kept the Windows CLI test running rather than gating it off, since its assertions are portable.
- Merged `origin/main` into the branch (branch protection is `strict`) rather than rebasing — no history rewrite.

## Open issues

- **Intermittent, pre-existing (M2):** `lost_temp_and_out_of_order_generation_remain_recovery_required` failed once on macOS with `Error: "Busy"` and passed on re-run over byte-identical sources (8/8 locally in isolation). A non-blocking `flock` `WOULDBLOCK` on the per-state-root lock. Not caused by this work; not investigated further.
- The two accepted residuals in `M3-matrix.md` (`test-hooks` advertisement override; `LiveJobAuthority::revalidate` under contention).
- Snapshots: your package expected "the deliberate mutation-tool change" — it doesn't exist. All 19 snapshots on `main` are byte-identical; `mutation-test-tool.json` is one of the four **new** files.

The receipt is committed on the branch (not `main`, since no merge occurred) at `docs/validation/M3-integration.json`.