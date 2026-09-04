# Estado de implementación — Rust Engineering MCP

Actualizado: 2026-09-04

Fuente principal: [`spec/rust-engineering-mcp-propuesta-v0.3.md`](spec/rust-engineering-mcp-propuesta-v0.3.md)

## Leyenda

| Estado | Significado |
| --- | --- |
| Not started | No existe implementación verificable. |
| In progress | Existe trabajo activo, todavía sin gate completo. |
| Blocked | No puede avanzar sin una decisión o dependencia externa concreta. |
| Done | Entregable presente y evidencia reproducible registrada. |

## Assessment del repositorio

| Área | Estado real | Evidencia |
| --- | --- | --- |
| Historial | M0-01..12 integrados mediante ramas ai/ y merges no-ff locales. | Cada validation/M0-*.md registra integración; no remoto. |
| Especificación/instrucciones | v0.3.1 y AGENTS.md revisados; decisiones ADR-001..045. | spec, ADRs y dispositions de reviewers. |
| Código Rust | Ocho crates: domain, application, MCP, project, execution, catalog, semantic y artifact. | Workspace real; domain soloSerde, application soloDomain. |
| MCP | Trece tools implementadas: project.open, project.inspect, toolchain.inspect, check, fmt.check, clippy, test, dependencies.audit, diagnostics.explain, quality.gate, catalog.status, crate.search y crate.inspect; Resources owner-bound; contratos tipados y cinco versiones wire probadas. | protocol/contract tests; M0-03/04/07. |
| Seguridad | I/O propio macOS/APFS no-follow; gateway Docker Linux ARM64 de probes y camino Rust ADR-031 revisado. | M0-04/05/06; [calibración Rust](validation/M1-01-rust-gateway.md), integración MCP ADR-032 validada. |
| Datos locales | SQLite/FTS5 autoritativo, E5 verificado/LanceDB derivado con persistencia verificada, ArtifactStore efímero. | M0-08/09/10a, fixtures y tests reales. |
| Pruebas/fixtures | Gate candidato M1-17: full19/19; 644 workspace tests y un doctest en etapas separadas, además de seguridad, catálogo, semántica y doctor. | [M1-17 full actual](validation/M1-17-final-gate.md); [conteos](validation/m1-17-final-gate/counts-derived-from-log.json). |
| CI/release | CI local core/full y GitHub CI/CD implementadas; full19 candidato pasó en macOS ARM64; sin release binaria. | scripts/gate.py, `.github/workflows/`, [recibo M1-17](validation/M1-17-final-gate.md); notices y matriz nativa pendientes. |
| Toolchain | Rust/Cargo1.98.1, edition2024, rustfmt/Clippy; host aarch64-apple-darwin. | rust-toolchain.toml y reporte de gate. |
| Configuración local | YouTrack deshabilitado para este repositorio. | .codex/config.toml; no afecta el producto. |

M0 está cerrada; M1-01..14 implementadas con evidencia por vertical. M1/0.1.0 todavía no está cerrado. Un foundation completo no
habilita Cargo arbitrario, distribución estable ni soporte de plataformas
no verificadas. Las limitaciones de M1 se conservan como criterios verificables.

## Resolución de alcance

M1 expone exactamente las trece tools enumeradas en la decisión de alcance inmediato
de la propuesta y en la instrucción del owner. `rust.dependencies.inspect` queda
fuera del contrato público M1 aunque aparezca en la sección descriptiva 23.9; la
metadata necesaria se implementará como soporte interno de `project.inspect`, audit
y catálogo. M2+ permanece fuera de alcance.

## M0 — Foundation

