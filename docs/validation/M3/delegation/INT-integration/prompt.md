# Package INT — Integrate M3: sanitize the evidence tree, commit, open the PR, watch the checks, merge (integrator delegate)

The owner authorized this integration model explicitly on 2026-09-05: work on the branch, create the pull request with the GitHub CLI, supervise the checks, merge when they pass, fix and retry when they fail. No tag, no release, no crates.io publication, no YouTrack.

## 0. Blocking prerequisite — credential material inside the evidence tree
`docs/validation/m3-clients/attempt-*/codex-home/` contains `auth.json` plus Codex session databases (`*.sqlite`, `*-wal`, `*-shm`, `installation_id`). That is credential material and per-attempt scratch state, not evidence. Before anything else:
1. Confirm the finding yourself and list every path involved.
2. Remove those `codex-home` directories from the working tree. Keep everything else in each attempt (the Inspector JSON/stdout/stderr, `codex-model-events.jsonl`, receipts) — that is the real evidence.
3. Fix `scripts/test-m3-clients.py` so a future run never stages a Codex home (or any credential file) into `docs/validation/`: point the client's home at a temporary directory outside the repository, or copy only the transcript files you need. Add an assertion in the script that the evidence directory it produced contains no file named `auth.json` and no `*.sqlite*`.
4. Add a `.gitignore` entry that would stop such a directory from ever being staged again, and record in the commit message and in the receipt what you removed and why.
5. Verify with `git status --porcelain` and a targeted `grep`/`find` that no credential-shaped file remains stageable anywhere under the repository.

## 1. What must not enter Git
- `Claude outputs/` — the owner's own untracked directory. Never stage it.
- The raw agent transcripts `docs/validation/m3-delegation/*/stdout.txt` (about 40 MB of JSONL). Keep them on disk, but do not commit them. Instead write `docs/validation/M3-delegation-log-inventory.json` listing, for every delegation package, the transcript path, its byte size and its SHA-256, plus which of its files you did commit. This mirrors the M2 precedent of explicitly selecting which logs enter Git (`docs/validation/M2-selective-log-tracking.json`).
- Anything under `target/`.
Everything else in `docs/validation/m3-delegation/` (prompt.md, command.txt, started-utc.txt, meta.json, stderr.txt, last-message.md, report.md, README.md) is small and is evidence: commit it.

