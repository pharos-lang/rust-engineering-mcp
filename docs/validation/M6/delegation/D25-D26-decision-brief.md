# D25/D26 — decisiones del orquestador para M6 (borrador vinculante para los ADR)

Autor: Claude Fable 5.1 (orquestador). Fecha: 2026-09-11. Estado: **decidido
salvo los puntos marcados `[R01]`**, que se cierran con el informe de
investigación R01 antes de redactar ADR-083 (D25) y ADR-084 (D26). Los ADR los
redacta un worker Sonnet sobre este texto; no cambian una decisión sin volver
al orquestador. Fuentes: [plan M6](../../../roadmap/m6-analyzer.md),
[D25/D26](../../../roadmap/adr-backlog-m2-m8.md), ADR-008/009/013/031/050/052,
[dossier de aprovisionamiento](../../../roadmap/m6-provisioning-request.md).

## 0. Hechos del código actual en los que se apoya

- Posiciones públicas existentes: `rust_engineering_domain::Position` es
  línea y **columna en Unicode scalars, ambas 1-based**; `ByteRange` 0-based
  exclusivo; `SourceSpan` con `bytes: Option<ByteRange>`. M6 reutiliza estos
  tipos en el wire; no introduce un segundo sistema de posiciones.
- Captura: `ProjectRegistry::source` → `SourceBundle` (ADR-031: 4096
  entradas, profundidad 32, ruta 100 ASCII, 1 MiB/archivo, 16 MiB total, sin
  `.cargo/config*`, sin `target/`/`.git`). La captura no es atómica.
- Writer M2: `MutationCandidate { kind, before: SourceBundle, after:
  SourceBundle, validation: String }`; `MutationPlans` en memoria (TTL 600 s,
  ≤4 planes, ≤64 MiB), `commit_mutation` con `MutationPublisher` (journal
  ADR-052, replay, receipt, recovery). `MutationKind` es enum de dominio.
- Gateway: `RustGateway` = volumen + fase ingest + fase run por contenedor
  (`--network=none --read-only --cap-drop=ALL --pids-limit=128 --cpus=1
  --memory=1g`, seccomp `seccomp-rust.json`, usuario 65534, `/source` RO,
  `/work` tmpfs exec 512m, `/tmp` tmpfs noexec 64m, env reconstruido). El
  `supervisor` escribe todo el stdin y luego lee hasta salida: **no hay
  sesión dúplex**.
- Tier ADR-009: `rust.project.inspect` (solo `cargo metadata`) es
  `restricted` con `executes_project_code=false`.

## 1. D25 — contrato analyzer y acciones

### 1.1 Inventario exacto de tools (5 nuevas; las 31 existentes intactas)

| Tool | Efecto | Annotations | Permiso host |
| --- | --- | --- | --- |
| `rust.analyzer.symbols` | lectura | readOnly, idempotent | `--rust` (gateway) |
| `rust.analyzer.references` | lectura | readOnly, idempotent | `--rust` |
| `rust.analyzer.diagnostics` | lectura | readOnly, idempotent | `--rust` |
| `rust.analyzer.actions` | lectura (lista + edits resueltos, sin efectos) | readOnly, idempotent | `--rust` |
| `rust.analyzer.action.apply` | **escritura** M2 (preview/commit/receipt) | destructive=false, idempotent=false | `--rust` + nuevo `--allow-analyzer-action-write <root>` (misma familia que `--allow-fmt-write`) |

Hover, go-to-definition, rename y cualquier otra capacidad LSP quedan
**Deferred** (C14); no se exponen ni como campo opcional.

### 1.2 Entradas

Comunes: `project_ref` (`^prj_[0-9a-f]{32}$`) y `expected_project_fingerprint`
(obligatorio en `actions`/`action.apply`, opcional en lecturas: si se envía y
no coincide con la identidad viva → `blocked/CONFLICT`, nunca datos stale).
Archivo: `file` = ruta relativa POSIX dentro de la captura (mismas reglas que
`validate_source_path`), sin `..`, sin absoluta, `.rs` obligatorio.
`position` = `Position` (1-based, Unicode scalar); `range` = `{start, end}`
con `start ≤ end`. `timeout_seconds` con default y máximo por tool (§2.4).