| ID | Corte / entregable | Estado | Definition of Done y evidencia requerida |
| --- | --- | --- | --- |
| M0-00 | Baseline repo-visible | Done | `AGENTS.md`, este tablero y ADR-001..ADR-020 revisados; links internos válidos. |
| M0-01 | Bootstrap del workspace | Done | Workspace/binario ejecutable, fmt/clippy/test config y docs iniciales; 8 tests, revisión Sonnet 5 y smoke post-merge `c86a82a` con 1.97.1: [evidencia histórica](validation/M0-01.md). Upgrade posterior a 1.98.1 en `cafe721`, validado en M0-02. |
| M0-02 | Dominio y contratos base | Done | `ProjectRef`, fingerprints distintos, resultados/errores, diagnostics, provenance/freshness y Clock; dominio libre de adapters. ADR-022, 21 tests de dominio + 1 compile-fail, revisión Sonnet 5 y gate post-merge `13b97c7`: [evidencia](validation/M0-02.md). |
| M0-03 | Bootstrap MCP stdio | Done | `rmcp` 3.2.0, discovery 2026-07-28 y cuatro versiones legacy; stdout solo protocolo, lista vacía determinista, 14 tests de protocolo, límite de entrada y fallos de I/O. ADR-023, revisión Sonnet 5 resuelta, check independiente del SDK y gate post-merge `910bb0b`: [evidencia](validation/M0-03.md). |
| M0-04 | `project.open` vertical | Done | Roots host, I/O relativo no-follow, workspace estructural (ADR-024), handle opaco y fingerprints separados; filesystem/races/invalid-ref y Cargo oracle. Windows/Linux fail-closed; junction enforcement pendiente de adapter y CI. Opus 5, 98 pruebas y merge `24545c4`: [evidencia](validation/M0-04.md). |
| M0-05 | Execution Gateway | Done | Allowlist tipada, `env_clear`, cwd validado, timeout, cancelación y containment real; Docker/Linux arm64, probes cerrados sin Cargo; 119 pruebas y revisión Opus 5 resuelta por el principal. [Evidencia](validation/M0-05.md). |
| M0-06 | Sandbox/capability detection | Done | Implementa ADR-009; strict/restricted fallan cerrados; CLI activa, perfiles control explícitos, oráculos reales de red/env/fs/races/children/wall/output/CPU/RAM/PID/disk. Scope imagen de probes, sin Cargo; 133 pruebas y revisión Opus 5 resuelta: [evidencia](validation/M0-06.md). |
| M0-07 | Contrato MCP | Done | Frontera genérica tipada usada por project.open, schemas cerrados, Serde y mapping de los cinco estados; snapshot intacto, 132 tests + doctest y revisión Sonnet 5: [evidencia](validation/M0-07.md). |
| M0-08 | SQLite catalog foundation | Done | SQLite3.53.2/FTS5, migrations transaccionales, snapshots bytes readonly, summaries latest_known/SemVer, límites, provenance/freshness. 147 tests + doctest, audit160 limpio y Opus5 resuelto: [evidencia](validation/M0-08.md). |
| M0-09 | Semantic foundation | Done | E5 real verificado sin downloads, ORT sin telemetry, LanceDB memory://, identidad/rebuild/fallback y facts SQLite. 156 tests core + doctest, 7 adicionales semánticos; Opus5 resuelto y gate offline real: [evidencia](validation/M0-09.md). |
| M0-10 | Fixtures | Done | Nueve fixtures + adversario fuente; 11 casos Cargo1.98.1 y oracle estático RSA, receipt pre-Cargo, Sonnet5 resuelto: [evidencia](validation/M0-10.md). |
| M0-10a | ArtifactStore mínimo | Done | Memoria efímera ADR-028, streaming/redacción, cuotas/TTL, owner-bound; 17 tests nuevos, oracle229950, 173 core + doctest y Opus5 resuelto: [evidencia](validation/M0-10a.md). Resource MCP en M1. |
| M0-11 | CI inicial | Done | CI local core/full ADR-029,10 etapas core verdes, deny/audit sin vulnerabilidades, matriz honesta y prerequisitos fail-closed; Sonnet5 resuelto: [evidencia](validation/M0-11.md). Full en M0-12. |
| M0-12 | Gate M0 | Done | Full gate12 etapas,185 tests Rust distintos, corpus11+1; Opus5 High resuelto, recibos y [evidencia](validation/M0-12.md); [prompt M1](prompts/continue-m1.md). |

