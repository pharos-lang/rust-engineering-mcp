# M8-06 — Reproducción de las guías públicas por un tercero

Rol: tercero que instala, configura y opera el producto siguiendo únicamente
`README.md`, `docs/tools.md`, `docs/client-configuration.md`,
`docs/security-model.md`, `SECURITY.md`, `docs/compatibility.md`,
`docs/publication.md`, `CHANGELOG.md` y la salida de `--help`. No se leyó
`crates/`, `scripts/`, `docs/validation/` (salvo este documento, escrito al
final), `docs/adr/` ni `docs/roadmap/`. Directorio de trabajo:
`target/m8-third-party/`. Modelo: Claude Sonnet 5, `--effort high`.

## Limitaciones del entorno de esta sesión (no imputables a la documentación)

Dos restricciones del sandbox de esta sesión concreta impidieron completar
algunos pasos del recorrido tal como un tercero real los ejecutaría. Ninguna
de las dos aparece en la documentación pública ni es una instrucción del
README: son artefactos de cómo esta sesión concreta puede invocar comandos.
Se documentan aquí por honestidad, y las filas de la tabla afectadas se
marcan `no verificable (entorno de sesión)` en vez de fingir un resultado.

1. **CLI externas interactivas bloqueadas.** `codex mcp add`, `codex mcp
   list`, `claude mcp add`, `git checkout` y la ejecución de scripts (`bash
   archivo.sh`) requieren una aprobación interactiva que esta sesión no
   puede conceder (no hay usuario disponible para aprobar el diálogo). Para
   la configuración de clientes se usó la alternativa de archivo, también
   documentada (`~/.codex/config.toml` / `.mcp.json`), que sí se pudo
   escribir y revisar textualmente contra los ejemplos de
   `docs/client-configuration.md`; no se pudo lanzar el cliente real para
   confirmar `codex mcp list` / `/mcp` en vivo.
2. **El patrón de tubería de una sola pasada cancela llamadas que usan el
   worker asíncrono.** El allowlist de esta sesión exige invocar el binario
   exactamente como `target/release/rust-engineering-mcp <subcomando> …` o,
   para JSON-RPC, `printf … | target/release/rust-engineering-mcp serve
   --stdio --root …` (sin mantener el proceso vivo entre mensajes: no hay
   forma aprobada en esta sesión de abrir un FIFO interactivo, iniciar el
   servidor en segundo plano o ejecutar un script envolvente). Con ese
   patrón, el EOF de stdin llega inmediatamente después del último byte
   escrito por `printf`, antes de que una llamada que requiere el worker
   (p. ej. `rust.project.open`) complete su despacho: la respuesta observada
   es siempre `status:"cancelled"`, `duration_ms:0`, reproducible de forma
   determinista en cuatro intentos independientes (con y sin `tools/list` en
   la misma tubería). Ningún documento público instruye a un tercero a
   probar el servidor con una tubería de una sola pasada — un cliente MCP
   real (Claude Code, Codex, Inspector) mantiene el proceso vivo durante toda
   la sesión — así que esto no se cuenta como desviación de la
   documentación. Sí limita qué se pudo verificar end-to-end por stdio en
   esta sesión: el handshake `initialize` y `tools/list` se completaron y
   coinciden con lo documentado; las llamadas a tools que ejecutan trabajo
   real (`rust.project.open`, `rust.project.inspect`, etc.) no se pudieron
   observar completas por esta vía y se verificaron en su lugar mediante los
   subcomandos CLI equivalentes cuando existían (`doctor`, `contract`,
   `mutation list`, `security-runtime inventory`).

Nota operativa adicional (no es un hallazgo, es transparencia sobre el
método): siguiendo la instrucción de crear el fixture con `cargo init` desde
dentro del repositorio, `cargo init --name m8_fixture --bin
target/m8-third-party/fixture` añadió automáticamente
`"target/m8-third-party/fixture"` como miembro del `[workspace]` del
`Cargo.toml` raíz (comportamiento estándar de Cargo al inicializar un
paquete dentro del árbol de un workspace existente sin `exclude`). Se
revirtió de inmediato con el Edit tool y se reescribió el `Cargo.toml` del
fixture como workspace independiente (`[workspace]` vacío) para que no
volviera a ocurrir. `git diff Cargo.toml` confirma que el archivo raíz quedó
idéntico al estado inicial. Esto no es un defecto de la documentación
pública — ésta nunca instruye a ejecutar `cargo init` dentro del repositorio
— y no se incluye en la tabla de hallazgos.

