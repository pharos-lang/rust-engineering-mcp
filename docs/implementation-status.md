# Estado de implementación — Rust Engineering MCP

Actualizado: 2026-09-08

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
| Especificación/instrucciones | v0.3.1 y AGENTS.md revisados; decisiones ADR-001..059. | spec, ADRs y dispositions de reviewers. |
| Código Rust | Ocho crates: domain, application, MCP, project, execution, catalog, semantic y artifact. | Workspace real; domain soloSerde, application soloDomain. |
| MCP | Trece tools M1 estables; checkout 0.3.0-dev conserva las cinco M2 e integra nextest, coverage, SemVer y mutation (22 tools). Tasks está anunciado y exige declaración mutua. | protocol/contract tests; [matriz M3](validation/M3-matrix.md), [M3-02](validation/M3-02.md), [M3-04](validation/M3-04-semver-calibration.md) y [M3-05](validation/M3-05-mutation-calibration.md). |
| Seguridad | I/O propio macOS/APFS no-follow; gateway Docker Linux ARM64 de probes y camino Rust ADR-031 revisado. | M0-04/05/06; [calibración Rust](validation/M1-01-rust-gateway.md), integración MCP ADR-032 validada. |
| Datos locales | SQLite/FTS5 autoritativo, E5 verificado/LanceDB derivado, ArtifactStore M1 efímero y store privado M3 persistente disponible en macOS ARM64/APFS. | M0-08/09/10a; [ADR-061](adr/ADR-061-private-quality-artifact-store.md), CLI y tests de calidad. |
| Imagen guest M3 | Imagen Linux ARM64 provisionada con plugins exactos; nextest calificado con el perfil mínimo quality de ADR-064. | [Digest/configuración](validation/M3-image-config.json), [provisioning](validation/M3-provisioning.json), [M3-01](validation/M3-01.md). |
| Pruebas/fixtures | Suite workspace no-Docker: 1,105 pasaron + 1 doctest y 0 fallaron; además, runtime Docker M3 62/62 y seguridad Rust 20/20. Medido sobre el tip final de la rama y re-medido sobre `main` tras el merge, que no cambió ningún byte. | [M3 runtime](validation/M3-runtime.json), [M3 security](validation/M3-rust-security.json), [integración](validation/M3-integration.json). |
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
y catálogo. Autorizaciones posteriores cerraron M2 e iniciaron los cortes M3-01 a
M3-05; M3-02 completó G4 y habilitó el anuncio de Tasks con negociación mutua.
M3-06 cerró el milestone y está integrado en `main` mediante el PR #14; otra
release permanece fuera de alcance.

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

## M2 — Safe Mutation / 0.2.x

Done con calificación conjunta y sin release nueva: 18 tools, los trece contratos
M1 intactos, full posterior a ADR-059, cliente stock y revisiones Accepted.
[Cierre, límites e integración](validation/M2-07.md). Integrado localmente con
merge no-ff `7554bcc` y [smoke y hashes](validation/M2-local-integration.json)
aprobados; llegó a `main` dentro del mismo PR #14 que M3. El detalle histórico
por corte permanece más abajo en este mismo tablero.

| ID | Corte | Estado | Evidencia / límite actual |
| --- | --- | --- | --- |
| M2-01 | Lints con preview/commit/receipt y Scratch | Done (histórico) | [Core 14/14](validation/M2-02-core-gate.json), [runtime](validation/M2-02-runtime-gate.json), [revisión](reviews/M2-01-review.md). |
| M2-02 | `rust.fmt.apply` y writer nativo | Done (histórico) | [fmt runtime 2/2](validation/M2-02-runtime-gate.json), [contrato Sonnet](reviews/M2-02-contract-review.md), [writer Opus](reviews/M2-02-native-review.md). |
| M2-03 | `rust.fix.apply` con perfil dedicado | Done | [ADR-056](adr/ADR-056-cargo-fix-isolated-loopback.md) y [máscara socket real](validation/M2-fix-socket-mask.json); no amplía el seccomp M1. |
| M2-04/05 | `rust.dependency.add` / `.remove` | Done | [ADR-055](adr/ADR-055-offline-cargo-data-and-lock-policy.md), [ADR-057](adr/ADR-057-typed-manifest-and-dependency-operations.md), [runtime 4/4](validation/M2-04-runtime-gate.json); vendor optativo y `preserve_presence`. |
| M2-06 | `rust.manifest.patch` tipado | Done | Cuatro familias tipadas sobre el editor LF/CRLF; runtime anterior. [Matriz](validation/M2-matrix.md). |
| M2-07 | Cierre conjunto | Done | [Full 24/24](validation/M2-full-gate.json) sobre 574 inputs con 836 resultados Rust y 1 doctest; [runtime 17/17 en 10 selecciones](validation/M2-final-runtime.json); [cliente PASS](validation/M2-clients.json). [Cierre](validation/M2-07.md) · [Trazabilidad](validation/M2-traceability.md). |

