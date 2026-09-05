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
