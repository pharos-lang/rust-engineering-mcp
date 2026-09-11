# Package D05 — Verify and commit the M4 complement prompt (documentation worker)

The orchestrator wrote `docs/prompts/implement-m4-complement-m3.md`: a handoff document for whoever executes M4, carrying forward what M3 built, decided and learned. It is currently untracked. Your job is to check it is true and then commit it.

## 1. Verify every factual claim against the repository
This document will be read as authority by the next milestone's agent, so no claim may be aspirational. Check each of these against the actual files and receipts, and correct anything that does not hold — report every correction you make:
- The merge commit, the preserved branch tip, the commits on top, and the current workspace version.
- The tool count and that the eighteen prior snapshots are unchanged (the mutation snapshot is the deliberate exception).
- Every gate count and receipt path cited in section 1, read from the receipts themselves.
- The `JobKind` variants, the `Phase` variants, and the names of the shared USTAR validator and the seccomp profile — that the identifiers named actually exist with those names.
- The ADR numbers, titles and their Accepted status.
- The claims about what is still open in section 6, against `docs/validation/M3-07.md` and `docs/validation/M3-matrix.md`.
- Every relative link resolves.
If a statement is right in substance but imprecise in a name or number, fix the name or number rather than deleting the statement.

## 2. Link it from where the next agent will look
Add a pointer to it from `docs/prompts/implement-m4.md` (a single line near its start saying the complement exists and what it carries), and from the handoff `docs/validation/M3-07.md` if it has a section for the next milestone. Do not rewrite either document beyond that pointer; `implement-m4.md` is the owner's base commission and its content stays as it is.

## 3. Commit
One documentation commit on `main`, in the repository's message style, ending with this session's two attribution lines:
```
Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud4rEkfBcrXE5GZsgdhPcf
```
Then push to `origin/main`. This is a documentation-only change: no gate needs re-running, and you must confirm that by checking the file you added is not part of the gate's `source_inputs` inventory — say so explicitly in your report.

## Rules
Branch `main`. Authorized: edits to the three documents named above, `git add`/`commit`, `git push origin main`, and read-only inspection. Not authorized: touching code, fixtures, scripts, receipts, ADRs, tagging, releasing, force-push, history rewriting. Run `git diff --check` and a relative-link check over what you edit. Never assert a number you did not read in this session.
Delivery: Task, Result, the corrections you made and why, Files changed with SHA-256, the commit hash, the checks you ran, Risks, Decisions, Open issues.
