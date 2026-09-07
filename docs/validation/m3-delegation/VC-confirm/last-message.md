This is clearly labeled historical/superseded, not ambiguous — matches the D03 note that these Consequences/Sources sections were intentionally left as historical record. All four items verify clean.

## Verdict: **Accepted**

| Item | Status | Where verified |
|---|---|---|
| N-01 (ADR-064/065 status) | **Resolved** | `docs/validation/M3-matrix.md:166,173,256,259`, `M3-07.md:134-135,268`, `implementation-status.md:25`, and both ADR files' headers + `docs/adr/README.md:79-80` all consistently say "Accepted 2026-09-06 by the M3 orchestrator" — no remaining "Proposed"/"pending" language, and D03's acceptance record backs it. |
| N-02 (M3-06-rollback oracle + arithmetic) | **Resolved** | `M3-06-rollback.md:13-15` states "10/10 selecciones... Seis son nuevas de W6 y cuatro reutilizan oráculos existentes" (6+4=10, matches receipt); `unsupported_platform_rejects_before_any_effect` no longer appears in the doc (only in unrelated delegation package logs). |
| N-03 (stale count/SHA in M3-04/M3-05 + ADRs) | **Resolved** | `M3-04.md:28-30,41-42` and `M3-05.md:25-27,43-44` both cite `62/62` and SHA `02b085bf2d...`, which matches the `shasum -a 256 docs/validation/M3-runtime.json` recomputed in D04's own check log and the live file's 62 passing steps (grep-verified, 0 failures). ADR-062/065 disambiguate historical `55/55` from current `62/62` unambiguously (ADR-065:137-138 explicitly labels the 55/55 as "historical W3... superseded by the current W6 receipt at 62/62"). |
| Residual-gap recording | **Resolved** | Both the `test-hooks` advertisement override and `LiveJobAuthority::revalidate`-under-contention gaps are recorded with matching consequence language in `M3-matrix.md:34-45` and `M3-07.md:274-281`. |

No contradictions remain in the files checked.

**Limitations:** I could not execute `shasum` myself (read-only tools only), so the M3-runtime.json hash match relies on cross-referencing the hash D04 recorded as its own check output against what M3-04.md/M3-05.md cite — internally consistent, but not independently recomputed by me. I did not re-open or re-audit any previously-settled engineering findings, per scope.