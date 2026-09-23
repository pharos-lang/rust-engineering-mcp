# Mapa de decisiones (ADR y especificación)

Este documento es el mapa compacto, no una segunda explicación. Cada fila
apunta al capítulo que contiene la decisión, el contexto, las alternativas
rechazadas, las consecuencias, el estado actual y los riesgos residuales —
eso vive en el capítulo señalado por "Sección canónica", no aquí. La
columna "Fuente histórica" es un permalink al ADR original en el commit
base de esta limpieza documental
(`51fa602e`, rama `ai/cleanup-product-docs`) — los 90 ADR originales quedan
solo en el historial de Git, no como archivos vigentes en el árbol.

Precedencia de resolución de contradicciones usada al construir este mapa:
seguridad > correctness > requisitos explícitos de la especificación >
compatibilidad MCP > contratos públicos existentes > testabilidad >
mantenibilidad > simplicidad operacional > rendimiento > ergonomía para
agentes > extensibilidad futura (la misma jerarquía de `AGENTS.md`).

Estado:
- **vigente** — la decisión sigue gobernando el producto hoy. Un paréntesis
  puede matizar (p. ej. "vigente, con una brecha autodeclarada"): la matización
  vive en el capítulo, aquí solo se anticipa.
- **sustituido por ADR-NNN** — un ADR posterior reemplazó el mecanismo (no
  siempre el principio) de este.
- **histórico** — describe un estado o un experimento ya cerrado que no
  gobierna ninguna decisión vigente del producto.

## Tabla de ADR (ADR-001 … ADR-090)

