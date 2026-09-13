# ADR-084 — Runtime rust-analyzer y ciclo de vida LSP

Fecha: 2026-09-11.

## Status

Accepted por decisión del orquestador (Claude Fable 5.1) del 2026-09-11, dentro
del encargo M6, sobre el [brief D25/D26](../validation/M6/delegation/D25-D26-decision-brief.md)
§2 y su cierre `[R01]` §4. D26 estaba **Proposed**. Depende de
[ADR-082](ADR-082-m6-runtime-provisioning.md) para el aprovisionamiento de los
binarios (adquisición/construcción de la imagen); la **admisión** de la imagen
en el gateway, con su calificación nativa, es un ADR separado (patrón
[ADR-077](ADR-077-m5-runtime-admission.md)) y no se decide aquí.
[ADR-083](ADR-083-analyzer-contract-and-actions.md) fija el contrato público
que consume este runtime.

## Context

El [plan M6](../roadmap/m6-analyzer.md) exige rust-analyzer exacto sobre un
snapshot identificado, con lifecycle/env/process ownership propiedad del
Execution Gateway único (ADR-008): «no `Command` fuera de él» y «MCP sigue
`rmcp`; el codec LSP no es una implementación alternativa de MCP/JSON-RPC».
`RustGateway` hoy ejecuta cada fase como contenedor con `docker container
create` + `start --attach [--interactive]`, y el `supervisor` escribe todo el
stdin y luego lee stdout/stderr hasta la salida: no existe una sesión dúplex
(escribir frames mientras se leen respuestas), que es lo que un servidor LSP
requiere. Esa es la frontera nueva de este ADR.

El informe de investigación R01 se descartó como evidencia por hashes de
tarball fabricados en su Q1; el resto de sus afirmaciones se acepta
**condicionalmente**, como mapa de verificación, con un oráculo local en la
calibración nativa de W04 (binario real, transcript real o schema real) — nunca
como cita. Los puntos marcados `[R01]` en el brief se cierran en su §4 con la
[disposición](../validation/M6/delegation/R01-ra-research/disposition.md); ese
cierre es la fuente de las decisiones de esta sección, no el informe.

## Decision

### 1. Identidad exacta

`rust-analyzer` 1.98.1 `aarch64-unknown-linux-gnu`, tarball `.tar.xz` sha256
`a0fd960a9ab36193ae9ba4310e5f780f6ca38fa86160fae739be4ac541b6d10c`, instalado en
`/opt/analyzer/bin/rust-analyzer`; `rust-src` 1.98.1, tarball `.tar.xz` sha256
`5c846ebcebcc7e2e0777a4cdaa12051691593f16a7e94edbae5e6241cc62d98c`, bajo
`/opt/rust`. Ambos provienen del mismo manifest de canal fijado que gobierna el
resto del runtime (ADR-082). La imagen M6 se deriva **por digest** de la imagen
M5 ya admitida y añade exactamente estos dos componentes; no cambia toolchain,
plugins M3, binarios M4/M5, usuario, `WORKDIR` ni `PATH` (ADR-082 §Ítem C). La
adquisición/construcción de la imagen es ADR-082; su **admisión** en el
gateway, con calificación nativa propia, es un ADR de admisión separado
(patrón ADR-077) y queda fuera del alcance de esta decisión.

Cada resultado de las cinco tools de ADR-083 publica `analyzer.version` (salida
real de `rust-analyzer --version`, capturada en la calibración nativa —
subject to native calibration: R01 afirmaba `1.98.1 (48a229cea 2026-09-01)` sin
oráculo local, y no se acepta como constante hasta que W04 la capture),
`binary_sha256`, `image_id` y `config_digest` (§3).

### 2. Lifecycle: instancia transitoria por consulta

