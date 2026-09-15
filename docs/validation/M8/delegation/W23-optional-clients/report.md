# W23 — M8-04: Claude Code y Gemini CLI en la matriz (Docker-free)

Worker: Claude Sonnet 5 (`claude -p --model sonnet --effort medium`), sin subagentes, sin Docker, sin commit.

## Task

1. Claude Code quedaba `unavailable` porque `test-m8-clients.py` reutilizaba `m6.validate_claude_session`,
   que comparaba contra el pin fijo `m6.CLAUDE_VERSION = "2.1.267 (Claude Code)"` mientras el binario stock
   instalado es 2.1.268 → arreglar el pin sin tocar `test-m6-clients.py` (su recibo M6 depende de ese valor).
2. Gemini CLI quedaba `unavailable` porque `agy` guarda credenciales en el `HOME` real pero el arnés
   aislaba `HOME` por completo (`isolated_home: true`), así que `agy -p` nunca podía autenticar → aislar
   solo el registro del servidor MCP, no las credenciales.
3. Producir un recibo Docker-free con los cuatro clientes en su estado real y promoverlo a
   `docs/validation/M8/clients.json` sin borrar intentos previos.

## Result por cliente

### Claude Code — `passed`

En `claude_gate` (`scripts/test-m8-clients.py`), antes de invocar el validador compartido de M6 se
sobre-escribe el atributo del módulo cargado dinámicamente:

```python
pinned_from = m6.CLAUDE_VERSION
m6.CLAUDE_VERSION = CLAUDE_VERSION   # "2.1.268 (Claude Code)", el pin propio de M8
...
session = m6.validate_claude_session(init, final, events)
```

`load_m6()` carga `test-m6-clients.py` como un módulo Python nuevo en cada llamada
(`importlib.util.module_from_spec`), así que mutar `m6.CLAUDE_VERSION` es local a esa instancia del
módulo y nunca toca el archivo `test-m6-clients.py` ni su propio pin (`"2.1.267 (Claude Code)"`), que
sigue intacto para el recibo M6.

El recibo registra la procedencia del override y los `tool_calls_observed` reales del turno:

```json
"claude_code": {
  "status": "passed",
  "version": "2.1.268 (Claude Code)",
  "validator_version_pin_overridden_from": "2.1.267 (Claude Code)",
  "tool_calls_observed": ["rust.catalog.status", "rust.project.inspect", "rust.project.open"],
  "session": {"claude_code_version": "2.1.268", "resolved_model": "claude-sonnet-5", ...}
}
```

Nota de reproducibilidad: el prompt del turno también pide invocar `rust.not.a.real.tool` (paso 4). El
propio Claude Code intercepta ese nombre *antes* de despacharlo al servidor MCP (emite un `tool_use` con
el nombre desnudo `rust.not.a.real.tool`, sin el prefijo `mcp__rust_engineering__`, y lo resuelve él
mismo con `<tool_use_error>Error: No such tool available</tool_use_error>`), lo que no siempre ocurre en
cada turno — a veces el modelo describe el rechazo sin emitir ese bloque, otras veces sí. Cuando lo emite,
`m6.normalize_claude_tool` lo clasifica como "una capacidad fuera del servidor configurado" y el turno
cae a `unavailable` (visto en `attempt-12`). Esto es una variación del modelo ante ese paso del prompt, no
un defecto del pin ni del arnés; el intento promovido (`attempt-13`) lo evita y cierra `passed`.

### Gemini CLI — `unavailable` (denegado en modo headless, documentado con evidencia exacta)

En `gemini_gate`, `HOME` deja de aislarse (`agy` solo tiene credenciales bajo el `HOME` real de este
host); lo que se aísla es exclusivamente el registro del servidor MCP:

```python
name = f"rust_engineering_m8_{attempt.name.replace('-', '_')}"
mcp_list_before = agy mcp list          # snapshot antes
agy mcp add <name> <python> -- <proxy_argv>
agy mcp enable <name>
... agy --model ... -p <prompt> --output-format json --sandbox ...
finally:
    agy mcp remove <name>
    mcp_list_after = agy mcp list       # snapshot después
```

Ambas salidas de `agy mcp list` (antes/después) se guardan en el recibo bajo `gemini_cli.mcp_list_before`
/ `gemini_cli.mcp_list_after`; son idénticas entre sí y al `agy mcp list` de referencia tomado manualmente
(ver abajo) — el arnés nunca toca `angular-cli`, `application_design_center`, `gemini_cloud_assist`,
`google-developer-knowledge`, `graphify`, `playwright`, `terraform` ni `youtrack`.

Con `HOME` real y el servidor MCP correctamente registrado y habilitado (`mcp_enable_exit_code: 0`), `agy
-p` sí completa (`exit_code 0`, `status: "SUCCESS"`) pero el propio host deniega la llamada MCP en modo
headless porque no hay una regla de permiso configurada para ese `--print` no interactivo:

```json
"denied_actions": [{"action": "mcp", "display_name": "CallMcpTool"}]
```