- `symbols`: `{ scope: "document", file }` o `{ scope: "workspace", query
  (1..=128 chars, sin control chars) }`.
- `references`: `{ file, position, include_declaration: bool = true }`.
- `diagnostics`: `{ file }` (un archivo por llamada; solo diagnósticos
  **nativos** de rust-analyzer; `cargo check` sigue siendo `rust.check`).
- `actions`: `{ file, range, only: [kind]? }` con `kind` ∈ enum cerrado
  (`quickfix`, `refactor`, `refactor_extract`, `refactor_inline`,
  `refactor_rewrite`, `source`) `[R01: kinds que emite RA]`.
- `action.apply`: `{ mode: preview { expected_project_fingerprint,
  action_digest, file, range } | commit { plan_id, plan_digest,
  idempotency_key } | receipt { operation_id, recover } }` — misma forma que
  `rust.fmt.apply`. `action_digest` es el sha256 del `CodeAction` resuelto
  (título, kind, edits normalizados, identidad analyzer/config/source); preview
  vuelve a consultar RA sobre una **captura nueva** y exige que el digest
  recalculado coincida; si no, `blocked/ACTION_STALE`.

### 1.3 Salida común (envelope)

```text
status            passed | failed | blocked | unavailable | cancelled  (semántica M2/M5)
reason            enum cerrado (CONFLICT, ANALYZER_NOT_READY, ANALYZER_CRASHED, FRAME_LIMIT,
                  MESSAGE_LIMIT, RESULT_LIMIT, TIMEOUT_INITIALIZE, TIMEOUT_QUERY, TIMEOUT_TOTAL,
                  UNSUPPORTED_PROJECT_CONFIG, FILE_NOT_UTF8, FILE_NOT_IN_SNAPSHOT,
                  POSITION_OUT_OF_RANGE, ACTION_STALE, ACTION_REJECTED, PERMISSION_DENIED, …)
snapshot          { source_fingerprint, project_fingerprint, files: n, semantics: latest_known,
                    atomic: false }
analyzer          { version: "<rust-analyzer --version>", binary_sha256, image_id,
                    config_digest, position_encoding: "utf-8"|"utf-16" }
toolchain         { rust_version: "1.98.1", sysroot: "present"|"omitted" }
completeness      { state: complete | incomplete, omissions: [{kind, count}],
                    reasons: [enum] }
limits            { …valores efectivos de §2.4… }
results           bounded (§2.4)
```

`omissions.kind` ∈ `external_uri` (resultados fuera de `file:///source/`),
`sysroot_location`, `dependency_location`, `limit_visible` (más de 512),
`not_utf8_file`, `unresolvable_position`. Nada de `unknown`/`partial` como
pass: `incomplete` con motivo siempre visible.

### 1.4 Resultados por tool

- symbols(document): árbol `DocumentSymbol` aplanado con `depth`, `name`,
  `kind` (enum cerrado LSP `SymbolKind`), `detail?`, `deprecated`, `range`,
  `selection_range`, ordenado por `range.start`; ≤512 visibles.
- symbols(workspace): lista `{name, kind, container?, file, range}` solo bajo
  `/source`; ≤512; `omissions` cuenta los externos.
- references: `{file, range, is_declaration}` ordenados por (file, start);
  ≤512.
- diagnostics: `{file, range, severity (error|warning|information|hint),
  code?, source: "rust-analyzer", message, related: [{file, range,
  message}]}`; ordenados; ≤512; `readiness: quiescent | not_quiescent`.
- actions: `{action_digest, title, kind, is_preferred, applicability:
  applicable | rejected {reason}, edits_summary: {files, edits, bytes_delta}}`;
  ≤32 acciones, ≤128 edits totales. Razones de rechazo (enum): `command`,
  `snippet`, `resource_operation` (create/rename/delete), `external_uri`,
  `version_mismatch`, `overlapping_ranges`, `edit_limit`, `bytes_limit`,
  `not_utf8`, `unresolved_edit`.