Se elige un **servidor rust-analyzer transitorio por consulta** frente a un
pool acotado keyed por identidad de source/analyzer/config/toolchain. La
captura de `SourceBundle` ya es por llamada (ADR-031), el gateway ya serializa
un job a la vez (`busy`, ADR-008), y un pool añadiría estado, invalidación y
superficie de ataque adicionales sin un SLI que lo justifique todavía.
Reconsiderar esta elección exige medición real de `cold init` en M6-06, no una
preferencia de diseño.

Las siete fases, todas dentro del `WorkBudget` existente:

1. **Captura** del `SourceBundle` (ADR-031), incluyendo el rechazo de
   `rust-analyzer.toml`/`.rust-analyzer.toml` (§6).
2. **Volumen + ingest** (fases ya existentes del gateway).
3. **Fase `Analyzer`** (nueva `Phase` del gateway único): contenedor con
   programa `/opt/analyzer/bin/rust-analyzer` (sin subcomando), env
   reconstruido (incluye `RA_LOG` desactivado), `--user=65534`, el mismo
   `seccomp-rust.json` salvo que la calibración nativa demuestre una excepción
   necesaria. Esta fase introduce la **sesión dúplex** nueva en el
   `supervisor` del execution-adapter: escritura de frames LSP mientras se lee
   concurrentemente, con límites de bytes por frame y totales, deadline,
   cancelación y `kill` + `rm` + verificación de ausencia antes de liberar
   `busy`. No existe `Command` fuera del gateway único, y este codec LSP no es
   una segunda pila MCP/JSON-RPC: es un protocolo de transporte distinto,
   propio de esta fase, que nunca se expone al peer MCP.
4. **`initialize`** con las capabilities mínimas de §4 e
   `initializationOptions` constantes (§3) → `initialized` → esperar el
   oráculo de readiness (§5). Sin oráculo dentro del plazo →
   `ANALYZER_NOT_READY` (`incomplete`, sin datos).
5. **`textDocument/didOpen`** con los bytes exactos capturados y `version: 1`.
   No hay `didChange`: cualquier cambio de fuente es una llamada nueva con
   captura nueva.
6. **Una** petición del tipo que la tool exige; para `references`, **dos**
   peticiones (enmienda 2026-09-12, D25 R5/M6-02): rust-analyzer no marca en su
   propia respuesta cuál ubicación es la declaración, así que la misma sesión
   envía `textDocument/references` dos veces —`includeDeclaration: true` y
   `false`— y toda ubicación presente solo en la primera se marca
   `is_declaration: true` por diferencia de conjuntos; ambas peticiones
   comparten el presupuesto de esta fase (§8), nunca uno cada una. Para
   `actions`, con las capabilities mínimas de §4 (sin
   `codeAction.resolveSupport`), rust-analyzer resuelve los edits dentro de la
   propia respuesta de `codeAction`: no hay una segunda petición
   `codeAction/resolve` en este lifecycle.
7. **`shutdown` → `exit`**; si el proceso no sale en 5 s, `kill`. El cleanup
   del contenedor se une y se verifica antes de liberar `busy` (gateway G3).

Invalidación de planes de acción: como M2 (fingerprint del proyecto + digest
del plan), más el `analyzer.config_digest` y `binary_sha256` registrados en el
plan (ADR-083 §6); un cambio en cualquiera de los dos rechaza el `commit` en
vez de aplicarlo contra una identidad de runtime distinta.

### 3. Configuración fija (`initializationOptions`)

JSON anidado, claves sin prefijo. Valores exactos, cerrados tras la
disposición R01 §4.5 del brief:

```text
cargo.buildScripts.enable=false
procMacro.enable=false
checkOnSave=false
cargo.noDeps=true
cargo.sysroot="discover"
cargo.targetDir=null
files.watcher="client"
cachePriming.enable=false
numThreads=1
lru.capacity=64
linkedProjects=["/source/Cargo.toml"]
diagnostics.experimental.enable=false
workspace.symbol.search.scope="workspace"
workspace.symbol.search.kind="all_symbols"
workspace.symbol.search.limit=512
references.excludeImports=false
references.excludeTests=false
```

