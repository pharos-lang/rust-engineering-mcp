# M6 — handoff (rust-analyzer Analyzer integration, 0.6.x)

Estado: **M6-01..06 Done local**, 2026-09-12 (gate UTC 2026-09-13). Rama `ai/m6-analyzer` desde `main`
`627729a`. Sin push, PR, tag ni release (prohibido sin autorización separada).
Orquestador: Claude Fable 5.1 (como Opus 4.8); todo el código/tests/fixtures/
scripts/ADRs/docs por agentes externos (registro: [delegation/README.md](delegation/README.md)).

## Qué se entregó

Cinco tools MCP del analyzer (inventario público **36**, los 31 previos
byte-idénticos), rust-analyzer 1.98.1 como *peer* de influencia hostil tratado
dentro del guest Docker (imagen `sha256:f39a5b33…`, `config_digest a2592cfc…`),
una instancia transitoria por consulta (ADR-084):

| Tool | Corte | Contrato |
| --- | --- | --- |
| `rust.analyzer.symbols` | M6-01 | document/workspace symbols; readiness quiescent; sin build script (prueba por símbolos) |
| `rust.analyzer.references` | M6-02 | `textDocument/references` por dos peticiones (set-difference), declaración incluida |
| `rust.analyzer.diagnostics` | M6-03 | pull diagnostics **sintaxis-only** (experimental off, Opción A owner); calidad semántica = deuda |
| `rust.analyzer.actions` | M6-04 | code actions → `WorkspaceEdit` validado estructuralmente → candidato M2 con diff previo |
| `rust.analyzer.action.apply` | M6-05 | apply por el **único writer M2** (`MutationKind::AnalyzerActionApply`), Opción A **no compile-verificado** |

## Evidencia de gate (G5, bytes finales)

- `gate.py full` (macOS ARM64, toolchain 1.98.1): recibo
  [M6-full-gate.json](M6-full-gate.json) `sha256:69a0be14c1e2ae0cce07014daeba1818c49fa115aa3b67313bb0baffe07f34d0`, **todas las etapas verdes**,
  `source_inputs_unchanged: true`. Cubre fmt/check/clippy/test/doctests/
  architecture + audit/deny + las etapas runtime Docker (docker-security,
  rust-security, m2/m3/m4/m5/m6-runtime, semantic, catalog, doctor).
- Calibración nativa M6: **12/12 cortes** (`m6-00..m6-11`), recibo
  [01-calibration.json](01-calibration.json) `sha256:7f935ac1032de5d29e78efe3ad9e7052dee4a419302ba3bef0e3ff92a04380cc`. La etapa `m6-runtime` del gate re-ejecuta
  estos cortes sobre las fuentes firmadas.
- Dos incidencias reales destapadas por el gate y corregidas antes del cierre:
  - **W11**: dos pines obsoletos de conteo de tools (31→36) en tests runtime
    `#[ignore]` que W08 omitió (categoría §4 (B): actualización esperada, no
    regresión del writer M2).
  - **W12/W12b**: el corte flaky `m6-03-guest-programs` — rust-analyzer 1.98.1
    ejecuta una sonda `rustc --print` por lotes que el allowlist cerrado
    (ADR-084 §7) no admitía; `docker top` la muestrea de forma no determinista
    (el recibo previo pasó por azar). Se admitió como consulta de solo lectura
    (`rustc_is_readonly_probe`, vocabulario cerrado), revisada por V12
    (Approve, 0 P0/P1/P2; P3-1 `--target` cerrado al triple del guest, P3-2
    documentado). No era regresión ni brecha.

## Matriz de clientes (G4/G8)

- Preflight (`test-m6-clients.py`): `status: ready`, inventario 36, imagen
  `f39a5b33…`.
- `--run` (Docker-free): cada tool del analyzer rechaza `unavailable/
  SANDBOX_DENIED` antes de crear contenedor. <RUNRESULT>
- `--run --with-runtime` (real): Inspector 2.5.0 + Claude Code 2.1.267 sobre
  los 5 tools, incluido apply preview→commit→receipt sobre copia temporal, y
  el negativo `ACTION_STALE`. Recibo [clients.json](clients.json) `sha256:cb11315edfcb0521a4991c470f1e090ba129133bb94f0ef295d02a23f200b4c7` (`status: passed`). **Inspector** (determinista, autoritativo) ejecutó la matriz completa de los 5 tools incluido `apply` preview→commit→receipt (`write_lifecycle: performed`) + los dos negativos. **Claude Code** (dirigido por modelo) demostró discovery (36) + `symbols` document + `actions` positivos + el negativo `FILE_NOT_IN_SNAPSHOT`; las 3 lecturas secundarias golpearon el rechazo transitorio de capacidad del analyzer (liberación asíncrona), registrado honestamente como `capacity_refused` (W09f); escritura best-effort omitida por assist vacío (no-determinismo de code actions). Inspector + los e2e nativos son la prueba autoritativa de escritura.

## G1–G9

Ver [g-disposition.md](g-disposition.md). Todos demostrados; sin P0/P1/P2 abiertos.

## Deuda trazada (M6-06)

- Calidad de diagnósticos (Opción B, requiere decisión owner; hoy sintaxis-only).
- `analyzer_runtime.rs` desgateado (flake de arnés M6-01, W05f).
- Precisión de `admitted` en audit tras denegaciones post-grant; flag por
  archivo generado/vendorizado en el preview.
- P3-2 V12: el oráculo m6-03 no ve fronteras de argumento (`docker top` une con
  espacios); no corregible barato desde fuera del guest.
- M1–M5 siguen calificadas contra sus propios digests (alcance global, ADR-085).

## Siguiente

No avanzar a M7. Release/PR/tag = gate separado con autorización explícita.
