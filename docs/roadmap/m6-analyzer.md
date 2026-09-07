# M6 — Analyzer / 0.6.x

Estado: **Planned**. Entrada M5 cerrado; M2 debe seguir calificado para mutaciones,
M3 para tareas/artifacts. Fuentes: spec §32/97 M6,
[ADR-004](../adr/ADR-004-hexagonal-architecture.md),
[ADR-008](../adr/ADR-008-execution-gateway.md),
[ADR-031](../adr/ADR-031-rust-source-transfer.md). Aplican [G1–G9](m2-m8.md).

## Objetivo, contrato y antiobjetivos

Symbols, references, diagnostics y code actions del rust-analyzer exacto sobre un
snapshot identificado. No reimplementar LSP/análisis Rust, IDE completo, indexación
global, rename general, hover/go-to-definition o cache persistente por defecto.
§32 enumera hover/definition/rename feasibility como capacidades útiles: quedan
Deferred como extensiones demand-driven, no requisitos huérfanos ni features M6
silenciosas. Reconsiderarlas exige scope y contrato explícitos.

Propuesta de tools: `rust.analyzer.symbols`, `rust.analyzer.references`,
`rust.analyzer.diagnostics`, `rust.analyzer.actions` (preview),
`rust.analyzer.action.apply` (write separado por annotations/permisos). D25 decide
inventario exacto antes de schemas; §97 no fija nombres. No volver mutable una
consulta existente. Input: ProjectRef, source generation y file/position/range
tipados según operación. Output: snapshot/analyzer/config/toolchain identities,
resultados acotados, completeness/omissions y provenance/freshness.

## Cortes end-to-end

| ID | Camino observable | Dependencias | Oráculo/gate | Tamaño |
| --- | --- | --- | --- | --- |
| M6-01 | symbols request→captura→RA lifecycle→document symbols→cleanup | M5, D25/D26 | Binario RA real, transcript/capabilities y unicode/never-ready | XL |
| M6-02 | Workspace symbols/references→snapshot versions→respuestas normalizadas | 01 | Nombres iguales/imports/declaration/URI externa/omission | L |
| M6-03 | Diagnostics→sincronización/readiness→pull/push→snapshot result | 01 | Error nativo/limpio, stale publication, duplicate/partial | L |
| M6-04 | Actions→WorkspaceEdit validado→MutationPlan M2→diff previo | 02/03, D25 | Multi-file/UTF/overlap/snippet/Command/resource ops rechazados | L |
| M6-05 | Action aprobada→autoridad/generation M2→commit→receipt/reopen | 04, M2 | Exacto writer M2, conflict/cancel/crash/replay y action TTL | L |
| M6-06 | Operaciones del inventario D25→fixtures hostiles→clientes/full→handoff | 01–05 | G1–G9 y review Sonnet+Opus High de lifecycle/writer | L |

Camino crítico: runtime/LSP→snapshots→acciones→M2 commit→cierre; tamaño XL.
Cada corte empieza con flujo real, no crate/port de analyzer vacío.

## Lifecycle y sincronización

D26 compara servidor transitorio por snapshot/consulta con pool acotado keyed por
source/analyzer/config/toolchain. Propuesta inicial transitoria: captura owned,
guest RO, initialize/initialized, readiness explícito, didOpen con bytes/version,
request, shutdown/exit y cleanup. No esperar un silencio arbitrario para inferir
readiness o diagnóstico completo. Si el binario no ofrece oracle de completitud,
reportar incomplete. Document changes incrementan generation y cancelan resultados
anteriores; invalidar cualquier action/cache al cambiar source/policy/runtime.
Snapshots no afirman atomicidad del árbol host.

Domain conserva posiciones/source spans y tipos; application compone AnalyzerPort,
captura y control de job M3. Adapter LSP traduce encoding de posiciones contra bytes
capturados (UTF-8/UTF-16 negociados, Unicode scalar público existente), rechaza
offset dentro de codepoint/rango invertido y normaliza URIs solo bajo snapshot.
Lifecycle/env/process ownership pertenecen al gateway único; no Command fuera de él.
MCP sigue rmcp; codec LSP no es una implementación alternativa de MCP/JSON-RPC.