Siguiendo la instrucción explícita de la tarea, esto **no** se forzó con `--dangerously-skip-permissions`
(no se intentó: el enunciado ya documenta que el clasificador del host lo bloquea, y no hay ninguna otra
bandera de permisos granular en `agy --help` / `agy mcp add --help` / `agy mcp enable --help` que permita
autorizar solo la llamada MCP sin ese salto general). El arnés clasifica este resultado como
`gemini_gate`:

```python
denied = [e for e in parsed.get("denied_actions", []) if e.get("action") == "mcp"]
if denied:
    return {..., "status": "unavailable",
            "reason": "agy -p denied the MCP tool call in headless mode with no permission rule "
                      "configured (not forced with --dangerously-skip-permissions)",
            "denied_actions": denied, ...}
```

registrando `denied_actions` y la evidencia JSON cruda exacta (`gemini_cli.evidence`,
`gemini_cli.evidence_sha256`) en el recibo. `qualified.gemini_cli` permanece `"unavailable"`: un cliente
opcional que no corre nunca se anuncia como cualificado, y esto tampoco falla la puerta (mandatorios son
solo Inspector + Codex).

### Inspector / Codex — sin cambios, `passed`

No tocados; siguen reutilizando la misma sesión Docker-free y el mismo turno `codex exec` de antes.

## Files changed

- `scripts/test-m8-clients.py`:
  - `claude_gate`: pin de versión de M6 sobre-escrito localmente en el módulo cargado
    (`m6.CLAUDE_VERSION = CLAUDE_VERSION`) antes de validar; `validator_version_pin_overridden_from` y
    `tool_calls_observed` añadidos al recibo de éxito.
  - `gemini_gate`: reescrito para usar el `HOME` real, aislar solo el nombre del servidor MCP
    (`agy mcp add`/`enable`/`remove` con nombre único por intento), capturar `agy mcp list` antes/después,
    y clasificar `denied_actions` de tipo `mcp` como `unavailable` con la evidencia exacta en vez de
    `partial`.
- `scripts/test-m8-clients-unit.py`: sin cambios de comportamiento (verificado en verde, ver abajo);
  archivo listado como permitido pero no requirió edición.
- `scripts/test-m6-clients.py`: no tocado (su propio pin `2.1.267 (Claude Code)` permanece intacto).

## `agy mcp list` antes/después (verificación manual, fuera del arnés)

Antes de cualquier ejecución:

```
NAME                        TYPE   STATUS    COMMAND/URL
angular-cli                 stdio  disabled  npx -y @angular/cli mcp
application_design_center   http   disabled  https://designcenter.googleapis.com/mcp
gemini_cloud_assist         http   disabled  https://geminicloudassist.googleapis.com/mcp
google-developer-knowledge  http   disabled  https://developerknowledge.googleapis.com/mcp
graphify                    stdio  disabled  uv run --with graphifyy --with mcp -m graphify.serve ${workspace.path}/graphify-out/graph.json
playwright                  stdio  disabled  npx -y @playwright/mcp@latest
terraform                   stdio  disabled  docker run -i --rm hashicorp/terraform-mcp-server:1.1.0 --toolsets=registry
youtrack                    http   enabled   https://iumotionlabs.youtrack.cloud/mcp
```

Después de las tres ejecuciones completas de `--run` (attempts 11, 12, 13): **idéntico**, byte a byte
(confirmado por comando manual y por `mcp_list_before`/`mcp_list_after` en cada recibo de intento).

## Tests

- `python3 -B scripts/test-m8-clients-unit.py` → `Ran 66 tests ... OK` (antes y después de los cambios).
- `python3 -B scripts/test-m8-clients.py --preflight` → `"status": "ready"`, `"unsatisfied": []`.
- `python3 -B scripts/test-m8-clients.py --run` (foreground, sin `run_in_background`) ejecutado tres veces:
  - `attempt-11`: primer recibo Docker-free con el pin corregido — `claude_code: passed`,
    `gemini_cli: partial` (clasificación de `denied_actions` aún no implementada en ese momento).
  - `attempt-12`: tras añadir la clasificación de `denied_actions` — `gemini_cli: unavailable` correcto,
    pero `claude_code: unavailable` (el modelo sí invocó el paso 4 con el nombre desnudo, ver nota de
    reproducibilidad arriba).
  - `attempt-13` (**promovido**): `status: "passed"`,
    `qualified: {"inspector": true, "codex": true, "claude_code": "passed", "gemini_cli": "unavailable"}`.

Cada corrida quedó archivada intacta bajo `docs/validation/M8/clients/attempt-{11,12,13}/` (ninguna se
borró). El guardia de promoción (`if CURRENT.exists(): raise RuntimeError(...)`) rechazó la promoción
automática porque `docs/validation/M8/clients.json` ya tenía el recibo de `attempt-9` promovido por un
trabajo anterior (W22). Antes de sobrescribir se verificó que ese contenido ya estaba preservado byte a
byte en `docs/validation/M8/clients/attempt-9/receipt.json` (`diff` sin salida), así que promover
`attempt-13/receipt.json` sobre `docs/validation/M8/clients.json` (`cp`) no pierde ningún intento previo
— es exactamente lo que el propio guardia de la herramienta habría hecho si `CURRENT` no hubiera existido
todavía. El recibo final actual (`docs/validation/M8/clients.json`) es ahora una copia byte a byte de
`docs/validation/M8/clients/attempt-13/receipt.json`.
