# Configuración de clientes MCP

Esta guía conecta Rust Engineering MCP con clientes que admiten servidores locales
mediante `stdio`. Usa rutas absolutas y reemplaza estos valores en cada ejemplo:

- `/ruta/absoluta/rust-engineering-mcp`: binario compilado;
- `/ruta/absoluta/al/proyecto`: root que el servidor puede abrir.

El comando mínimo que inicia el servidor es:

```text
/ruta/absoluta/rust-engineering-mcp serve --stdio --root /ruta/absoluta/al/proyecto
```

No uses un shell o script intermedio que escriba en `stdout`: ese canal está
reservado para MCP. Los argumentos de runtime Docker, RustSec y catálogo se añaden
al mismo arreglo de argumentos; el [README](../README.md#habilitar-ejecución-rust)
explica cada grupo.

## Estado de compatibilidad

El soporte de configuración no equivale a una calificación completa del cliente.
La evidencia preservada del proyecto cubre:

| Cliente | Estado verificado |
| --- | --- |
| MCP Inspector 2.5.0 | M4: 27 tools, cinco positivos, cinco negativos, cinco Resource reads y cancelación Tasks; M1/M2 conservan sus recibos. [Recibo M4](validation/M4-clients.json). |
| Codex 0.153.0 stock | M4: las cinco tools pasaron por el camino síncrono de hasta 60 s y un turno model-directed usó las cinco con resultado `passed`. El cliente no declaró Tasks ni acredita cancelación Tasks. [Recibo M4](validation/M4-clients.json). |
| Claude Code 2.1.260, Sonnet 5 medium (M2) | Cliente stock restringido a MCP: 17 llamadas/resultados passed, cinco preview/commit, seis opens y receipt final committed. [Intento 5](validation/M2-clients.json), con renovación de referencias explícita en prompt v2; intentos 1–4 fallidos preservados. |
| Gemini CLI, Cursor y VS Code | Configuración derivada del soporte `stdio` oficial de cada cliente; pendiente de calificación con este servidor. |

La [matriz de compatibilidad](compatibility.md) conserva el alcance de plataforma,
protocolo y runtime. Si un cliente cambia su esquema, sigue primero su documentación
oficial y abre un issue para actualizar esta guía y la evidencia.

## Codex

Registra el servidor desde la CLI:

```bash
codex mcp add rust-engineering -- \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

Como alternativa, usa `~/.codex/config.toml` para una configuración personal o
`.codex/config.toml` dentro de un proyecto confiable:

```toml
[mcp_servers.rust_engineering]
command = "/ruta/absoluta/rust-engineering-mcp"
args = ["serve", "--stdio", "--root", "/ruta/absoluta/al/proyecto"]
startup_timeout_sec = 45
tool_timeout_sec = 300
default_tools_approval_mode = "prompt"
```

Reinicia Codex después de modificar el archivo. Ejecuta `codex mcp list` o abre
`/mcp` para comprobar que el servidor está activo. Mantén el modo de aprobación en
`prompt` porque varias tools pueden ejecutar Cargo y, con ello, `build.rs`, proc
macros o código de tests.

Referencia: [configuración MCP de Codex](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

## Claude Code

La CLI permite registrar el servidor en alcance local, de proyecto o de usuario.
Este ejemplo crea una entrada compartible de proyecto:

```bash
claude mcp add --scope project rust-engineering -- \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

La forma equivalente en `.mcp.json`, en la raíz del proyecto, es:

```json
{
  "mcpServers": {
    "rust-engineering": {
      "type": "stdio",
      "command": "/ruta/absoluta/rust-engineering-mcp",
      "args": [
        "serve",
        "--stdio",
        "--root",
        "/ruta/absoluta/al/proyecto"
      ]
    }
  }
}
```

Ejecuta `claude mcp get rust-engineering`, `claude mcp list` o abre `/mcp`. Claude
Code pide aprobar los servidores de proyecto en un workspace confiable; revisa el
comando y sus roots antes de aceptarlo.

Referencia: [servidores MCP en Claude Code](https://code.claude.com/docs/en/mcp).

## Gemini CLI

Gemini CLI puede añadir el servidor al `settings.json` del proyecto:

```bash
gemini mcp add --scope project rust-engineering \
  /ruta/absoluta/rust-engineering-mcp \
  serve -- --stdio --root /ruta/absoluta/al/proyecto
```

También puedes editar `.gemini/settings.json` en el proyecto o
`~/.gemini/settings.json` para el usuario:

```json
{
  "mcpServers": {
    "rust-engineering": {
      "command": "/ruta/absoluta/rust-engineering-mcp",
      "args": [
        "serve",
        "--stdio",
        "--root",
        "/ruta/absoluta/al/proyecto"
      ],
      "timeout": 300000,
      "trust": false
    }
  }
}
```

Ejecuta `gemini mcp list` o `/mcp list`. Los servidores `stdio` solo aparecen
conectados cuando la carpeta actual es confiable. Conserva `trust: false` para que
las tools sigan el flujo de confirmación del cliente.

Referencia: [servidores MCP en Gemini CLI](https://geminicli.com/docs/tools/mcp-server/).

## Cursor

Crea `.cursor/mcp.json` en el proyecto, o `~/.cursor/mcp.json` para todos tus
proyectos:

```json
{
  "mcpServers": {
    "rust-engineering": {
      "type": "stdio",
      "command": "/ruta/absoluta/rust-engineering-mcp",
      "args": [
        "serve",
        "--stdio",
        "--root",
        "/ruta/absoluta/al/proyecto"
      ]
    }
  }
}
```

Revisa el servidor y sus tools en **Customize > MCPs**. En Cursor Agent CLI también
puedes usar `cursor-agent mcp list` y
`cursor-agent mcp list-tools rust-engineering`.

Referencia: [Model Context Protocol en Cursor](https://cursor.com/docs/mcp).

## VS Code y GitHub Copilot

Crea `.vscode/mcp.json` en el workspace:

```json
{
  "servers": {
    "rust-engineering": {
      "type": "stdio",
      "command": "/ruta/absoluta/rust-engineering-mcp",
      "args": [
        "serve",
        "--stdio",
        "--root",
        "/ruta/absoluta/al/proyecto"
      ]
    }
  }
}
```

Ejecuta **MCP: List Servers** desde la paleta de comandos para iniciar, detener,
reiniciar o abrir el output del servidor. Para Agent Host y configuraciones que
deban funcionar fuera del extension host, consulta las ubicaciones portables
indicadas por VS Code antes de copiar el archivo.

Referencia: [configuración MCP de VS Code](https://code.visualstudio.com/docs/agents/reference/mcp-configuration).

## MCP Inspector

Inspector sirve para revisar manualmente el inventario, los esquemas y las
respuestas sin depender de la selección de tools de un modelo. Usa una versión
fijada para que la sesión sea reproducible; la calificación M1 usó `2.5.0`:

```bash
npx @modelcontextprotocol/inspector@2.5.0 \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

El comando inicia la interfaz web y lanza el servidor como subproceso `stdio`.
También existen modos CLI y TUI en la línea 2.x; consulta sus opciones antes de
automatizar una prueba, porque la interfaz del Inspector puede cambiar entre
versiones. Si el paquete no está presente, `npx` puede solicitar descargarlo; revisa
el nombre y la versión antes de autorizar esa instalación local.

Referencia: [repositorio oficial de MCP Inspector](https://github.com/modelcontextprotocol/inspector).

## Añadir ejecución, audit o catálogo

Cada cliente separa el ejecutable de su arreglo de argumentos. Para habilitar el
runtime, agrega al final de `args` el grupo completo:

```text
--docker /ruta/absoluta/al/cliente/docker
--docker-socket /ruta/absoluta/docker.sock
--state-root /ruta/absoluta/a/estado-privado
--rust-image sha256:384a1742ecc53cdd3a9c0bf36c6f8b66db73ddd118aeeae6e55654ea998ae36a
```

Agrega juntos `--rustsec-snapshot` y `--rustsec-sha256` para audit. Agrega juntos
`--catalog-store` y `--catalog-trust` para catálogo léxico. El catálogo semántico
requiere, además, un binario compilado con `--features local`,
`--catalog-model-dir` y `--catalog-index-store`. `--allow-profiling
user-space-sampling` es la única concesión de profiling y exige el grupo Docker
completo; se documenta [más abajo](#configurar-las-tools-m5).

No pongas secretos en `args` ni habilites una confianza global para evitar las
confirmaciones. Rust Engineering MCP no necesita claves API para funcionar: sus
datos, runtime y archivos de confianza son locales y los aporta el operador.

### Configurar las tools M4

Las cinco definiciones M4 llevaron el inventario del checkout a 27 tools; hoy son
31 con las cuatro de M5. Las tools M4 están
calificadas localmente con el [runtime](validation/M4-runtime.json) y los
[clientes](validation/M4-clients.json); configurar los argumentos en otra máquina
no reproduce esa calificación ni cambia la release `0.1.0`.

Usa la imagen M4 admitida como parte del grupo Docker completo:

```text
--rust-image sha256:25ed3626e710081a571a86a29521eaf2e890e796afd422ba5e409e0ce1891635
```

Añade juntos el directorio vendor Cargo y el fingerprint de su árbol:

```text
--cargo-vendor-dir /ruta/absoluta/al/vendor
--cargo-vendor-tree-sha256 sha256:<64-hex>
```

`rust.deny` requiere además la policy cerrada aportada por el host;
`rust.supply_chain.inspect` y `rust.quality.gate.v2` consumen esa policy cuando
está configurada y conservan explícitamente la ausencia de esa evidencia:

```text
--security-policy /ruta/absoluta/security-policy.json
--security-policy-sha256 sha256:<64-hex>
```

El path de policy no puede estar dentro de ninguna `--root`. El vendor tampoco
puede solaparse en ninguna dirección con una root de proyecto. El servidor exige
cada path junto con su fingerprint; una mitad ausente, un hash inválido, un path
relativo o un solapamiento hacen inválida la invocación de `serve`. Deny, supply
chain y gate v2 usan también el par RustSec ya documentado. El runtime no busca
`deny.toml` del proyecto, no acepta exceptions del cliente y no adquiere vendor,
policy, RustSec ni catálogo durante la sesión MCP.

Para que supply chain pueda completar los hechos de catálogo, configura además la
generación local autenticada:

```text
--catalog-store /ruta/absoluta/al/store
--catalog-trust /ruta/absoluta/trust.json
```

El catálogo se lee durante `serve`; import, sync y rebuild siguen siendo comandos
CLI explícitos fuera de la sesión.

Los cinco contratos aceptan `execution_mode=auto|task|synchronous`. Sin Tasks, una
selección con `timeout_seconds <= 60` puede ejecutarse por el camino síncrono
calificado; `auto` lo elige cuando cumple ese límite y `synchronous` lo exige. Los
defaults siguen siendo 120 s para deny/scanner/supply chain y 300 s para
Miri/gate v2, por lo que `auto` con esos valores devuelve `TASKS_REQUIRED` si el
peer no declaró Tasks. `task` sin negociación y `synchronous` con más de 60 s se
rechazan con `-32602`. Los máximos del contrato son 120, 1800 y 3600 s
respectivamente. Gate v2 solo admite sincronía para `profile=strict` sin mutation;
`release` y mutation requieren Tasks, y mutation además exige su presupuesto
derivado más 300 s.

Consulta el inventario compilado con
`rust-engineering-mcp security-runtime inventory --json`; es pasivo, informa
imagen, cargo-deny, helper, nightly y sysroot, y declara
`installation_observed=false`.

Mantén la aprobación interactiva del cliente. Gate v2 y Miri ejecutan código de
proyecto dentro del sandbox; `readOnlyHint` describe que la tool no escribe el
checkout, no que el código evaluado sea inocuo. La calificación cliente M4 está
limitada a Inspector 2.5.0 y Codex 0.153.0 en el host local documentado.

### Configurar las tools M5

Las cuatro definiciones M5 están implementadas y **pendientes de calificación**:
la [matriz M5](validation/M5-matrix.md) conserva M5-01..04 en `In progress`.
`tools/list` devuelve 31 definiciones —las 27 anteriores sin cambio y las cuatro
nuevas—, pero ninguna de las cuatro tiene recibo de cliente. Esta sección documenta la
configuración del host que esos contratos exigen; no acredita una calificación ni
cambia la release `0.1.0`.

#### `--allow-profiling`

`rust.profile.flamegraph` exige una capability positiva del host. Se concede con
una única opción, que acepta **un solo valor**:

```text
--allow-profiling user-space-sampling
```

Cualquier otro valor —y repetir la opción— hace inválida la invocación de
`serve`; un valor desconocido es un error de configuración, nunca una concesión
más estrecha o más amplia en silencio. `user-space-sampling` concede exactamente
el muestreo de espacio de usuario (`exclude_kernel`, `exclude_hv`, eventos
software de reloj de CPU) sobre el proceso hijo que lanza el perfilador y sus
hilos, con una sola syscall añadida al perfil seccomp y solo en la fase de
muestreo. No añade capabilities Linux, no usa contenedores privilegiados, no
ejecuta `sudo` y no toca `perf_event_paranoid`. Detalles en el
[modelo de seguridad](security-model.md#m5--medición-capability-de-profiling-y-containment).

**La opción se rechaza de plano si no configuras el runtime Docker.** Sin el
grupo `--docker` / `--docker-socket` / `--state-root` / `--rust-image` completo
no hay contenedor que contener, así que `--allow-profiling` no se degrada: el
arranque de `serve` falla. La capability es por servidor y revocable; retirarla
del arreglo `args` y reiniciar cancela el trabajo en curso, hace join del árbol
de procesos, conserva la evidencia publicada y devuelve el runtime al perfil
calificado. Ninguna otra tool cambia de comportamiento por concederla.

Sin la concesión, un cliente ve `rust.profile.flamegraph` responder `blocked` con
`PROFILING_NOT_AUTHORIZED` (ADR-076 §5), **antes** de que se cree ningún
contenedor: no hay build, no hay ejecución del binario y no hay artifact. El peer
no puede pedir la capability, ni inferirla del proyecto, ni obtenerla por una URI
de Resource o por las annotations de la tool.

#### Vendor Cargo para `rust.benchmark.run`

El harness de benchmarks es una dependencia de desarrollo del proyecto
(Criterion 0.8.2, la única integración que M5 sabe medir) y se resuelve
**offline**. Por eso `rust.benchmark.run` exige el directorio vendor autenticado
por el host, el mismo par ya documentado para las tools M4:

```text
--cargo-vendor-dir /ruta/absoluta/al/vendor
--cargo-vendor-tree-sha256 sha256:<64-hex>
```

Sin ese par, la tool **reporta que faltan datos offline** —el mismo camino
`MISSING_OFFLINE_DATA` que ya usan las tools M4 cuando el vendor autenticado no
está configurado— en lugar de degradarse: no descarga el harness, no lo sustituye
y no emite un dataset parcial. `rust.profile.flamegraph` y `rust.binary.bloat`
consumen el mismo árbol vendor para construir el binario que miden.
`rust.benchmark.compare` no lo necesita: no ejecuta nada y opera sobre dos
artifacts del store privado identificados por sus IDs opacos, que solo emite un
`run` previo del mismo proyecto.

Si el proyecto no tiene Criterion 0.8.2 vendorizado, o usa otro harness, el
resultado es declarado (`harness_unrecognized`: ejecución, exit y logs, sin
dataset ni medidas), nunca una medida degradada.

#### Imagen del runtime M5

La imagen guest M5 `rust-engineering-runtime:1.98.1-arm64-m5`
(`sha256:e0a5ca1661b3e49d0a3d68ee3cc0963453078d08eb7fc43c30538c16b7998aac`)
está construida y con [recibo](validation/M5-provisioning.json), y
[ADR-077](adr/ADR-077-m5-runtime-admission.md) añade exactamente ese digest a la
lista de admisión del gateway. El puerto de performance exige esa imagen **y solo
esa**: cualquier otro digest devuelve `unavailable` antes de crear contenedor
alguno. Un host configurado con la imagen M4 sigue sirviendo las 27 tools
anteriores y recibe `unavailable` en las cuatro nuevas, que es el resultado
correcto y declarado.

La lista de digests que `serve` acepta en `--rust-image` es una comprobación
distinta de la del gateway. Comprueba que el binario que vas a ejecutar admita el
digest M5 antes de configurarlo: un digest no admitido no degrada nada, hace
fallar el arranque. Admitir la imagen tampoco califica las tools; la calificación
nativa M5 sigue abierta.

## Diagnóstico

1. Ejecuta `rust-engineering-mcp version --json` con la misma ruta configurada.
2. Ejecuta `rust-engineering-mcp doctor --json` para revisar capacidades pasivas.
3. Comprueba el estado del servidor desde el cliente.
4. Revisa `stderr` o el panel de output; `stdout` debe contener únicamente MCP.
5. Si una tool devuelve `SANDBOX_DENIED`, revisa el grupo Docker completo y la
   [matriz de compatibilidad](compatibility.md).

La [sección de solución de problemas](../README.md#solución-de-problemas) cubre los
errores de proyecto, runtime, RustSec y catálogo.

## Escritura local M2 en desarrollo

[ADR-050](adr/ADR-050-local-coordinated-mutation.md) fija el modo
`local_coordinated`. La release `0.1.0` conserva 13 tools; el binario compilado
desde el checkout `0.3.0-dev` descubre cinco tools M2 adicionales
[calificadas localmente](validation/M2-07.md).

### Permisos y runtime

Añade únicamente los grants que quieras habilitar:

```text
--allow-manifest-write /ruta/absoluta/al/workspace
--allow-fmt-write /ruta/absoluta/al/workspace
--allow-fix-write /ruta/absoluta/al/workspace
--allow-dependency-add /ruta/absoluta/al/workspace
--allow-dependency-remove /ruta/absoluta/al/workspace
```

Cada opción se puede repetir para otras roots, hasta 16 por clase. Un permiso no
autoriza preview, commit, receipt o recovery de otra tool. La ruta debe ser la raíz
exacta que devuelve `rust.project.open`, estar dentro de un `--root` y usar el mismo
`--state-root` en todas las instancias que escriben ese workspace. El state root y
su hijo `rust-mcp-mutations-v1` deben quedar fuera de todas las roots de proyecto.
No lo cambies mientras haya una operación pendiente. No existe un updater ni
downgrade gestionado para este checkout. Antes de volver a un binario anterior,
lista y reconcilia todos los journals pendientes: `0.1.0` no conoce el formato M2
y una invocación manual antigua no comprueba ese estado.

Se exige el mismo grupo Docker completo de M1. El workspace host no se monta con
escritura en Docker. `rust.fix.apply` mantiene `network=none`, aunque su perfil
aislado permite TCP loopback dentro del namespace para la coordinación interna de
Cargo. Fix puede ejecutar build scripts y proc macros; conserva la aprobación del
cliente y revisa siempre el diff.

### Datos Cargo opcionales

Features, workspace dependencies y dependency add/remove requieren un directory
source Cargo aprobado. Prepáralo administrativamente fuera del servidor y obtén su
fingerprint:

```text
cargo vendor --locked --versioned-dirs /ruta/privada/vendor
rust-engineering-mcp cargo-vendor inspect --directory /ruta/privada/vendor --json
```

La primera orden usa el Cargo del operador y puede requerir datos preparados por
este; nunca la ejecuta una tool MCP. `inspect` no ejecuta Cargo ni descarga. Añade
juntos al arreglo `args` la ruta absoluta y el `tree_fingerprint` devuelto:

```text
--cargo-vendor-dir /ruta/privada/vendor
--cargo-vendor-tree-sha256 sha256:DIGEST
```

El directorio debe quedar separado de las project roots. Configurar solo uno de los
dos flags hace fallar el arranque. El runtime captura esos bytes, verifica el
fingerprint y no hereda `CARGO_HOME`, proxies, credenciales o configuración Cargo
del host. No instala ni descarga crates. Lints, profiles, fmt y fix no usan este
dataset. La policy `preserve_presence` actualiza Cargo.lock si ya existía y no
publica el lock transitorio cuando no existía.

Como fix no recibe ese dataset, no se promete éxito en workspaces arbitrarios con
dependencias externas; un input frozen insuficiente falla sin candidato.

### Planes, receipts y recovery

Preview no escribe. Devuelve un plan de 600 s con el diff exacto; los cinco handlers
comparten cuatro planes/64 MiB. Commit exige ese plan/digest, una idempotency key y
la autoridad vigente. Reabre el proyecto después de commit y usa el nuevo `data.project_ref` en
TODAS las llamadas posteriores, incluidos receipt y recovery. Conserva el
`operation_id` de la operación.

Si aparece `recovery_required`, conserva el journal y los temporales
`.rust-mcp-mut-*.swap`, evita edits o Git cleanup en esos archivos y consulta el
receipt con `recover: true`, usando el mismo grant y state root. Recovery no fuerza
un overwrite: bytes o inodes desconocidos mantienen el estado pendiente. Un receipt
`aborted` requiere un preview nuevo.

La CLI local lista la retención y elimina un receipt terminal concreto:

```text
rust-engineering-mcp mutation list --state-root /ruta/privada/state --json
rust-engineering-mcp mutation prune --state-root /ruta/privada/state --operation-id mut_ID --plan-digest sha256:DIGEST --json
```

Usa el ID y digest exactos del listado. Prune elimina evidencia durable y protección
de replay; consúmela y descarta el plan antes. No toca source ni ejecuta Cargo, y
rechaza registros pendientes o dudosos. Un store lleno rechaza trabajo nuevo sin
borrar evidencia; admite 128 journals/256 MiB, con 48 MiB por journal. La admisión retiene hasta 207 MiB: reserva 48 MiB para staging de recovery y 1 MiB para crecimiento de metadata. No hay retención ni eliminación automática. Un store de desarrollo ya poblado bajo
el techo anterior de 208 MiB no obtiene retroactivamente la nueva holgura.

Si recovery sigue observando bytes desconocidos o un journal parcial/corrupto,
el store compartido puede bloquear list/prune y nuevos commits, incluso de otros
workspaces. Detén todas las instancias que lo usan y conserva juntos sus workspaces,
temporales y state root. No edites journals, borres temporales, fuerces prune ni
ejecutes `git clean`. Para continuar, prepara otra copia física en una ruta distinta
con contenido cuya generación hayas revisado; no arrastres temporales reservados.
Repite esta preparación por cada workspace que deba continuar, incluidos los que
solo estaban bloqueados. Sus recibos e idempotencia quedan en el store original.
Configura un state root privado **nuevo** y grants solo para esas copias nuevas.
Nunca conectes el store nuevo a las roots originales aún en cuarentena. Reabre y
solicita previews nuevos. Se restaura trabajo en la copia; no se repara el journal
original ni se traslada su idempotencia. Los originales quedan preservados para
reconciliación manual. [Contrato y límite](adr/ADR-052-mutation-journal-and-authorization.md).

## Calidad M3 y artifacts persistentes

El mismo `--state-root` hospeda también `rust-mcp-quality-artifacts-v1`. El store
usa TTL predeterminado de 1 h y cuotas de 32 MiB por artifact, 64 MiB por job,
128 MiB por owner y 256 MiB global. La calificación positiva está limitada a
macOS ARM64/APFS; Linux y Windows fallan cerrados. La CLI
`quality-artifacts recover|prune` está integrada en `main.rs` y disponible para el
operador local; ambos subcomandos exigen `--state-root` absoluto y aceptan `--json`.
[Recibo de upgrade/rollback](validation/M3-06-rollback.md).

Las cuatro tools de calidad M3 aceptan `execution_mode=auto|task|synchronous`.
`TASKS_ADVERTISEMENT_READY` está activado tras la puerta G4. Un peer que declare
`io.modelcontextprotocol/tasks` obtiene un `CreateTaskResult`, conserva el
`taskId`, consulta `tasks/get` y puede solicitar `tasks/cancel`. La cancelación es
cooperativa: el estado no pasa a `cancelled` hasta que el gateway haya unido y
limpiado el árbol de procesos. `tasks/update` no está admitido para jobs M3.

Sin declaración, solo una selección calificada con `timeout_seconds <= 60` puede
ejecutarse síncronamente; `auto` largo devuelve `TASKS_REQUIRED` antes de iniciar
trabajo y `task` devuelve `-32602`. Inspector 2.5.0 declaró Tasks en la evidencia;
Codex CLI/app-server 0.153.0 no la declaró y fue calificado por su fallback
síncrono. No agregues manualmente la extensión como workaround para un cliente:
la declaración tiene que venir del peer. [Estado y matriz](validation/M3-02.md).

Stage 1 define Resources con URI `rust-quality-artifact://`. El índice de un job
se lee con su cursor y los artifacts se leen por chunks (`offset` y `length`) desde
el path fijo del store; `resources/list` no enumera objetos. Esta lectura Stage 1
no autoriza trabajo, reparación ni renovación del TTL. Un miembro expirado se
devuelve como no encontrado y su proyección posterior en `tasks/get` pasa a
`unavailable` sin reescribir el resultado histórico del job.

Si el store necesita recuperación, detén las instancias que lo usan y conserva la
raíz original. Ejecuta `quality-artifacts recover` para obtener un reporte;
`quality-artifacts prune` reclama únicamente objetos y reservas ya expirados y
nunca desaloja evidencia viva ni pone nada en quarantine. Una regresión del reloj
durable bloquea `prune` con `recovery_required` hasta que `recover` re-base el
watermark. No edites archivos, borres objetos desconocidos ni reutilices M2
journals. Los objetos dudosos se ponen en quarantine y un store lleno rechaza
trabajo nuevo. Un registro con `format_version` desconocido (escrito por un binario
más nuevo) se rechaza cerrado y se conserva; este binario nunca lo reinterpreta ni
lo borra. [Recibo](validation/M3-06-rollback.md).

El límite de cuatro planes aplica a propuestas pendientes: los planes terminales
dejan capacidad para nuevas propuestas en la siguiente admisión. Un commit con
plan ausente/expirado solo puede repetir un journal existente con ID, digest y key
exactos, bajo grant vivo e identidad física original. No inicia efectos nuevos sin
preview vigente. Prune retira ese replay; un receipt terminal describe historia,
no el source actual. Véase [ADR-059](adr/ADR-059-terminal-plan-retirement-and-durable-replay.md).
