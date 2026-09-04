# M1-02 — External bounded review

Claude Code2.1.259 (version/help verified in this session), explicit claude-sonnet-5,
medium effort; observed modelUsage confirms that reviewer. Auxiliary Haiku CLI
telemetry does not substitute the review. Safe/restricted, tools disabled, strict
MCP config, no session persistence, permission-mode dontAsk: read-only packet.

Scope: ADR-033, domain/toolchain, application/toolchain, shared inspector, typed
runtime parser, MCP tool and schemas. No implementation, commands or git actions
by the reviewer. Gateway containment and M1-01 lifecycle remain their prior reviewed
baseline; the new command uses a fixed program/path and enters the fingerprint.

Reviewer found **no confirmed P0/P1/P2 findings**. It checked shared calibration/
quarantine, checkpoints between commands, exact complete records, duplicate/unknown
fields, inventory cardinality, domain/schema parity, three execution identities,
TTL/final identity revalidation and truncation rejection.

Principal review confirms: InstalledComponents is /usr/bin/cat with exactly
[--,/opt/rust/lib/rustlib/components], UID65534, same sandbox and active calibration;
no host/runtime provisioning. Actual MCP runtime test observes the installed file
and both verbose version commands in the approved immutable image. The standalone
provisioning verify.py gains the same inventory check but was not rerun as a
substitute for the gateway test. No unresolved review findings.