- action.apply preview: plan M2 `{plan_id (mut_…), plan_digest, expires_in,
  diff (unified, exacto, ≤512 KiB o truncado con marca), changes[]}`; commit y
  receipt: idénticos a `rust.fmt.apply`.

### 1.5 Semver/compatibilidad

Cinco contratos nuevos versionados por snapshot (`crates/mcp-server/tests/
snapshots/analyzer-*-tool.json`) y wire tests en las cinco versiones MCP. Enum
compartidos **no** se amplían salvo `MutationKind::AnalyzerActionApply`
(dominio; journal ADR-052 registra el kind: comprobar que un journal viejo con
kind desconocido sigue rechazándose antes de efectos — G6). El schema del
receipt M2 no cambia de forma.

### 1.6 Threat model (G2) — resumen por activo

| Amenaza | Control | Oráculo |
| --- | --- | --- |
| LSP hostil/malformado/oversized (RA comprometido por el proyecto) | codec con `Content-Length` ≤ 1 MiB, cabeceras acotadas, JSON estricto, ids propios, respuestas tardías/duplicadas descartadas; toda respuesta se valida contra un DTO cerrado | fake peer hostil en tests |
| `rust-analyzer.toml` / config del proyecto que reactiva build scripts, proc macros, check, `overrideCommand`, `extraEnv` | **rechazo en captura** de cualquier `rust-analyzer.toml` (cualquier profundidad, case-insensitive) → `UNSUPPORTED_PROJECT_CONFIG`; `initializationOptions` constante; `HOME`/`XDG_CONFIG_HOME` vacíos en el guest `[R01: precedencia exacta y lista de claves «local»]` | fixture con ratoml hostil rechazada; calibración nativa: RA con nuestro config no lanza `build-script-build` ni proc-macro-srv (observación `container top`) |
| Never-ready / hang / crash | deadlines por fase (§2.4), kill del contenedor y join, `incomplete`/`unavailable` | fixtures never-ready (fake) y crash real (SIGKILL al RA en calibración) |
| Path externo / URI fuera de `/source` | normalización solo bajo `/source`; externos → omisión o rechazo de acción | fixtures con sysroot/deps |
| Acción stale / rebase silencioso | digest de acción + fingerprint del proyecto + versión de documento; `ACTION_STALE`, nunca rebase | test de cambio entre `actions` y `preview` |
| Secretos en logs | stderr de RA no se publica; se registra solo tamaño/hash; tracing con ids opacos | test de que el resultado no contiene stderr |
| Servidor pide `workspace/applyEdit`, `client/registerCapability`, etc. | ninguna petición servidor→cliente concede nada: se responde con error/`null` sin efecto `[R01: conjunto mínimo de capabilities]` | fake peer que las emite |

## 2. D26 — runtime rust-analyzer y LSP

### 2.1 Identidad

rust-analyzer 1.98.1 `aarch64-unknown-linux-gnu` (tarball sha256
`a0fd960a…`) en `/opt/analyzer/bin/rust-analyzer`, imagen M6 derivada de la M5
(`e0a5ca16…`), `rust-src` 1.98.1 bajo `/opt/rust`. Constante
`APPROVED_M6_IMAGE` admitida en el gateway por ADR de admisión con
calificación nativa (patrón ADR-077). Cada resultado publica `analyzer.version`
(salida real de `--version` capturada en calibración), `binary_sha256`,
`image_id` y `config_digest`.

### 2.2 Lifecycle: instancia transitoria por consulta (alternativa elegida)

Se elige **servidor transitorio por consulta** frente al pool acotado: la
captura ya es por llamada, el gateway serializa un job por vez (`busy`), y un
pool añadiría estado, invalidación y superficie de ataque sin un SLI que lo
justifique. Reconsiderar solo con medición de `cold init` en M6-06.

Fases (todas dentro del `WorkBudget` existente):

1. captura `SourceBundle` (+ rechazo de `rust-analyzer.toml`);
2. volumen + ingest (fases existentes);
3. **fase `Analyzer`** (nueva `Phase`): contenedor con programa
   `/opt/analyzer/bin/rust-analyzer` (sin subcomando), env reconstruido (+
   `RA_LOG` off), `--user=65534`, mismo seccomp `seccomp-rust.json` salvo
   evidencia de calibración; **sesión dúplex** nueva en `supervisor`
   (escritura de frames y lectura concurrente, límites de bytes por frame y
   totales, deadline, cancelación, `kill` + `rm` + verificación de ausencia);