| Decisión / entregable | Estado | Evidencia |
| --- | --- | --- |
| D02 — edición local coordinada | Done (decided) | [ADR-050](adr/ADR-050-local-coordinated-mutation.md); el No-go histórico sigue válido para exclusión OS fuerte, que el producto no anuncia. |
| D05 — datos Cargo offline y política de lock | Done (decided) | [ADR-055](adr/ADR-055-offline-cargo-data-and-lock-policy.md); los probes son evidencia de decisión, no implementación. |
| M2-03 — perfil dedicado de Cargo fix | Done (decided) | [ADR-056](adr/ADR-056-cargo-fix-isolated-loopback.md), aceptado tras D06 (121 observaciones). |
| ADR-059 — cuota RAM del cliente stock | Done (decided) | [Recheck Accepted](reviews/M2-059-review.md), [full posterior](validation/M2-full-gate.json) y [cliente PASS](validation/M2-clients.json). |

## M3 — Quality

| ID | Corte | Estado | Evidencia / límite actual |
| --- | --- | --- | --- |
| M3-01 | `rust.test.nextest` síncrono, gateway, JUnit y artifacts privados | Done (qualified) | [M3-01](validation/M3-01.md): runtime 19/19 dentro de M3-runtime 62/62 y security 20/20; Tasks integrado por M3-02. |
| M3-02 | MCP Tasks y lifecycle job | Done (qualified) | Docker lifecycle 4/4 (D06-T04/T05/T08/T10), budgets con 30 muestras frías y 30 calientes por operación, five-version matrix, Inspector Tasks y Codex fallback; anuncio ON con declaración mutua. [M3-02](validation/M3-02.md). |
| M3-03 | Coverage | Done (qualified) | ADR-065 enmendado; Docker coverage 8/8 dentro de M3 runtime 62/62, rust-security 20/20, counts/formatos/dedupe/zero-denominator fijados: [M3-03](validation/M3-03.md). |
| M3-04 | SemVer baseline | Done (qualified) | Docker 18/18; exits y parser real fijados, roots inmutables, artifacts Stage 1/fallback. [M3-04](validation/M3-04.md). |
| M3-05 | Mutation testing | Done (qualified) | Docker 10/10; exits, outcomes/listas/bundle/identidad fijados; containment, cap y cleanup verificados. [M3-05](validation/M3-05.md). |
| M3-06 | Integración y handoff | Done (qualified) para el alcance local | Core 14/14 y full **25/25** (`audit-data` incluido) sobre 810 inputs; G6 con recibo propio 10/10; aceptación de ADR-064/065 y re-reviews registrados. [M3-07](validation/M3-07.md) · [Rollback](validation/M3-06-rollback.md). |

