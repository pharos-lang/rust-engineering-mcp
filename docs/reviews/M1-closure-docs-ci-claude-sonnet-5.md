# M1 closure documentation and CI — Claude Sonnet 5

Date: 2026-09-04. Read-only high-effort review of the ADR-048 documentation
synchronization, `scripts/gate.py`, portable CI and the single-target release
workflow. Claude Code 2.1.260 was invoked with explicit `claude-sonnet-5`, tools and
MCP disabled, no persistence, and reported only that model in `modelUsage`.

The initial result was **not accepted** with one transient P0 and one P1. Session
`1eb7bed1-d5f4-4957-a947-8697a8b96203`, result UUID
`ead8dfe9-4ec5-4722-81ad-e4386ab183db`.

## Findings and disposition

- P0, CI/gate referenced the two untracked Codex qualifier files: confirmed as an
  integration-order observation. The files were an active independently reviewed
  cut and were intentionally not staged during the review. They must be committed
  before this package can receive a final acceptance.
- P1, run `33928952807` was cited without the separate receipt that
  `docs/publication.md` itself required: resolved by the live, repo-visible
  [run and branch-protection observation](../validation/public-ci-live-33928952807.json)
  and links from the board, publication note and M1-17 matrix. The historical
  publication receipt remains unchanged.
- P2, tag validation used the expensive macOS runner: resolved by restoring a cheap
  `validate-ref` Ubuntu job before the artifact job.
- P2, Python spelling: accepted; workflow provisioning selects Python explicitly
  and the commands name the Python 3 interpreter, while the existing architecture
  step remains historical portable syntax.
- P2, artifact claims were outside the diff: the reviewer correctly treated them as
  unverified in that packet. The producer/consumer scripts have their own executable
  tests and accepted supply-chain review.

A focused follow-up is required after the qualifier files are tracked. No status is
promoted to Done based on this review.

The first focused follow-up confirmed the controller files were tracked, the cheap
tag validation, one-target artifact orchestration, exact thirteen-tool boundary and
honest platform/client/catalog claims. It rejected the integration with two P0:
the live CI receipt was present in the worktree but not staged in the reviewed diff,
and a premature stable `0.1.0` version/date contradicted the still-open M1 status.
The receipt was subsequently staged for version control and the workspace remained
at `0.1.0-dev.1` until final candidate authorization. Session
`5ff39932-718a-4d87-9d1b-f2cd6a0a3c4d`, result UUID
`ae10ca26-55cc-40d9-9a56-d697b136559d`, canonical model
`claude-sonnet-5`, high effort, 17,095 output tokens including 14,468 thinking
tokens, zero web/tool calls and no permission denials.

The second focused follow-up accepted the staged live-CI receipt and the retained
pre-release state with zero P0/P1. It confirmed the live receipt has its own schema,
run/job identities, branch-protection observation and explicit scope exclusions;
the historical receipt is unchanged; and the workspace remains
`0.1.0-dev.1`/`Unreleased` while M1-15/M1-17 remain in progress. Session
`9822d8cf-863c-4516-b7d2-9bd067fa17d3`, result UUID
`b0fe4336-ac02-4262-adfe-88259fe7f031`, canonical model
`claude-sonnet-5`, medium effort, 2,963 output tokens including 1,961 thinking
tokens, zero web/tool calls and no permission denials. The UTC observation date
is intentionally later than the local session date because Bogotá is UTC-5.