`config_digest` es el sha256 del JSON canónico de este objeto. Como
rust-analyzer **ignora en silencio** las claves que no reconoce, un typo en
esta lista dejaría el default del binario en vigor sin ningún error visible;
por eso la calibración nativa de W04 vuelca `--print-config-schema` del
binario real de la imagen admitida y **falla** si alguna clave de esta lista no
existe en ese schema. El schema volcado se archiva con su hash en el recibo de
calibración.

`cargo.sysroot="discover"` exige `rust-src` presente (ADR-082 Ítem B); sin él,
rust-analyzer no puede cargar `core`/`std` y los símbolos, referencias y
diagnósticos sobre la biblioteca estándar quedan sin resolver.

**Enmienda 2026-09-12 (calibración W04).** La lista pasa de diecinueve a
**diecisiete** claves: se eliminan `cargo.sysrootQueryMetadata=false` y
`cargo.autoreload=false`, y con ellas el `config_digest` cambia, que es lo que
debe pasar cuando cambia la configuración enviada. La primera **no existe** en
el schema que imprime el binario admitido (193 claves `rust-analyzer.*`, ninguna
con ese nombre): rust-analyzer la ignoraba en silencio, de modo que su valor no
tenía efecto alguno y solo contaminaba la identidad con la que se invalidan los
planes de acción; el oráculo de esta misma sección es quien lo detectó
([01.md F1](../validation/M6/01.md)). La segunda hacía que **todas** las
sesiones alcanzaran `quiescent` con `health: warning` —el servidor avisa de que
la recarga automática está desactivada y el workspace ha cambiado—, lo que
degradaba cada respuesta M6 a `incomplete` y volvía `complete` inalcanzable
([01.md F2](../validation/M6/01.md)); queda en vigor el default
`cargo.autoreload=true`, que en este lifecycle no recarga nada porque la sesión
es transitoria, no envía `didChange` y monta `/source` en solo lectura. Un
`health: warning` sigue degradando el resultado a `incomplete` (razón
`analyzer_warning`, sin publicar el `message`, que puede llevar texto del
proyecto).

**Enmienda 2026-09-12 (Opción A).** `diagnostics.experimental.enable` queda en
`false`, sin cambio de valor. Calibrado contra la imagen M6 real bajo esta
configuración mínima, `rust.analyzer.diagnostics` es una tool
**exclusivamente sintáctica**: no reporta errores de tipos, de préstamo ni
ítems no resueltos que dependen de `cargo check` (dominio de `rust.check`).
Se evaluó habilitar `diagnostics.experimental.enable=true` (Opción B) para
recuperar diagnósticos semánticos nativos, y se rechazó por ahora: bajo la
misma configuración mínima (sin build scripts, sin proc macros), los
diagnósticos experimentales inundan con falsos `unresolved-macro-call` sobre
macros de la librería estándar (`vec!`, `assert_eq!`, `#[test]`), porque la
resolución de macros std bajo esta configuración no es limpia. El oráculo en
banda de build scripts (`01.md` R1, M6-03) se reasienta en consecuencia sobre
`rust.analyzer.symbols`, no sobre diagnósticos: ver `docs/tools.md`
(`rust.analyzer.diagnostics`) y `docs/security-model.md`. Una decisión futura
podrá habilitar Opción B una vez que la resolución de macros std bajo esta
configuración esté limpia; hasta entonces queda registrada como deuda en
`docs/validation/M6/matrix.md` ("Deuda de M6").

### 4. Capabilities mínimas del cliente