| Decisión / entregable | Estado | Evidencia |
| --- | --- | --- |
| D06 — MCP Tasks | Done (decided) | [ADR-060](adr/ADR-060-bounded-job-execution-and-mcp-tasks.md) |
| D17 — private quality artifact store | Done (decided) | [ADR-061](adr/ADR-061-private-quality-artifact-store.md) |
| D18 — coverage and semver baselines | Done (decided) | [ADR-062](adr/ADR-062-coverage-accounting-and-semver-baselines.md) |
| M3 guest provisioning | Done (decided) | [ADR-063](adr/ADR-063-m3-guest-plugin-provisioning.md), [receipt](validation/M3-provisioning.json) |
| ADR-064 — perfil seccomp de los jobs de calidad | Accepted 2026-09-06 por el orquestador M3 | [ADR-064](adr/ADR-064-quality-job-seccomp-profile.md): una sola regla sobre el perfil base (`socketpair` AF_UNIX anónimo con flags enmascarados), perfil aplicado verificado contra el declarado por fase; [runtime 62/62](validation/M3-runtime.json), [rust-security 20/20](validation/M3-rust-security.json) y [V-SEC](validation/m3-delegation/V-SEC/last-message.md). |
| ADR-065 — volumen de target para coverage (enmendado) | Accepted 2026-09-06 por el orquestador M3 | [ADR-065](adr/ADR-065-coverage-target-volume.md): tmpfs por job, read-write solo en `CoverageRun`/`CoverageReport`, keeper read-only, ausente de todo exporter y destruido en cleanup; [runtime 62/62](validation/M3-runtime.json), [rust-security 20/20](validation/M3-rust-security.json), [V-SEC](validation/m3-delegation/V-SEC/last-message.md) y [revisión final](validation/m3-delegation/VF-opus-final/last-message.md). |

## Backlog inmediato

M0/M1 conservan Done. La [baseline live](roadmap/baseline-2026-09-05.md) distingue
la release histórica del HEAD público actual. La planificación [M2–M8](roadmap/m2-m8.md)
contiene [trazabilidad](roadmap/traceability-m2-m8.md), [decisiones Proposed](roadmap/adr-backlog-m2-m8.md)
y [validación/reviews](roadmap/planning-validation.md).

