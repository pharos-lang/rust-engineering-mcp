## Task

A17b — revise ADR-061 per V17 review dispositions.

## Result

Revised ADR-061 only. Status now reads: “Proposed — revised after review V17; pending orchestrator acceptance.”

## Files changed

- [ADR-061-private-quality-artifact-store.md](/Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-061-private-quality-artifact-store.md)  
  SHA-256: `b798a32fc40c557d2ac179d2c5853ba90919d1208e9caf5e9e05d94312710330`

## Tests executed

- `git diff --check -- docs/adr/ADR-061-private-quality-artifact-store.md` — exit 0.
- `shasum -a 256 docs/adr/ADR-061-private-quality-artifact-store.md` — exit 0.

## Evidence

- F1/F3: state-root `fstatfs` floor, 49 MiB M2 recovery headroom, 16 MiB proposed control headroom, best-effort `fallocate`, and ENOSPC handling: lines 87–99.
- F2/F9/F10/F12: explicit uid + state-root + granted-root boundary, fresh-ProjectRef re-access, non-constant-time limitation, no M1 reaper/lease touch: lines 59–63.
- F4/F11: shared primitive scope, unchanged M2-byte oracle, non-blocking `store.lock`: lines 31–35 and 154–156.
- F5/F6: hybrid clock, scoped fail-closed behaviour, recovery/prune CLI: lines 101–111.
- F7/F14: validated canonical `ArchiveBundle` USTAR ingress: lines 113–128 and 159.
- F8/F16: ADR-060 `JobId`, closed descriptor/source/runtime vocabulary: lines 41–57.
- F13: M3 Stage 0/Stage 1 adoption: lines 65–68.
- F15/G3/L11: bounded pagination/base64 arithmetic, six-column budget table, explicit rejection of eviction: lines 72–85 and 122–124.
- F17: expanded adversarial oracle matrix: lines 142–163.

## Risks

APFS allocation remains best-effort; fixture qualification must prove failure containment and M2 headroom preservation. The accepted same-uid/state-root/granted-root re-access boundary must be documented in SECURITY.md.

## Decisions

- Durable artifacts use `job_` and `qart_` IDs, not parallel job vocabularies.
- Capacity exhaustion rejects before guest execution; it never evicts retained evidence.
- Multi-file reports use one bounded, host-validated canonical USTAR `ArchiveBundle`.
- Linux/Windows fail closed before reservation or guest output.

## Open issues

- Final TTL maximum awaits fixture measurement.
- M3-01 must finalize closed `ArtifactSource`/`ArtifactRuntime` DTO fields.
- `SecretSuspected` retains only with explicit `PotentiallySensitive` host permission.
- Docs worker must record the accepted persistent-store privacy boundary.