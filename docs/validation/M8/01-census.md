# M8-01 — Censo de contratos y gate de superficie

Fecha: 2026-09-14. Rama `ai/m8-stabilization` desde `main` `e50c3fe` (M6 cerrado y
calificado; M7 Deferred con decisión). Este documento es la narrativa del censo
machine-readable [`01-census.json`](01-census.json); toda cifra citada aquí se
mide desde ese archivo o desde los comandos listados en §1. Worker: Claude
Sonnet 5 (`claude -p --model sonnet --effort high`), solo documentación bajo
`docs/`. La decisión final de clase/consolidación es del orquestador; este
documento **propone** con evidencia, no decide.

## 1. Método — comandos ejecutados

1. Verificación live del inventario público (JSON-RPC de tres líneas —
   `initialize` 2025-06-18, `notifications/initialized`, `tools/list` — contra
   `target/release/rust-engineering-mcp serve --stdio --root
   /private/tmp/m8-census-root.v52Agq`, root temporal absoluto bajo
   `/private/tmp`): `result.tools` tiene **36** entradas, respuesta de
   `tools/list` = **392 529 bytes** (línea 2 de la respuesta JSON-RPC), igual al
   número que el orquestador había verificado por separado. Los 36 nombres
   coinciden byte a byte con los de `crates/mcp-server/tests/snapshots/*-tool.json`
   (36 archivos `*-tool.json` + `doctor-report.json` = 37 ficheros).
2. Medición de `schema_bytes` por tool: cada snapshot se parseó con `json.load`
   y sus `inputSchema`/`outputSchema` se re-serializaron con
   `json.dumps(schema, separators=(',',':'), sort_keys=True)` para obtener un
   tamaño reproducible independiente del pretty-print del snapshot en disco
   (los snapshots están indentados a 2 espacios; el wire real es compacto pero
   con un orden de claves distinto — se documenta el método, no se afirma
   igualdad byte a byte con el wire).
3. `target/release/rust-engineering-mcp --help` para `cli_commands[]` (15
   subcomandos reales); `version --json` y `doctor --json --root
   /private/tmp/m8-census-root.v52Agq` para confirmar `format_version: 1`.
4. Censo de ADRs por tool: `grep`/regex sobre `docs/adr/ADR-*.md` buscando el
   nombre exacto de cada tool (`\brust\.foo\.bar\b`), complementado con lectura
   directa de los ADRs cuyo título topical coincide (p. ej. ADR-036
   "clippy-profiles" para `rust.clippy`) cuando el grep literal no basta.
5. Censo de tests: grep de cada nombre de tool sobre
   `crates/mcp-server/tests/**/*.rs` (protocol/inspection_runtime/analyzer_runtime)
   y sobre `crates/execution-adapter/**/*.rs` (native cuts reales, Docker-gated,
   `#[ignore]`); para los archivos `execution-adapter` sin el string literal del
   tool (la capa de ejecución no conoce nombres MCP) se verificaron nombres de
   función `#[test]`/`#[ignore]` uno a uno.
6. Censo de consumidores reales: lectura estructural (no solo grep) de
   `docs/validation/M{1,2,3,4,5,6}/clients.json` y, donde existían,
   `docs/validation/M{3,4}/clients/attempt-N/protocol.jsonl` — cada llamada
   `tools/call` real trae `tool`, `client`, `status`/`shape`/`mode` y a veces un
   `id` de invocación citable.
7. Censo de `$defs` duplicados en los 36 schemas (para el coste de contexto del
   gate de superficie): conteo de cuántos de los 36 archivos snapshot redefinen
   cada nombre de `$defs` compartido (`Data`, `Code`, `Freshness`, `Truncation`,
   `RuntimeIdentity`, `Provenance`, …).