## M1 — MVP / 0.1.0

| ID | Corte vertical | Depende de | Estado | Criterio verificable |
| --- | --- | --- | --- | --- |
| M1-01 | `rust.project.inspect` | M0-02,04,05,06 | Done | ADR-032; contrato/MCP real, metadata declarada, provenance/freshness, ProjectRef final y cleanup joined. Core277/10 etapas; cuatro tests Rust/MCP reales; Sonnet5 y Opus5 con dispositions. [Evidencia](validation/M1-01.md). |
| M1-02 | `rust.toolchain.inspect` | M0-05,06,07; M1-01 | Done | ADR-033; inventario instalado, tres comandos/fingerprints, gateway compartido y referencia revalidada. Core293/10 etapas, Rust/MCP real4/4; Sonnet5 sin findings confirmados. [Evidencia](validation/M1-02.md). |
| M1-03 | `rust.check` | M0-05,06,07,10; M1-01/02 | Done | [Evidencia](validation/M1-03.md): core332/10stages;6tests Docker exactos, E0502/E0106, locks frozen, Resources live, cleanup activo; Opus5 High+Medium y disposición principal. |
| M1-04 | `rust.fmt.check` | M0-05,06,07,10 | Done | ADR-035; core355/10stages,7tests Docker exactos, estilo/workspace/newlines/diff grande y siete logs verificados; Sonnet5 y disposition principal. [Evidencia](validation/M1-04.md). |
| M1-05 | `rust.clippy` | M0-05,06,07,10 | Done | ADR-036; core372/10stages,9tests Docker exactos,6casos MCP/perfiles/logs y2fixtures hostiles Clippy; Sonnet5 y disposición principal. [Evidencia](validation/M1-05.md). |
| M1-06 | `rust.test` | M0-05,06,07,10 | Done | ADR-037; core393/10etapas;13tests Docker,9casos MCP/logs, R2 descendientes timeout/cancel/overflow y MCP activo cancel/EOF; falsificación proc-macro confirmada/corregida y Opus5. [Evidencia](validation/M1-06.md). |
| M1-07 | `rust.dependencies.audit` | M0-05,06,08,10 | Done | ADR-038; core455/10etapas;16tests Docker y15casos audit finales; RustSec/SQLite real bajo network deny macOS; Opus5 y disposición. [Evidencia](validation/M1-07.md). |
| M1-08 | `rust.diagnostics.explain` | M0-05,06,07 | Done | ADR-039; core474/10etapas; MCP10casos sin proyecto, rustc real E0502/E9999, calibración6escenarios; Sonnet5 y disposición. [Evidencia](validation/M1-08.md). |
| M1-09 | `rust.quality.gate` | M1-03..08 | Done | ADR-040; captura única, etapas completas, runtime/freshness, logs agrupados/rollback y límites. Core498; full14/14,20 tests Rust reales y E5/LanceDB bajo network deny; Opus5 con disposición y seguimiento. [Evidencia](validation/M1-09.md). |
| M1-10 | Catalog CLI | M0-08,09,10 | Done | ADR-041; firmas/hashes/USTAR, floor durable/recovery/key rotation, HTTPS y native E5/Lance import/rebuild. Full15/15 pre-observabilidad; core540/Clippy all-features/CLI5+1 posteriores, fuentes separadas; Opus5/Sonnet5 y disposición. [Evidencia](validation/M1-10.md). |
| M1-11 | `rust.catalog.status` | M0-08,09 | Done | ADR-042; readonly, identidad/freshness por componente, floor/cache, RustSec independiente; core572, wire33, Clippy all-features y native E5/index2+1 network deny. Sonnet5/revisión principal; [evidencia](validation/M1-11.md). |
| M1-12 | `rust.crate.search` | M0-08,09 | Done | ADR-043; core603/10stages, wire35, Clippy all-features y native E5/index2+1 bajo network deny; filtros SQLite, ranks y fallback explícitos, budget MCP512KiB. Sonnet5/revisión principal; [evidencia](validation/M1-12.md). |
| M1-13 | `rust.crate.inspect` | M0-08 | Done | ADR-044; core629, wire37, Clippy all-features y2 CLI/MCP bajo network deny; pages por versión/fingerprint, unknown explícitos y budget512KiB. Sonnet5/revisión principal; [evidencia](validation/M1-13.md). |
| M1-14 | CLI y doctor | M0/M1 anteriores | Done | ADR-045; core645/10stages,37 protocolo,4 casos activos con SIGINT/TERM/HUP y cleanup,2 stdout bloqueados. JSON/humano y parser host compartido; Opus5 y disposición. [Evidencia](validation/M1-14.md). |
| M1-15 | Documentación/release | Todos | **Blocked** | ADR-047: fuente pública `MIT OR Apache-2.0`, IUMotion Labs y GitHub resueltos. Candidatos/instalación/doctor locales verificados. Kanaria/E5/ORT, notices finales por target, runners y clave Ed25519 de catálogo aún bloquean binarios/catálogos. |
| M1-16 | Experimento de utilidad | M1-01..15 | Done | [Piloto v2 medido](validation/M1-16.md): 24/24 runs, ambos brazos12/12 y12 pares both-pass. Endpoint saturado sin poder discriminante ni evidencia de equivalencia; B observó más solicitudes/tiempo/tokens. Sin inferencia causal/poblacional. |
| M1-17 | Gate 0.1.0 | Todos | **Blocked** | [Evidencia y matriz](validation/M1-17.md): full19 macOS ARM64, Inspector13/13, stock Codex directo y revisión Opus5 completados. Linux/Windows/x86, notices y clave Ed25519 de catálogo bloquean M1; uso model-driven en stock Codex no se probó. |