D26 fija versión/commit/digest de rust-analyzer, rustc/sysroot y compatibilidad.
Fuentes: [manual RA](https://rust-analyzer.github.io/book/),
[configuración](https://rust-analyzer.github.io/book/configuration.html),
[LSP](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/).
Reverificar release y APIs en la sesión de implementación, no descargar latest.
Artifact RA de toolchain 1.98.1 es un candidato a comprobar, no runtime ya aprobado.
CLI informa versión/capability y provisioning explícito con hashes/notices/SBOM.

Workspace trust: build scripts/proc macros/check-on-save desactivados inicialmente,
linkedProjects fijo, watcher controlado por cliente; neutralizar rust-analyzer.toml
hostil con prueba real de precedencia. Si no puede evitar su ejecución por config,
clasificar efectos como código del proyecto y requerir permiso/sandbox antes de
arrancar. La configuración por sí sola no sustituye containment.

## Code actions y permisos

Aceptar solo TextEdits en archivos existentes capturados, con versions exactas,
rangos no solapados, cambios/bytes acotados. Command, snippets, create/rename/delete,
URIs externas y server request workspace/applyEdit nunca se ejecutan implícitamente.
Acción no aplicable conserva razón; no hacer fallback a shell/editor genérico.

Preview produce plan M2 owner-bound que cubre action digest, analyzer/config/source,
TTL y diff exacto. Apply revalida permiso host para analyzer-action, generación y
plan y llama al mismo writer M2. No segundo lock/journal/filesystem. Generation
stale requiere nueva consulta/aprobación, no rebase silencioso. Receipt lookup tras
manifest changes sigue D01. Rollback/crash y dirty policy son exactamente M2.

## Threat model, límites, pruebas y operación

Amenazas: LSP hostil/malformed/oversized, IDs equivocados/late responses, never-ready,
RA crash/hang, config que reactive procesos, path externo, action stale, texto en
logs con secretos y filtro que oculte errores. Controles G2/G3, no-follow host,
snapshot guest, env limpio, red deny, cuotas y kill-tree; timeout durante initialize,
request o shutdown conserva cleanup unido. Ninguna petición LSP concede permisos.

Budgets propuestos D26: un RA activo por sesión, source ADR-031, frame LSP≤1 MiB,
messages≤4096/job, symbols/references≤512 visibles, actions≤32 con≤128 edits,
result MCP≤512 KiB, initialize≤60 s, query≤30 s, total≤180 s y memoria guest≤1 GiB
si el host/runtime puede imponerla. No cupo liberado hasta cleanup; exceso produce
incomplete/blocked y nunca datos stale exitosos. Retención plan M2 y artifacts M3;
no cache durable si benchmarks no prueban necesidad. SLI: cold init, readiness,
query latency, RSS peak, invalidaciones, stale descartados, cancel→cleanup.

Tests unit de traducción/parser/edit, protocol LSP fake-hostile más RA real,
contracts MCP/annotations/schema, integration symbols/references/diagnostics/actions,
adversarial config/build.rs/proc macro, timeout/cancel/EOF en cuatro fases, process
tree real y acciones multiarchivo que repiten crash/race M2. Fixtures Unicode,
macros/cfg, duplicate names, include_declaration, dependency/sysroot omitidos,
diagnósticos fuera de orden, URI/hardlink/parent race, snippets/commands.
Los fake peers no califican RA; los transcripts RA no califican writer M2.

Native positivo solo macOS/APFS+runtime guest calificado, Linux/Windows portable
fail-closed hasta D13. Plugin/runtime nuevo inventariado con licencia/notices,
SBOM/provenance, schema independiente y compatibilidad por versión. No distribuir
RA en core sin decisión. Rollback detiene RA, revoca planes ligados a esa identidad,
mantiene receipts/journals compatibles y evita downgrade con commit pendiente.

## DoR, DoD y aceptación

DoR: M5 cerrado y M2 reusable verificado, D25/D26 decididos, binario/sysroot exactos,
capabilities/config/readiness probados, fixtures/budgets y permisos claros. DoD:
M6-01..06 y G1–G9; Sonnet de paquetes, Opus High de procesos/acciones. P0/P1 y P2
de corrección de posiciones/autoridad/pérdida de datos bloquean.

- [ ] Symbols/references/diagnostics/code actions provienen de RA exacto y llevan
  snapshot/encoding/completeness comprobados. Fuente: spec §32/97 M6; M6-01..04.
- [ ] Config/URIs/server requests no activan efectos no autorizados; lifecycle
  conserva memory/time/kill-tree con oráculo real. Fuente: ADR-008/009/031; D26.
- [ ] Apply usa únicamente transacción M2 con diff aprobado, generation y recovery;
  acciones stale/commands se rechazan. Fuente: ADR-013, M2 y M6-04/05.
- [ ] Protocol/contract/native/client/full e inventario del runtime nuevo están
  ligados a source final, sin skips como pass. Fuente: G4/G5/G7/G8; M6-06.

Handoff: RA identities/transcripts, source/action schemas, performance/limits,
receipts/reviews y declaración de consultas diferidas; detener antes de M7.