4. `initialize` (capabilities mínimas, `positionEncodings: ["utf-8",
   "utf-16"]` `[R01]`, `initializationOptions` constante) → `initialized` →
   esperar `experimental/serverStatus{quiescent:true}` `[R01]`; sin oráculo en
   el plazo → `ANALYZER_NOT_READY` (`incomplete`, no datos);
5. `textDocument/didOpen` con los bytes exactos capturados y `version: 1`;
6. **una** petición; para `actions` los edits llegan en línea en la respuesta de `codeAction` (sin `resolveSupport`, no hay `codeAction/resolve`; véase §4.3);
7. `shutdown` → `exit`; si no sale en 5 s, kill; cleanup unido y verificado
   antes de liberar `busy` (G3).

Sin `didChange`: cualquier cambio de fuente es una llamada nueva con captura
nueva. Invalidación de planes: como M2 (fingerprint del proyecto + digest de
plan); además el plan de acción registra `analyzer.config_digest` y
`binary_sha256` y se rechaza si cambian (rollback de runtime).

### 2.3 Configuración fija (`initializationOptions`)

Claves y valores exactos se fijan tras R01; intención vinculante:
`procMacro.enable=false`, `cargo.buildScripts.enable=false`,
`checkOnSave=false`, `cargo.autoreload=false`, `files.watcher="client"` (sin
registro dinámico), `cachePriming.enable=false`, `linkedProjects=
["/source/Cargo.toml"]`, `cargo.noDeps` `[R01: offline sin registry]`,
`cargo.sysroot="discover"` con `rust-src` presente, `diagnostics.experimental.
enable=false`, `workspace.symbol.search.limit=512`, `numThreads` acotado
`[R01]`, `lru.capacity` acotado `[R01]`. `config_digest` = sha256 del JSON
canónico. Cualquier clave desconocida para el binario → calibración falla.

### 2.4 Budgets (fijados antes del código; unidad, fase, default/máximo, prueba de exceso)

| Límite | Valor | Fase | Exceso |
| --- | --- | --- | --- |
| RA activos por servidor | 1 (lock `busy` del gateway) | toda | `BUSY` |
| Frame LSP (Content-Length) | ≤ 1 MiB | sesión | `FRAME_LIMIT` → kill |
| Mensajes LSP por job | ≤ 4096 | sesión | `MESSAGE_LIMIT` → kill |
| Bytes stdout/stderr RA por job | ≤ 16 MiB / 1 MiB | sesión | `MESSAGE_LIMIT` |
| Symbols / references visibles | ≤ 512 | resultado | `incomplete: limit_visible` |
| Acciones / edits | ≤ 32 / ≤ 128 | resultado | `incomplete` / acción rechazada `edit_limit` |
| Resultado MCP | ≤ 512 KiB | resultado | recorte declarado (`RESULT_LIMIT`) |
| initialize→quiescent | ≤ 60 s (default 60, máx 60) | 4 | `ANALYZER_NOT_READY` |
| Petición | ≤ 30 s | 6 | `TIMEOUT_QUERY` |
| Total por llamada | ≤ 180 s (incluye captura, ingest, cleanup) | toda | `TIMEOUT_TOTAL` |
| Memoria / PIDs / CPU guest | 1 GiB / 128 / 1 (ya impuestos por el gateway) | 3–7 | OOMKilled → `ANALYZER_CRASHED` con `oom_killed=true` |
| Retención | planes M2 (600 s, ≤4, ≤64 MiB); sin cache durable | — | como M2 |

SLI a medir en M6-06: cold init, tiempo hasta quiescent, latencia por
petición, RSS pico (cgroup), invalidaciones, stale descartados,
cancel→cleanup.

### 2.5 Encoding y posiciones

