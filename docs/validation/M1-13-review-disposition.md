# M1-13 — Principal review disposition

Read-only Claude Code2.1.259 / Sonnet5 Medium; actual model confirmed in modelUsage.
The reviewer inspected a bounded source/test packet, without execution authority.

- P2 same-name/same-kind dependency rows: not reachable in schema1. The review
  explicitly made this conditional on unshown DDL. `catalog-adapter/src/schema.sql`
  defines PRIMARY KEY(version_id,name,kind), and records::validate separately rejects
  duplicate(name,kind) within a version before snapshot construction. Snapshot open
  validates the exact schema and recorded data. Therefore strict ordered uniqueness
  matches the authoritative schema; target/rename/richer dependency declarations are
  explicitly not represented. No incorrect sorting change or schema migration made.
- Confirmed pagination, snapshot checks, unknown fields, stable/yanked semantics,
  budgets, operational split and cancellation match ADR044. No confirmed actionable
  finding remains. Defensive complete-MCP budgeting is retained even though current
  valid maximum pages fit naturally; unit tests demonstrate exact preservation and
  non-skipping trim semantics separately.

Principal checked real CLI/MCP page reconstruction, source freshness, shared status
identity, retained generation after import and rejection in a new session. Full core
and actual local-feature MCP inspection under OS network deny pass. The latter needs
no E5/index and is not evidence of additional embedding inference or native hosts.
An initial test asserted status.snapshot_fingerprint instead of the existing
status.fingerprint; corrected the test to the unchanged public status field. No
production behavior changed to accommodate it. All twelve old tool snapshots remain
unchanged, and the new tool is exercised in all five wire versions.
