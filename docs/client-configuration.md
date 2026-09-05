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
| MCP Inspector 2.5.0 | Descubrimiento y llamadas exitosas a las 13 tools mediante la UI persistente. Resource read no quedó calificado. |
| Codex 0.153.0 | APIs directas del cliente: llamadas de tools, lectura de Resource e inventario estable. Dos intentos autónomos del modelo no produjeron una llamada y no acreditan ese flujo. |
| Claude Code, Gemini CLI, Cursor y VS Code | Configuración derivada del soporte `stdio` oficial de cada cliente; pendiente de calificación con este servidor. |

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

[ADR-050](adr/ADR-050-local-coordinated-mutation.md) fija edición local
coordinada para M2: permiso de escritura del host configurado una vez, preview/diff,
precondiciones, locks entre instancias MCP, journal y recuperación conservadora.
No exige servicios privilegiados, sudo, cuentas ni cambios de ownership del proyecto.
El host mantiene estables las roots y evita editar simultáneamente los archivos
afectados durante el commit breve. El MCP no bloquea al IDE ni promete CAS o atomicidad
visible multiarchivo. Conflictos observados detienen la operación; los posteriores
pueden requerir recuperación conservando evidencia y sin pisar bytes desconocidos.
La rama de desarrollo incorpora `rust.manifest.patch` para lints del `Cargo.toml`
raíz e integra `rust.fmt.apply` para archivos Rust existentes. Ambos usan
preview/commit/receipt, con grants separados `--allow-manifest-write WORKSPACE_ROOT`
y `--allow-fmt-write WORKSPACE_ROOT`. La calificación conjunta sigue en curso;
estas capacidades no forman parte de la release `0.1.0`. Las trece tools M1 y su
sandbox se conservan. Fix, dependencias y las demás familias de patch siguen
pendientes; el tablero enlaza la evidencia sin anunciar M2 terminado.

### Permisos y retención M2

En el binario compilado desde esta rama, añadir una sola opción a la configuración
existente de Cargo: `--allow-manifest-write /ruta/absoluta/al/workspace` para lints
o `--allow-fmt-write /ruta/absoluta/al/workspace` para rustfmt. Conceder solo las
operaciones deseadas; un permiso no autoriza receipts ni commits de la otra tool.
La ruta debe ser la raíz exacta que devuelve `rust.project.open`, estar incluida
en un `--root`, y usar el mismo `--state-root` en todas las instancias que escriben
ese workspace. No cambiar state root mientras haya una operación pendiente.
Se exige la configuración Docker aprobada de M1 para validar candidatos sin red.
No se monta el workspace del host con escritura en Docker.

El hijo privado `rust-mcp-mutations-v1` de `--state-root` se crea al usar la
capacidad; no precisa otra ruta ni servicio. El state root debe estar fuera de
todas las roots de lectura. Sin grant, la tool devuelve `permission_denied`.
Tras commit, reabrir el proyecto y consultar el operation ID con la nueva referencia.
Si aparece `recovery_required`, conservar journal y temporales `.rust-mcp-mut-*.swap`,
evitar edits/Git cleanup en esos archivos y consultar con `recover: true` usando el
mismo grant y state root. Un estado `aborted` requiere un preview nuevo.
La CLI local permite revisar la retención y eliminar un receipt terminal concreto:

```text
rust-engineering-mcp mutation list --state-root /ruta/privada/state --json
rust-engineering-mcp mutation prune --state-root /ruta/privada/state --operation-id mut_ID --plan-digest sha256:DIGEST --json
```

Usar el ID y digest exactos del listado. Prune elimina la evidencia durable y la
protección de replay de esa operación; consumir el receipt y descartar el plan
antes de hacerlo. No toca source ni ejecuta Cargo, funciona aunque el workspace ya
no exista, y rechaza registros pendientes o dudosos. No crea un store inexistente.
Un store lleno rechaza trabajo nuevo sin borrar evidencia y permite recovery por ID.

### Si recovery conserva bytes desconocidos

`recover: true` no es una reparación forzada. Si siguen apareciendo bytes o inodes
desconocidos, detener las instancias MCP que usan ese workspace, conservar juntos
el workspace original, sus temporales y el state root privado, y revisar los
cambios con las herramientas habituales del desarrollador. No editar el journal,
forzar prune, borrar `.rust-mcp-mut-*` ni ejecutar git clean en esa copia.

Para continuar trabajando sin destruir esa evidencia, preparar una nueva copia
física del proyecto en otra ruta, revisar y trasladar allí los cambios propios que
se quieran conservar y abrirla con un grant nuevo. No copiar los temporales
reservados al nuevo proyecto. El workspace original queda en cuarentena para
inspección; esto no declara recuperada ni deshecha su operación pendiente. Si el
journal está corrupto, la copia nueva puede usar un state root distinto; nunca
habilitar dos stores sobre el mismo workspace físico pendiente. Conservar ambos
hasta haber reconciliado manualmente los cambios. No requiere reinstalar el MCP.

El store tiene 256 MiB lógicos y conserva before/after completos: con source al
límite puede admitir aproximadamente cuatro journals grandes antes de rechazar
otra reserva. Usar list/prune sobre receipts terminales ya consumidos; los
registros pendientes se preservan. No hay retención ni eliminación automática.
