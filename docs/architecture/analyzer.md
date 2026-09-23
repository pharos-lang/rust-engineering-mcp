# Diagnósticos, Miri, coverage/SemVer y rust-analyzer

Este capítulo agrupa las capacidades de "evidencia derivada del compilador y
de herramientas de análisis" que no son ejecución directa de check/clippy/
test: explicaciones de códigos de error, integridad de clasificación de
Miri, coverage/SemVer como extensiones del mismo gateway, y la integración
completa de rust-analyzer (M6) con sus cinco tools `preview`.

## Explicaciones respaldadas por el compilador

Decisiones: ADR-039.

**Decisión.** `rust.diagnostics.explain` **no tiene `project_ref`** —
deliberadamente, porque no necesita ningún workspace concreto. Solo acepta
un código `E####` validado por formato; ejecuta el comando fijo `rustc
--explain <code>` con un `SourceBundle` vacío bajo el mismo sandbox
aprobado (sin autoridad de filesystem sobre ningún proyecto real, sin
Resources). Un código bien formado pero ausente en ese compilador concreto
es `unavailable` — **nunca fabricado** por el producto a partir de
conocimiento general.

El vocabulario de "código desconocido" está pinneado a la versión exacta
del compilador aprobado (verificado por un test dedicado, `E9999`, un
código garantizado inexistente) — cualquier upgrade de la imagen del
gateway debe re-ejecutar ese test o arriesga clasificar mal un código
disponible como error de infraestructura, o viceversa.

**Estado actual.** Vigente; evidencia
`crates/mcp-server/src/stdio/explaining.rs`,
`crates/execution-adapter/src/project_inspection.rs` (manejo de `E9999`);
test `tests/inspection_runtime/explain.rs` (ejercita tanto `E0502` real
como `E9999`).

## Coverage y SemVer como extensiones del mismo gateway

Decisiones: ADR-062.

**Decisión.** `rust.coverage` y `rust.semver.check` **extienden** el
`RustGateway`/`RustCommand` único (ver
[`execution-and-security.md`](execution-and-security.md)) — no crean un
segundo gateway. Coverage usa una captura de **dos fases**
(`--no-report` + `report`, nunca tres corridas independientes) para que
JSON, LCOV y HTML compartan un único profdata subyacente, evitando
divergencias entre formatos. "Cero datos no es 100%": un scope con
denominador 0 en sus métricas queda **ausente** del reporte, nunca se
reporta como `0%` ni como `100%` — cualquiera de esas dos cifras sería una
afirmación falsa sobre código nunca ejercitado. El JSON completo es
**solo artifact** (nunca inline, con un presupuesto de 512 KiB).

SemVer solo usa `--baseline-root` (**nunca** resolución vía red o Git) y
exige una extensión genuina del gateway: un segundo volumen read-only más
nuevas variantes de fase, no una reutilización superficial. El reporte
HTML/coverage se empaqueta como un `ArchiveBundle` acotado (decisión
conjunta con ADR-061, ver [`jobs-and-artifacts.md`](jobs-and-artifacts.md))
— nunca se previsualiza ni se extrae del lado del servidor.

**Limitación.** `fail_under` (un umbral de coverage que falle la tool) y la
cobertura de doctests permanecen **sin decidir** — no están implementados
ni rechazados, simplemente no se abordaron. El techo de 8 MiB para
HTML/LCOV está solo levemente medido; no es un claim de capacidad
garantizada para workspaces reales grandes.

**Estado actual.** Vigente; evidencia `crates/domain/src/coverage.rs`,
`crates/execution-adapter/src/{coverage_port,coverage_json,
semver_gateway}.rs`; test
`crates/domain/tests/coverage.rs`
(`zero_denominator_has_no_percent_bearing_metric`),
`tests/inspection_runtime/semver.rs`.

## Integridad de clasificación de Miri

Decisiones: ADR-072.

