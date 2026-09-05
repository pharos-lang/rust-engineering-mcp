# Architecture Decision Records

Los ADRs son append-only en intención: una decisión aceptada no se reescribe para
ocultar su historia; se crea otro ADR que la sustituya. Estados válidos: Proposed,
Accepted, Superseded y Rejected.

| ADR | Título | Estado |
| --- | --- | --- |
| [ADR-001](ADR-001-use-rust.md) | Rust y Tokio como plataforma | Accepted |
| [ADR-002](ADR-002-use-rmcp.md) | SDK oficial `rmcp` | Accepted |
| [ADR-003](ADR-003-stdio-first.md) | Transporte stdio primero | Accepted |
| [ADR-004](ADR-004-hexagonal-architecture.md) | Arquitectura hexagonal | Accepted |
| [ADR-005](ADR-005-no-internal-llm.md) | Sin LLM interno en core | Accepted |
| [ADR-006](ADR-006-structured-diagnostics.md) | Resultados y diagnósticos estructurados | Accepted |
| [ADR-007](ADR-007-explicit-project-handles.md) | Handles explícitos y autoridad de roots | Accepted |
| [ADR-008](ADR-008-execution-gateway.md) | Execution Gateway único | Accepted |
| [ADR-009](ADR-009-deny-by-default-security.md) | Seguridad deny-by-default verificable | Accepted |
| [ADR-010](ADR-010-no-arbitrary-shell.md) | Sin shell arbitrario | Accepted |
| [ADR-011](ADR-011-mcp-resources.md) | Resources para contexto reusable | Accepted |
| [ADR-012](ADR-012-semver-compatibility.md) | SemVer y compatibilidad MCP | Accepted |
| [ADR-013](ADR-013-safe-mutation.md) | Mutación segura fuera de M1 | Accepted |
| [ADR-014](ADR-014-artifact-handling.md) | Artifacts mínimos y acotados | Accepted |
| [ADR-015](ADR-015-json-rpc-and-json-schema.md) | JSON-RPC versus JSON Schema | Accepted |
| [ADR-016](ADR-016-sqlite-authoritative-catalog.md) | SQLite como catálogo autoritativo | Accepted |
| [ADR-017](ADR-017-lancedb-derived-index.md) | LanceDB como índice derivado | Accepted |
| [ADR-018](ADR-018-offline-catalog-sync.md) | Sincronización separada y snapshots seguros | Accepted |
| [ADR-019](ADR-019-local-embeddings.md) | Embeddings locales y reproducibles | Accepted |
| [ADR-020](ADR-020-provenance-freshness.md) | Provenance y freshness obligatorios | Accepted |
| [ADR-021](ADR-021-minimal-bootstrap.md) | Bootstrap ejecutable mínimo | Accepted |
| [ADR-022](ADR-022-domain-contracts.md) | Tipos e invariantes del dominio base | Accepted |
| [ADR-023](ADR-023-mcp-stdio-bootstrap.md) | Bootstrap MCP stdio acotado | Accepted |
| [ADR-024](ADR-024-project-open.md) | Project open estructural y roots | Accepted |
| [ADR-025](ADR-025-container-execution-gateway.md) | Execution Gateway Docker/Linux | Accepted |
| [ADR-026](ADR-026-catalog-memory-snapshots.md) | Snapshots SQLite acotados en memoria | Accepted |
| [ADR-027](ADR-027-semantic-offline-foundation.md) | E5 offline y generaciones LanceDB en memoria | Accepted |
| [ADR-028](ADR-028-ephemeral-artifact-store.md) | ArtifactStore efímero; precisa ADR-014 para M0 | Accepted |
| [ADR-029](ADR-029-local-ci-matrix.md) | CI local y matriz inicial de evidencia | Accepted |
| [ADR-030](ADR-030-m1-worker-admission.md) | Workers, cancelación y admisión MCP acotados | Accepted; workers/admisión revisados, integración Cargo pendiente |
| [ADR-031](ADR-031-rust-source-transfer.md) | Runtime Rust, source transfer y calibración | Accepted; prerrequisito revisado |
| [ADR-032](ADR-032-project-inspection.md) | Inspección de source capturado y evidencia | Accepted; M1-01 implementada y validada |
| [ADR-047](ADR-047-publication-license-and-delivery.md) | Licencia pública y entrega mediante GitHub | Accepted |
| [ADR-048](ADR-048-0.1.0-qualification-and-artifact-boundary.md) | Frontera de calificación y artifacts 0.1.0 | Accepted |
| [ADR-049](ADR-049-m2-write-boundary-qualification.md) | Calificación previa de escritura M2 | Proposed; D02 No-go del candidato actual, no writer implementado |

- [ADR-033 — Installed runtime toolchain observation](ADR-033-toolchain-inspection.md).

- [ADR-034 — Captured check and live artifact Resources](ADR-034-check-and-live-artifacts.md).

- [ADR-035: Captured formatting check](ADR-035-format-check.md).

- [ADR-036: Closed Clippy profiles](ADR-036-clippy-profiles.md).

- [ADR-037: Test execution](ADR-037-test-execution.md).
- [ADR-038: Owned RustSec audit](ADR-038-owned-rustsec-audit.md).
- [ADR-039: Compiler explanations](ADR-039-compiler-explanations.md).
- [ADR-040: Single-capture quality gate](ADR-040-single-capture-quality-gate.md).
- [ADR-041: Authenticated catalog bundles and durable activation](ADR-041-authenticated-catalog-bundles.md).
- [ADR-042: Catalog runtime status](ADR-042-catalog-runtime-status.md).
- [ADR-043: Catalog search modes](ADR-043-catalog-search-modes.md).
- [ADR-044: Paged crate inspection](ADR-044-paged-crate-inspection.md).
- [ADR-045: CLI doctor](ADR-045-cli-doctor.md).
- [ADR-046: Bounded utility experiment](ADR-046-bounded-utility-experiment.md).