## Backlog inmediato

1. M1-10..16 están integradas y la calificación local M1-17 está documentada. Resolver únicamente los bloqueos explícitos de runners nativos, licencias/notices y decisiones del owner. No avanzar a M2.

## In Progress

No hay vertical de implementación activa. M1-17 conserva evidencia local completa y
un cierre formalmente bloqueado; M1 no está cerrado.

## Blocked

Ningún bloqueo de foundation pendiente. M1-15/M1-17 están bloqueados por runners
nativos Linux/Windows/x86 aplicables; Kanaria/E5/ORT/notices finales; y custodia,
rotación/revocación de la clave Ed25519 de catálogos de producción. La
[matriz M1-17](validation/M1-17-matrix.md) mantiene cada categoría separada.

## Done

M0-00..12 y M0-10a: cada fila enlaza pruebas y revisión/integración correspondiente.
Los números de los reportes por corte son históricos; el total observado más
reciente se registra en el assessment y su evidencia enlazada. M0-12 conserva
el cierre histórico de M0. El handoff M1 sustituye los prompts antiguos.

## Technical Debt

- La propuesta contiene ejemplos con versiones placeholder (`1.xx`) y referencias
  temporales; la implementación debe generar datos reales, no copiarlos.
- El layout de muchos crates es una propuesta, no un mandato. M0 debe empezar con el
  mínimo de crates que preserve fronteras reales y medir el costo de compilación.
- No hay todavía benchmark que demuestre la calidad del modelo local ni el overhead
  de LanceDB; ADR-019 impone un gate antes de estabilizarlos.
- ADR-047 resolvió licencia dual, copyright y canal de fuente. Esto no resuelve las
  licencias/notices de terceros ni autoriza distribuir modelos o binarios.
- `scripts/gate.py` debe registrar timestamps exactos de inicio/fin y conteos por
  etapa en el reporte; el gate M1-17 conserva honestamente un inicio desconocido y
  conteos derivados del log.

## Decisions Pending

