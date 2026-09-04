# ADR-032 — Captured project inspection through the Rust gateway

## Status

Accepted and implemented for M1-01. Reviewed ADR-031 and joined MCP workers
are prerequisites; core and actual MCP/Rust gates recorded in validation/M1-01.md.

## Context

The selected source tree is captured through original live ProjectRef handles.
Cargo metadata --no-deps reports structure and declarations, not a resolved graph,
active features or effective profile configuration. Cargo output contains opaque
package IDs and guest paths; neither belongs directly in the public contract.
The existing Local evidence has no provenance/freshness. Expensive modern MCP
bootstrap requests cannot receive cancellation until rmcp starts its receive loop.

## Decision

Add rust.project.inspect with a closed ProjectRef-only input. Keep structural
project.open unchanged. A single admitted joined worker owns source capture,
explicit-host-configured gateway initialization/calibration, frozen metadata,
bounded typed parsing and final ProjectRef revalidation. Failed or cancelled application inspection does not renew the reference. A
successful application result may renew it before the transport sends its bytes. Missing runtime/policy or unsupported source layout
fails closed; neither the peer nor a manifest can provision tools or dependencies.

The host explicitly supplies Docker executable/socket/private state root and the
approved Rust image ID. Configure without child processes at MCP startup; the
worker initializes and calibrates lazily after protocol readiness. Reject costly
bootstrap calls with fixed SANDBOX_DENIED and allow discovery/retry. Preserve the
SDK negotiation and current-thread receive loop. Retain the worker permit until
synchronous cleanup completes, including cancellation or handler drop. Session
closure checks worker completion, persistent panic state and gateway quarantine.
Cleanup uncertainty outranks cancellation/timeout in error handling. A failed
calibration (every non-cancellation error) is latched for the session; later peer
requests cannot repeatedly run containment fixtures after failed verification.
A clean interrupted calibration may retry; uncertain cleanup always quarantines.
Only bootstrap denial suggests discovery/retry, not permanent host-policy denial.

The domain exposes typed structural facts; application composes registry/source
and inspection ports without Cargo, filesystem, Docker, JSON or MCP. The execution
adapter parses bounded external Cargo JSON, tolerates added external fields, and
rejects unknown semantic values, inconsistent IDs, paths outside /source, duplicates
and excess budgets. Map opaque Cargo IDs to response-local package indexes; return
only validated relative source paths. Dependency origins expose safe kind and
identity fingerprint, never credential-bearing URLs. Explicit MSRV is nullable;
edition is not an MSRV inference. Features/dependencies remain declared values.

Read profiles and the optional fixed1.98.1 toolchain selection from the same owned
SourceBundle, never by reopening paths. Profiles describe root manifest settings,
not a second Cargo profile resolver. Cargo configuration states the actual policy:
project .cargo/config files are rejected and effective gateway flags/environment
are fixed. Do not claim those files were analyzed or their settings were applied.

Extend SourceKind additively with ProjectSnapshot for captured source observations.
Use existing Provenance/SnapshotEvidence freshness assessment with a named policy,
created/observed/assessed times and network_used=false. Expose latest_known semantics
and a capture digest distinct from ProjectIdentityFingerprint and ExecutionFingerprint.
Freshness concerns captured bytes; revalidating the live ProjectRef does not make
the full filesystem snapshot atomic or prove it remains unchanged after capture.
The runtime image/configuration identity is part of the observation. This is local
inspection evidence, never an authenticated catalog import.

Bound source and metadata using ADR-031; public serialized response budget covers
both structuredContent and its duplicated text, plus envelope/schema fields.
Oversize or incomplete metadata is rejected rather than silently omitting graph
structure. Schemas and adversarial contract tests close every public nested DTO.

## Alternatives considered

- Host Cargo metadata: violates the single calibrated execution boundary.
- Parse paths from Cargo IDs: those IDs are opaque and may change format.
- Claim resolved dependencies from --no-deps: unsupported by the command contract.
- Treat Local evidence as a snapshot: omits required provenance/freshness.
- Run calibration inline during startup/first request: delays or blocks cancellation.
- Accept arbitrary Cargo config and then erase risky flags: changes project semantics.

## Consequences

Inspection has a deliberately narrow offline source subset and can reject projects
accepted by project.open. The Linux runtime is explicit even on a macOS host.
New source-kind vocabulary applies to M1 output; existing M0 serialization and
registry behavior remain stable. M1-01 is Done only after real MCP/Cargo integration,
adverse protocol/security cases, independent review, core gate and local merge smoke.

## Sources

- Specification23.2; ADR-024, ADR-030 and ADR-031.
- https://doc.rust-lang.org/cargo/commands/cargo-metadata.html
- https://doc.rust-lang.org/cargo/reference/profiles.html
- Pinned rmcp3.2.0 bootstrap/dispatch implementation.