Sin ninguna petición servidor→cliente que conceda nada:
`window.workDoneProgress=false`, `workspace.configuration=false`,
`workspace.didChangeWatchedFiles.dynamicRegistration=false`;
`textDocument.documentSymbol.hierarchicalDocumentSymbolSupport=true`;
`workspace.workspaceEdit.documentChanges=true`, **sin**
`resourceOperations`, **sin** `experimental.snippetTextEdit` (rust-analyzer
recorta los tabstops a texto plano; nunca se ejecuta un snippet), **sin**
`experimental.commands`, **sin** `codeAction.resolveSupport` (los edits viajan
en línea; no existe `codeAction/resolve` en este lifecycle);
`textDocument.diagnostic` anunciado (pull, §5). `general.positionEncodings:
["utf-8"]` (§5). Cualquier petición servidor→cliente que llegue de todos modos
(`workspace/applyEdit`, `client/registerCapability`, etc.) se responde con
error `-32601` y se cuenta; nunca concede efecto alguno. Sí se anuncia
`textDocument.codeAction` con literal support —`codeActionLiteralSupport`
listando los siete kinds que M6 resuelve, `isPreferredSupport: true`,
`dataSupport: false`, `disabledSupport: false`, y sin `resolveSupport`—
porque sin él un servidor conforme puede responder `textDocument/codeAction`
solo con objetos `Command`, que este cliente rechaza siempre, dejando la
funcionalidad vacía en silencio.

### 5. Encoding, readiness y diagnósticos

**Encoding**: se anuncia `general.positionEncodings: ["utf-8"]` y se exige
`positionEncoding == "utf-8"` en la respuesta real de `initialize`; si el
servidor negocia otra cosa, la respuesta es `unavailable` con
`ANALYZER_CAPABILITY_MISMATCH`. El adapter traduce los offsets de byte que
rust-analyzer produce a `Position` (Unicode scalar, público en ADR-083 §7)
contra los bytes exactos capturados; el índice de líneas usa `\n` como único
separador, igual que rust-analyzer (`\r` queda dentro de la línea).

**Readiness**: capability `experimental.serverStatusNotification: true`;
oráculo = notificación `experimental/serverStatus` con `quiescent: true` y
`health` distinto de `error` (`warning` se registra en
`completeness.reasons`, por ejemplo un sysroot incompleto). Sin ese oráculo
antes de 60 s, la fase 4 falla con `ANALYZER_NOT_READY`.

**Diagnósticos**: exclusivamente **pull**, vía `textDocument/diagnostic`
tras alcanzar quiescent. Las notificaciones `publishDiagnostics` que el
servidor emita se descartan sin publicarse: el resultado es determinista por
petición, no por un temporizador de silencio. Solo diagnósticos nativos de
rust-analyzer viajan por esta tool; con `checkOnSave=false` no existe
`flycheck` que mezclar.

**`ContentModified` (-32801)**: no se reintenta — no hay `didChange` en este
lifecycle. Se clasifica como `unavailable` con motivo `content_modified`,
tratado igual que un crash del analyzer, porque en una sesión de una sola
petición no debería producirse.

### 6. Rechazo de `rust-analyzer.toml` en captura

La captura rechaza cualquier archivo llamado `rust-analyzer.toml` o
`.rust-analyzer.toml` (case-insensitive, en cualquier profundidad del árbol
capturado) con `UNSUPPORTED_PROJECT_CONFIG`, **antes** de arrancar
rust-analyzer. La razón: la configuración del workspace tiene precedencia
sobre `initializationOptions` del cliente y puede reactivar
`cargo.buildScripts.enable`/`overrideCommand`, `check.overrideCommand`,
`runnables.command`, `rustfmt.overrideCommand` o `cargo.extraEnv` — y no existe
ninguna opción para desactivar su carga. La configuración fija de §3, por sí
sola, no es containment frente a un `rust-analyzer.toml` hostil (plan M6
§Workspace trust). Prueba nativa: una fixture con un `rust-analyzer.toml`
hostil se rechaza antes de que el proceso arranque; adicionalmente, `container
top` durante `initialize` con la configuración fija no debe mostrar
`build-script-build` ni `proc-macro-srv`.

### 7. Procesos externos esperados durante `initialize`

