# Estado de implementación — Rust Engineering MCP

Actualizado: 2026-09-05

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
| Especificación/instrucciones | v0.3.1 y AGENTS.md revisados; decisiones ADR-001..048. | spec, ADRs y dispositions de reviewers. |
| Código Rust | Ocho crates: domain, application, MCP, project, execution, catalog, semantic y artifact. | Workspace real; domain soloSerde, application soloDomain. |
| MCP | Trece tools implementadas: project.open, project.inspect, toolchain.inspect, check, fmt.check, clippy, test, dependencies.audit, diagnostics.explain, quality.gate, catalog.status, crate.search y crate.inspect; Resources owner-bound; contratos tipados y cinco versiones wire probadas. | protocol/contract tests; M0-03/04/07. |
| Seguridad | I/O propio macOS/APFS no-follow; gateway Docker Linux ARM64 de probes y camino Rust ADR-031 revisado. | M0-04/05/06; [calibración Rust](validation/M1-01-rust-gateway.md), integración MCP ADR-032 validada. |
| Datos locales | SQLite/FTS5 autoritativo, E5 verificado/LanceDB derivado con persistencia verificada, ArtifactStore efímero. | M0-08/09/10a, fixtures y tests reales. |
| Pruebas/fixtures | Gate final M1-17: 23/23 etapas; 644 workspace tests y un doctest separados, más seguridad, catálogo, semántica, release tooling y doctor. | [Recibo full v2 final](validation/m1-17-final-gate-v2.json). |
| CI/release | CI pública final verde en Linux x86_64, macOS ARM64, Windows x86_64 y supply chain; SonarCloud verde; release estable `v0.1.0` publicada para macOS ARM64 con hashes, smoke y attestations. | ADR-048, [recibo público final](validation/m1-17-public-release.json), [release v0.1.0](https://github.com/pharos-lang/rust-engineering-mcp/releases/tag/v0.1.0) y [full gate](validation/m1-17-final-gate-v2.json). |
| Toolchain | Rust/Cargo1.98.1, edition2024, rustfmt/Clippy; host aarch64-apple-darwin. | rust-toolchain.toml y reporte de gate. |
| Configuración local | YouTrack deshabilitado para este repositorio. | .codex/config.toml; no afecta el producto. |

M0 y M1/0.1.0 están cerradas con evidencia ejecutable y publicación verificable. Un foundation completo no
habilita Cargo arbitrario, distribución estable ni soporte de plataformas
no verificadas. Las limitaciones de M1 se conservan como criterios verificables.

## Resolución de alcance

M1 expone exactamente las trece tools enumeradas en la decisión de alcance inmediato
de la propuesta y en la instrucción del owner. `rust.dependencies.inspect` queda
fuera del contrato público M1 aunque aparezca en la sección descriptiva 23.9; la
metadata necesaria se implementará como soporte interno de `project.inspect`, audit
y catálogo. M2+ permanece fuera de alcance.

ADR-048 define 0.1.0 como cierre compuesto: un único archive core
`aarch64-apple-darwin` verificado y un full gate `local` source-bound en macOS26
ARM64/APFS con el gateway guest Docker Linux ARM64. Linux/Windows son CI
portable/fail-closed. No se distribuyen modelo, ORT, LanceDB, catálogo, trust,
fixtures, Docker ni toolchain; no existe catálogo oficial ni clave Ed25519 de producción 0.1.0.

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
| M1-15 | Documentación/release | Todos | Done | El [archive core local](release/0.1.0-local-artifact-receipt.json) pasó inventory/SBOM/notices/manifest/hash; el workflow tag-bound reconstruyó los bytes públicos, verificó instalación/smoke y attestations, y publicó la [release v0.1.0](https://github.com/pharos-lang/rust-engineering-mcp/releases/tag/v0.1.0). [Recibo final](validation/m1-17-public-release.json). |
| M1-16 | Experimentos acotados | M1-01..15 | Done | [Piloto v2](validation/M1-16.md): techo12/12 en ambos brazos, sin equivalencia/causalidad y con mayor costo B. [Benchmark retrieval](research/m1-16/benchmark/REPORT.md): una ejecución descriptiva8queries/15crates, sin claim general de calidad, multilingüe o utilidad de agente. |
| M1-17 | Gate 0.1.0 | Todos | Done | [Full v2 23/23](validation/m1-17-final-gate-v2.json), archive/smoke, Inspector 2.5.0, [stock Codex model-directed](validation/M1-17-codex-model.md), revisión Opus 5 sin P0/P1, PRs protegidos, CI final, tag, attestations y [release pública](validation/m1-17-public-release.json) pasaron. |

## Backlog inmediato

M0/M1 conservan Done. La [baseline live](roadmap/baseline-2026-09-05.md) distingue
la release histórica del HEAD público actual. La planificación [M2–M8](roadmap/m2-m8.md)
contiene [trazabilidad](roadmap/traceability-m2-m8.md), [decisiones Proposed](roadmap/adr-backlog-m2-m8.md)
y [validación/reviews](roadmap/planning-validation.md).

| Milestone | Estado de planificación | Plan / prompt de ejecución separado |
| --- | --- | --- |
| M2 / 0.2.x | Planned | [Safe Mutation](roadmap/m2-safe-mutation.md) · [prompt M2](prompts/implement-m2.md) |
| M3 / 0.3.x | Planned | [Quality](roadmap/m3-quality.md) · [prompt M3](prompts/implement-m3.md) |
| M4 / 0.4.x | Planned | [Security](roadmap/m4-security.md) · [prompt M4](prompts/implement-m4.md) |
| M5 / 0.5.x | Planned | [Performance](roadmap/m5-performance.md) · [prompt M5](prompts/implement-m5.md) |
| M6 / 0.6.x | Planned | [Analyzer](roadmap/m6-analyzer.md) · [prompt M6](prompts/implement-m6.md) |
| M7 / 0.7.x | Conditional; ejecución Deferred sin Go | [Remote](roadmap/m7-remote.md) · [prompt M7](prompts/implement-m7.md) |
| M8 / 0.8–0.9 / readiness 1.0 | Planned | [Stabilization](roadmap/m8-stabilization.md) · [prompt M8](prompts/implement-m8.md) |

El owner autoriza cerrar e integrar primero esta planificación y después implementar
solo M2. Esta tabla registra la fase documental; no acredita implementación nueva
ni autoriza avanzar a M3 o publicar otra release.

## In Progress

No hay vertical M0/M1 en progreso. La fuente, CI portable, SonarCloud, artifact y
release están enlazados desde el [recibo final](validation/m1-17-public-release.json).

## Blocked

No hay bloqueo M0/M1 ni decisión de alcance pendiente para 0.1.0. ADR-048 mantiene
fuera de esta release los artifacts/plataformas/assets no calificados y el catálogo
oficial. La [matriz M1-17](validation/M1-17-matrix.md) conserva esas limitaciones.

## Done

M0-00..12, M0-10a y M1-01..17: cada fila enlaza pruebas, revisión e integración
correspondientes.
Los números de los reportes por corte son históricos; el total observado más
reciente se registra en el assessment y su evidencia enlazada. M0-12 conserva
el cierre histórico de M0. El handoff M1 sustituye los prompts antiguos.

## Technical Debt

- La propuesta contiene ejemplos con versiones placeholder (`1.xx`) y referencias
  temporales; la implementación debe generar datos reales, no copiarlos.
- El layout de muchos crates es una propuesta, no un mandato. M0 debe empezar con el
  mínimo de crates que preserve fronteras reales y medir el costo de compilación.
- El benchmark retrieval acotado describe una sola proyección y no demuestra calidad
  general, cobertura multilingüe ni utilidad; sigue siendo una limitación, no deuda de ejecución.
- ADR-047 resolvió licencia dual, copyright y canal de fuente. Esto no resuelve las
  licencias/notices de terceros ni autoriza distribuir modelos o binarios.
- `scripts/gate.py` incorpora reportes v2 con timestamps/conteos directos; el gate
  M1-17 histórico conserva honestamente inicio desconocido y conteos derivados.

## Decisions Pending

| Decisión | Momento límite | Gate |
| --- | --- | --- |
| Catálogo oficial futuro | Antes de una release que lo distribuya | Nueva decisión de fuente/términos y procedimiento de custodia/rotación/revocación; no aplica a 0.1.0. |
| Soporte positivo adicional por OS | Antes de anunciar otro target | Adapter protegido y security tests nativos; CI portable no basta. |
| Distribución futura del perfil `local` | Antes de empaquetar E5/ORT/LanceDB | Licencias/notices y recibos nativos completos; excluido de 0.1.0. |

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
