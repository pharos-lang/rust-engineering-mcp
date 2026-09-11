# Package S02 — Make the SonarCloud quality gate pass honestly on PR #14 (integrator delegate)

Pull request #14 (`ai/m3-quality` → `main`, https://github.com/pharos-lang/rust-engineering-mcp/pull/14) is green on every platform gate — Linux, macOS and Windows portable builds, supply chain, CodeQL — and blocked only by the SonarCloud quality gate and by the owner's own review, which is not yours to give. Your job is the quality gate, and only in ways that are true.

Current failing conditions on new code (from the SonarCloud API for pull request 14):

| Condition | Actual | Threshold |
| --- | ---: | ---: |
| `new_security_rating` | E (5) | A (1) |
| `new_coverage` | 54.3 % | ≥ 80 % |
| `new_duplicated_lines_density` | 4.0 % | ≤ 3 % |

## 1. Security rating — six issues, fix the code
```
BLOCKER  scripts/probe-m2-fix-socket-mask.py:314,315,420   path built from user-controlled data
BLOCKER  scripts/probe-m2-cargo-fix.py:343                 path built from user-controlled data
MAJOR    crates/project-adapter/tests/cargo_vendor.rs:247,251  "make sure this permission is safe"
```
The previous integrator assessed all six as false positives (the paths come from `tempfile.TemporaryDirectory`; the test deliberately sets permissive modes to prove the validator rejects them). Even so, do not leave the gate red on an assertion. Change the code so the scanner has nothing to report and the meaning is unchanged or clearer: in the two probe scripts, derive every constructed path from a validated base directory the script itself created (resolve it once, assert the join stays inside it, and use that helper everywhere the finding points at); in the test, obtain the permissive mode through a helper whose intent is explicit and whose value is not a literal at the call site, keeping the assertion that the validator rejects it. If after that a finding still stands and is genuinely a false positive, say so precisely and leave it for the owner to mark in SonarCloud — do not disable a rule.

## 2. Coverage on new code — real tests first, exclusions only where measurement is impossible
The Linux scanner measures with `cargo-llvm-cov` and cannot execute anything that needs Docker, the macOS-only filesystem adapter, or a real release host. That is why `sonar-project.properties` already excludes a named set of host-bound entrypoints from the coverage percentage while keeping them analyzed, and `docs/ci.md` explains it. Work in this order:
1. **Add real unit tests** for new code that is pure and currently proven only by `#[ignore]`d Docker selections: option and selection validation, argv construction, the JUnit, LLVM-JSON, SemVer-output and mutants-outcome parsers, the coverage metric and dedupe rules, the job state machine and projections, the DTO conversions, and the Resource URI grammar. Much of this already has some tests; extend them where the uncovered lines are. Use the SonarCloud API for pull request 14 to find which files carry the uncovered new lines, and target those.
2. **Only then**, for code that genuinely cannot be measured on the portable scanner — Docker gateway phase construction and execution, the macOS-only durable store adapter, and the CLI entrypoints that need a real host — extend `sonar-project.properties`'s coverage exclusions in the same spirit as the existing entries. Every added path must be named individually (no broad wildcards over whole crates), and each one must be justified in `docs/ci.md` with the receipt that does prove its behaviour (`M3-runtime.json`, `M3-rust-security.json`, `M3-06-rollback.json`, `M3-full-gate.json`). Do not exclude anything a test could reasonably cover, and never exclude a file from analysis — only from the coverage percentage.

## 3. Duplication — reduce it, do not hide it
4.0 % of new lines are duplicated, largely because the four quality verticals were written in parallel with near-identical scaffolding (tool modules, gateway phase plumbing, runtime test harnesses). Extract the genuinely shared parts into helpers where that makes the code clearer, starting with the largest blocks the scanner reports. Do not merge things whose separation is meaningful, and do not add indirection that hurts readability just to move a number; if the remaining duplication is inherent to the closed per-tool contracts, say so with the specific blocks.

## 4. Verify and push
After each round: `cargo fmt --check`, `cargo check --workspace --all-targets --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo test --workspace --locked --offline`, `python3 -B scripts/check-architecture.py`, and `cargo clippy --target x86_64-unknown-linux-gnu ...` if you touch anything platform-conditional. Commit on `ai/m3-quality` with the repository's message style and the two attribution lines used by this session, push, then poll `gh pr checks 14` **from inside this session** until the run finishes; read the SonarCloud API again for the new condition values and iterate until the gate passes or you can state exactly why a condition cannot be met without faking it. Never end your turn while a check run or a gate is in flight.

## Rules
Branch `ai/m3-quality` only. Authorized: edits, `git add`/`commit`/`push origin ai/m3-quality`, `gh pr checks`/`gh run` reads, and the SonarCloud public API. **Not** authorized: merging, approving, tagging, releasing, force-push, history rewriting, changing repository or SonarCloud settings, disabling rules, lowering thresholds, or adding a coverage exclusion that hides code a test could cover. Never commit credentials. Keep every tool snapshot byte-identical unless a change is deliberate and reported.
Delivery: Task, Result (per condition, with the before/after values from the API), the security fixes and why each is now correct, the tests you added and what they cover, the exclusions you added with their justification and receipt, the duplication work, Files changed with SHA-256, Tests executed, the final check matrix, Risks, Decisions, Open issues.
