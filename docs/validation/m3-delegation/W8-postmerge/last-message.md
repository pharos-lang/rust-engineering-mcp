**Task.** W8: confirm the PR #14 merge changed no qualified byte, run a proportional post-merge smoke on `main`, prove Docker hygiene, write the M3 integration receipt, close the milestone records, and push the receipt commit to `origin/main`.

**Result.** Done. `main` is at `b4a4213`, pushed. Everything below came from a command I ran in this session.

## Tree comparison

```
git diff --stat 93991c4 main   →  empty output, exit 0
```

`main`'s tree is byte-identical to the qualified branch tip `93991c4`. Every W7 receipt still describes the integrated tree exactly. I re-verified their hashes on disk against what the receipt records — `M3-core-gate.json 7b59e852…`, `M3-full-gate.json 035bfad6…`, `M3-runtime.json 5cfab6c5…`, `M3-rust-security.json 4ad812a0…`, all match.

## Post-merge smoke on `main` (`57c4037`)

| Command | Exit | Counts |
| --- | --- | --- |
| `cargo fmt --check` | 0 | no diff |
| `cargo check --workspace --all-targets --locked --offline` | 0 | — |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | 0 | no warnings |
| `cargo test --workspace --locked --offline` | 0 | **1,105 passed + 1 doctest**, 0 failed, 118 ignored (Docker-gated) |
| `python3 -B scripts/check-architecture.py` | 0 | PASS boundaries / sole gateway / offline engine / memory-only model |

Runtime confirmation: **5 of the 62** M3 runtime Docker selections, all `passed` exit 0 against guest image `sha256:384a1742…` — nextest `all_passing_tests_report_the_hypothesized_success_exit_code` (21.6 s), Tasks lifecycle `tasks_cancel_before_start_during_execution_publication_and_cleanup_waits_for_join` (46.1 s), coverage `known_counts_fixture_has_exact_line_region_and_function_oracle` (18.5 s), semver `removed_public_function_is_a_deny_level_break` (17.5 s), mutation `missed_one_fixture_names_the_surviving_function` (21.8 s). **The full 62/62 were qualified pre-merge on identical bytes** (`M3-runtime.json`); these five re-ran after it. `scripts/test-m3-runtime.py` has no selection filter and was **not modified** — a driver reproducing its environment construction and per-step command form ran the subset, and the receipt records the script's hash and that fact.

## Docker hygiene

```
docker ... ps -a     --filter label=org.rust-mcp.execution=true   →  0 (before and after)
docker ... volume ls --filter label=org.rust-mcp.execution=true   →  0 (before and after)
```

## Receipts and documents

- `docs/validation/M3-integration.json` — `c4e9ee1a69bcc1af6d4ad6a787fd053cd145884a82dd3a274fa63ecea5b98263`. Extended, not replaced: the W7 `requalification` section and the 112 sanitized-file digests are untouched; the stale pre-merge headline (`outcome: not_merged`, tip `d3330e6`, old check matrix, old `not_done`) is preserved verbatim under `postmerge.superseded_premerge_record`. Now carries all 35 commits with subjects, the 10-check matrix with conclusions, the merge commit, the bypass, the smoke with exit codes and counts, the pre-merge gate receipt hashes, and the excluded-transcript inventory hash `25095e3c…`.
- `docs/validation/M3-postmerge-runtime.json` — `c2ef705c346d589840db402123ee8f803f159b45a1649ae7126bfdd8c8e36226`.
- `docs/validation/M3-07.md` — new integration section: merge, five green required checks, the bypass, the smoke table, the surviving open items.
- `docs/validation/M3-matrix.md` — M3-06 and G9 rows to Done; CodeQL added to accepted residuals.
- `docs/implementation-status.md` — M3 to Done with `57c4037`; "In Progress"/"Blocked" no longer claim pending integration (the M3-01..06 cut descriptions were moved into Done, not deleted).

## The admin bypass

Recorded as a review requirement that could not be satisfied, not a skipped check. All five required contexts were `success` on `93991c4`: the three portable builds, `supply chain`, `SonarCloud`. `main` requires one approving review with `require_code_owner_reviews: true`; CODEOWNERS makes `@cburgosro9303` code owner of the whole repository; that account authored the PR and is the only collaborator; GitHub does not permit self-approval. `enforce_admins: false` made the bypass the only mechanism. `reviewDecision` stays `REVIEW_REQUIRED` — the exact fingerprint.

## Surviving open items (both recorded in all four documents)

1. The unreproduced Linux `cli` failure (`crates/mcp-server/tests/cli.rs:131`), documented in `93991c4`, root cause NOT CONFIRMED — needs a Linux reproduction before anyone touches the oracle.
2. Dismiss the 33 CodeQL false positives and rename the `nonce` ownership label in its own cut (it touches five qualified gateways).

## Risks

- **The receipt push itself bypassed branch protection.** `git push origin main` succeeded but the remote reported `Bypassed rule violations for refs/heads/main: Changes must be made through a pull request / 5 of 5 required status checks are expected`. So `b4a4213` sits on `main` without a PR and without CI having run on it. It is documentation-only and `git diff 93991c4 b4a4213 -- ':!docs'` is empty, so no product byte moved — but the bypass happened and you should know it did.
- A green test stage still does not prove the W7-01 rmcp batch defect absent, only that it did not fire.

## Decisions

- Proportional smoke rather than repeating the gates, because the trees are identical — stated explicitly in the receipt alongside which 5 of 62 were re-run.
- Did not modify `scripts/test-m3-runtime.py` to add a filter; that would have changed a tracked gate input inside a documentation-only commit.
- Updated the receipt's stale headline fields instead of leaving `outcome: not_merged` sitting at the top of a file that now documents a merge, preserving the old values verbatim underneath.
- Included W7's `meta.json` and `last-message.md`, which W7 could not commit because it pushed before they existed.

## Open issues

- The CodeQL rust analysis on `refs/heads/main` had not been re-uploaded for `57c4037` when I read it — the newest `/language:rust` analysis on main is still `8e6364d0` (2026-09-06, 5 results), while python and actions were uploaded for `57c4037`. The 5 open alerts currently shown on `main` therefore describe the pre-merge base, not the merged bytes; the 33 assessed alerts remain the number to act on. This caveat is recorded in the receipt rather than smoothed over.
- An untracked `Claude outputs/` directory sits in the worktree. It is not mine and I left it alone.