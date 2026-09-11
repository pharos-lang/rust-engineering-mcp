# ADR-083 — Contrato analyzer y acciones

Fecha: 2026-09-11.

## Status

Accepted por decisión del orquestador (Claude Fable 5.1) del 2026-09-11, dentro del
encargo M6, sobre el [brief D25/D26](../validation/M6/delegation/D25-D26-decision-brief.md)
§1 y su cierre `[R01]` §4. D25 estaba **Proposed**. Depende de
[ADR-084](ADR-084-rust-analyzer-runtime-and-lsp-lifecycle.md) para identidad de
runtime y lifecycle LSP, y reutiliza el writer M2 de
[ADR-050](ADR-050-local-coordinated-mutation.md)/[ADR-052](ADR-052-mutation-journal-and-authorization.md).

## Context

El [plan M6](../roadmap/m6-analyzer.md) exige symbols, references, diagnostics y
code actions del rust-analyzer exacto sobre un snapshot identificado, sin
reimplementar LSP/análisis Rust ni convertir una consulta existente en mutable.
D25 debía fijar el inventario exacto de tools antes de los schemas; §97 de la spec
no fija nombres. El informe de investigación R01 se rechazó como evidencia
directa por hashes fabricados en su Q1 (ver
[disposición R01](../validation/M6/delegation/R01-ra-research/disposition.md)); lo
que de él se acepta entra aquí solo como mapa de verificación con oráculo local en
la calibración nativa de W04, no como cita.

El dominio ya publica `Position` (línea y columna en Unicode scalars, ambas
1-based), `ByteRange` (0-based exclusivo) y `SourceSpan` con `bytes:
Option<ByteRange>`; M6 reutiliza estos tipos en el wire y no introduce un segundo
sistema de posiciones. El writer M2 (`MutationCandidate { kind, before: after:
SourceBundle, validation }`, `MutationPlans` en memoria con TTL 600 s/≤4
planes/≤64 MiB, `commit_mutation` con `MutationPublisher`, journal ADR-052,
replay, receipt, recovery) es el único mecanismo de escritura del producto;
`MutationKind` es un enum de dominio cerrado.

## Decision

### 1. Inventario exacto: cinco tools nuevas, las treinta y una existentes intactas

| Tool | Efecto | Annotations | Permiso host |
| --- | --- | --- | --- |
| `rust.analyzer.symbols` | lectura | `readOnly`, `idempotent` | `--rust` (gateway) |
| `rust.analyzer.references` | lectura | `readOnly`, `idempotent` | `--rust` |
| `rust.analyzer.diagnostics` | lectura | `readOnly`, `idempotent` | `--rust` |
| `rust.analyzer.actions` | lectura (lista + edits resueltos, sin efectos) | `readOnly`, `idempotent` | `--rust` |
| `rust.analyzer.action.apply` | escritura M2 (preview/commit/receipt) | `destructive=false`, `idempotent=false` | `--rust` + `--allow-analyzer-action-write <root>` |

Hover, go-to-definition, rename y cualquier otra capacidad LSP quedan
**Deferred** (spec §32/C14): no se exponen ni como campo opcional de ninguna de
las cinco tools. Reconsiderarlas exige scope y contrato explícitos, no una
ampliación silenciosa de este ADR.

`--allow-analyzer-action-write <root>` es de la misma familia que
`--allow-manifest-write`/`--allow-fmt-write`/`--allow-fix-write`: repetible,
exige estar dentro de un `--root` configurado y coincidir exactamente con la raíz
del workspace a editar (ADR-052); no concede subproyectos implícitos ni amplía la
autoridad de ejecución del gateway (ADR-050 §2).

### 2. Entradas

Comunes a las cinco tools: `project_ref` (`^prj_[0-9a-f]{32}$`) y
`expected_project_fingerprint` — obligatorio en `actions`/`action.apply`,
opcional en las tres de lectura; si se envía y no coincide con la identidad viva,
la respuesta es `blocked/CONFLICT` y nunca datos stale. `file` es una ruta
relativa POSIX dentro de la captura, con las mismas reglas que
`validate_source_path` (sin `..`, sin ruta absoluta, extensión `.rs`
obligatoria). `position` es un `Position` del dominio (Unicode scalar, 1-based);
`range` es `{start, end}` con `start ≤ end`. `timeout_seconds` tiene default y
máximo por tool según la tabla de presupuestos de ADR-084 §7.