Preferir `utf-8` `[R01]`; si el servidor negocia `utf-16`, el adapter
convierte contra los bytes capturados. Público siempre `Position` (Unicode
scalar). Offsets dentro de un codepoint, rangos invertidos o fuera del archivo
→ `POSITION_OUT_OF_RANGE`; archivos no UTF-8 → `FILE_NOT_UTF8` (y omisión en
listados). Tests con fixtures Unicode (BMP, astral, combinantes, CRLF, BOM).

### 2.6 Native positivo y rollback

Positivo solo macOS ARM64/APFS + guest Linux ARM64 imagen M6; Linux/Windows
fail-closed. Rollback: apuntar el gateway al digest M5; los planes de acción
ligados a la identidad M6 se revocan (digest incluye la identidad); receipts y
journals conservan formato.

## 3. Cortes y paquetes de trabajo (archivos disjuntos)

| Corte | Paquete | Agente | Archivos |
| --- | --- | --- | --- |
| DoR | W01 aprovisionamiento | Sonnet | `fixtures/rust-runtime/m6/**`, `scripts/build-m6-runtime.py`, `scripts/test-m6-provisioning.py`, ADR-082, wiring gate/sonar/ci.md |
| DoR | W02 ADR-083 (D25) + ADR-084 (D26) | Sonnet | `docs/adr/ADR-083*`, `ADR-084*`, backlog D25/D26 → Decided |
| M6-01 | W03 dominio + codec LSP + fake-peer hostil | Sonnet (High) | `crates/domain/src/analyzer.rs`, `crates/execution-adapter/src/lsp_codec.rs` (+tests) |
| M6-01 | W04 sesión dúplex + fase Analyzer + lifecycle + admisión imagen M6 + calibración nativa | **Opus** | `crates/execution-adapter/src/supervisor_session.rs`, `analyzer_gateway.rs`, `analyzer_native.rs`, `rust_gateway.rs` (solo `Phase::Analyzer` y admisión), ADR-085 admisión |
| M6-01 | W05 application port + tool `symbols` + snapshot/wire/protocol tests + docs | Sonnet | `crates/application/src/analyzer.rs`, `crates/mcp-server/src/stdio/analyzer*.rs`, snapshots, docs |
| M6-02/03 | W06 references/workspace symbols; W07 diagnostics | Sonnet | módulos propios |
| M6-04 | W08 WorkspaceEdit→candidato M2 (validación, rechazos) | **Opus** | `crates/domain/src/analyzer_edit.rs`, application |
| M6-05 | W09 `action.apply` sobre writer M2 + permiso host + calibración | **Opus** | mcp-server analyzer apply, host_config |
| M6-06 | W10 fixtures hostiles + suite nativa `test-m6-runtime.py` + clientes `test-m5-clients`-style; reviews G8; handoff | Sonnet + reviews Opus/Sonnet/Gemini | scripts, docs/validation/M6 |

## 4. Cierre de los puntos `[R01]` (2026-09-11, tras la [disposición R01](R01-ra-research/disposition.md))

Todo lo siguiente es decisión vinculante, y **cada punto tiene un oráculo
local en la calibración W04** porque el informe R01 contenía hashes fabricados
y no se acepta como evidencia por sí mismo.

1. **Encoding**: anunciar `general.positionEncodings: ["utf-8"]`; exigir
   `positionEncoding == "utf-8"` en el `initialize` real, si no → `unavailable`
   (`ANALYZER_CAPABILITY_MISMATCH`). El adapter traduce offsets de byte →
   `Position` (Unicode scalar) contra los bytes capturados; índice de líneas
   con `\n` como único salto (igual que RA; `\r` queda dentro de la línea).
2. **Readiness**: capability `experimental.serverStatusNotification: true`;
   oráculo = `experimental/serverStatus` con `quiescent: true` y `health` ≠
   `error` (`warning` se registra en `completeness.reasons`, p. ej. sysroot).
   Sin oráculo antes de 60 s → `ANALYZER_NOT_READY`.