## 2. Commits
Create coherent, reviewable commits on `ai/m3-quality`, in dependency order, each with a message that states what it does and why (Spanish or English, consistent with the repository's recent history — look at `git log` for the M2 style). A reasonable grouping, adjust if the diff suggests better:
1. Decisions: ADR-060 … ADR-065 and the ADR index.
2. Guest provisioning: `fixtures/rust-runtime/**` and the provisioning receipts.
3. Domain and application layers for jobs, artifacts and the four quality verticals.
4. Execution adapter: gateway phases, seccomp profile, parsers, ports.
5. Project adapter: the durable quality artifact store and shared primitives.
6. MCP server: tools 19–22, Tasks handlers, Resources, CLI subcommands, snapshots, protocol tests.
7. Fixtures for nextest, coverage, semver, mutation and hostile reports.
8. Gate scripts and harnesses.
9. Documentation: README, CHANGELOG, SECURITY, architecture, security-model, compatibility, client-configuration, tools, implementation-status, roadmap status.
10. Evidence: `docs/validation/M3-*.md`, `docs/validation/M3-*.json`, the delegation records (minus the transcripts) and the inventory file from §1.
Include `docs/prompts/implement-m3-fable-orchestrator.md` (the owner's M3 prompt) with the documentation commit. End every commit message with the two attribution lines this session uses:
```
Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud4rEkfBcrXE5GZsgdhPcf
```

## 3. Pre-push verification
After the commits and before pushing: `git status --porcelain` must show nothing unexpected; `cargo fmt --check`, `cargo check --workspace --all-targets --locked --offline`, `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`, `cargo test --workspace --locked --offline` and `python3 -B scripts/check-architecture.py` must all pass on the committed bytes; and the 18 M1/M2 tool snapshots must still be byte-identical to `main` (`git diff main -- crates/mcp-server/tests/snapshots/` should show only the four new files and the deliberate mutation-tool change already recorded).

## 4. Push and pull request
Push `ai/m3-quality` to `origin` and open a pull request against `main` with `gh`. The body must be accurate and self-contained: what M3 delivers (the four tools, the job lifecycle with negotiated Tasks, the durable artifact store, the new guest image), the decisions with ADR links, the evidence with counts and receipt paths (core gate, full gate, M3 runtime, Rust security, clients, budgets, provisioning), the independent reviews and their dispositions, the explicit limits (no release, no new platform, macOS ARM64/APFS only, what Tasks does and does not promise), and the known open items if any remain. End the body with:
```
🤖 Generated with [Claude Code](https://claude.com/claude-code)

https://claude.ai/code/session_01Ud4rEkfBcrXE5GZsgdhPcf
```

## 5. Watch the checks and finish
Poll the PR checks with `gh pr checks` (or `gh run watch`) from inside this session — do not end your turn while checks are pending. CI builds on Linux x86_64, macOS ARM64 and Windows x86_64 plus a supply-chain job and SonarCloud; expect the portable platforms to exercise the fail-closed paths of the macOS-only code. If a check fails: read the failing job's log, fix the cause on the branch (a real portability or lint defect is a real defect — do not disable a check or weaken an assertion), commit, push and re-watch. When every required check passes, merge the pull request into `main` **preserving the branch** and using a merge commit (no squash, no rebase) so the history matches the M0–M2 precedent, then record the merge commit and run a proportional post-merge smoke: `cargo test --workspace --locked --offline` plus the M3 runtime Docker gate if the merge changed any qualified byte (it should not), and confirm the working tree on `main` is clean.
Finally, write `docs/validation/M3-integration.json` with: the branch tip, every commit hash and subject, the PR number and URL, the check results, the merge commit, the post-merge smoke results, and the SHA-256 of the files you sanitized or excluded. Commit that receipt on `main` as a documentation-only commit.

## Rules
You are authorized for exactly: git add/commit on `ai/m3-quality`, `git push origin ai/m3-quality`, `gh pr create`, `gh pr checks`/`gh run` reads, fixes on the branch, `gh pr merge --merge` once green, and the post-merge smoke. You are **not** authorized to tag, release, publish to crates.io, force-push, rewrite history, delete branches, touch other repositories, or change repository settings. Never commit credentials. If a check cannot be made to pass for a reason outside the branch (an infrastructure outage, a missing secret), stop, report it precisely, and leave the PR open.
Delivery: Task, Result, the sanitization report, the commit list, the PR URL, the check matrix, the merge commit or the reason it is not merged, the post-merge smoke, Risks, Decisions, Open issues.

## Closure state at the time of this package (verify, do not assume)
- All six M3 cuts are qualified. Gates on the final bytes: `docs/validation/M3-core-gate.json` 14/14, `docs/validation/M3-full-gate.json` 25/25 (with `m3-runtime` 62/62 and `rust-security` 20/20 inside it, `audit-data` passing unweakened, `source_inputs_unchanged: true` over 801 inputs), plus `docs/validation/M3-06-rollback.json` 10/10.
- Decisions ADR-060 through ADR-065 are all Accepted; the last two were accepted by the orchestrator on 2026-09-06 with the record in `docs/validation/m3-delegation/D03-adr-acceptance/`.
- Independent reviews: contracts, security, traceability, two final reviews and a confirmation pass; the confirmation pass verdict is **Accepted** (`docs/validation/m3-delegation/VC-confirm/last-message.md`). Two residual gaps are recorded and accepted, not hidden, in `docs/validation/M3-matrix.md`.
- The handoff is `docs/validation/M3-07.md`.
Re-run the workspace gates yourself before committing; if anything is not as described, stop and report rather than committing over it.

## CRITICAL operating instruction
CI checks take many minutes. Start any long command in the background and **keep polling from inside this same session** until it finishes. Never end your turn while a gate, a push or a check run is still in flight.