| Decisión | Momento límite | Gate |
| --- | --- | --- |
| Clave Ed25519 de catálogo: custodia, rotación y revocación | Antes de distribuir un catálogo firmado | Responsable y procedimiento explícitos; fixture seed42 prohibida. GitHub artifacts usan OIDC sin clave persistente. |
| Soporte fuerte de sandbox por OS | Antes de habilitar R1/R2 en cada target | Security tests reales por plataforma y tabla de capabilities. |
| Distribución estable y benchmark del modelo | Antes de RC M1 | E5 revision/hashes locales fijados en M0-09; falta calidad ES/EN, CPU/RAM/startup, recibos nativos por target y licencia de distribución. |
| Fuente/licencia del snapshot global | Antes de `catalog sync` público | Revisión de términos del registry, formato firmado y provenance. |
| Matriz mínima de CI ARM64/x86_64 | Antes del RC 0.1.0 | Runners disponibles y limitaciones de LanceDB/ONNX documentadas. |

## Riesgos activos

| Riesgo | Impacto | Mitigación / evidencia requerida |
| --- | --- | --- |
| “network deny” falso | Crítico | ADR-009: fail closed; prueba con proceso que intenta red. |
| `build.rs`/proc macros durante check | Crítico | Clasificar R1 como ejecución potencial; sandbox y opt-in documentado. |
| Escape por symlink/junction/TOCTOU | Alto | I/O relativo a handles no-follow/reparse-safe y fixtures concurrentes; canonicalización no basta (ADR-007). |
| Proceso hijo huérfano | Alto | Containment fuerte y fixture de descendiente desacoplado; process group solo best-effort (ADR-008). |
| Supply chain pesada (LanceDB/ONNX) | Alto | Features aisladas, binario medido, lockfile, audit/deny/SBOM. |
| Staleness presentado como live | Alto | Tipos que obliguen provenance/freshness y contract tests. |
| Presupuestos/cancelación de stdio | Medio | ADR-023: input por línea limitado; concurrencia/salida global, clientes lentos y primer request largo requieren controles antes de tools costosas. |
| Drift de MCP/rmcp | Medio | Versión fijada, protocolo negociado y compatibility matrix por release. |
| Scope creep | Medio | Las trece tools anteriores son el único contrato M1. |

M1-01 integrated52139e6/a726d18; clean-main post-merge MCP runtime smoke2/2.
M1-02 ADR-033 starts from clean main in ai/m1-02-toolchain.

M1-02 integratedf6c5c59/882fb6e; actual shared-inspection post-merge smoke1/1.
M1-03 ADR-034 validated on ai/m1-03-check: core332,6exactDocker tests, reviews/disposition tracked. Integrado `96fc984`/`4ddc696`; smoke MCP/Resources real post-merge1/1 (18.72s).

M1-07 decisión previa: ADR-038, snapshot RustSec propio con SHA esperado por host,
SQLite autoritativo y matcher oficial sin Git/HTTP. Aprovisionamiento explícito
de dependencia de desarrollo; import firmado/antirollback durable siguen M1-10.

M1-07 integrada be74318/c6236af; M1-08 comienza en ai/m1-08-explain desde main limpio, ADR-039 previo a código.

M1-08 integrada571469d/897268c, smoke real1/1 (16.33s). M1-09 inicia desde main limpio en ai/m1-09-quality-gate; ADR-040 previo a código.

M1-09 integrada983e5ad/cc04f0c; rama conservada. Smoke real standard1/1,3casos,
39.58s y12hashes de logs verificados en main limpio. [Recibo](validation/M1-09-postmerge.json).
El árbol de código coincide con el full14/14; registro final solo documental.

M1-13 integrada08e41f3/392a8f2; smoke2 inspect/37 protocolo y296 hashes verificados.
M1-14 implementada con evidencia; M1 aún no cerrado.

M1-14 integrada a72216d/20689cf; main limpio para smoke3 doctor +9 capabilities
+37 protocolo;304 hashes verificados y Clippy all-features final aprobado.
