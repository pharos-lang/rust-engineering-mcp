# W28b — informe

## 1. Codex: negativo obligatorio movido a `project_ref` inválido

`attempt-14/receipt.json` mostraba `unknown_tool_refused_on_wire: false` por
diseño: un modelo que descubre el inventario real de 36 tools nunca invoca
`rust.not.a.real.tool` tras el discovery, así que ese negativo simplemente no
aparece en el wire. Cambios en `scripts/test-m8-clients.py`:

- **Prompt del turno de Codex** (`codex_gate`): el paso obligatorio ahora es
  una segunda llamada a `rust.project.inspect` con
  `project_ref = "prj_00000000000000000000000000000000"` (bien formado, nunca
  abierto en esta sesión). La tool inexistente (`rust.not.a.real.tool`) queda
  como paso opcional/informativo — el prompt dice explícitamente "optionally,
  if you want to".
- **`codex_protocol_evidence`**: añade `unknown_project_ref_wire_refused`,
  calculado por posición sobre `protocol.jsonl` — para cada `tools/call`
  cliente seguido de una fila `server`, si la tool es `rust.project.inspect`
  y la fila del servidor lleva `structuredContent.status` en
  `{"blocked","unavailable"}` **y** `structuredContent.error_code ==
  "PROJECT_NOT_FOUND"`, el rechazo se confirma por wire. `unknown_tool_wire_refused`
  se conserva sin cambios de comportamiento, ahora puramente informativo.
- **`codex_classification`**: `passed` exige `open` + `inspect` válido
  observados **y** `unknown_project_ref_wire_refused`; el paso opcional de
  tool inexistente ya no gatea la clasificación.

### El proxy no registraba `error_code` — nuevo `wire_proxy`

El proxy compartido (`test-m3-clients.py::proxy`, fuera del alcance de
archivos permitidos) sólo registraba metadatos acotados (`method`, `tool`,
`tasks_declared`/`_advertised`) — nunca el cuerpo de la respuesta, así que el
rechazo `PROJECT_NOT_FOUND` no era confirmable por wire, sólo por el
transcript del cliente. Como no se puede tocar `test-m3-clients.py`, se
añadió en `test-m8-clients.py` un `wire_proxy` propio (mismo comportamiento
que el de M3, reutilizando `m3.digest`/`m3.append_observation`/
`m3.tasks_declared`) que además captura, sólo para la fila `server` que seguía
inmediatamente a un `tools/call` cliente, dos campos seguros y ya públicos en
el contrato de cada tool: `structuredContent.status` y
`structuredContent.error_code`. `main()` despacha ahora el subcomando
`proxy` a este `wire_proxy` en lugar de `m3.proxy`, para los cuatro clientes
por igual (Inspector, Codex, Claude Code, Gemini CLI comparten el mismo
subcomando), así que la ampliación de `safe_keys` en
`validate_protocol_metadata` (con los mismos dos nombres) cubre a los cuatro
sin cambios adicionales por cliente.

## 2. Gemini: clasificación honesta (ya correcta, verificada con evidencia)

`attempt-14/gemini-events.json` sólo lleva `denied_actions: [{"action":
"read_file", "display_name": "ViewFile"}]` — una denegación no relacionada
con MCP (una lectura de archivo, no una llamada a tool). `gemini_gate` sólo
clasifica `unavailable` cuando la denegación tiene `action == "mcp"`; aquí no
la hay, así que cae correctamente en `partial` (0 `tool_calls_observed`, sin
llamadas MCP observadas). Verificado leyendo el archivo y la lógica existente
en `scripts/test-m8-clients.py::gemini_gate` — no hizo falta ningún cambio de
código para este caso: la clasificación ya era honesta, sólo le faltaba la
verificación con evidencia que pedía el encargo.

En el `--run` de esta sesión (`attempt-15`), Gemini CLI sí produjo una
denegación `action: "mcp"` (`display_name: "CallMcpTool"`) — el caso que
`gemini_gate` clasifica `unavailable` con el texto exacto de la razón. Ningún
caso observado produjo `passed`.

## 3. `--run` (Docker-free, foreground)

`python3 -B scripts/test-m8-clients.py --run` → `attempt-15`, `status:
"passed"`.

| Cliente | Resultado |
| --- | --- |
| Inspector (docker_free) | `contract_equality: true`; 4 negativos genéricos + 30 negativos estructurados confirmados por wire |
| Codex (mandatorio) | `classification: "passed"`; `protocol_evidence.unknown_project_ref_refused_on_wire: true`, `project_open_observed: true`, `project_inspect_observed: true` |
| Claude Code (opcional) | `status: "passed"` |
| Gemini CLI (opcional) | `status: "unavailable"`, `denied_actions: [{"action": "mcp", ...}]` — no anunciado como cualificado, no bloquea la puerta |

El recibo se promovió a `docs/validation/M8/clients.json` (sustituyendo el de
`attempt-13`, que usaba el oráculo antiguo de Codex).

## 4. Unidad

`python3 -B scripts/test-m8-clients-unit.py` → 98/98 verdes (añadidas 5
pruebas: `unknown_project_ref_wire_refused` en sus tres formas —
confirmado/rechazado por `passed`/sin fila `server` — y las dos firmas
actualizadas de `codex_classification`/`codex_protocol_evidence`).

## Archivos tocados

- `scripts/test-m8-clients.py`: `wire_proxy` (nuevo), `MAX_OUTPUT`,
  `validate_protocol_metadata` (safe_keys), `codex_protocol_evidence`,
  `codex_classification`, `codex_gate` (prompt + protocol_evidence), `main()`
  (dispatcha a `wire_proxy`), `preflight()` (`m3_reuse` corregido).
- `scripts/test-m8-clients-unit.py`: pruebas actualizadas/añadidas para las
  firmas y el nuevo campo.
- `docs/validation/M8/clients.json`: promovido desde `attempt-15`.
- `docs/validation/M8/clients/attempt-15/`: evidencia nueva (receipt +
  protocol.jsonl + eventos).

No commit, sin Docker, sin subagentes, ninguna ejecución en segundo plano.
