# Package S03 — Fix the two macOS CI test failures on PR #14 (Claude Opus 5 worker)

PR #14 (`ai/m3-quality` → `main`) is green everywhere except one job: `portable / aarch64-apple-darwin` fails its "Test workspace" step at commit `36c1d93`. SonarCloud now passes, Linux and Windows pass, supply chain and CodeQL pass. This is the last technical blocker before the owner's review.

## The failure
```
error: test failed, to rerun pass `-p rust-engineering-project --lib`
---- filesystem::macos::mutation::tests::deterministic_pre_effect_interruptions_recover_without_source_write stdout ----
Error: "Busy"
---- filesystem::macos::mutation::tests::manifest_and_lock_first_swap_crash_rolls_forward_only_known_bytes stdout ----
Error: "Busy"
test result: FAILED. 63 passed; 2 failed; 2 ignored; finished in 33.97s
```
Both are pre-existing M2 tests in `crates/project-adapter/src/filesystem/macos/mutation.rs`, and both fail with the store's own `Busy` — the non-blocking exclusive lock was already held by someone else. They pass on the development host and passed every local gate; they fail on the GitHub macOS runner. Full log: `gh run view 34068141381 --log-failed`.

## What to do
1. **Reproduce it deliberately** before changing anything. Likely differences on the runner: more test threads, different timing, and a different temporary directory. Try `cargo test -p rust-engineering-project --lib -- --test-threads=16` repeatedly, and also run just those two tests together in a loop. If they only fail under contention, that is your reproduction.
2. **Find the real cause.** The question to answer precisely: what do these two tests share that makes one see the other's lock? Candidates worth checking in the code, not by guessing: whether the fixture derives its state root from something that is not unique per test (a fixed name, the process id, a shared parent that the lock covers), whether the lock is taken on a parent directory rather than the store child, whether a recently added reclamation or attach path takes the lock where it previously did not, and whether anything in the M3 work made a previously per-test lock scope process-wide.
3. **Fix the cause, not the symptom.** Give each test its own store, or narrow the lock to what it must protect. Do not serialize the suite, do not add sleeps or retries around the assertion, and do not relax the `Busy` contract — that contract is exactly what ADR-050 and ADR-061 rely on. If the honest fix is that the fixture was always sharing state and the development host merely hid it, say so plainly.
4. **Prove it.** Run the two tests together at high parallelism at least 30 times, then `cargo test -p rust-engineering-project --locked --offline` in full (the M2 mutation-store suite must stay 20/20), then `cargo fmt --check`, `cargo check --workspace --all-targets --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo test --workspace --locked --offline`, and `python3 -B scripts/check-architecture.py`.
5. **Commit and push** on `ai/m3-quality` with the repository's message style and this session's two attribution lines, then poll `gh pr checks 14` from inside this session until the whole matrix is green — including a re-analysed SonarCloud. Never end your turn while a check run is in flight. If SonarCloud's new-code conditions move because of your change, fix them the same honest way the previous package did (real tests, no rule disabling, no threshold changes).

## Rules
Branch `ai/m3-quality` only. Authorized: edits, `git add`/`commit`/`push origin ai/m3-quality`, `gh` reads. Not authorized: merging, approving, tagging, releasing, force-push, history rewriting, repository or SonarCloud settings, disabling rules. Never commit credentials. Keep every tool snapshot byte-identical. Do not touch the Docker-gated evidence or receipts — no gate needs re-running for a test-isolation fix unless you changed product code, and if you did, say exactly which receipt is now stale.
Delivery: Task, Result, the reproduction you achieved and the root cause with file and line, the fix and why it is the cause and not the symptom, Files changed with SHA-256, Tests executed (including the loop and its pass count), the final check matrix, Risks, Decisions, Open issues.
