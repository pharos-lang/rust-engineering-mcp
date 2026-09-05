# M0/M1 final candidate review — Claude Opus 5

Date: 2026-09-05. Scope commit:
`a09a493a36fb029f3be324868ea6354a5441b9d8`. Public baseline at review:
`c18e4ef10079484dcec8cdf82ec22be92fd10db0` (merged PR #6).

The independent review was read-only, explicitly selected `claude-opus-5` at high
effort, disabled tools and MCP, used no session persistence, and consumed the
bounded M0/M1 closure packet. Session
`27fc28dc-4a83-4193-8327-f1af6419d64e`, result UUID
`018957c8-9853-44f1-8a37-4bd86af76855`; 21,068 output tokens including 15,482
thinking tokens, zero web/tool calls, permission denials or subagents.

## Verdict

- **ACCEPTED** for M0/M1/0.1.0 publication.
- Publication recommendation: **READY**.
- P0/P1 findings: none.
- M2 implemented: **false**.

The review reconciled the 23/23 full gate, 680 observed Rust passes, 65 Python
tests, the local artifact receipt, the final Inspector and stock Codex receipts,
the exact thirteen-tool contract, core-only feature exclusion, process cleanup,
fail-closed defaults, supply-chain controls and the qualification review trail.

## Principal disposition

- The P2 request to verify, rather than merely generate, provenance was resolved
  before publication: the release workflow now verifies all three candidate
  subjects with `gh attestation verify` before transfer.
- Cross-job custody was hardened by exporting all three build-job digests and
  comparing them against the downloaded draft-job bytes.
- Release concurrency is tag-scoped so duplicate dispatches do not race.
- The published Actions rebuild is not claimed to be byte-identical to the local
  Darwin build. Inspector/Codex bind the source-equivalent local binary; CI smoke
  and OIDC provenance bind the published bytes.
- Raw Inspector/Codex evidence remains private because it contains local paths or
  account telemetry. Public derived receipts retain the hashes; immutable local
  originals are preserved.
- `git diff --stat a6ea6b7..a09a493 -- ':!docs'` is empty. The artifact, full gate
  and final documentation therefore describe the same production code, manifests,
  lockfile, tests and CI candidate before the release-workflow hardening above.
- Export redactions and every changed-file hash must be inspected before the public
  PR. Public Linux and Windows CI must be green on the exported candidate.

## Publication conditions retained

Before publishing the GitHub Release, verify the tag-bound workflow, candidate file
set, SHA256SUMS, smoke receipt, exact OIDC attestation signer workflow and subject
digests. After publication, download the immutable public assets, repeat the smoke
on a clean macOS 26 ARM64 host, prove their hashes did not change across the
draft-to-published transition, and only then mark M1-15/M1-17 Done.

The release notes must retain the stated boundary: no catalog snapshot, model,
ONNX Runtime, LanceDB data, production signing key, positive Linux/Windows native
capability claim, or M2 implementation is part of 0.1.0.

## Publication-condition resolution

All conditions above passed on 2026-09-05. Protected PRs #8 and #9 produced public
commit `452acdbf3a634d2cc0b9d153db09718237625b9d`; final main CI and SonarCloud passed.
The definitive tag-bound workflow run `33948798048` built, installed, smoked and
attested the three published subjects. A fresh download verified `SHA256SUMS`, the
exact signer workflow/ref/source commit and 13-tool smoke; a second download after
promotion was byte-identical. The stable release and hashes are recorded in
[`m1-17-public-release.json`](../validation/m1-17-public-release.json).

The earlier run `33948251834` qualified its artifact but failed to create a draft
because the no-checkout job did not give `gh release create` an explicit repository.
No release existed. PR #9 fixed the workflow, the pre-release tag was replaced on
the protected corrected commit, and the complete definitive run succeeded. Both
attempts remain visible; only the latter is accepted release provenance.
