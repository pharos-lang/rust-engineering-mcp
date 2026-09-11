## Task

A17 — drafted ADR-061 as Proposed.

## Result

Created the owner-bound private persistent artifact-store decision, including format, authorization, quotas, recovery, Resources, hostile-content policy, rollback, and 11 M3-01 test/oracle cases. No product code changed.

## Files changed

- [ADR-061-private-quality-artifact-store.md](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-061-private-quality-artifact-store.md)  
  SHA-256: `25afe84d20ed52b830e9eec400802125db8d31cfb847d967a25abbda74324f0e`

## Tests executed

- `git diff --check -- docs/adr/ADR-061-private-quality-artifact-store.md` — exit 0; 0 whitespace defects.
- Required ADR-section check via `rg` — exit 0; 6/6 required sections.
- M3-01 oracle-row check via `rg` — exit 0; 11 cases.
- `shasum -a 256 docs/adr/ADR-061-private-quality-artifact-store.md` — exit 0.

## Evidence

ADR-061 names the M2 primitives to extract/reuse, keeps M1 artifacts and URIs unchanged, isolates `rust-mcp-quality-artifacts-v1` from M2 journals, and fail-closes outside macOS ARM64/APFS.

## Risks

Physical APFS preallocation/reservation is a required native qualification; logical quota or free-space estimates are explicitly insufficient.

## Decisions

Persistent artifacts use v1 descriptors, opaque owner bindings revalidated from live host authority, pre-job reservation, absolute TTL, and descriptor-last atomic publication.

## Open issues

Owner acceptance is required before enabling persistence because it changes the public privacy posture from process-ephemeral M1 artifacts to state-root retention. Alternatives: accept and update SECURITY/client documentation; retain only ephemeral M1 artifacts and defer persistent M3 evidence. D06 must also define how an authorized restarted job/result can reissue a URI with a fresh ProjectRef without global artifact discovery.