| ADR | Título | Estado | Sección canónica | Fuente histórica |
| --- | --- | --- | --- | --- |
| ADR-001 | Rust y Tokio como plataforma | vigente | [overview.md](overview.md#rust-y-tokio-como-plataforma) | [ADR-001-use-rust.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-001-use-rust.md) |
| ADR-002 | SDK oficial `rmcp` | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#rmcp-como-frontera-mcp) | [ADR-002-use-rmcp.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-002-use-rmcp.md) |
| ADR-003 | Transporte stdio primero | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#stdio-como-único-transporte-y-su-presupuesto) | [ADR-003-stdio-first.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-003-stdio-first.md) |
| ADR-004 | Arquitectura hexagonal | vigente | [overview.md](overview.md#arquitectura-hexagonal-y-fronteras-de-crate) | [ADR-004-hexagonal-architecture.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-004-hexagonal-architecture.md) |
| ADR-005 | Sin LLM interno en el core | vigente | [domain-and-application.md](domain-and-application.md#sin-llm-interno-en-el-core) | [ADR-005-no-internal-llm.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-005-no-internal-llm.md) |
| ADR-006 | Resultados y diagnósticos estructurados | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#álgebra-de-resultados-y-json-schema-por-tool) | [ADR-006-structured-diagnostics.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-006-structured-diagnostics.md) |
| ADR-007 | Handles explícitos y autoridad de roots | vigente | [execution-and-security.md](execution-and-security.md#roots-confiables-y-handles-explícitos) | [ADR-007-explicit-project-handles.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-007-explicit-project-handles.md) |
| ADR-008 | Execution Gateway único | vigente (regla; alcance de plataforma parcial, ver capítulo) | [execution-and-security.md](execution-and-security.md#execution-gateway-único) | [ADR-008-execution-gateway.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-008-execution-gateway.md) |
| ADR-009 | Seguridad deny-by-default verificable | vigente (regla; alcance de plataforma parcial, ver capítulo) | [execution-and-security.md](execution-and-security.md#deny-by-default-verificable-e-invariantes-de-aislamiento) | [ADR-009-deny-by-default-security.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-009-deny-by-default-security.md) |
| ADR-010 | Sin shell arbitrario | vigente | [execution-and-security.md](execution-and-security.md#sin-shell-arbitrario) | [ADR-010-no-arbitrary-shell.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-010-no-arbitrary-shell.md) |
| ADR-011 | Resources para contexto reusable | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#resources-para-contexto-ya-computado) | [ADR-011-mcp-resources.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-011-mcp-resources.md) |
| ADR-012 | SemVer y compatibilidad MCP | vigente | [../reference/compatibility.md](../reference/compatibility.md) | [ADR-012-semver-compatibility.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-012-semver-compatibility.md) |
| ADR-013 | Mutación segura fuera de M1 | sustituido por ADR-050 (mecanismo); histórico en su alcance M1 | [mutation.md](mutation.md#mutación-segura-fuera-de-m1-histórico-en-su-alcance-principios-continuados) | [ADR-013-safe-mutation.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-013-safe-mutation.md) |
| ADR-014 | Artifacts mínimos y acotados | sustituido por ADR-028 (mecanismo real; el "directorio privado" nunca se implementó) | [jobs-and-artifacts.md](jobs-and-artifacts.md#artifactstore-efímero-de-m1-mecanismo-real-no-el-de-adr-014) | [ADR-014-artifact-handling.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-014-artifact-handling.md) |
| ADR-015 | JSON-RPC versus JSON Schema | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#álgebra-de-resultados-y-json-schema-por-tool) | [ADR-015-json-rpc-and-json-schema.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-015-json-rpc-and-json-schema.md) |
| ADR-016 | SQLite como catálogo autoritativo | vigente | [catalog-and-search.md](catalog-and-search.md#sqlite-autoritativo-lancedb-derivado) | [ADR-016-sqlite-authoritative-catalog.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-016-sqlite-authoritative-catalog.md) |
| ADR-017 | LanceDB como índice derivado | vigente | [catalog-and-search.md](catalog-and-search.md#sqlite-autoritativo-lancedb-derivado) | [ADR-017-lancedb-derived-index.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-017-lancedb-derived-index.md) |
| ADR-018 | Sincronización separada y snapshots seguros | vigente (separación CLI/runtime); la cláusula de "admin override" de rollback quedó sustituida por ADR-041, que nunca la construyó | [catalog-and-search.md](catalog-and-search.md#bundles-firmados-y-activación-durable-mecanismo-real-de-adr-018) | [ADR-018-offline-catalog-sync.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-018-offline-catalog-sync.md) |
| ADR-019 | Embeddings locales y reproducibles | vigente (condición de gate satisfecha) | [catalog-and-search.md](catalog-and-search.md#embeddings-locales-reproducibles) | [ADR-019-local-embeddings.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-019-local-embeddings.md) |
| ADR-020 | Provenance y freshness obligatorios | vigente | [domain-and-application.md](domain-and-application.md#provenance-y-freshness-obligatorios) | [ADR-020-provenance-freshness.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-020-provenance-freshness.md) |
| ADR-021 | Bootstrap ejecutable mínimo | vigente (convenciones); histórico (comportamiento CLI original) | [overview.md](overview.md#bootstrap-ejecutable-mínimo-histórico-en-su-alcance-de-comportamiento-vigente-en-sus-convenciones) | [ADR-021-minimal-bootstrap.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-021-minimal-bootstrap.md) |
| ADR-022 | Tipos e invariantes del dominio base | vigente | [domain-and-application.md](domain-and-application.md#tipos-e-invariantes-del-dominio-base) | [ADR-022-domain-contracts.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-022-domain-contracts.md) |
| ADR-023 | Bootstrap MCP stdio acotado | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#stdio-como-único-transporte-y-su-presupuesto) | [ADR-023-mcp-stdio-bootstrap.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-023-mcp-stdio-bootstrap.md) |
| ADR-024 | Project open estructural y roots | vigente | [execution-and-security.md](execution-and-security.md#roots-confiables-y-handles-explícitos) | [ADR-024-project-open.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-024-project-open.md) |
| ADR-025 | Execution Gateway Docker/Linux | vigente | [execution-and-security.md](execution-and-security.md#el-gateway-dockerlinux-como-frontera-concreta) | [ADR-025-container-execution-gateway.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-025-container-execution-gateway.md) |
| ADR-026 | Snapshots SQLite acotados en memoria | vigente (mecánica); encuadre sustituido operacionalmente por ADR-041 | [catalog-and-search.md](catalog-and-search.md#snapshots-acotados-en-memoria-ahora-con-activación-durable) | [ADR-026-catalog-memory-snapshots.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-026-catalog-memory-snapshots.md) |
| ADR-027 | E5 offline y generaciones LanceDB en memoria | vigente | [catalog-and-search.md](catalog-and-search.md#embeddings-locales-reproducibles) | [ADR-027-semantic-offline-foundation.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-027-semantic-offline-foundation.md) |
| ADR-028 | ArtifactStore efímero M0 | vigente | [jobs-and-artifacts.md](jobs-and-artifacts.md#artifactstore-efímero-de-m1-mecanismo-real-no-el-de-adr-014) | [ADR-028-ephemeral-artifact-store.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-028-ephemeral-artifact-store.md) |
| ADR-029 | CI local y matriz inicial de evidencia | vigente (solo la prohibición de Actions remoto fue levantada por ADR-047) | [../development/testing.md](../development/testing.md) | [ADR-029-local-ci-matrix.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-029-local-ci-matrix.md) |
| ADR-030 | Workers, cancelación y admisión MCP acotados | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#admisión-de-workers-cancelación-y-transporte) | [ADR-030-m1-worker-admission.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-030-m1-worker-admission.md) |
| ADR-031 | Runtime Rust, source transfer y calibración | vigente (enmienda ya aplicada en código) | [execution-and-security.md](execution-and-security.md#el-gateway-dockerlinux-como-frontera-concreta) | [ADR-031-rust-source-transfer.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-031-rust-source-transfer.md) |
| ADR-032 | Inspección de source capturado y evidencia | vigente | [domain-and-application.md](domain-and-application.md#inspección-de-proyecto-y-de-toolchain) | [ADR-032-project-inspection.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-032-project-inspection.md) |
| ADR-033 | Installed runtime toolchain observation | vigente | [domain-and-application.md](domain-and-application.md#inspección-de-proyecto-y-de-toolchain) | [ADR-033-toolchain-inspection.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-033-toolchain-inspection.md) |
| ADR-034 | Captured check and live artifact Resources | vigente | [execution-and-security.md](execution-and-security.md#las-tools-m1-individuales-sobre-el-gateway) | [ADR-034-check-and-live-artifacts.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-034-check-and-live-artifacts.md) |
| ADR-035 | Captured formatting check | vigente | [execution-and-security.md](execution-and-security.md#las-tools-m1-individuales-sobre-el-gateway) | [ADR-035-format-check.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-035-format-check.md) |
| ADR-036 | Closed Clippy profiles | vigente | [execution-and-security.md](execution-and-security.md#las-tools-m1-individuales-sobre-el-gateway) | [ADR-036-clippy-profiles.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-036-clippy-profiles.md) |
| ADR-037 | Test execution | vigente | [execution-and-security.md](execution-and-security.md#las-tools-m1-individuales-sobre-el-gateway) | [ADR-037-test-execution.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-037-test-execution.md) |
| ADR-038 | Owned RustSec audit | vigente | [catalog-and-search.md](catalog-and-search.md#auditoría-rustsec-propia-y-offline) | [ADR-038-owned-rustsec-audit.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-038-owned-rustsec-audit.md) |
| ADR-039 | Compiler explanations | vigente | [analyzer.md](analyzer.md#explicaciones-respaldadas-por-el-compilador) | [ADR-039-compiler-explanations.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-039-compiler-explanations.md) |
| ADR-040 | Single-capture quality gate | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#el-gate-de-calidad-de-captura-única-y-sus-contratos-congelados) | [ADR-040-single-capture-quality-gate.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-040-single-capture-quality-gate.md) |
| ADR-041 | Authenticated catalog bundles and durable activation | vigente | [catalog-and-search.md](catalog-and-search.md#bundles-firmados-y-activación-durable-mecanismo-real-de-adr-018) | [ADR-041-authenticated-catalog-bundles.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-041-authenticated-catalog-bundles.md) |
| ADR-042 | Catalog runtime status | vigente | [catalog-and-search.md](catalog-and-search.md#estado-del-catálogo-en-runtime) | [ADR-042-catalog-runtime-status.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-042-catalog-runtime-status.md) |
| ADR-043 | Catalog search modes | vigente | [catalog-and-search.md](catalog-and-search.md#modos-de-búsqueda-híbrida) | [ADR-043-catalog-search-modes.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-043-catalog-search-modes.md) |
| ADR-044 | Paged crate inspection | vigente | [catalog-and-search.md](catalog-and-search.md#inspección-paginada-de-crates) | [ADR-044-paged-crate-inspection.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-044-paged-crate-inspection.md) |
| ADR-045 | CLI doctor | vigente | [../operations/runtime-provisioning.md](../operations/runtime-provisioning.md) | [ADR-045-cli-doctor.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-045-cli-doctor.md) |
| ADR-046 | Bounded utility experiment | histórico | [../development/testing.md](../development/testing.md) | [ADR-046-bounded-utility-experiment.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-046-bounded-utility-experiment.md) |
| ADR-047 | Publication license and delivery | vigente | [../operations/release-verification.md](../operations/release-verification.md) | [ADR-047-publication-license-and-delivery.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-047-publication-license-and-delivery.md) |
| ADR-048 | 0.1.0 qualification and artifact boundary | vigente | [../operations/release-verification.md](../operations/release-verification.md) | [ADR-048-0.1.0-qualification-and-artifact-boundary.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-048-0.1.0-qualification-and-artifact-boundary.md) |
| ADR-049 | M2 write boundary qualification (D02) | sustituido por ADR-050 (evidencia negativa conservada como histórica) | [mutation.md](mutation.md#por-qué-no-hay-exclusión-a-nivel-de-sistema-operativo-d02) | [ADR-049-m2-write-boundary-qualification.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-049-m2-write-boundary-qualification.md) |
| ADR-050 | Local coordinated mutation | vigente | [mutation.md](mutation.md#local_coordinated-y-sus-límites-explícitos) | [ADR-050-local-coordinated-mutation.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-050-local-coordinated-mutation.md) |
| ADR-051 | Semantic manifest editor | vigente (ADR-057 añade excepción acotada) | [mutation.md](mutation.md#editor-semántico-de-manifiestos-y-operaciones-tipadas) | [ADR-051-semantic-manifest-editor.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-051-semantic-manifest-editor.md) |
| ADR-052 | Mutation journal and authorization | vigente (ADR-059 refina retención, no sustituye) | [mutation.md](mutation.md#journal-privado-autorización-y-retirada-de-planes) | [ADR-052-mutation-journal-and-authorization.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-052-mutation-journal-and-authorization.md) |
| ADR-053 | Bounded guest mutation staging | vigente | [execution-and-security.md](execution-and-security.md#staging-y-fix-de-mutación-dentro-del-mismo-gateway) | [ADR-053-bounded-guest-mutation-staging.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-053-bounded-guest-mutation-staging.md) |
| ADR-054 | Multiple file mutation publication | vigente | [mutation.md](mutation.md#publicación-multiarchivo-y-datos-de-cargo-offline) | [ADR-054-multiple-file-mutation-publication.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-054-multiple-file-mutation-publication.md) |
| ADR-055 | Offline cargo data and lock policy | vigente | [mutation.md](mutation.md#publicación-multiarchivo-y-datos-de-cargo-offline) | [ADR-055-offline-cargo-data-and-lock-policy.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-055-offline-cargo-data-and-lock-policy.md) |
| ADR-056 | Cargo fix isolated loopback | vigente | [execution-and-security.md](execution-and-security.md#staging-y-fix-de-mutación-dentro-del-mismo-gateway) | [ADR-056-cargo-fix-isolated-loopback.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-056-cargo-fix-isolated-loopback.md) |
| ADR-057 | Typed manifest and dependency operations | vigente | [mutation.md](mutation.md#editor-semántico-de-manifiestos-y-operaciones-tipadas) | [ADR-057-typed-manifest-and-dependency-operations.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-057-typed-manifest-and-dependency-operations.md) |
| ADR-058 | Local mutation observability | vigente | [mutation.md](mutation.md#observabilidad-local-de-mutación) | [ADR-058-local-mutation-observability.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-058-local-mutation-observability.md) |
| ADR-059 | Terminal plan retirement and durable replay | vigente | [mutation.md](mutation.md#journal-privado-autorización-y-retirada-de-planes) | [ADR-059-terminal-plan-retirement-and-durable-replay.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-059-terminal-plan-retirement-and-durable-replay.md) |
| ADR-060 | Bounded job execution and negotiated MCP Tasks | vigente | [jobs-and-artifacts.md](jobs-and-artifacts.md#ejecución-de-jobs-acotada-y-mcp-tasks-negociadas) | [ADR-060-bounded-job-execution-and-mcp-tasks.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-060-bounded-job-execution-and-mcp-tasks.md) |
| ADR-061 | Private quality artifact store | vigente (extendido por ADR-062) | [jobs-and-artifacts.md](jobs-and-artifacts.md#store-privado-de-artifacts-de-quality-jobs) | [ADR-061-private-quality-artifact-store.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-061-private-quality-artifact-store.md) |
| ADR-062 | Coverage accounting and semver baselines | vigente | [analyzer.md](analyzer.md#coverage-y-semver-como-extensiones-del-mismo-gateway) | [ADR-062-coverage-accounting-and-semver-baselines.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-062-coverage-accounting-and-semver-baselines.md) |
| ADR-063 | M3 guest plugin provisioning | vigente (registro de provisioning) | [../operations/runtime-provisioning.md](../operations/runtime-provisioning.md) | [ADR-063-m3-guest-plugin-provisioning.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-063-m3-guest-plugin-provisioning.md) |
| ADR-064 | Quality job seccomp profile | vigente | [execution-and-security.md](execution-and-security.md#extensiones-de-seccomp-para-quality-y-coverage) | [ADR-064-quality-job-seccomp-profile.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-064-quality-job-seccomp-profile.md) |
| ADR-065 | Coverage target volume | vigente (versión enmendada final) | [execution-and-security.md](execution-and-security.md#extensiones-de-seccomp-para-quality-y-coverage) | [ADR-065-coverage-target-volume.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-065-coverage-target-volume.md) |
| ADR-066 | M4 runtime provisioning | vigente (adquisición); admisión delegada y completada por ADR-068 | [../operations/runtime-provisioning.md](../operations/runtime-provisioning.md) | [ADR-066-m4-runtime-provisioning.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-066-m4-runtime-provisioning.md) |
| ADR-067 | Security policy and quality contracts (D19) | vigente | [mcp-and-contracts.md](mcp-and-contracts.md#el-gate-de-calidad-de-captura-única-y-sus-contratos-congelados) | [ADR-067-security-policy-and-quality-contracts.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-067-security-policy-and-quality-contracts.md) |
| ADR-068 | M4 runtime admission | vigente | [execution-and-security.md](execution-and-security.md#admisión-por-identidad-inmutable) | [ADR-068-m4-runtime-admission.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-068-m4-runtime-admission.md) |
| ADR-069 | Isolated unsafe syntax scanner (D20) | vigente (v3 gobierna) | [execution-and-security.md](execution-and-security.md#scanner-de-sintaxis-unsafe-aislado) | [ADR-069-isolated-unsafe-syntax-scanner.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-069-isolated-unsafe-syntax-scanner.md) |
| ADR-070 | Task cleanup attestation | vigente | [jobs-and-artifacts.md](jobs-and-artifacts.md#atestación-real-de-cleanup-no-solo-el-retorno-del-worker) | [ADR-070-task-cleanup-attestation.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-070-task-cleanup-attestation.md) |
| ADR-071 | Supply chain facts without catalog migration (D22) | vigente | [catalog-and-search.md](catalog-and-search.md#hechos-de-supply-chain-sin-migrar-el-catálogo) | [ADR-071-supply-chain-facts-without-catalog-migration.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-071-supply-chain-facts-without-catalog-migration.md) |
| ADR-072 | Miri classification integrity (D21) | vigente | [analyzer.md](analyzer.md#integridad-de-clasificación-de-miri) | [ADR-072-miri-classification-integrity.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-072-miri-classification-integrity.md) |
| ADR-073 | Benchmark method and dataset (D23) | vigente (el método; los veredictos direccionales no — ver ADR-081) | [performance.md](performance.md#criterios-estadísticos-leídos-siempre-juntos) | [ADR-073-benchmark-method-and-dataset.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-073-benchmark-method-and-dataset.md) |
| ADR-074 | Profiling capability and containment (D24) | vigente (§3.1 corregido en `89ec1140`; no bloquea el cierre de M5 — ver errata C1 P1-2) | [performance.md](performance.md#capability-de-profiling-y-helper-propio-con-la-corrección-de-31-ya-cerrada) | [ADR-074-profiling-capability-and-containment.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-074-profiling-capability-and-containment.md) |
| ADR-075 | M5 runtime provisioning | vigente (extendido por ADR-082 aditivamente) | [../operations/runtime-provisioning.md](../operations/runtime-provisioning.md) | [ADR-075-m5-runtime-provisioning.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-075-m5-runtime-provisioning.md) |
| ADR-076 | M5 performance contracts | vigente; §6 sustituido por ADR-079, §5 completado por ADR-080 | [performance.md](performance.md#contratos-públicos-m5) | [ADR-076-m5-performance-contracts.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-076-m5-performance-contracts.md) |
| ADR-077 | M5 runtime admission | vigente (auto-enmendado) | [execution-and-security.md](execution-and-security.md#admisión-por-identidad-inmutable) | [ADR-077-m5-runtime-admission.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-077-m5-runtime-admission.md) |
| ADR-078 | Offline vendor capture | vigente (plenamente enmendado) | [execution-and-security.md](execution-and-security.md#captura-de-vendor-a-gran-escala-separada-de-sourcebundle) | [ADR-078-offline-vendor-capture.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-078-offline-vendor-capture.md) |
| ADR-079 | Bloat result semantics | vigente (sustituye ADR-076 §6) | [performance.md](performance.md#semántica-de-éxito-de-rustbinarybloat-sustituye-adr-076-6) | [ADR-079-bloat-result-semantics.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-079-bloat-result-semantics.md) |
| ADR-080 | Harness logs as artifacts | vigente (brecha autodeclarada, sin corregir) | [jobs-and-artifacts.md](jobs-and-artifacts.md#logs-del-harness-como-artifacts-privados) | [ADR-080-harness-logs-as-artifacts.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-080-harness-logs-as-artifacts.md) |
| ADR-081 | Benchmark statistical requalification | vigente (implementado en código: `METHOD_QUALIFIED_FOR_DIRECTION = false`) | [performance.md](performance.md#criterios-estadísticos-leídos-siempre-juntos) | [ADR-081-benchmark-statistical-requalification.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-081-benchmark-statistical-requalification.md) |
| ADR-082 | M6 runtime provisioning | vigente | [../operations/runtime-provisioning.md](../operations/runtime-provisioning.md) | [ADR-082-m6-runtime-provisioning.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-082-m6-runtime-provisioning.md) |
| ADR-083 | Analyzer contract and actions | vigente | [analyzer.md](analyzer.md#contrato-de-tools-del-analyzer) | [ADR-083-analyzer-contract-and-actions.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-083-analyzer-contract-and-actions.md) |
| ADR-084 | Rust-analyzer runtime and LSP lifecycle | vigente (como enmendado) | [analyzer.md](analyzer.md#runtime-de-rust-analyzer-y-ciclo-de-vida-lsp) | [ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md) |
| ADR-085 | M6 runtime admission | vigente | [execution-and-security.md](execution-and-security.md#admisión-por-identidad-inmutable) | [ADR-085-m6-runtime-admission.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-085-m6-runtime-admission.md) |
| ADR-086 | Deprecation and freeze policy (0.8→1.0) | vigente | [../reference/compatibility.md](../reference/compatibility.md) | [ADR-086-deprecation-and-freeze-policy.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-086-deprecation-and-freeze-policy.md) |
| ADR-087 | 1.0 host scope and spec clarification (D13) | vigente | [../reference/compatibility.md](../reference/compatibility.md) | [ADR-087-1.0-host-scope.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-087-1.0-host-scope.md) |
| ADR-088 | Migration, downgrade and backup/restore policy (0.3.0→0.8.0) (D12) | vigente | [../operations/backup-and-recovery.md](../operations/backup-and-recovery.md) | [ADR-088-migration-rollback-policy.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-088-migration-rollback-policy.md) |
| ADR-089 | Residual risk register 1.0 | vigente | [execution-and-security.md](execution-and-security.md#registro-de-riesgos-residuales-de-10) | [ADR-089-residual-risk-register.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-089-residual-risk-register.md) |
| ADR-090 | Offline verification and incident response (D14) | vigente (política de incidentes); cadena tag→run→digest→attestation solo probada en RC1; la ruta de verificación offline con `--bundle` de §2 no está implementada/ejercida — ver [`release-verification.md`](../operations/release-verification.md#cómo-verificar-un-artifact-descargado) | [../operations/release-verification.md](../operations/release-verification.md) | [ADR-090-offline-verification-and-incident-response.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/adr/ADR-090-offline-verification-and-incident-response.md) |

## Grupo de filas: la especificación original

[`docs/spec/rust-engineering-mcp-propuesta-v0.3.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) (~5400 líneas) fue la
propuesta fundacional del producto. Cada una de sus cláusulas normativas se
clasificó (`vigente-implementado`/`vigente-limitación`/`pendiente-
autorizado`/`sustituido`/`aspiracional`/`descriptivo`) contra código y tests
reales durante esta limpieza documental (~315 filas), en un mapa de trabajo
temporal que nunca fue la fuente de verdad — el código y los tests reales
lo son; algunas de sus filas quedaron además superadas por hallazgos
posteriores de esta misma limpieza (ver
[`../operations/backup-and-recovery.md`](../operations/backup-and-recovery.md)
y [`execution-and-security.md`](execution-and-security.md) para los casos
corregidos). Las filas siguientes son las decisiones de la especificación
que no tienen un ADR propio y que sí quedaron redistribuidas en capítulos
propios de este documento:

| Origen | Decisión de la especificación | Estado | Sección canónica | Fuente histórica |
| --- | --- | --- | --- | --- |
| §13 | Layout propuesto de 10 crates | sustituido (layout real de 8 crates, más grueso, deuda técnica registrada sin ADR numerada) | [overview.md](overview.md#arquitectura-hexagonal-y-fronteras-de-crate) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §71 | Dependencias "core" sugeridas: `thiserror`, `camino`, `tempfile`, `cargo_metadata` | vigente-limitación (ninguna es dependencia; enums hechos a mano + `rustix`) | [overview.md](overview.md#lenguaje-interno-tipos-y-dependencias-no-adoptadas) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §72 | `thiserror` en dominio/adapters, `anyhow` en los bordes | sustituido (enums de error cerrados hechos a mano, sin ADR que lo registre) | [domain-and-application.md](domain-and-application.md#datos-de-proyecto-frente-a-datos-de-catálogo-y-separación-de-errores) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §9.2 / §88 | URIs de Resources sugeridas `rust-project://…`, `rust-catalog://…` | sustituido (URIs opacas dinámicas `rust-artifact://`/`rust-quality-artifact://`) | [mcp-and-contracts.md](mcp-and-contracts.md#resources-para-contexto-ya-computado) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §2 / §8.1 / §96 / M7 | Transporte remoto HTTP | pendiente-autorizado (diferido por decisión explícita, [`docs/roadmap/m7-g0-decision.md`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/roadmap/m7-g0-decision.md), no simplemente no-implementado) | [mcp-and-contracts.md](mcp-and-contracts.md#stdio-como-único-transporte-y-su-presupuesto) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §33.1 / §33.2 | Tool de documentación local (rustdoc/std) y remota (docs.rs/crates.io) | vigente-limitación (ninguna existe; `rust.crate.inspect`/`.search` cubren metadata, no contenido de documentación) | [analyzer.md](analyzer.md#documentación-local-y-remota-sin-tool-dedicada) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §42 | Adapters de sandbox nativos por plataforma (Linux/macOS/Windows) | vigente-limitación (solo el guest Docker Linux ARM64 está qualified) | [execution-and-security.md](execution-and-security.md#frontera-de-distribución-por-qué-vigente-casi-siempre-significa-macos-arm64) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §68 | Contadores/métricas locales estándar (ejecuciones, duración, cache-hit, fallos) | vigente-limitación (sin subsistema estándar; medición puntual M8-05 en su lugar) | [performance.md](performance.md#sin-un-subsistema-de-métricas-estándar) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §93 | Enum genérico de perfil de rendimiento (`dev/ci/release/benchmark/size`) | vigente-limitación (no existe; `rust.binary.bloat` cubre "size" concretamente) | [performance.md](performance.md#sin-un-subsistema-de-métricas-estándar) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |
| §95 / §61 / §97 | Matriz aspiracional de 5 triples de host / cross-platform como criterio de 1.0 | sustituido (ADR-087 redefine 1.0 a macOS ARM64 + gateway Docker Linux ARM64; plan de portabilidad pendiente) | [execution-and-security.md](execution-and-security.md#frontera-de-distribución-por-qué-vigente-casi-siempre-significa-macos-arm64) | [rust-engineering-mcp-propuesta-v0.3.md](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) |

El resto de cláusulas de la especificación (`vigente-implementado`,
`descriptivo`, u otras `vigente-limitación`/`pendiente-autorizado` cuyo
destino cae en `docs/guides/`, `docs/reference/`, `docs/operations/` o
`docs/development/`) quedan resueltas por sus capítulos correspondientes.
La especificación original sigue disponible íntegra en el historial de Git
en [`docs/spec/rust-engineering-mcp-propuesta-v0.3.md` en `51fa602e`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/spec/rust-engineering-mcp-propuesta-v0.3.md)
para verificar cualquier cláusula contra su redacción original al retirar
[`docs/spec/`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/spec)
del árbol vigente.

## Cómo registrar nuevas decisiones

No se recrea un directorio de 90 ADR. Una decisión nueva que cambie
contrato público, arquitectura, seguridad, persistencia, compatibilidad
MCP, distribución o una dependencia estratégica se registra como una
**subsección dentro del capítulo que la posee**, no como un archivo nuevo:

1. Elegir el capítulo correcto (`overview.md`, `domain-and-application.md`,
   `mcp-and-contracts.md`, `execution-and-security.md`, `mutation.md`,
   `catalog-and-search.md`, `jobs-and-artifacts.md`, `analyzer.md`,
   `performance.md`, o el capítulo de `reference/`/`operations/`/
   `development/` correspondiente).
2. Añadir una subsección con encabezado `## D-AAAA-MM-DD-<slug>: <título>`
   (fecha ISO, slug corto en kebab-case) y, dentro de ese encabezado:
   **Context** (por qué se decide ahora), **Decision** (qué se decide),
   **Alternatives considered** (qué se rechazó y por qué), **Consequences**
   (qué cambia para el producto/agente/operador) y **Status** (`Accepted`/
   `Superseded by D-...`/etc).
3. Añadir una fila nueva a la tabla de este documento: `D-AAAA-MM-DD-<slug>`
   en la columna ADR, el título, el estado, la sección canónica (con su
   anchor real) y, como fuente histórica, el permalink al commit donde se
   introdujo esa subsección (no un ADR — las decisiones nuevas no generan
   archivos en [`docs/adr/`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/adr)).
4. Si la decisión sustituye una anterior (ADR o `D-...`), actualizar la fila
   de la decisión sustituida (`sustituido por D-AAAA-MM-DD-<slug>`) en vez
   de borrarla.

Los 90 ADR históricos permanecen únicamente en el historial de Git bajo los
permalinks de la tabla anterior; no se restauran como archivos en el árbol.