8. `docs/roadmap/m2-m8.md`: se identificaron los commits de merge reales de
   M3-M6 (`git log --oneline --merges`) y el tag `v0.3.0` (`git log -1 --format=%H
   v0.3.0` = `6ea330d…`, el mismo commit que el merge de PR #17) para reemplazar
   "Planned" por el estado real con evidencia enlazada.

## 2. Resumen por tool (36/36)

| Tool | Corte | Clase propuesta | Consumidor real | Bytes de schema (in+out) | Cliente stock | Candidato a consolidación |
| --- | --- | --- | --- | --- | --- | --- |
| `rust.project.open` | M1/0.1.0 | stable | sí | 214+2552B | sí | none |
| `rust.project.inspect` | M1/0.1.0 | stable | sí | 224+10834B | sí | none |
| `rust.toolchain.inspect` | M1/0.1.0 | stable | sí | 224+6544B | sí | rust.project.inspect |
| `rust.check` | M1/0.1.0 | stable | sí | 870+9466B | sí | none |
| `rust.fmt.check` | M1/0.1.0 | stable | sí | 224+9288B | sí | rust.fmt.apply |
| `rust.clippy` | M1/0.1.0 | stable | sí | 819+9413B | sí | none |
| `rust.test` | M1/0.1.0 | stable | sí | 929+9676B | sí | rust.test.nextest |
| `rust.dependencies.audit` | M1/0.1.0 | stable | sí | 224+9789B | sí | rust.supply_chain.inspect |
| `rust.diagnostics.explain` | M1/0.1.0 | stable | sí | 203+5616B | sí | none |
| `rust.quality.gate` | M1/0.1.0 | stable | sí | 350+16764B | sí | **rust.quality.gate.v2** |
| `rust.catalog.status` | M1/0.1.0 | stable | sí | 119+9561B | sí | none |
| `rust.crate.search` | M1/0.1.0 | stable | sí | 867+9899B | sí | none |
| `rust.crate.inspect` | M1/0.1.0 | stable | sí | 735+9138B | sí | none |
| `rust.manifest.patch` | M2/0.2.x | stable | sí | 6906+6006B | sí | none |
| `rust.fmt.apply` | M2/0.2.x | stable | sí | 1184+6006B | sí | rust.fmt.check |
| `rust.fix.apply` | M2/0.2.x | stable | sí | 1181+6006B | sí | none |
| `rust.dependency.add` | M2/0.2.x | stable | sí | 2202+6006B | sí | rust.dependency.remove |
| `rust.dependency.remove` | M2/0.2.x | stable | sí | 1710+6006B | sí | rust.dependency.add |
| `rust.test.nextest` | M3/0.3.x | stable | sí | 1206+5431B | sí | rust.test |
| `rust.coverage` | M3/0.3.x | stable | sí | 1044+3537B | no | none |
| `rust.semver.check` | M3/0.3.x | stable | sí | 1112+5660B | no | none |
| `rust.mutation.test` | M3/0.3.x | stable | sí | 1092+6370B | no | none |
| `rust.deny` | M4/0.4.x | stable | sí | 456+10874B | sí | rust.supply_chain.inspect |
| `rust.unsafe.scan` | M4/0.4.x | stable | sí | 456+7159B | sí | none |
| `rust.supply_chain.inspect` | M4/0.4.x | stable | sí | 456+12439B | sí | **audit + deny** (superset) |
| `rust.quality.gate.v2` | M4/0.4.x | stable | sí | 1283+15676B | sí | **rust.quality.gate** |
| `rust.miri` | M4/0.4.x | stable | sí | 457+5599B | sí | none |
| `rust.benchmark.run` | M5/0.5.x | stable | sí | 1087+16659B | sí | none |
| `rust.benchmark.compare` | M5/0.5.x | stable | sí | 580+16769B | sí | none |
| `rust.profile.flamegraph` | M5/0.5.x | stable | sí | 852+7774B | sí | none |
| `rust.binary.bloat` | M5/0.5.x | stable | sí | 1311+14354B | sí | none |
| `rust.analyzer.symbols` | M6/0.6.x | preview | sí | 773+10589B | sí | analyzer.references/diagnostics |
| `rust.analyzer.references` | M6/0.6.x | preview | sí | 1106+8797B | sí | analyzer.symbols |
| `rust.analyzer.diagnostics` | M6/0.6.x | preview | sí | 510+9470B | sí | analyzer.symbols |
| `rust.analyzer.actions` | M6/0.6.x | preview | sí | 2114+10160B | sí | none |
| `rust.analyzer.action.apply` | M6/0.6.x | preview | sí | 2830+7181B | sí | none |

36/36 tienen `real_consumers[]` no vacío; 33/36 tienen «Cliente stock: sí»
(invocación registrada en un recibo de cliente stock). Las 3 con «no»
(`rust.coverage`, `rust.semver.check`, `rust.mutation.test`) solo tienen e2e
nativo como consumidor — ver `01-census.json.stock_client_coverage` y el
campo `stock_client_invoked` por tool. Detalle completo (adapter_source,
application_entry, domain_types, tests, annotations, error_codes, status_values,
class_rationale y surface_gate) en `01-census.json`.

### Clasificación propuesta — resumen y justificación

- **stable (31)**: M1 (13) + M2 (5) + M3 (4) + M4 (5) + M5 (4). Los 31 están en
  la release etiquetada y publicada `v0.3.0` (`docs/release/0.3.0/publication-receipt.json`,
  tag = commit `6ea330d…` = merge de PR #17 "feat: implement and qualify M5
  performance, release 0.3.0"); las trece M1 además están en `v0.1.0`. 28 de
  los 31 tienen consumidor real positivo (Inspector y/o Claude Code y/o Codex
  app-server) con al menos una ejecución `status: passed` en modo `runtime`, no
  solo `docker_free`/negativo. Las 3 restantes — `rust.coverage`,
  `rust.semver.check`, `rust.mutation.test` — **no** tienen invocación en
  ningún recibo de cliente stock: `docs/validation/M3/clients/attempt-11/protocol.jsonl`,
  el único recibo de M3, solo invoca `rust.project.open` y
  `rust.test.nextest`. Su único consumidor es el e2e nativo
  (`crates/execution-adapter/tests/coverage_runtime.rs`, `semver_runtime.rs`,
  `mutation_runtime.rs`). Conservan la clase `stable` por su publicación en
  `v0.3.0` más esa evidencia nativa completa, con la condición de que la
  matriz M8-04 las ejercite con Inspector y/o Codex/Claude Code stock antes de
  cerrar el freeze (hallazgo F12, corregido por auditoría R01: ver
  `docs/validation/M8/delegation/R01-census-traceability/report.md` §2.2/§2.3/§3).
- **preview (5)**: las cinco tools M6 (`rust.analyzer.symbols/references/diagnostics/actions/action.apply`).
  Están fusionadas en `main` (PR #20) y tienen consumidor real (Inspector
  positivo en modo `runtime`, `docs/validation/M6/clients.json`), pero **no**
  están en ninguna release etiquetada (no existe `v0.4.0`-`v0.7.0`); el propio
  `docs/validation/M6/handoff.md` las registra como "Done local"; ADR-083 deja
  hover/definition/rename explícitamente Deferred. Un elemento sin release
  publicada tiene una barra de evidencia menor que las 31 anteriores (que ya
  pasaron dos ciclos de publicación con OIDC attestation e independent
  verification); D11 exige consumidor+test+docs como mínimo necesario, no
  suficiente, y dado que el propio equipo las etiquetó "Done local" en vez de
  publicarlas, `preview` es la propuesta más conservadora y consistente con esa
  autodescripción.
- **internal (0)**: por definición D11 "internal no se anuncia en `tools/list`";
  las 36 tools SÍ se anuncian en `tools/list` en vivo, así que ninguna puede
  proponerse `internal` sin antes retirarla de la superficie pública — cambio
  que el plan prohíbe hacer para llegar a un número arbitrario.

## 3. Gate de superficie

### 3.1 Tool vs Resource/Prompt

Las 36 son legítimamente tools bajo spec §9.1/§20: cada una ejecuta trabajo
real por llamada (compilación, lint, escaneo, journal de mutación, consulta
LSP transitoria o composición estadística) — ninguna es simplemente "datos ya
calculados" reutilizables sin efecto, que es la frontera que spec §9.2 traza
para Resources. Ver `surface_gate.why_tool_not_resource_or_prompt` por tool en
el JSON para la justificación individual; los casos límite son:

- `rust.catalog.status` — spec §9.2 lista `rust-catalog://status` como ejemplo
  conceptual de Resource, pero nunca se implementó como tal (ver §4).
- `rust.crate.search`/`rust.crate.inspect` — spec §9.2 lista
  `rust-catalog://crate/{name}` como plantilla conceptual; tampoco implementada.
- `rust.project.inspect` — spec §9.2 lista `rust-project://workspace/metadata`;
  tampoco implementada.

Estos tres casos son hallazgos de superficie legítimos (F6): el diseño real
diverge de los ejemplos conceptuales de la spec, con una decisión explícita en
ADR-011 ("si aporta valor... M1 implementa el mínimo") que nunca se ejerció en
M1-M6. No se propone convertirlos a Resource ahora (romper 3 contratos stable
publicados sin necesidad); se propone documentar la divergencia.

### 3.2 Coste de contexto

`tools/list` = 392 529 bytes en vivo. La tabla §2 da bytes por tool
(inputSchema+outputSchema medidos desde el snapshot, método §1.2). Top-5 por
bytes (input+output): `rust.benchmark.run` (17 746B, 4.52%), `rust.benchmark.compare`
(17 349B, 4.42%), `rust.quality.gate` (17 114B, 4.36%), `rust.quality.gate.v2`
(16 959B, 4.32%), `rust.binary.bloat` (15 665B, 3.99%). Ningún tool individual
excede el 5% del total — el coste agregado de 392 KiB viene de la suma de 36
schemas razonablemente acotados, no de un outlier.

**Nota metodológica**: el numerador de cada tool es solo
`inputSchema+outputSchema`, sin `description` ni `annotations`; el
denominador (392 529B) es el `tools/list` en vivo completo, que sí incluye
`description`/`annotations` de las 36 tools. Esta asimetría subestima
ligeramente el porcentaje real de cada tool individual; no cambia la
conclusión de que ningún outlier supera el margen actual (~4.5%).

**Hallazgo de coste estructural (F9)**: los `$defs` de envelope compartido se
redefinen de forma independiente en cada schema de tool porque MCP no permite
`$ref` entre tools distintos dentro de `tools/list`. Medido sobre los 36
snapshots: `Data` aparece en 31/36 schemas, `Code` en 26/36, `Freshness` en
21/36, `Truncation` en 19/36, `RuntimeIdentity` en 16/36, `Provenance` en
14/36. Esta es la explicación estructural principal del tamaño total de
`tools/list`, no un defecto corregible sin cambiar el protocolo.

### 3.3 Consolidación — top candidatos (orden de prioridad)

1. **`rust.quality.gate` ↔ `rust.quality.gate.v2`** (34 073B combinados, 8.68%
   de `tools/list`): dos tools implementan gates de calidad por etapas con
   vocabularios de perfil solapados (`fast/standard` vs `strict/release`) y
   presupuestos síncronos distintos. Es el candidato más fuerte del catálogo.
   `gate` es M1 stable congelado (AGENTS.md prohíbe tocar las trece); `gate.v2`
   es M4. Recomendación: diseñar un contrato sucesor unificado antes del
   freeze 0.8, sin borrar `gate` v1 para llegar a un número — la consolidación
   real solo puede completarse en 1.0 vía deprecación anunciada en 0.8.0 por D11.
2. **`rust.dependencies.audit` + `rust.deny` ↔ `rust.supply_chain.inspect`**:
   ADR-071 confirma que `supply_chain.inspect` ya compone "una auditoría
   RustSec, una ejecución de deny... y facts del lock/metadata" internamente —
   es arquitectónicamente el superconjunto de los otros dos. Mantenerlos
   separados es más barato para una consulta estrecha (9789B/10874B de output
   frente a 12439B); no se recomienda borrar ninguno, solo documentar
   explícitamente la relación de composición en `docs/tools.md`.
3. **`rust.dependency.add` ↔ `rust.dependency.remove`**: espejo estructural
   opuesto; un solo `rust.dependency.edit(mode=add|remove)` es concebible pero
   ambos son ya contratos `stable` publicados en `v0.3.0` con formas de input
   distintas — fusionarlos es un cambio de ruptura minor con migration notes
   por D11, no antes del freeze.
4. **`rust.fmt.check` ↔ `rust.fmt.apply`**: par lectura/escritura del mismo
   rustfmt; `fmt.check` es M1 stable `readOnlyHint`, `fmt.apply` es M2
   `destructiveHint` journaled. Riesgo de consolidación alto (cambiaría
   `readOnlyHint` de un contrato congelado); no recomendado. Igual patrón para
   `rust.test`↔`rust.test.nextest` (pero `rust.test` está entre las trece
   congeladas — sin fusión posible antes de 1.0).
5. **`rust.analyzer.symbols`/`references`/`diagnostics`** (M6, preview): tres
   consultas LSP de solo lectura con anotaciones casi idénticas; fusionarlas en
   un `rust.analyzer.query(kind=...)` es concebible pero de baja prioridad — el
   `oneOf` resultante probablemente pesaría más que la suma actual (4 503B de
   input combinados hoy).

Ningún candidato anterior se propone para **borrar** un contrato con
consumidor real; el plan (`docs/roadmap/m8-stabilization.md` §Contratos) lo
prohíbe explícitamente.

## 4. Resources

`resources/list` devuelve `[]` en vivo, sin sesión — y **seguirá devolviendo
`[]` con sesión**, porque el servidor nunca implementa `list_resources`
(`grep` sobre `crates/mcp-server/src` no encontró ningún handler de listado;
solo `.enable_resources()` + `read_resource` en `crates/mcp-server/src/stdio.rs:91,816`).
Existen exactamente dos familias de URI, ambas dinámicas y no enumerables —
solo aparecen embebidas en la respuesta de una tool ya invocada:

| URI template | Productor | Usado por | Consumidor real |
| --- | --- | --- | --- |
| `rust-artifact://{project_ref}/{artifact_id}` | `crates/mcp-server/src/stdio/resources.rs` | check/fmt.check/clippy/test/quality.gate | `docs/validation/M5/clients.json` calls[8]-[12] (`artifacts_read` > 0, modo runtime) |
| `rust-quality-artifact://{project_ref}/{id}?offset&length` | ídem, `QUALITY_PREFIX` | los 12 tools M3-M5 respaldados por Tasks | ídem |

Ver F6 para la divergencia frente a los ejemplos conceptuales de spec §9.2
(`rust-project://workspace/metadata`, `rust-catalog://status`,
`rust-catalog://crate/{name}`), nunca implementados como Resources reales pese
a estar en la lista de ejemplos desde el borrador original.

## 5. CLI

15 subcomandos reales verificados vía `--help` (Comando/Subcomando exacto en
`01-census.json.cli_commands[]`): `serve`, `doctor`, `version`, `capabilities`,
`catalog status/import/sync/rebuild-index`, `mutation list/prune`,
`cargo-vendor inspect`, `security-runtime inventory`, `quality-artifacts
recover/prune`, `help`. `version --json` y `doctor --json` confirmados en vivo
con `format_version: 1`; el resto de subcomandos JSON comparte ese mismo
`format_version: 1` por convención de código (mismo dispatcher en
`crates/mcp-server/src/main.rs`), no se ejecutaron todos individualmente por
requerir fixtures de estado (`--state-root`, `--store`, `--trust`) fuera del
alcance de un censo de solo documentación. Exit codes: 0 éxito, 1
fallo/degradado (`crates/mcp-server/tests/doctor.rs` líneas 250/257/284), 2
error de uso de CLI (extensamente probado en `crates/mcp-server/tests/cli.rs`).
Los 15 subcomandos se proponen `stable`: ninguno es nuevo, todos tienen test y
documentación citada en `docs/tools.md`/ADRs.

## 6. Modelo de errores

Dos convenciones de casing incompatibles para el mismo campo conceptual
`error_code` (F5, P2): `OperationalErrorCode` (`crates/domain/src/result.rs`)
en SCREAMING_SNAKE_CASE para 31 tools de lectura/análisis (extendido por
tool con variantes propias en el `Code` local de cada schema), y una réplica
local `Reason` (`crates/mcp-server/src/stdio/mutation.rs:207-226`, espejo de
`MutationError` en `crates/domain/src/mutation.rs`) en snake_case para **5**
tools de mutación M2 (`rust.manifest.patch`, `rust.fmt.apply`,
`rust.fix.apply`, `rust.dependency.add`, `rust.dependency.remove`).
`rust.analyzer.action.apply` **no** usa esa réplica snake_case: su enum de
error cerrado es `ApplyCode`
(`crates/mcp-server/src/stdio/mutation/analyzer_action.rs:171-175`,
`#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`, 30 variantes), por lo que
queda en el grupo SCREAMING_SNAKE_CASE de 31 tools, no en el de 5
(corregido por auditoría R01, hallazgo F11:
`docs/validation/M8/delegation/R01-census-traceability/report.md` §3).
`status` es consistente: `passed|failed|blocked|unavailable|cancelled`
en ambas familias. Detalle completo en `01-census.json.error_model`.

## 7. Formatos en disco

10 formatos censados (`01-census.json.disk_formats[]`): config host (sin
formato persistido), journal de mutación M2 (**sin `format_version` en el
archivo**, solo namespacing de directorio `-v1`, F7), store de artifacts M3
(`PayloadFormatVersion` por tipo de payload, versionado explícito), estado
analyzer M6 (transitorio, sin estado persistente), catálogo SQLite
(`PRAGMA user_version`), bundle de confianza del catálogo (`format_version`
de campo), índice LanceDB derivado (`IndexMetadata.schema_version`), policy de
seguridad (`SecurityPolicyDocument.schema_version`), snapshot RustSec (formato
externo, solo pin de integridad SHA-256) y vendor tree de Cargo (formato
externo de Cargo, solo pin de hash de árbol completo). 7/10 tienen marcador de
versión explícito a nivel de campo; 3/10 (host config, analyzer state, vendor
tree/RustSec externos) no lo necesitan por ser sin estado persistente propio o
por ser formatos ajenos versionados por su propio ecosistema. `floor_or_trust_state: true`
en catálogo SQLite, bundle de confianza y snapshot RustSec/vendor tree
(antirollback relevante para D12).

## 8. Huérfanos

**Cero.** Cada tool, Resource, comando CLI y formato en disco de este censo
tiene owner (`adapter_source`/`reader_writer`), fuente (`adr[]` y/o sección de
spec citada en `class_rationale`), al menos un test citado y al menos un
consumidor real (`real_consumers[]`) o, para CLI/formatos, tests directos que
ejercitan el camino completo. Ver `01-census.json.orphans_note`.

## 9. Findings

11 findings, por severidad: **P2 = 6** (F1, F2, F5, F7, F12, F13), **P3 = 5**
(F3, F4, F6, F8, F9), **P0/P1 = 0**. `blocks_freeze: true` en F1, F2, F4, F5,
F13 — deben resolverse antes de 0.8 (M8-02) porque afectan a la exactitud de
la documentación pública del contrato, a la interpretabilidad cross-tool del
campo `error_code`, o a la exactitud de la documentación pública sobre M6.
F10 y F11 (hallazgos de la auditoría independiente R01 sobre defectos del
propio censo, no del producto) fueron corregidos directamente en este censo
y no se listan como entradas separadas en `findings[]`:
`docs/validation/M8/delegation/R01-census-traceability/report.md` §3,
disposición en
`docs/validation/M8/delegation/R01-census-traceability/disposition.md`.

| ID | Severidad | Dónde | Resumen | Bloquea freeze |
| --- | --- | --- | --- | --- |
| F1 | P2 | `docs/tools.md:3-4` | Cuenta de tools desactualizada (31, falta M6); contradice `docs/compatibility.md:8` (36) en el mismo checkout | sí |
| F2 | P2 | `crates/mcp-server/src/main.rs:59` | `--help` "Available tools" omite las 6 tools de M6/analyzer (lista 30/36) | sí |
| F3 | P3 | `docs/adr/README.md` | ADR-078/079/081/085 existen pero no están indexados | no |
| F4 | P3 | `docs/roadmap/m2-m8.md:12` | Línea resumen no actualizada tras corregir la tabla de hitos (fuera del alcance de esta edición) | sí (seguimiento) |
| F5 | P2 | `result.rs` vs `mutation.rs` vs `analyzer_action.rs` | Dos casings de `error_code` incompatibles (SCREAMING_SNAKE_CASE en 31 tools, snake_case en 5) entre familias de tools; `rust.analyzer.action.apply` usa `ApplyCode` en SCREAMING_SNAKE_CASE, no la réplica snake_case de M2 (corregido, F11) | sí |
| F6 | P3 | `stdio.rs`/`resources.rs` | Resources nunca enumerables (`resources/list` siempre `[]`); ejemplos conceptuales de spec §9.2 nunca implementados | no |
| F7 | P2 | `filesystem/macos/mutation.rs` | Journal de mutación M2 sin `format_version` en archivo, solo namespacing de directorio | no |
| F8 | P3 | `docs/ci.md:333-364` | Conteos de etapas de gate no reconciliables por grep estático de `scripts/gate.py` (fuera de alcance de este worker) | no |
| F9 | P3 | `crates/mcp-server/tests/snapshots/*.json` | `$defs` de envelope compartido duplicados hasta en 31/36 tools; explica el tamaño de `tools/list` | no |
| F12 | P2 | `01-census.md` §2, `01-census.json` (rust.coverage, rust.semver.check, rust.mutation.test) | `rust.coverage`, `rust.semver.check`, `rust.mutation.test` no tienen invocación en ningún recibo de cliente stock; solo e2e nativo; siguen `stable` por publicación en v0.3.0 + evidencia nativa, con la condición de ejercitarlas en la matriz M8-04 | no |
| F13 | P2 | `docs/client-configuration.md` (§M6), `docs/compatibility.md:8` | Documentación pública describía M6 como no integrado/en desarrollo; en realidad las 5 tools `rust.analyzer.*` están fusionadas en `main` (PR #20) y calificadas | sí |

Detalle completo (`evidence`, `proposed_fix`) por finding en `01-census.json.findings[]`.

## 10. Reconciliación del roadmap

`docs/roadmap/m2-m8.md` tenía M3, M4, M5 y M6 marcados "Planned" en la tabla de
hitos pese a estar cerrados, calificados e integrados en `main` desde hace
días/semanas. Esta sesión corrigió las cuatro filas de esa tabla (única edición
autorizada en ese archivo) con enlace a evidencia real de cierre:

- **M3** → Done; [matriz de cierre](../M3/matrix.md), PR #14 (`57c40373597541ac3d57bc8446ec4e2e598b904e`).
- **M4** → Done; [handoff](../M4/handoff.md), PR #15 (`90d72f2c4727e2487e9281623ed8e0860b392c91`).
- **M5** → Done; [handoff](../M5/handoff.md), PR #17 (`6ea330debc27a2cf1564fbbc258d4b358f5b0f1a`), el mismo commit que el tag `v0.3.0`.
- **M6** → Done; [handoff](../M6/handoff.md) y [g-disposition](../M6/g-disposition.md), PR #20 (`e50c3fefaff03fc45b89ae899cf5736af1fd0a72`, HEAD de esta sesión).

La línea resumen de `docs/roadmap/m2-m8.md:12` ("M2 Done local; M3–M8
Planned/Conditional") queda inconsistente con la tabla corregida; no se tocó
por estar fuera del alcance textual del encargo (solo las cuatro filas de la
tabla) — ver F4 para el seguimiento propuesto al orquestador.