Todos son binarios del guest, ninguno del proyecto: `rustc --print sysroot`,
`cargo locate-project`, `cargo --version`, `rustc -vV`, `cargo rustc -Z
unstable-options --print cfg` (falla en stable; rust-analyzer cae a `rustc
--print cfg -O`), `--print target-spec-json` (mismo patrón de fallback),
`cargo metadata --no-deps --format-version 1`. `--no-deps` no escribe
`Cargo.lock`; `/source` sigue montado de solo lectura. **Cualquier otro
programa observado en la calibración nativa es un hallazgo P1**, no un detalle
a documentar después.

**Enmienda 2026-09-12 (calibración W12).** rust-analyzer 1.98.1 también lanza
en `initialize` una sonda `rustc` por lotes —`rustc - --crate-name ___
--print=file-names --target … --crate-type … --print=sysroot
--print=split-debuginfo --print=crate-name --print=cfg -Wwarnings`— que vive
unos milisegundos y el muestreo de `container top` solo captura a veces. Se
admite como **consulta de solo lectura**: la única entrada es `-` (fuente
sintética por stdin), cada token pertenece a un vocabulario cerrado (`-`,
`-vV`, `-O`, `-Wwarnings`, `-Z unstable-options`, `--crate-name`/`--crate-type`
con un valor que es una palabra simple, `--target` con el único triple del
guest (`aarch64-unknown-linux-gnu`) y `--print`/`--print=`) y hace falta al
menos un `--print` con un tipo de la lista cerrada `cfg`, `crate-name`,
`file-names`, `split-debuginfo`, `sysroot`, `target-spec-json`. Siguen fuera
cualquier ruta, `-o`, `--out-dir`, `--emit`, `-L`, `--extern`, `-C…`, la forma
`--print KIND=PATH` (escribe a un archivo) y `native-static-libs`/`link-args`
(rustc los imprime al enlazar, así que antes compila). `--target` queda
cerrado al triple del guest y no a cualquier palabra simple porque un triple
distinto haría que rustc buscara `<valor>.json` en disco
(`RUST_TARGET_PATH`, el CWD `/source`, o el sysroot) en vez de responder
desde su spec interno. Oráculo: `rustc_is_readonly_probe` en
`crates/execution-adapter/src/analyzer_native.rs`. Límite conocido del
oráculo de muestreo: `docker container top` une el argv con espacios, así
que un argumento con un espacio dentro se vería como dos, y el corte no
puede ver los límites reales de cada argumento.

### 8. Presupuestos (fijados antes del código)

| Límite | Valor | Fase | Exceso |
| --- | --- | --- | --- |
| RA activos por servidor | 1 (lock `busy` del gateway) | toda | `BUSY` |
| Frame LSP (`Content-Length`) | ≤ 1 MiB | sesión | `FRAME_LIMIT` → kill |
| Mensajes LSP por job | ≤ 4096 | sesión | `MESSAGE_LIMIT` → kill |
| Bytes stdout/stderr RA por job | ≤ 16 MiB / 1 MiB | sesión | `MESSAGE_LIMIT` |
| Symbols / references visibles | ≤ 512 | resultado | `incomplete: limit_visible` |
| Acciones / edits | ≤ 32 / ≤ 128 | resultado | `incomplete` / acción rechazada `edit_limit` |
| Resultado MCP | ≤ 512 KiB | resultado | recorte declarado (`RESULT_LIMIT`) |
| `initialize`→quiescent | ≤ 60 s (default 60, máx 60) | 4 | `ANALYZER_NOT_READY` |
| Petición | ≤ 30 s | 6 | `TIMEOUT_QUERY` |
| Total por llamada | ≤ 180 s (incluye captura, ingest, cleanup) | toda | `TIMEOUT_TOTAL` |
| Memoria / PIDs / CPU guest | 1 GiB / 128 / 1 (ya impuestos por el gateway) | 3–7 | OOMKilled → `ANALYZER_CRASHED` con `oom_killed=true` |
| Retención | planes M2 (600 s, ≤4, ≤64 MiB); sin cache durable | — | como M2 |