**Decisión.** El modo de "integridad de clasificación" es
**deliberadamente acotado**: antes de correr Miri, la metadata ya capturada
del proyecto debe **probar** la ausencia de proc-macros, build scripts y
harnesses de test personalizados; si no puede probarlo, el resultado es
`classification_integrity_unsupported` — **nunca** afirma limpio, UB o
no-soportado sin esa prueba previa. La clasificación final se deriva
**solo** de la terminación del gateway, la estructura JUnit, el código de
salida del runner y los diagnósticos JSON propios de Miri — **nunca** del
stdout/stderr del programa interpretado, que un binario hostil podría
forjar. Nightly, sysroot y wrapper quedan fijos precisamente para que el
binario interpretado no pueda fabricar diagnósticos que se confundan con
los reales. Cualquier `.cargo/config`, config de nextest o
`rust-toolchain` presente en cualquier lugar del bundle capturado se
rechaza **antes** de crear ningún recurso Docker. Solo `--tests` — nunca
doctests, benches ni examples.

Un run limpio certifica **solo** los tests y la configuración
seleccionados — nunca la ausencia universal de UB en el código. El
aislamiento de Miri **no sustituye** al Execution Gateway a nivel de
sistema operativo; son dos capas independientes.

**Verificación contra evidencia real.**
[`docs/validation/M4/miri-native.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M4/miri-native.json) reporta `status: passed`
sobre la imagen M4 nativa, con casos de clasificación y de
admisión/lifecycle explícitos; [`docs/validation/M4/full-gate.json`](https://github.com/pharos-lang/rust-engineering-mcp/blob/51fa602e/docs/validation/M4/full-gate.json)
(`mode: full`, `status: passed`, 33/33 pasos, corrida
2026-09-08T16:47–17:41Z) incluye el paso `m4-runtime` que ejercita Miri
junto al resto del gate M4. La confirmación independiente final que el
propio ADR-072 marcaba como pendiente en su título queda respaldada por
este gate completo.

**Estado actual.** Vigente; evidencia
`crates/domain/src/miri.rs`,
`crates/execution-adapter/src/{miri_port,miri_output,miri_log,
miri_admission,miri_native}.rs`.

## Contrato de tools del analyzer

Decisiones: ADR-083.

**Decisión.** Exactamente **5 tools nuevas**: `rust.analyzer.symbols`,
`.references`, `.diagnostics`, `.actions` (las cuatro de solo lectura) y
`.action.apply` (de escritura) — inventario 31→36. **Hover, go-to-definition
y rename quedan explícitamente Deferred**, nunca un campo opcional oculto
detrás de otra tool.

`action.apply` **reutiliza el writer M2 verbatim**
(`MutationCandidate{kind: AnalyzerActionApply}`, ver
[`mutation.md`](mutation.md#los-cinco-tools-de-escritura-m2-y-el-sexto-reutilizado-por-el-analyzer))
— sin un segundo lock, journal o staging de filesystem propio; comparte el
mismo journal, replay, receipt y recovery de ADR-050/052. La aceptación de
un `WorkspaceEdit` producido por rust-analyzer es un **allow-list
estricto**: solo `TextEdit`s planos sobre archivos `.rs` ya capturados, sin
rangos superpuestos, dentro de los caps de edición y de bytes.
`Command`, `SnippetTextEdit`, `ResourceOperation`, cualquier URI fuera de
`file:///source/` o un desajuste de versión de documento se **rechazan sin
fallback parcial** — no se aplica "lo que sí se entiende" de una edición
mixta.

Una entrada de journal preexistente con un `kind` desconocido (por ejemplo
escrita por un binario más nuevo o más viejo que reconoce un kind que este
binario no reconoce) debe rechazarse **antes de cualquier efecto**, nunca
interpretarse con un default silencioso — test obligatorio G6.

Las cinco tools quedan clasificadas `preview` (ADR-086, ver
[`reference/compatibility.md`](../reference/compatibility.md)), no
`stable` — ese dato de estabilidad proviene de una ADR de milestone
posterior, no del propio ADR-083.

**Estado actual.** Vigente; evidencia `crates/domain/src/analyzer.rs`,
`crates/mcp-server/src/stdio/analyzer.rs` y su subdirectorio,
`crates/mcp-server/src/stdio/mutation/analyzer_action.rs`; tests
`crates/mcp-server/tests/analyzer_runtime.rs`; snapshots
`analyzer-{symbols,references,diagnostics,actions,action-apply}-tool.json`.

## Runtime de rust-analyzer y ciclo de vida LSP

Decisiones: ADR-084.

**Decisión.** Una instancia **transitoria** de rust-analyzer por consulta —
sin pool de procesos reutilizados — dentro de una nueva `Phase::Analyzer`
del Execution Gateway único (ver
[`execution-and-security.md`](execution-and-security.md)); **nunca** un
`Command` creado fuera de él. Se añade una sesión dúplex nueva en el
supervisor del execution-adapter (escribir frames LSP mientras se lee
concurrentemente la respuesta) — explícitamente **no** un segundo stack
MCP/JSON-RPC paralelo, solo el transporte de frames del protocolo LSP hacia
un proceso hijo contenido.

`initializationOptions` son fijas (17 claves, tras la enmienda de
calibración) con un `config_digest` validado contra el propio
`--print-config-schema` del binario rust-analyzer instalado en la imagen —
si el binario cambia su schema, la discrepancia se detecta antes de
confiar en su configuración. El oráculo de disponibilidad es la
notificación `experimental/serverStatus` con `quiescent: true` (timeout
60 s). Los diagnósticos son **solo pull** vía `textDocument/diagnostic`;
`publishDiagnostics` (push) se descarta. Los **diagnósticos permanecen
syntax-only** — la Opción A del ADR fue la retenida; la Opción B
(diagnósticos semánticos) fue rechazada explícitamente por producir falsos
positivos alrededor de macros de la std. `rust-analyzer.toml` y
`.rust-analyzer.toml` se rechazan en la captura — podrían reactivar
build-scripts o proc-macros silenciosamente si se respetaran. Hay una
allow-list estricta de procesos hijo esperados durante `initialize`.
Presupuestos fijos: 1 instancia de rust-analyzer concurrente, cap de frame
LSP de 1 MiB, 4096 mensajes por job, 512 símbolos/referencias visibles, 32
acciones/128 edits, timeout de init 60 s, de request 30 s, total 180 s. El
alcance positivo nativo está restringido a macOS ARM64/APFS con guest Linux
ARM64 M6 — Linux/Windows nativos fallan cerrados pendiente de un
subprograma futuro (D13, ver
[`execution-and-security.md`](execution-and-security.md#frontera-de-distribución-por-qué-vigente-casi-siempre-significa-macos-arm64)).

**Limitación explícita del propio ADR.** Cualquier valor numérico o de
configuración que no esté ligado a un artefacto de calibración citado
(`01.md`, W04, W12 en [`docs/validation/M6/`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation/M6)) debe tratarse como **no
confirmado por evidencia nativa** — no asumir que todo número en este
documento tiene el mismo respaldo empírico.

**Estado actual.** Vigente, como enmendado (tres enmiendas fechadas
incorporadas al mismo documento). Evidencia:
`crates/execution-adapter/src/analyzer_gateway.rs` (digest exacto de
`config_digest`), `analyzer_native.rs`; [`docs/validation/M6/{01-calibration,01-config-schema,01.md}`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation/M6).

## Documentación local y remota: sin tool dedicada

**Limitación (spec §33.1/§33.2, sin ADR propia).** La especificación
original pedía dos capacidades de consulta de documentación separadas de la
integración con rust-analyzer: una tool de **documentación local**
(rustdoc del proyecto, metadata, source, documentación de la std),
priorizada por defecto, y una tool de **documentación remota** (docs.rs/
crates.io), marcada `openWorld` y completamente deshabilitable. **Ninguna
de las dos existe** como tool del producto. `rust.crate.inspect` y
`rust.crate.search` (ver [`catalog-and-search.md`](catalog-and-search.md))
cubren metadata del catálogo — versión, licencia, features, advisories —
pero no contenido de documentación en sí (no renderizan ni indexan rustdoc
ni páginas de docs.rs). Las cinco tools `rust.analyzer.*` de este capítulo
tampoco cubren este hueco: exponen símbolos, referencias, diagnósticos y
acciones del código fuente vía LSP, no documentación renderizada. Esta
ausencia es una limitación real, no una implementación parcial oculta —
no existe código, ADR ni test que la respalde.
