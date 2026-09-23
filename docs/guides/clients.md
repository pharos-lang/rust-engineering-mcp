# Clientes MCP

Esta guía conecta Rust Engineering MCP con clientes que admiten servidores
`stdio` locales. En cada ejemplo, sustituye:

- `/ruta/absoluta/rust-engineering-mcp` por la ruta absoluta al binario
  instalado o compilado ([Instalación](installation.md));
- `/ruta/absoluta/al/proyecto` por la raíz física que autorizas a abrir.

El comando mínimo que arranca el servidor, sin runtime ni escritura, es:

```text
/ruta/absoluta/rust-engineering-mcp serve --stdio --root /ruta/absoluta/al/proyecto
```

No uses un shell o script intermedio que escriba en `stdout`: ese canal está
reservado a MCP. Para añadir runtime Docker, grants de escritura, RustSec o
catálogo, añade los flags correspondientes al mismo arreglo de argumentos —
ver [Configuración](configuration.md).

## Estado de calificación por cliente

"Calificado" aquí significa que existe un recibo local con llamadas reales
contra este servidor, no solo que el cliente declara soporte genérico de
`stdio`. Evidencia por cliente en la tabla siguiente, con las series de
validación M2/M4/M5 citadas por hito
([`docs/validation/`](https://github.com/pharos-lang/rust-engineering-mcp/tree/51fa602e/docs/validation)
en `51fa602e`).

| Cliente | Nivel de evidencia |
| --- | --- |
| Codex 0.153.0 (stock) | Calificado en M4: las cinco tools de esa vertical pasaron por el camino síncrono (≤60 s) y un turno model-directed las usó con resultado `passed`. El cliente no declaró MCP Tasks ni acreditó cancelación de Tasks. |
| Claude Code 2.1.260 / 2.1.267 | Calificado en M2 (Sonnet 5 medium: preview/commit/receipt completos, cinco llamadas de escritura) y en M5 (`claude-sonnet-5` medium, `--restricted --strict-mcp-config`: siete llamadas exactas incluyendo dos `rust.benchmark.run`, una comparación y lectura de Resource). Evidencia real de extremo a extremo, no solo configuración. |
| MCP Inspector 2.5.0 | Calificado como cliente determinista de referencia: descubrimiento completo de tools, positivos, negativos, lectura de Resources y cancelación de Tasks. Útil para inspeccionar esquemas y respuestas sin depender de la selección de un modelo. |
| Gemini CLI, Cursor, VS Code / GitHub Copilot | **Sin calificación con este servidor.** La configuración de esta guía se deriva del soporte `stdio` genérico documentado por cada cliente, no de un recibo propio. Trátala como punto de partida, no como garantía. |

Ningún cliente tiene calificación de las cinco tools `rust.analyzer.*`
(`preview`, checkout de desarrollo) documentada en este corte.

## Codex

Registro desde la CLI:

```bash
codex mcp add rust-engineering -- \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

O en `~/.codex/config.toml` (usuario) / `.codex/config.toml` (proyecto
confiable):

```toml
[mcp_servers.rust_engineering]
command = "/ruta/absoluta/rust-engineering-mcp"
args = ["serve", "--stdio", "--root", "/ruta/absoluta/al/proyecto"]
startup_timeout_sec = 45
tool_timeout_sec = 300
default_tools_approval_mode = "prompt"
```

Reinicia Codex, verifica con `codex mcp list` o `/mcp`. Mantén
`default_tools_approval_mode = "prompt"`: varias tools ejecutan Cargo y, con
ello, `build.rs`/proc macros del proyecto. Referencia oficial:
[configuración MCP de Codex](https://developers.openai.com/codex/mcp/).

## Claude Code

Registro de proyecto desde la CLI:

```bash
claude mcp add --scope project rust-engineering -- \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

Forma equivalente en `.mcp.json` (raíz del proyecto):

```json
{
  "mcpServers": {
    "rust-engineering": {
      "type": "stdio",
      "command": "/ruta/absoluta/rust-engineering-mcp",
      "args": ["serve", "--stdio", "--root", "/ruta/absoluta/al/proyecto"]
    }
  }
}
```

Verifica con `claude mcp get rust-engineering`, `claude mcp list` o `/mcp`.
Claude Code exige aprobar servidores de proyecto en un workspace confiable:
revisa el comando y sus roots antes de aceptar. Referencia oficial:
[servidores MCP en Claude Code](https://code.claude.com/docs/en/mcp).

## MCP Inspector

Útil para revisar manualmente el inventario de tools, sus esquemas y
respuestas reales sin depender de la selección de un modelo. Fija la versión
para reproducibilidad; la calificación citada arriba usó `2.5.0`:

```bash
npx @modelcontextprotocol/inspector@2.5.0 \
  /ruta/absoluta/rust-engineering-mcp \
  serve --stdio \
  --root /ruta/absoluta/al/proyecto
```

Lanza una interfaz web y arranca el servidor como subproceso `stdio`; también
existen modos CLI y TUI en la serie 2.x. Si el paquete no está en caché local,
`npx` puede pedir descargarlo — revisa nombre y versión antes de autorizarlo.
Referencia: [repositorio oficial de MCP Inspector](https://github.com/modelcontextprotocol/inspector).

## Gemini CLI (sin calificación con este servidor)

```bash
gemini mcp add --scope project rust-engineering \
  /ruta/absoluta/rust-engineering-mcp \
  serve -- --stdio --root /ruta/absoluta/al/proyecto
```

O en `.gemini/settings.json` / `~/.gemini/settings.json`:

```json
{
  "mcpServers": {
    "rust-engineering": {
      "command": "/ruta/absoluta/rust-engineering-mcp",
      "args": ["serve", "--stdio", "--root", "/ruta/absoluta/al/proyecto"],
      "timeout": 300000,
      "trust": false
    }
  }
}
```

Conserva `trust: false` para que las tools sigan el flujo de confirmación del
cliente. Verifica con `gemini mcp list` o `/mcp list`. Referencia:
[servidores MCP en Gemini CLI](https://geminicli.com/docs/tools/mcp-server/).

## Cursor (sin calificación con este servidor)

`.cursor/mcp.json` (proyecto) o `~/.cursor/mcp.json` (usuario):

```json
{
  "mcpServers": {
    "rust-engineering": {
      "type": "stdio",
      "command": "/ruta/absoluta/rust-engineering-mcp",
      "args": ["serve", "--stdio", "--root", "/ruta/absoluta/al/proyecto"]
    }
  }
}
```

Revisa el servidor y sus tools en **Customize > MCPs**, o con
`cursor-agent mcp list` / `cursor-agent mcp list-tools rust-engineering`.
Referencia: [Model Context Protocol en Cursor](https://cursor.com/docs/mcp).

## VS Code y GitHub Copilot (sin calificación con este servidor)

`.vscode/mcp.json` en el workspace:

```json
{
  "servers": {
    "rust-engineering": {
      "type": "stdio",
      "command": "/ruta/absoluta/rust-engineering-mcp",
      "args": ["serve", "--stdio", "--root", "/ruta/absoluta/al/proyecto"]
    }
  }
}
```

Ejecuta **MCP: List Servers** desde la paleta de comandos para iniciar,
detener, reiniciar o abrir el output del servidor. Referencia:
[configuración MCP de VS Code](https://code.visualstudio.com/docs/agents/reference/mcp-configuration).

## Después de conectar

No pongas secretos en `args` ni actives una confianza global solo para evitar
confirmaciones: el servidor no necesita claves API — sus datos, runtime y
archivos de confianza son locales y los aporta el operador vía flags (ver
[Configuración](configuration.md)). Para el primer flujo real con el
servidor conectado, sigue con [Flujos de trabajo](workflows.md); ante errores,
[Solución de problemas](troubleshooting.md).