- `symbols`: `{ scope: "document", file }` o `{ scope: "workspace", query
  (1..=128 caracteres, sin caracteres de control) }`.
- `references`: `{ file, position, include_declaration: bool = true }`.
- `diagnostics`: `{ file }`, un archivo por llamada; solo diagnósticos
  **nativos** de rust-analyzer (pull, ADR-084 §5). `cargo check` sigue siendo
  `rust.check` y no se mezcla con esta tool.
- `actions`: `{ file, range, only: [kind]? }`, con `kind` en el enum cerrado
  `quickfix | refactor | refactor_extract | refactor_inline | refactor_rewrite |
  source`.
- `action.apply`: `{ mode: preview { expected_project_fingerprint,
  action_digest, file, range } | commit { plan_id, plan_digest, idempotency_key
  } | receipt { operation_id, recover } }` — la misma forma que
  `rust.fmt.apply`. `action_digest` es el sha256 del `CodeAction` resuelto
  (título, kind, edits normalizados, identidad analyzer/config/source); el
  `preview` vuelve a consultar rust-analyzer sobre una **captura nueva** y exige
  que el digest recalculado coincida, o responde `blocked/ACTION_STALE`.

### 3. Envelope de salida común

```text
status            passed | failed | blocked | unavailable | cancelled  (semántica M2/M5)
reason            enum cerrado: CONFLICT, ANALYZER_NOT_READY, ANALYZER_CRASHED,
                  FRAME_LIMIT, MESSAGE_LIMIT, RESULT_LIMIT, TIMEOUT_INITIALIZE,
                  TIMEOUT_QUERY, TIMEOUT_TOTAL, UNSUPPORTED_PROJECT_CONFIG,
                  FILE_NOT_UTF8, FILE_NOT_IN_SNAPSHOT, POSITION_OUT_OF_RANGE,
                  ACTION_STALE, ACTION_REJECTED, PERMISSION_DENIED, …
snapshot          { source_fingerprint, project_fingerprint, files: n,
                    semantics: latest_known, atomic: false }
analyzer          { version: "<rust-analyzer --version>", binary_sha256,
                    image_id, config_digest, position_encoding: "utf-8"|"utf-16" }
toolchain         { rust_version: "1.98.1", sysroot: "present"|"omitted" }
completeness      { state: complete | incomplete, omissions: [{kind, count}],
                    reasons: [enum] }
limits            { …valores efectivos de ADR-084 §7… }
results           acotado (ADR-084 §7)
```

`omissions.kind` es uno de `external_uri` (resultados fuera de
`file:///source/`), `sysroot_location`, `dependency_location`, `limit_visible`
(más de 512 elementos), `not_utf8_file`, `unresolvable_position`. No existe un
valor `unknown`/`partial` que se reporte como éxito: toda incompletitud es
`incomplete` con motivo siempre visible, igual que la completeness de M1/M2.

### 4. Resultados por tool y sus límites

- **symbols(document)**: árbol `DocumentSymbol` aplanado con `depth`, `name`,
  `kind` (enum cerrado `SymbolKind` de LSP), `detail?`, `deprecated`, `range`,
  `selection_range`, ordenado por `range.start`; ≤512 visibles.
- **symbols(workspace)**: lista `{name, kind, container?, file, range}` solo
  bajo `/source`; ≤512; los externos cuentan en `omissions`.
- **references**: `{file, range, is_declaration}` ordenados por `(file,
  start)`; ≤512.
- **diagnostics**: `{file, range, severity (error|warning|information|hint),
  code?, source: "rust-analyzer", message, related: [{file, range, message}]}`;
  ordenados; ≤512; `readiness: quiescent | not_quiescent`.
- **actions**: `{action_digest, title, kind, is_preferred, applicability:
  applicable | rejected {reason}, edits_summary: {files, edits, bytes_delta}}`;
  ≤32 acciones, ≤128 edits totales. Razones de rechazo, enum cerrado: `command`,
  `snippet`, `resource_operation` (create/rename/delete), `external_uri`,
  `version_mismatch`, `overlapping_ranges`, `edit_limit`, `bytes_limit`,
  `not_utf8`, `unresolved_edit`.
- **action.apply**: `preview` produce un plan M2 `{plan_id (mut_…), plan_digest,
  expires_in, diff (unified, exacto, ≤512 KiB o truncado con marca), changes[]}`;
  `commit` y `receipt` son idénticos en forma a `rust.fmt.apply`.

### 5. Aceptación del `WorkspaceEdit` y rechazos cerrados

