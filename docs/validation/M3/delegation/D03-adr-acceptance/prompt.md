# Package D03 — Formal ADR acceptance and the residual documentation findings (documentation worker, Luna)

Two independent final reviews closed with `Revise` and listed, besides the contradictions the integrator already fixed, a set of small documentation defects. This package clears them and records the orchestrator's formal acceptance of two decisions.

Read first: `docs/validation/m3-delegation/VF-opus-final/last-message.md` (findings VF-02 through VF-06) and `docs/validation/m3-delegation/VF-contracts-final/last-message.md`. Then read the two ADRs and the receipts they depend on.

## Orchestrator decision to record
**ADR-064 (quality job seccomp profile) and ADR-065 (coverage target volume, as amended) are Accepted**, effective 2026-09-06, by the M3 orchestrator. Grounds, which you must state in each ADR's Status section in your own words and with links:
- The independent security review (`docs/validation/m3-delegation/V-SEC/last-message.md`) verified that ADR-064's profile differs from the base profile by exactly one rule — an AF_UNIX anonymous stream `socketpair` with the creation flags masked out — with `socket`, `bind`, `connect`, `listen` and `accept` still absent, and that the applied container is verified against the phase's declared profile so a wider profile fails closed.
- The same review, and the final review (`VF-opus-final`), judged the amended ADR-065 shape contained: a per-job tmpfs, absent from every exporter and every non-coverage phase, read-only for the keeper, destroyed at cleanup, with the mount matrix now pinned by literal per-phase expectations and negative mutations.
- Runtime qualification passed on the approved guest image: the M3 runtime gate at 59/59 (`docs/validation/M3-runtime.json`) and the Rust security gate at 20/20 (`docs/validation/M3-rust-security.json`).
Keep each ADR's Context, Decision, Alternatives and Consequences as they are; change the Status section and add the evidence links.

## Findings to clear
- **VF-03**: `docs/adr/README.md` understates five decisions. Refresh the ADR-060 through ADR-065 entries so the index matches each ADR's own Status: 060, 061, 062 and 063 accepted and now qualified; 064 and 065 accepted today with their qualification receipts.
- **VF-02**: several documents say the Docker runtime gate passed 55/55 while the receipt they cite contains 59 selections, all passed. Normalize every count you own to 59/59 with the per-cut breakdown (nextest 19, Tasks lifecycle 4, coverage 8, SemVer 18, mutation 10). Files you own for this: `docs/tools.md`, `README.md`, `CHANGELOG.md`. Do **not** touch `docs/validation/M3-matrix.md`, `M3-07.md` or `docs/implementation-status.md` — another worker is finishing those in parallel.
- **VF-04**: `docs/validation/M3-02.md` and ADR-060's measured-budgets subsection say each candidate had 30 cold and 30 warm runs, but `docs/validation/M3-02-budgets.json` marks the nextest series `resumed_from_current_attempt_log: true` while coverage and SemVer are `false`. Add one accurate sentence to both places saying the nextest samples were carried forward from the attempt log rather than produced by the recorded run.
- **VF-05**: `docs/validation/M3-02.md`'s stock-client table reports Inspector cancel and Resource as passing, while the same receipt shows the one-shot CLI probes exiting non-zero because the Inspector CLI exposes no `tasks/cancel` and no live artifact URI; both cells were qualified through the persistent session. Footnote those two cells with that fact.
- **VF-06**: `docs/tools.md`'s Tasks section must say that a member reported `unavailable` on a poll may be transient registry contention rather than expiry, and that re-polling after the active job finishes is meaningful.

## Rules
Branch `ai/m3-quality`; no commits, merge, push, Docker, Cargo, installs or downloads. Files you own here: `docs/adr/ADR-064-*.md`, `docs/adr/ADR-065-*.md`, `docs/adr/README.md`, `docs/adr/ADR-060-*.md` (only the measured-budgets sentence for VF-04), `docs/validation/M3-02.md`, `docs/tools.md`, `README.md`, `CHANGELOG.md`. Nothing else — especially not the matrix, the handoff, the implementation status, or any code. Every number you write must come from a receipt you read in this session; cite it. Run `git diff --check` and a relative-link check over what you edited.
Delivery: Task, Result, Files changed with SHA-256, Checks executed, Evidence (finding → where it is now addressed), Risks, Decisions, Open issues.