3. **Capabilities mínimas del cliente** (sin peticiones servidor→cliente):
   `window.workDoneProgress=false`, `workspace.configuration=false`,
   `workspace.didChangeWatchedFiles.dynamicRegistration=false`;
   `textDocument.documentSymbol.hierarchicalDocumentSymbolSupport=true`;
   `workspace.workspaceEdit.documentChanges=true`, **sin**
   `resourceOperations`, **sin** `experimental.snippetTextEdit` (RA recorta
   los tabstops a texto plano; no se ejecuta ningún snippet), **sin**
   `experimental.commands`, **sin** `codeAction.resolveSupport` (edits en
   línea; no hay `codeAction/resolve`); `textDocument.diagnostic` (pull).
   Cualquier petición servidor→cliente que llegue igualmente se responde con
   error `-32601` y se cuenta; nunca concede nada. Sí se anuncia
   `textDocument.codeAction` con literal support —`codeActionLiteralSupport`
   listando los siete kinds que M6 resuelve, `isPreferredSupport: true`,
   `dataSupport: false`, `disabledSupport: false`, y sin `resolveSupport`—
   porque sin él un servidor conforme puede responder `textDocument/codeAction`
   solo con objetos `Command`, que este cliente rechaza siempre, dejando la
   funcionalidad vacía en silencio.
4. **Diagnósticos**: **pull** `textDocument/diagnostic` tras quiescent; las
   notificaciones `publishDiagnostics` se descartan (deterministas por
   petición, no por debounce). Solo diagnósticos nativos; `flycheck` no
   existe con `checkOnSave=false`.
5. **Config fija** (`initializationOptions`, JSON anidado, claves sin
   prefijo):
   `cargo.buildScripts.enable=false`, `procMacro.enable=false`,
   `checkOnSave=false`, `cargo.noDeps=true`, `cargo.sysroot="discover"`,
   `cargo.sysrootQueryMetadata=false`, `cargo.autoreload=false`,
   `cargo.targetDir=null`, `files.watcher="client"`,
   `cachePriming.enable=false`, `numThreads=1`, `lru.capacity=64`,
   `linkedProjects=["/source/Cargo.toml"]`,
   `diagnostics.experimental.enable=false`,
   `workspace.symbol.search.scope="workspace"`,
   `workspace.symbol.search.kind="all_symbols"`,
   `workspace.symbol.search.limit=512`, `references.excludeImports=false`,
   `references.excludeTests=false`. Como RA **ignora en silencio** claves
   desconocidas, la calibración W04 vuelca `--print-config-schema` del
   binario real de la imagen y falla si alguna clave de esta lista no existe
   en él; el schema volcado se archiva con hash en el recibo.
6. **`rust-analyzer.toml`**: la captura rechaza cualquier archivo llamado
   `rust-analyzer.toml` o `.rust-analyzer.toml` (case-insensitive, cualquier
   profundidad) con `UNSUPPORTED_PROJECT_CONFIG`, porque el workspace puede
   reactivar build scripts / `overrideCommand` / `extraEnv` por encima de
   `initializationOptions` y no existe opción para desactivar su carga.
   Prueba nativa: fixture con ratoml hostil rechazada antes de arrancar RA;
   además, `container top` durante initialize sin `build-script-build` ni
   `proc-macro-srv` con la config fija.
7. **Procesos externos esperados** durante initialize (todos binarios del
   guest, ninguno del proyecto): `rustc --print sysroot`, `cargo
   locate-project`, `cargo --version`, `rustc -vV`, `cargo rustc -Z
   unstable-options --print cfg` (falla en stable y RA cae a `rustc --print
   cfg -O`), `--print target-spec-json` (ídem), `cargo metadata --no-deps
   --format-version 1`. `--no-deps` no escribe `Cargo.lock`; `/source` sigue
   RO. Cualquier otro programa observado en calibración es hallazgo P1.
8. **Ciclo de vida**: `shutdown` → `exit` → esperar salida (código 0
   esperado; ≠0 se registra); 5 s de gracia y kill; EOF/timeout/cancel →
   kill + rm + verificación de ausencia antes de liberar `busy`.
9. **`ContentModified` (-32801)**: no se reintenta (no hay `didChange`); se
   clasifica `ANALYZER_CRASHED`-like → `unavailable` con motivo
   `content_modified`, porque en nuestra sesión no debería ocurrir.