Durante la sesión también se observaron cambios no atribuibles a este
tercero en archivos ya modificados al inicio de la conversación
(`crates/mcp-server/src/host_config.rs`, `crates/mcp-server/tests/cli.rs`,
`docs/adr/ADR-088-*.md`, `docs/adr/ADR-089-*.md`, `docs/security-model.md`,
`docs/validation/M8/08-threat-model.md`), consistentes con los paquetes de
trabajo `W31-runtime-mode-fix`, `W32-v04-docs` y `W33-v04-hardening` ya
presentes como directorios sin seguimiento al inicio de esta conversación.
No se tocaron ni revirtieron: pertenecen a otro trabajo en curso sobre el
mismo árbol.

## Recorrido

| # | Documento §sección | Comando | Exit | Esperado según la guía | Observado | ok/desviación |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | README §Instalar la release macOS ARM64 | `shasum -a 256 -c SHA256SUMS` / descarga `v0.3.0` | — | Archive y `SHA256SUMS` descargables desde GitHub Releases | No existe release `v0.3.0` ni `v0.8.0` publicada; tampoco existe `target/m8-release/` con un ensayo local para usar como sustituto | **Desviación — F-1 (P1)** |
| 2 | README §Compilar desde el código fuente | `cargo build --release --locked -p rust-engineering-mcp` | 0 | Binario en `target/release/rust-engineering-mcp` | Compilación exitosa (ya estaba actualizado, "Finished" inmediato); binario presente | ok |
| 3 | README §Compilar desde el código fuente | `target/release/rust-engineering-mcp version --json` | 0 | `format_version`, `package`, `version`, `compiled_local`, `target_os`, `target_arch` | `{"format_version":1,"operation":"version","package":"rust-engineering-mcp","version":"0.8.0","compiled_local":false,"target_os":"macos","target_arch":"aarch64"}` — coincide con "checkout de desarrollo, versión 0.8.0" del README | ok |
| 4 | README §Compilar desde el código fuente | `target/release/rust-engineering-mcp doctor --json` | 0 | `warning` con capacidades opcionales `not_configured`, nunca instala/repara | 14 checks, todos `not_configured`/`not_checked`/`not_used`/`available` según corresponda, `status:"warning"`, `catalog`/`runtime`/`mutation_journals` en `unavailable`/`null` | ok |
| 5 | tools.md §CLI y doctor M1-14 (D-5) | `doctor --state-root <dir vacío nunca usado> --json` | 0 | `mutation_journals` no nulo solo con `--state-root`; `pending`/`terminal`/`unknown_format` en 0 para un store vacío | `"mutation_journals":{"pending":0,"terminal":0,"unknown_format":0,"kinds":{},"downgrade_blocked":false,"downgrade_blocking_kinds":[],"notes":[]}` | ok |
| 6 | client-configuration.md §Planes, receipts y recovery | `mutation list --state-root <mismo tipo de dir vacío nunca usado> --json` | 1 | Listado (vacío) de journals retenidos, "misma lectura" que `doctor --state-root` según D-5 | `{"format_version":1,"status":"blocked","action":"list","error_code":"io","message":"Journal administration did not complete; preserve pending evidence and use authorized recovery for interrupted operations","records":[]}` sobre un directorio jamás tocado, sin journal ni interrupción previa; reproducido dos veces con directorios distintos | **Desviación — F-2 (P2)** |
| 7 | tools.md §CLI y doctor M1-14 / spec §56 | `contract --json` | 0 | `document_kind:"rust_engineering_capabilities"`, `format_version:1`, `server_version`, `tool_count`, 31 `stable` + 5 `preview` | Coincide exactamente: `server_version:"0.8.0"`, `tool_count:36`, `primary_version:"2026-07-28"`, 5× `"stability":"preview"`, 31× `"stability":"stable"` | ok |
| 8 | client-configuration.md §Configurar las tools M4 | `security-runtime inventory --json` | 0 | Imagen, cargo-deny, nightly, sysroot; `installation_observed:false` | `image_id` = `sha256:25ed3626e71…` (coincide con la imagen M4 citada en README/security-model.md), `nightly:"nightly-2026-09-07"`, `installation_observed:false` | ok |
| 9 | security-model.md §Detección activa M0-06 | `capabilities --json` (sin flags obligatorios) | 2 | "uso inválido es exit2" | Exit 2, stderr fijo: `Unsupported invocation. Use 'rust-engineering-mcp --help'.` | ok |
| 10 | README §Habilitar ejecución Rust | `doctor --active --json` (sin grupo Docker) | 0 | `--active` calibra solo si hay runtime configurado; sin él, se mantiene `not_configured` | `mode:"active"`, mismos 14 checks `not_configured`, `runtime:null` — no intenta calibrar sin configuración | ok |
| 11 | README §Uso general / `--help` | `--help` | 0 | Lista de comandos y de las 36 tools | Coincide con el inventario de `contract --json`; cabecera propia no documentada (ver F-3) | ok (con nota F-3) |
| 12 | client-configuration.md §Codex | Escritura manual de `config.toml` en `CODEX_HOME` temporal (alternativa documentada a `codex mcp add`) | — | Formato TOML con `command`/`args`/`startup_timeout_sec`/`tool_timeout_sec`/`default_tools_approval_mode` | Archivo escrito y verificado carácter a carácter contra el ejemplo del documento; `codex mcp add`/`codex mcp list` no ejecutables en esta sesión (ver limitaciones) | no verificable en vivo (entorno de sesión) |
| 13 | client-configuration.md §Claude Code | `claude mcp add --scope project …` | — | Registro vía CLI o `.mcp.json` | No ejecutable en esta sesión (CLI interactiva requiere aprobación no disponible) | no verificable en vivo (entorno de sesión) |
| 14 | README §Iniciar el servidor / compatibility.md §Matriz wire | `printf '<initialize>' '<initialized>' '<project.open>' | serve --stdio --root <fixture>` | — | `initialize` responde `protocolVersion`/`serverInfo` según la versión negociada | `{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18","capabilities":{"extensions":{"io.modelcontextprotocol/tasks":{}},"resources":{},"tools":{}},"serverInfo":{"name":"rust-engineering-mcp","version":"0.8.0"}}}` — versión preservada, tal como documenta la matriz wire | ok |
| 15 | README §Funcionalidades / tools.md §rust.project.open | Mismo `tools/call rust.project.open` de la fila 14, mismo proceso | — | `status:"passed"`, `project_ref`, `workspace_root`, `fingerprint` | `status:"cancelled"`, `duration_ms:0/1`, `summary:"Project registration cancelled"` — atribuible al cierre de stdin de la tubería de una sola pasada, no reproducible como fallo del producto en un cliente real | no verificable end-to-end (entorno de sesión, ver nota) |
| 16 | tools.md §rust.project.inspect | `rust.project.inspect` con el `project_ref` de la fila 15 | — | `blocked`/`unavailable` sin runtime Docker configurado | No ejecutable: depende de un `project_ref` vivo que la fila 15 no llegó a emitir en esta sesión | no verificable (depende de #15) |
| 17 | SECURITY.md §Reportar un problema | Lectura únicamente | — | Procedimiento accionable (GitHub private vulnerability reporting, versión/commit/plataforma/pasos/impacto, no incidencia pública antes de coordinar) | Texto claro, autocontenido y accionable; no se pudo verificar la alcanzabilidad de la URL de GitHub (sesión sin red de salida verificada) | ok (contenido); red no verificada |
| 18 | docs/publication.md §Incident response | Lectura únicamente | — | Credencial de publicación, plan ante asset comprometido, separación de la firma del catálogo | Texto internamente coherente con SECURITY.md y README §Seguridad | ok |

## Hallazgos

- **F-1 (P1).** El paso «Instalar la release macOS ARM64» del README no se
  puede completar por un tercero tal como está escrito: no existe una
  release publicada `v0.3.0` (ni `v0.8.0`, que además el propio README
  aclara que "sin tag ni release todavía") en el momento de esta
  reproducción, y no hay un archive de ensayo local en `target/m8-release/`
  que sirva de sustituto. La única vía practicable es la alternativa
  «Compilar desde el código fuente», que si funcionó (fila 2–4). El README
  no señala explícitamente que la vía de instalación primaria pueda estar
  indisponible ni ofrece un aviso de "si no hay release, usa esta otra
  sección" — un tercero sin acceso a este repositorio de desarrollo no tiene
  cómo saber por adelantado que debe saltar directamente a "Compilar desde
  el código fuente".
- **F-2 (P2).** `rust-engineering-mcp mutation list --state-root <ruta
  nunca usada> --json` devuelve `status:"blocked"`, `error_code:"io"` y un
  mensaje de recuperación de interrupción ("preserve pending evidence and
  use authorized recovery for interrupted operations") sobre un directorio
  vacío que jamás tuvo actividad, en dos directorios distintos probados de
  forma independiente (ambos exit 1). Esto contradice tanto la expectativa
  natural de listar un store vacío sin error como el propio texto de
  `tools.md` §CLI y doctor M1-14, que describe `doctor --state-root` (sin el
  resto de la tupla Docker) como "la misma lectura mínima que `mutation
  list --state-root` exige" — y `doctor --state-root` sobre el mismo tipo de
  directorio (fila 5) sí reporta un store limpio (`pending:0, terminal:0,
  unknown_format:0`) sin ningún error. La guía de recovery
  (`client-configuration.md#planes-receipts-y-recovery`) documenta
  `mutation list` como la forma normal de "listar la retención", no como una
  operación que pueda fallar sobre un store nunca inicializado.
- **F-3 (P3).** `CHANGELOG.md`, sección `0.8.0`, entrada "M6-04/M6-05:
  `rust.analyzer.actions` y `rust.analyzer.action.apply`" cierra con
  "Calificación nativa pendiente del orquestador." Ese texto queda
  desactualizado frente a `docs/security-model.md` §M6 — analyzer, que
  documenta que "los doce cortes nativos M6 quedaron verdes en el gate
  `full` de cierre M6 ([matriz M6]; corrección M8-08)" y que el corte
  `m6-10-diagnostics-build-script-oracle` "está verde en el gate `full` de
  cierre M6 (matriz M6; corrección M8-08)" — es decir, la propia
  documentación registra en otro lugar que la calificación nativa de M6 ya
  se cerró, sin que la entrada del CHANGELOG que originalmente anunció esas
  dos tools como pendientes se haya actualizado o anotado con la corrección
  posterior. Es exactamente el tipo de "texto histórico que contradiga el
  estado actual" que este recorrido busca detectar.
- **F-4 (P3).** La salida de `--help` se autodescribe como "Rust Engineering
  MCP — development server". Ningún documento público citado (README,
  `docs/tools.md`, `docs/client-configuration.md`, `docs/compatibility.md`)
  usa esa frase ni explica el literal "development server"; el README solo
  distingue entre la release soportada `0.3.0` y el "checkout de desarrollo"
  `0.8.0`, sin conectar esa distinción con el texto exacto que ve un
  operador en el propio binario.

## Veredicto

**Reproducible por tercero: no de forma completa — parcialmente, con 4
desviaciones documentadas** (1× P1, 1× P2, 2× P3), más las dos limitaciones
del entorno de esta sesión concreta descritas arriba (no contadas como
desviaciones de la documentación, ya que ningún documento público instruye
a un tercero a operar de esa forma). La instalación tal como la describe el
README §Instalar la release macOS ARM64 no fue reproducible (F-1); la ruta
alternativa documentada de compilación desde fuente sí lo fue íntegramente.
La configuración pasiva (`version`, `doctor`, `contract`,
`security-runtime inventory`, `capabilities` sin flags) y el handshake MCP
por stdio coincidieron con lo documentado en todos los casos probados. El
procedimiento de recovery de journals de mutación mostró una inconsistencia
reproducible entre `doctor --state-root` y `mutation list --state-root`
sobre el mismo tipo de directorio vacío (F-2). No se detectaron enlaces
rotos ni comandos inexistentes frente al `--help` real del binario; sí se
detectó un texto histórico desactualizado en `CHANGELOG.md` (F-3) y una
cabecera de `--help` no explicada por la documentación pública (F-4).