| Milestone | Estado de planificación | Plan / prompt de ejecución separado |
| --- | --- | --- |
| M2 / 0.2.x | Done local; sin release nueva | [Safe Mutation](roadmap/m2-safe-mutation.md) · [prompt M2](prompts/implement-m2.md) |
| M3 / 0.3.x | Done; integrado en `main` como `57c4037` (PR #14); sin release nueva | [Quality](roadmap/m3-quality.md) · [matriz M3](validation/M3-matrix.md) · [integración](validation/M3-integration.json) |
| M4 / 0.4.x | Done local; integración mediante PR #15; sin release nueva | [Security](roadmap/m4-security.md) · [handoff M4](validation/M4-handoff.md) · [confirmación final](reviews/m4-final-evidence/review.md) |
| M5 / 0.5.x | In progress; profiling y bloat calificados nativamente sobre la imagen admitida, M5-01 bloqueado, método de comparación en corrección tras revisión G8 | [Performance](roadmap/m5-performance.md) · [matriz M5](validation/M5-matrix.md) · [handoff M5](validation/M5-handoff.md) |
| M6 / 0.6.x | Planned | [Analyzer](roadmap/m6-analyzer.md) · [prompt M6](prompts/implement-m6.md) |
| M7 / 0.7.x | Conditional; ejecución Deferred sin Go | [Remote](roadmap/m7-remote.md) · [prompt M7](prompts/implement-m7.md) |
| M8 / 0.8–0.9 / readiness 1.0 | Planned | [Stabilization](roadmap/m8-stabilization.md) · [prompt M8](prompts/implement-m8.md) |

El owner autorizó M3-01..05 mediante sus paquetes de integración después del
cierre M2. I06 autorizó M3-02 y W4 completó su G4 antes de habilitar Tasks. W5
autorizó el cierre local M3-06 y el owner autorizó después el flujo de PR y el
merge, pero no una release nueva.

## In Progress

No hay implementación M2 ni M3 en progreso: ambos milestones están integrados en
`main` y aparecen en [Done](#done).

No hay vertical M0/M1 en progreso. La fuente, CI portable, SonarCloud, artifact y
release están enlazados desde el [recibo final](validation/m1-17-public-release.json).

## Blocked

No hay bloqueo M3. El cierre dejó de estar bloqueado por gates, código y
decisiones con W7, y el bloqueo restante —la revisión del owner— se resolvió con
el merge del PR #14. Ver [Done](#done) y
[Integración](validation/M3-07.md#integración).

El bloqueo de decisión D02 se resolvió por delegación explícita del owner mediante
[ADR-050](adr/ADR-050-local-coordinated-mutation.md): edición local coordinada sin
broker privilegiado. El No-go histórico sigue siendo válido para exclusión OS fuerte,
que el producto no anuncia. El writer M2-01/02 ya tiene calificación positiva; las ampliaciones se cierran con
el gate conjunto M2.

No hay bloqueo M0/M1 ni decisión de alcance pendiente para 0.1.0. ADR-048 mantiene
fuera de esta release los artifacts/plataformas/assets no calificados y el catálogo
oficial. La [matriz M1-17](validation/M1-17-matrix.md) conserva esas limitaciones.

## Done

M0-00..12, M0-10a y M1-01..17: cada fila enlaza pruebas, revisión e integración
correspondientes. M2-01..07: [matriz](validation/M2-matrix.md), [full final](validation/M2-full-gate.json),
[cliente PASS](validation/M2-clients.json) y [trazabilidad](validation/M2-traceability.md).
Los números de los reportes por corte son históricos; el total observado más
reciente se registra en el assessment y su evidencia enlazada. M0-12 conserva
el cierre histórico de M0. El handoff M1 sustituye los prompts antiguos.

M2-01..07 tiene calificación conjunta: 18 tools, trece contratos M1 intactos,
full posterior a ADR-059, cliente stock y revisiones Accepted.
[Cierre, límites e integración](validation/M2-07.md). Integrado localmente con
merge no-ff `7554bcc`; [smoke y hashes](validation/M2-local-integration.json)
posteriores aprobados. Llegó a `main` dentro del mismo PR #14 que M3.

### M3 — integrado en `main` como `57c4037`

M3-01..06 están Done y integrados. El PR #14 se mergeó el 2026-09-07 desde el
tip `93991c4` de `ai/m3-quality`, con los cinco checks obligatorios verdes
(`portable` x3, `supply chain`, `SonarCloud`) y un bypass de admin autorizado por
el owner: CODEOWNERS exige la revisión de `@cburgosro9303`, que es también el
autor del PR y el único colaborador, así que la aprobación exigida no podía
existir; con `enforce_admins: false` el bypass era el único mecanismo. **No se
saltó ningún check.** El merge no cambió ningún byte calificado
(`git diff --stat 93991c4 main` vacío) y el smoke post-merge sobre `main` pasó
fmt/check/clippy/test/arquitectura en exit 0 con 1,105 tests + 1 doctest, más 5
de las 62 selecciones del runtime Docker re-ejecutadas como confirmación real.
[Integración](validation/M3-07.md#integración) ·
[Recibo](validation/M3-integration.json) ·
[Runtime post-merge](validation/M3-postmerge-runtime.json).

- **M3-01** integra contrato, gateway, JUnit, Stage 1 durable con fallback
  Stage 0, D06 core corregido y perfil quality. Runtime 19/19 y security 20/20
  bajo P02; su camino Tasks quedó calificado en M3-02.
  [Evidencia](validation/M3-01.md).
- **M3-02** califica el camino Tasks productivo para las cuatro tools de calidad:
  un peer mutuamente negociado recibe `CreateTaskResult`, poll/cancel/update usan
  el JobExecutor owner-bound y el trabajo conserva el permit ADR-030 hasta
  cleanup. W4 pasó la matriz de cinco versiones, Docker T04/T05/T08/T10, budgets
  30/30 e Inspector 2.5.0; Codex app-server 0.153.0 no declaró Tasks y usó el
  camino síncrono. El anuncio está habilitado y sigue gated por la declaración
  del peer. [Matriz](validation/M3-02.md).
- **M3-03** está calificado bajo la enmienda acotada de ADR-065: target tmpfs
  dedicado por job, read-write solo en `CoverageRun`/`CoverageReport`, keeper
  read-only, export ausente, verifier/fingerprint y cleanup fail-closed. W2 pasó
  coverage 8/8 dentro del runtime de 62/62 (55 tools + 7 Tasks) y rust-security
  20/20. [Evidencia](validation/M3-03.md).
- **M3-04** `rust.semver.check` es la tool 21, conserva selección idéntica en
  ambos roots, evidencia separada y falla cerrada ante parser incierto; publica
  el raw output por Stage 1 durable con fallback Stage 0. Q01 pasó 18/18 y fijó
  exits 0/100/101, goldens reales y el target dir compartido bajo `/work`.
  [Recibo](validation/M3-04.md).
- **M3-05** `rust.mutation.test` es la tool 22, exige baseline, aplica mutantes
  solo en una copia privada, no exporta fuente mutada y deriva el veredicto de
  `mutants.out`. Q01 pasó 10/10 y fijó exits 0/1/2/3/4, schema/listas/bundle e
  identidad guest. Sin declaración Tasks del peer responde `TASKS_REQUIRED`.
  [Recibo](validation/M3-05.md).
- **M3-06** cierre y handoff: core 14/14, full 25/25 sobre 810 inputs con
  `source_inputs_unchanged`, rollback G6 10/10.
  [Matriz](validation/M3-matrix.md) · [Handoff](validation/M3-07.md).

**Dos elementos sobreviven al milestone y siguen abiertos:**

1. El fallo de `closed_output_stream_returns_one_without_panicking`
   (`crates/mcp-server/tests/cli.rs:131`) observado una vez en
   `portable / x86_64-unknown-linux-gnu`, documentado en el commit `93991c4`,
   **sin causa raíz confirmada**: no reproducido en 40 corridas en macOS y con la
   hipótesis de herencia de descriptores refutada (0/400). Necesita una
   reproducción en Linux antes de que nadie toque el oráculo, que es correcto tal
   como está.
2. Las 33 alertas CodeQL evaluadas como falsos positivos —el check agregado no es
   obligatorio—. Se recomienda descartarlas en la pestaña Security y renombrar la
   etiqueta de propiedad Docker `nonce` **en su propio corte**, porque el rename
   toca cinco gateways ya calificados y necesita su propio gate.
   [Evaluación](validation/M3-07.md#evaluación-codeql-33-alertas-check-no-obligatorio).

No hay tag, release ni publicación en crates.io para M3. El encargo del owner del
2026-09-07 autoriza implementar M4 mediante sus dos prompts; M5 y otra publicación
permanecen fuera de alcance.

## M4 — Security en `ai/m4-security`

**Done local — 2026-09-08.**
Las cinco tools M4 están implementadas: `rust.deny`, `rust.unsafe.scan`,
`rust.supply_chain.inspect`, `rust.quality.gate.v2` y `rust.miri`. El inventario
contiene 27 tools; cinco snapshots nuevos y 23 previos sin cambios. D19–D22 se
resuelven en ADR-067/069/071/072; ADR-066 documenta la adquisición autorizada y
ADR-068 admite la imagen final exacta. El runtime no adquiere dependencias.

| Evidencia | Resultado |
| --- | --- |
| [Core](validation/M4-core-gate.json) | 19/19; 1220 tests Rust, 1 doctest, 11 helper, protocolo 44/44 y controles auxiliares. |
| [Full](validation/M4-full-gate.json) | 33/33 monolítico posterior a la remediación del PR, sobre 990 inputs; paso workspace con 1250 tests Rust, 1 doctest, 94 tests Python y selecciones nativas; sin descarga y con fuentes sin cambios durante el gate. |
| [Runtime M4](validation/M4-runtime.json) | 19/19 sobre `25ed…`, con rollback interno deliberado a M3. |
| [Scanner](validation/M4-scanner-native.json) / [Miri](validation/M4-miri-native.json) | 7 casos scanner; 13 clasificaciones y 7 admisión/lifecycle Miri, todos ligados a fuentes actuales y cleanup verificado. |
| [Clientes](validation/M4-clients.json) | Intento 6 PASS sobre los mismos 990 inputs: Inspector 2.5.0 y Codex stock 0.153.0, cinco tools reales y Resources; Tasks/cancelación en Inspector, sync/modelo en Codex. |
| [Hardening](validation/M4-hardening-map.md) | Privacidad, canarios, inventario pasivo, imagen alterada, cleanup/revocación y regresiones M2/M3 pasados. |
| [Presupuestos](validation/M4-budgets.json) | 300/300, máximo 17044 ms; binario histórico explícito con inventario, separado de las rutas finales calificadas por clientes. |

La [revisión final de código](reviews/m4-final-closure/review.md) no halló P0/P1
ni P2 nuevos. La renovación nativa resuelve la evidencia stale retenida; la
[confirmación Opus](reviews/m4-final-evidence/review.md) acepta el cierre sin
P0/P1/P2 abiertos y el [Technical Owner](reviews/m4-final-evidence/disposition.md)
cierra los seis cortes y G1–G9. Se preservan fallos e intentos
sin atribuir causas no observadas. CI/Sonar remotos y host Linux/x86_64 no se
acreditan. Versión `0.3.0-dev`, implementación `07814664379628f00857feca13148b507de687b9`
en [PR #15](https://github.com/pharos-lang/rust-engineering-mcp/pull/15), sin tag,
release ni M5.

[Matriz](validation/M4-matrix.md) · [Handoff y límites](validation/M4-handoff.md).

## M5 — Performance en `ai/m5-performance`

**In progress — 2026-09-09. No está Done.** Las decisiones D23 y D24 están
cerradas (ADR-073/074) junto con el aprovisionamiento (ADR-075), los contratos
(ADR-076) y la admisión de imagen (ADR-077). Las cuatro tools están registradas
y el inventario público es de treinta y una; que estén anunciadas **no** las
califica, y esta tabla dice qué está demostrado.

Dos revisiones independientes G8 —seguridad/containment y método/contratos—
devolvieron `Block` con hallazgos reales. Sus textos íntegros, sus resúmenes y
las disposiciones del owner están en `docs/reviews/m5-security/` y
`docs/reviews/m5-method/`. El defecto de containment está corregido y
recalificado; el del modelo de varianza está en corrección y M5 no puede
cerrarse antes de que aterrice.

La imagen M5 se reconstruyó tras la corrección del helper, así que ADR-077
sustituyó su digest: la evidencia nativa se volvió a capturar entera sobre la
imagen admitida y los recibos anteriores se archivan con el digest que
realmente midieron.

| Corte | Estado | Evidencia |
| --- | --- | --- |
| M5-01 `rust.benchmark.run` | Blocked en el positivo; negativos y controles calificados, y el bloqueo tiene su propio oráculo. Los logs del harness pasan a publicarse por repetición ([ADR-080](adr/ADR-080-harness-logs-as-artifacts.md)): implementado y cubierto por pruebas de dominio, adapter y publicación; **la calificación nativa y la matriz de clientes se rehacen sobre bytes finales y no están hechas** | [runtime](validation/M5-01-runtime.json) · [oráculo](validation/M5-01-blocked-runtime.json) · [bloqueo](validation/M5-01-blocker.json) |
| M5-02 `rust.benchmark.compare` | Implementado; el modelo de varianza está **en corrección** tras la revisión de método | [revisión](reviews/m5-method/review.md) · [disposición](reviews/m5-method/disposition.md) |
| M5-03 `rust.profile.flamegraph` | Calificado nativamente, positivo y denegación, sobre la imagen admitida | [runtime](validation/M5-03-runtime.json) · [capability](validation/M5-profiling-capability-probe.json) |
| M5-04 `rust.binary.bloat` | Calificado nativamente sobre la imagen admitida | [runtime](validation/M5-04-runtime.json) |
| M5-05 cierre | In progress; gate conjunto y matriz de clientes sin ejecutar | — |

El positivo de profiling —la puerta que el plan señalaba— está demostrado: 195
muestras sin ninguna perdida sobre las 16 CPUs del guest y la pila esperada, con
`--cap-drop=ALL`, `no-new-privileges`, uid 65534, sin red, `perf_event_paranoid`
intacto en 2, sin capability añadida, sin contenedor privilegiado, sin `sudo` y
sin cambio de `sysctl`. Una sola syscall se añadió al perfil seccomp, y solo una
fase la usa. El negativo obligatorio —permiso denegado— también está en el mismo
recibo generado: EPERM, sin stacks y sin SVG.

**M5-01 está bloqueado por una condición reproducible**, no por falta de
trabajo: el cierre de criterion (6 014 archivos, 156 MB) no cabe en el contrato
de datos offline (`SourceBundle`: 4 096 entradas, 16 MiB, 1 MiB por archivo).
Los límites **no** se subieron; pertenecen al contrato calificado en M2/M4 y
ampliarlos exigiría decisión y recalificación. [Detalle y opciones](validation/M5-01-blocker.json).

El contrato separado que [ADR-078](adr/ADR-078-offline-vendor-capture.md) decide
**ya está implementado**: tipo de dominio propio con los límites de la tabla del
ADR, alfabeto ampliado solo a lo que el ADR admite, captura y verificación
incrementales, identidad por digest, rechazo de enlaces y de un árbol que se
mueve, limpieza sin residuo tras cancelación, provisión explícita
(`cargo-vendor capture`) y `rust.benchmark.run` resolviendo su vendor desde una
captura además de desde el `CargoVendorSnapshot` de siempre. Lo que **no** está
hecho, y no cambia el estado Blocked, es la calificación nativa: ingerir la
captura del cierre real en el guest exige Docker y bytes finales, y ese paso se
ejecuta aparte. `SourceBundle` y `validate_source_path` quedan intactos.

Sin tag, sin release, sin PR y sin push. M6 no está iniciado.
[Matriz](validation/M5-matrix.md) · [Handoff](validation/M5-handoff.md).

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

### M2-01/02 — Done en la base local `331d163`

El corte mínimo de lints y fmt.apply tienen preview/commit/receipt/recovery,
autorización separada, captura protegida y publicación recuperable.
[Gate core](validation/M2-02-core-gate.json): 14/14, 748 pruebas Rust y un doctest.
[Runtime actual](validation/M2-02-runtime-gate.json): manifest 1/1 y fmt 2/2,
con comandos, hashes y source inventory del snapshot. Trece contratos M1 intactos.
[Contrato Sonnet](reviews/M2-02-contract-review.md) y
[writer Opus](reviews/M2-02-native-review.md) aceptados sin P0/P1 pendientes.
[Medición APFS](validation/M2-02-native-performance.json): 128 archivos/16 MiB,
commit 5.687 s, replay 173 ms y recuperación terminal 103 ms, una observación.

La revisión registra P2 sobre headroom de recovery heterogéneo, pico RSS no medido
y precisión del harness cfg(test). Se siguen en el cierre conjunto; no se afirma
atomicidad multiarchivo ni exclusión de editores externos. En ese snapshot, M2 completo seguía In
Progress. El [cierre posterior](validation/M2-07.md) acredita full/client de las
cinco tools y las extensiones con sus propios gates. No hay tag, push ni release nueva.


D05 resuelto por ADR-055: directory source de Cargo vendor optativo, fingerprint
host, verificación completa de checksums y policy preserve_presence del lock.
Los probes son evidencia de decisión, no implementación de dependency.add/remove.

M2-03: ADR-056 acepta el perfil dedicado de Cargo fix tras D06 (121 observaciones);
implementado con pruebas reales de preview/commit/restart y fallos. No amplía
seccomp M1. El cierre depende del gate conjunto y revisión de esta ampliación.

M2 cliente stock detectó cuota RAM consumida por terminales. ADR-059 lo corrigió;
[recheck Accepted](reviews/M2-059-review.md), [full posterior](validation/M2-full-gate.json)
y [cliente PASS intento 5](validation/M2-clients.json) acreditan el cierre.
El full anterior y los intentos fallidos permanecen históricos y preservados.
