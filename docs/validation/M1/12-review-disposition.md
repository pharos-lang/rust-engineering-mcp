# M1-12 — Principal review disposition

Read-only Claude Code2.1.259 / Sonnet5 Medium; actual model verified from
modelUsage. The packet contained ADR043, domain/application/SQLite/MCP source
and unit/boundary tests. The reviewer did not execute tests or modify files.

- P2 divergent validators: speculative drift, not a current defect. Application
  name_valid and adapter records::valid_name both require1..64 ASCII alphanumeric,
  underscore or hyphen bytes. Every candidate reaches the latter after the former;
  neither accepts an input the other rejects. InternalError remains appropriate
  for the unreachable broken-port InvalidInput invariant. No preventive contract
  or architecture refactor introduced on hypothetical future changes.
- P3 published_at signed-i64 bound: retained. Domain/application ports can have
  other implementations, so a domain boundary check need not be removed merely
  because the current SQLite adapter already guarantees the range.
- Confirmed ranking, literal FTS5, sentinel bounds, latest_known_stable independence,
  MSRV filtering, typed fallback, complete MCP output trimming and cancellation.
  No P0/P1 or confirmed actionable P2 remains.

Principal additionally checked shared session provider ownership, real native
E5/Lance rankings and corruption fallback, exact twelve-tool discovery and eleven
unchanged old tool snapshots. A core Clippy collapsible-if warning was fixed.
An initial parallel protocol run failed the existing closed-stdout10s harness
once; six isolated runs and the subsequent full core run passed. Independent
source inspection identified startup/write-deadline contention as a hypothesis,
not a proven cause. No production timeout or security behavior was relaxed.
The native test corpus is illustrative boundary evidence, not the M1-16 utility
experiment. Platform, distribution and client qualification remain pending.
