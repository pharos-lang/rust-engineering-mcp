# R01 — informe del auditor (Gemini 3.8 Flash High vía agy 1.2.1, read-only, --sandbox)

Invocación: `agy --model gemini-3.8-flash-high --effort high --sandbox --print-timeout 30m --output-format json -p "$(cat prompt-header.md)"` desde el cwd del proyecto. Inicio 2026-09-14T14:13:45Z, fin 2026-09-14T14:21:23Z, status SUCCESS, 453 s, 1 turno(s). Los enlaces `file:///` son del auditor; el orquestador verificó F10 y F11 contra los snapshots antes de disponer.

---

# Informe de Auditoría R01 — Trazabilidad del Censo M8-01

**Auditor:** Gemini 3.8 Flash (High) — Auditor independiente *read-only*  
**Repositorio:** `/Users/cburgosro/Projects/rust-mcp`  
**Rama / Commit:** `ai/m8-stabilization` (`e50c3fefaff03fc45b89ae899cf5736af1fd0a72`)  
**Fecha de auditoría:** 2026-09-14  
**Fuentes normativas:** [`docs/spec/rust-engineering-mcp-propuesta-v0.3.md`](file:///Users/cburgosro/Projects/rust-mcp/docs/spec/rust-engineering-mcp-propuesta-v0.3.md) (§9, §20, §53–59, §78, §85, §116.1), [`docs/adr/ADR-012-semver-compatibility.md`](file:///Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-012-semver-compatibility.md), [`docs/adr/ADR-086-deprecation-and-freeze-policy.md`](file:///Users/cburgosro/Projects/rust-mcp/docs/adr/ADR-086-deprecation-and-freeze-policy.md), [`docs/roadmap/m8-stabilization.md`](file:///Users/cburgosro/Projects/rust-mcp/docs/roadmap/m8-stabilization.md), [`docs/roadmap/m2-m8.md`](file:///Users/cburgosro/Projects/rust-mcp/docs/roadmap/m2-m8.md) §G1–G9 y [`AGENTS.md`](file:///Users/cburgosro/Projects/rust-mcp/AGENTS.md).

---

## 1. Veredicto Global

**`Approve con findings`**

El censo [`01-census.json`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01-census.json) y su narrativa [`01-census.md`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01-census.md), junto con las decisiones del orquestador en [`01.md`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01.md), proporcionan una base de trabajo sólida y fidedigna en su inventario central: 36 tools MCP verificadas contra el wire, 15 subcomandos CLI, 10 formatos en disco y 0 consolidaciones aceptadas en M8 para no violar la congelación de las trece tools M1 (G1) ni los grants de host (G2).

No obstante, la auditoría identificó **cuatro hallazgos nuevos relevantes** (F10–F13, con dos P1 y dos P2) que deben corregirse en `01-census.json` y en la narrativa antes del freeze de M8-02: omisión de enums de códigos de error en 5 snapshots, un error fáctico sobre el casing de `rust.analyzer.action.apply`, relajación no declarada del criterio de «consumidor real» para 3 tools M3, e inconsistencias residuales en documentación pública.

---

## 2. Comprobaciones Obligatorias

### 2.1. Existencia de fuentes, snapshots, annotations y error_codes
Se auditaron exhaustivamente las **36 tools** del censo:
* **`adapter_source`**: 36/36 existen en las rutas declaradas bajo `crates/mcp-server/src/stdio/`.
* **`adr[]`**: Los 86 ADRs citados (172 referencias acumuladas) existen físicamente en `docs/adr/`.
* **`tests.contract_snapshot`**: 36/36 snapshots existen en `crates/mcp-server/tests/snapshots/*-tool.json`.
* **`annotations`**: 36/36 snapshots declaran **exactamente** las annotations afirmadas en el censo (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`).
* **`tests.protocol_tests[]` y `native_cuts[]`**: 100% de los archivos y funciones citadas existen en `crates/mcp-server/tests/` y `crates/execution-adapter/src/` o `tests/`.
* **`error_codes[]` vs enum `Code` de snapshot**:
  * En 31 tools hay coincidencia exacta con el enum de snapshot (`Code` o `Reason`).
  * **Discrepancia crítica (F10)**: En 5 tools (`rust.project.open`, `rust.analyzer.references`, `rust.analyzer.diagnostics`, `rust.analyzer.actions`, `rust.analyzer.action.apply`), el censo declara `"error_codes": []`, pero los snapshots en disco declaran enums cerrados completos: `BlockedCode`/`UnavailableCode` (7+2 variantes), `ReferencesCode` (19 variantes), `DiagnosticsCode` (18 variantes), `ActionsCode` (19 variantes) y `ApplyCode` (30 variantes).

### 2.2. Verificación de `real_consumers[]` (recibos y llamadas)
* Se abrieron y verificaron todos los recibos citados (`M1/17-inspector.md`, `M2/clients.json`, `M3/clients/attempt-11/protocol.jsonl`, `M4/clients/attempt-6/protocol.jsonl`, `M5/clients.json`, `M6/clients.json`).
* Las citas de índices y líneas para M1, M2, M4, M5 y M6 coinciden exactamente:
  * M2: `calls[7]/[8]` para `manifest.patch`, `calls[10]/[11]` para `fmt.apply`, `calls[13]/[14]/[16]` para `fix.apply`, `calls[1]/[2]` para `dependency.add`, `calls[4]/[5]` para `dependency.remove`.
  * M4: líneas exactas 7, 45, 57, 71, 97, 115, etc. en `attempt-6/protocol.jsonl`.
  * M5: `calls[0]`, `calls[8]/[9]`, etc. en `M5/clients.json`.
  * M6: `calls[0]..[4]` (unavailable) y `calls[5]..[12]` (passed) en `M6/clients.json`.
* **Discrepancia en M3 (F12)**: Para `rust.coverage`, `rust.semver.check` y `rust.mutation.test`, el censo cita tests nativos de Rust (`coverage_runtime.rs`, `semver_runtime.rs`, `mutation_runtime.rs`). En `docs/validation/M3/clients/attempt-11/protocol.jsonl` **únicamente** se invocaron `rust.project.open` y `rust.test.nextest`. Las otras 3 tools de M3 no cuentan con una invocación en un recibo de cliente (Inspector / Claude Code / Codex).

### 2.3. Clasificación `stable`/`preview` frente a ADR-086 §1
* ADR-086 §1 estipula: *«Un elemento sin consumidor real o sin test no puede clasificarse `stable`»*.
* Las 5 tools de M6 (`rust.analyzer.*`) están correctamente clasificadas como `preview`: tienen consumidor real verificado en `M6/clients.json` pero acarrean deuda de contrato documentada en [`docs/validation/M6/matrix.md`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M6/matrix.md) (`SANDBOX_DENIED` mixto, diagnósticos sintaxis-only, assists no deterministas).
* Para las 31 tools `stable`: 28 cuentan con consumidor cliente real (`runtime: passed`). Las 3 restantes (`rust.coverage`, `rust.semver.check`, `rust.mutation.test`) solo cuentan con tests end-to-end del runtime Docker (`e2e nativo`). Si bien su inclusión en `stable` se sostiene por haber sido publicadas formalmente en `v0.3.0` (commit `6ea330d`), la afirmación de `01-census.md:107` es inexacta y debe ajustarse (ver F12).

### 2.4. Consolidaciones descartadas en `01.md` §3 frente a spec y plan M8
* El descarte de las 5 consolidaciones evaluadas **no contradice** la spec ni el plan M8; por el contrario, los respeta rigurosamente:
  1. `quality.gate` ↔ `quality.gate.v2`: `quality.gate` está congelada individualmente por M1 (G1 y `AGENTS.md`). Descartada correctamente.
  2. `audit` + `deny` ↔ `supply_chain.inspect`: Mantenerlas separadas optimiza el consumo de contexto (spec §78).
  3. `dependency.add` ↔ `dependency.remove`: Fusionarlas violaría la separación de privilegios de host (G2: `--allow-dependency-add` vs `--allow-dependency-remove`).
  4. Pares `check`/`apply` y `test`/`nextest`: G1 prohíbe transformar lecturas en escrituras y alterar `readOnlyHint`.
  5. `analyzer.query(kind)`: Aumentaría tokens de schema mediante un `oneOf` voluminoso sin demanda real.
* El inventario resultante de 36 tools cumple el objetivo de «aproximadamente 35 tools» del plan M8, aplicando la regla de *«no borrar contratos usados para lograr un número arbitrario»* (ADR-086 §9 y spec §116.1).

### 2.5. Verificación de findings F1–F9
* **F1 (P2)**: **Real**. `docs/tools.md:3-4` omitía M6 y reportaba 31 tools.
* **F2 (P2)**: **Real**. `crates/mcp-server/src/main.rs:59` reportaba 30 tools en `--help`.
* **F3 (P3)**: **Real**. `docs/adr/README.md` omitía indexar ADR-078, 079, 081 y 085.
* **F4 (P3)**: **Real**. `docs/roadmap/m2-m8.md:12` decía `M2 Done local; M3–M8 Planned/Conditional`.
* **F5 (P2)**: **Real en el fondo, pero inexacto en el detalle**. La discrepancia de casing existe, pero `rust.analyzer.action.apply` usa `SCREAMING_SNAKE_CASE` (ver F11).
* **F6 (P3)**: **Real**. `crates/mcp-server/src/stdio.rs:91` habilita resources, pero no implementa `list_resources`.
* **F7 (P2)**: **Real**. `crates/project-adapter/src/filesystem/macos/mutation.rs` no incluye `format_version` en los archivos de journal.
* **F8 (P3)**: **Real**. Desalineación de conteo estático en `docs/ci.md:351-364`.
* **F9 (P3)**: **Real**. Repetición de `$defs` (`Data`, `Code`, etc.) en los 36 snapshots por restricción de JSON Schema en MCP.

### 2.6. Verificación de commits citados en `01-census.md` §10
Se verificaron mediante `git log -1 <hash>` y `git cat-file`:
1. `57c40373597541ac3d57bc8446ec4e2e598b904e`: **Existe**. Es el merge de PR #14 (M3).
2. `90d72f2c4727e2487e9281623ed8e0860b392c91`: **Existe**. Es el merge de PR #15 (M4).
3. `6ea330debc27a2cf1564fbbc258d4b358f5b0f1a`: **Existe**. Es el merge commit de PR #17 (M5); además el tag anotado `v0.3.0` apunta exactamente a este commit (`v0.3.0^{commit}`).
4. `e50c3fefaff03fc45b89ae899cf5736af1fd0a72`: **Existe**. Es el merge de PR #20 (M6, HEAD actual de la rama).

---

## 3. Lista Detallada de Findings

| ID | Sev | Archivo:Línea | Resumen |
|---|---|---|---|
| **F10** | **P1** | `docs/validation/M8/01-census.json` (tools 0, 32–35) | `error_codes` vacíos en 5 tools con enums de error definidos en snapshots |
| **F11** | **P2** | `docs/validation/M8/01-census.md:249-251`, `01.md:64`, `01-census.json` (F5) | Afirmación inexacta de que las 6 tools de mutación usan `snake_case` (`ApplyCode` es `SCREAMING_SNAKE_CASE`) |
| **F12** | **P1** | `docs/validation/M8/01-census.md:106-109`, `01.md:16-17,23` | Afirmación falsa sobre consumidor cliente real para todas las tools `stable` (3 tools M3 solo tienen tests nativos) |
| **F13** | **P2** | `docs/client-configuration.md:406-427`, `docs/compatibility.md:8` | Documentación pública desactualizada sobre M6 (omite tools 33–36 y flags; M6 figura como no integrado) |

### F10 — Omisión de `error_codes` en 5 snapshots de herramientas (P1)
* **Archivo:Línea:** [`docs/validation/M8/01-census.json`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01-census.json) (entradas de `rust.project.open`, `rust.analyzer.references`, `rust.analyzer.diagnostics`, `rust.analyzer.actions`, `rust.analyzer.action.apply`).
* **Evidencia textual:**
  En `01-census.json`, las 5 tools tienen `"error_codes": []`.
  Sin embargo, los snapshots correspondientes declaran:
  * [`crates/mcp-server/tests/snapshots/project-open-tool.json`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/snapshots/project-open-tool.json): `$defs.BlockedCode` (7 variantes) y `$defs.UnavailableCode` (2 variantes).
  * [`crates/mcp-server/tests/snapshots/analyzer-references-tool.json`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/snapshots/analyzer-references-tool.json): `$defs.ReferencesCode` (19 variantes).
  * [`crates/mcp-server/tests/snapshots/analyzer-diagnostics-tool.json`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/snapshots/analyzer-diagnostics-tool.json): `$defs.DiagnosticsCode` (18 variantes).
  * [`crates/mcp-server/tests/snapshots/analyzer-actions-tool.json`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/snapshots/analyzer-actions-tool.json): `$defs.ActionsCode` (19 variantes).
  * [`crates/mcp-server/tests/snapshots/analyzer-action-apply-tool.json`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/tests/snapshots/analyzer-action-apply-tool.json): `$defs.ApplyCode` (30 variantes).
* **Acción propuesta:** Actualizar el parser extractor en el worker de censo para registrar los nombres de enums específicos de cada tool en `01-census.json`.

### F11 — Inexactitud factual en F5 y §6 sobre el casing de `rust.analyzer.action.apply` (P2)
* **Archivo:Línea:** [`docs/validation/M8/01-census.md:249-251`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01-census.md#L249-L251), [`docs/validation/M8/01.md:64`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01.md#L64), [`docs/validation/M8/01-census.json`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01-census.json) (finding F5).
* **Evidencia textual:**
  * `01-census.md:249-251`: *«y una réplica local Reason... en snake_case para las 6 tools de mutación.»*
  * `01.md:64`: *«el envelope de mutación M2 (snake_case, Reason) y el operacional (SCREAMING_SNAKE_CASE...)... unificar casing sería ruptura de 6 o de 30 contratos stable.»*
  * En contraste, [`crates/mcp-server/src/stdio/mutation/analyzer_action.rs:171-175`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/mutation/analyzer_action.rs#L171-L175):
    ```rust
    /// The closed codes of this tool (ADR-083 §3), spelled like the other M6
    /// tools rather than like the frozen M2 reasons.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
    #[serde(rename_all = "SCREAMING_SNAKE_CASE")]
    enum ApplyCode { ... }
    ```
* **Acción propuesta:** Corregir la descripción de F5 en `01-census.json`, `01-census.md` §6 y `01.md` §4: `analyzer.action.apply` utiliza `SCREAMING_SNAKE_CASE`. El inventario real es de **5** tools en `snake_case` (M2) y **31** tools en `SCREAMING_SNAKE_CASE`.

### F12 — Asignación de consumidor cliente real no demostrable en 3 tools M3 (P1)
* **Archivo:Línea:** [`docs/validation/M8/01-census.md:106-109`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01-census.md#L106-L109), [`docs/validation/M8/01.md:16-17,23`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M8/01.md#L16-L17).
* **Evidencia textual:**
  * `01-census.md:107-109`: *«Todos tienen consumidor real positivo (Inspector y/o Claude Code y/o Codex app-server) con al menos una ejecución status: passed en modo runtime, no solo docker_free/negativo.»*
  * En `01-census.json`, los `real_consumers` para `rust.coverage`, `rust.semver.check` y `rust.mutation.test` citan únicamente suites de tests nativos de Rust (`coverage_runtime.rs`, `semver_runtime.rs`, `mutation_runtime.rs`). En [`docs/validation/M3/clients/attempt-11/protocol.jsonl`](file:///Users/cburgosro/Projects/rust-mcp/docs/validation/M3/clients/attempt-11/protocol.jsonl) solo se registran invocaciones a `rust.project.open` y `rust.test.nextest`.
* **Acción propuesta:** 
  1. Rectificar la frase en `01-census.md:107` reconociendo que `rust.coverage`, `rust.semver.check` y `rust.mutation.test` no tienen ejecuciones en clientes externos en `docs/validation/M3/clients/`.
  2. Ajustar la justificación en `01.md` §2: estas 3 tools conservan la clase `stable` por haber sido publicadas en la release oficial `v0.3.0` y contar con evidencia nativa completa (G4/G5), debiendo programarse su calificación en cliente stock dentro de M8-04.

### F13 — Secciones desactualizadas sobre M6 en documentación pública (P2)
* **Archivo:Línea:** [`docs/client-configuration.md:406-427`](file:///Users/cburgosro/Projects/rust-mcp/docs/client-configuration.md#L406-L427), [`docs/compatibility.md:8`](file:///Users/cburgosro/Projects/rust-mcp/docs/compatibility.md#L8).
* **Evidencia textual:**
  * `docs/client-configuration.md:406`: *«`rust.analyzer.symbols` es la primera tool M6 y está en desarrollo, no calificada... No hay ninguna otra bandera nueva que configurar»*. Omite las tools 33–36 y la bandera de autorización `--allow-analyzer-action-write`.
  * `docs/compatibility.md:8`: Describe M6 como *«sin commit de integración, PR, tag ni publicación»*, a pesar de que M6 ya está integrado en `main` vía PR #20 (`e50c3fe`).
* **Acción propuesta:** Incluir ambos archivos en la tarea de saneamiento de documentación pública (W03 / M8-02).

---

## 4. Muestra Representativa de Herramientas Auditadas (30/36)

| Milestone | Herramienta | `adapter_source` | `adr[]` | Snapshot y Annotations | `error_codes` coinciden | Consumidor Real Verificado |
|---|---|:---:|:---:|:---:|:---:|---|
| **M1** | [`rust.project.open`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/project.rs) | OK | OK (3) | OK | **Discrepancia** (F10) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.project.inspect`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/inspection.rs) | OK | OK (2) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.toolchain.inspect`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/toolchain.rs) | OK | OK (2) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.check`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/check.rs) | OK | OK (3) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.fmt.check`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/fmt.rs) | OK | OK (2) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.clippy`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/clippy.rs) | OK | OK (2) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.test`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/test.rs) | OK | OK (3) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.dependencies.audit`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/dependencies.rs) | OK | OK (3) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.diagnostics.explain`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/diagnostics.rs) | OK | OK (2) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.quality.gate`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/quality_gate.rs) | OK | OK (2) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.catalog.status`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/catalog_status.rs) | OK | OK (3) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.crate.search`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/crate_search.rs) | OK | OK (4) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M1** | [`rust.crate.inspect`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/crate_inspect.rs) | OK | OK (3) | OK | OK (`Code`) | Inspector UI (`M1/17-inspector.md`) |
| **M2** | [`rust.manifest.patch`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/mutation.rs) | OK | OK (4) | OK | OK (`Reason`) | Claude Code (`M2/clients.json` calls 7/8) |
| **M2** | [`rust.fmt.apply`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/mutation.rs) | OK | OK (3) | OK | OK (`Reason`) | Claude Code (`M2/clients.json` calls 10/11) |
| **M2** | [`rust.dependency.add`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/mutation.rs) | OK | OK (4) | OK | OK (`Reason`) | Claude Code (`M2/clients.json` calls 1/2) |
| **M3** | [`rust.test.nextest`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/nextest.rs) | OK | OK (3) | OK | OK (`Code`) | Inspector / Codex (`M3/protocol.jsonl`) |
| **M3** | [`rust.coverage`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/coverage.rs) | OK | OK (4) | OK | OK (`Code`) | **Solo test nativo** (F12) |
| **M3** | [`rust.semver.check`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/semver.rs) | OK | OK (4) | OK | OK (`Code`) | **Solo test nativo** (F12) |
| **M3** | [`rust.mutation.test`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/mutation_test.rs) | OK | OK (4) | OK | OK (`Code`) | **Solo test nativo** (F12) |
| **M4** | [`rust.deny`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/security.rs) | OK | OK (4) | OK | OK (`Code`) | Inspector / Codex (`M4/protocol.jsonl`) |
| **M4** | [`rust.unsafe.scan`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/security.rs) | OK | OK (3) | OK | OK (`Code`) | Inspector / Codex (`M4/protocol.jsonl`) |
| **M4** | [`rust.supply_chain.inspect`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/security.rs) | OK | OK (4) | OK | OK (`Code`) | Inspector / Codex (`M4/protocol.jsonl`) |
| **M4** | [`rust.quality.gate.v2`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/security.rs) | OK | OK (3) | OK | OK (`Code`) | Inspector / Codex (`M4/protocol.jsonl`) |
| **M5** | [`rust.benchmark.run`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/performance.rs) | OK | OK (6) | OK | OK (`Code`) | Inspector (`M5/clients.json` calls 0, 8, 9, 13) |
| **M5** | [`rust.binary.bloat`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/performance.rs) | OK | OK (5) | OK | OK (`Code`) | Inspector (`M5/clients.json` calls 6, 12) |
| **M6** | [`rust.analyzer.symbols`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/analyzer_symbols.rs) | OK | OK (4) | OK | OK (`Code`) | Inspector / Claude Code (`M6/clients.json`) |
| **M6** | [`rust.analyzer.references`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/analyzer_references.rs) | OK | OK (4) | OK | **Discrepancia** (F10) | Inspector / Claude Code (`M6/clients.json`) |
| **M6** | [`rust.analyzer.diagnostics`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/analyzer_diagnostics.rs) | OK | OK (4) | OK | **Discrepancia** (F10) | Inspector / Claude Code (`M6/clients.json`) |
| **M6** | [`rust.analyzer.action.apply`](file:///Users/cburgosro/Projects/rust-mcp/crates/mcp-server/src/stdio/mutation/analyzer_action.rs) | OK | OK (4) | OK | **Discrepancia** (F10/F11) | Inspector / Claude Code (`M6/clients.json`) |

*(Nota: las 6 tools restantes no tabuladas —`rust.fix.apply`, `rust.dependency.remove`, `rust.miri`, `rust.benchmark.compare`, `rust.profile.flamegraph`, `rust.analyzer.actions`— también pasaron las verificaciones programáticas de código, snapshots y recibos).*

