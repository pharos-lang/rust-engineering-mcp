# Documentación de Rust Engineering MCP

Índice de la documentación canónica. Cada entrada resume el documento en una
línea; el contenido vive en el archivo enlazado, no aquí.

## Guías (`guides/`)

- [`installation.md`](guides/installation.md) — instalar la release publicada o compilar desde fuente con el toolchain fijado.
- [`configuration.md`](guides/configuration.md) — todas las flags de `serve --stdio`, roots, grants, políticas y rutas de datos.
- [`clients.md`](guides/clients.md) — configuración por cliente MCP y evidencia real de calificación.
- [`workflows.md`](guides/workflows.md) — flujos típicos de un agente: inspección, quality gate, mutación, catálogo, analyzer.
- [`troubleshooting.md`](guides/troubleshooting.md) — errores reales (`SANDBOX_DENIED`, etc.), `doctor` y configuraciones incorrectas comunes.

## Referencia (`reference/`)

- [`tools.md`](reference/tools.md) — contrato, límites y ejemplos de cada tool MCP.
- [`cli.md`](reference/cli.md) — subcomandos del binario (`doctor`, `catalog`, `mutation`, `contract`, `cargo-vendor`, `quality-artifacts`, `security-runtime`).
- [`compatibility.md`](reference/compatibility.md) — matriz de plataformas, protocolo MCP y política de deprecación/freeze.
- [`limits.md`](reference/limits.md) — límites numéricos por tool y por store (tamaños, timeouts, cuotas).
- [`data-formats.md`](reference/data-formats.md) — formatos de catálogo, journal, artifacts y otros datos versionados.

## Arquitectura (`architecture/`)

- [`overview.md`](architecture/overview.md) — visión general, capas hexagonales y decisiones de lenguaje/runtime.
- [`domain-and-application.md`](architecture/domain-and-application.md) — dominio, casos de uso y ports, independientes de MCP.
- [`mcp-and-contracts.md`](architecture/mcp-and-contracts.md) — frontera `rmcp`, envelope de resultado, Resources, transporte.
- [`execution-and-security.md`](architecture/execution-and-security.md) — Execution Gateway, deny-by-default, I/O no-follow, sandbox.
- [`mutation.md`](architecture/mutation.md) — `local_coordinated`, journal, planes/receipts y sus límites.
- [`catalog-and-search.md`](architecture/catalog-and-search.md) — SQLite autoritativo, LanceDB derivado, búsqueda híbrida.
- [`jobs-and-artifacts.md`](architecture/jobs-and-artifacts.md) — MCP Tasks, store de artifacts de calidad, Resources.
- [`analyzer.md`](architecture/analyzer.md) — integración rust-analyzer y las tools `rust.analyzer.*` (preview).
- [`performance.md`](architecture/performance.md) — benchmarking, profiling y binary bloat.
- [`decisions.md`](architecture/decisions.md) — mapa compacto de ADRs (ID original → estado → capítulo).

## Operación (`operations/`)

- [`runtime-provisioning.md`](operations/runtime-provisioning.md) — construir/cargar las imágenes Docker aprobadas y qué sirve cada una.
- [`catalog-maintenance.md`](operations/catalog-maintenance.md) — CLI de sync/import/rebuild del catálogo; el runtime nunca descarga.
- [`backup-and-recovery.md`](operations/backup-and-recovery.md) — backup, restore y rollback de binario/estado.
- [`release-verification.md`](operations/release-verification.md) — verificación offline de releases y respuesta a incidentes de publicación.

## Desarrollo (`development/`)

- [`testing.md`](development/testing.md) — gate local, CI y pruebas de contrato/protocolo/seguridad.

## Otros documentos del repositorio

- [`../SECURITY.md`](../SECURITY.md) — política de reporte de vulnerabilidades.
- [`../CONTRIBUTING.md`](../CONTRIBUTING.md) — cómo contribuir y qué gate ejecutar antes de un PR.
- [`../CHANGELOG.md`](../CHANGELOG.md) — cambios por versión relevantes para el usuario.

## Historia

La especificación original, los ADRs individuales y la evidencia de cada
milestone (M0–M8) no se conservan como árbol de documentación operativa; viven
en el historial de Git, resolubles desde el commit `51fa602e` en adelante. Los
capítulos de `architecture/` y `decisions.md` resumen lo vigente con referencia
a ese commit; no hace falta recorrer los ADRs originales para operar el MCP.