Solo se aceptan `TextEdit`s sobre archivos existentes ya capturados en el
snapshot, con rangos que no se solapan entre sí, y dentro de los techos de
edits/bytes de la tabla anterior. Se rechaza, sin excepción y sin intento de
resolución parcial: `Command` embebido, `SnippetTextEdit`, cualquier
`ResourceOperation` (`create`/`rename`/`delete`), cualquier URI fuera de
`file:///source/`, y cualquier acción cuya versión de documento no coincida con
la capturada. No hay fallback a un editor o shell genérico: una acción no
aplicable conserva su razón de rechazo y no se reintenta con una forma
distinta.

### 6. Flujo `action_digest` → preview → commit → receipt

`rust.analyzer.action.apply` reutiliza el writer M2 sin un segundo lock, un
segundo journal ni un segundo filesystem de staging. El `WorkspaceEdit`
resuelto por rust-analyzer se traduce a un `MutationCandidate { kind:
MutationKind::AnalyzerActionApply, before, after, validation }`; el resto del
ciclo de vida (`MutationPlans` en memoria, `commit_mutation`, `MutationPublisher`,
journal, replay, receipt, recovery) es exactamente el de ADR-050/ADR-052. El
plan de acción registra además `analyzer.config_digest` y `binary_sha256`
(ADR-084 §1) y se rechaza en `commit` si cambian respecto al `preview`: un
rollback de runtime invalida los planes pendientes en vez de aplicarlos contra
una identidad distinta. `commit`/`receipt` revalidan el permiso host, la
generación del proyecto y el plan exactamente como cualquier otra mutación M2;
no hay una segunda ruta de autorización.

### 7. Posiciones

Todas las posiciones públicas de las cinco tools son `Position` del dominio:
línea y columna en **Unicode scalars, 1-based**. No se introduce UTF-16 ni
offsets de byte en ningún campo público; la traducción de la codificación que
rust-analyzer negocie internamente (ADR-084 §5) ocurre en el adapter, antes de
construir la respuesta.

## Alternatives considered

- **Cinco tools frente a una tool multiplexada.** Una sola tool con un campo
  `operation` mezclaría cuatro presupuestos y cuatro superficies de permiso en
  un contrato, y forzaría un schema de entrada con campos condicionales según
  la operación. Se descarta por la misma razón que ADR-076 §Alternatives
  descartó `rust.performance` único: permisos y presupuestos distintos exigen
  tools distintas.
- **Actions como Resource.** Un Resource no admite parámetros de invocación
  (`range`, `only`) ni una relación 1:1 con una consulta LSP puntual sobre un
  snapshot vivo; convertiría una consulta acotada en una lectura de estado
  ambigua y perdería el `expected_project_fingerprint` obligatorio.
- **Aplicar mediante un modo tipado de una tool M2 existente.** Cambiaría un
  contrato ya calificado (`rust.fmt.apply` o cualquier otra) para aceptar una
  forma de candidato ajena a su dominio; D25/D26 exigen sin excepción no
  ampliar un contrato ya congelado. `MutationKind::AnalyzerActionApply` es la
  única extensión del enum de dominio, no del schema público de una tool
  anterior.
- **Exponer hover/go-to-definition ahora.** El plan M6 las deja Deferred
  explícitamente (§32); añadirlas sin scope y contrato propios convertiría una
  extensión demand-driven en una feature M6 silenciosa.

## Consequences

El inventario público pasa de treinta y una a treinta y seis tools. Las
treinta y una anteriores no cambian de forma: un test de contrato las compara
byte a byte contra la base M5, igual que ADR-076 hizo contra M4. Las cinco
tools nuevas añaden sus propios snapshots
(`crates/mcp-server/tests/snapshots/analyzer-*-tool.json`) y wire tests en las
cinco versiones MCP soportadas. Ningún enum compartido se amplía salvo
`MutationKind`, que gana exactamente la variante `AnalyzerActionApply`; el
schema del receipt M2 no cambia de forma.

G6 exige verificar que un journal ADR-052 preexistente con un `kind`
desconocido — es decir, escrito por un binario anterior a esta ADR y leído por
uno posterior, o viceversa tras un rollback — sigue **rechazándose antes de
cualquier efecto**, nunca interpretándose con un default silencioso. Esta
comprobación es una prueba obligatoria de M6-05/M6-06, no una garantía nueva:
es la misma disciplina de journal versionado que ADR-052 ya exige para
cualquier variante de `MutationKind`.
