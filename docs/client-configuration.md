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
| MCP Inspector 2.5.0 | M1: UI con trece tools, sin calificación de Resource read. M2: CLI con 18 tools, snapshots M1, open positivo y cinco denegaciones sin grants; exit 5 esperado en estas últimas. [Recibo M2](validation/M2-clients.json). |
| Codex 0.153.0 (M1) | APIs directas: tools, Resource e inventario. Un flujo model-directed posterior pasó; los dos intentos anteriores fallidos se conservan. [Recibo del flujo](validation/M1-17-codex-model.md). |
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
--rust-image sha256:8fac70723a8d04b6ec9633ab721806b8a55f4f083a1b3f988c61bf6a00fa1909
```

Agrega juntos `--rustsec-snapshot` y `--rustsec-sha256` para audit. Agrega juntos
`--catalog-store` y `--catalog-trust` para catálogo léxico. El catálogo semántico
requiere, además, un binario compilado con `--features local`,
`--catalog-model-dir` y `--catalog-index-store`.

No pongas secretos en `args` ni habilites una confianza global para evitar las
confirmaciones. Rust Engineering MCP no necesita claves API para funcionar: sus
datos, runtime y archivos de confianza son locales y los aporta el operador.

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
desde el checkout `0.2.0-dev` descubre cinco tools M2 adicionales
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

El límite de cuatro planes aplica a propuestas pendientes: los planes terminales
dejan capacidad para nuevas propuestas en la siguiente admisión. Un commit con
plan ausente/expirado solo puede repetir un journal existente con ID, digest y key
exactos, bajo grant vivo e identidad física original. No inicia efectos nuevos sin
preview vigente. Prune retira ese replay; un receipt terminal describe historia,
no el source actual. Véase [ADR-059](adr/ADR-059-terminal-plan-retirement-and-durable-replay.md).