### 9. SLIs a medir en M6-06

Cold init, tiempo hasta quiescent, latencia por petición, RSS pico (cgroup
`memory.peak`), invalidaciones, planes stale descartados, cancel→cleanup.
Ninguno de estos valores es un presupuesto normativo todavía: son mediciones
que pueden motivar reconsiderar el lifecycle transitorio (§2).

### 10. Rollback y alcance nativo

Rollback: apuntar el gateway al digest de la imagen M5; los planes de acción
ligados a la identidad M6 (config_digest/binary_sha256, ADR-083 §6) se revocan
por esa comparación, nunca se rebasan silenciosamente; los receipts y journals
conservan su formato. Alcance nativo positivo: exclusivamente host macOS
ARM64/APFS con imagen guest M6 Linux ARM64; Linux/Windows quedan fail-closed
hasta que exista una decisión de portabilidad explícita (D13).

## Alternatives considered

- **Pool acotado keyed por identidad.** Descartado por ahora (§2): añade
  estado, invalidación entre consultas y superficie de ataque sin que ningún
  SLI medido lo justifique frente a la instancia transitoria, que ya encaja con
  la captura por llamada y la serialización `busy` existentes.
- **rust-analyzer en el host macOS.** Descartado: el analyzer parsea y expande
  código del proyecto tratado como hostil; aunque build scripts y proc macros
  queden desactivados por configuración, la configuración no es containment
  (plan M6 §Workspace trust; ADR-082 §3). Contradice ADR-008/031: todo código
  del proyecto corre en el gateway aislado.
- **CLI batch (`rust-analyzer diagnostics`/`scip`).** Descartado: exige
  igualmente el binario exacto, no ofrece code actions, y el plan M6 ya fija
  el lifecycle LSP como el camino crítico (D26).
- **Push diagnostics con temporizador de silencio.** Descartado: un debounce
  no es un oráculo determinista de completitud; el plan M6 exige no inferir
  readiness de un silencio arbitrario. El pull explícito tras quiescent es
  reproducible por petición.
- **Anunciar snippets/commands/resource operations.** Descartado: cada uno
  ampliaría lo que rust-analyzer puede proponer sin que ADR-083 tenga un
  mecanismo de ejecución seguro para ello; todos se rechazan explícitamente en
  las capabilities anunciadas (§4) y en las reglas de aceptación de
  `WorkspaceEdit` (ADR-083 §5).

## Consequences

El gateway único gana una fase (`Phase::Analyzer`) y el execution-adapter gana
una sesión dúplex nueva en su `supervisor`; ninguna fase existente cambia de
forma. El codec LSP vive enteramente dentro de esa fase y no se convierte en un
segundo transporte MCP. Las cinco tools de ADR-083 dependen de que la
calibración nativa de W04 confirme, con oráculo local, cada punto que R01 solo
proponía: la versión real de `--version`, el `positionEncoding` negociado, el
transcript de `experimental/serverStatus`, el schema de
`--print-config-schema` y el árbol de procesos observado durante
`initialize`. Mientras esa calibración no exista, ningún valor marcado
"subject to native calibration" en este ADR se trata como medido.

## Sources

- [LSP 3.17 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/).
- [rust-analyzer book](https://rust-analyzer.github.io/book/).
- [rust-analyzer configuration reference](https://rust-analyzer.github.io/book/configuration.html).
- [`channel-rust-1.98.1.toml`](https://static.rust-lang.org/dist/channel-rust-1.98.1.toml), el manifest de canal fijado (ADR-082).

Los hallazgos del informe R01 son objetivos de verificación para la
calibración nativa de W04, no evidencia: la
[disposición](../validation/M6/delegation/R01-ra-research/disposition.md)
rechazó su Q1 por hashes fabricados y condicionó el resto a un oráculo local
(binario real, transcript real o schema real).
